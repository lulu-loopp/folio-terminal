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
//!      flushed through `bt_platform::logon_hook`, U-22), then on macOS any
//!      image still mounted under `H/<txn>` (detached on a worker of its own,
//!      which the start does not wait for — U-27), then `H\<txn>`, then the
//!      journal, each removed durably through `bt_platform::install_txn`, all
//!      under the transaction lock; continue;
//!    - `destructive`, or the trial of another transaction, or no trial at all
//!      → the rescue build is started detached with
//!      `--update-recover [<home>] --then-launch <this command line>` (the
//!      home named on macOS, whose rescue clone cannot find it from its own
//!      path — U-29), and this process leaves with one line. On a macOS
//!      bundle whose rescue clone is missing or cannot be started, this
//!      start's own program is started the same way instead — the ordinary
//!      start as R (U-29b: recovery runs whatever build can run it) — and only
//!      when that cannot be started either does the start continue, with one
//!      line and the *Update incomplete.* card; elsewhere a missing rescue
//!      build is named in one line and the start continues untouched
//!      (coordinator ruling, 2026-09-27);
//!    - `destructive`, whatever its outcome, and this start carries
//!      `--update-failed <journal>` → continue: a lock holder sent this start
//!      after a rollback, or after its own recovery failed (U-29, U-29b).
//!      Every start that carries the word reads the card it raises from the
//!      header alone (`update_txn::after_rollback`): *Previous version
//!      restored.* once a rolled-back transaction is retired, *Update
//!      incomplete.* and the journal's folder while the transaction is still
//!      `destructive` ([`failed`]) — a trial's start included;
//!    - `preparing` / `deferred` → continue, unless this start's own image is
//!      not the rescue copy of the build that began the transaction (the
//!      folder was replaced by hand): then `H\<txn>` (its image detached first,
//!      as above) and the journal are removed under the lock, nothing in the
//!      install, and the start continues. A transaction the start continues
//!      past is **left for this launch's job owner** ([`waiting`]): the update
//!      job's pass sweeps, counts, resumes or discards it on its worker
//!      (`update_job::Job::after_start`, U-33) — the start itself never does.
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
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn::{self, Held, Hold};

use crate::cli;
use crate::update_job::Failure;
use crate::update_txn::{
    AfterRollback, Class, Digest, Effect, Header, Home, JournalRead, Nonce, StartAction, StartView,
    TxnId, after_rollback, at_start, rolled_back_untried,
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

/// **The failure a rollback sent this start to report**, set only by
/// [`pass`] (U-29).
static FAILED: OnceLock<Failure> = OnceLock::new();

/// **The home whose `preparing` / `deferred` transaction this start continued
/// past**, set only by [`pass`] (U-33).
static WAITING: OnceLock<Home> = OnceLock::new();

/// **The installation home whose transaction waits for this launch's job
/// owner** — the start found a `preparing` or `deferred` journal there and
/// continued past it, touching nothing (the ordinary start's rule). The update
/// job's pass at this launch (`update_job::Job::after_start`) is what sweeps,
/// counts, resumes or discards it. `None` for every other start.
pub(crate) fn waiting() -> Option<Home> {
    WAITING.get().cloned()
}

/// **What the card of this launch says about an earlier launch's update**:
/// set when this start carried `--update-failed` and its home's journal says
/// `rolled_back` — the job starts at `Failed` with it
/// (`update_job::Job::after_rollback`). `None` for every other start.
pub(crate) fn failed() -> Option<Failure> {
    FAILED.get().cloned()
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
    /// **The mount points under `folder`**, from the mount table — which
    /// waits on nothing (`bt_platform::macos_update::mounts_under`). None on a
    /// platform with no disk images to mount; a table that cannot be read is
    /// an error, never "nothing mounted", because a deletion follows.
    fn mounts_under(&mut self, folder: &Path) -> Result<Vec<PathBuf>, String>;
    /// **Run `job` on a worker of its own, and do not wait for it**: the rest
    /// of a retirement that must first detach an image, since a detach waits
    /// on `hdiutil` and the start's window thread waits on nothing it does not
    /// have to. The job says, as its error, the one line a failure gets.
    fn on_a_worker(&mut self, job: OffThread) -> io::Result<()>;
}

/// The rest of a retirement, handed to a worker ([`World::on_a_worker`]).
pub(crate) type OffThread =
    Box<dyn FnOnce(&bt_platform::admission::WorkerCtx) -> Result<(), String> + Send>;

/// What one start is: its own executable, its installation home, its command
/// line, its `--update-trial` values and its `--update-failed` journal.
pub(crate) struct Start<'a> {
    pub(crate) own_exe: &'a Path,
    pub(crate) home: &'a Home,
    pub(crate) argv: &'a [OsString],
    pub(crate) trial: Option<&'a cli::UpdateTrialArg>,
    pub(crate) failed: Option<&'a Path>,
}

/// What the pass decided.
pub(crate) enum Verdict {
    /// Start as usual, holding `admission` for the life of the process, with
    /// the card a rollback sent this start to raise, and the home whose
    /// `preparing` / `deferred` transaction the start left for this launch's
    /// job owner ([`waiting`]).
    Continue {
        admission: Option<Held>,
        trial: Option<(TxnId, Nonce)>,
        failed: Option<Failure>,
        waiting: Option<Home>,
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
        failed: request.update_failed.as_deref(),
    };
    match run(&start, &mut Machine) {
        Verdict::Exit(code) => bt_platform::leave_process(code),
        Verdict::Continue {
            admission,
            trial,
            failed,
            waiting,
        } => {
            if let Some(held) = admission {
                let _ = ADMISSION.set(held);
            }
            if let Some(home) = waiting {
                let _ = WAITING.set(home);
            }
            if let Some((txn, nonce)) = trial {
                let _ = TRIAL.set(Trial { txn, nonce, home });
            }
            if let Some(failure) = failed {
                let _ = FAILED.set(failure);
            }
            Admitted(())
        }
    }
}

/// **The pass over any home**: the steps of the module header, in order.
pub(crate) fn run(start: &Start<'_>, world: &mut impl World) -> Verdict {
    let admission = admit(start.home, world);
    let journal_path = start.home.journal();
    let mut untried = false;
    let journal = match file_reads::read(Lane::Install, &journal_path) {
        Ok(bytes) => match Header::parse(&bytes) {
            Ok(header) if start.failed.is_some() => {
                untried = rolled_back_untried(&bytes);
                JournalRead::Read(header)
            }
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
                failed: None,
                waiting: None,
            };
        }
    };
    let JournalRead::Read(header) = &journal else {
        return Verdict::Continue {
            admission,
            trial: None,
            failed: None,
            waiting: None,
        };
    };
    let header = header.clone();
    let trial = trial_of(start.trial, world);
    // The card is read before a retirement removes the journal it is read
    // from. The word's value is not read (U-32, U-29's open point 6): the
    // card is this home's header, so the folder it names is this home's —
    // the folder of the journal just read — whatever path the word carried.
    let failed = start
        .failed
        .and_then(|_| after_rollback(&header))
        .map(|after| match after {
            AfterRollback::Restored if untried => Failure::Interrupted,
            AfterRollback::Restored => Failure::RolledBack,
            AfterRollback::Incomplete => Failure::Incomplete {
                folder: start.home.root().to_path_buf(),
            },
        });
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
        sent_by_rollback: start.failed.is_some(),
    };
    // The start's lock is let go before the job owner asks for it: a
    // transaction continued past is the job's, at this launch (U-33).
    let waiting =
        matches!(header.class, Class::Preparing | Class::Deferred).then(|| start.home.clone());
    match at_start(&view) {
        StartAction::Continue => Verdict::Continue {
            admission,
            trial: None,
            failed,
            waiting,
        },
        StartAction::RunAsTrial => Verdict::Continue {
            admission,
            trial,
            failed,
            waiting: None,
        },
        action @ (StartAction::Retire | StartAction::Discard) => {
            retire(action, &header, start.home, lock, world);
            Verdict::Continue {
                admission,
                trial: None,
                failed,
                waiting: None,
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
///
/// **An image still mounted under `H/<txn>` is detached before the folder is
/// deleted** (U-17's debt 7, the coordinator's ruling in U-27): a read-only
/// volume inside the folder would stop the deletion halfway and keep the
/// transaction for ever. The mount table is read here, which waits on nothing;
/// when it lists a mount, the detach and every effect after it — holding the
/// transaction lock — go to a worker ([`World::on_a_worker`]), because a
/// detach waits on `hdiutil` and this is the window thread. The start goes on
/// without waiting for it.
fn retire(
    action: StartAction,
    header: &Header,
    home: &Home,
    mut lock: Option<Held>,
    world: &mut impl World,
) {
    let effects = action.effects();
    let folder = home.transaction(header.txn);
    for (at, effect) in effects.iter().enumerate() {
        let done = match effect {
            Effect::RemoveEntrance => world.retire_entrance(header.txn),
            Effect::DetachMount => match world.mounts_under(&folder) {
                Ok(points) if points.is_empty() => Ok(()),
                Ok(_) => {
                    let rest = effects[at..].to_vec();
                    let (txn, home, held) = (header.txn, home.clone(), lock.take());
                    let job: OffThread = Box::new(move |worker| {
                        // The lock goes with the effects it guards, and is
                        // let go when they are done.
                        let _held = held;
                        for effect in rest {
                            perform(worker, effect, txn, &home).map_err(|failure| {
                                format!(
                                    "BT_UPDATE_START transaction {txn} is kept for the next start: {failure}"
                                )
                            })?;
                        }
                        Ok(())
                    });
                    match world.on_a_worker(job) {
                        Ok(()) => return,
                        Err(error) => Err(format!("no worker to detach its image on: {error}")),
                    }
                }
                Err(error) => Err(error),
            },
            Effect::DeleteTxnDir | Effect::DeleteJournal => {
                delete(*effect, header.txn, home).map_err(|failure| failure.to_string())
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

/// One of a retirement's effects on a worker: the detach of every image under
/// `H/<txn>` (`bt_platform::macos_update::detach_all_under`), or a deletion.
fn perform(
    worker: &bt_platform::admission::WorkerCtx,
    effect: Effect,
    txn: TxnId,
    home: &Home,
) -> Result<(), String> {
    match effect {
        Effect::DetachMount => {
            bt_platform::macos_update::detach_all_under(worker, &home.transaction(txn))
                .map_err(|refusal| refusal.to_string())
        }
        Effect::DeleteTxnDir | Effect::DeleteJournal => {
            delete(effect, txn, home).map_err(|failure| failure.to_string())
        }
        other => unreachable!("a start's action has no {other:?} after its detach"),
    }
}

/// `H\<txn>` or the journal, removed durably.
fn delete(effect: Effect, txn: TxnId, home: &Home) -> Result<(), install_txn::Failure> {
    match effect {
        Effect::DeleteTxnDir => install_txn::durable_remove(&home.transaction(txn)),
        _ => install_txn::durable_remove(&home.journal()),
    }
}

/// The rescue build takes this start: it is started with this start's own
/// command line after `--then-launch`, and this process leaves.
///
/// **A rescue build that is missing or cannot be started never stops the
/// start** (coordinator ruling, 2026-09-27). On a macOS bundle this start's
/// own program runs the recovery instead (U-29b: the recovery door takes the
/// home from the line, and the installed build is a build of the same
/// publisher), so every phase is still finished and exactly one start
/// follows. Where that cannot be started either — or on Windows, until U-24 —
/// one line names the program and the transaction, and the start continues as
/// a waiting transaction's does — no journal write, no deletion — on a macOS
/// bundle with the *Update incomplete.* card and the home's folder. An app
/// that never opens again is not an answer.
fn hand_to_rescue(
    start: &Start<'_>,
    header: &Header,
    admission: Option<Held>,
    world: &mut impl World,
) -> Verdict {
    let program = start.home.rescue_program(&header.rescue);
    // A macOS rescue clone is found from the home the line names (F-3); the
    // Windows rescue build derives its home from its own path (F-2).
    let named = start.home.installed_bundle().map(|_| start.home.root());
    let line = cli::recover_command_line(named, start.argv);
    let mut refused = match world.spawn_detached(&program, &line) {
        Ok(()) => {
            world.say(&format!(
                "BT_UPDATE_START an update is being finished by {}; Folio opens when it is done",
                program.display()
            ));
            return Verdict::Exit(0);
        }
        Err(error) => format!("its rescue build {} ({error})", program.display()),
    };
    if named.is_some() {
        match world.spawn_detached(start.own_exe, &line) {
            Ok(()) => {
                world.say(&format!(
                    "BT_UPDATE_START an update is being finished by {} in place of {}; Folio opens when it is done",
                    start.own_exe.display(),
                    program.display()
                ));
                return Verdict::Exit(0);
            }
            Err(error) => {
                refused.push_str(&format!(
                    " or this build {} ({error})",
                    start.own_exe.display()
                ));
            }
        }
    }
    world.say(&format!(
        "BT_UPDATE_START transaction {} is unfinished and {refused} could not be started; Folio starts without it",
        header.txn,
    ));
    Verdict::Continue {
        admission,
        trial: None,
        failed: named.map(|home| Failure::Incomplete {
            folder: home.to_path_buf(),
        }),
        waiting: None,
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
        // its door (U-22). macOS: the LaunchAgent plist in
        // `~/Library/LaunchAgents`, removed and its folder flushed by its door
        // (U-26; wired here by U-28, the first ticket whose transactions reach
        // `Armed`). An entrance already gone is success on both.
        match bt_platform::host_platform() {
            bt_platform::HostPlatform::Windows => {
                bt_platform::logon_hook::disarm(txn.bytes()).map_err(|refusal| refusal.to_string())
            }
            bt_platform::HostPlatform::MacOs => crate::update_apply_macos::retire_entrance_in(
                crate::update_apply_macos::launch_agents().as_deref(),
                txn,
            ),
            bt_platform::HostPlatform::OtherUnix => Ok(()),
        }
    }

    fn mounts_under(&mut self, folder: &Path) -> Result<Vec<PathBuf>, String> {
        // Only macOS mounts an update's image (U-17); elsewhere there is none.
        if bt_platform::host_platform() != bt_platform::HostPlatform::MacOs {
            return Ok(Vec::new());
        }
        bt_platform::macos_update::mounts_under(folder).map_err(|refusal| refusal.to_string())
    }

    fn on_a_worker(&mut self, job: OffThread) -> io::Result<()> {
        bt_platform::spawn_at_priority(
            "bt-update-sweep",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                if let Err(line) = job(worker) {
                    bt_platform::write_std_error(format!("{line}\n").as_bytes());
                }
            },
        )
        .map(drop)
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
            /// What each job handed to a worker answered; the test's world
            /// waits for it, where the product's does not.
            workers: Vec<Result<(), String>>,
        }

        impl World for Recorded {
            fn mounts_under(&mut self, folder: &Path) -> Result<Vec<PathBuf>, String> {
                Machine.mounts_under(folder)
            }

            fn on_a_worker(&mut self, job: OffThread) -> io::Result<()> {
                let worker = bt_platform::spawn_at_priority(
                    "bt-update-sweep-test",
                    bt_platform::ThreadPriority::BelowNormal,
                    job,
                )?;
                self.workers.push(
                    worker
                        .join()
                        .expect("the retirement's worker does not panic"),
                );
                Ok(())
            }

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
                self.journal_of(class, crate::update_txn::HeaderOutcome::None)
            }

            fn journal_of(
                &self,
                class: Class,
                outcome: crate::update_txn::HeaderOutcome,
            ) -> Vec<u8> {
                let bytes = Header {
                    txn: txn(),
                    rescue: self.rescue.to_string_lossy().into_owned(),
                    class,
                    outcome,
                }
                .encode();
                std::fs::write(self.home.journal(), &bytes).unwrap();
                bytes
            }

            /// The pass for a start that carries `--update-failed <journal>`.
            fn run_sent(&self, journal: &Path, world: &mut Recorded) -> Verdict {
                let argv = [
                    OsString::from(cli::UPDATE_FAILED_FLAG),
                    journal.as_os_str().to_owned(),
                ];
                world.admission = Some(self.home.admission());
                run(
                    &Start {
                        own_exe: &self.own_exe,
                        home: &self.home,
                        argv: &argv,
                        trial: None,
                        failed: Some(journal),
                    },
                    world,
                )
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
                        failed: None,
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
                Verdict::Continue {
                    admission, trial, ..
                } => (admission, trial),
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
                    cli::recover_command_line(
                        None,
                        &["--cwd", "D:\\x", "--tab"].map(OsString::from)
                    )
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

        /// RED (U-27) — **a retirement and a discard detach an image still
        /// mounted under `H/<txn>` before they delete it**, so a leftover
        /// mount no longer wedges the deletion; the detach runs on a worker of
        /// its own, never on the start's thread (macOS: the only platform that
        /// mounts an update's image).
        ///
        /// U-17's debt 7, the coordinator's ruling: `remove_dir_all` descends
        /// into a read-only volume and fails, and the transaction was then kept
        /// at every start for ever. The image is attached here the way a dead
        /// Prepare leaves it — no record, just a mount point under the folder.
        ///
        /// MUTATION: drop `Effect::DetachMount` from `StartAction::Retire`'s and
        /// `StartAction::Discard`'s effects in `update_txn`.
        #[test]
        fn retire_and_discard_detach_before_deleting() {
            use crate::update_prepare_macos::tests::fixture;
            if !fixture::on_macos() {
                return;
            }
            let scratch = fixture::Scratch::new("startup-detach");
            let image = fixture::blank_image(&scratch.root.join("left.dmg"));
            for (class, tag) in [(Class::Terminal, "retire"), (Class::Deferred, "discard")] {
                let Some(scene) = Scene::new(&format!("detach-{tag}")) else {
                    return;
                };
                scene.journal(class);
                if class == Class::Deferred {
                    // The folder was replaced by hand: the start's own image
                    // is no longer the rescue copy, so the start discards.
                    std::fs::write(&scene.own_exe, b"a folio.exe unpacked over it").unwrap();
                }
                let folder = scene.home.transaction(txn());
                // Dropped before `scene`: whatever is still mounted comes off,
                // on a failed assertion too.
                let _detach = fixture::Detach(scene.root.clone());
                fixture::attach(&image, &folder.join("mnt"));
                let mut world = Recorded::default();
                let (admission, _) = continued(scene.run(&[], None, &mut world));
                assert!(admission.is_some(), "{tag}");
                assert_eq!(world.workers, vec![Ok(())], "{tag}: {:?}", world.said);
                assert!(fixture::mounted(&scene.root).is_empty(), "{tag}: detached");
                assert!(!folder.exists(), "{tag}: H/<txn> is deleted");
                assert!(!scene.home.journal().exists(), "{tag}: then the journal");
                assert!(world.said.is_empty(), "{tag}: {:?}", world.said);
            }
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

        /// RED (U-29) — **a start that a rollback sent continues past the
        /// transaction it rolled back and carries the card the header asks
        /// for: `Update incomplete.` with the named journal's folder while the
        /// journal is still destructive, `Previous version restored.` once it
        /// is retired (and the start retires it, after reading it).** Without
        /// the word, the same destructive journal hands the start over.
        ///
        /// The rescue build that could not finish sends the start with
        /// `--update-failed <journal>` so that Folio opens at all; handing it
        /// back would send it round again.
        ///
        /// MUTATION: in `run`, build the view with `sent_by_rollback: false`.
        #[test]
        fn a_start_sent_after_a_rollback_continues_with_its_card() {
            use crate::update_txn::HeaderOutcome;
            let Some(scene) = Scene::new("sent") else {
                return;
            };
            let journal = scene.journal_of(Class::Destructive, HeaderOutcome::RolledBack);
            let mut world = Recorded::default();
            let verdict = scene.run_sent(&scene.home.journal(), &mut world);
            let Verdict::Continue {
                admission, failed, ..
            } = verdict
            else {
                panic!("a start the rollback sent must continue");
            };
            assert_eq!(
                failed,
                Some(Failure::Incomplete {
                    folder: scene.home.root().to_path_buf()
                })
            );
            assert!(world.spawned.is_empty(), "{:?}", world.spawned);
            assert_eq!(std::fs::read(scene.home.journal()).unwrap(), journal);
            drop(admission);

            let mut world = Recorded::default();
            assert_eq!(exited(scene.run(&[], None, &mut world)), 0);
            assert_eq!(world.spawned.len(), 1, "without the word it hands over");

            scene.journal_of(Class::Terminal, HeaderOutcome::RolledBack);
            let mut world = Recorded::default();
            let Verdict::Continue { failed, .. } =
                scene.run_sent(&scene.home.journal(), &mut world)
            else {
                panic!("a retired transaction never stops a start");
            };
            assert_eq!(failed, Some(Failure::RolledBack));
            assert!(!scene.home.journal().exists(), "and it is retired");

            let mut world = Recorded::default();
            let Verdict::Continue { failed, .. } =
                scene.run_sent(&scene.home.journal(), &mut world)
            else {
                panic!("no journal never stops a start");
            };
            assert_eq!(failed, None, "no journal, no card");
        }

        /// RED (U-42a) — **a start sent after a rollback no trial ever began
        /// says the update was interrupted before the new version started,
        /// and one sent after a trial's rollback still says the new version
        /// did not start; both say the previous version is back.**
        ///
        /// 0.4.6's D-7: after a power cut during `Moving` (W6) the card said
        /// *The new version did not start.* — the new version never ran. The
        /// journals here are the real transitions' (`Moving` or `Trial` →
        /// `RollbackIntent` → `RolledBack` → `Retired`), read by the real
        /// pass, painted by the real card.
        ///
        /// MUTATION: in `update_txn::next`, write `RolledBack { untried:
        /// false }` whatever the trial — the W6 card says the new version did
        /// not start.
        #[test]
        fn a_rollback_before_any_trial_says_the_update_was_interrupted() {
            use crate::i18n::Text;
            use crate::update_txn::{
                Body, Event, Inventories, Journal, Layout, Phase, TrialProcess,
            };
            let Some(scene) = Scene::new("interrupted") else {
                return;
            };
            let at = |phase: Phase| Journal {
                txn: txn(),
                rescue: scene.rescue.to_string_lossy().into_owned(),
                body: Body {
                    adapter: crate::update_txn::Adapter::Ours,
                    phase,
                    layout: Layout::Members(Inventories {
                        old_shipped: vec!["folio.exe".to_owned()],
                        old_present: Vec::new(),
                        new: Vec::new(),
                    }),
                },
            };
            let trial = Phase::Trial {
                nonce: nonce(),
                process: TrialProcess {
                    pid: 4242,
                    started: 7,
                },
                began_ms: 1,
            };
            for (from, failure, heading) in [
                (
                    Phase::Moving,
                    Failure::Interrupted,
                    Text::UpdateFailedInterrupted,
                ),
                (trial, Failure::RolledBack, Text::UpdateFailedTrial),
            ] {
                let retired = [Event::RollbackDeclared, Event::RolledBack, Event::Retired]
                    .iter()
                    .try_fold(at(from.clone()), |journal, event| journal.advance(event))
                    .expect("the rollback's transitions");
                std::fs::write(scene.home.journal(), retired.encode()).unwrap();
                let mut world = Recorded::default();
                let Verdict::Continue { failed, .. } =
                    scene.run_sent(&scene.home.journal(), &mut world)
                else {
                    panic!("a retired transaction never stops a start");
                };
                assert_eq!(failed, Some(failure.clone()), "from {from:?}");
                let card =
                    crate::update_card::paint(&crate::update_job::State::Failed(None, failure))
                        .expect("a failed card");
                assert_eq!(
                    card.heading.as_deref(),
                    Some(heading.text()),
                    "from {from:?}"
                );
                assert_eq!(
                    card.detail.as_deref(),
                    Some(Text::UpdateCardRestored.text()),
                    "from {from:?}"
                );
            }
        }

        /// RED (U-32) — **the card a start sent with `--update-failed` raises
        /// names the folder of its own home's journal, whatever path the word
        /// carried.**
        ///
        /// U-29's open point 6: the word's value was used only as the card's
        /// folder, and was never checked. The card is read from this home's
        /// header, so the folder it names is this home's; the value is not
        /// read at all, and so needs no check. A word naming another place —
        /// a stale path, a path spelt differently — shows the same folder.
        ///
        /// MUTATION: in `run`, take `Incomplete`'s folder from the word's
        /// value again (`journal.parent()`).
        #[test]
        fn the_incomplete_card_names_its_own_homes_folder_whatever_the_word_says() {
            use crate::update_txn::HeaderOutcome;
            let Some(scene) = Scene::new("sent-elsewhere") else {
                return;
            };
            scene.journal_of(Class::Destructive, HeaderOutcome::RolledBack);
            let elsewhere = scene.root.join("elsewhere").join("journal.json");
            let mut world = Recorded::default();
            let Verdict::Continue { failed, .. } = scene.run_sent(&elsewhere, &mut world) else {
                panic!("a start the rollback sent must continue");
            };
            assert_eq!(
                failed,
                Some(Failure::Incomplete {
                    folder: scene.home.root().to_path_buf()
                }),
                "the folder of the journal the card was read from"
            );
        }

        /// RED (U-32) — **a trial started over `Stuck` that commits forward
        /// says the update is done: its card, *Update incomplete.* at launch,
        /// becomes the updated card once its watch reads `Committed`, and a
        /// card the reader closed comes back once to say so.**
        ///
        /// U-29b's open point 3 and the coordinator's ruling 2: a lock holder
        /// that finds `Stuck` with the new build live starts it as a trial —
        /// `--update-trial` with a recorded nonce and `--update-failed` — and
        /// that trial's receipt commits the transaction forward. The card had
        /// been read from the header at launch (`destructive` → *Update
        /// incomplete.*) and kept saying so after the commit. Every step here
        /// is the product's: the journal is `Stuck` with the retrial recorded
        /// and then `Committed` on its receipt through `update_txn`'s own
        /// transitions, the start is the real pass, the job is seeded by what
        /// the pass decided, and the commit is read by the trial's real watch.
        /// A trial that was never told anything (the happy path) is not told
        /// this either.
        ///
        /// MUTATION: in `Job::after_commit`, answer `false` and leave the
        /// state as it is.
        #[test]
        fn a_stuck_retrial_that_commits_shows_the_update_done() {
            use crate::update_card::{self, CardVerb};
            use crate::update_job::{Job, Presenters, State, Verb};
            use crate::update_txn::{
                Body, Event, Inventories, Journal, Layout, Phase, Receipt, Retrial, TrialProcess,
            };
            let Some(scene) = Scene::new("stuck-retrial") else {
                return;
            };
            let process = TrialProcess {
                pid: 4242,
                started: 7,
            };
            let stuck = Journal {
                txn: txn(),
                rescue: scene.rescue.to_string_lossy().into_owned(),
                body: Body {
                    adapter: crate::update_txn::Adapter::Ours,
                    phase: Phase::Stuck {
                        trial: None,
                        last_error: "the exchange back was refused".to_owned(),
                        attempts: 1,
                        retrial: None,
                    },
                    layout: Layout::Members(Inventories {
                        old_shipped: vec!["folio.exe".to_owned()],
                        old_present: Vec::new(),
                        new: Vec::new(),
                    }),
                },
            }
            .advance(&Event::RetrialBegan {
                nonce: nonce(),
                process,
                began_ms: 1,
            })
            .expect("the holder records the trial it started over Stuck");
            assert!(matches!(
                stuck.body.phase,
                Phase::Stuck {
                    retrial: Some(Retrial { .. }),
                    ..
                }
            ));
            std::fs::write(scene.home.journal(), stuck.encode()).unwrap();

            // The trial's start: both words, as the holder writes them.
            let journal = scene.home.journal();
            let trial = trial_arg(txn(), nonce());
            let argv = [
                OsString::from(cli::UPDATE_FAILED_FLAG),
                journal.clone().into_os_string(),
            ];
            let mut world = Recorded {
                admission: Some(scene.home.admission()),
                ..Recorded::default()
            };
            let Verdict::Continue {
                admission,
                trial: as_trial,
                failed,
                ..
            } = run(
                &Start {
                    own_exe: &scene.own_exe,
                    home: &scene.home,
                    argv: &argv,
                    trial: Some(&trial),
                    failed: Some(&journal),
                },
                &mut world,
            )
            else {
                panic!("the retrial continues as the trial");
            };
            assert_eq!(as_trial, Some((txn(), nonce())), "a trial");
            let presenters = Presenters {
                visited: &[1],
                open: &[1],
                quake: None,
            };
            let mut job: Job<u32> = Job::with_offers(true).after_rollback(failed);
            job.hand_over(&presenters);
            let at_launch = update_card::paint(job.state()).expect("a card at launch");
            assert_eq!(
                at_launch.detail.as_deref(),
                Some(crate::i18n::Text::UpdateCardIncomplete.text()),
                "at launch the header said the update was incomplete"
            );

            // Its receipt commits it forward; the trial's watch reads that.
            let committed = stuck
                .advance(&Event::ReceiptAccepted(Receipt {
                    txn: txn(),
                    nonce: nonce(),
                    pid: process.pid,
                    version: "0.4.7".to_owned(),
                    started: None,
                }))
                .expect("the receipt of the recorded retrial commits");
            assert_eq!(committed.body.phase, Phase::Committed);
            std::fs::write(&journal, committed.encode()).unwrap();
            let gate = crate::update_trial::Gate::new();
            let woke = std::cell::Cell::new(false);
            crate::update_trial::watch(
                &gate,
                &journal,
                txn(),
                std::time::Duration::from_millis(5),
                &|| woke.set(true),
                &mut crate::update_trial::watchdog_asleep(),
            );
            assert!(woke.get(), "the watch read the commit and woke the window");

            assert!(job.after_commit("0.4.7"), "the card follows the commit");
            assert_eq!(job.state(), &State::Updated("0.4.7".to_owned()));
            assert_eq!(job.card_window(), Some(1), "in the window the card was in");
            let done = update_card::paint(job.state()).expect("a card after the commit");
            assert_eq!(done.heading.as_deref(), Some("Folio 0.4.7"));
            assert_eq!(
                done.detail.as_deref(),
                Some(crate::i18n::Text::UpdateCardUpdated.text())
            );
            assert_eq!(done.folder, None, "no folder to show: nothing is left");
            assert_eq!(done.verbs, vec![CardVerb::Close]);
            assert!(!job.after_commit("0.4.7"), "said once");
            job.answer_verb(
                Verb::Later,
                &crate::update_job::Unsupported,
                &nothing_fetched(),
            )
            .expect("Close puts it away");
            assert_eq!(job.state(), &State::Idle);
            drop(admission);

            // The reader closed *Update incomplete.* before the commit: the
            // updated card comes back once, in the last active window.
            let mut closed: Job<u32> =
                Job::with_offers(true).after_rollback(Some(Failure::Incomplete {
                    folder: scene.home.root().to_path_buf(),
                }));
            closed.hand_over(&presenters);
            closed
                .answer_verb(
                    Verb::Later,
                    &crate::update_job::Unsupported,
                    &nothing_fetched(),
                )
                .expect("Close");
            assert_eq!(closed.card_window(), None);
            assert!(closed.after_commit("0.4.7"));
            closed.hand_over(&presenters);
            assert_eq!(closed.card_window(), Some(1));

            // A trial nobody told anything (the happy path) shows nothing.
            let mut plain: Job<u32> = Job::with_offers(true).after_rollback(None);
            assert!(!plain.after_commit("0.4.7"));
            assert_eq!(update_card::paint(plain.state()), None);
            let mut restored: Job<u32> =
                Job::with_offers(true).after_rollback(Some(Failure::RolledBack));
            assert!(
                !restored.after_commit("0.4.7"),
                "a retired rollback is never committed"
            );
        }

        /// The transport a verb that fetches nothing is handed.
        struct NoTransport;

        impl crate::update_job::Transport for NoTransport {
            fn fetch(
                &self,
                _request: &crate::update_job::Request,
                _into: &Path,
                _fetching: &crate::update_job::Fetching,
            ) -> Result<PathBuf, String> {
                Err("nothing is fetched here".to_owned())
            }
        }

        fn nothing_fetched() -> crate::update_job::SharedTransport {
            std::sync::Arc::new(NoTransport)
        }

        /// RED (U-29) — **a start in a macOS bundle hands itself to the rescue
        /// clone with the installation home named on the line**: the clone
        /// cannot find the home from its own path, and `--update-recover`
        /// without one is refused there (`Home::of_rescue` is Windows' only).
        /// A Windows start's line names none, as before.
        ///
        /// MUTATION: in `hand_to_rescue`, pass `None` for the home.
        #[test]
        fn a_macos_start_names_the_home_when_it_hands_over() {
            let Some(scene) = Scene::new("named") else {
                return;
            };
            let bundle = scene.root.join("Applications").join("Folio.app");
            let exe = bundle.join(crate::update_txn::MACOS_EXECUTABLE_INSIDE);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, RUNNING_BUILD).unwrap();
            let home = Home::of(bt_platform::HostPlatform::MacOs, &exe).unwrap();
            let rescue = home.rescue_bundle(txn()).unwrap();
            let program = home.rescue_executable(txn()).unwrap();
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            std::fs::write(&program, RUNNING_BUILD).unwrap();
            std::fs::write(
                home.journal(),
                Header {
                    txn: txn(),
                    rescue: rescue.to_string_lossy().into_owned(),
                    class: Class::Destructive,
                    outcome: crate::update_txn::HeaderOutcome::None,
                }
                .encode(),
            )
            .unwrap();
            let argv = [OsString::from("--tab")];
            let mut world = Recorded::default();
            let verdict = run(
                &Start {
                    own_exe: &exe,
                    home: &home,
                    argv: &argv,
                    trial: None,
                    failed: None,
                },
                &mut world,
            );
            assert_eq!(exited(verdict), 0);
            assert_eq!(
                world.spawned,
                vec![(program, cli::recover_command_line(Some(home.root()), &argv))]
            );
            assert_eq!(
                cli::update_door(world.spawned[0].1.clone()),
                Some(Ok(cli::UpdateDoor::Recover {
                    home: Some(home.root().to_path_buf()),
                    then_launch: Some(argv.to_vec()),
                    handed_back: None,
                }))
            );
        }

        /// RED (U-29b) — **a macOS start whose rescue clone cannot be started
        /// runs the recovery with its own program instead, the same line and
        /// the home named, and leaves; only when that cannot be started either
        /// does it go on — with the *Update incomplete.* card and the home's
        /// folder, touching nothing.**
        ///
        /// The coordinator's ruling 1 (U-29b): the recovery is run "by the
        /// rescue entrance (`--update-recover`) and, when the rescue cannot be
        /// reached, by the ordinary start itself as R"; ruling 2: when recovery
        /// cannot run, the start still opens with the card. On BASE the start
        /// went on as if nothing were unfinished.
        ///
        /// MUTATION: in `hand_to_rescue`, skip the start of this start's own
        /// program.
        #[test]
        fn an_unreachable_rescue_leaves_the_recovery_to_the_start_itself() {
            let Some(scene) = Scene::new("own-program") else {
                return;
            };
            let bundle = scene.root.join("Applications").join("Folio.app");
            let exe = bundle.join(crate::update_txn::MACOS_EXECUTABLE_INSIDE);
            std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
            std::fs::write(&exe, RUNNING_BUILD).unwrap();
            let home = Home::of(bt_platform::HostPlatform::MacOs, &exe).unwrap();
            let rescue = home.rescue_bundle(txn()).unwrap();
            std::fs::create_dir_all(home.root()).unwrap();
            let journal = Header {
                txn: txn(),
                rescue: rescue.to_string_lossy().into_owned(),
                class: Class::Destructive,
                outcome: crate::update_txn::HeaderOutcome::None,
            }
            .encode();
            std::fs::write(home.journal(), &journal).unwrap();
            let argv = [OsString::from("--tab")];
            let start = Start {
                own_exe: &exe,
                home: &home,
                argv: &argv,
                trial: None,
                failed: None,
            };
            let line = cli::recover_command_line(Some(home.root()), &argv);
            let program = home.rescue_executable(txn()).unwrap();

            let mut world = Recorded::default();
            assert_eq!(exited(run(&start, &mut world)), 0);
            assert_eq!(
                world.spawned,
                vec![(program.clone(), line.clone()), (exe.clone(), line.clone())]
            );
            assert_eq!(world.said.len(), 1, "{:?}", world.said);

            std::fs::remove_file(&exe).unwrap();
            let mut world = Recorded::default();
            let Verdict::Continue { trial, failed, .. } = run(&start, &mut world) else {
                panic!("a start that can start nothing goes on");
            };
            assert_eq!(world.spawned.len(), 2);
            assert_eq!(trial, None);
            assert_eq!(
                failed,
                Some(Failure::Incomplete {
                    folder: home.root().to_path_buf()
                })
            );
            assert_eq!(world.said.len(), 1, "{:?}", world.said);
            assert_eq!(std::fs::read(home.journal()).unwrap(), journal);
            assert!(world.entrances.is_empty());
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
