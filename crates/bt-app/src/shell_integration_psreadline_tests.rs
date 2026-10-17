//! **Folio's PSReadLine in a Windows PowerShell 5.1 pane, and the decisions that put it there**
//! (T-INTEGRATION-INJECT-2-BUILD; the design note
//! `docs/plans/design/integration-inject-2-2026-10-15.md` §5.1).
//!
//! Two halves. The decisions — the module path's composition, the module's writer and the
//! retirement of other builds' artefacts, and when a pane is given the prefix at all — are pure or
//! over the test's own directories and hold on every platform. The real panes are Windows-only by
//! nature ([`real_shell`]): Windows PowerShell 5.1 and its module path are Windows facts, and the
//! executable carries the module's bytes for a Windows target only (`psreadline::module_files`,
//! empty elsewhere, which `psreadline`'s own test pins on every platform).

use super::*;
use std::cell::Cell;

/// The tests that publish or read Windows PowerShell's edition observation take this turn, as the
/// Profiles-row tests do (`tests::WINDOWS_POWERSHELL_OBSERVATIONS`): a pane's prefix reads it.
fn observation_turn() -> std::sync::MutexGuard<'static, ()> {
    super::tests::WINDOWS_POWERSHELL_OBSERVATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Paths spelled the hard ways a real account spells them: a space, Chinese, and each of the five
/// quote characters PowerShell reads as a single quote.
const AWKWARD: &str = "Folio data 数据 '‘’‚‛";

fn os(text: &str) -> OsString {
    OsString::from(text)
}

/// RED (T-INTEGRATION-INJECT-2-BUILD) — **a 5.1 pane's module path is Folio's directory first,
/// the Documents entry only where 5.1 would have added it, and the pane's own path after, entry
/// for entry, with nothing twice** (the note's §2.3, spike Follow-up C).
///
/// Each case is one clause of the rule: the machine's own value inherited (5.1 adds Documents, so
/// does this); any other value (5.1 keeps it as it is, so does this); nothing inherited (5.1
/// composes Documents and the machine's value); a path already naming Folio's directory, in
/// another case and with a trailing separator (not twice); a path already naming Documents (not
/// twice); the account's block with `%ProgramFiles%` unexpanded against the machine's expanded
/// value (equal once expanded, as 5.1 compares them); a reference to a variable nobody set (left
/// as written); no machine value and nothing inherited (nothing that would still find the
/// system's modules — no prefix); and a Folio directory holding the separator (not an entry).
/// Every path is spelled with a space, Chinese and the five quote characters.
///
/// MUTATIONS: add the Documents entry whatever was inherited (drop `defaulted`) — the second case
/// names it; compare the raw texts (no expansion) — the `%ProgramFiles%` case names it.
#[test]
fn the_module_path_is_folios_first_then_what_5_1_would_have_had() {
    let root = PathBuf::from(format!(r"C:\Users\alice\AppData\Local\{AWKWARD}"));
    let folio = root.join(r"psmodules\cb9390e97332");
    let documents = PathBuf::from(format!(r"D:\{AWKWARD}\Documents"));
    let personal = documents.join("WindowsPowerShell").join("Modules");
    let machine = os(
        r"C:\Program Files\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules",
    );
    let f = folio.display().to_string();
    let p = personal.display().to_string();
    let m = machine.to_string_lossy().into_owned();
    let program_files = format!(r"C:\Program Files {AWKWARD}");
    let environment = vec![(os("ProgramFiles"), os(&program_files))];
    let compose = |value: Option<&str>, machine: Option<&OsStr>| {
        compose_module_path(
            &folio,
            Some(&documents),
            value.map(OsStr::new),
            machine,
            &environment,
        )
        .map(|path| path.to_string_lossy().into_owned())
    };

    assert_eq!(
        compose(Some(&m), Some(&machine)),
        Some(format!("{f};{p};{m}")),
        "the machine's own value: Documents is added, as 5.1 adds it"
    );
    let chosen = format!(r"E:\{AWKWARD}\modules;{m}");
    assert_eq!(
        compose(Some(&chosen), Some(&machine)),
        Some(format!("{f};{chosen}")),
        "any other value is kept as it is, and nothing is added to it"
    );
    assert_eq!(
        compose(None, Some(&machine)),
        Some(format!("{f};{p};{m}")),
        "nothing inherited: what 5.1 itself composes, after Folio's"
    );
    let twice = format!(
        "{};{m}",
        f.to_uppercase().replace("PSMODULES", "psmodules") + "\\"
    );
    assert_eq!(
        compose(Some(&twice), Some(&machine)),
        Some(format!("{f};{m}")),
        "an entry already naming Folio's directory is not kept twice"
    );
    let with_documents = format!("{m};{p}");
    assert_eq!(
        compose(
            Some(&with_documents),
            Some(&OsString::from(&with_documents))
        ),
        Some(format!("{f};{with_documents}")),
        "Documents already on the path is not added again"
    );
    // The account's fresh block as Windows builds it: `%ProgramFiles%` left unexpanded beside an
    // expanded system root, against the machine's value as the registry answers it, expanded.
    let unexpanded = r"%PROGRAMFILES%\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules";
    let expanded_machine = os(&format!(
        r"{program_files}\WindowsPowerShell\Modules;C:\WINDOWS\system32\WindowsPowerShell\v1.0\Modules"
    ));
    assert_eq!(
        compose(Some(unexpanded), Some(&expanded_machine)),
        Some(format!("{f};{p};{unexpanded}")),
        "the two are compared expanded, as 5.1 compares them, and the pane's own text is kept"
    );
    assert_eq!(
        compose(Some(r"%NOT_SET%\Modules"), Some(&os(r"%NOT_SET%\Modules"))),
        Some(format!(r"{f};{p};%NOT_SET%\Modules")),
        "a reference to nothing is left as written, on both sides"
    );
    assert_eq!(
        compose(None, None),
        None,
        "nothing to compose a working path from"
    );
    assert_eq!(
        compose_module_path(
            Path::new(r"C:\Fo;lio"),
            Some(&documents),
            Some(machine.as_os_str()),
            Some(&machine),
            &environment,
        ),
        None,
        "a directory holding the separator is not an entry"
    );
}

/// The facts a declaration is handed, for a module at `module` and nothing else of the machine.
fn facts_with(
    module: Option<PathBuf>,
    asked: &Cell<bool>,
) -> impl FnOnce() -> ModulePathFacts + '_ {
    move || {
        asked.set(true);
        ModulePathFacts {
            module,
            documents: None,
            machine: Some(os(r"C:\Program Files\WindowsPowerShell\Modules")),
        }
    }
}

fn declared(profile: &[(OsString, OsString)]) -> Vec<String> {
    profile
        .iter()
        .filter(|(name, _)| environment_name_eq(name, OsStr::new(MODULE_PATH)))
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .collect()
}

/// RED (T-INTEGRATION-INJECT-2-BUILD) — **the prefix and the loader are one decision**: a Windows
/// PowerShell pane that got the loader is declared Folio's module path, on top of the row's own
/// `PSModulePath`, once; no other pane is declared anything, and the machine is not even asked
/// (the note's §2.3, §5.1 "no prefix when the loader is not composed").
///
/// No prefix for: a pane the composer did not load into (`loads` false: the switch off, `-File`,
/// `-NonInteractive`, an argv it refused), pwsh 7 (its own folders come first and its in-box
/// editor is already 2.4.5), a program that is not a PowerShell, an edition observed in
/// Constrained Language Mode (the loader does nothing there), and an executable that carries no
/// module — any target but Windows, which is the macOS twin of the real panes below: the
/// composition adds nothing to any pane's environment.
///
/// MUTATION: drop `observed_constrained` from `declare_psreadline_module`'s guard — the
/// Constrained case is declared a prefix.
#[test]
fn a_prefix_is_declared_only_where_the_loader_is_and_once() {
    let _turn = observation_turn();
    let module = PathBuf::from(format!(r"C:\Local\{AWKWARD}\psmodules\cb9390e97332"));
    let row_path = os(r"E:\row\modules");
    let before = vec![(
        os("PSModulePath"),
        os(r"C:\Program Files\WindowsPowerShell\Modules"),
    )];
    let row_layer = vec![
        (os("psmodulepath"), row_path.clone()),
        (os("OTHER"), os("1")),
    ];

    let asked = Cell::new(false);
    let mut profile = row_layer.clone();
    declare_psreadline_module(
        Path::new("powershell.exe"),
        true,
        &before,
        &[],
        &mut profile,
        facts_with(Some(module.clone()), &asked),
    );
    assert!(asked.get());
    assert_eq!(
        declared(&profile),
        [format!(
            "{};{}",
            module.display(),
            row_path.to_string_lossy()
        )],
        "one declaration: Folio's directory, then the row's own value"
    );
    assert!(
        profile.iter().any(|(name, _)| name == "OTHER"),
        "the row's other rows stand"
    );

    for (program, loads, module, why) in [
        (
            "powershell.exe",
            false,
            Some(module.clone()),
            "the loader was not composed",
        ),
        ("pwsh.exe", true, Some(module.clone()), "PowerShell 7"),
        ("cmd.exe", true, Some(module.clone()), "not a PowerShell"),
        (
            "powershell.exe",
            true,
            None,
            "no module to offer (any target but Windows)",
        ),
    ] {
        let asked = Cell::new(false);
        let mut profile = row_layer.clone();
        declare_psreadline_module(
            Path::new(program),
            loads,
            &before,
            &[],
            &mut profile,
            facts_with(module, &asked),
        );
        assert_eq!(profile, row_layer, "{why}: the row's layer is untouched");
        if !loads || program != "powershell.exe" {
            assert!(!asked.get(), "{why}: the machine is not asked");
        }
    }

    let program = Path::new("powershell.exe");
    let constrained = ProfileObservation {
        full_language: false,
        path: PathBuf::from("profile.ps1"),
        scopes: super::tests::scopes(
            crate::psreadline::ExecutionPolicy::Undefined,
            crate::psreadline::ExecutionPolicy::Undefined,
            crate::psreadline::ExecutionPolicy::Undefined,
            crate::psreadline::ExecutionPolicy::Undefined,
            crate::psreadline::ExecutionPolicy::Undefined,
        ),
        edition_says: Some(true),
        location: ProfileLocation {
            local_fixed: true,
            marked: false,
        },
        ordinary_process: None,
        seen: ProfileRevision::default(),
    };
    publish_profile_observation(program, next_observation(), constrained);
    let asked = Cell::new(false);
    let mut profile = row_layer.clone();
    declare_psreadline_module(
        program,
        true,
        &before,
        &[],
        &mut profile,
        facts_with(Some(module), &asked),
    );
    PROFILE_OBSERVATIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&powershell_edition(program));
    assert_eq!(profile, row_layer, "Constrained Language Mode: no prefix");
    assert!(!asked.get(), "and the machine is not asked");
}

/// RED (T-INTEGRATION-INJECT-2-BUILD) — **the writer lays the module down whole, the manifest
/// last, writes nothing a second time, and repairs a file that differs** (the note's §2.2: the
/// script writer's compare-and-repair, the atomic replace, the INJECT follow-up F4 lesson).
///
/// Over the test's own directory and a module this test hands in, on every platform: the files
/// land under `<psmodules>\<artefact>\`, a second preparation finds them current and leaves every
/// one untouched, a file truncated since is put right, and a write that cannot land (the artefact
/// path is a file) is an error the caller says and the next birth retries — nothing is kept.
///
/// MUTATION: write without comparing (always `atomic_replace_preserving`) — the untouched file's
/// modification time moves.
#[test]
fn the_module_writer_lays_it_down_once_and_repairs_what_differs() {
    let root = bt_testpath::temp_path("psreadline-module-writer");
    let files: [(&str, &[u8]); 3] = [
        (
            "PSReadLine/2.4.5/Microsoft.PowerShell.PSReadLine.dll",
            b"MZ assembly",
        ),
        ("PSReadLine/2.4.5/netstd/Polyfiller.dll", b"MZ polyfiller"),
        (
            "PSReadLine/2.4.5/PSReadLine.psd1",
            b"@{ ModuleVersion = '2.4.5' }",
        ),
    ];
    let psmodules = root.join(format!("{AWKWARD} psmodules"));
    let artefact = install_psreadline_files_at(&psmodules, "cb9390e97332", &files)
        .expect("the module is written");
    assert_eq!(artefact, psmodules.join("cb9390e97332"));
    for (name, bytes) in files {
        assert_eq!(std::fs::read(artefact.join(name)).unwrap(), bytes, "{name}");
    }
    let manifest = artefact.join(files[2].0);
    let written_at = std::fs::metadata(&manifest).unwrap().modified().unwrap();

    let dll = artefact.join(files[0].0);
    std::fs::write(&dll, b"MZ").unwrap();
    install_psreadline_files_at(&psmodules, "cb9390e97332", &files).expect("repaired");
    assert_eq!(
        std::fs::read(&dll).unwrap(),
        files[0].1,
        "a file that differs is repaired"
    );
    assert_eq!(
        std::fs::metadata(&manifest).unwrap().modified().unwrap(),
        written_at,
        "a file that is current is not written again"
    );

    let blocked = root.join("blocked");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(
        blocked.join("cb9390e97332"),
        b"a file where the directory goes",
    )
    .unwrap();
    assert!(
        install_psreadline_files_at(&blocked, "cb9390e97332", &files).is_err(),
        "a write that cannot land is an error for the caller to say"
    );
    assert!(
        install_psreadline_files_at(&blocked, "cb9390e97332", &[]).is_err(),
        "and an executable with no module writes nothing"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// RED (T-INTEGRATION-INJECT-2-BUILD) — **the artefacts other builds left are removed, this
/// build's is kept, and one whose assembly a shell holds is refused whole before a byte of it
/// goes** (the note's §2.2: prune at launch, kept when held, tried again at the next).
///
/// A held assembly is stood in for by a deletion the test refuses, which is what Windows answers
/// for a mapped image (measured on a real 5.1 pane by `real_shell::a_loaded_artefact_is_kept_until_its_shell_ends`);
/// the artefact is then left exactly as it was, every file in it. An artefact with two version
/// directories loses both assemblies before its directory goes, and a file beside the artefacts
/// is not this function's.
///
/// MUTATION: remove the directory without trying the assembly first — the held artefact loses its
/// files.
#[test]
fn other_builds_artefacts_are_pruned_and_a_held_one_is_kept_whole() {
    let psmodules = bt_testpath::temp_path("psreadline-prune");
    let lay = |artefact: &str, version: &str| {
        let leaf = psmodules.join(artefact).join("PSReadLine").join(version);
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(leaf.join(crate::psreadline::ASSEMBLY), artefact).unwrap();
        std::fs::write(leaf.join("PSReadLine.psd1"), version).unwrap();
    };
    lay("cb9390e97332", "2.4.5");
    lay("0123456789ab", "2.4.5");
    lay("0123456789ab", "2.4.6");
    lay("fedcba987654", "2.4.5");
    std::fs::write(psmodules.join("note.txt"), b"not an artefact").unwrap();
    let held = psmodules
        .join("fedcba987654")
        .join("PSReadLine")
        .join("2.4.5")
        .join(crate::psreadline::ASSEMBLY);
    let kept = prune_psreadline_artefacts(
        &psmodules,
        "cb9390e97332",
        &crate::psreadline::directory_entries,
        &|path| {
            if path == held {
                Err(std::io::Error::other(
                    "the file is mapped by a running process",
                ))
            } else {
                std::fs::remove_file(path)
            }
        },
    );
    assert_eq!(
        kept,
        [(
            "fedcba987654".to_owned(),
            "the file is mapped by a running process".to_owned()
        )]
    );
    let mut left: Vec<String> = std::fs::read_dir(&psmodules)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["cb9390e97332", "fedcba987654", "note.txt"]);
    for file in [held.clone(), held.with_file_name("PSReadLine.psd1")] {
        assert!(
            file.is_file(),
            "the held artefact is whole: {}",
            file.display()
        );
    }
    let _ = std::fs::remove_dir_all(&psmodules);
}

/// RED (T-INTEGRATION-INJECT-2-BUILD) — **an update's trial reads the durable module and never
/// writes it** (`update_trial`, F-7, the script's rule): a durable artefact that holds this
/// build's bytes is named as it is; one that does not is left exactly as the old build left it and
/// the trial's own folder gets the module.
///
/// MUTATION: write the durable copy from the trial (`install_psreadline_module_at(durable)`) —
/// the stale durable file is rewritten.
#[test]
fn a_trial_names_a_current_durable_module_and_otherwise_writes_its_own() {
    if crate::psreadline::module_files().is_empty() {
        // No module to compare: nothing a trial could name or write (any target but Windows).
        return;
    }
    let root = bt_testpath::temp_path("psreadline-trial");
    let durable = root.join("durable");
    let trial = root.join("trial");
    install_psreadline_module_at(&durable).expect("the durable copy");
    assert_eq!(
        psreadline_module_in_trial(&durable, &trial).unwrap(),
        durable.join(crate::psreadline::ARTEFACT),
        "a current durable copy is named"
    );
    assert!(!trial.exists(), "and nothing is written for the trial");

    let (name, _) = crate::psreadline::module_files()[0];
    let stale = durable.join(crate::psreadline::ARTEFACT).join(name);
    std::fs::write(&stale, b"the old build's").unwrap();
    assert_eq!(
        psreadline_module_in_trial(&durable, &trial).unwrap(),
        trial.join(crate::psreadline::ARTEFACT),
        "a stale durable copy is not the trial's to name"
    );
    assert_eq!(
        std::fs::read(&stale).unwrap(),
        b"the old build's",
        "and not the trial's to write"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// **The real panes**: Windows PowerShell 5.1 on a pseudoconsole, each made the way a birth makes
/// one — the module written by the product's writer ([`install_psreadline_module_at`]) into the
/// test's own directory, the argv composed by the product's composer, the module path by
/// [`declare_psreadline_module`], the script by [`install_script_at`], and a resize committed
/// through the product's own resize door (`commit_leaf_resize`) with the anchor chord paid the way
/// `Runtime::finish_resize_if_quiescent` pays it. The shell is started through `bt_pty::test_shell`
/// — `-NoProfile`, a temporary HOME, USERPROFILE, APPDATA and LOCALAPPDATA, and
/// `HistorySaveStyle SaveNothing` read back before a byte is typed — and writes only under that
/// test's own directory. The account's Documents is never named (the composer is handed none), so
/// nothing the account installed can stand in for the module under test.
#[cfg(windows)]
mod real_shell {
    use super::*;
    use crate::nonzero_u32;
    use crate::test_support::{grid_of, on_the_window_thread};
    use bt_pty::test_shell::{Hygiene, TestShell};
    use bt_term::DualPlaneSession;
    use std::time::{Duration, Instant};

    /// How long a child may say nothing before a wait for it is called stalled; the ceiling stops a
    /// child that talks for ever without reaching what is awaited. A count of silence, never a total
    /// (`docs/CONVENTIONS.md` §三): a loaded machine delivers the same bytes further apart.
    const SILENCE_BUDGET: Duration = Duration::from_secs(30);
    const CEILING: Duration = Duration::from_secs(240);
    /// The quiet a settled screen is judged after.
    const QUIET: Duration = Duration::from_millis(900);

    /// The prompt the resize arms install: forty-four cells, so it already wraps at thirty-eight.
    const PROMPT: &str = "BTINVOKE 012345678901234567890123456789012> ";
    /// What is typed at it: longer than any width the chain passes through.
    const TYPED: &str = "Write-Output 012345678901234567890123456789012345678901234567890123456789";

    /// One Windows PowerShell pane, and the screen this side keeps of it.
    struct Pane {
        pty: TestShell,
        session: DualPlaneSession,
        raw: Vec<u8>,
        grid: (u16, u16),
        /// The module directory the pane's path was composed with.
        module: PathBuf,
        /// `LeafState::pending_psreadline_resize_reanchor`, for this one pane.
        reanchor_debt: bool,
        started: Instant,
        last_output: Instant,
    }

    /// Windows PowerShell 5.1, by the path the machine gives it.
    fn windows_powershell() -> PathBuf {
        bt_platform::program_on_path(Path::new("powershell.exe"))
            .expect("Windows PowerShell is on every supported Windows")
    }

    /// A 5.1 pane composed the product's way from `row`, `columns` by `rows`. `command_row` says the
    /// row carries its own `-Command` text, whose parse answer is handed in as true (the startup's
    /// parser question is `real_command_rows_integrate_without_touching_account_state`'s subject).
    fn folio_pane(row: &[&str], command_row: bool, columns: u16, rows: u16) -> Pane {
        let program = windows_powershell();
        let hygiene = Hygiene::new();
        let module =
            install_psreadline_module_at(&hygiene.root().join("local-data").join("psmodules"))
                .expect("the product's writer lays the module down in the test's directory");
        let script = install_script_at(
            &hygiene.root().join(SCRIPT_DIRECTORY),
            SCRIPT_FILE_PS1,
            SCRIPT_PS1,
        )
        .expect("the product's writer lays folio.ps1 down in the test's directory");
        let row: Vec<OsString> = row.iter().map(OsString::from).collect();
        let composed = if command_row {
            ComposedBirth {
                arguments: composed_powershell_arguments(&program, &row, &script, Some(true))
                    .expect("the row's command composes"),
                loads: true,
            }
        } else {
            compose_with_prepared_and_retry(&program, &row, true, || Some(script.clone()), |_| {})
        };
        assert!(
            composed.loads,
            "the pane gets Folio's loader: {:?}",
            composed.arguments
        );

        // The module path the test shell gives a Windows PowerShell — the account's fresh one — is
        // what the pane would otherwise have, as the birth's environment would have it.
        let before_folio: Vec<(OsString, OsString)> = hygiene
            .command(&program, bt_platform::quiet_command)
            .get_envs()
            .filter(|(name, _)| environment_name_eq(name, OsStr::new(MODULE_PATH)))
            .filter_map(|(name, value)| Some((name.to_owned(), value?.to_owned())))
            .collect();
        let mut profile = Vec::new();
        declare_psreadline_module(
            &program,
            composed.loads,
            &before_folio,
            &[],
            &mut profile,
            || ModulePathFacts {
                module: Some(module.clone()),
                documents: None,
                machine: bt_platform::environment::machine_variable(MODULE_PATH),
            },
        );
        let declared = profile
            .iter()
            .find(|(name, _)| environment_name_eq(name, OsStr::new(MODULE_PATH)))
            .map(|(_, value)| value.to_string_lossy().into_owned())
            .expect("a composed 5.1 pane declares its module path");
        assert!(
            declared.starts_with(&*module.to_string_lossy()),
            "Folio's directory is first: {declared}"
        );

        crate::test_support::install_host_answers();
        let home = hygiene.home();
        let hygiene = hygiene.with_powershell_shape(super::tests::test_shell_shape(
            &program,
            &composed.arguments,
        ));
        let pty = TestShell::spawn_shell_in(
            hygiene,
            program.clone(),
            &composed.arguments,
            &|| last_resort_arguments(false),
            &profile,
            bt_pty::PtySize::cells(
                std::num::NonZeroU16::new(columns).unwrap(),
                std::num::NonZeroU16::new(rows).unwrap(),
            ),
            Some(home),
        )
        .expect("Windows PowerShell starts on a pseudoconsole");
        Pane {
            pty,
            session: DualPlaneSession::new(nonzero_u32(columns), nonzero_u32(rows)),
            raw: Vec::new(),
            grid: (columns, rows),
            module,
            reanchor_debt: false,
            started: Instant::now(),
            last_output: Instant::now(),
        }
    }

    impl Pane {
        /// One turn: what the child wrote, fed to this side's session, and the session's answers
        /// (the cursor reports the pseudoconsole asks for) written back.
        fn pump(&mut self) -> bool {
            let bytes = self.pty.read_output();
            if bytes.is_empty() {
                return false;
            }
            let now = Instant::now();
            self.last_output = now;
            self.raw.extend_from_slice(&bytes);
            self.session.feed_at(&bytes, now).unwrap();
            for reply in self.session.take_pty_writes() {
                self.pty.reply(&reply).unwrap();
            }
            true
        }

        fn give_up_if_stalled(&mut self, waiting_for: &str) {
            if self.last_output.elapsed() < SILENCE_BUDGET && self.started.elapsed() < CEILING {
                return;
            }
            panic!(
                "gave up waiting for {waiting_for} after {:?}, the last {:?} of it silent, {} bytes \
             read; {}; screen {:?}",
                self.started.elapsed(),
                self.last_output.elapsed(),
                self.raw.len(),
                self.pty.account(),
                self.rows()
            );
        }

        /// Pump until `done` holds of this pane.
        fn until(&mut self, waiting_for: &str, done: impl Fn(&Self) -> bool) {
            loop {
                if !self.pump() {
                    if done(self) {
                        return;
                    }
                    self.give_up_if_stalled(waiting_for);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        /// [`Self::until`] for a measurement that records what happens rather than asserting it:
        /// `false`, and no panic, once the child has been silent for `silence` without `done`.
        fn reached_within(&mut self, silence: Duration, done: impl Fn(&Self) -> bool) -> bool {
            let mut quiet_since = Instant::now();
            loop {
                if self.pump() {
                    quiet_since = Instant::now();
                } else if done(self) {
                    return true;
                } else if quiet_since.elapsed() >= silence {
                    return false;
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        /// Pump until the child has said nothing for [`QUIET`].
        fn settle(&mut self) {
            let mut quiet_since = Instant::now();
            loop {
                if self.pump() {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() >= QUIET {
                    return;
                } else {
                    self.give_up_if_stalled("the child to fall quiet");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        fn rows(&self) -> Vec<String> {
            self.session.terminal().visible_text()
        }

        fn output_since(&self, mark: usize) -> String {
            String::from_utf8_lossy(&self.raw[mark..]).into_owned()
        }

        /// Type `line` and Enter, and pump until `needle` has arrived since and the next prompt has
        /// opened its input after it; answers everything the child wrote since the line was typed.
        fn run(&mut self, line: &str, needle: &str) -> String {
            let mark = self.raw.len();
            self.pty.write(format!("{line}\r").as_bytes()).unwrap();
            self.until(
                &format!("{needle:?} and the next prompt after {line:?}"),
                |pane| {
                    let since = pane.output_since(mark);
                    since
                        .find(needle)
                        .is_some_and(|at| since[at..].contains("\x1b]133;B"))
                },
            );
            self.settle();
            self.output_since(mark)
        }

        /// **One resize through the product's own door**: the reflow, the pseudoconsole told and the
        /// reconcile (`commit_leaf_resize`), then — once the transaction is quiet — the anchor chord
        /// paid exactly as `Runtime::finish_resize_if_quiescent` pays it, to the door `integration`
        /// names. `Integration::None` is the pane that is owed no chord.
        fn resize(&mut self, columns: u16, integration: profiles::Integration) -> usize {
            let (local, rows) = self.grid;
            crate::commit_leaf_resize(
                &mut self.session,
                Some(self.pty.session_mut()),
                crate::ResizeReanchor {
                    pending: &mut self.reanchor_debt,
                    integration,
                },
                crate::ReleaseGrids {
                    local: grid_of(local, rows),
                    conpty: grid_of(local, rows),
                    next: grid_of(columns, rows),
                },
                winit::dpi::PhysicalSize::new(u32::from(columns) * 8, u32::from(rows) * 16),
                Instant::now(),
            )
            .expect("the resize commits");
            self.grid = (columns, rows);
            let mut chords = 0;
            loop {
                self.pump();
                if self
                    .session
                    .finish_resize_if_quiescent(Instant::now())
                    .unwrap()
                {
                    let opened = self.session.shell_prompt_opened_in_order();
                    if let Some(chord) = crate::take_psreadline_resize_reanchor_input(
                        crate::ResizeReanchor {
                            pending: &mut self.reanchor_debt,
                            integration,
                        },
                        opened,
                    ) {
                        self.pty.write(chord).unwrap();
                        chords += 1;
                    }
                    break;
                }
                self.give_up_if_stalled("the resize transaction to go quiet");
                std::thread::sleep(Duration::from_millis(5));
            }
            self.settle();
            chords
        }

        /// The input line as the screen holds it: from the last row that opens with the prompt's
        /// head to the cursor's row, joined, as a wrapped line reads.
        fn input_line(&self) -> String {
            let rows = self.rows();
            let columns = usize::from(self.grid.0);
            let opening = &PROMPT[..columns.min(PROMPT.len())];
            let cursor = (self.session.terminal().cursor().row as usize).min(rows.len() - 1);
            (0..=cursor)
                .rev()
                .find(|row| rows[*row].starts_with(opening))
                .map(|start| rows[start..=cursor].concat().trim_end().to_owned())
                .unwrap_or_default()
        }

        fn prompt_copies(&self) -> usize {
            self.rows().concat().matches("BTINVOKE ").count()
        }

        fn close(mut self) {
            let _ = self.pty.write(b"exit\r");
            let _ = self.pty.shutdown();
        }
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **a composed 5.1 pane loads Folio's 2.4.5 from Folio's
    /// directory, silently, under Restricted, AllSigned and RemoteSigned alike** (the spike's Q7,
    /// re-proven here through the product's writer and composer).
    ///
    /// The module is binary-only: no `.psm1`, no `.ps1xml`, a manifest whose root is the assembly,
    /// none of which an execution policy governs — so no policy prints a line before the first prompt
    /// and none stops at the untrusted-publisher prompt. The loader defines `PSConsoleHostReadLine`
    /// from the module's assembly (the function upstream's `.psm1` would have defined), so the pane
    /// edits with PSReadLine and not the host's legacy line editor. Then the integration is whole:
    /// `133;A` and `133;B` at the first prompt, a first `$?` of `True`, and `C`, `D;0`, `A`, `B`
    /// around the command after it.
    ///
    /// MUTATION: drop the prefix from the composition (`declare_psreadline_module` returning before
    /// it declares) — red at the pane's own making (`folio_pane`: "a composed 5.1 pane declares its
    /// module path"), before a shell is started on the account's module.
    #[test]
    fn a_windows_powershell_pane_loads_folios_psreadline_silently_under_every_policy() {
        let _turn = observation_turn();
        for policy in ["Restricted", "AllSigned", "RemoteSigned"] {
            let mut pane = folio_pane(&["-NoLogo", "-ExecutionPolicy", policy], false, 120, 30);
            pane.until(&format!("{policy}: the first prompt"), |pane| {
                pane.output_since(0).contains("\x1b]133;B")
            });
            pane.settle();
            let before = pane.output_since(0);
            assert!(
                before.contains("\x1b]133;A"),
                "{policy}: the prompt opened its region: {before:?}"
            );
            let lines: Vec<String> = pane
                .rows()
                .into_iter()
                .filter(|row| !row.trim().is_empty())
                .collect();
            assert_eq!(
                lines.len(),
                1,
                "{policy}: nothing is printed before the first prompt: {lines:?}"
            );
            assert!(
                lines[0].starts_with("PS "),
                "{policy}: the one line is the prompt: {lines:?}"
            );

            let first = pane.run("$?", "True");
            assert!(
                first.contains("True"),
                "{policy}: the first $? is True: {first:?}"
            );
            // The markers are spelled in two halves, so the echo of the typed line never holds them.
            // The function drive, and not `Get-Command`: a lookup of a missing function would be an
            // autoload (`powershell_load_command`'s note), and would itself import a second module.
            let facts = pane.run(
                "'BTPS' + 'RL|' + (Get-Module PSReadLine).Version + '|' + (Get-Module PSReadLine).\
             ModuleBase + '|' + @(Get-Module PSReadLine).Count + '|' + (Test-Path \
             Function:PSConsoleHostReadLine) + '|E' + 'ND'",
                "|END",
            );
            let line = facts
                .split("BTPSRL|")
                .nth(1)
                .and_then(|tail| tail.split("|END").next())
                .unwrap_or_default()
                .replace(['\r', '\n'], "");
            eprintln!("BT_PSRL_PANE policy={policy} facts={line}");
            let fields: Vec<&str> = line.split('|').collect();
            assert_eq!(
                fields.first().copied(),
                Some(crate::psreadline::SHIPPED_VERSION),
                "{policy}: the pane runs Folio's PSReadLine: {line}"
            );
            let base = fields.get(1).copied().unwrap_or_default();
            assert!(
                Path::new(base).starts_with(&pane.module),
                "{policy}: from Folio's directory {}: {line}",
                pane.module.display()
            );
            assert_eq!(
                fields.get(2).copied(),
                Some("1"),
                "{policy}: one module: {line}"
            );
            assert_eq!(
                fields.get(3).copied(),
                Some("True"),
                "{policy}: the loader defined PSConsoleHostReadLine: {line}"
            );
            let c = facts.find("\x1b]133;C").expect("the command region opened");
            let d = facts[c..].find("\x1b]133;D;0").map(|at| at + c);
            let a = d.and_then(|d| facts[d..].find("\x1b]133;A"));
            assert!(
                d.is_some() && a.is_some(),
                "{policy}: C, then D;0, then A: {facts:?}"
            );
            for refusal in [
                "untrusted publisher",
                "cannot be loaded",
                "Cannot load PSReadline",
                "无法加载",
            ] {
                assert!(
                    !pane.output_since(0).contains(refusal),
                    "{policy}: printed {refusal:?}"
                );
            }
            pane.close();
        }
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **the resize repair holds on Microsoft's 2.4.5 in a real
    /// 5.1 pane**: narrow it with wrapped, unsubmitted input, widen it, narrow it again, and the line
    /// is intact at every width and takes the next keystroke where it belongs — when the product pays
    /// the anchor chord; and the same chain with no chord corrupts it.
    ///
    /// This is the claim the owner's Q1 rested on (2026-10-15): the fork's native anchor fix retires
    /// because `folio.ps1`'s reflection repair, which pwsh 7's in-box 2.4.5 already gets, puts the
    /// stock 2.4.5's anchor right in Windows PowerShell too. The pair is one shell, one prompt, one
    /// typed line and one chain of widths, twice; the only difference is whether the product's door
    /// pays the chord (`Integration::PowerShellOptIn`, the door a PowerShell pane has) or owes none
    /// (`Integration::None`). The red arm is a standing test that the green one measures the repair
    /// and not the terminal.
    ///
    /// MUTATION: key `folio.ps1`'s repair on the fork's number again (`$psReadLineVersion -ge
    /// [version]'2.4.6'` in front of its 2.4.x branch: 2.4.5 falls to the no-op branch) — the green
    /// arm carves exactly as the red one does (measured 2026-10-17).
    #[test]
    fn the_anchor_repair_holds_wrapped_input_through_a_resize_chain_on_folios_psreadline() {
        let _turn = observation_turn();
        on_the_window_thread();
        let startup = format!(
            "{} function global:prompt {{ '{PROMPT}' }}",
            bt_pty::test_shell::HYGIENE
        );
        let expected = format!("{PROMPT}{TYPED}");
        let mut outcomes = Vec::new();
        for integration in [
            profiles::Integration::None,
            profiles::Integration::PowerShellOptIn,
        ] {
            let mut pane = folio_pane(&["-NoLogo", "-NoExit", "-Command", &startup], true, 38, 24);
            pane.until("the first prompt", |pane| {
                pane.output_since(0).contains("\x1b]133;B")
            });
            pane.settle();
            let version = pane.run(
                "'BTV' + 'ER|' + (Get-Module PSReadLine).Version + '|E' + 'ND'",
                "|END",
            );
            assert!(
                version.contains(&format!("BTVER|{}|END", crate::psreadline::SHIPPED_VERSION)),
                "the pane runs Folio's PSReadLine: {version:?}"
            );
            // A clean screen, so the prompt on it is the one the chain is judged on.
            pane.run("Clear-Host", "\x1b]133;D;0");
            pane.pty.write(TYPED.as_bytes()).unwrap();
            pane.until("the typed line", |pane| pane.input_line() == expected);
            pane.settle();

            let mut clean_throughout = true;
            let mut chords = 0;
            let mut steps = Vec::new();
            for columns in [24, 100, 24] {
                chords += pane.resize(columns, integration);
                let input = pane.input_line();
                let copies = pane.prompt_copies();
                let clean = input == expected && copies == 1;
                clean_throughout &= clean;
                eprintln!(
                    "BT_PSRL_RESIZE integration={integration:?} columns={columns} clean={clean} \
                 copies={copies} input={input:?} rows={:?}",
                    pane.rows()
                        .iter()
                        .filter(|row| !row.trim().is_empty())
                        .collect::<Vec<_>>()
                );
                steps.push((columns, clean, input));
            }
            pane.pty.write(b"Z").unwrap();
            pane.settle();
            let after_edit = pane.input_line();
            let edit_clean = after_edit == format!("{expected}Z") && pane.prompt_copies() == 1;
            eprintln!(
                "BT_PSRL_RESIZE_SUMMARY integration={integration:?} chords={chords} \
             clean_throughout={clean_throughout} edit_clean={edit_clean} input={after_edit:?} \
             rows={:?}",
                pane.rows()
                    .iter()
                    .filter(|row| !row.trim().is_empty())
                    .collect::<Vec<_>>()
            );
            outcomes.push((integration, chords, clean_throughout && edit_clean, steps));
            pane.close();
        }
        let (_, red_chords, red_clean, red_steps) = &outcomes[0];
        let (_, green_chords, green_clean, green_steps) = &outcomes[1];
        assert_eq!(*red_chords, 0, "the red arm is owed no chord");
        assert_eq!(
            *green_chords, 3,
            "the green arm is paid one chord per committed resize"
        );
        assert!(
            *green_clean,
            "with the chord, the wrapped line is intact at every width and after an edit: \
         {green_steps:?}"
        );
        assert!(
            !*red_clean,
            "without the chord the same chain carves the line, or the pair measures nothing: \
         {red_steps:?}"
        );
    }

    /// A row whose `-Command` dot-sources `profile` — a file in the test's own directory written
    /// with `text` — the way the spike emulated an account's profile (its Q6): the file runs after
    /// the host's own import of PSReadLine and before Folio's loader, which is where a real
    /// profile stands. `-ExecutionPolicy Bypass` on the row, because a dot-sourced file is a
    /// script and the subject here is what it does, not whether a policy lets it run.
    fn profile_pane(text: &str) -> (Pane, PathBuf) {
        let profile = bt_testpath::temp_path("psreadline-profile").with_extension("ps1");
        std::fs::write(&profile, text).unwrap();
        let startup = format!(
            "{} . {}",
            bt_pty::test_shell::HYGIENE,
            powershell_single_quoted(&profile)
        );
        let pane = folio_pane(
            &[
                "-NoLogo",
                "-ExecutionPolicy",
                "Bypass",
                "-NoExit",
                "-Command",
                &startup,
            ],
            true,
            120,
            30,
        );
        (pane, profile)
    }

    /// The pane's PSReadLine facts, one line: every loaded module's version and base, the
    /// continuation prompt, and whether the function the loader defines is there.
    const FACTS: &str = "'BTPS' + 'RL|' + ((Get-Module PSReadLine | ForEach-Object { [string]$_.Version \
                         + '@' + $_.ModuleBase }) -join ',') + '|' + (Get-PSReadLineOption).\
                         ContinuationPrompt + '|' + (Test-Path Function:PSConsoleHostReadLine) + \
                         '|E' + 'ND'";

    fn facts_of(output: &str) -> String {
        output
            .split("BTPSRL|")
            .nth(1)
            .and_then(|tail| tail.split("|END").next())
            .unwrap_or_default()
            .replace(['\r', '\n'], "")
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **an account's profile configures Folio's PSReadLine,
    /// and a by-name `Import-Module PSReadLine` in it changes nothing** (the note's §2.3 table, its
    /// first three rows, measured).
    ///
    /// The host imported Folio's module before the profile ran, so `Set-PSReadLineOption` is
    /// Folio's module being configured, and an import by name finds the module of that name
    /// already loaded: one module, Folio's, configured.
    ///
    /// MUTATION: compose the prefix after the pane's own path instead of in front of it — red at the
    /// pane's own making (`folio_pane`: "Folio's directory is first"), and the composition's own
    /// test names it too.
    #[test]
    fn a_profile_configures_folios_psreadline_and_a_by_name_import_changes_nothing() {
        let _turn = observation_turn();
        let (mut pane, profile) = profile_pane(
            "Set-PSReadLineOption -ContinuationPrompt 'BTCONT> '\r\nImport-Module PSReadLine\r\n",
        );
        pane.until("the first prompt", |pane| {
            pane.output_since(0).contains("\x1b]133;B")
        });
        pane.settle();
        let facts = facts_of(&pane.run(FACTS, "|END"));
        eprintln!("BT_PSRL_PROFILE row=set-option+import-by-name facts={facts}");
        let fields: Vec<&str> = facts.split('|').collect();
        let modules: Vec<&str> = fields[0].split(',').collect();
        assert_eq!(modules.len(), 1, "one PSReadLine: {facts}");
        let (version, base) = modules[0].split_once('@').expect("version@base");
        assert_eq!(version, crate::psreadline::SHIPPED_VERSION, "{facts}");
        assert!(
            Path::new(base).starts_with(&pane.module),
            "Folio's: {facts}"
        );
        assert_eq!(
            fields.get(1).copied(),
            Some("BTCONT> "),
            "configured: {facts}"
        );
        assert_eq!(fields.get(2).copied(), Some("True"), "{facts}");
        pane.close();
        let _ = std::fs::remove_file(profile);
    }

    /// MEASURED, NOT ASSERTED (the note's §2.3 last row, §5.1): **a profile that demands another
    /// PSReadLine by version**, in the two shapes a profile has: `Import-Module PSReadLine
    /// -RequiredVersion 2.0.0` alone (the in-box version every Windows ships), and
    /// `Remove-Module PSReadLine` before it. What follows is the account's explicit choice and its
    /// consequence; Folio does not detect it, repair it or refuse the pane, and the Shell
    /// integration switch is the person's road back to the account's editor. This records what
    /// happens — the modules loaded, whether a prompt comes back, whether a typed command runs, and
    /// which integration marks arrive around it — on `BT_PSRL_PROFILE` lines, and the design
    /// note's table states the measurement. It asserts only that each measurement was taken.
    #[test]
    fn a_profile_that_demands_another_psreadline_version_is_measured() {
        let _turn = observation_turn();
        for (shape, text) in [
            (
                "require-2.0.0",
                "Import-Module PSReadLine -RequiredVersion 2.0.0\r\n\
                 [Console]::Write('BTPROFILE' + 'DONE')\r\n",
            ),
            (
                "remove+require-2.0.0",
                "Remove-Module PSReadLine\r\nImport-Module PSReadLine -RequiredVersion 2.0.0\r\n\
                 [Console]::Write('BTPROFILE' + 'DONE')\r\n",
            ),
        ] {
            let (mut pane, profile) = profile_pane(text);
            let ran = pane.reached_within(Duration::from_secs(20), |pane| {
                pane.output_since(0).contains("BTPROFILEDONE")
            });
            let prompted = pane.reached_within(Duration::from_secs(20), |pane| {
                pane.output_since(0).contains("\x1b]133;B")
            });
            pane.settle();
            let start = pane.rows();
            let mark = pane.raw.len();
            pane.pty.write(format!("{FACTS}\r").as_bytes()).unwrap();
            let answered = pane.reached_within(Duration::from_secs(20), |pane| {
                pane.output_since(mark).contains("|END")
            });
            pane.settle();
            let output = pane.output_since(mark);
            let shown = |rows: &[String]| {
                rows.iter()
                    .filter(|row| !row.trim().is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            eprintln!(
                "BT_PSRL_PROFILE row={shape} profile_ran={ran} prompt_opened={prompted} \
                 command_answered={answered} facts={} marks_after={:?} screen_before={:?} \
                 screen_after={:?}",
                facts_of(&output),
                ["\x1b]133;A", "\x1b]133;B", "\x1b]133;C", "\x1b]133;D"]
                    .iter()
                    .map(|mark| (
                        mark.trim_start_matches("\x1b]133;"),
                        output.matches(mark).count()
                    ))
                    .collect::<Vec<_>>(),
                shown(&start),
                shown(&pane.rows())
            );
            pane.close();
            let _ = std::fs::remove_file(profile);
            assert!(
                ran,
                "{shape}: the profile ran, so what follows is its consequence"
            );
        }
    }

    /// PIN (the spike's Follow-up B, pinned here) — **a pwsh 7 started inside a 5.1 pane keeps its
    /// own PSReadLine**: it puts its own module folders ahead of the path it inherited, Folio's
    /// prefix included, so the first PSReadLine it finds is not Folio's. Skipped, and said, on a
    /// machine with no pwsh 7.
    ///
    /// MUTATION: none in Folio's code reaches this — it pins pwsh 7's own rule, which the design
    /// relies on to leave a nested pwsh untouched.
    #[test]
    fn a_nested_pwsh_7_keeps_its_own_psreadline() {
        let Some(pwsh) = bt_platform::program_on_path(Path::new("pwsh.exe")) else {
            eprintln!("pwsh.exe: not installed; the nested pwsh 7 pin did not run");
            return;
        };
        let _turn = observation_turn();
        let mut pane = folio_pane(&["-NoLogo"], false, 160, 30);
        pane.until("the first prompt", |pane| {
            pane.output_since(0).contains("\x1b]133;B")
        });
        pane.settle();
        let output = pane.run(
            &format!(
                "& {} -NoLogo -NoProfile -Command \"'BTNE' + 'STED|' + (Get-Module -ListAvailable \
                 PSReadLine | Select-Object -First 1).ModuleBase + '|E' + 'ND'\"",
                powershell_single_quoted(&pwsh)
            ),
            "|END",
        );
        let base = output
            .split("BTNESTED|")
            .nth(1)
            .and_then(|tail| tail.split("|END").next())
            .unwrap_or_default()
            .replace(['\r', '\n'], "");
        eprintln!("BT_PSRL_NESTED_PWSH first_psreadline={base}");
        assert!(!base.is_empty(), "the nested pwsh answered: {output:?}");
        assert!(
            !Path::new(&base).starts_with(&pane.module),
            "a nested pwsh 7 finds its own PSReadLine first, not Folio's: {base}"
        );
        pane.close();
    }

    /// RED (T-INTEGRATION-INJECT-2-BUILD) — **an artefact a live 5.1 shell has loaded is kept
    /// whole, and goes once the shell has ended** — the measurement the pruning's rule rests on:
    /// Windows refuses to delete an assembly a running process has mapped.
    ///
    /// The pane's own artefact is pruned as though another build were keeping its own: while the
    /// shell runs, the attempt is refused at the assembly and every file of the module is still
    /// there; once the shell has ended, the same call removes the directory.
    ///
    /// MUTATION: remove the directory without trying the assembly first
    /// (`prune_psreadline_artefacts`) — the live shell's module directory loses its other files.
    #[test]
    fn a_loaded_artefact_is_kept_until_its_shell_ends() {
        let _turn = observation_turn();
        let mut pane = folio_pane(&["-NoLogo"], false, 120, 30);
        pane.until("the first prompt", |pane| {
            pane.output_since(0).contains("\x1b]133;B")
        });
        pane.settle();
        let module = pane.module.clone();
        let psmodules = module
            .parent()
            .expect("an artefact is under psmodules")
            .to_path_buf();
        let artefact = module
            .file_name()
            .expect("the artefact's name")
            .to_string_lossy()
            .into_owned();
        let prune = || {
            prune_psreadline_artefacts(
                &psmodules,
                "another-builds-artefact",
                &crate::psreadline::directory_entries,
                &|path| std::fs::remove_file(path),
            )
        };

        let kept = prune();
        eprintln!("BT_PSRL_PRUNE_LIVE kept={kept:?}");
        assert_eq!(
            kept.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [artefact.as_str()],
            "the live shell's artefact is kept: {kept:?}"
        );
        for (name, _) in crate::psreadline::module_files() {
            assert!(
                module.join(name).is_file(),
                "and kept whole: {name} is still there"
            );
        }

        pane.pty.shutdown().expect("the shell ends");
        let kept = prune();
        assert!(kept.is_empty(), "nothing holds it now: {kept:?}");
        assert!(!module.exists(), "and it is gone");
    }
}
