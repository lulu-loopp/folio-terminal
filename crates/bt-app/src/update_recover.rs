//! **`folio --update-recover [<home>] [--then-launch <argument>...]`: the rescue
//! build's recovery door** (0.4.6 tickets U-22, U-29, U-29b, U-23 and U-24;
//! `docs/plans/design/self-update-2026-09-16.md` revision (b), F-2, F-6 and
//! M9–M11).
//!
//! Two things start the rescue build this way: the entrance at logon
//! (`bt_platform::logon_hook`, `"<H>\<txn>\rescue\folio.exe" --update-recover`;
//! on macOS the LaunchAgent, `--update-recover <home>`), and an ordinary start
//! that found a transaction it may not continue through (`update_startup`,
//! which adds `--then-launch` and its own command line, and on macOS the home
//! before it). On Windows the rescue build finds everything from its own path:
//! the installation home `H` is three folders up, the journal is
//! `H\journal.json`, and the installed program is `<install>\<its own name>`
//! (`update_txn::Home::of_rescue`). On macOS the line names the home
//! (`bt_platform::launch_agent`, U-26): the journal is in it, and the installed
//! bundle is the one the home's name is made of
//! (`update_txn::Home::of_rescue_named`).
//!
//! **What it does.** It reads the journal's frozen header and says one
//! diagnostics line, naming the transaction and its class, on its standard
//! error **and** appended to a file, because the run the entrance starts at
//! logon has no console and a line only on stderr would be lost (coordinator
//! ruling, 2026-09-27). The file is the `diagnostics.log` of the data
//! directory (`persist::storage_dir_unmoved`: the installed program's storage
//! rule, without the relocation or creation an ordinary start may do; the log
//! is append-only and outside the trial's write gate). When that directory
//! does not exist, the line goes to `recover.log` beside the journal, in the
//! home, and says so.
//!
//! **A macOS bundle, in every phase** (U-29, U-29b): when the header is
//! `destructive`, whatever its outcome, the rescue build takes the
//! transaction lock and finishes, as R, whatever a dead applier left
//! (`update_apply_macos::recover`, the coordinator's ruling 1): `Handoff` and
//! `Armed` back to `Prepared`; `Exchanging` decided by the live identity (the
//! old one back to `Prepared`, the new one decided by a trial it starts);
//! `Trial` waited for while its recorded process lives, committed on its
//! receipt, else rolled back; `RollbackIntent`, `Stuck` and `RolledBack` rolled
//! back and retired (M9–M11); `Committed` retired. Every line of it is
//! appended to the same file.
//!
//! **A Windows member set** (U-23, U-24, [`run_windows`]): whatever the
//! header says, the rescue build takes the transaction lock and finishes, as
//! R, whatever a dead applier left, by the applier's own code
//! (`update_apply_windows::recover`), each step `update_txn::decide`'s answer
//! for `Asker::Rescue`: `Handoff` or `Armed` — nothing moved — back to
//! `Prepared`, the entrance removed; `Moving` rolled back from what is on disk
//! (W6); `Trial` waited on while its recorded trial lives and its deadline has
//! not passed, then `Committed` and its retirement, or rolled back;
//! `RollbackIntent`, `Stuck` and `RolledBack` rolled back and retired
//! (W9–W11); `Committed` retired (W12). Every line of it is appended to the
//! same file.
//!
//! **Then it leaves through the one exit guard** (U-34,
//! `update_apply::ExitGuard`; U-29b's "every phase opens Folio" is now this one
//! rule, not a list of ends): a successor it leaves behind still running —
//! the trial it started with the handed command line after its words (over a
//! `Stuck` whose new build is live, recorded so its receipt commits forward,
//! ruling 3), or the applier it found at `Handoff` — opens Folio; otherwise
//! exactly one start of what the disk names, with the handed command line
//! after its words: the old build plainly, or with `--update-failed <journal>`
//! after a rollback or while the journal is still `destructive` (so the start
//! continues instead of handing itself back, and its card says *Update
//! incomplete.*); the new build plainly once `Committed`, and before that only
//! as a trial (`--update-trial <txn> <nonce>`); on Windows, where the install
//! folder holds neither whole set, the rescue copy with `--update-failed`
//! (O's own image, whose own home holds no journal). The same holds when the
//! recovery itself fails, when its lock wait runs out, when the standalone
//! main is refused, and on a panic. **The one exception is the run at logon
//! that did nothing a person is owed a window for** — no command line, and no
//! revert or finished rollback (W8, W11): nobody is waiting, and nothing is
//! started.
//!
//! **A live applier at `Handoff`, on both platforms** (U-24, the
//! coordinator's ruling 3; since U-34 by the window's mark): the applier O
//! started takes the duty that a window follows as soon as it knows its
//! transaction (`H\<txn>\owner`, `update_apply::OWNER_FILE`), before it waits
//! for O's lock; recovery that finds `Handoff` while a live process holds that
//! mark leaves the handed-off transaction to it, writes nothing and waits for
//! nothing — that process is its successor, and opens Folio
//! (`update_apply::the_window_is_theirs`). A process of the rescue image that
//! never took the mark is not waited for.
//!
//! **Any other home**, reading the header:
//!
//! * handed a command line, and the transaction is no longer one an ordinary
//!   start hands over — `terminal`, `preparing` or `deferred`, no journal, or
//!   one this build cannot read — the installed Folio is started with the
//!   original arguments, detached, and the door exits 0 (on a macOS bundle
//!   whose retired outcome is `rolled_back`, with `--update-failed <journal>`
//!   first, for its card). The start that handed itself over is thereby made
//!   (U-12's contract), and it cannot come back here: the ordinary start hands
//!   over only a `destructive` class;
//! * a `destructive` class on a home no road of this build recovers: the
//!   installed Folio with `--update-failed <journal>` before the handed
//!   arguments, which continues past the header with *Update incomplete.*
//!   (U-34: the exit guard's start; U-24 started nothing here).
//!
//! Headless, like the other argv doors: it runs before the parse and the
//! admission in `fn main`, on a standalone main that is a worker
//! (`admission::enter_standalone_main`), never opens a window, and never holds
//! the admission shared (the rescue build is the one process that takes it
//! exclusive, to move files). The reads go through `file_reads` on
//! `Lane::Install`; the start through `bt_platform::quiet_command`, never
//! waited on.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};

use crate::cli;
use crate::update_apply::{ExitGuard, Leave, Left, Opens};
use crate::update_apply_macos::{self, Hands, Limits, Road};
use crate::update_txn::{Class, Header, Home};

/// **The recovery door's effects**: a lock holder's (its lines, the exchange,
/// the check of a restored bundle), and the start of the installed Folio.
pub(crate) trait World: Hands {
    /// Start `program` with `args`, detached: never waited on.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()>;
}

/// **What one run of the door is over**: the home, the installed program, the
/// command line it was handed, the data directory whose log it writes to, the
/// LaunchAgents folder and the limits a rollback waits within.
pub(crate) struct Door<'a> {
    pub(crate) home: &'a Home,
    pub(crate) installed: &'a Path,
    pub(crate) then_launch: Option<&'a [OsString]>,
    pub(crate) data: &'a Path,
    pub(crate) agents: Option<&'a Path>,
    pub(crate) limits: Limits,
    /// This process's starter (U-37, H.3): no candidate.
    pub(crate) starter: Option<bt_platform::install_flip::Running>,
    /// The trial that handed its transaction back (`--from-trial`, H.4).
    pub(crate) handed_back: Option<crate::update_apply::HandedBack>,
}

/// **The door, for this process**: this executable must be a rescue build.
pub(crate) fn run_here(
    home: Option<PathBuf>,
    then_launch: Option<Vec<OsString>>,
    handed_back: Option<crate::update_apply::HandedBack>,
) -> i32 {
    // **The smallest outer guard, first** (U-34, round 2, blocker 3): until the
    // road's own guard carries the duty, a person's start handed here that
    // cannot be finished ends in the failure window, shown by this process;
    // the run at logon owes nobody.
    let mut outer = ExitGuard::new(Unnamed {
        world: Machine,
        failed: None,
    });
    if then_launch.is_none() {
        outer.nobody_waiting();
    }
    let mut world = Machine;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            world.say(&format!(
                "BT_UPDATE_RECOVER cannot name its own executable: {error}"
            ));
            let left = outer.leave();
            world.say(&format!("BT_UPDATE_RECOVER {}", left.said()));
            return 2;
        }
    };
    let platform = bt_platform::host_platform();
    let found = match &home {
        Some(named) => Home::of_rescue_named(platform, &exe, named),
        None => Home::of_rescue(platform, &exe),
    };
    let Some((home, installed)) = found else {
        world.say(&format!(
            "BT_UPDATE_RECOVER {} is not a rescue build; {} runs only from an installation home's rescue folder",
            exe.display(),
            cli::UPDATE_RECOVER_FLAG
        ));
        let left = outer.leave();
        world.say(&format!("BT_UPDATE_RECOVER {}", left.said()));
        return 2;
    };
    // One standalone main for both roads: a Windows member set is recovered
    // by the applier's own code (U-23, U-24), a macOS bundle by U-29/U-29b's.
    let windows = (platform == bt_platform::HostPlatform::Windows).then(|| {
        let mut road =
            crate::update_apply_windows::Road::of_this_copy(home.clone(), installed.clone(), exe);
        road.handed_back = handed_back;
        road
    });
    let data = crate::persist::storage_dir_unmoved();
    let agents = update_apply_macos::launch_agents();
    let door = Door {
        home: &home,
        installed: &installed,
        then_launch: then_launch.as_deref(),
        data: &data,
        agents: agents.as_deref(),
        limits: Limits::PRODUCT,
        starter: bt_platform::install_flip::parent_of_this_process(),
        handed_back,
    };
    match bt_platform::admission::enter_standalone_main("folio-update-recover", |worker| {
        // The road's guard takes the duty over as its first statement.
        outer.hand_on();
        match &windows {
            Some(road) => run_windows(
                worker,
                road,
                then_launch.as_deref(),
                &mut crate::update_apply_windows::Machine { log: None },
            ),
            None => run(worker, &door, &mut world),
        }
    }) {
        Ok(code) => code,
        Err(refused) => {
            world.say(&format!("BT_UPDATE_RECOVER {refused:?}"));
            outer.hand_on();
            // The home is known: this way out leaves through the exit guard
            // too (U-34), with nothing recovered.
            let handed = then_launch.as_deref().unwrap_or(&[]);
            let left = match &windows {
                Some(road) => {
                    let mut machine = crate::update_apply_windows::Machine { log: None };
                    let mut guard = ExitGuard::new(crate::update_apply_windows::WindowsLeave {
                        road,
                        world: &mut machine,
                        handed,
                        worker: None,
                    });
                    if then_launch.is_none() {
                        guard.nobody_waiting();
                    }
                    guard.leave()
                }
                None => {
                    let mut guard = ExitGuard::new(DoorLeave {
                        worker: None,
                        door: &door,
                        world: &mut world,
                    });
                    if then_launch.is_none() {
                        guard.nobody_waiting();
                    }
                    guard.leave()
                }
            };
            world.say(&format!("BT_UPDATE_RECOVER {}", left.said()));
            2
        }
    }
}

/// **How the recovery door leaves before it can name a home** (U-34, round 2):
/// nothing can be started, so the failure window is shown by this process.
struct Unnamed<W: World> {
    world: W,
    failed: Option<String>,
}

impl<W: World> Leave for Unnamed<W> {
    fn say(&mut self, line: &str) {
        self.world.say(line);
    }

    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        None
    }

    fn start(&mut self, _program: &Path, _words: &[OsString]) -> io::Result<()> {
        Err(io::Error::other("nothing can be named to start"))
    }

    fn acknowledged(&mut self) -> bool {
        false
    }

    fn show_here(&mut self, why: &str) {
        self.failed = Some(why.to_owned());
        self.world
            .show_here(&crate::update_apply::failure_text(None));
    }
}

/// **How the recovery door leaves over a home [`run`] reads** (U-34): the
/// installed program started directly with what the disk names — for a macOS
/// bundle's transaction `update_apply_macos::opens_now`; over a home no road
/// of this build recovers, the installed program with `--update-failed` while
/// the header is `destructive` (it continues past it, with *Update
/// incomplete.*), and plainly otherwise — with the handed command line after
/// its words.
struct DoorLeave<'a, W: World> {
    worker: Option<&'a WorkerCtx>,
    door: &'a Door<'a>,
    world: &'a mut W,
}

impl<W: World> Leave for DoorLeave<'_, W> {
    fn say(&mut self, line: &str) {
        self.world.say(line);
    }

    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        let home = self.door.home;
        let opens = if home.installed_bundle().is_some() {
            update_apply_macos::opens_now(self.worker, home)
        } else {
            let destructive = header_of(home)
                .header
                .is_some_and(|header| header.class == Class::Destructive);
            Opens::Installed {
                failed: destructive,
            }
        };
        let mut words = opens.words(home);
        words.extend_from_slice(self.door.then_launch.unwrap_or(&[]));
        Some((self.door.installed.to_path_buf(), words))
    }

    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()> {
        self.world.spawn_detached(program, words)
    }

    fn acknowledged(&mut self) -> bool {
        self.world.acknowledged(self.worker, self.door.data)
    }

    fn show_here(&mut self, why: &str) {
        self.world.say(&format!(
            "BT_UPDATE_EXIT no start was delivered ({why}); the failure window is shown here"
        ));
        self.world
            .show_here(&crate::update_apply::failure_text(Some(self.door.home)));
    }
}

/// The exit code a door's end answers: a start delivered, 0; none delivered
/// (the failure window shown here), 1; otherwise `ended`, the code of what the recovery ended as (0 when
/// nothing was recovered).
fn code_of(left: &Left, ended: i32) -> i32 {
    match left {
        Left::Started(_) => 0,
        Left::ShownHere(_) => 1,
        _ => ended,
    }
}

/// What the header says, for the line and for the decisions after it.
struct Read {
    state: String,
    header: Option<Header>,
}

fn header_of(home: &Home) -> Read {
    let journal = home.journal();
    match file_reads::read(Lane::Install, &journal) {
        Ok(bytes) => match Header::parse(&bytes) {
            Ok(header) => Read {
                state: format!(
                    "transaction {} is {:?} in {}",
                    header.txn,
                    header.class,
                    home.root().display()
                ),
                header: Some(header),
            },
            Err(refusal) => Read {
                state: format!("{} is left as it is: {refusal}", journal.display()),
                header: None,
            },
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Read {
            state: format!("no transaction in {}", home.root().display()),
            header: None,
        },
        Err(error) => Read {
            state: format!("{} could not be read: {error}", journal.display()),
            header: None,
        },
    }
}

/// **The door over any home** — see the module header. Answers the exit
/// code. Every way out leaves through the exit guard (U-34).
pub(crate) fn run(worker: &WorkerCtx, door: &Door<'_>, world: &mut impl World) -> i32 {
    // Every way out, a panic included, leaves through the exit guard (U-34),
    // made first.
    let mut guard = ExitGuard::new(DoorLeave {
        worker: Some(worker),
        door,
        world,
    });
    let home = door.home;
    let (log, whereabouts) = log_file(home, door.data);
    let appended = appended_to(&log);
    let first = header_of(home);
    // A macOS bundle is the layout this door recovers (U-29, U-29b); a
    // Windows member set is `run_windows`'s.
    let recovers_here = home.installed_bundle().is_some();
    let destructive = first
        .header
        .as_ref()
        .filter(|header| recovers_here && header.class == Class::Destructive);
    let (did, ended) = match destructive {
        Some(header) => {
            let road = Road {
                home: home.clone(),
                txn: header.txn,
                nonce: None,
                data: door.data.to_path_buf(),
                agents: door.agents.map(Path::to_path_buf),
                limits: door.limits,
                starter: door.starter,
                handed_back: door.handed_back,
            };
            let mut logged = Logged {
                world: &mut *guard.inner().world,
                log: appended,
            };
            let recovered =
                update_apply_macos::recover(worker, &road, &mut logged, door.then_launch);
            guard.succeeded_by(recovered.successor);
            if !recovered.waiting {
                guard.nobody_waiting();
            }
            if recovered.ended.deferred_to_a_holder() {
                guard.window_elsewhere();
            }
            (
                format!("recovery ended {:?}", recovered.ended),
                Some(recovered.ended),
            )
        }
        None => {
            if door.then_launch.is_none() {
                guard.nobody_waiting();
            }
            let did = if recovers_here {
                String::from("nothing to recover")
            } else {
                String::from("nothing was recovered")
            };
            (did, None)
        }
    };
    let left = guard.leave();
    let code = code_of(
        &left,
        ended.as_ref().map_or(0, update_apply_macos::Ended::code),
    );
    let line = format!(
        "BT_UPDATE_RECOVER {}; {did}; {}{whereabouts}",
        first.state,
        left.said()
    );
    let world = &mut *guard.inner().world;
    world.say(&line);
    if appended.is_some_and(|log| !crate::diagnostics::append_note(log, &line)) {
        world.say(&format!(
            "BT_UPDATE_RECOVER the line above could not be appended to {}",
            log.display()
        ));
    }
    code
}

/// **The Windows door** (U-23, U-24): what a dead applier left, recovered by
/// the applier's own code (`update_apply_windows::recover`, each of its lines
/// kept in the same file as the door's own), then the one line, naming the
/// header as it stands now, and exactly the start the recovery owes
/// (`update_apply::Opens`) with the handed command line after its words.
pub(crate) fn run_windows(
    worker: &WorkerCtx,
    road: &crate::update_apply_windows::Road,
    then_launch: Option<&[OsString]>,
    world: &mut impl crate::update_apply_windows::World,
) -> i32 {
    // Every way out, a panic included, leaves through the exit guard (U-34),
    // made first.
    let mut guard = ExitGuard::new(crate::update_apply_windows::WindowsLeave {
        road,
        world,
        handed: then_launch.unwrap_or(&[]),
        worker: Some(worker),
    });
    let (log, whereabouts) = log_file(&road.home, &road.data);
    let appended = appended_to(&log);
    let recovered = crate::update_apply_windows::recover(
        worker,
        road,
        &mut LoggedWindows {
            world: &mut *guard.inner().world,
            log: appended,
        },
        then_launch,
    );
    guard.succeeded_by(recovered.successor);
    if !recovered.waiting {
        guard.nobody_waiting();
    }
    if recovered.ended.deferred_to_a_holder() {
        guard.window_elsewhere();
    }
    let left = guard.leave();
    let now = header_of(&road.home);
    let line = format!(
        "BT_UPDATE_RECOVER {}; recovery ended {:?}; {}{whereabouts}",
        now.state,
        recovered.ended,
        left.said()
    );
    let world = &mut *guard.inner().world;
    world.say(&line);
    if appended.is_some_and(|log| !crate::diagnostics::append_note(log, &line)) {
        world.say(&format!(
            "BT_UPDATE_RECOVER the line above could not be appended to {}",
            log.display()
        ));
    }
    code_of(&left, recovered.ended.code())
}

/// **A Windows world whose every line is also appended to the log** — the
/// recovery's lines, which the run at logon would otherwise lose.
struct LoggedWindows<'w, W> {
    world: &'w mut W,
    /// `None` when the world's standard error is the log already
    /// ([`appended_to`]).
    log: Option<&'w Path>,
}

impl<W: crate::update_apply_windows::World> crate::update_apply_windows::World
    for LoggedWindows<'_, W>
{
    fn say(&mut self, line: &str) {
        self.world.say(line);
        if let Some(log) = self.log {
            let _ = crate::diagnostics::append_note(log, line);
        }
    }

    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        self.world.spawn_detached(program, args)
    }

    fn arm(
        &mut self,
        txn: crate::update_txn::TxnId,
        rescue: &Path,
    ) -> Result<bt_platform::install_txn::Armed, String> {
        self.world.arm(txn, rescue)
    }

    fn disarm(&mut self, txn: crate::update_txn::TxnId) -> Result<(), String> {
        self.world.disarm(txn)
    }

    fn is_armed(&mut self, txn: crate::update_txn::TxnId) -> Result<bool, String> {
        self.world.is_armed(txn)
    }

    fn launch_trial(&mut self, program: &Path, args: &[OsString]) -> io::Result<u32> {
        self.world.launch_trial(program, args)
    }

    fn moved(&mut self, done: &crate::update_txn::Move) {
        self.world.moved(done);
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        self.world.acknowledged(worker, data)
    }

    fn show_here(&mut self, text: &str) {
        self.world.show_here(text);
    }
}

/// **A world whose every line is also appended to the log** — the
/// rollback's lines, which the run at logon would otherwise lose.
struct Logged<'w, W> {
    world: &'w mut W,
    /// `None` when the world's standard error is the log already
    /// ([`appended_to`]).
    log: Option<&'w Path>,
}

impl<W: World> Hands for Logged<'_, W> {
    fn say(&mut self, line: &str) {
        self.world.say(line);
        if let Some(log) = self.log {
            let _ = crate::diagnostics::append_note(log, line);
        }
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        self.world.exchange(live, staged)
    }

    fn verify_restored(&mut self, worker: &WorkerCtx, bundle: &Path) -> Result<(), String> {
        self.world.verify_restored(worker, bundle)
    }

    fn launch_trial(
        &mut self,
        bundle: &Path,
        args: &[OsString],
    ) -> io::Result<update_apply_macos::Launch> {
        self.world.launch_trial(bundle, args)
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        self.world.acknowledged(worker, data)
    }

    fn show_here(&mut self, text: &str) {
        self.world.show_here(text);
    }
}

/// **The log the door appends its lines to by name**, or `None` when this
/// process's standard error is that very file (0.4.7 U-42d; 0.4.6's D-11):
/// the recovery a trial's watchdog starts inherits the trial's streams, which
/// are its `diagnostics.log`, so the world's own line is already there and a
/// second append would write it twice. Every other road (the logon, an
/// ordinary start's hand-over, made before its streams are redirected) has a
/// standard error that is not the log, and the append is the line's only way
/// into it.
fn appended_to(log: &Path) -> Option<&Path> {
    (!bt_platform::standard_error_is(log)).then_some(log)
}

/// **Where the one line is kept**: the data directory's `diagnostics.log`
/// when that directory exists, else `recover.log` beside the journal — and the
/// words the line carries to say it went there instead.
pub(crate) fn log_file(home: &Home, data: &Path) -> (PathBuf, String) {
    if data.is_dir() {
        (crate::diagnostics::log_path(data), String::new())
    } else {
        let log = home.root().join("recover.log");
        let said = format!(
            " (no data directory at {}, so this line is in {})",
            data.display(),
            log.display()
        );
        (log, said)
    }
}

/// This process's own world.
struct Machine;

impl Hands for Machine {
    fn say(&mut self, line: &str) {
        bt_platform::write_std_error(format!("{line}\n").as_bytes());
    }

    fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
        bt_platform::install_flip::exchange(live, staged).map_err(|failure| failure.to_string())
    }

    fn verify_restored(&mut self, _worker: &WorkerCtx, bundle: &Path) -> Result<(), String> {
        update_apply_macos::verify_restored_here(bundle)
    }

    fn launch_trial(
        &mut self,
        bundle: &Path,
        args: &[OsString],
    ) -> io::Result<update_apply_macos::Launch> {
        update_apply_macos::open_trial(bundle, args)
    }

    fn acknowledged(&mut self, worker: Option<&WorkerCtx>, data: &Path) -> bool {
        crate::update_apply::claimed_within(worker, data, crate::update_apply::ACKNOWLEDGED_WITHIN)
    }

    fn show_here(&mut self, text: &str) {
        bt_platform::standalone_alert(crate::APP_NAME, text);
    }
}

impl World for Machine {
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
        // `quiet_command` is the one door for a child; the child is dropped at
        // once, never waited on or ended.
        bt_platform::quiet_command(program)
            .args(args)
            .spawn()
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    //! Each test builds an installation in a temporary folder — the installed
    //! program, the home, the rescue build and the journal — and runs the door
    //! over it with the real read. Only the two effects of [`super::World`] are
    //! recorded instead of performed; nothing is started.
    use super::*;
    use crate::update_txn::{Body, Inventories, Journal, Layout, Phase, TxnId};
    use bt_platform::HostPlatform;
    use std::path::PathBuf;

    #[derive(Default)]
    struct Recorded {
        said: Vec<String>,
        spawned: Vec<(PathBuf, Vec<OsString>)>,
        shown: Vec<String>,
    }

    impl Hands for Recorded {
        fn say(&mut self, line: &str) {
            self.said.push(line.to_owned());
        }

        fn exchange(&mut self, live: &Path, staged: &Path) -> Result<(), String> {
            panic!("a Windows home is never exchanged: {live:?} {staged:?}")
        }

        fn verify_restored(&mut self, _: &WorkerCtx, bundle: &Path) -> Result<(), String> {
            panic!("a Windows home is never verified: {bundle:?}")
        }

        fn launch_trial(
            &mut self,
            bundle: &Path,
            _: &[OsString],
        ) -> io::Result<update_apply_macos::Launch> {
            panic!("a Windows home never starts a trial: {bundle:?}")
        }

        fn acknowledged(&mut self, _worker: Option<&WorkerCtx>, _data: &Path) -> bool {
            true
        }

        fn show_here(&mut self, text: &str) {
            self.shown.push(text.to_owned());
        }
    }

    impl World for Recorded {
        fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()> {
            self.spawned.push((program.to_path_buf(), args.to_vec()));
            Ok(())
        }
    }

    /// `install\folio.exe`, `install\.folio-update\<txn>\rescue\folio.exe` and,
    /// when `phase` is given, the journal in that phase.
    fn installation(tag: &str, phase: Option<Phase>) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("bt-update-recover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let txn = TxnId::new([0x7a; 16]);
        let install = root.join("install");
        let rescue = install
            .join(crate::update_txn::WINDOWS_HOME)
            .join(txn.to_string())
            .join("rescue")
            .join("folio.exe");
        std::fs::create_dir_all(rescue.parent().unwrap()).unwrap();
        std::fs::write(install.join("folio.exe"), b"installed").unwrap();
        std::fs::write(&rescue, b"rescue").unwrap();
        if let Some(phase) = phase {
            let journal = Journal {
                txn,
                rescue: rescue.display().to_string(),
                body: Body {
                    phase,
                    layout: Layout::Members(Inventories {
                        old_shipped: Vec::new(),
                        old_present: Vec::new(),
                        new: Vec::new(),
                    }),
                },
            };
            std::fs::write(
                install
                    .join(crate::update_txn::WINDOWS_HOME)
                    .join("journal.json"),
                journal.encode(),
            )
            .unwrap();
        }
        (root, rescue)
    }

    /// **The door on a worker the thread door lends**, over `home` with the
    /// product's limits and no LaunchAgents folder: its exit code and what it
    /// asked of `world`.
    fn run_over(
        home: &Home,
        installed: &Path,
        then_launch: Option<Vec<OsString>>,
        data: &Path,
    ) -> (i32, Recorded) {
        let (home, installed, data) = (home.clone(), installed.to_path_buf(), data.to_path_buf());
        bt_platform::spawn_at_priority(
            "bt-update-recover-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let mut world = Recorded::default();
                let door = Door {
                    home: &home,
                    installed: &installed,
                    then_launch: then_launch.as_deref(),
                    data: &data,
                    agents: None,
                    limits: Limits::PRODUCT,
                    starter: None,
                    handed_back: None,
                };
                let code = run(worker, &door, &mut world);
                (code, world)
            },
        )
        .expect("the thread door starts a thread")
        .join()
        .expect("the door does not panic")
    }

    /// A data directory of the test's own, beside the installation.
    fn data_root(root: &Path) -> PathBuf {
        let data = root.join("roaming").join("Folio");
        std::fs::create_dir_all(&data).unwrap();
        data
    }

    fn handed() -> Vec<OsString> {
        ["--cwd", r"D:\x", "--", "--tab"]
            .into_iter()
            .map(OsString::from)
            .collect()
    }

    /// RED (U-22) — **`--update-recover --then-launch` over a finished
    /// transaction starts the installed Folio with the original arguments,
    /// verbatim, and touches nothing.**
    ///
    /// F-6: "When the transaction is terminal, the helper launches the installed
    /// `folio.exe` with the original arguments." The installed program and the
    /// journal are both found from the rescue build's own path.
    ///
    /// MUTATION: in `run`, start `installed` with `argv` minus its first
    /// argument (or not at all).
    #[test]
    fn a_terminal_journal_relaunches_the_installed_folio_with_the_original_arguments() {
        let (root, rescue) = installation("terminal", Some(Phase::Abandoned));
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        assert_eq!(installed, root.join("install").join("folio.exe"));
        let journal = std::fs::read(home.journal()).unwrap();
        let data = data_root(&root);
        let (code, world) = run_over(&home, &installed, Some(handed()), &data);
        assert_eq!(code, 0);
        assert_eq!(world.spawned, vec![(installed.clone(), handed())]);
        assert_eq!(world.said.len(), 1, "{:?}", world.said);
        assert!(world.said[0].starts_with("BT_UPDATE_RECOVER transaction 7a7a"));
        assert_eq!(std::fs::read(home.journal()).unwrap(), journal);
        assert!(rescue.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-22; U-23; U-24; U-34) — **the door over a home no road of its
    /// own recovers still opens a Folio, and never one that hands itself
    /// back**: handed a command line, the installed Folio with
    /// `--update-failed <journal>` first — it continues past the unfinished
    /// header with *Update incomplete.* — exit 0, the journal as it was; with
    /// no command line nobody is waiting and it says one line and exits 0;
    /// with no journal at all the handed line is launched into the installed
    /// Folio.
    ///
    /// A Windows member set is `run_windows`'s (every phase opens a Folio:
    /// `update_apply_windows::tests::every_phase_left_by_a_dead_applier_still_opens_folio`).
    /// U-24 started nothing here; U-34 makes every way out of the door the exit
    /// guard's, which starts what the disk names — `--update-failed` while the
    /// header is `destructive`, so the start never hands itself back.
    ///
    /// MUTATION: in `DoorLeave::opening`, answer `failed: false` for an
    /// unfinished header (the start would hand itself back).
    #[test]
    fn a_destructive_journal_on_a_home_this_door_does_not_recover_opens_with_the_incomplete_card() {
        let (root, rescue) = installation("destructive", Some(Phase::Moving));
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        let journal = std::fs::read(home.journal()).unwrap();
        let data = data_root(&root);
        let (code, world) = run_over(&home, &installed, Some(handed()), &data);
        assert_eq!(code, 0);
        let mut words = crate::update_apply::failed_words(&home).to_vec();
        words.extend(handed());
        assert_eq!(world.spawned, vec![(installed.clone(), words)]);
        assert!(world.said[0].contains("Destructive"), "{:?}", world.said);
        assert_eq!(std::fs::read(home.journal()).unwrap(), journal);

        let (code, world) = run_over(&home, &installed, None, &data);
        assert_eq!(code, 0);
        assert!(world.spawned.is_empty());
        assert_eq!(world.said.len(), 1);
        assert!(
            world.said[0].ends_with("nobody is waiting; nothing was started"),
            "{:?}",
            world.said
        );
        let _ = std::fs::remove_dir_all(&root);

        let (root, rescue) = installation("absent", None);
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        let data = data_root(&root);
        let (code, world) = run_over(&home, &installed, Some(handed()), &data);
        assert_eq!(code, 0);
        assert_eq!(world.spawned, vec![(installed, handed())]);
        assert!(world.said[0].contains("no transaction in"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-42d, review finding 3) — **the recovery door's lines reach
    /// `diagnostics.log` once when its standard error is that same log** (the
    /// recovery a trial's watchdog starts), through both of the door's worlds:
    /// the Windows road's and the macOS road's; and once, by the append, when
    /// its standard error is not the log.
    ///
    /// 0.4.6's D-11 on the door's road: the world wrote the line to standard
    /// error, which was the trial's `diagnostics.log`, and the door appended
    /// the same bytes to that file by name. The child here is a copy of this
    /// test binary whose standard error is the log.
    ///
    /// MUTATION: make `appended_to` answer the log whatever standard error is:
    /// each line is in the file twice.
    #[test]
    fn a_recovery_line_reaches_the_log_once_whatever_standard_error_is() {
        const CHILD: &str = "BT_U42D_RECOVER_SAY";
        if let Some(log) = std::env::var_os(CHILD) {
            let log = PathBuf::from(log);
            let mut windows = crate::update_apply_windows::Machine { log: None };
            crate::update_apply_windows::World::say(
                &mut LoggedWindows {
                    world: &mut windows,
                    log: appended_to(&log),
                },
                "BT_UPDATE_RECOVER the Windows road, once",
            );
            let mut macos = Machine;
            Hands::say(
                &mut Logged {
                    world: &mut macos,
                    log: appended_to(&log),
                },
                "BT_UPDATE_RECOVER the macOS road, once",
            );
            return;
        }
        let text = crate::update_apply_windows::tests::said_by_a_child_whose_stderr_is_the_log(
            "update_recover::tests::a_recovery_line_reaches_the_log_once_whatever_standard_error_is",
            CHILD,
            "recover",
        );
        for line in [
            "BT_UPDATE_RECOVER the Windows road, once",
            "BT_UPDATE_RECOVER the macOS road, once",
        ] {
            assert_eq!(text.matches(line).count(), 1, "{line}: {text}");
        }

        // Standard error elsewhere (this test process's): the append is the
        // line's one way into the log.
        let folder = std::env::temp_dir().join(format!(
            "bt-u42d-recover-{}-{}",
            std::process::id(),
            bt_platform::attention_pipe::unguessable_bits() % 1_000_000
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let log = folder.join("diagnostics.log");
        std::fs::write(&log, b"").unwrap();
        assert_eq!(appended_to(&log), Some(log.as_path()));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// RED (U-42d, round 3) — **a symlinked log name is compared with the
    /// target both standard error and `append_note` actually open.**
    ///
    /// MUTATION: change `standard_error_is` back to `symlink_metadata`; each
    /// recovery line occurs twice.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_log_path_is_the_same_standard_error_target() {
        const CHILD: &str = "BT_U42D_RECOVER_SAY";
        if let Some(log) = std::env::var_os(CHILD) {
            let log = PathBuf::from(log);
            let mut windows = crate::update_apply_windows::Machine { log: None };
            crate::update_apply_windows::World::say(
                &mut LoggedWindows {
                    world: &mut windows,
                    log: appended_to(&log),
                },
                "BT_UPDATE_RECOVER the Windows symlink road, once",
            );
            let mut macos = Machine;
            Hands::say(
                &mut Logged {
                    world: &mut macos,
                    log: appended_to(&log),
                },
                "BT_UPDATE_RECOVER the macOS symlink road, once",
            );
            return;
        }
        let text = said_by_a_child_whose_stderr_is_the_symlinked_log(
            "update_recover::tests::a_symlinked_log_path_is_the_same_standard_error_target",
            CHILD,
        );
        for line in [
            "BT_UPDATE_RECOVER the Windows symlink road, once",
            "BT_UPDATE_RECOVER the macOS symlink road, once",
        ] {
            assert_eq!(text.matches(line).count(), 1, "{line}: {text}");
        }
    }

    /// Run the recovery child with standard error opened through the same
    /// symlink the recovery appends to. `append_note` follows the link, so the
    /// identity comparison must follow it too.
    ///
    /// MUTATION: change `standard_error_is` back to `symlink_metadata`; each
    /// recovery line occurs twice and the caller's count is red.
    #[cfg(unix)]
    fn said_by_a_child_whose_stderr_is_the_symlinked_log(name: &str, child: &str) -> String {
        use std::process::Stdio;

        let folder = std::env::temp_dir().join(format!(
            "bt-u42d-recover-link-{}-{}",
            std::process::id(),
            bt_platform::attention_pipe::unguessable_bits() % 1_000_000
        ));
        std::fs::create_dir_all(&folder).expect("make the log folder");
        let target = folder.join("diagnostics-target.log");
        std::fs::write(&target, b"").expect("make the log target");
        let log = folder.join("diagnostics.log");
        std::os::unix::fs::symlink(&target, &log).expect("link the log name to its target");
        let stream = std::fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .expect("open standard error through the link");
        let status = bt_platform::quiet_command(std::env::current_exe().expect("this test binary"))
            .args(["--exact", name, "--test-threads=1"])
            .env(child, &log)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stream))
            .status()
            .expect("the child runs");
        let text = std::fs::read_to_string(&target).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&folder);
        assert!(status.success(), "the child failed: {text}");
        text
    }

    /// RED (U-22, coordinator ruling 2026-09-27) — **the one line is appended
    /// to the data directory's `diagnostics.log`, beside what is already
    /// there, and to `recover.log` in the home when there is no data
    /// directory — which the line then says.**
    ///
    /// The run the entrance starts at logon has no console: a line only on
    /// stderr is the trace of exactly that run, lost.
    ///
    /// MUTATION: drop the `append_note` call in `run`.
    #[test]
    fn the_recover_line_lands_in_the_diagnostics_log() {
        let (root, rescue) = installation("logged", Some(Phase::Abandoned));
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        let data = data_root(&root);
        let log = crate::diagnostics::log_path(&data);
        std::fs::write(&log, "an earlier run's line\n").unwrap();
        let (code, world) = run_over(&home, &installed, None, &data);
        assert_eq!(code, 0);
        let kept = std::fs::read_to_string(&log).unwrap();
        assert_eq!(kept, format!("an earlier run's line\n{}\n", world.said[0]));
        assert!(world.said[0].starts_with("BT_UPDATE_RECOVER transaction 7a7a"));
        assert!(!home.root().join("recover.log").exists());

        let missing = root.join("nobody").join("Folio");
        let (code, world) = run_over(&home, &installed, None, &missing);
        assert_eq!(code, 0);
        assert_eq!(world.said.len(), 1, "{:?}", world.said);
        assert!(world.said[0].contains("so this line is in"));
        assert_eq!(
            std::fs::read_to_string(home.root().join("recover.log")).unwrap(),
            format!("{}\n", world.said[0])
        );
        assert!(!missing.exists(), "the data directory is never created");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-22) — **only an executable in a home's `rescue` folder is a
    /// rescue build**; the installed `folio.exe` is not one, and neither is any
    /// copy on a platform whose rescue is not this shape yet.
    ///
    /// MUTATION: drop the `rescue` / `.folio-update` name check in
    /// `Home::of_rescue`.
    #[test]
    fn the_rescue_home_is_found_only_from_a_rescue_folder() {
        let install = PathBuf::from("Folio");
        let home_root = install.join(crate::update_txn::WINDOWS_HOME);
        let rescue = home_root.join("7a7a").join("rescue").join("folio.exe");
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        assert_eq!(home.root(), home_root);
        assert_eq!(home.journal(), home_root.join("journal.json"));
        assert_eq!(installed, install.join("folio.exe"));
        assert_eq!(
            Home::of_rescue(HostPlatform::Windows, &install.join("folio.exe")),
            None
        );
        let elsewhere = install
            .join("other")
            .join("7a7a")
            .join("rescue")
            .join("folio.exe");
        assert_eq!(Home::of_rescue(HostPlatform::Windows, &elsewhere), None);
        assert_eq!(Home::of_rescue(HostPlatform::MacOs, &rescue), None);
    }
}
