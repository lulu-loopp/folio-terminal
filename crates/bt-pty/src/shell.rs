//! Default-shell resolution for `PtySession::spawn_default`.
//!
//! Ruling (2026-08-04, evidence-backed): PowerShell 5.1 ships PSReadLine 2.0.0 (2020), whose
//! stale render anchor corrupts an unsubmitted wrapped input line whenever the pane narrows —
//! reproduced in this terminal and in Windows Terminal, while PowerShell 7's PSReadLine 2.4.5 is
//! clean in both. Modern terminals already default to `pwsh` when it is present, so the default
//! shell moves too: `pwsh.exe` when the probe below can find an install, `powershell.exe`
//! otherwise. See `docs/shell-integration.md` for the user-facing note.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

const BT_SHELL_ENV: &str = "BT_SHELL";
const PWSH_EXE: &str = "pwsh.exe";
const WINDOWS_POWERSHELL_EXE: &str = "powershell.exe";

/// Filesystem/environment access used by shell resolution, injected so `resolve_default_shell`
/// is a pure function of its inputs and its tests never depend on what happens to be installed on
/// the host that runs them.
pub trait ShellEnvironment {
    /// Mirrors `std::env::var_os`.
    fn var_os(&self, key: &str) -> Option<OsString>;
    /// Whether `path` names a program this machine could start: `true` for a file that is there,
    /// `false` for a directory, for nothing at all, and for a link that leads nowhere.
    ///
    /// **Not "can this file be opened".** The Microsoft Store installs PowerShell 7 as an *app
    /// execution alias*: an `AppExecLink` reparse point at
    /// `%LocalAppData%\Microsoft\WindowsApps\pwsh.exe`, zero bytes long, whose tag nothing in the
    /// filesystem stack will follow for an ordinary `CreateFileW` — that call answers
    /// `ERROR_CANT_ACCESS_FILE` (1920). `CreateProcess` **does** follow it, and starts the real
    /// `pwsh.exe` out of the package directory. So a probe built on opening the file would be
    /// *stricter than spawning*, and would report a perfectly startable PowerShell 7 missing on
    /// every machine that got it from the Store — greying the row in the picker and dropping the
    /// default shell to Windows PowerShell 5.1.
    ///
    /// `Path::is_file` is not such a probe, and this is why the implementation must stay spelled
    /// that way: `std::fs::metadata` treats a refused open as a reason to ask again rather than an
    /// answer, falling back to `FindFirstFileExW`, which reads the entry's attributes and reparse
    /// tag without following anything. An `AppExecLink` comes back as a non-directory that is not
    /// a symlink — a file — while a symlink into thin air still comes back as nothing, because
    /// that path is followed and does not arrive. Pinned by
    /// `an_app_exec_link_is_probed_as_a_startable_program`.
    fn is_file(&self, path: &Path) -> bool;
}

/// The real environment: `std::env::var_os` and an actual filesystem probe. Used by
/// `PtySession::spawn_default`; every other caller injects a fake so resolution stays testable.
pub struct SystemShellEnvironment;

impl ShellEnvironment for SystemShellEnvironment {
    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }

    fn is_file(&self, path: &Path) -> bool {
        // `Path::is_file` and deliberately nothing narrower — see the trait's doc for what a
        // direct open does to a Store install of PowerShell 7.
        path.is_file()
    }
}

/// How `resolve_default_shell` picked the program it returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellChoice {
    /// `BT_SHELL` was set to a non-empty value; that value was used verbatim, unresolved and
    /// unvalidated.
    Override,
    /// PowerShell 7 (`pwsh.exe`) was found on `PATH` or at a well-known install location.
    PowerShellCore,
    /// Neither `BT_SHELL` nor a `pwsh.exe` install was found; Windows PowerShell 5.1 is the
    /// last-resort default.
    WindowsPowerShell,
}

/// The outcome of shell resolution: which program to spawn, and why it was picked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedShell {
    pub program: OsString,
    pub choice: ShellChoice,
}

/// Shell selection order: `BT_SHELL` wins outright over everything else; otherwise PowerShell 7
/// is preferred when `environment` can find an install, and Windows PowerShell 5.1 is the
/// default when it cannot.
///
/// `BT_SHELL` semantics: its value is used verbatim as the child process's program — a full path
/// (`C:\Tools\pwsh.exe`) or a bare executable name (`pwsh`, resolved against `PATH` by the OS at
/// spawn time, exactly as `CommandBuilder`/`CreateProcess` would resolve it) are both accepted.
/// The value is never checked for existence here, and an empty value is treated the same as an
/// unset one. A spawn failure — including one caused by a bad `BT_SHELL` — still falls back to
/// `powershell.exe` the same way an unavailable `pwsh` install would; see
/// `PtySession::spawn_default`.
pub fn resolve_default_shell(environment: &dyn ShellEnvironment) -> ResolvedShell {
    if let Some(overridden) = environment
        .var_os(BT_SHELL_ENV)
        .filter(|value| !value.is_empty())
    {
        return ResolvedShell {
            program: overridden,
            choice: ShellChoice::Override,
        };
    }
    match find_pwsh(environment) {
        Some(program) => ResolvedShell {
            program,
            choice: ShellChoice::PowerShellCore,
        },
        None => ResolvedShell {
            program: WINDOWS_POWERSHELL_EXE.into(),
            choice: ShellChoice::WindowsPowerShell,
        },
    }
}

/// PowerShell 7 and **only** PowerShell 7 — `BT_SHELL`'s override, else an install of `pwsh.exe`,
/// else nothing.
///
/// The same first two steps as [`resolve_default_shell`] without its third, and the difference is
/// the whole of it: that function answers "what shell should this terminal start when nothing
/// says otherwise", so it must always answer, and Windows PowerShell is what it answers with. This
/// one answers "where is PowerShell 7 on this machine", which has a real `None` — and a profile
/// named `PowerShell` that quietly started 5.1 would be a row that says one thing and does
/// another, on precisely the machines where the two are visibly different products.
///
/// `BT_SHELL` stays on this side of the split (ruling 2026-08-10, Q4: it is the PowerShell
/// profile's override and not a fifth profile's worth of configuration), and it is still taken
/// verbatim and unprobed — an override that pointed at nothing would leave the profile greyed
/// rather than silently ignored, which is the honest reading of "used verbatim".
#[must_use]
pub fn resolve_powershell_seven(environment: &dyn ShellEnvironment) -> Option<OsString> {
    if let Some(overridden) = environment
        .var_os(BT_SHELL_ENV)
        .filter(|value| !value.is_empty())
    {
        return Some(overridden);
    }
    find_pwsh(environment)
}

/// `PATH` search first, then the two well-known install locations that are not guaranteed to be
/// on `PATH`: the traditional MSI/`winget` layout under `%ProgramFiles%\PowerShell\7`, and the
/// Microsoft Store app-execution alias under `%LocalAppData%\Microsoft\WindowsApps`. All three are
/// probed by direct filesystem check rather than assumed present, because on a real machine
/// PowerShell 7 can land through any one of an MSI install, `winget`, or the Store, and only the
/// first of those reliably ends up on `PATH`.
fn find_pwsh(environment: &dyn ShellEnvironment) -> Option<OsString> {
    if let Some(found) = search_path_for(environment, PWSH_EXE) {
        return Some(found.into_os_string());
    }
    if let Some(program_files) = environment.var_os("ProgramFiles") {
        let candidate = Path::new(&program_files)
            .join("PowerShell")
            .join("7")
            .join(PWSH_EXE);
        if environment.is_file(&candidate) {
            return Some(candidate.into_os_string());
        }
    }
    if let Some(local_app_data) = environment.var_os("LocalAppData") {
        let candidate = Path::new(&local_app_data)
            .join("Microsoft")
            .join("WindowsApps")
            .join(PWSH_EXE);
        if environment.is_file(&candidate) {
            return Some(candidate.into_os_string());
        }
    }
    None
}

/// A `PATH`-directory search for `file_name`, going through the injected probe end to end
/// (`env::split_paths` only parses the already-fetched `PATH` value; it touches neither the real
/// environment nor the real filesystem).
fn search_path_for(environment: &dyn ShellEnvironment, file_name: &str) -> Option<PathBuf> {
    let path = environment.var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(file_name))
        .find(|candidate| environment.is_file(candidate))
}

/// An in-memory `ShellEnvironment` so resolution-order and fallback tests are deterministic
/// regardless of what is actually installed on the host running them. `pub(crate)` (not nested in
/// `mod tests`) so `PtySession::spawn_default`'s own tests in `lib.rs` can drive the exact same
/// fake through `spawn_default_with`.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeShellEnvironment {
    vars: std::cell::RefCell<std::collections::HashMap<String, OsString>>,
    files: std::cell::RefCell<std::collections::HashSet<PathBuf>>,
}

#[cfg(test)]
impl FakeShellEnvironment {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_var(self, key: &str, value: impl Into<OsString>) -> Self {
        self.vars.borrow_mut().insert(key.to_owned(), value.into());
        self
    }

    pub(crate) fn with_file(self, path: impl Into<PathBuf>) -> Self {
        self.files.borrow_mut().insert(path.into());
        self
    }
}

#[cfg(test)]
impl ShellEnvironment for FakeShellEnvironment {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.vars.borrow().get(key).cloned()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains(path)
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, os::windows::fs::MetadataExt};

    use super::*;

    /// `FILE_ATTRIBUTE_REPARSE_POINT`, named here rather than pulled in: this crate has no Win32
    /// bindings and needs none for a bit that `std::os::windows::fs::MetadataExt` already hands
    /// over as a `u32`.
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    /// The Store's app execution alias for PowerShell 7 — but only when this machine really has
    /// one, and only when the thing at that path really is a reparse point.
    ///
    /// Both halves matter. `None` on a machine with no Store install (which includes the CI
    /// runner) lets the tests below say nothing rather than something false; and refusing a plain
    /// file sitting at that path stops them passing *vacuously* on a machine where something else
    /// put an ordinary `pwsh.exe` in `WindowsApps`, which would prove nothing about the case they
    /// exist for.
    fn store_pwsh_alias() -> Option<PathBuf> {
        let candidate = Path::new(&env::var_os("LocalAppData")?)
            .join("Microsoft")
            .join("WindowsApps")
            .join(PWSH_EXE);
        let attributes = candidate.symlink_metadata().ok()?.file_attributes();
        (attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0).then_some(candidate)
    }

    /// Skip-with-a-line, in the shape `tests/shell_integration_osc133.rs` already uses: a gate
    /// that quietly passes when its subject is missing is not a gate, so it says so on stderr.
    fn skipped_for_want_of_a_store_install(test: &str) {
        eprintln!(
            "BT_SHELL_PROBE skipped={test} reason=no-store-appexeclink \
             (%LocalAppData%\\Microsoft\\WindowsApps\\pwsh.exe is not a reparse point here)"
        );
    }

    /// Variables named by the test, files taken from the real machine.
    ///
    /// Neither `FakeShellEnvironment` nor `SystemShellEnvironment` can ask the question these two
    /// tests ask. The fake answers `is_file` from a set the test filled in, so it can only confirm
    /// what the test already believes; the real one reads `PATH` and `%ProgramFiles%` off this
    /// process, so what it finds depends on the shell that launched the test run — and the
    /// scenario under test is precisely a machine where *neither* of those two leads anywhere and
    /// the Store alias is the only PowerShell 7 there is. Naming the variables and leaving the
    /// filesystem real reproduces that machine without touching this process's environment.
    struct NamedVarsRealFiles(Vec<(&'static str, OsString)>);

    impl ShellEnvironment for NamedVarsRealFiles {
        fn var_os(&self, key: &str) -> Option<OsString> {
            self.0
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
        }

        fn is_file(&self, path: &Path) -> bool {
            SystemShellEnvironment.is_file(path)
        }
    }

    /// The probe answers for a program `CreateProcess` can start, and not for one `CreateFileW`
    /// can open — those are two different questions and only the first is the one being asked.
    ///
    /// A Store install of PowerShell 7 is the case where they come apart: opening
    /// `%LocalAppData%\Microsoft\WindowsApps\pwsh.exe` fails with `ERROR_CANT_ACCESS_FILE` (1920)
    /// while spawning it works, so a probe that opens is stricter than the spawn it stands in for.
    ///
    /// **Red gate**: spell `SystemShellEnvironment::is_file` as the direct open its doc comment
    /// used to claim it was — `File::open(path).is_ok_and(…)` — and this fails on any machine with
    /// a Store install, taking `a_store_only_install_of_powershell_seven_still_resolves` with it.
    #[test]
    fn an_app_exec_link_is_probed_as_a_startable_program() {
        let Some(alias) = store_pwsh_alias() else {
            skipped_for_want_of_a_store_install(
                "an_app_exec_link_is_probed_as_a_startable_program",
            );
            return;
        };
        assert!(
            SystemShellEnvironment.is_file(&alias),
            "{} is an AppExecLink that CreateProcess follows; the probe must not be stricter \
             than the spawn it stands in for",
            alias.display()
        );
    }

    /// And the whole resolution lands on it: on a machine where `PATH` carries no PowerShell 7 and
    /// no MSI put one under `%ProgramFiles%`, the Store alias is the third probe and the answer.
    ///
    /// This is the machine in `docs/plans/release/readiness-gaps-2026-08-27.md` §B1 — where the
    /// persistent `PATH` that `folio.exe` inherits from Explorer names the `WindowsApps` alias
    /// directory and nothing else, so the package directory holding the real `pwsh.exe` is
    /// reachable only through the alias.
    #[test]
    fn a_store_only_install_of_powershell_seven_still_resolves() {
        let Some(alias) = store_pwsh_alias() else {
            skipped_for_want_of_a_store_install(
                "a_store_only_install_of_powershell_seven_still_resolves",
            );
            return;
        };
        let local_app_data = env::var_os("LocalAppData").expect("`store_pwsh_alias` read it");
        // `PATH` and `ProgramFiles` are left unnamed rather than pointed somewhere empty: an unset
        // variable skips its probe outright, which is the same miss without depending on some
        // directory staying free of a `pwsh.exe`.
        let environment = NamedVarsRealFiles(vec![("LocalAppData", local_app_data)]);
        assert_eq!(
            resolve_powershell_seven(&environment),
            Some(alias.clone().into_os_string())
        );
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, alias.into_os_string());
    }

    /// The other side of the same contract, and the part that needs no Store install: the probe is
    /// permissive about *how* a program is reached, not about whether it is there.
    #[test]
    fn the_real_probe_still_refuses_a_directory_and_a_path_with_nothing_at_it() {
        let system_root = env::var_os("SystemRoot").expect("Windows always sets %SystemRoot%");
        let directory = PathBuf::from(&system_root);
        assert!(
            !SystemShellEnvironment.is_file(&directory),
            "a directory is not a program"
        );
        assert!(
            SystemShellEnvironment.is_file(
                &directory
                    .join("System32")
                    .join("WindowsPowerShell")
                    .join("v1.0")
                    .join(WINDOWS_POWERSHELL_EXE)
            ),
            "and an ordinary file still is — Windows PowerShell 5.1, the floor every fallback in \
             this module is written against"
        );
        assert!(
            !SystemShellEnvironment.is_file(&directory.join("no-such-program-lives-here.exe")),
            "nothing at the path is nothing to start"
        );
    }

    fn path_var(directories: &[&str]) -> OsString {
        env::join_paths(directories.iter().map(PathBuf::from))
            .expect("test PATH directories must join cleanly")
    }

    #[test]
    fn bt_shell_override_wins_even_when_pwsh_is_also_installed() {
        let pwsh = PathBuf::from(r"C:\PATHDIR\pwsh.exe");
        let environment = FakeShellEnvironment::new()
            .with_var("BT_SHELL", r"C:\Tools\custom-shell.exe")
            .with_var("PATH", path_var(&[r"C:\PATHDIR"]))
            .with_file(pwsh);
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::Override);
        assert_eq!(resolved.program, OsStr::new(r"C:\Tools\custom-shell.exe"));
    }

    #[test]
    fn bt_shell_override_is_used_verbatim_as_a_bare_name() {
        let environment = FakeShellEnvironment::new().with_var("BT_SHELL", "my-shell");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::Override);
        assert_eq!(resolved.program, OsStr::new("my-shell"));
    }

    #[test]
    fn empty_bt_shell_is_treated_as_unset() {
        let environment = FakeShellEnvironment::new().with_var("BT_SHELL", "");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::WindowsPowerShell);
    }

    #[test]
    fn pwsh_found_on_path_is_preferred_over_windows_powershell() {
        let pwsh = PathBuf::from(r"C:\PATHDIR\pwsh.exe");
        let environment = FakeShellEnvironment::new()
            .with_var("PATH", path_var(&[r"C:\OtherDir", r"C:\PATHDIR"]))
            .with_file(pwsh.clone());
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, pwsh.into_os_string());
    }

    #[test]
    fn pwsh_found_under_program_files_seven_is_used_when_absent_from_path() {
        let candidate = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let environment = FakeShellEnvironment::new()
            .with_var("PATH", path_var(&[r"C:\OtherDir"]))
            .with_var("ProgramFiles", r"C:\Program Files")
            .with_file(candidate.clone());
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, candidate.into_os_string());
    }

    #[test]
    fn pwsh_found_under_the_windows_apps_alias_is_used_as_the_last_probe() {
        let candidate =
            PathBuf::from(r"C:\Users\Example\AppData\Local\Microsoft\WindowsApps\pwsh.exe");
        let environment = FakeShellEnvironment::new()
            .with_var("PATH", path_var(&[r"C:\OtherDir"]))
            .with_var("ProgramFiles", r"C:\Program Files")
            .with_var("LocalAppData", r"C:\Users\Example\AppData\Local")
            .with_file(candidate.clone());
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, candidate.into_os_string());
    }

    #[test]
    fn windows_powershell_is_the_default_when_pwsh_is_nowhere_to_be_found() {
        let environment = FakeShellEnvironment::new()
            .with_var("PATH", path_var(&[r"C:\OtherDir"]))
            .with_var("ProgramFiles", r"C:\Program Files")
            .with_var("LocalAppData", r"C:\Users\Example\AppData\Local");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::WindowsPowerShell);
        assert_eq!(resolved.program, OsStr::new("powershell.exe"));
    }

    #[test]
    fn missing_path_variable_does_not_panic_and_still_probes_well_known_locations() {
        let candidate = PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe");
        let environment = FakeShellEnvironment::new()
            .with_var("ProgramFiles", r"C:\Program Files")
            .with_file(candidate.clone());
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::PowerShellCore);
        assert_eq!(resolved.program, candidate.into_os_string());
    }
}
