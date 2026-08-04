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
    /// Mirrors `Path::is_file`: true only for a real, directly-openable file. A directory or a
    /// dangling reparse point answers `false`, exactly as a spawn attempt against it would fail.
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
    use std::ffi::OsStr;

    use super::*;

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
