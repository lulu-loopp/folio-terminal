//! Default-shell resolution for `PtySession::spawn_default`.
//!
//! Ruling (2026-08-04, evidence-backed): PowerShell 5.1 ships PSReadLine 2.0.0 (2020), whose
//! stale render anchor corrupts an unsubmitted wrapped input line whenever the pane narrows —
//! reproduced in this terminal and in Windows Terminal, while PowerShell 7's PSReadLine 2.4.5 is
//! clean in both. Modern terminals already default to `pwsh` when it is present, so the default
//! shell moves too: `pwsh.exe` when the probe below can find an install, `powershell.exe`
//! otherwise. See `docs/shell-integration.md` for the user-facing note.
//!
//! Ruling (2026-09-12, Q8 of `docs/plans/port/macos-plan-2026-09-12.md`, and remote T2 of
//! `docs/plans/remote/research-2026-09-10.md`): **off Windows none of that is reachable.** There
//! the order is the user's `$SHELL` when it names a program this machine can start, then the
//! system shells — `/bin/zsh` first on macOS, where it is the platform's own default, then
//! `/bin/bash` — and `/bin/sh` as the floor, which is not probed because POSIX says it is there.
//! The shell is started **interactive and non-login**: a pane is a session inside an application
//! the user already logged into, so `.zprofile`/`.bash_profile` are not ours to re-run. The cost
//! is stated rather than hidden — an app launched from Finder inherits almost no environment, so
//! a `PATH` that only `.zprofile` sets will not be there, and the answer to that is a shipped
//! profile that asks for a login shell (M1-5), not a default that quietly re-logs everybody in.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

const BT_SHELL_ENV: &str = "BT_SHELL";
const PWSH_EXE: &str = "pwsh.exe";
#[cfg(windows)]
const WINDOWS_POWERSHELL_EXE: &str = "powershell.exe";

/// The user's own shell, as every Unix has recorded it since `login` first exported it.
#[cfg(unix)]
const SHELL_ENV: &str = "SHELL";

/// The system shells a macOS machine is probed for, in order, when `$SHELL` cannot answer.
///
/// `/bin/zsh` first because it is what macOS makes every account's shell and what the platform's
/// own terminal starts; `/bin/bash` second because a machine whose zsh has been removed is still
/// a machine somebody works on.
#[cfg(unix)]
const MACOS_SYSTEM_SHELLS: &[&str] = &["/bin/zsh", "/bin/bash"];

/// The same list where zsh is not the platform's own answer: a Linux box keeps zsh under
/// `/usr/bin` as often as `/bin` and does not install it at all by default, so probing a hard
/// `/bin/zsh` there would be a guess wearing a probe's clothes. `bash` is the one a Linux
/// userland really does put at `/bin/bash`.
#[cfg(unix)]
const OTHER_UNIX_SYSTEM_SHELLS: &[&str] = &["/bin/bash"];

/// The floor, and the one entry in this module that is **not** probed.
///
/// POSIX requires `/bin/sh` to be there, which is exactly the standing Windows PowerShell 5.1 is
/// given on the other side of this file: the answer resolution gives when every probe has missed,
/// and the one a spawn failure has nothing left to retry against.
#[cfg(unix)]
pub(crate) const BOURNE_SHELL: &str = "/bin/sh";

/// The flags this terminal starts any PowerShell with — and the only entry in the argument
/// tables below that is not empty, because it is the only shell in them that needs telling.
#[cfg(windows)]
const POWERSHELL_INTERACTIVE_ARGS: &[&str] = &["-NoLogo"];

/// What a Unix shell is told about being interactive: **nothing**, and that is the answer rather
/// than an omission.
///
/// A shell whose standard input is a terminal is interactive by its own rule — zsh, bash, dash
/// and ksh all say so in their own manuals, and `-i` is the flag for the other case, where the
/// input is a pipe or a file and the shell would otherwise run in batch. Every terminal on this
/// platform starts the shell the same way: Terminal.app, iTerm2, xterm and gnome-terminal pass
/// no `-i`, and the only argument any of them adds is the `-` in front of argv\[0\] that means
/// *login*, which Q8 rules out. Adding `-i` would buy nothing on the three shells we pick
/// ourselves and would be a **guess about a program the user named** on `$SHELL` — `pwsh` reads
/// `-i` as an abbreviation of `-InputFormat` and refuses to start.
///
/// Non-login is the other half and it is spelled by what is absent twice over: no `-l`, and
/// argv\[0\] left as the program's own path. `portable_pty`'s `CommandBuilder` only prefixes
/// argv\[0\] with `-` for a builder made by `new_default_prog`, and this crate never makes one.
#[cfg(unix)]
const UNIX_INTERACTIVE_ARGS: &[&str] = &[];

/// Filesystem/environment access used by shell resolution, injected so `resolve_default_shell`
/// is a pure function of its inputs and its tests never depend on what happens to be installed on
/// the host that runs them.
pub trait ShellEnvironment {
    /// Mirrors `std::env::var_os`.
    fn var_os(&self, key: &str) -> Option<OsString>;
    /// Whether `path` names a program this machine could start: `true` for a file that is there,
    /// `false` for a directory, for nothing at all, and for a link that leads nowhere.
    ///
    /// **The name is Windows' spelling of the question and the question is the same on both
    /// platforms.** There, "a file that is there" is the whole of it, for the reason below.
    /// On Unix a file with no execute bit is a file `open` succeeds on and `execve` refuses, so
    /// the faithful answer to *could this machine start it* has to read the mode as well — see
    /// [`SystemShellEnvironment::is_file`]. A caller asking "is there a file at this path" for
    /// any other reason wants `Path::is_file` and not this.
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
        // Windows: `Path::is_file` and deliberately nothing narrower — see the trait's doc for
        // what a direct open does to a Store install of PowerShell 7. There is no execute bit to
        // read here; whether a file is startable is the loader's business and the extension's.
        #[cfg(windows)]
        {
            path.is_file()
        }
        // Unix: the same question asked in this platform's own terms. `metadata` follows
        // symlinks, so a `/bin/zsh` that is a link to the real one still answers `true`, and the
        // mode is read because a regular file without an execute bit is precisely the case where
        // "there is a file here" and "this machine can start it" come apart. The three execute
        // bits are read together rather than resolved against this process's own uid and gid:
        // that narrower question is `access(2)`'s, it needs a libc this crate does not depend on,
        // and it would answer `false` for a shell the user can perfectly well run under a
        // different identity — the probe stays permissive about *who*, exactly as its Windows
        // half stays permissive about *how*.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::metadata(path).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        }
    }
}

/// How `resolve_default_shell` picked the program it returned.
///
/// **The PowerShell arms exist only on Windows and the Unix arms only off it**, so that "this is
/// never reached off Windows" is a thing the compiler checks rather than a thing a comment
/// claims. `Override` is on both because `BT_SHELL` is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellChoice {
    /// `BT_SHELL` was set to a non-empty value; that value was used verbatim, unresolved and
    /// unvalidated.
    Override,
    /// PowerShell 7 (`pwsh.exe`) was found on `PATH` or at a well-known install location.
    #[cfg(windows)]
    PowerShellCore,
    /// Neither `BT_SHELL` nor a `pwsh.exe` install was found; Windows PowerShell 5.1 is the
    /// last-resort default.
    #[cfg(windows)]
    WindowsPowerShell,
    /// `$SHELL` named a program this machine can start, and that is the user's own answer to
    /// which shell they work in.
    #[cfg(unix)]
    UserShell,
    /// `$SHELL` said nothing usable, and one of the system shells this platform is probed for
    /// (`MACOS_SYSTEM_SHELLS`, `OTHER_UNIX_SYSTEM_SHELLS`) was there.
    #[cfg(unix)]
    SystemShell,
    /// Every probe missed and `BOURNE_SHELL` is the floor — the Unix twin of
    /// `WindowsPowerShell`, and like it the one resolution a spawn failure cannot retry past.
    #[cfg(unix)]
    BourneShell,
}

/// The outcome of shell resolution: which program to spawn, **which arguments resolution states
/// for it**, and why it was picked.
///
/// `args` is here rather than at the spawn door because the only code that can name a shell's
/// flags is the code that decided which shell this is. It used to be a single `-NoLogo` welded
/// into `spawn_default`, which was true exactly as long as every shell this terminal could start
/// was a PowerShell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedShell {
    pub program: OsString,
    pub args: &'static [&'static str],
    pub choice: ShellChoice,
}

/// Shell selection order. `BT_SHELL` wins outright over everything else on every platform; what
/// follows it is the platform's own question and the two answers share nothing but that first
/// step.
///
/// **Windows**: PowerShell 7 is preferred when `environment` can find an install, and Windows
/// PowerShell 5.1 is the default when it cannot.
///
/// **Unix**: `$SHELL` when it names a program this machine can start, else the system shells this
/// platform is probed for in order, else `BOURNE_SHELL`. Interactive and non-login, and the
/// arguments are the empty list for the reasons written at `UNIX_INTERACTIVE_ARGS`.
///
/// `BT_SHELL` semantics: its value is used verbatim as the child process's program — a full path
/// (`C:\Tools\pwsh.exe`, `/opt/homebrew/bin/fish`) or a bare executable name (`pwsh`, resolved
/// against `PATH` by the OS at spawn time, exactly as `CommandBuilder`/`CreateProcess` would
/// resolve it) are both accepted. The value is never checked for existence here, and an empty
/// value is treated the same as an unset one. A spawn failure — including one caused by a bad
/// `BT_SHELL` — still falls back to the platform's last-resort shell the same way an unavailable
/// `pwsh` install would; see `PtySession::spawn_default`.
pub fn resolve_default_shell(environment: &dyn ShellEnvironment) -> ResolvedShell {
    if let Some(overridden) = environment
        .var_os(BT_SHELL_ENV)
        .filter(|value| !value.is_empty())
    {
        return ResolvedShell {
            program: overridden,
            // Windows: `BT_SHELL` picks *which PowerShell-family build* runs (ruling 2026-08-10,
            // Q4), so it takes the family's flag. Unix: it picks an unrelated program we were
            // told about rather than one we chose, and nothing is guessed about its command line.
            args: OVERRIDE_ARGS,
            choice: ShellChoice::Override,
        };
    }
    #[cfg(windows)]
    {
        resolve_windows_default_shell(environment)
    }
    #[cfg(unix)]
    {
        resolve_unix_default_shell(environment, system_shell_candidates())
    }
}

/// What `BT_SHELL`'s verbatim program is started with. See [`resolve_default_shell`].
#[cfg(windows)]
const OVERRIDE_ARGS: &[&str] = POWERSHELL_INTERACTIVE_ARGS;
#[cfg(unix)]
const OVERRIDE_ARGS: &[&str] = UNIX_INTERACTIVE_ARGS;

/// The Windows half of [`resolve_default_shell`], after `BT_SHELL` has declined to answer.
#[cfg(windows)]
fn resolve_windows_default_shell(environment: &dyn ShellEnvironment) -> ResolvedShell {
    match find_pwsh(environment) {
        Some(program) => ResolvedShell {
            program,
            args: POWERSHELL_INTERACTIVE_ARGS,
            choice: ShellChoice::PowerShellCore,
        },
        None => ResolvedShell {
            program: WINDOWS_POWERSHELL_EXE.into(),
            args: POWERSHELL_INTERACTIVE_ARGS,
            choice: ShellChoice::WindowsPowerShell,
        },
    }
}

/// The Unix half of [`resolve_default_shell`], after `BT_SHELL` has declined to answer.
///
/// `candidates` is a parameter rather than a `cfg` read inside the body so that **both** of this
/// platform's tables can be driven from a test on either of them: what a macOS machine does with
/// a zsh present is a claim a Linux runner is entitled to check, and the other way round.
#[cfg(unix)]
fn resolve_unix_default_shell(
    environment: &dyn ShellEnvironment,
    candidates: &[&'static str],
) -> ResolvedShell {
    // `$SHELL` is the user's recorded answer and it is *probed* rather than taken on trust: it
    // outlives the shell it names — an account moved off a Homebrew fish, a `/usr/local` wiped —
    // and a default that spawns a program that is not there would spend its one recoverable
    // failure before the system shells below ever got a turn.
    if let Some(shell) = environment
        .var_os(SHELL_ENV)
        .filter(|value| !value.is_empty())
        && environment.is_file(Path::new(&shell))
    {
        return ResolvedShell {
            program: shell,
            args: UNIX_INTERACTIVE_ARGS,
            choice: ShellChoice::UserShell,
        };
    }
    for candidate in candidates {
        if environment.is_file(Path::new(candidate)) {
            return ResolvedShell {
                program: OsString::from(*candidate),
                args: UNIX_INTERACTIVE_ARGS,
                choice: ShellChoice::SystemShell,
            };
        }
    }
    ResolvedShell {
        program: BOURNE_SHELL.into(),
        args: UNIX_INTERACTIVE_ARGS,
        choice: ShellChoice::BourneShell,
    }
}

/// Which system-shell table this build's platform uses.
///
/// `cfg!` rather than `#[cfg]` on purpose: both tables stay compiled on every Unix, so the one
/// this machine does not use is still type-checked, still readable, and still nameable by the
/// tests that pin the other platform's order.
#[cfg(unix)]
fn system_shell_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        MACOS_SYSTEM_SHELLS
    } else {
        OTHER_UNIX_SYSTEM_SHELLS
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
///
/// It compiles off Windows and answers `None` there, which is the truth rather than an
/// accommodation: `pwsh.exe` is a file name no Unix install of PowerShell 7 uses. The shipped
/// profiles a macOS build offers are M1-5's, and the day one of them is a PowerShell this
/// function grows the name that platform spells it with.
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
    use std::ffi::OsStr;
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt;

    use super::*;

    /// `FILE_ATTRIBUTE_REPARSE_POINT`, named here rather than pulled in: this crate has no Win32
    /// bindings and needs none for a bit that `std::os::windows::fs::MetadataExt` already hands
    /// over as a `u32`.
    #[cfg(windows)]
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    /// The Store's app execution alias for PowerShell 7 — but only when this machine really has
    /// one, and only when the thing at that path really is a reparse point.
    ///
    /// Both halves matter. `None` on a machine with no Store install (which includes the CI
    /// runner) lets the tests below say nothing rather than something false; and refusing a plain
    /// file sitting at that path stops them passing *vacuously* on a machine where something else
    /// put an ordinary `pwsh.exe` in `WindowsApps`, which would prove nothing about the case they
    /// exist for.
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    #[cfg(windows)]
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
    /// Windows: it asks the real machine about `%SystemRoot%` and about the
    /// Windows PowerShell 5.1 under it, which is the floor every fallback in
    /// this module is written against. There is no floor to point at elsewhere.
    #[cfg(windows)]
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

    /// A `PATH` in Windows' grammar, and Windows-only for the same reason every test that calls
    /// it is: `env::join_paths` refuses `C:\PATHDIR` where `:` is the separator rather than a
    /// drive letter, and `Path::join` does not spell a Windows path off Windows either. These are
    /// tests about *Windows* shell resolution — `pwsh.exe` under `%ProgramFiles%`, the Store's
    /// `WindowsApps` alias, `powershell.exe` when neither is there. The Unix rule has its own
    /// fixtures further down, which name Unix paths for the same reason and are gated the same
    /// way.
    #[cfg(windows)]
    fn path_var(directories: &[&str]) -> OsString {
        env::join_paths(directories.iter().map(PathBuf::from))
            .expect("test PATH directories must join cleanly")
    }

    #[cfg(windows)]
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

    #[cfg(windows)]
    #[test]
    fn empty_bt_shell_is_treated_as_unset() {
        let environment = FakeShellEnvironment::new().with_var("BT_SHELL", "");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::WindowsPowerShell);
    }

    /// The same claim where the last-resort default is a different program: an empty `BT_SHELL`
    /// is an unset `BT_SHELL`, and with nothing else to go on resolution lands on the floor.
    #[cfg(unix)]
    #[test]
    fn empty_bt_shell_is_treated_as_unset() {
        let environment = FakeShellEnvironment::new().with_var("BT_SHELL", "");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::BourneShell);
        assert_eq!(resolved.program, OsStr::new("/bin/sh"));
    }

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    #[cfg(windows)]
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

    /// Windows states one flag for every shell it can resolve, because every shell it can
    /// resolve is a PowerShell.
    #[cfg(windows)]
    #[test]
    fn windows_resolution_states_the_powershell_flag_for_every_arm() {
        let pwsh = PathBuf::from(r"C:\PATHDIR\pwsh.exe");
        for environment in [
            FakeShellEnvironment::new().with_var("BT_SHELL", r"C:\Tools\custom.exe"),
            FakeShellEnvironment::new()
                .with_var("PATH", path_var(&[r"C:\PATHDIR"]))
                .with_file(pwsh.clone()),
            FakeShellEnvironment::new(),
        ] {
            assert_eq!(resolve_default_shell(&environment).args, &["-NoLogo"]);
        }
    }

    // ── the Unix rule (ruling 2026-09-12, Q8 / remote T2) ─────────────────────────────────
    //
    // Unix paths, for the mirror image of the reason the Windows fixtures above are gated:
    // `/bin/zsh` is not a path Windows can spell, and the four claims below are claims about a
    // platform whose shells live at absolute paths with no extension.

    /// The user's own answer wins over anything this module would pick for them — and it wins
    /// only when the machine can actually start it, which is the whole reason `$SHELL` is
    /// probed rather than trusted.
    #[cfg(unix)]
    #[test]
    fn the_user_shell_wins_when_it_names_a_program_this_machine_can_start() {
        let environment = FakeShellEnvironment::new()
            .with_var("SHELL", "/opt/homebrew/bin/fish")
            .with_file("/opt/homebrew/bin/fish")
            .with_file("/bin/zsh")
            .with_file("/bin/bash");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::UserShell);
        assert_eq!(resolved.program, OsStr::new("/opt/homebrew/bin/fish"));
    }

    /// An account whose `$SHELL` outlived the shell — a Homebrew prefix wiped, a package
    /// removed — falls through to the system shells instead of spending the session's one
    /// recoverable spawn failure on a program that is known to be gone.
    #[cfg(unix)]
    #[test]
    fn a_shell_variable_naming_nothing_startable_falls_through_to_the_system_shells() {
        let environment = FakeShellEnvironment::new()
            .with_var("SHELL", "/opt/homebrew/bin/fish")
            .with_file("/bin/zsh")
            .with_file("/bin/bash");
        let resolved = resolve_unix_default_shell(&environment, MACOS_SYSTEM_SHELLS);
        assert_eq!(resolved.choice, ShellChoice::SystemShell);
        assert_eq!(resolved.program, OsStr::new("/bin/zsh"));
    }

    /// An empty `$SHELL` is an unset `$SHELL`, the same reading `BT_SHELL` gets.
    #[cfg(unix)]
    #[test]
    fn an_empty_shell_variable_is_treated_as_unset() {
        let environment = FakeShellEnvironment::new()
            .with_var("SHELL", "")
            .with_file("/bin/bash");
        let resolved = resolve_unix_default_shell(&environment, OTHER_UNIX_SYSTEM_SHELLS);
        assert_eq!(resolved.choice, ShellChoice::SystemShell);
        assert_eq!(resolved.program, OsStr::new("/bin/bash"));
    }

    /// **The order is the platform's, and each platform's order is checked on both.** Same
    /// machine, same two shells present, two tables: macOS reaches for zsh because that is the
    /// shell macOS gives every account, and everywhere else reaches for bash because a Linux
    /// `/bin/zsh` is a package that may not be installed and may not be at that path.
    ///
    /// MUTATION: swap the two entries of `MACOS_SYSTEM_SHELLS` and this fails on the Linux
    /// runner as well as on the Mac, which is the point of passing the table in.
    #[cfg(unix)]
    #[test]
    fn macos_reaches_for_zsh_first_and_every_other_unix_for_bash() {
        let machine = || {
            FakeShellEnvironment::new()
                .with_file("/bin/zsh")
                .with_file("/bin/bash")
        };
        assert_eq!(
            resolve_unix_default_shell(&machine(), MACOS_SYSTEM_SHELLS).program,
            OsStr::new("/bin/zsh")
        );
        assert_eq!(
            resolve_unix_default_shell(&machine(), OTHER_UNIX_SYSTEM_SHELLS).program,
            OsStr::new("/bin/bash")
        );
        assert_eq!(
            system_shell_candidates(),
            if cfg!(target_os = "macos") {
                MACOS_SYSTEM_SHELLS
            } else {
                OTHER_UNIX_SYSTEM_SHELLS
            }
        );
    }

    /// The floor, and the one step that asks the filesystem nothing: a machine that answered
    /// `false` to every probe still gets a shell, because POSIX puts one at `/bin/sh`.
    #[cfg(unix)]
    #[test]
    fn the_bourne_shell_is_the_floor_and_is_never_probed() {
        let environment = FakeShellEnvironment::new();
        for table in [MACOS_SYSTEM_SHELLS, OTHER_UNIX_SYSTEM_SHELLS] {
            let resolved = resolve_unix_default_shell(&environment, table);
            assert_eq!(resolved.choice, ShellChoice::BourneShell);
            assert_eq!(resolved.program, OsStr::new("/bin/sh"));
        }
    }

    /// `BT_SHELL` is still the first question asked, and it is still answered without a probe.
    #[cfg(unix)]
    #[test]
    fn bt_shell_override_wins_over_the_user_shell_and_is_not_probed() {
        let environment = FakeShellEnvironment::new()
            .with_var("BT_SHELL", "/opt/custom/shell")
            .with_var("SHELL", "/bin/zsh")
            .with_file("/bin/zsh");
        let resolved = resolve_default_shell(&environment);
        assert_eq!(resolved.choice, ShellChoice::Override);
        assert_eq!(resolved.program, OsStr::new("/opt/custom/shell"));
    }

    /// **Interactive is what a terminal already is, and non-login is what nobody asked for**, so
    /// every Unix arm states no arguments at all. See `UNIX_INTERACTIVE_ARGS` for why `-i` is
    /// absent on purpose rather than forgotten.
    #[cfg(unix)]
    #[test]
    fn every_unix_arm_states_no_arguments() {
        let override_shell = FakeShellEnvironment::new().with_var("BT_SHELL", "/opt/custom/shell");
        let user_shell = FakeShellEnvironment::new()
            .with_var("SHELL", "/bin/zsh")
            .with_file("/bin/zsh");
        let system_shell = FakeShellEnvironment::new().with_file("/bin/bash");
        let floor = FakeShellEnvironment::new();
        for environment in [override_shell, user_shell, system_shell, floor] {
            let resolved = resolve_default_shell(&environment);
            assert!(
                resolved.args.is_empty(),
                "{:?} was given {:?}",
                resolved.choice,
                resolved.args
            );
        }
    }

    /// The real probe, asked about this machine rather than about a fake — the Unix twin of
    /// `the_real_probe_still_refuses_a_directory_and_a_path_with_nothing_at_it`.
    ///
    /// **Red gate**: spell `SystemShellEnvironment::is_file` as `path.is_file()`, the way its
    /// Windows half is spelled, and the third assertion fails — a regular file with mode `0o644`
    /// is a file, and it is not a program.
    #[cfg(unix)]
    #[test]
    fn the_real_probe_reads_the_execute_bit_and_still_refuses_a_directory() {
        use std::os::unix::fs::PermissionsExt;

        assert!(
            SystemShellEnvironment.is_file(Path::new(BOURNE_SHELL)),
            "every Unix has a startable /bin/sh; that is what makes it the floor"
        );
        assert!(
            !SystemShellEnvironment.is_file(Path::new("/bin")),
            "a directory is not a program"
        );
        assert!(
            !SystemShellEnvironment.is_file(Path::new("/bin/no-such-program-lives-here")),
            "nothing at the path is nothing to start"
        );

        let unreadable_as_a_program = env::temp_dir().join(format!(
            "bt-pty-not-a-program-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&unreadable_as_a_program, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(
            &unreadable_as_a_program,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let answer = SystemShellEnvironment.is_file(&unreadable_as_a_program);
        std::fs::remove_file(&unreadable_as_a_program).unwrap();
        assert!(
            !answer,
            "a regular file with no execute bit is a file this machine cannot start"
        );
    }
}
