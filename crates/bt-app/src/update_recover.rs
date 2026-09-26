//! **`folio --update-recover [<home>] [--then-launch <argument>...]`: the rescue
//! build's recovery door** (0.4.6 ticket U-22;
//! `docs/plans/design/self-update-2026-09-16.md` revision (b), F-2 and F-6).
//!
//! Two things start the rescue build this way: the entrance at logon
//! (`bt_platform::logon_hook`, `"<H>\<txn>\rescue\folio.exe" --update-recover`),
//! and an ordinary start that found a transaction it may not continue through
//! (`update_startup`, which adds `--then-launch` and its own command line).
//! On Windows the rescue build finds everything from its own path: the
//! installation home `H` is three folders up, the journal is
//! `H\journal.json`, and the installed program is `<install>\<its own name>`
//! (`update_txn::Home::of_rescue`). On macOS the LaunchAgent names the home
//! (`bt_platform::launch_agent`, `--update-recover <home>`, U-26): the journal
//! is in it, and the installed bundle is the one the home's name is made of
//! (`update_txn::Home::of_rescue_named`).
//!
//! **What it does today, until recovery itself arrives (U-23, U-24).** It
//! reads the journal's frozen header — nothing else, and it moves and removes
//! nothing — and says one diagnostics line, naming the transaction and its
//! class, on its standard error **and** appended to a file, because the run
//! the entrance starts at logon has no console and a line only on stderr would
//! be lost (coordinator ruling, 2026-09-27). The file is the `diagnostics.log`
//! of the data directory (`persist::storage_dir_unmoved`: the installed
//! program's storage rule, without the relocation or creation an ordinary
//! start may do; the log is append-only and outside the trial's write gate).
//! When that directory does not exist, the line goes to `recover.log` beside
//! the journal, in the home, and says so. Then, only when it was handed a
//! command line:
//!
//! * the transaction is no longer one an ordinary start hands over — its
//!   class is `terminal`, `preparing` or `deferred`, or there is no journal,
//!   or one this build cannot read — so the installed Folio is started with
//!   the original arguments, detached, and the door exits 0. The start that
//!   handed itself over is thereby made (U-12's contract), and it cannot come
//!   back here: the ordinary start hands over only a `destructive` class;
//! * the class is `destructive`: the update is not finished and this build
//!   cannot finish it, so the line says so and the door exits 1, starting
//!   nothing — starting the installed Folio would only hand it back here.
//!
//! Headless, like the other argv doors: it runs before the parse and the
//! admission in `fn main`, never opens a window, and never holds the admission
//! (the rescue build is the one process that takes it exclusive, to move
//! files). The read goes through `file_reads` on `Lane::Install`; the start
//! through `bt_platform::quiet_command`, never waited on.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use bt_platform::file_reads::{self, Lane};

use crate::cli;
use crate::update_txn::{Class, Header, Home};

/// **The recovery door's effects**: its one line, and the start of the
/// installed Folio.
pub(crate) trait World {
    /// The one diagnostics line.
    fn say(&mut self, line: &str);
    /// Start `program` with `args`, detached: never waited on.
    fn spawn_detached(&mut self, program: &Path, args: &[OsString]) -> io::Result<()>;
}

/// **The door, for this process**: this executable must be a rescue build.
pub(crate) fn run_here(home: Option<PathBuf>, then_launch: Option<Vec<OsString>>) -> i32 {
    let mut world = Machine;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            world.say(&format!(
                "BT_UPDATE_RECOVER cannot name its own executable: {error}"
            ));
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
        return 2;
    };
    let data = crate::persist::storage_dir_unmoved();
    run(&home, &installed, then_launch.as_deref(), &data, &mut world)
}

/// **The door over any home** — see the module header. Answers the exit
/// code.
pub(crate) fn run(
    home: &Home,
    installed: &Path,
    then_launch: Option<&[OsString]>,
    data: &Path,
    world: &mut impl World,
) -> i32 {
    let journal = home.journal();
    let (state, unfinished) = match file_reads::read(Lane::Install, &journal) {
        Ok(bytes) => match Header::parse(&bytes) {
            Ok(header) => (
                format!(
                    "transaction {} is {:?} in {}",
                    header.txn,
                    header.class,
                    home.root().display()
                ),
                header.class == Class::Destructive,
            ),
            Err(refusal) => (
                format!("{} is left as it is: {refusal}", journal.display()),
                false,
            ),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (
            format!("no transaction in {}", home.root().display()),
            false,
        ),
        Err(error) => (
            format!("{} could not be read: {error}", journal.display()),
            false,
        ),
    };
    let (outcome, code) = match then_launch {
        None => (String::from("nothing was touched"), 0),
        Some(_) if unfinished => (
            String::from(
                "the update is not finished and this build cannot finish it yet; Folio was not started",
            ),
            1,
        ),
        Some(argv) => match world.spawn_detached(installed, argv) {
            Ok(()) => (format!("started {}", installed.display()), 0),
            Err(error) => (
                format!("{} could not be started: {error}", installed.display()),
                1,
            ),
        },
    };
    let (log, whereabouts) = log_file(home, data);
    let line = format!(
        "BT_UPDATE_RECOVER {state}; recovery is not in this build yet; {outcome}{whereabouts}"
    );
    world.say(&line);
    if !crate::diagnostics::append_note(&log, &line) {
        world.say(&format!(
            "BT_UPDATE_RECOVER the line above could not be appended to {}",
            log.display()
        ));
    }
    code
}

/// **Where the one line is kept**: the data directory's `diagnostics.log`
/// when that directory exists, else `recover.log` beside the journal — and the
/// words the line carries to say it went there instead.
fn log_file(home: &Home, data: &Path) -> (PathBuf, String) {
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

impl World for Machine {
    fn say(&mut self, line: &str) {
        bt_platform::write_std_error(format!("{line}\n").as_bytes());
    }

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
    }

    impl World for Recorded {
        fn say(&mut self, line: &str) {
            self.said.push(line.to_owned());
        }

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
        let mut world = Recorded::default();
        assert_eq!(
            run(&home, &installed, Some(&handed()), &data, &mut world),
            0
        );
        assert_eq!(world.spawned, vec![(installed.clone(), handed())]);
        assert_eq!(world.said.len(), 1, "{:?}", world.said);
        assert!(world.said[0].starts_with("BT_UPDATE_RECOVER transaction 7a7a"));
        assert_eq!(std::fs::read(home.journal()).unwrap(), journal);
        assert!(rescue.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-22) — **an unfinished transaction is never relaunched into**:
    /// the installed Folio would only hand itself back. The door says so in
    /// one line and exits 1; with no command line to launch it says one line
    /// and exits 0; with no journal at all the handed line is launched.
    ///
    /// MUTATION: drop the `unfinished` arm in `run`.
    #[test]
    fn a_destructive_journal_starts_nothing_and_says_so() {
        let (root, rescue) = installation("destructive", Some(Phase::Moving));
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        let data = data_root(&root);
        let mut world = Recorded::default();
        assert_eq!(
            run(&home, &installed, Some(&handed()), &data, &mut world),
            1
        );
        assert!(world.spawned.is_empty());
        assert!(world.said[0].contains("Destructive"), "{:?}", world.said);
        assert!(world.said[0].contains("Folio was not started"));

        let mut world = Recorded::default();
        assert_eq!(run(&home, &installed, None, &data, &mut world), 0);
        assert!(world.spawned.is_empty());
        assert_eq!(world.said.len(), 1);
        assert!(world.said[0].ends_with("nothing was touched"));
        let _ = std::fs::remove_dir_all(&root);

        let (root, rescue) = installation("absent", None);
        let (home, installed) = Home::of_rescue(HostPlatform::Windows, &rescue).unwrap();
        let data = data_root(&root);
        let mut world = Recorded::default();
        assert_eq!(
            run(&home, &installed, Some(&handed()), &data, &mut world),
            0
        );
        assert_eq!(world.spawned, vec![(installed, handed())]);
        assert!(world.said[0].contains("no transaction in"));
        let _ = std::fs::remove_dir_all(&root);
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
        let mut world = Recorded::default();
        assert_eq!(run(&home, &installed, None, &data, &mut world), 0);
        let kept = std::fs::read_to_string(&log).unwrap();
        assert_eq!(kept, format!("an earlier run's line\n{}\n", world.said[0]));
        assert!(world.said[0].starts_with("BT_UPDATE_RECOVER transaction 7a7a"));
        assert!(!home.root().join("recover.log").exists());

        let missing = root.join("nobody").join("Folio");
        let mut world = Recorded::default();
        assert_eq!(run(&home, &installed, None, &missing, &mut world), 0);
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
