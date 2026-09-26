//! **What every start does about an update before anything else** — the
//! startup row of `docs/plans/design/self-update-2026-09-16.md` R.3 and the
//! ordinary start's rule of revision (b), §(b).2 (0.4.6 ticket U-12).
//!
//! [`pass`] is called once, in `fn main`, after the argv doors and the parse
//! and before the data directory is resolved, settings are read, a sidecar is
//! loaded or the launch is handed to a running Folio
//! (`launch_wire::hand_over`, which takes the [`Admitted`] this returns). In
//! order:
//!
//! 1. **Admission.** The installation home's `admission` file is held
//!    **shared** for the life of the process (a static; the operating system
//!    releases it at exit), so the mover, which needs it exclusive, cannot move
//!    files under a running copy (F-6). A copy started before the home existed
//!    holds nothing; the mover finds such copies by listing processes.
//! 2. **One look at the journal.** One attempt to read `H\journal.json`; no
//!    journal is the whole of an ordinary start's cost.
//! 3. **The frozen header only** (`update_txn::Header::parse`), and the
//!    ordinary start's rule (`update_txn::at_start`):
//!    - `terminal` → the entrance (on Windows its `Run` value, removed and
//!      flushed through `bt_platform::logon_hook`, U-22), then `H\<txn>`, then
//!      the journal, each removed durably through `bt_platform::install_txn`,
//!      all under the transaction lock; continue;
//!    - `destructive`, or the trial of another transaction, or no trial at all
//!      → the rescue build is started detached with
//!      `--update-recover --then-launch <this command line>`, and this process
//!      leaves with one line — unless the rescue build is missing or cannot be
//!      started, when one line names it and the start continues untouched
//!      (coordinator ruling, 2026-09-27);
//!    - `preparing` / `deferred` → continue, unless this start's own image is
//!      not the rescue copy of the build that began the transaction (the
//!      folder was replaced by hand): then `H\<txn>` and the journal are
//!      removed under the lock, nothing in the install, and the start
//!      continues.
//!
//! A journal this build cannot read, a lock somebody else holds, and a file
//! that cannot be measured all leave everything as it is and continue.
//! **Nothing durable is written by an ordinary start except a retirement.**
//!
//! # Where it runs, and its doors
//!
//! On the window thread in the phase `Starting` (`bt_platform::admission`):
//! there is no loop yet to be blocked, which is §5.3 row 18's reasoning for the
//! hand-over beside it. It never waits: both locks are one non-blocking
//! attempt (`install_txn::try_hold`). Reads go through `file_reads` on
//! `Lane::Install`; locks and removals through `install_txn`; the rescue build
//! is started through `bt_platform::quiet_command` and never waited on — the
//! `--from-explorer` child's shape (ARCHITECTURE §2.2).

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Held, Hold};

use crate::cli;
use crate::update_txn::{
    Class, Digest, Effect, Header, Home, JournalRead, Nonce, StartAction, StartView, TxnId,
    at_start,
};

/// **The pass has run.** Only [`pass`] makes one, and `launch_wire::hand_over`
/// asks for it, so no start can hand itself to another Folio before its
/// admission is taken.
pub(crate) struct Admitted(());

/// This process's shared hold on `H\admission`, kept until the process ends.
static ADMISSION: OnceLock<Held> = OnceLock::new();

/// **This start's trial**: the transaction and nonce of `--update-trial`, set
/// only when the journal confirms this start is that transaction's trial, and
/// the installation home whose journal said so.
static TRIAL: OnceLock<Trial> = OnceLock::new();

/// What [`TRIAL`] holds.
struct Trial {
    txn: TxnId,
    nonce: Nonce,
    home: Home,
}

/// **Whether this start is an update's trial, and its nonce** — the fact the
/// trial's write gate and its receipt read (`update_trial`, U-13).
pub(crate) fn trial() -> Option<(TxnId, Nonce)> {
    TRIAL.get().map(|trial| (trial.txn, trial.nonce))
}

/// **The installation home of this start's trial**: where its journal is read
/// and its receipt written (`update_trial`, U-13). `None` exactly when
/// [`trial`] is.
pub(crate) fn trial_home() -> Option<&'static Home> {
    TRIAL.get().map(|trial| &trial.home)
}

/// **Make this test process an update's trial** — what [`pass`] records when
/// the journal confirms `--update-trial`, for a test that runs a start's
/// writers in a process of its own (`update_trial`'s tests). Once per process,
/// as the real one is.
#[cfg(test)]
pub(crate) fn become_trial(txn: TxnId, nonce: Nonce, home: Home) -> bool {
    TRIAL.set(Trial { txn, nonce, home }).is_ok()
}

/// **The start's effects that are not files in the home**: its one line, the
/// rescue build's start, and the entrance.
pub(crate) trait World {
    /// One line for the person who started this process (the front door's
    /// console; nowhere when there is none).
    fn say(&mut self, line: &str);
    /// Start `program` with `args`, detached: never waited on.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()>;
    /// Remove the transaction's logon entrance, if one is still there; a
    /// failure is said and stops the retirement.
    fn retire_entrance(&mut self, txn: TxnId) -> Result<(), String>;
}

/// What one start is: its own executable, its installation home, its command
/// line and its `--update-trial` values.
pub(crate) struct Start<'a> {
    pub(crate) own_exe: &'a Path,
    pub(crate) home: &'a Home,
    pub(crate) argv: &'a [OsString],
    pub(crate) trial: Option<&'a cli::UpdateTrialArg>,
}

/// What the pass decided.
pub(crate) enum Verdict {
    /// Start as usual, holding `admission` for the life of the process.
    Continue {
        admission: Option<Held>,
        trial: Option<(TxnId, Nonce)>,
    },
    /// Leave now, with this exit code: the rescue build has this start.
    Exit(i32),
}

/// **The pass, for this process** — see the module header.
pub(crate) fn pass(request: &cli::CliRequest) -> Admitted {
    // A start that cannot name its own executable has no installation home to
    // find, and is a start as every Folio before 0.4.6 was.
    let Ok(own_exe) = std::env::current_exe() else {
        return Admitted(());
    };
    let Some(home) = Home::of(bt_platform::host_platform(), &own_exe) else {
        return Admitted(());
    };
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let start = Start {
        own_exe: &own_exe,
        home: &home,
        argv: &argv,
        trial: request.update_trial.as_ref(),
    };
    match run(&start, &mut Machine) {
        Verdict::Exit(code) => bt_platform::leave_process(code),
        Verdict::Continue { admission, trial } => {
            if let Some(held) = admission {
                let _ = ADMISSION.set(held);
            }
            if let Some((txn, nonce)) = trial {
                let _ = TRIAL.set(Trial { txn, nonce, home });
            }
            Admitted(())
        }
    }
}

/// **The pass over any home**: the steps of the module header, in order.
pub(crate) fn run(start: &Start<'_>, world: &mut impl World) -> Verdict {
    let admission = admit(start.home, world);
    let journal_path = start.home.journal();
    let journal = match file_reads::read(Lane::Install, &journal_path) {
        Ok(bytes) => match Header::parse(&bytes) {
            Ok(header) => JournalRead::Read(header),
            Err(refusal) => {
                world.say(&format!(
                    "BT_UPDATE_START {} is left as it is: {refusal}",
                    journal_path.display()
                ));
                JournalRead::Unreadable(refusal)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => JournalRead::Absent,
        Err(error) => {
            world.say(&format!(
                "BT_UPDATE_START {} could not be read: {error}",
                journal_path.display()
            ));
            return Verdict::Continue {
                admission,
                trial: None,
            };
        }
    };
    let JournalRead::Read(header) = &journal else {
        return Verdict::Continue {
            admission,
            trial: None,
        };
    };
    let header = header.clone();
    let trial = trial_of(start.trial, world);
    // The transaction lock is asked for only where its answer decides
    // something: a retirement or a discard needs it; a destructive class is
    // never the start's to touch, and asking would contend with the applier.
    let lock = match header.class {
        Class::Terminal | Class::Preparing | Class::Deferred => take_lock(start.home, world),
        Class::Destructive => None,
    };
    let measured = lock.is_some() && matches!(header.class, Class::Preparing | Class::Deferred);
    let view = StartView {
        journal,
        lock_free: lock.is_some(),
        own_image: measured.then(|| image(start.own_exe)).flatten(),
        rescue_image: measured
            .then(|| image(&start.home.rescue_program(&header.rescue)))
            .flatten(),
        trial_of: trial.map(|(txn, _)| txn),
    };
    match at_start(&view) {
        StartAction::Continue => Verdict::Continue {
            admission,
            trial: None,
        },
        StartAction::RunAsTrial => Verdict::Continue { admission, trial },
        action @ (StartAction::Retire | StartAction::Discard) => {
            retire(action, &header, start.home, world);
            drop(lock);
            Verdict::Continue {
                admission,
                trial: None,
            }
        }
        StartAction::HandToRescue => hand_to_rescue(start, &header, admission, world),
    }
}

/// Step 1: `H\admission`, shared, one attempt. `None` when there is no
/// admission file (a copy started before any transaction), when the mover
/// holds it (the journal then says why), or when it cannot be opened.
fn admit(home: &Home, world: &mut impl World) -> Option<Held> {
    match install_txn::try_hold(&home.admission(), Hold::Shared) {
        Ok(held) => held,
        Err(failure) if failure.error.kind() == io::ErrorKind::NotFound => None,
        Err(failure) => {
            world.say(&format!("BT_UPDATE_START {failure}"));
            None
        }
    }
}

/// The transaction lock, one attempt; `None` when somebody else holds it.
fn take_lock(home: &Home, world: &mut impl World) -> Option<Held> {
    match install_txn::try_hold(&home.lock(), Hold::Exclusive) {
        Ok(held) => held,
        Err(failure) => {
            world.say(&format!("BT_UPDATE_START {failure}"));
            None
        }
    }
}

/// `--update-trial`'s two values, when both are well formed.
fn trial_of(arg: Option<&cli::UpdateTrialArg>, world: &mut impl World) -> Option<(TxnId, Nonce)> {
    let arg = arg?;
    match (TxnId::parse(&arg.txn), Nonce::parse(&arg.nonce)) {
        (Ok(txn), Ok(nonce)) => Some((txn, nonce)),
        (Err(refusal), _) | (_, Err(refusal)) => {
            world.say(&format!(
                "BT_UPDATE_START {} names no trial: {refusal}",
                cli::UPDATE_TRIAL_FLAG
            ));
            None
        }
    }
}

/// The SHA-256 of the executable at `program`, or `None` when it cannot be
/// read.
fn image(program: &Path) -> Option<Digest> {
    let bytes = file_reads::read(Lane::Install, program).ok()?;
    Some(Digest::new(bt_winres::digest::sha256(&bytes)))
}

/// A retirement or a discard: the action's effects, in the protocol's order,
/// stopping at the first that fails — the journal is removed only once
/// `H\<txn>` is gone, so a start that is stopped halfway leaves a journal that
/// still names what is left (U-10, "journal deleted last").
fn retire(action: StartAction, header: &Header, home: &Home, world: &mut impl World) {
    for effect in action.effects() {
        let done = match effect {
            Effect::RemoveEntrance => world.retire_entrance(header.txn),
            Effect::DeleteTxnDir => install_txn::durable_remove(&home.transaction(header.txn))
                .map_err(|failure| failure.to_string()),
            Effect::DeleteJournal => {
                install_txn::durable_remove(&home.journal()).map_err(|failure| failure.to_string())
            }
            other => unreachable!("a start's action has no {other:?}"),
        };
        if let Err(failure) = done {
            world.say(&format!(
                "BT_UPDATE_START transaction {} is kept for the next start: {failure}",
                header.txn
            ));
            return;
        }
    }
}

/// The rescue build takes this start: it is started with this start's own
/// command line after `--then-launch`, and this process leaves.
///
/// **A rescue build that is missing or cannot be started never stops the
/// start** (coordinator ruling, 2026-09-27): one line names the program and
/// the transaction, and the start continues as a waiting transaction's does —
/// no journal write, no deletion. The state is left for the recovery's
/// `Stuck` handling (U-22, U-24) to read; an app that never opens again is not
/// an answer.
fn hand_to_rescue(
    start: &Start<'_>,
    header: &Header,
    admission: Option<Held>,
    world: &mut impl World,
) -> Verdict {
    let program = start.home.rescue_program(&header.rescue);
    match world.spawn_detached(&program, &cli::recover_command_line(start.argv)) {
        Ok(()) => {
            world.say(&format!(
                "BT_UPDATE_START an update is being finished by {}; Folio opens when it is done",
                program.display()
            ));
            Verdict::Exit(0)
        }
        Err(error) => {
            world.say(&format!(
                "BT_UPDATE_START transaction {} is unfinished and its rescue build {} could not be started ({error}); Folio starts without it",
                header.txn,
                program.display()
            ));
            Verdict::Continue {
                admission,
                trial: None,
            }
        }
    }
}

/// This process's own world.
struct Machine;

impl World for Machine {
    fn say(&mut self, line: &str) {
        bt_platform::write_std_error(format!("{line}\n").as_bytes());
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        // `quiet_command` is the one door for a child, and the child is dropped
        // at once: nothing here waits on it or ends it.
        bt_platform::quiet_command(program)
            .args(args)
            .spawn()
            .map(drop)
    }

    fn retire_entrance(&mut self, txn: TxnId) -> Result<(), String> {
        // Windows: the `Run` value `FolioUpdate-<txn8>`, removed and flushed by
        // its door (U-22); a value already gone is success. The macOS
        // LaunchAgent and its door arrive with U-26; until then no macOS
        // transaction has an entrance, and there is nothing to remove.
        if bt_platform::host_platform() != bt_platform::HostPlatform::Windows {
            return Ok(());
        }
        bt_platform::logon_hook::disarm(txn.bytes()).map_err(|refusal| refusal.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! One test reads `fn main`'s order from the source; every other one
    //! builds its own installation in a temporary folder — an executable, the
    //! home, the journal, the transaction's folder — and runs the pass over it
    //! with the real locks, reads and removals. Only the three effects of
    //! [`super::World`] are recorded instead of performed.

    /// RED (U-12) — **the admission is taken in `fn main` after the parse and
    /// before the data directory, the settings, the endpoints and the
    /// hand-over.**
    ///
    /// F-6: "taken in `fn main` before settings, sidecars or
    /// `launch_wire::hand_over`". The hand-over half is also a type:
    /// `launch_wire::hand_over` takes the [`super::Admitted`] only [`super::pass`] makes.
    /// What the type cannot say — that `persist::storage_dir` (which may move
    /// the data directory) and `diagnostics::enter_resident_run` come after —
    /// is read from `main`'s body.
    ///
    /// MUTATION: move the `update_startup::pass(&request)` line below
    /// `let storage = persist::storage_dir();`.
    #[test]
    fn admission_is_taken_before_hand_over() {
        use bt_source::{Index, ItemQuery};
        let main = Index::of_package("bt-app")
            .body_of(&ItemQuery::function("main"))
            .unwrap_or_else(|failure| panic!("{failure}"));
        let at = |needle: &str| {
            main.find(needle)
                .unwrap_or_else(|| panic!("`main` no longer does `{needle}`"))
        };
        let parsed = at("cli::parse(");
        let window_thread = at("bt_platform::admission::enter_window_thread()");
        let admitted = at("update_startup::pass(&request)");
        let storage = at("persist::storage_dir()");
        let claim = at("persist::is_writer_of(");
        let handed_over = at("launch_wire::hand_over(");
        let resident = at("diagnostics::enter_resident_run(");
        let event_loop = at("EventLoop::<AppEvent>::with_user_event()");
        assert!(parsed < admitted, "after the argv doors and the parse");
        assert!(
            window_thread < admitted,
            "on the window thread, in the phase `Starting`"
        );
        for (later, what) in [
            (storage, "the data directory"),
            (claim, "the data directory's claim"),
            (handed_over, "the hand-over"),
            (resident, "the resident run"),
            (event_loop, "the event loop"),
        ] {
            assert!(admitted < later, "the admission comes before {what}");
        }
    }

    /// The pass over real folders: the locks and removals are
    /// `install_txn`'s Windows and macOS arms. Every other platform has no
    /// installation home (`Home::of`), so the pass never runs there, and
    /// `install_txn` refuses these effects by name
    /// (`install_txn::tests::the_portable_arm_refuses_by_name`); these tests
    /// answer at once on such a platform.
    mod on_disk {
        use super::super::*;
        use std::path::PathBuf;

        /// What the pass asked of its world.
        #[derive(Default)]
        struct Recorded {
            said: Vec<String>,
            spawned: Vec<(PathBuf, Vec<OsString>)>,
            entrances: Vec<TxnId>,
            /// Whether `H\admission` could be taken exclusive at the moment of the
            /// spawn (it must not: this start holds it shared).
            exclusive_free_at_spawn: Vec<bool>,
            admission: Option<PathBuf>,
        }

        impl World for Recorded {
            fn say(&mut self, line: &str) {
                self.said.push(line.to_owned());
            }

            fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
                if let Some(admission) = &self.admission {
                    self.exclusive_free_at_spawn.push(
                        install_txn::try_hold(admission, Hold::Exclusive)
                            .unwrap()
                            .is_some(),
                    );
                }
                self.spawned.push((program.to_path_buf(), args.to_vec()));
                // As the operating system answers: a program that is not there
                // cannot be started.
                if program.is_file() {
                    Ok(())
                } else {
                    Err(io::Error::from(io::ErrorKind::NotFound))
                }
            }

            fn retire_entrance(&mut self, txn: TxnId) -> Result<(), String> {
                self.entrances.push(txn);
                Ok(())
            }
        }

        fn txn() -> TxnId {
            TxnId::new([0x7a; 16])
        }

        fn nonce() -> Nonce {
            Nonce::new([0x5c; 32])
        }

        /// **One installation in a temporary folder**: `install\folio.exe`, a
        /// sidecar beside it, the home with its admission file, and the
        /// transaction's folder with the rescue copy of the running build.
        struct Scene {
            root: PathBuf,
            install: PathBuf,
            own_exe: PathBuf,
            home: Home,
            rescue: PathBuf,
        }

        const RUNNING_BUILD: &[u8] = b"the running folio.exe";

        impl Scene {
            /// `None` where the pass has no home to run over (see the module).
            fn new(tag: &str) -> Option<Self> {
                if bt_platform::host_platform() == bt_platform::HostPlatform::OtherUnix {
                    return None;
                }
                let root = std::env::temp_dir()
                    .join(format!("bt-update-startup-{tag}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&root);
                let install = root.join("Folio");
                let home_root = install.join(crate::update_txn::WINDOWS_HOME);
                let rescue = home_root
                    .join(txn().to_string())
                    .join("rescue")
                    .join("folio.exe");
                std::fs::create_dir_all(rescue.parent().unwrap()).unwrap();
                std::fs::write(install.join("folio.exe"), RUNNING_BUILD).unwrap();
                std::fs::write(install.join("conpty.dll"), b"sidecar").unwrap();
                std::fs::write(home_root.join("admission"), b"").unwrap();
                std::fs::write(&rescue, RUNNING_BUILD).unwrap();
                Some(Self {
                    own_exe: install.join("folio.exe"),
                    home: Home::at(home_root),
                    install,
                    root,
                    rescue,
                })
            }

            fn journal(&self, class: Class) -> Vec<u8> {
                let bytes = Header {
                    txn: txn(),
                    rescue: self.rescue.to_string_lossy().into_owned(),
                    class,
                    outcome: crate::update_txn::HeaderOutcome::None,
                }
                .encode();
                std::fs::write(self.home.journal(), &bytes).unwrap();
                bytes
            }

            fn run(
                &self,
                argv: &[&str],
                trial: Option<&cli::UpdateTrialArg>,
                world: &mut Recorded,
            ) -> Verdict {
                let argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
                world.admission = Some(self.home.admission());
                run(
                    &Start {
                        own_exe: &self.own_exe,
                        home: &self.home,
                        argv: &argv,
                        trial,
                    },
                    world,
                )
            }

            /// Every path under the installation, sorted, relative to it.
            fn listing(&self) -> Vec<String> {
                fn walk(base: &Path, at: &Path, out: &mut Vec<String>) {
                    for entry in std::fs::read_dir(at).unwrap() {
                        let path = entry.unwrap().path();
                        out.push(
                            path.strip_prefix(base)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/"),
                        );
                        if path.is_dir() {
                            walk(base, &path, out);
                        }
                    }
                }
                let mut out = Vec::new();
                walk(&self.install, &self.install, &mut out);
                out.sort();
                out
            }
        }

        impl Drop for Scene {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        fn trial_arg(txn: TxnId, nonce: Nonce) -> cli::UpdateTrialArg {
            cli::UpdateTrialArg {
                txn: txn.to_string(),
                nonce: nonce.to_string(),
            }
        }

        fn continued(verdict: Verdict) -> (Option<Held>, Option<(TxnId, Nonce)>) {
            match verdict {
                Verdict::Continue { admission, trial } => (admission, trial),
                Verdict::Exit(code) => panic!("the start left with {code} instead of continuing"),
            }
        }

        fn exited(verdict: Verdict) -> i32 {
            match verdict {
                Verdict::Exit(code) => code,
                Verdict::Continue { .. } => panic!("the start continued instead of leaving"),
            }
        }

        /// RED (U-12) — **a start that finds a destructive journal starts the
        /// rescue build with `--update-recover --then-launch` and its own command
        /// line, and leaves; it touches nothing and holds its admission while it
        /// hands over.**
        ///
        /// (b).2: "`destructive`, or `Trial` without its nonce → start the rescue
        /// helper with `--then-launch <argv>`, and exit", and owner ruling 3 (a
        /// start after Restart waits). The install may be mid-change here, so the
        /// start must not read settings, claim the data directory or open a
        /// window: [`Verdict::Exit`] is `fn main` leaving before any of that
        /// (`admission_is_taken_before_hand_over` pins where).
        ///
        /// MUTATION: in `hand_to_rescue`, answer `Verdict::Continue { admission:
        /// None, trial: None }` after a successful spawn.
        #[test]
        fn a_start_during_a_destructive_state_hands_to_the_rescue_and_exits() {
            let Some(scene) = Scene::new("destructive") else {
                return;
            };
            let journal = scene.journal(Class::Destructive);
            let before = scene.listing();
            let mut world = Recorded::default();
            let code = exited(scene.run(&["--cwd", "D:\\x", "--tab"], None, &mut world));
            assert_eq!(code, 0);
            assert_eq!(
                world.spawned,
                vec![(
                    scene.rescue.clone(),
                    cli::recover_command_line(&["--cwd", "D:\\x", "--tab"].map(OsString::from))
                )]
            );
            assert_eq!(
                world.exclusive_free_at_spawn,
                vec![false],
                "the start holds its admission shared while it hands over"
            );
            assert_eq!(world.said.len(), 1, "{:?}", world.said);
            assert_eq!(std::fs::read(scene.home.journal()).unwrap(), journal);
            assert_eq!(scene.listing(), before, "nothing is created or removed");
            assert!(world.entrances.is_empty());
        }

        /// RED (U-12, coordinator ruling 2026-09-27) — **a start that must hand
        /// itself to a rescue build that is not there continues, with one line
        /// naming the missing program and the transaction, and touches
        /// nothing.**
        ///
        /// Exiting would leave a Folio that never opens again for as long as
        /// the journal says `destructive`. The journal and the transaction's
        /// folder stay exactly as they were, for the recovery's `Stuck`
        /// handling (U-22, U-24) to read.
        ///
        /// MUTATION: in `hand_to_rescue`, answer `Verdict::Exit(1)` when the
        /// spawn fails.
        #[test]
        fn a_missing_rescue_does_not_brick_the_start() {
            let Some(scene) = Scene::new("missing-rescue") else {
                return;
            };
            let journal = scene.journal(Class::Destructive);
            std::fs::remove_file(&scene.rescue).unwrap();
            let before = scene.listing();
            let mut world = Recorded::default();
            let (admission, trial) = continued(scene.run(&["--tab"], None, &mut world));
            assert!(admission.is_some(), "the start keeps its admission");
            assert_eq!(trial, None);
            assert_eq!(world.said.len(), 1, "{:?}", world.said);
            let line = &world.said[0];
            assert!(
                line.contains(&scene.rescue.display().to_string())
                    && line.contains(&txn().to_string()),
                "the line names the missing rescue build and the transaction: {line}"
            );
            assert_eq!(std::fs::read(scene.home.journal()).unwrap(), journal);
            assert_eq!(scene.listing(), before, "nothing is written or removed");
            assert!(world.entrances.is_empty());
        }

        /// RED (U-12) — **a terminal journal is retired at start: the entrance
        /// hook, then `H\<txn>`, then the journal; the install, the admission file
        /// and the lock stay, and the start continues.**
        ///
        /// (b).2: "`terminal` → delete `H\<txn>` and the journal (and the entrance
        /// if one is still there), then continue", under the transaction lock
        /// (U-10 decision 13), with the journal last (decision 14). This is the
        /// one durable thing an ordinary start ever does.
        ///
        /// MUTATION: in `retire`, answer `Ok(())` for `Effect::DeleteJournal`
        /// without removing it.
        #[test]
        fn a_terminal_journal_is_retired_at_start() {
            let Some(scene) = Scene::new("terminal") else {
                return;
            };
            scene.journal(Class::Terminal);
            std::fs::create_dir_all(scene.home.transaction(txn()).join("backup")).unwrap();
            std::fs::write(
                scene
                    .home
                    .transaction(txn())
                    .join("backup")
                    .join("folio.exe"),
                b"older",
            )
            .unwrap();
            let mut world = Recorded::default();
            let (admission, trial) = continued(scene.run(&[], None, &mut world));
            assert!(admission.is_some());
            assert_eq!(trial, None);
            assert_eq!(world.entrances, vec![txn()]);
            assert!(world.spawned.is_empty());
            assert!(world.said.is_empty(), "{:?}", world.said);
            assert_eq!(
                scene.listing(),
                vec![
                    ".folio-update",
                    ".folio-update/admission",
                    ".folio-update/lock",
                    "conpty.dll",
                    "folio.exe",
                ]
            );
            assert!(
                install_txn::try_hold(&scene.home.lock(), Hold::Exclusive)
                    .unwrap()
                    .is_some(),
                "the transaction lock is released after the retirement"
            );
        }

        /// RED (U-12) — **a terminal journal whose lock somebody else holds is left
        /// for a later start, whole.**
        ///
        /// U-10 decision 13: a start deletes only with the transaction lock free.
        ///
        /// MUTATION: in `run`, build the view with `lock_free: true` whatever
        /// `take_lock` answered.
        #[test]
        fn a_retirement_waits_for_a_free_transaction_lock() {
            let Some(scene) = Scene::new("locked") else {
                return;
            };
            let journal = scene.journal(Class::Terminal);
            let holder = install_txn::try_hold(&scene.home.lock(), Hold::Exclusive)
                .unwrap()
                .unwrap();
            let before = scene.listing();
            let mut world = Recorded::default();
            continued(scene.run(&[], None, &mut world));
            assert_eq!(scene.listing(), before);
            assert_eq!(std::fs::read(scene.home.journal()).unwrap(), journal);
            assert!(world.entrances.is_empty());
            drop(holder);
        }

        /// RED (U-12) — **a start holds `H\admission` shared for as long as its
        /// hold lives, so the mover cannot take it exclusive; a start with no
        /// admission file holds nothing and creates nothing.**
        ///
        /// F-6: every Folio started from this install holds the admission shared
        /// for its lifetime, taken before settings, sidecars or the hand-over; a
        /// process started before the home existed holds no lock (the mover lists
        /// processes for those).
        ///
        /// MUTATION: in `run`, drop the admission (`let admission = None;` after
        /// `admit`).
        #[test]
        fn a_start_holds_its_admission_shared_while_it_runs() {
            let Some(scene) = Scene::new("admission") else {
                return;
            };
            let before = scene.listing();
            let mut world = Recorded::default();
            let (admission, _) = continued(scene.run(&[], None, &mut world));
            assert!(admission.is_some());
            assert!(
                install_txn::try_hold(&scene.home.admission(), Hold::Exclusive)
                    .unwrap()
                    .is_none(),
                "the mover cannot take the admission while the start holds it"
            );
            let second = install_txn::try_hold(&scene.home.admission(), Hold::Shared).unwrap();
            assert!(second.is_some(), "every other start may share it");
            drop((admission, second));
            assert!(
                install_txn::try_hold(&scene.home.admission(), Hold::Exclusive)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                scene.listing(),
                before,
                "no journal: nothing else is touched"
            );
            assert!(world.said.is_empty(), "{:?}", world.said);

            std::fs::remove_file(scene.home.admission()).unwrap();
            let before = scene.listing();
            let (admission, _) = continued(scene.run(&[], None, &mut Recorded::default()));
            assert!(admission.is_none());
            assert_eq!(
                scene.listing(),
                before,
                "the admission file is never created by a start"
            );
        }

        /// RED (U-12) — **a preparing or deferred transaction lets the start
        /// continue, untouched, when the start is the build that began it.**
        ///
        /// (b).2: "`preparing` or `deferred` → continue normally. The in-app job
        /// decides." The lock is asked for (a discard would need it) and given
        /// back.
        ///
        /// MUTATION: in `run`, `std::mem::forget(lock)` in the `Continue` arm
        /// (the lock is never given back).
        #[test]
        fn a_waiting_transaction_lets_its_own_build_continue() {
            for class in [Class::Preparing, Class::Deferred] {
                let Some(scene) = Scene::new("waiting") else {
                    return;
                };
                let journal = scene.journal(class);
                let mut world = Recorded::default();
                let (_, trial) = continued(scene.run(&[], None, &mut world));
                assert_eq!(trial, None);
                assert_eq!(std::fs::read(scene.home.journal()).unwrap(), journal);
                assert!(scene.rescue.exists(), "{class:?}");
                assert!(world.spawned.is_empty() && world.entrances.is_empty());
                assert!(world.said.is_empty(), "{:?}", world.said);
                assert!(
                    install_txn::try_hold(&scene.home.lock(), Hold::Exclusive)
                        .unwrap()
                        .is_some()
                );
            }
        }

        /// RED (U-12) — **a start whose own image is not the rescue copy, while
        /// the transaction waits, abandons the transaction: `H\<txn>` and the
        /// journal are removed, and nothing of the install.**
        ///
        /// (b).2: "A start whose own image digest differs from the journal's old
        /// `folio.exe` while `class` is `preparing` or `deferred` means the folder
        /// was replaced by hand. The transaction is abandoned, and only `H\<txn>`
        /// is deleted" — the journal with it, once the folder is gone, so no
        /// journal names a folder that is not there (U-10 decision 14). The
        /// digests are the real SHA-256 of the real files.
        ///
        /// MUTATION: in `run`, take `rescue_image` from `image(start.own_exe)` (the
        /// two digests are then always equal).
        #[test]
        fn an_install_replaced_by_hand_abandons_only_the_transaction() {
            let Some(scene) = Scene::new("replaced") else {
                return;
            };
            scene.journal(Class::Deferred);
            std::fs::write(&scene.own_exe, b"a folio.exe unpacked over the old one").unwrap();
            let mut world = Recorded::default();
            continued(scene.run(&[], None, &mut world));
            assert_eq!(
                scene.listing(),
                vec![
                    ".folio-update",
                    ".folio-update/admission",
                    ".folio-update/lock",
                    "conpty.dll",
                    "folio.exe",
                ]
            );
            assert_eq!(
                std::fs::read(&scene.own_exe).unwrap(),
                b"a folio.exe unpacked over the old one"
            );
            assert!(
                world.entrances.is_empty(),
                "a discard has no entrance to retire"
            );
            assert!(world.spawned.is_empty());
        }

        /// RED (U-12) — **a start carrying the trial words of the destructive
        /// transaction is its trial: it continues and knows its nonce; a start
        /// without them, with another transaction's, or with malformed ones is
        /// handed to the rescue build.**
        ///
        /// (b).2: "`destructive`, or `Trial` without its nonce → … the rescue
        /// helper"; F-7: the start carrying the trial nonce runs, and its write
        /// gate (U-13) reads the fact kept here.
        ///
        /// MUTATION: in `run`, build the view with `trial_of: None`.
        #[test]
        fn only_the_trial_of_the_destructive_transaction_continues() {
            let Some(scene) = Scene::new("trial") else {
                return;
            };
            scene.journal(Class::Destructive);
            let own = trial_arg(txn(), nonce());
            let mut world = Recorded::default();
            let (admission, trial) =
                continued(scene.run(&["--update-trial"], Some(&own), &mut world));
            assert_eq!(trial, Some((txn(), nonce())));
            assert!(world.spawned.is_empty());
            drop(admission);

            let another = trial_arg(TxnId::new([0x11; 16]), nonce());
            let malformed = cli::UpdateTrialArg {
                txn: "not hex".to_owned(),
                nonce: nonce().to_string(),
            };
            for (trial, lines) in [(None, 1), (Some(&another), 1), (Some(&malformed), 2)] {
                let mut world = Recorded::default();
                assert_eq!(exited(scene.run(&[], trial, &mut world)), 0);
                assert_eq!(world.spawned.len(), 1);
                assert_eq!(world.said.len(), lines, "{:?}", world.said);
            }
        }

        /// RED (U-12) — **a journal this build cannot read is left exactly as it
        /// is, and the start continues with one line.**
        ///
        /// U-10 decision 12: nothing is deleted on the strength of bytes nobody
        /// understood — a later version's header is not this build's to judge.
        ///
        /// MUTATION: drop the `world.say` in `run`'s unreadable-header arm.
        #[test]
        fn an_unreadable_journal_is_left_alone() {
            let Some(scene) = Scene::new("unreadable") else {
                return;
            };
            let bytes = br#"{"v":2,"txn":"7a","rescue":"x","class":"terminal"}"#;
            std::fs::write(scene.home.journal(), bytes).unwrap();
            let before = scene.listing();
            let mut world = Recorded::default();
            continued(scene.run(&[], None, &mut world));
            assert_eq!(std::fs::read(scene.home.journal()).unwrap(), bytes);
            assert_eq!(scene.listing(), before);
            assert_eq!(world.said.len(), 1, "{:?}", world.said);
            assert!(world.spawned.is_empty() && world.entrances.is_empty());
        }
    }
}
