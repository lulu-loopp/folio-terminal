//! **A real shell for a test, started so that it cannot reach the account it runs
//! under** (T-TEST-SHELL-HYGIENE, 2026-10-04).
//!
//! The account's PSReadLine history held five copies of
//! `Write-Output ('BT_APP_' + 'INPUT_OK')`: `bt-app`'s
//! `real_powershell_input_reaches_a_viewport_owned_frame` started the default
//! shell the way a pane starts it, so the user's own `$PROFILE` ran and the
//! line it typed was appended to the user's own history file, once per full
//! test run. Other tests had each written their own `-NoProfile` and their own
//! `Set-PSReadLineOption -HistorySaveStyle SaveNothing`, which is a rule kept
//! by everybody remembering it. This module is the rule kept by construction:
//! every real shell a test starts goes through here, and
//! `crates/bt-source/tests/test_shells.rs` is red for one that does not.
//!
//! # The rule, per shell family
//!
//! Every family runs with this test's own [`Hygiene`] directory standing in
//! for every place a shell keeps per-user state that is found through the
//! environment: `HOME`, the four `XDG_*_HOME`s and, on Windows, `APPDATA` and
//! `LOCALAPPDATA`. A test that sets one of these itself must point it inside
//! that directory; anything else is refused before a process starts.
//!
//! * **PowerShell** (`powershell.exe`, `pwsh`). `-NoProfile`, always — the
//!   user's `$PROFILE` is found through the Documents known folder, which no
//!   environment variable moves, so the flag is the only way it is not run.
//!   An *interactive* PowerShell (one that will draw a prompt: `-NoExit`, or no
//!   script at all) is a line editor that appends every accepted line to a
//!   history file the user owns, and that file is also found through a known
//!   folder. So before anything the test wrote can be read as a line, the
//!   startup script ([`powershell_hygiene`]) sets `HistorySaveStyle
//!   SaveNothing` *and* points `HistorySavePath` into the hygiene directory —
//!   the second so that the line editor does not even load the user's history
//!   — reads both back, and writes what it read to a proof file; if either
//!   step fails it writes why and exits. [`TestShell::write`] does not type
//!   until the proof is there and says exactly that, and fails the test with
//!   the shell's own reason otherwise. The script runs before the first prompt
//!   because `-Command` runs before the first prompt; the gate is what turns a
//!   shell that could not establish it into a test that says so, rather than a
//!   test that waits for a prompt it was never going to get. The module
//!   analysis cache is redirected too (`PSModuleAnalysisCachePath`), and
//!   `pwsh`'s telemetry and update check are off. A one-shot PowerShell
//!   (`-Command`, `-EncodedCommand` or `-File` without `-NoExit`) never runs a
//!   line editor, so it has no history to refuse.
//! * **cmd** — `/D`, so the `AutoRun` commands in the user's registry are not
//!   run. `cmd` keeps its history in memory only.
//! * **POSIX shells** (`sh`, `bash`, `dash`, `ksh`, `mksh`, `zsh`, `fish`) —
//!   every startup file a user owns is found through `HOME`, `ZDOTDIR`,
//!   `XDG_CONFIG_HOME` or `ENV`/`BASH_ENV`, and every history file through
//!   `HISTFILE`, `HOME` or `XDG_DATA_HOME`; all of them are the hygiene
//!   directory's (`ENV` and `BASH_ENV` empty), so the user's files are neither
//!   read nor written. On top of that `bash` is given `--norc --noprofile`,
//!   `zsh` `-f` and `fish` `--no-config` — unless the test's subject *is* the
//!   startup files ([`Hygiene::reading_startup_files`]), in which case the ones
//!   it reads are the ones it wrote into its own temporary `HOME`.
//! * **`wsl`** is refused: it starts a shell inside a distribution, under the
//!   distribution's own home directory, which nothing on this side can move.
//! * **Anything else** (`node`, `git`, a test's own program) gets the
//!   environment alone.
//!
//! Every family also starts **without the variables Folio announces to a pane**
//! ([`PANE_ANNOUNCEMENTS`]), unless the test sets one itself: a test run from a
//! shell inside a Folio pane inherits `TERM_PROGRAM=Folio`, `FORCE_HYPERLINK`
//! and the rest, and a CI runner does not, so a gate that passes only because
//! its child inherited them passes for a reason CI does not have. A child on a
//! pseudoconsole still gets what `bt-pty` itself declares to every child
//! (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `COLORTERM`, `TERM`) — the
//! product's declaration, the same on every machine.
//!
//! Process starts that are not on a pseudoconsole — a one-shot script, a shell
//! fed on a pipe — take the same rule through [`Hygiene::command`].
//!
//! Tests only: this crate's own, and those of a crate that turns on the
//! `test-shell` feature on its dev-dependency on this one. A build of the
//! shipped program never has it.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use portable_pty::ExitStatus;

use crate::{
    OutputWake, PtyCommand, PtyError, PtySession, PtySize, RingStats, ShellEnvironment,
    ShellFallback, SystemShellEnvironment, upsert_environment,
};

/// Where a test's PowerShell startup script wants the history refusal, when not
/// first.
///
/// A PowerShell block comment, so a script that carries it is valid whether or
/// not it is replaced. The refusal goes where this stands, exactly once, or
/// first when the script does not carry it — which is right for every startup
/// script except one that must load a particular PSReadLine before anything
/// asks for one (`Import-Module` first, the refusal after it).
pub const HYGIENE: &str = "<#folio-test-shell-hygiene#>";

/// The exit code a PowerShell leaves with when its history refusal could not be
/// established.
pub const REFUSED_EXIT_CODE: u32 = 75;

/// How long [`TestShell::write`] waits for a PowerShell to say its history is
/// off. Not a measure of anything: the startup script either finishes, fails
/// (and the shell exits), or the shell exits — every one of those ends the
/// wait. This is the backstop for a shell that does none of them, set where the
/// tests' own ceilings are (`bt-app`'s `real_powershell_input_reaches_a_viewport_owned_frame`).
const ESTABLISH_CEILING: Duration = Duration::from_secs(180);
const ESTABLISH_POLL: Duration = Duration::from_millis(10);

/// **The variables Folio announces to a pane by name** — the ones a process
/// started from inside a Folio pane inherits and a CI runner does not.
///
/// `bt-pty`'s own declarations to every child (`PtyCommand`'s environment
/// layers: [`crate::TERM_PROGRAM`]'s variable, its version, `COLORTERM`,
/// `TERM`) and `bt-app`'s per-pane ones: the hyperlink capability, the
/// attention wire's three, and the shell-integration marker and zsh bridge.
/// `bt-app` pins its names to this list
/// (`shell_integration::tests::every_pane_announcement_is_one_the_test_shell_strips`).
/// A `WSLENV` entry that forwards one of these is an announcement too, and is
/// dropped from the list it stands in. Values Folio derives from the user's own
/// (`PROMPT`, `WSLENV`'s other entries, the locale, `ZDOTDIR`) are the user's,
/// and are left as they are.
pub const PANE_ANNOUNCEMENTS: [&str; 10] = [
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    "TERM",
    "FORCE_HYPERLINK",
    "FOLIO_PANE",
    "FOLIO_ATTENTION",
    "FOLIO_ATTENTION_PIPE",
    "BT_SHELL_INTEGRATION",
    "BT_USER_ZDOTDIR",
];

/// **What a PowerShell child must not inherit from the session that runs the tests**: the
/// module path. PowerShell 7 rewrites `PSModulePath` for every process it starts, and a Windows
/// PowerShell started from it then autoloads PowerShell 7's copy of
/// `Microsoft.PowerShell.Security` and fails — `Get-ExecutionPolicy` is not found (measured
/// 2026-10-04 from a PowerShell 7 session; a run from cmd or a CI step that is not PowerShell 7
/// does not see it). With the variable absent each edition computes its own default, which is
/// what a test means by "this PowerShell".
const POWERSHELL_INHERITED: [&str; 1] = ["PSModulePath"];

/// The names `family` is started without, on top of [`PANE_ANNOUNCEMENTS`].
fn not_inherited(family: Family) -> &'static [&'static str] {
    match family {
        Family::PowerShell => &POWERSHELL_INHERITED,
        Family::Cmd | Family::Posix(_) | Family::Program => &[],
    }
}

fn is_pane_announcement(key: &OsStr) -> bool {
    PANE_ANNOUNCEMENTS
        .iter()
        .any(|name| crate::environment_key_eq(key, OsStr::new(name)))
}

/// `WSLENV` with every entry that forwards a pane announcement removed; `None`
/// when nothing is left.
fn wslenv_without_announcements(value: &OsStr) -> Option<OsString> {
    let value = value.to_string_lossy();
    let kept = value
        .split(':')
        .filter(|entry| {
            let name = entry.split('/').next().unwrap_or_default();
            !name.is_empty() && !is_pane_announcement(OsStr::new(name))
        })
        .collect::<Vec<_>>();
    (!kept.is_empty()).then(|| kept.join(":").into())
}

/// `environment` as a CI runner would hand it on: without the pane
/// announcements, and with `WSLENV` no longer forwarding them.
pub fn without_pane_announcements(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(key, _)| !is_pane_announcement(key))
        .filter_map(|(key, value)| {
            if crate::environment_key_eq(&key, OsStr::new("WSLENV")) {
                wslenv_without_announcements(&value).map(|value| (key, value))
            } else {
                Some((key, value))
            }
        })
        .collect()
}

/// The family a program belongs to, by its file name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    PowerShell,
    Cmd,
    Posix(Posix),
    /// A program that is not a shell: it gets the environment alone.
    Program,
}

/// Which POSIX shell, for the one flag each has that skips its startup files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Posix {
    Bash,
    Zsh,
    Fish,
    /// `sh`, `dash`, `ksh`, `mksh`: no such flag, and nothing to find but `ENV`.
    Bourne,
}

impl Family {
    /// The family of `program`, read off its name ([`program_name`]); `wsl` is
    /// refused here, with the reason.
    #[must_use]
    pub fn of(program: &OsStr) -> Self {
        let stem = program_name(program);
        match stem.as_deref() {
            Some("powershell" | "pwsh") => Self::PowerShell,
            Some("cmd") => Self::Cmd,
            Some("bash") => Self::Posix(Posix::Bash),
            Some("zsh") => Self::Posix(Posix::Zsh),
            Some("fish") => Self::Posix(Posix::Fish),
            Some("sh" | "dash" | "ksh" | "mksh") => Self::Posix(Posix::Bourne),
            Some("wsl") => panic!(
                "a test cannot start {}: WSL runs a shell inside a distribution, under that \
                 distribution's own home directory, and nothing on this side can give it a \
                 temporary one — so its startup files and its history would be the user's",
                Path::new(program).display()
            ),
            _ => Self::Program,
        }
    }

    /// The flags that skip the user's startup files, for a family that has
    /// them. Empty for one whose isolation is the environment alone.
    fn startup_flags(self) -> &'static [&'static str] {
        match self {
            Self::PowerShell => &["-NoProfile"],
            Self::Cmd => &["/D"],
            Self::Posix(Posix::Bash) => &["--norc", "--noprofile"],
            Self::Posix(Posix::Zsh) => &["-f"],
            Self::Posix(Posix::Fish) => &["--no-config"],
            Self::Posix(Posix::Bourne) | Self::Program => &[],
        }
    }
}

/// **The name a program is known by**, the same on every platform: the last
/// component of its spelling with either separator — `/` or `\` — taken as
/// one, without its extension, lower-cased.
///
/// Not `Path::file_stem`: off Windows a backslash is an ordinary file-name
/// character, so `C:\Windows\System32\wsl.exe` would be one name there and
/// `wsl` here, and the helper's contract — what it refuses, what flags it
/// gives — is about the program a test names, not about the host reading the
/// name (CI run 37221762304: refused on Windows, admitted on macOS and Linux).
#[must_use]
pub fn program_name(program: &OsStr) -> Option<String> {
    let spelled = program.to_string_lossy();
    let last = spelled.rsplit(['/', '\\']).next()?;
    let stem = match last.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => last,
    };
    (!stem.is_empty()).then(|| stem.to_ascii_lowercase())
}

/// **This test's stand-in for the account**: a temporary directory that holds
/// every per-user location a shell would write to, removed when it is dropped.
#[derive(Debug)]
pub struct Hygiene {
    root: PathBuf,
    reads_startup_files: bool,
    powershell_shape: Option<PowerShellShape>,
}

/// **A PowerShell command line's shape, as an authoritative classifier read it** — for a test
/// whose subject is the argv itself (a product-composed row), so the helper does not read the
/// argv a second, different way.
///
/// `bt-app`'s `shell_integration::classify_powershell_arguments` reads each edition's own
/// option table, abbreviations included (`-noe -c`, the genuine Visual Studio row); this
/// crate's reading ([`Hygiene::prepare`] without a shape) knows the full names and the
/// documented aliases and refuses an abbreviation. A test that has the better reading hands
/// it over here ([`Hygiene::with_powershell_shape`]) rather than keeping two.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerShellShape {
    /// The shell will draw a prompt — `-NoExit`, or no script at all — so a line editor will
    /// read what is typed and the history refusal is owed. `false` is a one-shot: no refusal,
    /// no gate, the argv as given.
    pub draws_a_prompt: bool,
    /// The index (in the argv as the test gives it) of the startup text the refusal goes into:
    /// where it carries [`HYGIENE`], or first. `None` for a startup the argv names but does not
    /// spell — a `-File` row — which must then carry [`powershell_hygiene`] itself (the test
    /// writes that file); the argv stays exactly as given, and [`TestShell::write`] still types
    /// nothing until the proof is there.
    pub startup_text: Option<usize>,
    /// The argv already says `-NoProfile`, in any spelling; when it does not, `-NoProfile` is
    /// put first.
    pub has_no_profile: bool,
}

impl Default for Hygiene {
    fn default() -> Self {
        Self::new()
    }
}

impl Hygiene {
    /// A fresh directory, with every location below it already made.
    #[must_use]
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "folio-test-shell-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // The filesystem is the boundary: a run killed before its `Drop` leaves
        // this name behind, and what it left must not be read as this run's.
        let _ = std::fs::remove_dir_all(&root);
        let hygiene = Self {
            root,
            reads_startup_files: false,
            powershell_shape: None,
        };
        for directory in hygiene.directories() {
            std::fs::create_dir_all(&directory).unwrap_or_else(|error| {
                panic!(
                    "the test shell's directory {} cannot be made: {error}",
                    directory.display()
                )
            });
        }
        hygiene
    }

    /// For a test whose subject is a POSIX shell's startup files: the shell is
    /// not told to skip them, and the ones it finds are the ones the test wrote
    /// under [`Self::home`] — `HOME`, `ZDOTDIR` and `XDG_CONFIG_HOME` are still
    /// this directory's. PowerShell's `-NoProfile` and cmd's `/D` stay: the
    /// startup files they skip are found where no test can put its own.
    #[must_use]
    pub fn reading_startup_files(mut self) -> Self {
        self.reads_startup_files = true;
        self
    }

    /// For a PowerShell whose command line a test has classified itself: the helper takes
    /// `shape` instead of reading the argv ([`PowerShellShape`]).
    #[must_use]
    pub fn with_powershell_shape(mut self, shape: PowerShellShape) -> Self {
        self.powershell_shape = Some(shape);
        self
    }

    /// The directory itself. A test that places files a shell will read puts
    /// them below it.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `HOME`, and `ZDOTDIR`.
    #[must_use]
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The history file a POSIX shell is handed and the path PSReadLine is
    /// pointed at. Nothing writes it unless a test asks a shell to.
    #[must_use]
    pub fn history_file(&self) -> PathBuf {
        self.root.join("history")
    }

    fn proof_file(&self) -> PathBuf {
        self.root.join("history-refusal.proof")
    }

    fn refusal_file(&self) -> PathBuf {
        self.root.join("history-refusal.failed")
    }

    fn directories(&self) -> Vec<PathBuf> {
        let mut directories = vec![self.home()];
        directories.extend(self.xdg().into_iter().map(|(_, path)| path));
        directories.extend(self.application_data().into_iter().map(|(_, path)| path));
        directories
    }

    fn xdg(&self) -> [(&'static str, PathBuf); 4] {
        let xdg = self.root.join("xdg");
        [
            ("XDG_CONFIG_HOME", xdg.join("config")),
            ("XDG_DATA_HOME", xdg.join("data")),
            ("XDG_STATE_HOME", xdg.join("state")),
            ("XDG_CACHE_HOME", xdg.join("cache")),
        ]
    }

    /// Windows' two per-user data directories, as far as a program finds them
    /// through the environment. Off Windows nothing reads these names.
    fn application_data(&self) -> Vec<(&'static str, PathBuf)> {
        if cfg!(windows) {
            let data = self.root.join("appdata");
            vec![
                ("APPDATA", data.join("roaming")),
                ("LOCALAPPDATA", data.join("local")),
            ]
        } else {
            Vec::new()
        }
    }

    /// Every variable this family is started with, and whether a test may set
    /// it itself (to a path inside [`Self::root`]) — `false` for the values
    /// that are not locations.
    fn environment(&self, family: Family) -> Vec<(&'static str, OsString, bool)> {
        let mut environment: Vec<(&'static str, OsString, bool)> =
            vec![("HOME", self.home().into_os_string(), true)];
        if cfg!(windows) {
            // What a Windows program that does not read `HOME` finds a home by (Git, Node,
            // a profile row's own scripts). PowerShell's `$PROFILE` is not among them: that is a
            // known folder no variable moves, which is why `-NoProfile` stays.
            environment.push(("USERPROFILE", self.home().into_os_string(), true));
        }
        environment.extend(
            self.xdg()
                .into_iter()
                .chain(self.application_data())
                .map(|(key, path)| (key, path.into_os_string(), true)),
        );
        match family {
            Family::PowerShell => {
                let cache = self
                    .application_data()
                    .last()
                    .map_or_else(|| self.root.clone(), |(_, local)| local.clone())
                    .join("ModuleAnalysisCache");
                environment.push(("PSModuleAnalysisCachePath", cache.into_os_string(), true));
                environment.push(("POWERSHELL_TELEMETRY_OPTOUT", "1".into(), false));
                environment.push(("POWERSHELL_UPDATECHECK", "Off".into(), false));
            }
            Family::Posix(_) => {
                environment.push(("ZDOTDIR", self.home().into_os_string(), true));
                environment.push(("HISTFILE", self.history_file().into_os_string(), true));
                environment.push(("ENV", OsString::new(), true));
                environment.push(("BASH_ENV", OsString::new(), true));
            }
            Family::Cmd | Family::Program => {}
        }
        environment
    }

    /// Whether a value a test chose for one of the hygiene's own variables keeps
    /// it inside this test: a location below [`Self::root`], or nothing at all.
    fn keeps_inside(&self, value: &OsStr) -> bool {
        value.is_empty() || Path::new(value).starts_with(&self.root)
    }

    /// `arguments` with this family's startup flags in front, each only if the
    /// caller did not already write it.
    fn flagged(&self, family: Family, arguments: Vec<OsString>) -> Vec<OsString> {
        let skips_startup =
            matches!(family, Family::PowerShell | Family::Cmd) || !self.reads_startup_files;
        let mut flagged: Vec<OsString> = if skips_startup {
            family
                .startup_flags()
                .iter()
                .filter(|flag| match family {
                    Family::PowerShell => !match self.powershell_shape {
                        Some(shape) => shape.has_no_profile,
                        None => {
                            powershell_switches(&arguments).contains(&PowerShellSwitch::NoProfile)
                        }
                    },
                    _ => !arguments
                        .iter()
                        .any(|argument| argument.to_string_lossy().eq_ignore_ascii_case(flag)),
                })
                .map(OsString::from)
                .collect()
        } else {
            Vec::new()
        };
        flagged.extend(arguments);
        flagged
    }

    /// `command`, rewritten to the rule for its program's family, and what has to
    /// be established before anything is typed into it.
    fn prepare(&self, mut command: PtyCommand) -> (PtyCommand, Gate) {
        let family = Family::of(&command.program);
        let given = command.arguments.len();
        let arguments = self.flagged(family, std::mem::take(&mut command.arguments));
        let flags = arguments.len() - given;
        let (arguments, gate) = match family {
            Family::PowerShell => match self.powershell_shape {
                Some(shape) => self.shaped_powershell_arguments(arguments, shape, flags),
                None => self.powershell_arguments(arguments),
            },
            Family::Cmd | Family::Posix(_) | Family::Program => (arguments, Gate::Open),
        };
        command.arguments = arguments;
        // The child's base block is this process's, without the pane announcements: through the
        // refresh seam `PtySession::spawn` clears the inherited block and starts from this one,
        // then lays the command's own declarations — and so anything the test set — over it.
        command.environment_refresh = Some(match command.environment_refresh.take() {
            Some(refresh) => crate::EnvironmentRefresh::new(
                without_pane_announcements(refresh.fresh),
                without_pane_announcements(refresh.launch_snapshot),
                without_pane_announcements(refresh.inherited),
            ),
            None => {
                let inherited = without_pane_announcements(std::env::vars_os());
                crate::EnvironmentRefresh::new(inherited.clone(), inherited.clone(), inherited)
            }
        });
        if let Some(refresh) = command.environment_refresh.as_mut() {
            for list in [
                &mut refresh.fresh,
                &mut refresh.launch_snapshot,
                &mut refresh.inherited,
            ] {
                list.retain(|(key, _)| {
                    !not_inherited(family)
                        .iter()
                        .any(|name| crate::environment_key_eq(key, OsStr::new(name)))
                });
            }
        }
        for (key, value, settable) in self.environment(family) {
            let chosen = command
                .environment
                .iter()
                .chain(&command.profile_environment)
                .find(|(existing, _)| crate::environment_key_eq(existing, OsStr::new(key)))
                .map(|(_, value)| value.clone());
            match chosen {
                Some(chosen) if settable && self.keeps_inside(&chosen) => {}
                Some(chosen) if settable => panic!(
                    "a test's shell may not be started with {key}={}: that is a place the shell \
                     keeps per-user state, and it has to be inside the test's own directory {}",
                    Path::new(&chosen).display(),
                    self.root.display()
                ),
                Some(_) | None => upsert_environment(&mut command.environment, key.into(), value),
            }
        }
        (command, gate)
    }

    /// [`Self::powershell_arguments`] for a command line the test classified itself: the argv
    /// as given (after any `-NoProfile` put first, `flags` words), with the refusal in the
    /// startup text the shape names.
    fn shaped_powershell_arguments(
        &self,
        mut arguments: Vec<OsString>,
        shape: PowerShellShape,
        flags: usize,
    ) -> (Vec<OsString>, Gate) {
        if !shape.draws_a_prompt {
            return (arguments, Gate::Open);
        }
        if let Some(at) = shape.startup_text {
            let at = at + flags;
            let script = arguments[at].to_string_lossy().into_owned();
            arguments[at] = with_refusal(&script, &powershell_hygiene(self)).into();
        }
        (arguments, self.powershell_gate())
    }

    fn powershell_gate(&self) -> Gate {
        Gate::PowerShell {
            proof: self.proof_file(),
            refusal: self.refusal_file(),
            history: self.history_file(),
            established: false,
        }
    }

    /// A PowerShell's argument list with the history refusal in its startup
    /// script, when it is a PowerShell that will draw a prompt.
    fn powershell_arguments(&self, mut arguments: Vec<OsString>) -> (Vec<OsString>, Gate) {
        let switches = powershell_switches(&arguments);
        let stays_open = switches.contains(&PowerShellSwitch::NoExit);
        let script = switches.iter().position(|switch| {
            matches!(
                switch,
                PowerShellSwitch::Command
                    | PowerShellSwitch::EncodedCommand
                    | PowerShellSwitch::File
            )
        });
        let refusal = powershell_hygiene(self);
        match script {
            // A one-shot: no prompt, so no line editor and no history.
            Some(_) if !stays_open => return (arguments, Gate::Open),
            None => {
                if !stays_open {
                    arguments.push("-NoExit".into());
                }
                arguments.push("-Command".into());
                arguments.push(refusal.into());
            }
            Some(at) if switches[at] == PowerShellSwitch::Command => {
                assert_eq!(
                    arguments.len(),
                    at + 2,
                    "a test's interactive PowerShell takes its startup script as the one \
                     argument after -Command, so the history refusal has one place to go"
                );
                let script = arguments[at + 1].to_string_lossy().into_owned();
                arguments[at + 1] = with_refusal(&script, &refusal).into();
            }
            Some(at) => panic!(
                "an interactive PowerShell started with {} has no startup script the history \
                 refusal can be put in front of: write the startup as -Command",
                arguments[at].to_string_lossy()
            ),
        }
        (arguments, self.powershell_gate())
    }

    /// A process started off a pseudoconsole — a one-shot script, a shell fed on
    /// a pipe — under the same rule: `program`, made by `new`, with its family's
    /// startup flags already given and the hygiene environment. The caller adds
    /// its own arguments after them. The directory must outlive the child.
    ///
    /// `new` is the caller's own door: `std::process::Command::new` from an
    /// integration test, `bt_platform::quiet_command` from a test inside `src/`,
    /// where that is the only door (`no_command_is_built_outside_the_quiet_door`).
    /// This module builds no child of its own.
    ///
    /// No PowerShell history refusal is put in front of anything here: a
    /// PowerShell that draws a prompt is started on a pseudoconsole
    /// ([`TestShell`]), and one that does not has no line editor to save a line.
    /// A POSIX shell made interactive on a pipe (`bash -i`) does keep a history,
    /// and writes it to the `HISTFILE` this sets.
    pub fn command(
        &self,
        program: impl AsRef<OsStr>,
        new: impl FnOnce(OsString) -> std::process::Command,
    ) -> std::process::Command {
        let program = program.as_ref();
        let family = Family::of(program);
        let mut command = new(program.to_os_string());
        command.args(self.flagged(family, Vec::new()));
        // Without the pane announcements; a test that wants one sets it after this, and wins.
        for name in PANE_ANNOUNCEMENTS.iter().chain(not_inherited(family)) {
            command.env_remove(name);
        }
        match std::env::var_os("WSLENV") {
            Some(listed) => match wslenv_without_announcements(&listed) {
                Some(kept) => command.env("WSLENV", kept),
                None => command.env_remove("WSLENV"),
            },
            None => &mut command,
        };
        for (key, value, _) in self.environment(family) {
            command.env(key, value);
        }
        command
    }
}

impl Drop for Hygiene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The switches a PowerShell command line's shape depends on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PowerShellSwitch {
    NoExit,
    NoProfile,
    Command,
    EncodedCommand,
    File,
    /// Any other switch, or a switch's value.
    Other,
}

/// Each argument of a PowerShell command line as a switch, up to and including the one that
/// starts its script (`-Command`, `-EncodedCommand`, `-File`) — what follows that is the
/// script's, not PowerShell's.
///
/// Read the way PowerShell reads them: a `-`, `--` or `/` in front, any case, the full name or
/// one of the documented aliases (`-c`, `-e`, `-ec`, `-f`). PowerShell also takes any
/// unambiguous abbreviation (`-noe`, `-noprof`, `-comm`); an abbreviation of one of the five
/// switches that decide whether this shell will draw a prompt, or has a profile, is refused
/// here rather than guessed at, because a misread one would open the history gate on a shell
/// that draws a prompt.
fn powershell_switches(arguments: &[OsString]) -> Vec<PowerShellSwitch> {
    const NAMED: [(&str, PowerShellSwitch); 9] = [
        ("noexit", PowerShellSwitch::NoExit),
        ("noprofile", PowerShellSwitch::NoProfile),
        ("command", PowerShellSwitch::Command),
        ("c", PowerShellSwitch::Command),
        ("encodedcommand", PowerShellSwitch::EncodedCommand),
        ("e", PowerShellSwitch::EncodedCommand),
        ("ec", PowerShellSwitch::EncodedCommand),
        ("file", PowerShellSwitch::File),
        ("f", PowerShellSwitch::File),
    ];
    let mut switches = Vec::new();
    for argument in arguments {
        let spelled = argument.to_string_lossy();
        let name = spelled
            .strip_prefix("--")
            .or_else(|| spelled.strip_prefix('-'))
            .or_else(|| spelled.strip_prefix('/'))
            .map(str::to_ascii_lowercase);
        let switch = match name {
            Some(name) if !name.is_empty() => match NAMED.iter().find(|(full, _)| *full == name) {
                Some((_, switch)) => *switch,
                None => {
                    if let Some((full, _)) = NAMED
                        .iter()
                        .find(|(full, _)| full.len() > 2 && full.starts_with(name.as_str()))
                    {
                        panic!(
                            "a test's PowerShell is started with {spelled:?}, which PowerShell \
                             reads as an abbreviation of -{full}: spell it in full, so the \
                             helper reads the command line the way PowerShell does"
                        );
                    }
                    PowerShellSwitch::Other
                }
            },
            _ => PowerShellSwitch::Other,
        };
        switches.push(switch);
        if matches!(
            switch,
            PowerShellSwitch::Command | PowerShellSwitch::EncodedCommand | PowerShellSwitch::File
        ) {
            break;
        }
    }
    switches
}

/// `script` with the history refusal where it carries [`HYGIENE`], or first.
fn with_refusal(script: &str, refusal: &str) -> String {
    match script.matches(HYGIENE).count() {
        0 => format!("{refusal} {script}"),
        1 => script.replacen(HYGIENE, refusal, 1),
        many => panic!(
            "the startup script names HYGIENE {many} times; the history refusal goes in one place"
        ),
    }
}

/// `text` as a PowerShell single-quoted literal. PowerShell reads the four
/// typographic single quotes as quotes too, so each of them is doubled as well.
fn powershell_literal(text: &str) -> String {
    let mut literal = String::from("'");
    for character in text.chars() {
        if matches!(
            character,
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}'
        ) {
            literal.push(character);
        }
        literal.push(character);
    }
    literal.push('\'');
    literal
}

/// The history refusal a test's interactive PowerShell runs before its first
/// prompt: history saving off and its file inside the hygiene directory, both
/// read back and written to the proof file — or, when any step fails, the
/// reason written to the refusal file and then to the console, and the shell gone.
///
/// The console line is what a test sees when it is waiting for a prompt that
/// will never come: written only to a file, a refusal is an empty screen and a
/// silence budget running out (CI run 37221762304), and the reason is lost.
///
/// Only the PSReadLine cmdlets the old startup scripts already called, and the
/// .NET file calls, in a full language mode — no other module command, so the
/// refusal adds no module load to the shell it runs in; the `Set-Content` /
/// `Move-Item` arm is for a constrained language mode, where a .NET call is
/// refused. Single quotes only, because Windows PowerShell's own command-line
/// parsing mangles a double quote inside `-Command`. The proof is written beside
/// its name and moved into place, so a reader never sees half of it.
#[must_use]
pub fn powershell_hygiene(hygiene: &Hygiene) -> String {
    let history = powershell_literal(&hygiene.history_file().to_string_lossy());
    let proof = hygiene.proof_file();
    let partial = powershell_literal(&format!("{}.part", proof.to_string_lossy()));
    let proof = powershell_literal(&proof.to_string_lossy());
    let refusal = powershell_literal(&hygiene.refusal_file().to_string_lossy());
    format!(
        "try {{ Set-PSReadLineOption -HistorySaveStyle SaveNothing -HistorySavePath {history} \
         -ErrorAction Stop; $__FolioTestShell = Get-PSReadLineOption -ErrorAction Stop; \
         $__FolioTestShell = ([string]$__FolioTestShell.HistorySaveStyle) + [char]10 + \
         [string]$__FolioTestShell.HistorySavePath; \
         if ($ExecutionContext.SessionState.LanguageMode -eq 'FullLanguage') {{ \
         [IO.File]::WriteAllText({partial}, $__FolioTestShell); [IO.File]::Move({partial}, {proof}) \
         }} else {{ Set-Content -LiteralPath {partial} -Value $__FolioTestShell -NoNewline \
         -Encoding UTF8 -ErrorAction Stop; Move-Item -LiteralPath {partial} -Destination {proof} \
         -Force -ErrorAction Stop }}; $__FolioTestShell = $null }} catch {{ \
         if ($ExecutionContext.SessionState.LanguageMode -eq 'FullLanguage') {{ \
         [IO.File]::WriteAllText({refusal}, [string]$_) }} else {{ Set-Content -LiteralPath \
         {refusal} -Value ([string]$_) -Encoding UTF8 -ErrorAction SilentlyContinue }}; \
         'folio test shell: the history refusal failed, so this shell will not be typed into: ' + \
         [string]$_; exit {REFUSED_EXIT_CODE} }};"
    )
}

/// What has to be true before a test may type.
#[derive(Debug)]
enum Gate {
    /// Nothing: no line editor here keeps a history the user owns.
    Open,
    /// A PowerShell that will draw a prompt: its history refusal, read back.
    PowerShell {
        proof: PathBuf,
        refusal: PathBuf,
        history: PathBuf,
        established: bool,
    },
}

/// Two spellings of one file: the same directory, as the file system resolves
/// it, and the same name.
fn same_file_location(left: &Path, right: &Path) -> bool {
    let resolve = |path: &Path| {
        let directory = path.parent()?.canonicalize().ok()?;
        Some((directory, path.file_name()?.to_os_string()))
    };
    matches!((resolve(left), resolve(right)), (Some(left), Some(right)) if left == right)
}

/// **The one door a test starts a real shell on a pseudoconsole through.**
///
/// The session is not handed out: [`Self::write`] is typing and waits for the
/// history refusal, [`Self::reply`] is a terminal's answer to a query and does
/// not, and everything else a test asks of the session is forwarded.
pub struct TestShell {
    // Declared first, so the child is ended before the directory it may still be
    // writing into is removed.
    session: PtySession,
    gate: Gate,
    program: OsString,
    hygiene: Hygiene,
}

/// A process-starting door with the one `spawn` it makes per attempt handed in.
type Door<'a> = dyn FnOnce(
        &mut dyn FnMut(PtyCommand, PtySize, OutputWake) -> Result<PtySession, PtyError>,
    ) -> Result<PtySession, PtyError>
    + 'a;

impl TestShell {
    /// `command`, rewritten to the rule for its family, started in a fresh
    /// [`Hygiene`].
    pub fn spawn(command: PtyCommand, size: PtySize) -> Result<Self, PtyError> {
        Self::spawn_in(Hygiene::new(), command, size)
    }

    /// [`Self::spawn`] in a [`Hygiene`] the test prepared — files of its own
    /// placed below it, or [`Hygiene::reading_startup_files`].
    pub fn spawn_in(
        hygiene: Hygiene,
        command: PtyCommand,
        size: PtySize,
    ) -> Result<Self, PtyError> {
        Self::through(
            hygiene,
            Box::new(move |spawn| spawn(command, size, quiet())),
        )
    }

    /// The default shell, resolved and started the way
    /// [`PtySession::spawn_default`] does it — the fallback included — with every
    /// attempt under the rule.
    pub fn spawn_default(size: PtySize) -> Result<Self, PtyError> {
        Self::spawn_default_with(size, &SystemShellEnvironment)
    }

    /// [`Self::spawn_default`] with the resolution's environment handed in.
    pub(crate) fn spawn_default_with(
        size: PtySize,
        environment: &dyn ShellEnvironment,
    ) -> Result<Self, PtyError> {
        Self::through(
            Hygiene::new(),
            Box::new(move |spawn| {
                PtySession::spawn_default_with(size, quiet(), None, environment, spawn)
            }),
        )
    }

    /// `program` with `arguments` and `environment`, started the way
    /// [`PtySession::spawn_shell_in`] does it — the fallback to the last-resort
    /// shell included — with every attempt under the rule.
    pub fn spawn_shell_in(
        hygiene: Hygiene,
        program: impl Into<OsString>,
        arguments: &[OsString],
        fallback_arguments: &dyn Fn() -> Vec<OsString>,
        environment: &[(OsString, OsString)],
        size: PtySize,
        working_directory: Option<PathBuf>,
    ) -> Result<Self, PtyError> {
        let program = program.into();
        Self::through(
            hygiene,
            Box::new(move |spawn| {
                PtySession::spawn_shell_in_with(
                    program,
                    arguments,
                    fallback_arguments,
                    environment,
                    size,
                    quiet(),
                    working_directory,
                    spawn,
                )
            }),
        )
    }

    fn through(hygiene: Hygiene, door: Box<Door<'_>>) -> Result<Self, PtyError> {
        let mut started = None;
        let session = door(&mut |command, size, wake| {
            let (command, gate) = hygiene.prepare(command);
            let program = command.program.clone();
            let session = PtySession::spawn(command, size, wake)?;
            started = Some((gate, program));
            Ok(session)
        })?;
        let (gate, program) = started.expect("a door that answered a session spawned one");
        Ok(Self {
            session,
            gate,
            program,
            hygiene,
        })
    }

    /// The program that was started, after any fallback.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// The directory standing in for the account.
    #[must_use]
    pub fn hygiene(&self) -> &Hygiene {
        &self.hygiene
    }

    /// **Type** `bytes` into the shell — once its history refusal is
    /// established, and never otherwise.
    ///
    /// # Panics
    ///
    /// When the shell said it could not refuse history, read back something
    /// other than what it was told, or ended without saying anything: nothing is
    /// typed, and the message says which.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        self.establish();
        self.session.write(bytes)
    }

    /// A terminal's **answer** to something the child asked — a cursor report,
    /// a colour report — which the child may be waiting on before it can reach
    /// the point where its history refusal is established. Never typing.
    pub fn reply(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.session.write(bytes)
    }

    /// The session itself, for a product function that takes one — under the
    /// same condition as [`Self::write`], because whoever holds it can type.
    pub fn session_mut(&mut self) -> &mut PtySession {
        self.establish();
        &mut self.session
    }

    fn establish(&mut self) {
        let Gate::PowerShell {
            proof,
            refusal,
            history,
            established,
        } = &mut self.gate
        else {
            return;
        };
        if *established {
            return;
        }
        let program = Path::new(&self.program).display().to_string();
        let started = Instant::now();
        loop {
            if let Ok(said) = std::fs::read_to_string(&*proof) {
                let said = said.trim_start_matches('\u{feff}');
                let (style, path) = said.split_once('\n').unwrap_or((said, ""));
                assert!(
                    style == "SaveNothing" && same_file_location(Path::new(path), history),
                    "{program} was told to save no history and to keep its history file at {}, \
                     and read back style {style:?} and file {path:?}; nothing was typed into it",
                    history.display()
                );
                *established = true;
                return;
            }
            if let Ok(reason) = std::fs::read_to_string(&*refusal) {
                panic!(
                    "{program} could not turn its history saving off, so nothing was typed into \
                     it; it said: {}",
                    reason.trim_start_matches('\u{feff}').trim()
                );
            }
            match self.session.try_wait() {
                Ok(Some(status)) => panic!(
                    "{program} ended ({status:?}) before it said its history saving was off, so \
                     nothing was typed into it"
                ),
                Ok(None) => {}
                Err(error) => panic!(
                    "{program} could not be asked whether it is still running ({error}) before \
                     it said its history saving was off, so nothing was typed into it"
                ),
            }
            assert!(
                started.elapsed() < ESTABLISH_CEILING,
                "{program} neither said its history saving was off nor ended within {:?}, so \
                 nothing was typed into it",
                ESTABLISH_CEILING
            );
            std::thread::sleep(ESTABLISH_POLL);
        }
    }

    /// **What the shell's hygiene and the shell itself have come to**, for a test's message
    /// when it gives up waiting: whether the history refusal was established, what the proof
    /// says, or the reason the shell gave for refusing; and whether the shell has ended, with
    /// its exit code. A wait that ran out on an empty screen says nothing else.
    pub fn account(&mut self) -> String {
        let program = Path::new(&self.program).display().to_string();
        let refusal = match &self.gate {
            Gate::Open => "not owed (no line editor here keeps a history)".to_owned(),
            Gate::PowerShell {
                established: true, ..
            } => "established".to_owned(),
            Gate::PowerShell { proof, refusal, .. } => {
                if let Ok(reason) = std::fs::read_to_string(refusal) {
                    format!("refused: {}", reason.trim_start_matches('\u{feff}').trim())
                } else if let Ok(said) = std::fs::read_to_string(proof) {
                    format!("written, reading {:?}", said.trim_start_matches('\u{feff}'))
                } else {
                    "not yet written (the startup script has not reached it)".to_owned()
                }
            }
        };
        let child = match self.session.try_wait() {
            Ok(Some(status)) if status.exit_code() == REFUSED_EXIT_CODE => {
                format!("ended with exit code {REFUSED_EXIT_CODE}, the history refusal's own")
            }
            Ok(Some(status)) => format!("ended with exit code {}", status.exit_code()),
            Ok(None) => "is still running".to_owned(),
            Err(error) => format!("cannot be asked whether it is running ({error})"),
        };
        format!("{program}: history refusal {refusal}; the shell {child}")
    }

    pub fn read_output(&self) -> Vec<u8> {
        self.session.read_output()
    }

    pub fn output_is_drained(&self) -> bool {
        self.session.output_is_drained()
    }

    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        self.session.resize(size)
    }

    pub fn size(&self) -> Result<PtySize, PtyError> {
        self.session.size()
    }

    pub fn clear_host_buffer(&self, keep_cursor_row: bool) -> Result<bool, PtyError> {
        self.session.clear_host_buffer(keep_cursor_row)
    }

    pub fn ring_stats(&self) -> RingStats {
        self.session.ring_stats()
    }

    #[must_use]
    pub fn child_id(&self) -> Option<u32> {
        self.session.child_id()
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        self.session.try_wait()
    }

    pub fn shutdown(&mut self) -> Result<Option<ExitStatus>, PtyError> {
        self.session.shutdown()
    }

    pub fn take_shell_fallback(&mut self) -> Option<ShellFallback> {
        self.session.take_shell_fallback()
    }

    #[must_use]
    pub fn conpty_kind(&self) -> crate::ConPtyKind {
        self.session.conpty_kind()
    }

    #[must_use]
    pub fn inbox_conpty_reason(&self) -> Option<String> {
        self.session.inbox_conpty_reason()
    }
}

/// A test reads its child's output by asking; nothing is woken.
fn quiet() -> OutputWake {
    Arc::new(|| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(command: &PtyCommand) -> Vec<String> {
        command
            .arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn value(command: &PtyCommand, key: &str) -> Option<OsString> {
        command
            .environment
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    /// PIN — **a test shell starts as a CI runner would, not as a Folio pane
    /// would**: the pane announcements are gone from every block it is started
    /// from, `WSLENV` no longer forwards them, and what a test sets itself is
    /// kept.
    ///
    /// RED (mutations: `is_pane_announcement` answers `false`; `Hygiene::command`
    /// removes nothing; `prepare` leaves `environment_refresh` as it found it;
    /// `not_inherited` answers `&[]` for PowerShell).
    #[test]
    fn a_test_shell_does_not_inherit_what_folio_announces_to_its_pane() {
        let block = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value)))
                .collect::<Vec<_>>()
        };
        let inherited = block(&[
            ("TERM_PROGRAM", "Folio"),
            ("TERM_PROGRAM_VERSION", "0.4.6"),
            ("COLORTERM", "truecolor"),
            ("TERM", "xterm-256color"),
            ("FORCE_HYPERLINK", "1"),
            ("FOLIO_PANE", "1.1"),
            ("FOLIO_ATTENTION", "capability"),
            ("FOLIO_ATTENTION_PIPE", "pipe"),
            ("BT_SHELL_INTEGRATION", "marker"),
            ("BT_USER_ZDOTDIR", "/zsh"),
            ("WSLENV", "USERPROFILE/p:TERM_PROGRAM/u:FORCE_HYPERLINK/u"),
            ("PATH", "kept"),
        ]);
        assert_eq!(
            without_pane_announcements(inherited),
            block(&[("WSLENV", "USERPROFILE/p"), ("PATH", "kept")])
        );
        assert_eq!(
            without_pane_announcements(block(&[("WSLENV", "TERM_PROGRAM/u")])),
            block(&[])
        );

        let hygiene = Hygiene::new();
        let command = hygiene.command("powershell.exe", std::process::Command::new);
        let removed = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_owned())
            .collect::<Vec<_>>();
        for name in PANE_ANNOUNCEMENTS {
            assert!(
                removed
                    .iter()
                    .any(|key| crate::environment_key_eq(key, OsStr::new(name))),
                "{name} is removed from a child off a pseudoconsole"
            );
        }
        // And a PowerShell child computes its own module path (`POWERSHELL_INHERITED`).
        assert!(
            removed
                .iter()
                .any(|key| crate::environment_key_eq(key, OsStr::new("PSModulePath"))),
            "a PowerShell child does not inherit the session's PSModulePath"
        );

        let (prepared, _) = hygiene.prepare(
            PtyCommand::new("powershell.exe")
                .arg("-NoProfile")
                .arg("-Command")
                .arg("exit")
                .env("TERM_PROGRAM", "a test's own"),
        );
        let refresh = prepared
            .environment_refresh
            .as_ref()
            .expect("a test shell starts from a cleaned block");
        for list in [&refresh.fresh, &refresh.launch_snapshot, &refresh.inherited] {
            assert!(
                list.iter().all(|(key, _)| !is_pane_announcement(key)),
                "{list:?}"
            );
        }
        assert_eq!(
            value(&prepared, "TERM_PROGRAM").as_deref(),
            Some(OsStr::new("a test's own")),
            "what the test sets itself is kept"
        );
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **each family is started without the user's startup files,
    /// with the hygiene directory standing in for every per-user location.**
    ///
    /// MUTATIONS: return `arguments` unflagged from `Hygiene::flagged` and the flags are missing;
    /// skip the environment loop in `Hygiene::prepare` and `HOME` is the account's.
    #[test]
    fn every_family_is_started_without_the_users_startup_files() {
        let hygiene = Hygiene::new();
        let inside = |key: &str, command: &PtyCommand| {
            let value = value(command, key).unwrap_or_else(|| panic!("{key} is set"));
            assert!(
                Path::new(&value).starts_with(hygiene.root()),
                "{key}={value:?} is outside {}",
                hygiene.root().display()
            );
        };

        let (cmd, gate) = hygiene.prepare(PtyCommand::new("cmd.exe").arg("/C").arg("echo hi"));
        assert_eq!(arguments(&cmd), ["/D", "/C", "echo hi"]);
        assert!(matches!(gate, Gate::Open));
        inside("HOME", &cmd);

        let (bash, _) = hygiene.prepare(PtyCommand::new("/usr/bin/bash").arg("-c").arg("true"));
        assert_eq!(arguments(&bash), ["--norc", "--noprofile", "-c", "true"]);
        for key in [
            "HOME",
            "ZDOTDIR",
            "HISTFILE",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ] {
            inside(key, &bash);
        }
        assert_eq!(value(&bash, "ENV"), Some(OsString::new()));
        assert_eq!(value(&bash, "BASH_ENV"), Some(OsString::new()));

        let (zsh, _) = hygiene.prepare(PtyCommand::new("/bin/zsh"));
        assert_eq!(arguments(&zsh), ["-f"]);

        let (sh, _) = hygiene.prepare(PtyCommand::new("/bin/sh").arg("-c").arg("true"));
        assert_eq!(
            arguments(&sh),
            ["-c", "true"],
            "a Bourne shell has no such flag"
        );
        inside("HISTFILE", &sh);

        let (node, gate) = hygiene.prepare(PtyCommand::new("node").arg("-e").arg("0"));
        assert_eq!(arguments(&node), ["-e", "0"]);
        assert!(matches!(gate, Gate::Open));
        inside("HOME", &node);
        if cfg!(windows) {
            inside("APPDATA", &node);
            inside("LOCALAPPDATA", &node);
        }
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **an interactive PowerShell has its history refusal first in
    /// its startup script, or where the script asks; a one-shot has none and needs none.**
    ///
    /// MUTATION: answer `Gate::Open` for every PowerShell in `Hygiene::powershell_arguments` and
    /// the interactive ones are not gated.
    #[test]
    fn an_interactive_powershell_refuses_history_before_its_startup_script() {
        let hygiene = Hygiene::new();
        let refusal = powershell_hygiene(&hygiene);

        let (bare, gate) = hygiene.prepare(PtyCommand::new("powershell.exe").arg("-NoLogo"));
        assert_eq!(
            arguments(&bare),
            [
                "-NoProfile",
                "-NoLogo",
                "-NoExit",
                "-Command",
                refusal.as_str()
            ]
        );
        assert!(matches!(gate, Gate::PowerShell { .. }));

        let (first, _) = hygiene.prepare(
            PtyCommand::new("pwsh")
                .arg("-NoExit")
                .arg("-Command")
                .arg("function global:prompt { 'P> ' }"),
        );
        assert_eq!(
            arguments(&first)[3],
            format!("{refusal} function global:prompt {{ 'P> ' }}")
        );

        let (placed, _) = hygiene.prepare(
            PtyCommand::new("powershell.exe")
                .arg("-NoProfile")
                .arg("-NoExit")
                .arg("-Command")
                .arg(format!("Import-Module PSReadLine; {HYGIENE} Write-Host x")),
        );
        assert_eq!(
            arguments(&placed),
            [
                "-NoProfile".to_owned(),
                "-NoExit".to_owned(),
                "-Command".to_owned(),
                format!("Import-Module PSReadLine; {refusal} Write-Host x"),
            ],
            "-NoProfile is not given twice, and the refusal stands where the script put it"
        );

        let (one_shot, gate) = hygiene.prepare(
            PtyCommand::new("powershell.exe")
                .arg("-Command")
                .arg("Start-Sleep 1"),
        );
        assert_eq!(
            arguments(&one_shot),
            ["-NoProfile", "-Command", "Start-Sleep 1"]
        );
        assert!(matches!(gate, Gate::Open));
        assert!(value(&one_shot, "PSModuleAnalysisCachePath").is_some());
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **a location a test chooses itself must be inside its own
    /// directory.**
    ///
    /// MUTATION: accept every caller-chosen value in `Hygiene::prepare` and no panic comes.
    #[test]
    #[should_panic(expected = "a test's shell may not be started with HOME=")]
    fn a_home_outside_the_test_is_refused() {
        let hygiene = Hygiene::new();
        let outside = hygiene.root().parent().unwrap().to_path_buf();
        let _ = hygiene.prepare(PtyCommand::new("/bin/sh").env("HOME", outside));
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **and one inside it is kept**: a test whose subject is the
    /// startup files puts them in its own `HOME`, and is not told to skip them.
    ///
    /// MUTATION: ignore `reads_startup_files` in `Hygiene::flagged` and `--norc` comes back.
    #[test]
    fn startup_files_a_test_wrote_into_its_own_home_are_read() {
        let hygiene = Hygiene::new().reading_startup_files();
        let home = hygiene.home();
        let zdotdir = hygiene.root().join("zdotdir");
        let (login, _) = hygiene.prepare(
            PtyCommand::new("/bin/bash")
                .arg("-l")
                .env("HOME", &home)
                .env("ZDOTDIR", &zdotdir),
        );
        assert_eq!(arguments(&login), ["-l"]);
        assert_eq!(value(&login, "HOME"), Some(home.into_os_string()));
        assert_eq!(value(&login, "ZDOTDIR"), Some(zdotdir.into_os_string()));
        let (powershell, _) =
            hygiene.prepare(PtyCommand::new("powershell.exe").arg("-Command").arg("1"));
        assert_eq!(
            arguments(&powershell)[0],
            "-NoProfile",
            "a profile is never a test's"
        );
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **WSL is refused**, by its name, on every platform: its
    /// shell's home is the distribution's. Spelled as a Windows path on purpose: off Windows a
    /// backslash is a file-name character, and CI run 37221762304 admitted this spelling on
    /// macOS and Linux while refusing it on Windows.
    ///
    /// MUTATIONS: map `wsl` to `Family::Program` and no panic comes; name the program with
    /// `Path::file_stem` in `program_name` and no panic comes off Windows.
    #[test]
    #[should_panic(expected = "WSL runs a shell inside a distribution")]
    fn wsl_has_no_hygienic_form() {
        let _ = Family::of(OsStr::new(r"C:\Windows\System32\wsl.exe"));
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **a program is named by its spelling, the same on every
    /// platform**: the last component under either separator, without its extension, in any
    /// case.
    ///
    /// MUTATIONS: name the program with `Path::file_stem` and the backslash rows fail off
    /// Windows; drop the lower-casing and the upper-case rows fail everywhere.
    #[test]
    fn a_program_is_named_the_same_on_every_platform() {
        for (spelled, name) in [
            (r"C:\Windows\System32\wsl.exe", "wsl"),
            ("/mnt/c/Windows/System32/WSL.EXE", "wsl"),
            (r"C:\Program Files\PowerShell\7\pwsh.exe", "pwsh"),
            ("/opt/microsoft/powershell/7/pwsh", "pwsh"),
            ("PowerShell.exe", "powershell"),
            (r"C:\WINDOWS\system32\CMD.EXE", "cmd"),
            ("/bin/zsh", "zsh"),
            ("bash", "bash"),
        ] {
            assert_eq!(
                program_name(OsStr::new(spelled)).as_deref(),
                Some(name),
                "{spelled}"
            );
        }
        assert_eq!(
            Family::of(OsStr::new(r"C:\Program Files\Git\bin\bash.exe")),
            Family::Posix(Posix::Bash)
        );
        assert_eq!(
            Family::of(OsStr::new(r"C:\WINDOWS\system32\CMD.EXE")),
            Family::Cmd
        );
    }

    /// RED (T-TEST-SHELL-HYGIENE review) — **a PowerShell switch is read in every spelling
    /// PowerShell reads it**: `/NoExit`, `--noexit`, `-NOEXIT` make a shell that draws a prompt,
    /// and `/NoProfile` is not given twice.
    ///
    /// MUTATION: strip only a leading `-` in `powershell_switches` and `/NoExit` reads as a
    /// one-shot whose gate is open.
    #[test]
    fn every_spelling_of_no_exit_is_an_interactive_powershell() {
        let hygiene = Hygiene::new();
        for spelled in ["/NoExit", "--noexit", "-NOEXIT"] {
            let (command, gate) = hygiene.prepare(
                PtyCommand::new("powershell.exe")
                    .arg(spelled)
                    .arg("/Command")
                    .arg("function global:prompt { 'P> ' }"),
            );
            assert!(
                matches!(gate, Gate::PowerShell { .. }),
                "{spelled} is a shell that draws a prompt"
            );
            assert!(
                arguments(&command)
                    .last()
                    .is_some_and(|script| script.starts_with("try {")),
                "{spelled}: the refusal is first in the script"
            );
        }
        let (command, _) = hygiene.prepare(
            PtyCommand::new("pwsh")
                .arg("/NoProfile")
                .arg("-Command")
                .arg("1"),
        );
        assert_eq!(arguments(&command), ["/NoProfile", "-Command", "1"]);
    }

    /// RED (T-TEST-SHELL-HYGIENE, merge with T-INTEGRATION-INJECT-3) — **a command line a test
    /// classified itself is taken as classified**: the genuine `-noe -c` row is not refused,
    /// the refusal goes where its startup text carries `HYGIENE`, a `-File` row's argv is left
    /// exactly as given yet still gated, a one-shot is untouched and ungated, and a missing
    /// `-NoProfile` is put first with the startup text's index moved past it.
    ///
    /// MUTATIONS: ignore the shape in `Hygiene::prepare` and `-noe` is refused as an
    /// abbreviation; drop the `flags` offset in `shaped_powershell_arguments` and the refusal
    /// lands in `-c` instead of its text.
    #[test]
    fn a_classified_command_line_is_taken_as_classified() {
        let shaped = |draws_a_prompt, startup_text, has_no_profile| {
            Hygiene::new().with_powershell_shape(PowerShellShape {
                draws_a_prompt,
                startup_text,
                has_no_profile,
            })
        };
        let hygiene = shaped(true, Some(3), true);
        let refusal = powershell_hygiene(&hygiene);
        let (command, gate) = hygiene.prepare(
            PtyCommand::new("powershell.exe")
                .arg("-NoProfile")
                .arg("-noe")
                .arg("-c")
                .arg(format!("{HYGIENE} Write-Output ready\r\nloader")),
        );
        assert_eq!(
            arguments(&command),
            [
                "-NoProfile".to_owned(),
                "-noe".to_owned(),
                "-c".to_owned(),
                format!("{refusal} Write-Output ready\r\nloader"),
            ]
        );
        assert!(matches!(gate, Gate::PowerShell { .. }));

        let hygiene = shaped(true, None, true);
        let row = ["-NoProfile", "-NoExit", "-File", "row.ps1"];
        let (command, gate) = hygiene.prepare(
            row.iter()
                .fold(PtyCommand::new("pwsh.exe"), |c, a| c.arg(a)),
        );
        assert_eq!(arguments(&command), row, "a -File row stays as written");
        assert!(
            matches!(gate, Gate::PowerShell { .. }),
            "and is still gated"
        );

        let hygiene = shaped(false, Some(2), true);
        let row = ["-NoProfile", "-Command", "Write-Output once"];
        let (command, gate) = hygiene.prepare(
            row.iter()
                .fold(PtyCommand::new("pwsh.exe"), |c, a| c.arg(a)),
        );
        assert_eq!(arguments(&command), row);
        assert!(matches!(gate, Gate::Open));

        let hygiene = shaped(true, Some(2), false);
        let refusal = powershell_hygiene(&hygiene);
        let (command, _) = hygiene.prepare(
            PtyCommand::new("powershell.exe")
                .arg("-noe")
                .arg("-c")
                .arg("Write-Output ready"),
        );
        assert_eq!(
            arguments(&command),
            [
                "-NoProfile".to_owned(),
                "-noe".to_owned(),
                "-c".to_owned(),
                format!("{refusal} Write-Output ready"),
            ]
        );
    }

    /// RED (T-TEST-SHELL-HYGIENE review) — **an abbreviated switch is refused, not guessed**:
    /// PowerShell reads `-noe` as `-NoExit`, and a helper that did not would open the gate on a
    /// shell that draws a prompt.
    ///
    /// MUTATION: drop the abbreviation check in `powershell_switches` and `-noe` reads as
    /// another switch, so no panic comes.
    #[test]
    #[should_panic(expected = "reads as an abbreviation of -noexit")]
    fn an_abbreviated_no_exit_is_refused() {
        let _ = Hygiene::new().prepare(
            PtyCommand::new("powershell.exe")
                .arg("-noe")
                .arg("-Command")
                .arg("1"),
        );
    }

    /// RED (T-TEST-SHELL-HYGIENE) — **off a pseudoconsole, the family's flags come first and the
    /// environment is the hygiene's.** Nothing is started.
    ///
    /// MUTATION: drop the `args` call in `Hygiene::command` and `-NoProfile` is gone.
    #[test]
    fn a_command_off_a_pseudoconsole_takes_the_same_rule() {
        let hygiene = Hygiene::new();
        let command = hygiene.command("powershell.exe", std::process::Command::new);
        let words: Vec<_> = command.get_args().collect();
        assert_eq!(words, ["-NoProfile"]);
        let home = command
            .get_envs()
            .find(|(key, _)| *key == "HOME")
            .and_then(|(_, value)| value)
            .expect("HOME is set");
        assert!(Path::new(home).starts_with(hygiene.root()));
    }
}
