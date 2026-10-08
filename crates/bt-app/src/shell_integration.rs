//! Handing a shell the script that makes it legible.
//!
//! `docs/shell-integration.md` describes what the markers mean; this module is
//! the one place that decides **how the script reaches the shell**, which is a
//! different question for every family and has exactly one right answer per
//! family:
//!
//! | profile | mechanism |
//! |---|---|
//! | PowerShell | `-NoExit -Command <guarded text loader>`, when the row's switches are safe |
//! | Git Bash | `bash --init-file <script> <its own words, less the login flag>` |
//! | a zsh | `ZDOTDIR`, pointed at a directory holding the script three times |
//! | WSL | `wsl.exe … -e sh -c <the login-shell question> folio <script> <zdotdir>` |
//! | Command Prompt | the `PROMPT` variable, carrying `OSC 7` and `OSC 133;D`/`;A` |
//! | `sh`, `dash`, anything else | none, and the pane's own state says so |
//!
//! PowerShell has no process-scoped init-file argument that runs after the
//! user's profile. Its explicit opt-in therefore edits that user's file. This
//! module records where the mark lands, migrates legacy marks at startup and
//! owns their removal. bash's `--init-file` touches no startup file on disk.
//!
//! zsh has neither: no `--init-file`, and a startup file it looks for in a
//! *directory* rather than at a path. So zsh's automatic install is that
//! directory — `ZDOTDIR` pointed at one of ours, holding `folio.zsh` under the
//! three names zsh reads out of it, each of which sources the reader's file of
//! the same name and hands `ZDOTDIR` back. `sh` and `dash` have no door at all,
//! and it is `--init-file` accepted-and-ignored that made that worth saying
//! outright (review row R3-6).
//!
//! `cmd.exe` has no startup file to name and no hook to install, so its whole
//! integration is a *format string* — see [`profiles::Integration::CmdPrompt`]
//! for why that string carries the two markers that describe the moment it is
//! expanded at, and neither of the two that would open a region it could never
//! close.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bt_platform::LocaleDeclaration;
use bt_pty::ShellEnvironment;

use crate::{
    persist,
    profiles::{self, Integration, Profile, windows_to_wsl},
};

pub mod profile_marks;
mod profile_runtime;
pub use profile_runtime::{
    ProfileInstallOutcome, begin_profile_install, begin_profile_install_undo,
    begin_profile_observation_for, begin_removal, begin_startup_migration, profile_installs_for,
    remove_shell_integration, remove_shell_integration_at, take_profile_installs, take_removal,
};

/// The script, compiled in.
///
/// Embedded rather than found next to the executable, and the reason is a
/// protocol one rather than a packaging one: the script and the terminal are two
/// halves of one agreement about what `OSC 133;D` means, and a build that could
/// load an older or newer half would be a build whose markers mean whatever
/// happens to be on disk. `include_str!` makes the two halves ship as one thing.
const SCRIPT: &str = include_str!("../../../scripts/shell-integration/folio.bash");

/// The name it is written under, in `%APPDATA%\Folio\`.
const SCRIPT_FILE: &str = "folio.bash";

/// zsh's script, under the same roof.
///
/// Not a second copy of bash's: zsh has no `--init-file`, so what it is handed
/// is a **directory** (see [`ZDOTDIR`]) holding this file under three names.
const SCRIPT_ZSH: &str = include_str!("../../../scripts/shell-integration/folio.zsh");

/// The directory `ZDOTDIR` is pointed at, under `%APPDATA%\Folio\`.
const ZDOTDIR_DIRECTORY: &str = "zdotdir";

/// The three names [`SCRIPT_ZSH`] is written under in that directory, and the
/// whole of what zsh will look for there before it has read a line.
///
/// `.zlogin` is deliberately absent: `.zshrc` hands `ZDOTDIR` back to the reader
/// at the end of itself, so zsh looks that one up in their own directory and
/// finds their own file. Adding a fourth here would take it away from them.
const ZDOTDIR_FILES: [&str; 3] = [".zshenv", ".zprofile", ".zshrc"];

/// The variable that tells the script it is being used as an init file, and is
/// therefore responsible for the startup chain `--init-file` displaced.
///
/// Its absence is equally meaningful: a hand-installed copy dot-sourced from the
/// user's own `~/.bashrc` must **not** source the login files, because bash
/// already did.
///
/// **Its value is the mode and not a bare `1`** (review row R3-8). bash has two
/// startup chains and reads exactly one of them: an interactive shell that is
/// not a login shell reads `~/.bashrc` alone, and a login shell reads
/// `/etc/profile` and then the first of `~/.bash_profile`, `~/.bash_login`,
/// `~/.profile` and never `~/.bashrc`. Which of the two the pane is owed is a
/// fact about the profile — whether its own arguments asked for a login shell —
/// and this side is the only one that can read it, so this side says it.
const INSTALLED_MARKER: &str = "BT_SHELL_INTEGRATION";

/// The reader asked for a login shell, and its chain is the script's to put back.
const MODE_LOGIN: &str = "login";

/// The reader asked for a plain interactive shell, whose one file is `~/.bashrc`.
const MODE_INTERACTIVE: &str = "interactive";

/// Where zsh reads its startup files from, and the whole of zsh's door.
const ZDOTDIR: &str = "ZDOTDIR";

/// Where the reader's own `ZDOTDIR` is carried, since the real one is taken.
///
/// `folio.zsh` reads it to find the files it has to put back. Unset when the
/// session had none, which is the same sentence as "their files are in `$HOME`".
const USER_ZDOTDIR: &str = "BT_USER_ZDOTDIR";

/// The variable `cmd.exe` prints its prompt from, and this build's only way in.
const CMD_PROMPT: &str = "PROMPT";

/// What a Rust CLI tool asks before it will print an `OSC 8` hyperlink — see
/// [`hyperlink_declaration`].
///
/// Public since §7.1.6c-6c, because the profile editor's `Force hyperlinks` row
/// is *this variable* read as one question and writes a row of this name into a
/// profile's environment. One constant rather than a second spelling in
/// `settings.rs`: a variable whose name is written twice is a variable that will
/// one day be written differently in the two places, and the symptom — a picker
/// that appears to do nothing — would look like a broken control rather than a
/// typo.
pub const FORCE_HYPERLINK: &str = "FORCE_HYPERLINK";

/// What `cmd.exe` prints when `PROMPT` is unset — its own documented default,
/// `<drive and path>` then `>`.
///
/// Spelled out rather than left to the default, because the moment this profile
/// sets `PROMPT` at all it owes the whole string: a prefix handed to a shell
/// whose `PROMPT` was empty would be the *entire* prompt, and the user would
/// lose the one thing every `cmd` prompt has ever shown.
const CMD_DEFAULT_PROMPT: &str = "$P$G";

/// The report, in the only alphabet `PROMPT` has.
///
/// `$e` is the escape character and `$P` the current drive and path — two of the
/// dozen-odd substitutions `cmd.exe` performs on this string, and the only two
/// that exist. `$e\` is therefore `ESC \`, the string terminator, and it is used
/// rather than `BEL` because `PROMPT` has no code that produces a `BEL` byte;
/// both terminators are accepted (`osc_7_reports_its_working_directory_uri_…`).
///
/// **The URI is Win32-spelled, and that is forced rather than chosen.** `$P`
/// expands to `D:\Developer\folio-terminal`, and `PROMPT` has no substitution,
/// no loop and no escape hatch that could turn those separators into `/` or
/// percent-encode a space — measured, not assumed: `cmd.exe` under ConPTY puts
/// `file:///C:\Program Files` on the wire for a directory with a space in it. So
/// the report says the directory in the spelling the shell can say it in, the
/// same principle Git Bash's `pwd -W` follows, and `file_uri_to_local_path`
/// accepts it because a backslash is not a path separator in a URI and never
/// splits a segment. That acceptance is pinned in `bt-term`
/// (`a_working_directory_may_be_spelled_the_way_a_windows_shell_can_spell_it`)
/// so that tightening the URI parser cannot silently blank every `cmd` pane's
/// directory — the failure would be invisible from inside that crate.
const CMD_OSC7: &str = r"$e]7;file:///$P$e\";

/// The two OSC 133 markers a format string can carry, and the whole of what
/// `cmd.exe` can say about a command boundary.
///
/// `D` ends the command the previous prompt started — bare, because `PROMPT`
/// has no substitution for `ERRORLEVEL` and a status this shell cannot read is
/// one it must not claim — and `A` opens the prompt about to be printed. Both
/// describe *this* moment, which is the one moment `PROMPT` is expanded at, and
/// that is why these two fit where `B` and `C` cannot: `B` would open an input
/// region whose only closer is a `C` this shell has no moment to send, and the
/// pane would spend every command's run with its output inside the region that
/// says "this is what the reader is typing". See
/// [`profiles::Integration::CmdPrompt`] for the whole of that reasoning and for
/// what the terminal had to learn before these two could be sent at all.
const CMD_MARKS_BEFORE_REPORT: &str = r"$e]133;D$e\";
const CMD_MARKS_AFTER_REPORT: &str = r"$e]133;A$e\";

/// The variables a WSL shell is given, listed for `WSLENV` so that they cross
/// the Win32/Linux boundary.
///
/// All four are the identity declarations `PtyCommand` already puts in every
/// child's environment; a WSL shell is the one child that could not see them,
/// because `wsl.exe` forwards nothing it was not told to. Forwarding them is not
/// this ticket inventing a capability — it is the same declaration every other
/// profile has always received, finally reaching the one that could not.
/// `FORCE_HYPERLINK` is listed whether or not this process sets it: the listing
/// forwards whatever value ends up on the Win32 side, so a user who set their
/// own answer has it carried into the distribution rather than overwritten by
/// its absence.
///
/// **[`INSTALLED_MARKER`] is deliberately not among them** (2026-09-07). It is
/// set inside the distribution, by the one branch of [`WSL_LOGIN_SHELL`] that
/// actually reads the init file, because it is the only variable here whose
/// meaning depends on which shell was started: a zsh session carrying
/// `BT_SHELL_INTEGRATION=1` for its whole life would tell every nested `bash`
/// that somebody had already run its startup files, and a reader with a
/// hand-installed copy of the script in their own `~/.bashrc` would have the
/// login chain sourced twice in every one of those shells.
const FORWARDED: [&str; 4] = [
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    FORCE_HYPERLINK,
];

/// **The login-shell question, asked by the pane that needs the answer.**
///
/// `wsl.exe` is a launcher, and which shell it logs the user into is a fact
/// about a Linux user account that only the distribution's own password
/// database holds. It used to be asked by a *second* `wsl.exe` started beside
/// the pane (`wsl::begin_login_shell_probe`), whose answer nothing waited for —
/// so the **first** WSL pane of every run composed its command line before the
/// answer existed, fell through to a bare `wsl.exe`, and got no init file, no
/// `OSC 133` and no `OSC 7` at all, while every pane after it in the same
/// process got the lot. That was booked in `docs/DESIGN.md` §7.40 ③ and measured
/// in `docs/plans/shell-matrix-2026-09-07.md` T-2.
///
/// The answer is that a question about the distribution is asked *inside* the
/// distribution, in the same command line as the shell it decides. There is
/// nothing left to wait for, because there is nothing left to race: no second
/// process, no answer in flight, and every WSL pane — the first one included —
/// is composed from the same six arguments.
///
/// It keeps both branches the probe had, and for the probe's own reasons:
///
/// * **bash** takes `--init-file`, which is bash's own flag for naming the
///   startup file of one interactive shell, and `BT_SHELL_INTEGRATION` tells
///   `folio.bash` that it is responsible for the startup chain `--init-file`
///   displaced. Exported here rather than across the boundary so that it is set
///   for exactly the shell that reads it;
/// * **zsh** takes `ZDOTDIR`, which is the directory zsh reads *all* its
///   startup files out of and the only door zsh has — it has no `--init-file`
///   and refuses the flag outright, which is what a zsh login used to be handed
///   (review row R3-6). The reader's own `ZDOTDIR`, if they had one, travels in
///   `BT_USER_ZDOTDIR` so that `folio.zsh` can put their files back. It is still
///   the login shell `wsl.exe` would have started: `-l`, and their own shell;
/// * **anything else** — fish, a login shell somebody built themselves — keeps
///   its shell and is started as the login shell `wsl.exe` would have started,
///   which is the documented degradation (`docs/shell-integration.md`) rather
///   than a substitution. Handing either door to a shell that reads neither
///   would replace a reader's shell with one they did not choose, every time
///   they opened a tab.
///
/// `getent passwd` and not `$SHELL`, for the reason the probe used it: this is
/// not a login shell, so `SHELL` is either unset or inherited from the Win32
/// side. An empty answer — a distribution with no `getent`, or a user whose
/// account is not in the local database — falls to `/bin/sh`, the one shell a
/// POSIX system is required to have, rather than to `exec ""`.
///
/// One line, because it crosses to `wsl.exe` as a single argument.
const WSL_LOGIN_SHELL: &str = concat!(
    r#"shell=$(getent passwd "$(id -u)" 2>/dev/null | cut -d: -f7); "#,
    r#"[ -n "$shell" ] || shell=/bin/sh; "#,
    r#"case "${shell##*/}" in "#,
    r#"bash) [ -n "$1" ] || exec "$shell" -l; "#,
    r#"BT_SHELL_INTEGRATION=login; export BT_SHELL_INTEGRATION; "#,
    r#"exec "$shell" --init-file "$1" -i;; "#,
    r#"zsh) [ -n "$2" ] || exec "$shell" -l; "#,
    r#"[ -n "${ZDOTDIR:-}" ] && { BT_USER_ZDOTDIR=$ZDOTDIR; export BT_USER_ZDOTDIR; }; "#,
    r#"ZDOTDIR="$2"; export ZDOTDIR; exec "$shell" -l;; "#,
    r#"*) exec "$shell" -l;; "#,
    "esac",
);

/// The name [`WSL_LOGIN_SHELL`] answers to, so that a message `sh` prints about
/// it says where it came from rather than `sh: 1: …`.
///
/// It is `$0`, which is why the init file is `$1` and the `ZDOTDIR` directory is
/// `$2`: both travel as **arguments** rather than spliced into the script text,
/// so that a reader whose Windows account name has a space or a quote in it gets
/// paths this shell reads verbatim instead of ones it re-parses.
const WSL_ARGV0: &str = "folio";

/// The arguments that ask bash for a **login** shell, and which therefore cannot
/// travel beside `--init-file`.
///
/// bash's own rule, measured rather than assumed: `--init-file` names the
/// startup file of an interactive shell that is *not* a login shell, and a shell
/// started with `-l` never reads it. So the flag is dropped here and the chain
/// it stands for is emulated by the script, which is told which one it owes by
/// [`INSTALLED_MARKER`].
///
/// A cluster (`-li`) is one argument carrying several short flags, and dropping
/// the whole of it would take the reader's other flags with it — so the `l` is
/// removed from the cluster and what is left is kept. A cluster that was nothing
/// but `l` disappears, because `-` on its own is not an argument bash reads.
fn without_login_flag(argument: &OsStr) -> Option<OsString> {
    let text = argument.to_string_lossy();
    if text == "--login" {
        return None;
    }
    if !text.starts_with('-') || text.starts_with("--") || text.len() < 2 {
        return Some(argument.to_owned());
    }
    if !text.contains('l') {
        return Some(argument.to_owned());
    }
    let kept: String = text[1..].chars().filter(|flag| *flag != 'l').collect();
    if kept.is_empty() {
        return None;
    }
    Some(OsString::from(format!("-{kept}")))
}

/// Whether this profile's own arguments asked for a login shell.
///
/// The **profile's** words and not the whole command line: a
/// [`profiles::SpawnPlace`] argument says where to stand and never which mode to
/// start in, and the question here is what the row asked for.
///
/// `pub(crate)` for [`profiles::launch_args`] (0.4.6 ticket 74), which asks it
/// before adding the row's login flag, so that a row whose own words already say
/// `--login` is not told twice.
pub(crate) fn asks_for_login(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        without_login_flag(OsStr::new(argument)).as_deref() != Some(OsStr::new(argument))
    })
}

/// Where the script is on this machine, written out on first use.
///
/// `None` when it could not be written, and that is a whole, honest outcome
/// rather than an error to report: a shell with no init file is a shell on the
/// documented fallback path, which is where every bash pane was before this
/// existed.
///
/// **An update's trial writes nothing** (`update_trial`, F-7): it names the
/// script the old build left, if there is one, and the commit writes this
/// build's.
pub fn script_path() -> Option<&'static Path> {
    static INSTALLED: OnceLock<Option<PathBuf>> = OnceLock::new();
    static STANDING: OnceLock<Option<PathBuf>> = OnceLock::new();
    if crate::update_trial::defer(crate::update_trial::Writer::BashScript) {
        return STANDING
            .get_or_init(|| {
                let path = persist::storage_dir()
                    .join(SCRIPT_DIRECTORY)
                    .join(SCRIPT_FILE);
                path.is_file().then_some(path)
            })
            .as_deref();
    }
    INSTALLED.get_or_init(install).as_deref()
}

fn install() -> Option<PathBuf> {
    install_script_at(
        &persist::storage_dir().join(SCRIPT_DIRECTORY),
        SCRIPT_FILE,
        SCRIPT,
    )
}

/// **One file of this build's, written into a directory, and kept current**
/// (review row R4-6).
///
/// The compare-and-repair all three installed scripts go through, as a function
/// of a directory so it can be exercised against a temp one rather than against
/// the machine's own `%APPDATA%`.
///
/// Rewritten only when it differs, so that the common start — the same build
/// opening the same kind of tab again — is one read rather than one write, and
/// an open shell reading the file at that moment is not reading a truncated one.
/// Rewritten *whenever* it differs, which is the half that was missing for
/// PowerShell: an upgraded, deleted or truncated copy is the same finding as a
/// copy that was never there.
fn install_script_at(directory: &Path, name: &str, text: &str) -> Option<PathBuf> {
    let path = directory.join(name);
    std::fs::create_dir_all(directory).ok()?;
    // The launch worker and a pane birth can arrive together. A native
    // preserving replace may lose that race without changing the destination;
    // re-read and retry so at least one complete build copy wins, while a real
    // permission or volume failure still returns promptly and is never cached.
    for _ in 0..3 {
        if bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Settings, &path)
            .is_ok_and(|existing| existing == text)
        {
            return Some(path);
        }
        let written = if path.exists() {
            bt_persist::atomic_replace_preserving(&path, text.as_bytes())
        } else {
            bt_persist::atomic_write(&path, text.as_bytes())
        };
        if written.is_ok() {
            return Some(path);
        }
    }
    None
}

/// The directory zsh is pointed at, written out on first use.
///
/// [`script_path`]'s twin, down to the compare-before-write, and the three files
/// are one outcome: a directory holding two of them is a `ZDOTDIR` whose missing
/// third file is a startup file of the reader's that silently stops running. So
/// a write that fails leaves this `None`, and the pane takes the documented
/// fallback — the shell the distribution logs into, with no integration — rather
/// than a half-built directory.
///
/// **An update's trial writes nothing** (`update_trial`, F-7): it names the
/// directory the old build left, if it is whole, and the commit writes this
/// build's.
pub fn zdotdir_path() -> Option<&'static Path> {
    static INSTALLED: OnceLock<Option<PathBuf>> = OnceLock::new();
    static STANDING: OnceLock<Option<PathBuf>> = OnceLock::new();
    if crate::update_trial::defer(crate::update_trial::Writer::ZshScripts) {
        return STANDING
            .get_or_init(|| {
                let directory = persist::storage_dir()
                    .join(SCRIPT_DIRECTORY)
                    .join(ZDOTDIR_DIRECTORY);
                ZDOTDIR_FILES
                    .iter()
                    .all(|name| directory.join(name).is_file())
                    .then_some(directory)
            })
            .as_deref();
    }
    INSTALLED.get_or_init(install_zdotdir).as_deref()
}

fn install_zdotdir() -> Option<PathBuf> {
    let directory = persist::storage_dir()
        .join(SCRIPT_DIRECTORY)
        .join(ZDOTDIR_DIRECTORY);
    let stale = ZDOTDIR_FILES.iter().any(|name| {
        !bt_platform::file_reads::read_to_string(
            bt_platform::file_reads::Lane::Settings,
            directory.join(name),
        )
        .is_ok_and(|existing| existing == SCRIPT_ZSH)
    });
    if !stale {
        return Some(directory);
    }
    std::fs::create_dir_all(&directory).ok()?;
    for name in ZDOTDIR_FILES {
        std::fs::write(directory.join(name), SCRIPT_ZSH).ok()?;
    }
    Some(directory)
}

/// Where this build's installed integration assets are on this machine — **one
/// per door that has a file**, because bash's is a file and zsh's is a directory.
///
/// A pair rather than two parameters, so that a caller cannot hand the bash
/// script to a zsh: which one a profile is served by is [`profiles::Integration`]'s
/// answer and not the caller's, and it is asked inside [`shell_command`].
///
/// PowerShell's script is not here: it is prepared and named on the shell's
/// birth worker ([`compose_powershell_birth`]), never on the window thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scripts<'a> {
    /// `folio.bash`, named by `--init-file`.
    pub bash: Option<&'a Path>,
    /// The directory `folio.zsh` was written into, named by `ZDOTDIR`.
    pub zdotdir: Option<&'a Path>,
}

impl<'a> Scripts<'a> {
    /// The pair this machine has, both written out on first use.
    #[must_use]
    pub fn installed() -> Scripts<'static> {
        Scripts {
            bash: script_path(),
            zdotdir: zdotdir_path(),
        }
    }
}

/// Everything the spawn needs to say, beyond the program itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellCommand {
    pub arguments: Vec<OsString>,
    /// Folio's terminal and integration declarations.
    pub environment: Vec<(OsString, OsString)>,
    /// The selected profile's final environment layer.
    pub profile_environment: Vec<(OsString, OsString)>,
    /// The facts needed to re-derive environment-dependent declarations at pane birth.
    pub(crate) environment_derivation: EnvironmentDerivation,
}

/// Which Folio declarations depend on values already present in a pane's environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EnvironmentDerivation {
    pub(crate) integration: Integration,
    pub(crate) crosses_wsl: bool,
    pub(crate) forwards_terminal_into_wsl: bool,
}

impl Default for EnvironmentDerivation {
    fn default() -> Self {
        Self {
            integration: Integration::None,
            crosses_wsl: false,
            forwards_terminal_into_wsl: false,
        }
    }
}

/// The whole argument list and environment for one leaf of `profile`, with the
/// integration folded in where there is one to fold.
///
/// One function rather than an integration layer bolted onto the profile's own
/// arguments, because for Git Bash the integration **replaces** an argument
/// rather than adding one: `--login` and `--init-file` are mutually exclusive in
/// effect (bash reads the init file only for a shell that is *not* a login
/// shell), so a caller that appended would produce a command line where the
/// script is silently never read. That failure has no symptom other than the
/// absence of markers, which is indistinguishable from a shell that has none.
///
/// `place_arguments` are [`profiles::SpawnPlace`]'s and follow the profile's own
/// words, which matters for WSL alone: `--cd` is a flag to the *launcher* and
/// must come before the `-e` that ends the launcher's own arguments.
///
/// `environment` is read, not written: Command Prompt's integration is a
/// variable this process already has one of, and prefixing rather than replacing
/// it means the composition has to see what is there.
#[must_use]
pub fn shell_command(
    profile: &Profile,
    place_arguments: &[OsString],
    scripts: Scripts<'_>,
    environment: &dyn ShellEnvironment,
) -> ShellCommand {
    // The row's login flag and then its own words (0.4.6 ticket 74): what the row
    // asks the program for, before any door below trades a word for its script.
    let words = profiles::launch_args(profile);
    let own = || {
        let words = words.iter().map(OsString::from);
        if profiles::served_by(profile) == Integration::PowerShellOptIn {
            // PowerShell keeps parsing non-terminal switches only until a terminal such as
            // -Command. A starting-place launcher flag therefore has to precede the row's own
            // words; after -Command it would become user command text instead of a host flag.
            place_arguments
                .iter()
                .cloned()
                .chain(words)
                .collect::<Vec<_>>()
        } else {
            words
                .chain(place_arguments.iter().cloned())
                .collect::<Vec<_>>()
        }
    };
    let mut command = shell_command_for(profile, scripts, environment, &own);
    command.environment_derivation = EnvironmentDerivation {
        integration: profiles::served_by(profile),
        crosses_wsl: profile.paths == profiles::PathNamespace::Wsl,
        forwards_terminal_into_wsl: command
            .environment
            .iter()
            .any(|(name, _)| environment_name_eq(name, OsStr::new("WSLENV"))),
    };
    let mine = &profile.env;
    command.environment.extend(hyperlink_declaration(
        profiles::served_by(profile),
        environment,
        mine,
    ));
    command.environment.extend(locale_declaration(
        bt_platform::system_locale_declaration(),
        environment,
        mine,
    ));
    // **Across the boundary before the layering**, because what has to cross is
    // decided by the names this profile is about to set and `WSLENV` is itself
    // one of the names it could set — a reader who writes their own `WSLENV` row
    // is answering the question outright, and the layering below is what lets
    // them.
    if profile.paths == profiles::PathNamespace::Wsl {
        forward_into_wsl(&mut command.environment, mine);
    }
    layer_profile_environment(&mut command.profile_environment, mine);
    command
}

struct ComposedEnvironment<'a>(&'a [(OsString, OsString)]);

impl ShellEnvironment for ComposedEnvironment<'_> {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0
            .iter()
            .rev()
            .find(|(name, _)| environment_name_eq(name, OsStr::new(key)))
            .map(|(_, value)| value.clone())
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Rebuild every Folio declaration derived from a value the pane already has.
///
/// Called once on the pane-birth worker with the composed fresh account block plus explicit
/// launch overrides, before Folio's declarations and the profile's final layer are applied.
pub(crate) fn derive_environment_for_birth(
    derivation: EnvironmentDerivation,
    environment: &[(OsString, OsString)],
    folio: &mut Vec<(OsString, OsString)>,
    profile: &[(OsString, OsString)],
) {
    let environment = ComposedEnvironment(environment);
    let profile_has = |wanted: &str| {
        profile
            .iter()
            .any(|(name, _)| environment_name_eq(name, OsStr::new(wanted)))
    };
    let remove = |rows: &mut Vec<(OsString, OsString)>, wanted: &str| {
        rows.retain(|(name, _)| !environment_name_eq(name, OsStr::new(wanted)));
    };

    if derivation.integration == Integration::CmdPrompt {
        remove(folio, CMD_PROMPT);
        folio.push((
            OsString::from(CMD_PROMPT),
            cmd_prompt(environment.var_os(CMD_PROMPT)),
        ));
    }

    remove(folio, FORCE_HYPERLINK);
    if derivation.integration != Integration::PowerShellOptIn
        && !profile_has(FORCE_HYPERLINK)
        && environment.var_os(FORCE_HYPERLINK).is_none()
    {
        folio.push((OsString::from(FORCE_HYPERLINK), OsString::from("1")));
    }

    for name in LOCALE_VARIABLES {
        remove(folio, name);
    }
    if !LOCALE_VARIABLES.iter().any(|name| profile_has(name)) {
        folio.extend(locale_declaration(
            bt_platform::system_locale_declaration(),
            &environment,
            &[],
        ));
    }

    if derivation.integration == Integration::ZshDotDir {
        remove(folio, USER_ZDOTDIR);
        if let Some(theirs) = environment.var_os(ZDOTDIR) {
            folio.push((OsString::from(USER_ZDOTDIR), theirs));
        }
    }

    if derivation.crosses_wsl {
        remove(folio, "WSLENV");
        let mut list = environment
            .var_os("WSLENV")
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut append = |name: &str| {
            if list
                .split(':')
                .any(|entry| entry.split('/').next().is_some_and(|entry| entry == name))
            {
                return;
            }
            if !list.is_empty() && !list.ends_with(':') {
                list.push(':');
            }
            list.push_str(name);
            list.push_str("/u");
        };
        if derivation.forwards_terminal_into_wsl {
            for name in FORWARDED {
                append(name);
            }
        }
        for (name, _) in profile {
            let name = name.to_string_lossy();
            if !name.is_empty() && !name.eq_ignore_ascii_case("WSLENV") {
                append(&name);
            }
        }
        if !list.is_empty() {
            folio.push((OsString::from("WSLENV"), OsString::from(list)));
        }
    }
}

/// The PowerShell console-host parser's meaning for one spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PowerShellOptionKind {
    Flag,
    Value,
    NoExit,
    NonInteractive,
    Command,
    EncodedCommand,
    File,
    CommandWithArgs,
    Unsupported,
}

/// One `MatchSwitch` call in PowerShell's command-line parser.
///
/// `minimum` is deliberately explicit. The hosts do not calculate prefixes
/// from a set: each parser call names its own smallest accepted spelling, with
/// aliases such as `ep`, `wd`, `ec`, and `cwa` as separate calls. That is why
/// `-e` means EncodedCommand while the shorter shared prefixes of the `no*`
/// family are errors. The order below is the host's order where two calls can
/// overlap.
#[derive(Clone, Copy)]
struct PowerShellOption {
    name: &'static str,
    /// **The parameter this call binds** — the option's identity, whatever it was spelled as.
    /// An alias call (`ep`, `ec`, `wd`, …) binds the same parameter as its full name, so it
    /// names that full name here; every reader that asks *which* option a word is asks this.
    parameter: &'static str,
    minimum: &'static str,
    kind: PowerShellOptionKind,
}

const WINPS_OPTIONS: &[PowerShellOption] = &[
    PowerShellOption {
        name: "version",
        parameter: "version",
        minimum: "v",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "help",
        parameter: "help",
        minimum: "h",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "?",
        parameter: "?",
        minimum: "?",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "noexit",
        parameter: "noexit",
        minimum: "noe",
        kind: PowerShellOptionKind::NoExit,
    },
    PowerShellOption {
        name: "noprofile",
        parameter: "noprofile",
        minimum: "nop",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "nologo",
        parameter: "nologo",
        minimum: "nol",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "noninteractive",
        parameter: "noninteractive",
        minimum: "noni",
        kind: PowerShellOptionKind::NonInteractive,
    },
    PowerShellOption {
        name: "configurationname",
        parameter: "configurationname",
        minimum: "config",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "command",
        parameter: "command",
        minimum: "c",
        kind: PowerShellOptionKind::Command,
    },
    PowerShellOption {
        name: "windowstyle",
        parameter: "windowstyle",
        minimum: "w",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "file",
        parameter: "file",
        minimum: "f",
        kind: PowerShellOptionKind::File,
    },
    PowerShellOption {
        name: "outputformat",
        parameter: "outputformat",
        minimum: "o",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "of",
        parameter: "outputformat",
        minimum: "o",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "inputformat",
        parameter: "inputformat",
        minimum: "in",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "if",
        parameter: "inputformat",
        minimum: "if",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "executionpolicy",
        parameter: "executionpolicy",
        minimum: "ex",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "ep",
        parameter: "executionpolicy",
        minimum: "ep",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "encodedcommand",
        parameter: "encodedcommand",
        minimum: "e",
        kind: PowerShellOptionKind::EncodedCommand,
    },
    PowerShellOption {
        name: "ec",
        parameter: "encodedcommand",
        minimum: "e",
        kind: PowerShellOptionKind::EncodedCommand,
    },
    PowerShellOption {
        name: "encodedarguments",
        parameter: "encodedarguments",
        minimum: "encodeda",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "ea",
        parameter: "encodedarguments",
        minimum: "ea",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "servermode",
        parameter: "servermode",
        minimum: "s",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "sta",
        parameter: "sta",
        minimum: "sta",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "mta",
        parameter: "mta",
        minimum: "mta",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "psconsolefile",
        parameter: "psconsolefile",
        minimum: "psconsolefile",
        kind: PowerShellOptionKind::Value,
    },
];

const PWSH_OPTIONS: &[PowerShellOption] = &[
    PowerShellOption {
        name: "version",
        parameter: "version",
        minimum: "v",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "help",
        parameter: "help",
        minimum: "h",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "?",
        parameter: "?",
        minimum: "?",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "login",
        parameter: "login",
        minimum: "l",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "noexit",
        parameter: "noexit",
        minimum: "noe",
        kind: PowerShellOptionKind::NoExit,
    },
    PowerShellOption {
        name: "noprofile",
        parameter: "noprofile",
        minimum: "nop",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "nologo",
        parameter: "nologo",
        minimum: "nol",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "noninteractive",
        parameter: "noninteractive",
        minimum: "noni",
        kind: PowerShellOptionKind::NonInteractive,
    },
    PowerShellOption {
        name: "noprofileloadtime",
        parameter: "noprofileloadtime",
        minimum: "noprofileloadtime",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "interactive",
        parameter: "interactive",
        minimum: "i",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "configurationfile",
        parameter: "configurationfile",
        minimum: "configurationfile",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "configurationname",
        parameter: "configurationname",
        minimum: "config",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "custompipename",
        parameter: "custompipename",
        minimum: "cus",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "commandwithargs",
        parameter: "commandwithargs",
        minimum: "commandwithargs",
        kind: PowerShellOptionKind::CommandWithArgs,
    },
    PowerShellOption {
        name: "cwa",
        parameter: "commandwithargs",
        minimum: "cwa",
        kind: PowerShellOptionKind::CommandWithArgs,
    },
    PowerShellOption {
        name: "command",
        parameter: "command",
        minimum: "c",
        kind: PowerShellOptionKind::Command,
    },
    PowerShellOption {
        name: "windowstyle",
        parameter: "windowstyle",
        minimum: "w",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "file",
        parameter: "file",
        minimum: "f",
        kind: PowerShellOptionKind::File,
    },
    PowerShellOption {
        name: "outputformat",
        parameter: "outputformat",
        minimum: "o",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "of",
        parameter: "outputformat",
        minimum: "o",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "inputformat",
        parameter: "inputformat",
        minimum: "inp",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "if",
        parameter: "inputformat",
        minimum: "if",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "executionpolicy",
        parameter: "executionpolicy",
        minimum: "ex",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "ep",
        parameter: "executionpolicy",
        minimum: "ep",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "encodedcommand",
        parameter: "encodedcommand",
        minimum: "e",
        kind: PowerShellOptionKind::EncodedCommand,
    },
    PowerShellOption {
        name: "ec",
        parameter: "encodedcommand",
        minimum: "e",
        kind: PowerShellOptionKind::EncodedCommand,
    },
    PowerShellOption {
        name: "encodedarguments",
        parameter: "encodedarguments",
        minimum: "encodeda",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "ea",
        parameter: "encodedarguments",
        minimum: "ea",
        kind: PowerShellOptionKind::Unsupported,
    },
    PowerShellOption {
        name: "settingsfile",
        parameter: "settingsfile",
        minimum: "settings",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "sta",
        parameter: "sta",
        minimum: "sta",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "mta",
        parameter: "mta",
        minimum: "mta",
        kind: PowerShellOptionKind::Flag,
    },
    PowerShellOption {
        name: "workingdirectory",
        parameter: "workingdirectory",
        minimum: "wo",
        kind: PowerShellOptionKind::Value,
    },
    PowerShellOption {
        name: "wd",
        parameter: "workingdirectory",
        minimum: "wd",
        kind: PowerShellOptionKind::Value,
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
enum PowerShellTerminal {
    None,
    Command { option: usize, text: String },
    EncodedCommand { payload: usize, text: String },
    File,
    CommandWithArgs,
    Stdin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PowerShellNonTerminal {
    /// [`PowerShellOption::parameter`]: one identity for every accepted spelling.
    parameter: &'static str,
    option: usize,
    value: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedPowerShellArguments {
    non_terminal: Vec<PowerShellNonTerminal>,
    no_exit: bool,
    non_interactive: bool,
    terminal: PowerShellTerminal,
}

fn is_pwsh(program: &Path) -> bool {
    program
        .file_stem()
        .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("pwsh"))
}

fn option_table(program: &Path) -> &'static [PowerShellOption] {
    if is_pwsh(program) {
        PWSH_OPTIONS
    } else {
        WINPS_OPTIONS
    }
}

fn decode_encoded_command(value: &str) -> Option<String> {
    let bytes = BASE64.decode(value).ok()?;
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }
    let wide = chunks
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&wide).ok()
}

fn encode_command(value: &str) -> String {
    let bytes = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    BASE64.encode(bytes)
}

fn classify_powershell_arguments(
    program: &Path,
    arguments: &[OsString],
) -> Option<ParsedPowerShellArguments> {
    let mut non_terminal = Vec::new();
    let mut no_exit = false;
    let mut non_interactive = false;
    let mut at = 0;
    while at < arguments.len() {
        let word = arguments[at].to_str()?.trim();
        if word.is_empty() {
            at += 1;
            continue;
        }
        let mut characters = word.chars();
        let prefix = characters.next()?;
        if !matches!(prefix, '-' | '/') {
            return None;
        }
        let mut key = characters.as_str();
        if prefix == '-' && key.starts_with('-') {
            key = &key[1..];
        }
        let option = option_table(program).iter().find(|option| {
            key.len() >= option.minimum.len()
                && option.name.len() >= key.len()
                && option.name[..key.len()].eq_ignore_ascii_case(key)
        })?;
        if (is_pwsh(program) && option.parameter == "login" && at != 0)
            || (!is_pwsh(program) && option.parameter == "psconsolefile" && at != 0)
        {
            return None;
        }
        match option.kind {
            PowerShellOptionKind::Flag => {
                non_terminal.push(PowerShellNonTerminal {
                    parameter: option.parameter,
                    option: at,
                    value: None,
                });
                at += 1;
            }
            PowerShellOptionKind::NoExit => {
                no_exit = true;
                non_terminal.push(PowerShellNonTerminal {
                    parameter: option.parameter,
                    option: at,
                    value: None,
                });
                at += 1;
            }
            PowerShellOptionKind::Value => {
                non_terminal.push(PowerShellNonTerminal {
                    parameter: option.parameter,
                    option: at,
                    value: Some(at + 1),
                });
                at += 2;
                if at > arguments.len() {
                    return None;
                }
            }
            PowerShellOptionKind::NonInteractive => {
                non_interactive = true;
                non_terminal.push(PowerShellNonTerminal {
                    parameter: option.parameter,
                    option: at,
                    value: None,
                });
                at += 1;
            }
            PowerShellOptionKind::Unsupported => return None,
            PowerShellOptionKind::File => {
                if at + 1 >= arguments.len() {
                    return None;
                }
                return Some(ParsedPowerShellArguments {
                    non_terminal,
                    no_exit,
                    non_interactive,
                    terminal: PowerShellTerminal::File,
                });
            }
            PowerShellOptionKind::CommandWithArgs => {
                if at + 1 >= arguments.len() {
                    return None;
                }
                return Some(ParsedPowerShellArguments {
                    non_terminal,
                    no_exit,
                    non_interactive,
                    terminal: PowerShellTerminal::CommandWithArgs,
                });
            }
            PowerShellOptionKind::Command => {
                let option = at;
                let tail = &arguments[at + 1..];
                if tail.len() == 1 && tail[0] == OsStr::new("-") {
                    return Some(ParsedPowerShellArguments {
                        non_terminal,
                        no_exit,
                        non_interactive,
                        terminal: PowerShellTerminal::Stdin,
                    });
                }
                if tail.is_empty() {
                    return None;
                }
                let text = tail
                    .iter()
                    .map(|word| word.to_str())
                    .collect::<Option<Vec<_>>>()?
                    .join(" ");
                return Some(ParsedPowerShellArguments {
                    non_terminal,
                    no_exit,
                    non_interactive,
                    terminal: PowerShellTerminal::Command { option, text },
                });
            }
            PowerShellOptionKind::EncodedCommand => {
                let payload = at + 1;
                if payload >= arguments.len() || payload + 2 < arguments.len() {
                    return None;
                }
                let text = decode_encoded_command(arguments[payload].to_str()?)?;
                if payload + 1 < arguments.len() {
                    let trailing = arguments[payload + 1].to_str()?.trim();
                    let trailing = trailing
                        .strip_prefix('-')
                        .or_else(|| trailing.strip_prefix('/'))?;
                    if !trailing.eq_ignore_ascii_case("noexit") {
                        return None;
                    }
                    no_exit = true;
                    non_terminal.push(PowerShellNonTerminal {
                        parameter: "noexit",
                        option: payload + 1,
                        value: None,
                    });
                }
                return Some(ParsedPowerShellArguments {
                    non_terminal,
                    no_exit,
                    non_interactive,
                    terminal: PowerShellTerminal::EncodedCommand { payload, text },
                });
            }
        }
    }
    Some(ParsedPowerShellArguments {
        non_terminal,
        no_exit,
        non_interactive,
        terminal: PowerShellTerminal::None,
    })
}

fn composed_powershell_arguments(
    program: &Path,
    row_arguments: &[OsString],
    script: &Path,
    parse_answer: Option<bool>,
) -> Option<Vec<OsString>> {
    let parsed = classify_powershell_arguments(program, row_arguments)?;
    if parsed.non_interactive {
        return None;
    }
    let loader = powershell_load_command(program, script);
    let mut arguments = row_arguments.to_vec();
    match parsed.terminal {
        PowerShellTerminal::None => {
            arguments.push(OsString::from("-NoExit"));
            arguments.push(OsString::from("-Command"));
            arguments.push(OsString::from(loader));
        }
        PowerShellTerminal::Command { option, text } => {
            if parse_answer != Some(true) {
                return None;
            }
            arguments.truncate(option + 1);
            arguments.push(OsString::from(format!("{text}\r\n{loader}")));
        }
        PowerShellTerminal::EncodedCommand { payload, text } => {
            if parse_answer != Some(true) {
                return None;
            }
            arguments[payload] = OsString::from(encode_command(&format!("{text}\r\n{loader}")));
        }
        PowerShellTerminal::File
        | PowerShellTerminal::CommandWithArgs
        | PowerShellTerminal::Stdin => return None,
    }
    let within_limit = bt_pty::windows_command_line_len(program, &arguments)
        .is_some_and(|length| length <= 32_766);
    within_limit.then_some(arguments)
}

/// Compose a PowerShell process's complete argv without changing the row's own words.
///
/// This pure seam is also the conservative answer before the asynchronous
/// target-parser probe has answered: command-bearing rows remain byte-for-byte
/// as written. Births use [`compose_with_prepared`], which supplies the cached
/// answer when one exists.
#[must_use]
#[cfg(test)]
pub fn compose_powershell_arguments(
    program: &Path,
    row_arguments: &[OsString],
    script: Option<&Path>,
    enabled: bool,
) -> Vec<OsString> {
    if !enabled || !is_powershell(program) {
        return row_arguments.to_vec();
    }
    script
        .and_then(|script| composed_powershell_arguments(program, row_arguments, script, None))
        .unwrap_or_else(|| row_arguments.to_vec())
}

/// **A shell's argv, finished on its birth worker** (`pty_door::spawn_shell`).
///
/// The row's words come from the window thread; the PowerShell load is appended here, because
/// naming the script means preparing it ([`powershell_script_for_birth`]) and that is disk work —
/// a compare, maybe a write — which can stall on a redirected or sleeping drive. Asked here, a slow
/// disk delays this one pane's birth and never the window. Nothing is prepared for a program the
/// composer would not load into.
#[must_use]
pub fn compose_powershell_birth(
    program: &Path,
    arguments: &[OsString],
    powershell_integration: bool,
) -> Vec<OsString> {
    compose_with_prepared_and_retry(
        program,
        arguments,
        powershell_integration,
        powershell_script_for_birth,
        schedule_parse_retry,
    )
}

/// [`compose_powershell_birth`] with the preparation handed in: `prepare` is asked only when the
/// composer would load into this argv, and its answer is the script named.
#[cfg(test)]
fn compose_with_prepared(
    program: &Path,
    arguments: &[OsString],
    powershell_integration: bool,
    prepare: impl FnOnce() -> Option<PathBuf>,
) -> Vec<OsString> {
    compose_with_prepared_and_retry(program, arguments, powershell_integration, prepare, |_| {})
}

/// The birth seam with its background retry handed in. The callback is told
/// only about a command whose parse answer is unknown; it must return without
/// waiting for the question to be answered.
fn compose_with_prepared_and_retry(
    program: &Path,
    arguments: &[OsString],
    powershell_integration: bool,
    prepare: impl FnOnce() -> Option<PathBuf>,
    retry: impl FnOnce(ParseQuestion),
) -> Vec<OsString> {
    if !powershell_integration || !is_powershell(program) {
        return arguments.to_vec();
    }
    let Some(parsed) = classify_powershell_arguments(program, arguments) else {
        return arguments.to_vec();
    };
    if parsed.non_interactive {
        return arguments.to_vec();
    }
    let parse_answer = match &parsed.terminal {
        PowerShellTerminal::Command { .. } | PowerShellTerminal::EncodedCommand { .. } => {
            cached_parse_answer(program, arguments)
        }
        PowerShellTerminal::None => None,
        PowerShellTerminal::File
        | PowerShellTerminal::CommandWithArgs
        | PowerShellTerminal::Stdin => return arguments.to_vec(),
    };
    if matches!(
        parsed.terminal,
        PowerShellTerminal::Command { .. } | PowerShellTerminal::EncodedCommand { .. }
    ) && parse_answer != Some(true)
    {
        if let Some(text) = command_text(program, arguments) {
            retry(ParseQuestion::new(program, arguments, text));
        }
        return arguments.to_vec();
    }
    let Some(script) = prepare().filter(|path| path.is_file()) else {
        return arguments.to_vec();
    };
    composed_powershell_arguments(program, arguments, &script, parse_answer)
        .unwrap_or_else(|| arguments.to_vec())
}

/// The complete argument list `bt-pty`'s last-resort retry is started with when a pane's own
/// program would not start: [`bt_pty::LAST_RESORT_ARGUMENTS`] through
/// [`compose_powershell_birth`], so the retry — Windows PowerShell — carries the same load as any
/// other PowerShell this process starts, and `/bin/sh` off Windows carries nothing. Asked by
/// `bt-pty` only when the retry happens, on the birth worker.
#[must_use]
pub fn last_resort_arguments(powershell_integration: bool) -> Vec<OsString> {
    compose_powershell_birth(
        Path::new(bt_pty::LAST_RESORT_SHELL),
        &bt_pty::last_resort_arguments(),
        powershell_integration,
    )
}

/// Whether this argv is known, right now, to receive integration at birth.
///
/// Command text needs the asynchronous target-shell parse answer; terminal and
/// noninteractive forms always decline. This is the Settings capability line's
/// side-effect-free view of the same cache the birth seam reads.
#[must_use]
pub fn powershell_arguments_are_safe(program: &Path, arguments: &[OsString]) -> bool {
    let Some(parsed) = classify_powershell_arguments(program, arguments) else {
        return false;
    };
    if parsed.non_interactive {
        return false;
    }
    let parse_answer = match parsed.terminal {
        PowerShellTerminal::None => None,
        PowerShellTerminal::Command { .. } | PowerShellTerminal::EncodedCommand { .. } => {
            if cached_parse_answer(program, arguments) != Some(true) {
                return false;
            }
            Some(true)
        }
        PowerShellTerminal::File
        | PowerShellTerminal::CommandWithArgs
        | PowerShellTerminal::Stdin => return false,
    };
    let prospective = persist::storage_dir()
        .join(SCRIPT_DIRECTORY)
        .join(SCRIPT_FILE_PS1);
    composed_powershell_arguments(program, arguments, &prospective, parse_answer).is_some()
}

fn powershell_single_quoted(value: &Path) -> String {
    let mut escaped = String::new();
    for character in value.to_string_lossy().chars() {
        if matches!(
            character,
            '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}'
        ) {
            escaped.push(character);
        }
        escaped.push(character);
    }
    format!("'{escaped}'")
}

/// The command text appended after `-NoExit -Command`.
#[must_use]
pub fn powershell_load_command(program: &Path, script: &Path) -> String {
    let path = powershell_single_quoted(script);
    let revive = if cfg!(windows) && is_pwsh(program) {
        "if (-not (Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine) -and ([AppDomain]::CurrentDomain.GetAssemblies() | Where-Object { $_.GetName().Name -eq 'Microsoft.PowerShell.PSReadLine' })) { Import-Module (Join-Path $PSHOME 'Modules\\PSReadLine\\Microsoft.PowerShell.PSReadLine.dll') }; "
    } else {
        ""
    };
    format!(
        "if ($ExecutionContext.SessionState.LanguageMode -eq 'FullLanguage') {{ {revive}if ((Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine) -and -not (Get-Command PSConsoleHostReadLine -CommandType Function -ErrorAction Ignore)) {{ if (-not $Global:__FolioShellIntegration) {{ $Global:__FolioShellIntegration = @{{}} }}; $Global:__FolioShellIntegration.ReadLineType = (Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine | Where-Object ImplementingAssembly | Select-Object -Last 1).ImplementingAssembly.GetType('Microsoft.PowerShell.PSConsoleReadLine'); function global:PSConsoleHostReadLine {{ $lastRunStatus = $?; Microsoft.PowerShell.Core\\Set-StrictMode -Off; $Global:__FolioShellIntegration.ReadLineType::ReadLine($host.Runspace, $ExecutionContext, $lastRunStatus) }} }}; if (Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine) {{ $folioScript = $null; try {{ $folioScript = [IO.File]::ReadAllText({path}) }} catch {{}}; if ($null -ne $folioScript) {{ . ([scriptblock]::Create($folioScript)) }} }} }}"
    )
}

/// Write this profile's own environment over what the terminal has said —
/// **the last of the three layers** (plan §1.7, `profiles::Profile::env`).
///
/// Replace-in-place rather than append, so that one name is one entry and the
/// list this returns is readable as the sentence it is. Appending would work at
/// the far end — `PtyCommand::env` replaces case-insensitively and the last
/// write wins — but it would leave two contradictory rows in the record every
/// test and every reader of this function has to see through.
///
/// **A row with no name is not a variable** and is dropped here. It is what the
/// editor's `Add` produces before anybody types, it round-trips through
/// `profiles.json` as a key of `""`, and it is the one shape a child's
/// environment block genuinely cannot carry.
///
/// An empty **value** is carried through unchanged, and what the child then has
/// is *no such variable* — measured, not assumed: Windows removes an
/// environment-block entry with an empty value rather than binding the name to
/// the empty string, so a profile carrying `FOO=` takes `FOO` away from its
/// sessions even when this window inherited one. That is left to the operating
/// system to answer rather than filtered here, because filtering would be this
/// terminal inventing a rule about somebody else's environment block — and the
/// answer it gives is the one a reader who cleared a value box meant.
///
/// The terminal's *other* declarations — `TERM_PROGRAM`,
/// `TERM_PROGRAM_VERSION`, `COLORTERM`, `TERM` — are not in this list at all;
/// they are `bt_pty::PtyCommand`'s, and it already yields to an explicit value
/// from the caller. So a profile row named `TERM_PROGRAM` arrives there as the
/// caller's explicit value and wins, which is the same rule reaching the same
/// answer one layer down rather than a second mechanism.
pub fn layer_profile_environment(
    environment: &mut Vec<(OsString, OsString)>,
    mine: &[(String, String)],
) {
    for (name, value) in mine {
        if !a_child_can_be_given(name) {
            continue;
        }
        let name = OsString::from(name);
        let value = OsString::from(value);
        match environment
            .iter_mut()
            .find(|(existing, _)| environment_name_eq(existing, &name))
        {
            Some(existing) => *existing = (name, value),
            None => environment.push((name, value)),
        }
    }
}

/// **Whether a profile row names a variable a child can actually be given**
/// (review row R2-22).
///
/// An environment block is a run of `NAME=VALUE` strings with a NUL between
/// them, so the first `=` in an entry is where the name ends and a NUL is where
/// the entry does. A row named `A=B` therefore does not create a variable called
/// `A=B`: it writes `A=B=value`, which the child reads as `A` set to `B=value`
/// — the row silently overwriting a *different* variable than the one it names.
/// A row whose name carries a NUL ends the block early and takes every entry
/// after it away from the child.
///
/// So such a row is not layered, exactly as a row with an empty name is not, and
/// [`crate::profiles::Profile::env`] is where that is written down for a reader
/// of the table. The launcher refuses the same two spellings again at the block
/// itself (`portable_pty`'s `environment_block`), because that is the boundary
/// where the grammar is real.
fn a_child_can_be_given(name: &str) -> bool {
    !name.is_empty() && !name.contains('=') && !name.contains('\0')
}

/// Windows environment variable names are case-insensitive, and a profile that
/// wrote `Term_Program` means the one the terminal declared.
fn environment_name_eq(left: &OsStr, right: &OsStr) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

/// List this profile's own variable names in `WSLENV`, so that they cross.
///
/// **A variable set on `wsl.exe` is set on a Win32 process**, and the
/// distribution behind it sees nothing that was not named in `WSLENV` — which
/// is why the terminal's own declarations are listed there already (see
/// [`FORWARDED`]). A profile's environment would otherwise be stored, written to
/// the launcher, and invisible to the only shell it was aimed at: the row would
/// look like it worked from every side except the one that matters.
///
/// `/u` — Win32 to WSL only, value carried verbatim — because that is what these
/// are: values, not paths, and this terminal has no way to know that a user's
/// own variable holds a path that should be translated. A reader who wants
/// translation writes their own `WSLENV` row, and the layering above lets that
/// row win outright.
///
/// The terminal's own five are **not** added here, and that is deliberate rather
/// than an omission: they are listed by the install path alone, which is what
/// `docs/shell-integration.md`'s matrix states about a WSL login that lands in
/// `zsh` ("set, but not forwarded"). What changes in this slice is that a
/// profile's own rows cross whatever the login shell turns out to be, because
/// they are the reader's instruction and not this terminal's guess.
fn forward_into_wsl(environment: &mut Vec<(OsString, OsString)>, mine: &[(String, String)]) {
    let names: Vec<&str> = mine
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("WSLENV"))
        .collect();
    if names.is_empty() {
        return;
    }
    let listed = environment
        .iter()
        .position(|(key, _)| environment_name_eq(key, OsStr::new("WSLENV")));
    let existing = match listed {
        Some(at) => environment[at].1.clone(),
        None => std::env::var_os("WSLENV").unwrap_or_default(),
    };
    let mut list = existing.to_string_lossy().into_owned();
    for name in names {
        // Already carried — by [`FORWARDED`], or by a second row of the same
        // name — and listing it twice would put a `FORCE_HYPERLINK/u` in there
        // for every profile that answers the hyperlink question.
        if list
            .split(':')
            .any(|entry| entry.split('/').next().is_some_and(|it| it == name))
        {
            continue;
        }
        if !list.is_empty() && !list.ends_with(':') {
            list.push(':');
        }
        list.push_str(name);
        list.push_str("/u");
    }
    let list = OsString::from(list);
    match listed {
        Some(at) => environment[at].1 = list,
        None => environment.push((OsString::from("WSLENV"), list)),
    }
}

fn shell_command_for(
    profile: &Profile,
    scripts: Scripts<'_>,
    environment: &dyn ShellEnvironment,
    own: &dyn Fn() -> Vec<OsString>,
) -> ShellCommand {
    match profiles::served_by(profile) {
        Integration::BashInitFile if scripts.bash.is_some() => match profile.paths {
            // Git Bash: bash *is* the program, and takes the flag directly.
            //
            // **The profile's own words are kept** (review row R3-7). This arm
            // used to build the whole command line out of three literals, so a
            // reader who added `--noediting`, or `-O globstar`, or anything else
            // to the row lost it the moment the row had integration — the
            // profile said one thing and the spawn did another, with nothing to
            // show why. What is dropped is the *login flag* alone, and only
            // because bash will not read an init file for a login shell; the
            // chain it stood for is put back by the script, which is told which
            // one it owes.
            profiles::PathNamespace::Windows => {
                let Some(script) = scripts.bash else {
                    unreachable!("guarded by the arm")
                };
                let mut arguments = vec![OsString::from("--init-file"), script.into()];
                let mut interactive = false;
                let login = asks_for_login(&profiles::launch_args(profile));
                for argument in own().iter().filter_map(|it| without_login_flag(it)) {
                    interactive |= argument == *"-i" || argument == *"--interactive";
                    arguments.push(argument);
                }
                // `-i` is what makes bash read the init file at all, and a
                // profile that had only `--login` said it wanted an interactive
                // shell by saying `--login` to a terminal.
                if !interactive {
                    arguments.push(OsString::from("-i"));
                }
                ShellCommand {
                    arguments,
                    environment: installed_environment(login),
                    profile_environment: Vec::new(),
                    environment_derivation: EnvironmentDerivation::default(),
                }
            }
            // WSL: `wsl.exe` is a launcher, so the shell and its flag come
            // after it — and *which* shell that is, is a question only the
            // distribution can answer, so what the launcher is handed is the
            // question (see [`WSL_LOGIN_SHELL`]). The script has to be named in
            // the distribution's own spelling because it is the distribution
            // that will open it.
            //
            // **`-e` and not `--`, and it is load-bearing** (measured
            // 2026-09-07, on this machine's Ubuntu-24.04). `wsl.exe --` joins
            // everything after it into *one command line* and hands that to the
            // user's login shell, which re-parses it: a script argument
            // carrying spaces, quotes, `$`, `|` and `;` comes apart, and what
            // ran here was the fragments of it as separate commands. `wsl.exe
            // -e` executes the program directly, argv for argv — `$0` is
            // `folio`, `$1` is the init file with its space intact — which is
            // what `ask_login_shell` was already relying on and what makes
            // passing the path as an argument work at all. The old spelling was
            // `--` and survived only because none of `/bin/bash --init-file
            // <path> -i` had a space in it.
            profiles::PathNamespace::Wsl => wsl_command(scripts, own),
        },
        // zsh's door is a directory and not a flag, so nothing is added to the
        // command line at all: the profile's own words go through untouched and
        // `ZDOTDIR` says where the startup files are. The reader's own
        // `ZDOTDIR`, if this window inherited one, travels beside it so that
        // `folio.zsh` can put their files back.
        Integration::ZshDotDir if scripts.zdotdir.is_some() => {
            let Some(zdotdir) = scripts.zdotdir else {
                unreachable!("guarded by the arm")
            };
            match profile.paths {
                profiles::PathNamespace::Windows => ShellCommand {
                    arguments: own(),
                    environment: zdotdir_environment(zdotdir.as_os_str().to_owned(), environment),
                    profile_environment: Vec::new(),
                    environment_derivation: EnvironmentDerivation::default(),
                },
                // Under WSL the launcher is handed the question, and the
                // directory has to be named in the distribution's own spelling
                // because it is the distribution that will open it.
                profiles::PathNamespace::Wsl => wsl_command(scripts, own),
            }
        }
        Integration::CmdPrompt => ShellCommand {
            arguments: own(),
            environment: vec![(
                OsString::from(CMD_PROMPT),
                cmd_prompt(environment.var_os(CMD_PROMPT)),
            )],
            profile_environment: Vec::new(),
            environment_derivation: EnvironmentDerivation::default(),
        },
        _ => ShellCommand {
            arguments: own(),
            environment: Vec::new(),
            profile_environment: Vec::new(),
            environment_derivation: EnvironmentDerivation::default(),
        },
    }
}

/// The launcher's whole command line: where to stand, then the question only the
/// distribution can answer, then both doors named in the distribution's own
/// spelling ([`WSL_LOGIN_SHELL`]).
///
/// Both, and not the one this side guessed at: which shell a `wsl.exe` logs the
/// reader into is a fact about a Linux password database, so the answer to
/// "bash's init file or zsh's directory" is reached inside the distribution and
/// each branch is handed what it needs. A path this machine keeps somewhere WSL
/// cannot name travels as the empty string, and the branch that needed it falls
/// through to the plain login shell — which is what every WSL pane did before
/// there was anything to hand over.
fn wsl_command(scripts: Scripts<'_>, own: &dyn Fn() -> Vec<OsString>) -> ShellCommand {
    let named = |path: Option<&Path>| {
        path.and_then(windows_to_wsl)
            .and_then(|path| path.into_os_string().into_string().ok())
            .unwrap_or_default()
    };
    let bash = named(scripts.bash);
    let zdotdir = named(scripts.zdotdir);
    if bash.is_empty() && zdotdir.is_empty() {
        return ShellCommand {
            arguments: own(),
            environment: Vec::new(),
            profile_environment: Vec::new(),
            environment_derivation: EnvironmentDerivation::default(),
        };
    }
    ShellCommand {
        arguments: own()
            .into_iter()
            .chain(
                [
                    "-e",
                    "sh",
                    "-c",
                    WSL_LOGIN_SHELL,
                    WSL_ARGV0,
                    &bash,
                    &zdotdir,
                ]
                .into_iter()
                .map(OsString::from),
            )
            .collect(),
        environment: crossing_environment(),
        profile_environment: Vec::new(),
        environment_derivation: EnvironmentDerivation::default(),
    }
}

/// `ZDOTDIR`, and the reader's own beside it when this window inherited one.
fn zdotdir_environment(
    ours: OsString,
    environment: &dyn ShellEnvironment,
) -> Vec<(OsString, OsString)> {
    let mut declared = vec![(OsString::from(ZDOTDIR), ours)];
    if let Some(theirs) = environment.var_os(ZDOTDIR) {
        declared.push((OsString::from(USER_ZDOTDIR), theirs));
    }
    declared
}

/// `FORCE_HYPERLINK=1`, unless somebody has already answered that question.
///
/// **This settles R-d** (`docs/M2-persistence-schema-v1.md` §296-299: "挂靠尚不
/// 存在的 per-profile 环境变量覆盖机制，profile 系统落地时一并做"). The variable
/// is the `supports-hyperlinks` convention — the crate half the Rust CLI
/// ecosystem asks before it will emit `OSC 8` — and its default answer is a
/// guess about the terminal made from `TERM` and a list of known names, which
/// this terminal is not on and will not be for years. It renders `OSC 8`. So
/// the answer is yes, and the terminal is the only party that knows it.
///
/// It used to be `folio.ps1` line 16 that said so, which made a
/// capability of the *terminal* a property of one profile's *opt-in script*:
/// hyperlinks worked in PowerShell if you had installed the script, and nowhere
/// else, for no reason a user could have discovered. It is stated here instead,
/// for every profile, on the channel the profile system now has — which is what
/// the deferred ruling was waiting for.
///
/// **Not stated for PowerShell**, and that is the one exception rather than an
/// oversight: its script still sets it, and setting it from both ends would mean
/// two places to change and one of them silently redundant. The script is also
/// the half that ships to a user who has installed it into a `pwsh` this
/// terminal did not start.
///
/// An inherited value of any kind — including `0`, which is how the crate spells
/// "no" — is left exactly as it is. This is a declaration, not an override: the
/// person who set it has answered the question already, and the whole point of
/// answering it is that somebody wanted a say.
///
/// **A row of this name in the profile's own environment is that same answer**
/// (§7.1.6c-6c), which is why it is asked here rather than left to the layering
/// below: `Force hyperlinks` = `On`/`Off` *is* that row, and a declaration
/// pushed on top of it would put two contradictory entries in one list — the
/// right one would still win at the far end, and the record every test and
/// every reader sees would still be a lie. `Auto` is the profile saying
/// nothing, so this behaves byte for byte as it did before the picker existed.
fn hyperlink_declaration(
    integration: Integration,
    environment: &dyn ShellEnvironment,
    mine: &[(String, String)],
) -> Option<(OsString, OsString)> {
    let answered = mine
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(FORCE_HYPERLINK));
    (integration != Integration::PowerShellOptIn
        && !answered
        && environment.var_os(FORCE_HYPERLINK).is_none())
    .then(|| (OsString::from(FORCE_HYPERLINK), OsString::from("1")))
}

/// The three variables that answer *which language and encoding a child works
/// in*, in the order `setlocale` lets them overrule each other.
///
/// Any one of them being set means the question has been answered already —
/// `LC_ALL` overrules everything, `LC_CTYPE` overrules `LANG` for the one
/// category a terminal cares about, and `LANG` is the answer itself.
const LOCALE_VARIABLES: [&str; 3] = ["LC_ALL", "LC_CTYPE", "LANG"];

/// **The system's locale, for a pane that would otherwise have none** (M1-5,
/// plan §8 Q8; T-MAC-LOCALE).
///
/// # Why this exists on one platform and not on the other
///
/// A Windows pane inherits a full environment from a logon session, and `LANG`
/// is not part of how that platform tells a child its encoding, so
/// [`bt_platform::system_locale_declaration`] answers `None` there and this
/// declares nothing — the Windows spawn is byte for byte what it was. An app
/// launched from Finder inherits `launchd`'s environment instead, which sets
/// `HOME`, `USER`, `SHELL`, `TMPDIR` and a bare `PATH` and **no `LC_*` at all**:
/// a shell started in it runs in the `C` locale, where a UTF-8 filename lists as
/// question marks and the three bytes of a Chinese character come back out of
/// the line editor as three separate characters. So the declaration is the
/// platform's fact rather than a preference, and the value is the system's own
/// setting rather than one this product picked — see that function for what it
/// refuses to invent, and for why a machine whose region names no installed
/// locale is told the encoding alone.
///
/// # The variables are the platform's to choose, not this function's
///
/// One or two of them, named and filled in by
/// [`bt_platform::LocaleDeclaration`]: `LANG` for the system's own locale, and
/// `LANG=C.UTF-8` with `LC_CTYPE=UTF-8` — Apple's own pair, which Terminal.app
/// declares in exactly this case — where no installed locale names the system's
/// language and region. Which fact was found and which variables say it are one
/// decision, made where the machine was read; this function's whole share of it
/// is *whether anyone is listening*.
///
/// # The one rule
///
/// **Anything that has already answered wins, and nothing is layered on top of
/// it.** A `LANG` this window inherited is the answer the launching environment
/// gave; an `LC_ALL` or `LC_CTYPE` beside it outranks `LANG` anyway, so
/// declaring one under either would put a variable in the record that changes
/// nothing. A row of any of the three in the profile's own environment is the
/// reader answering outright, which is [`hyperlink_declaration`]'s rule one
/// function up and for its reason: the row would still win at the far end, and
/// two contradictory entries in one list is a record that lies.
///
/// The reader's own file is the layer after this one and stays it: a `.zshrc`
/// that exports `LANG` is read by the shell this declaration starts, so the
/// person who set one keeps it.
///
/// **The Profiles page's ghost rows do not list this yet** — see
/// [`declared_environment`], which answers from an integration alone and has no
/// machine to ask. That page is drawn by a settings surface whose own port is a
/// later ticket, and a ghost row that claimed a locale variable for a window
/// that had inherited one would be the page saying something the spawn does not.
fn locale_declaration(
    system: Option<&LocaleDeclaration>,
    environment: &dyn ShellEnvironment,
    mine: &[(String, String)],
) -> Vec<(OsString, OsString)> {
    let Some(declaration) = system else {
        return Vec::new();
    };
    let answered = LOCALE_VARIABLES.iter().any(|name| {
        environment
            .var_os(name)
            .is_some_and(|value| !value.is_empty())
            || mine.iter().any(|(mine, _)| mine.eq_ignore_ascii_case(name))
    });
    if answered {
        return Vec::new();
    }
    declaration
        .variables()
        .iter()
        .map(|(name, value)| (OsString::from(*name), OsString::from(value)))
        .collect()
}

/// **Whether a session of this profile is told this terminal renders links** —
/// the hyperlink half of the honest capability sentence (J85).
///
/// Three facts and no probe, in the order they overrule each other:
///
/// 1. a `FORCE_HYPERLINK` row in the profile's own environment is the answer,
///    whatever it is: `0` is how the `supports-hyperlinks` convention spells
///    "no", anything else is a yes, and either way this profile has answered;
/// 2. otherwise a PowerShell's links come from `folio.ps1`, which declares them
///    only for a session whose `TERM_PROGRAM` it recognises as this terminal's
///    (`the_integration_script_knows_the_name_this_terminal_announces`) — so a
///    profile that overrides `TERM_PROGRAM` has switched its own links off, and
///    the sentence has to say so rather than repeat a promise the script will
///    not keep;
/// 3. otherwise this module declares them, for every door but PowerShell's.
///
/// **The environment this *window* inherited is deliberately not read.** It can
/// silence the declaration too (see above), but it is a fact about how Folio was
/// launched rather than about the profile, it is the same for every row on the
/// page, and the reader's answer to it is the row this function asks about
/// first.
#[must_use]
pub fn declares_hyperlinks(profile: &Profile) -> bool {
    if let Some((_, value)) = profile
        .env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(FORCE_HYPERLINK))
    {
        return value != "0";
    }
    if profiles::served_by(profile) != Integration::PowerShellOptIn {
        return true;
    }
    profile
        .env
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("TERM_PROGRAM"))
        .is_none_or(|(_, value)| value == bt_pty::TERM_PROGRAM)
}

/// The rows the editor's environment table draws on the reader's behalf —
/// **what this terminal itself will say to a session of this profile** (plan
/// §1.7, user ruling 2026-08-17 Q7).
///
/// Derived rather than a constant list, because a constant list was already
/// wrong in one place: `FORCE_HYPERLINK` is the one declaration this module does
/// *not* make for a PowerShell — its own script is the half that says it, and
/// saying it from both ends would be two places to change with one silently
/// redundant. A ghost drawn there would be this page pretending, which is what
/// this page exists to stop.
///
/// The reader's own rows are not filtered out here: the caller knows which names
/// it holds, and a ghost is dropped by the surface that can see both lists
/// (`settings::EditorSubject`).
#[must_use]
pub fn declared_environment(integration: Integration) -> Vec<(&'static str, &'static str)> {
    let mut declared = vec![
        ("TERM_PROGRAM", bt_pty::TERM_PROGRAM),
        ("COLORTERM", "truecolor"),
    ];
    if integration != Integration::PowerShellOptIn {
        declared.push((FORCE_HYPERLINK, "1"));
    }
    declared
}

/// `existing` — whatever `PROMPT` this process inherited — with the command
/// boundary markers and the working directory report in front of it.
///
/// The order is `133;D`, then `OSC 7`, then `133;A`, then the prompt somebody
/// wrote. It is `folio.bash`'s own order — the report is emitted immediately
/// before the prompt marker there too — so the two doors report in one order
/// and the page that documents them needs one sentence rather than two.
///
/// **Prefixed, never replaced.** A `PROMPT` in the environment is a prompt
/// somebody wrote: `setx PROMPT` is how a person keeps `$T$G` or a coloured
/// two-line prompt across sessions, and a terminal that overwrote it would have
/// silently taken their prompt away in exchange for a directory they cannot see.
/// In front rather than behind because the report must be printed before the
/// row the cursor ends on, and because a `PROMPT` ending in `$_` (a newline)
/// would otherwise push our escape onto the line the user types on.
///
/// The report is emitted **once per prompt**, which is once per command, which
/// is the same cadence every other profile's script reports at.
fn cmd_prompt(existing: Option<OsString>) -> OsString {
    let existing = existing.filter(|prompt| !prompt.is_empty());
    // Already ours. A `cmd` pane exports `PROMPT` to everything it starts, so a
    // Folio launched from one — or a `cmd` started inside a `cmd` —
    // inherits a string that already carries the report, and prefixing again
    // would print the directory twice per prompt and go on doubling.
    if existing
        .as_ref()
        .is_some_and(|prompt| prompt.to_string_lossy().contains(CMD_OSC7))
    {
        return existing.unwrap_or_default();
    }
    let mut prompt = OsString::from(CMD_MARKS_BEFORE_REPORT);
    prompt.push(CMD_OSC7);
    prompt.push(CMD_MARKS_AFTER_REPORT);
    prompt.push(existing.unwrap_or_else(|| OsString::from(CMD_DEFAULT_PROMPT)));
    prompt
}

/// The marker that tells the script it is being used as an init file, for the
/// door where this side of the boundary is the only side there is.
fn installed_environment(login: bool) -> Vec<(OsString, OsString)> {
    let mode = if login { MODE_LOGIN } else { MODE_INTERACTIVE };
    vec![(OsString::from(INSTALLED_MARKER), OsString::from(mode))]
}

/// The list of what to carry over the WSL boundary.
///
/// `WSLENV` is appended to rather than assigned, because it is a variable the
/// user may already be using to pass their own values into the distribution, and
/// replacing it would silently stop that.
///
/// **Listed for every WSL pane, whatever shell it turns out to log into**
/// (2026-09-07). It used to be listed only on the branch that had established
/// the shell was a bash, which made a terminal's own identity — `TERM_PROGRAM`,
/// `COLORTERM`, `FORCE_HYPERLINK` — a property of the reader's choice of shell:
/// a zsh user's pane rendered the same hyperlinks as anybody else's and told the
/// programs in it that it did not. Which shell answers is now the pane's own
/// business (see [`WSL_LOGIN_SHELL`]) and this side cannot know it, so the
/// question this function asks is the one it was always really asking —
/// *is this pane crossing into WSL* — and the answer no longer depends on a
/// probe that had not come back.
fn crossing_environment() -> Vec<(OsString, OsString)> {
    let inherited = std::env::var_os("WSLENV").unwrap_or_default();
    let mut list = inherited.to_string_lossy().into_owned();
    for name in FORWARDED {
        if !list.is_empty() && !list.ends_with(':') {
            list.push(':');
        }
        list.push_str(name);
        // `/u` is "Win32 to WSL only" — these describe the terminal on this
        // side of the boundary and mean nothing travelling the other way.
        list.push_str("/u");
    }
    vec![(OsString::from("WSLENV"), OsString::from(list))]
}

// ── PowerShell's own door: the profile, and the one line that opens it ──────
//
// The table at the top of this file says PowerShell's mechanism is "none — the
// user dot-sources it into `$PROFILE` themselves", and that stays true: nothing
// below runs on its own. What it adds is the ability to *offer* — to read the
// file, see that the line is not in it, and write the line if the reader asks
// for it in so many words. The asymmetry with bash is unchanged, because
// `--init-file` touches no file at all and this touches one that belongs to
// somebody, which is why a new mark is installed only on a press. Startup migrates
// existing marks; it never installs an absent one.

/// PowerShell's script, under the same roof as bash's.
const SCRIPT_PS1: &str = include_str!("../../../scripts/shell-integration/folio.ps1");

/// The name it is written under, beside [`SCRIPT_FILE`].
const SCRIPT_FILE_PS1: &str = "folio.ps1";

/// The sub-directory of `%APPDATA%\Folio\` both scripts live in.
const SCRIPT_DIRECTORY: &str = "shell-integration";

static POWERSHELL_INTEGRATION: AtomicBool = AtomicBool::new(true);
/// The durable `folio.ps1`, compared and repaired once per process — outside a trial, or after
/// its commit.
static DURABLE_POWERSHELL_SCRIPT: OnceLock<PathBuf> = OnceLock::new();
/// The script an update's trial names while its writes are held back (see
/// [`powershell_script_for_birth`]).
static TRIAL_POWERSHELL_SCRIPT: OnceLock<PathBuf> = OnceLock::new();
static POWERSHELL_PROFILE_LINE_PRESENT: AtomicBool = AtomicBool::new(false);

/// Publish the persisted answer for subsequent shell births.
pub fn set_powershell_integration(enabled: bool) {
    POWERSHELL_INTEGRATION.store(enabled, Ordering::Release);
}

#[must_use]
pub fn powershell_integration_enabled() -> bool {
    POWERSHELL_INTEGRATION.load(Ordering::Acquire)
}

/// The startup/removal worker's cached answer for the conditional Settings
/// verb. Reading Settings never touches the profile itself.
#[must_use]
pub fn powershell_profile_line_present() -> bool {
    POWERSHELL_PROFILE_LINE_PRESENT.load(Ordering::Acquire)
}

fn publish_powershell_profile_line_present(present: bool) {
    POWERSHELL_PROFILE_LINE_PRESENT.store(present, Ordering::Release);
}

/// Whether this program is a PowerShell at all.
///
/// The stem rather than the whole leaf, for [`profiles::derive_integration`]'s
/// reason, and the same two names it recognises — so a pane this function
/// answers `true` for is exactly a pane [`Integration::PowerShellOptIn`] serves.
/// It is the *gate* and not the answer: where that shell's `$PROFILE` is comes
/// from the shell itself ([`profile_probe`]).
#[must_use]
pub fn is_powershell(program: &Path) -> bool {
    let Some(leaf) = program.file_name() else {
        return false;
    };
    let leaf = leaf.to_string_lossy();
    let stem = leaf
        .rsplit_once('.')
        .map_or(leaf.as_ref(), |(stem, _)| stem)
        .to_ascii_lowercase();
    matches!(stem.as_str(), "pwsh" | "powershell")
}

// ── where the file is: asked of the shell, never composed ───────────────────

/// The one command: what this PowerShell calls its own `$PROFILE`.
///
/// `CurrentUserCurrentHost` because that is the file `$PROFILE` names when a
/// reader prints it — the same one every "add this to your `$PROFILE`"
/// instruction on the internet means — rather than `profile.ps1`, the all-hosts
/// file nobody is told about.
///
/// `-NoProfile` because a profile is exactly what must not run: it may print, it
/// may take seconds, and running the reader's startup file in order to find out
/// where their startup file is would be absurd. `-NonInteractive` so nothing can
/// stop for a prompt on a thread with no console.
///
/// After the path, the effective policy and then the five scopes it is decided from, one
/// `Scope=Policy` line each (`Get-ExecutionPolicy -List`), so the row can say who set a
/// refusing policy ([`policy_cause`]). Then two more facts the same run can answer without
/// running anything:
///
/// * `Zone=` — **the edition's own answer** to where its `RemoteSigned` would place the profile:
///   `ClrFacade.GetFileSecurityZone`, the function each edition's authorization manager itself
///   asks, reached by reflection (it is internal). Measured 2026-10-05 on both editions with a
///   file and its Mark of the Web, and through real UNC paths (`\\localhost\c$`,
///   `\\127.0.0.1\c$`): in all ten cases the answer predicted the edition's own decision — and
///   the editions differ. PowerShell 7 keys on the Mark of the Web alone (an unmarked file on any
///   share is `MyComputer` and loads); Windows PowerShell maps the path's URL zone as well (the
///   dotted share is `Internet` and is refused). An edition without the function answers
///   nothing, and then the next two lines are what can still be known.
/// * `Local=` and `Marked=` — by public means only: whether the profile's path is on a local
///   fixed drive (`[Uri]`, `[IO.DriveInfo]`), and whether the file carries a Mark of the Web
///   (`Get-Item -Stream Zone.Identifier`). Read only when the edition gave no zone answer
///   ([`remote_signed_loads`]): the zone function is internal, and a future edition that renames
///   it, or a language mode that refuses the reflection, must not take every local profile's
///   road away.
/// * `NoProcess=` — the effective policy once the probe's own Process scope is cleared (an
///   in-process change of the probe's own session; nothing persistent is written). When no other
///   scope is set this is the machine's default, which `Get-ExecutionPolicy` never names.
///
/// Windows only, like its one reader: off Windows the profile path is not asked.
#[cfg(windows)]
const PROFILE_COMMAND: &str = r#"[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding; $p = $PROFILE.CurrentUserCurrentHost; $p; (Get-ExecutionPolicy).ToString(); Get-ExecutionPolicy -List | ForEach-Object { '{0}={1}' -f $_.Scope, $_.ExecutionPolicy }; $z = 'Unknown'; try { $z = [string][psobject].Assembly.GetType('System.Management.Automation.ClrFacade').GetMethod('GetFileSecurityZone', [Reflection.BindingFlags]'NonPublic,Static').Invoke($null, @([string]$p)) } catch { }; 'Zone=' + $z; $local = $false; try { $local = (-not ([Uri]$p).IsUnc) -and ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($p)).DriveType -eq [IO.DriveType]::Fixed) } catch { }; 'Local=' + $local; 'Marked=' + [bool](Get-Item -LiteralPath $p -Stream Zone.Identifier -ErrorAction SilentlyContinue); Set-ExecutionPolicy -Scope Process -ExecutionPolicy Undefined -Force -ErrorAction SilentlyContinue; 'NoProcess=' + (Get-ExecutionPolicy).ToString()"#;

/// **The one command a user may run to let PowerShell load `$PROFILE`**: the CurrentUser scope,
/// which needs no elevation, set to `RemoteSigned`. Folio copies it on the user's click and
/// never runs it.
pub const POLICY_COMMAND: &str = "Set-ExecutionPolicy -Scope CurrentUser RemoteSigned";

/// The policy [`POLICY_COMMAND`] writes into the CurrentUser scope — what decides once it has
/// run, so what the profile's zone is judged against ([`policy_cause`]).
const POLICY_COMMAND_POLICY: crate::psreadline::ExecutionPolicy =
    crate::psreadline::ExecutionPolicy::RemoteSigned;

/// The five scopes `Get-ExecutionPolicy -List` reports, and the policy that applies when every
/// one of them is `Undefined`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PolicyScopes {
    machine_policy: crate::psreadline::ExecutionPolicy,
    user_policy: crate::psreadline::ExecutionPolicy,
    process: crate::psreadline::ExecutionPolicy,
    current_user: crate::psreadline::ExecutionPolicy,
    local_machine: crate::psreadline::ExecutionPolicy,
    /// What applies when no scope is set: the probe's `NoProcess=` answer, the effective policy
    /// with its own Process scope cleared. [`policy_cause`] reads it only when MachinePolicy,
    /// UserPolicy, CurrentUser and LocalMachine are all `Undefined` too — and then that answer is
    /// the machine's default, as PowerShell itself resolved it.
    default: crate::psreadline::ExecutionPolicy,
}

/// **What a row's own arguments say about its Process scope.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowProcessScope {
    /// The row carries no `-ExecutionPolicy`; the process scope is the environment's.
    Absent,
    /// The row's own `-ExecutionPolicy` — the **last** occurrence, as both editions' binders
    /// take it.
    Set(crate::psreadline::ExecutionPolicy),
    /// The classifier cannot read the row's arguments (a `-ep:Bypass` colon form, which neither
    /// binder reads as an option either; an option the table does not know), so this build
    /// cannot say what the process scope is — and makes no claim about the policy.
    Unreadable,
}

/// **Who decides that `$PROFILE` may not load**, for one row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyCause {
    /// The effective policy lets `$PROFILE` load.
    Allows,
    /// The CurrentUser or LocalMachine scope, or the default, refuses, and nothing above
    /// CurrentUser is set — so [`POLICY_COMMAND`] decides the effective policy once it is run.
    Changeable,
    /// Group Policy (MachinePolicy or UserPolicy) refuses; no user command changes it.
    Organisation,
    /// The process scope refuses: the row's own `-ExecutionPolicy`, or a
    /// `PSExecutionPolicyPreference` in the environment. It is above CurrentUser, so
    /// [`POLICY_COMMAND`] would not change it.
    Process,
    /// `RemoteSigned` is (or, after [`POLICY_COMMAND`], would be) the deciding policy and the
    /// edition says its `RemoteSigned` would not load the profile from where it is, or gave no answer.
    Location,
    /// The row's arguments cannot be read ([`RowProcessScope::Unreadable`]): no claim.
    Unreadable,
    /// A question this answer depends on got no answer — PowerShell did not answer in time, or
    /// the account's environment could not be read — so nothing is claimed, and the next visit
    /// to the Profiles page asks again (release read m2).
    Undetermined,
}

impl PolicyCause {
    /// The capability sentence for a cause that keeps `$PROFILE` from being offered; `None`
    /// when the policy allows it. An unreadable row says only what is true without a claim:
    /// the shipped "not provided" sentence.
    #[must_use]
    pub fn sentence(self) -> Option<crate::i18n::Text> {
        match self {
            Self::Allows => None,
            Self::Changeable => Some(crate::i18n::Text::CapPowerShellPolicyChangeable),
            Self::Organisation => Some(crate::i18n::Text::CapPowerShellPolicyManaged),
            Self::Process => Some(crate::i18n::Text::CapPowerShellPolicyProcess),
            Self::Location => Some(crate::i18n::Text::CapPowerShellPolicyLocation),
            Self::Unreadable => Some(crate::i18n::Text::CapPowerShellNotProvided),
            Self::Undetermined => Some(crate::i18n::Text::CapPowerShellUndetermined),
        }
    }
}

/// **Whether the managed line may be offered for a row, and what the row says when it may not**
/// (release read B1).
///
/// The line goes into the edition's `$PROFILE.CurrentUserCurrentHost`, which **every** session of
/// that edition reads — Folio's other rows, Windows Terminal, the Start menu's PowerShell, an
/// editor's terminal. So two sessions are asked, and both must load the file:
///
/// * **an ordinary session** of the edition: the scopes with no row's arguments, its Process scope
///   the one the account's fresh environment gives every session
///   ([`ProfileObservation::ordinary_process`]). If it would refuse the file, writing the file
///   would make every such session print "cannot be loaded" in red;
/// * **the clicked row**: its own `-ExecutionPolicy` as its Process scope ([`policy_cause`]). If it
///   refuses, the line would do nothing for the row it was asked for.
///
/// When only one refuses, its cause is the row's. When both do, the cause shown is the one that
/// decides what the user can do: Group Policy first (nothing the user runs changes it), then the
/// row's own Process scope (it outranks [`POLICY_COMMAND`], so the command would not help this
/// row), then the ordinary session's cause — whose command, when it is [`PolicyCause::Changeable`],
/// sets the CurrentUser scope every session of the account reads.
fn edition_cause(observed: &ProfileObservation, row: RowProcessScope) -> PolicyCause {
    let loads = observed.remote_signed_loads();
    let own = policy_cause(observed.scopes, loads, row);
    let ordinary = observed
        .ordinary_process
        .map_or(PolicyCause::Undetermined, |process| {
            policy_cause(observed.scopes, loads, RowProcessScope::Set(process))
        });
    both_sessions(ordinary, own)
}

/// [`edition_cause`]'s one decision, over the two sessions' causes.
fn both_sessions(ordinary: PolicyCause, own: PolicyCause) -> PolicyCause {
    use PolicyCause::{Allows, Organisation, Process, Undetermined, Unreadable};
    match (ordinary, own) {
        (_, Unreadable) => Unreadable,
        (Undetermined, _) | (_, Undetermined) => Undetermined,
        (Allows, own) => own,
        (ordinary, Allows) => ordinary,
        (Organisation, _) | (_, Organisation) => Organisation,
        (_, Process) => Process,
        (ordinary, _) => ordinary,
    }
}

impl PowerShellProfileFallback {
    /// The policy cause a refusing state stands for.
    #[must_use]
    pub fn policy_cause(self) -> Option<PolicyCause> {
        match self {
            Self::PolicyChangeable => Some(PolicyCause::Changeable),
            Self::PolicyManaged => Some(PolicyCause::Organisation),
            Self::PolicyProcess => Some(PolicyCause::Process),
            Self::PolicyLocation => Some(PolicyCause::Location),
            Self::Unreadable => Some(PolicyCause::Unreadable),
            Self::Undetermined => Some(PolicyCause::Undetermined),
            Self::NotNeeded
            | Self::Pending
            | Self::Offer
            | Self::Enabled
            | Self::NoProfile
            | Self::Unsupported => None,
        }
    }
}

/// **PowerShell's precedence, read once** — MachinePolicy, UserPolicy, Process, CurrentUser,
/// LocalMachine, then the default: the first scope that is not `Undefined` decides. The process
/// scope is the row's own `-ExecutionPolicy` when it carries one.
///
/// This is also the answer to "would [`POLICY_COMMAND`] make the policy permissive": `Changeable`
/// is returned only when nothing above CurrentUser is set, which is exactly when the CurrentUser
/// scope the command writes becomes the deciding one; anything set above it is the cause instead.
/// And whichever policy will decide — the one that does now, or [`POLICY_COMMAND_POLICY`] once
/// the command has run — when it is `RemoteSigned`, the edition has to have said it would load
/// the profile from where it is (`remote_signed_loads`, [`ProfileObservation`]), or the cause is
/// the location.
fn policy_cause(
    scopes: PolicyScopes,
    remote_signed_loads: Option<bool>,
    row: RowProcessScope,
) -> PolicyCause {
    use crate::psreadline::ExecutionPolicy;
    let set = |policy: &ExecutionPolicy| *policy != ExecutionPolicy::Undefined;
    let process = match row {
        RowProcessScope::Unreadable => return PolicyCause::Unreadable,
        RowProcessScope::Set(policy) => policy,
        RowProcessScope::Absent => scopes.process,
    };
    let (deciding, refused) = if let Some(policy) = [scopes.machine_policy, scopes.user_policy]
        .into_iter()
        .find(set)
    {
        (policy, PolicyCause::Organisation)
    } else if set(&process) {
        (process, PolicyCause::Process)
    } else {
        let policy = [scopes.current_user, scopes.local_machine]
            .into_iter()
            .find(set)
            .unwrap_or(scopes.default);
        (policy, PolicyCause::Changeable)
    };
    // A policy this build has no name for (`Unknown`: only a scope the probe printed in words a
    // newer PowerShell may use — every Process-scope value is read by
    // [`process_scope_value`]) is neither a refusal nor a permission this build can vouch for.
    if deciding == ExecutionPolicy::Unknown {
        return PolicyCause::Undetermined;
    }
    let (applies, cause) = if !deciding.blocks_script() {
        (deciding, PolicyCause::Allows)
    } else if refused == PolicyCause::Changeable {
        (POLICY_COMMAND_POLICY, PolicyCause::Changeable)
    } else {
        return refused;
    };
    if applies == ExecutionPolicy::RemoteSigned && remote_signed_loads != Some(true) {
        return PolicyCause::Location;
    }
    cause
}

/// **What a PowerShell makes of a Process-scope value** — the `-ExecutionPolicy` on its command
/// line, or the `PSExecutionPolicyPreference` in its environment (measured 2026-10-05 on both
/// editions, release read review F1): one of the five policy names, in any case and nothing
/// around it, is that policy; **anything else is `Restricted`** — `Undefined`, a name with a
/// space around it, garbage. Windows PowerShell runs such a session under `Restricted`;
/// PowerShell 7 does the same for the variable and refuses to start on such an
/// `-ExecutionPolicy`, so no session of that row loads anything either way. An empty variable is
/// no variable (Windows keeps no empty value), which the reader of the block decides.
fn process_scope_value(value: &str) -> crate::psreadline::ExecutionPolicy {
    use crate::psreadline::ExecutionPolicy::{
        AllSigned, Bypass, RemoteSigned, Restricted, Unrestricted,
    };
    [Restricted, AllSigned, RemoteSigned, Unrestricted, Bypass]
        .into_iter()
        .find(|policy| policy.name().eq_ignore_ascii_case(value))
        .unwrap_or(Restricted)
}

/// What the row's own arguments say about its Process scope — through the classifier, the one
/// reader of a PowerShell row: its parsed `executionpolicy` option, the last one as the binders
/// take it, or [`RowProcessScope::Unreadable`] when it cannot read the row at all.
fn row_process_scope(program: &Path, arguments: &[OsString]) -> RowProcessScope {
    let Some(parsed) = classify_powershell_arguments(program, arguments) else {
        return RowProcessScope::Unreadable;
    };
    parsed
        .non_terminal
        .iter()
        .rev()
        .find(|option| option.parameter == "executionpolicy")
        .and_then(|option| option.value)
        .map_or(RowProcessScope::Absent, |value| {
            RowProcessScope::Set(process_scope_value(&arguments[value].to_string_lossy()))
        })
}

/// **The path is asked of the shell and never composed**, and the machine this
/// was written on is why.
///
/// Its Documents folder is redirected to `D:\Documents`, and both PowerShells
/// answer `D:\Documents\…\Microsoft.PowerShell_profile.ps1`. `%USERPROFILE%\
/// Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` also exists
/// there, two bytes long, and no PowerShell has ever read it. A build that
/// spelled the path itself would have read that file, reported "not installed"
/// about a machine where the integration has been installed since 2026-08-14,
/// and written its line into a file no shell opens — the exact failure the
/// PSReadLine slice already ruled about (`psreadline::documents_directory`),
/// arrived at from the other end. Reading the known folder instead of
/// `%USERPROFILE%` fixes the common case and still leaves this build composing a
/// path on somebody else's behalf; the shell is the only party that knows, and
/// asking it costs one process per generation per launch.
///
/// The answer per program, because the two generations answer differently and a
/// reader's own profile row may name a third `pwsh` entirely.
///
/// **Only an answer is kept** (release read m2): a probe that ran out of time or failed is not an
/// answer about the file, and keeping it would drop that edition from every later removal for the
/// life of the process. The slot is held while its one question is asked, so concurrent askers
/// share an answer that arrives; after a failure the next asker asks again.
type ProfileAnswer = std::sync::Arc<Mutex<Option<PathBuf>>>;
type ProfileAnswers = std::collections::BTreeMap<PathBuf, ProfileAnswer>;
static PROFILE_ANSWERS: OnceLock<std::sync::Mutex<ProfileAnswers>> = OnceLock::new();

/// The two profile scopes a Windows account can carry. Every executable of one
/// edition reads the same CurrentUserCurrentHost profile, so this is the key
/// that makes two uncomposable rows flip together.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub enum PowerShellEdition {
    #[serde(rename = "windows_powershell")]
    WindowsPowerShell,
    #[serde(rename = "powershell_7")]
    PowerShellSeven,
}

fn powershell_edition(program: &Path) -> PowerShellEdition {
    if is_pwsh(program) {
        PowerShellEdition::PowerShellSeven
    } else {
        PowerShellEdition::WindowsPowerShell
    }
}

#[derive(Clone, Debug)]
struct ProfileObservation {
    path: PathBuf,
    scopes: PolicyScopes,
    /// **What the edition said about whether its `RemoteSigned` would load an unsigned
    /// `$PROFILE` from where it is** — its own zone answer ([`PROFILE_COMMAND`]'s `Zone=`):
    /// `Some(true)` for this computer, the intranet or a trusted site, `Some(false)` for the
    /// Internet or an untrusted site, `None` when the edition gave no answer.
    edition_says: Option<bool>,
    /// What can be known without the edition's answer ([`PROFILE_COMMAND`]'s `Local=`/`Marked=`).
    location: ProfileLocation,
    /// **The Process scope an ordinary session of the edition starts with** — the
    /// `PSExecutionPolicyPreference` of the account's fresh logon environment, the block every
    /// session started from the Start menu, Explorer or another terminal inherits; `Undefined`
    /// when the account sets none. The probe's own Process scope ([`PolicyScopes::process`]) is
    /// Folio's environment, which no other session shares. `None` when the fresh block could not
    /// be read: then what an ordinary session does is not known ([`PolicyCause::Undetermined`]).
    ordinary_process: Option<crate::psreadline::ExecutionPolicy>,
    line_present: bool,
}

/// Where `$PROFILE` is, as far as public means can say: the fallback's two facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProfileLocation {
    /// On a local fixed drive (not a share, not a removable drive).
    local_fixed: bool,
    /// The file carries a `Zone.Identifier` stream.
    marked: bool,
}

impl ProfileLocation {
    /// The kind of path, for the diagnostics line.
    fn kind(self) -> &'static str {
        match (self.local_fixed, self.marked) {
            (_, true) => "a profile carrying a Mark of the Web",
            (true, false) => "a profile on a local fixed drive",
            (false, false) => "a profile on a network or removable drive",
        }
    }
}

impl ProfileObservation {
    /// [`remote_signed_loads`] for this observation.
    fn remote_signed_loads(&self) -> Option<bool> {
        remote_signed_loads(self.edition_says, self.location)
    }
}

/// **Whether `RemoteSigned` would load the profile from where it is** — the edition's own answer
/// when it gave one, in every case. Without one, the one fallback this build keeps: an unmarked
/// profile on a local fixed drive loads under `RemoteSigned` on both editions (measured
/// 2026-10-05, round 6), so its road stays; a marked file or a network or removable path cannot
/// be known without the edition, and is `None` — the Location sentence.
fn remote_signed_loads(edition_says: Option<bool>, location: ProfileLocation) -> Option<bool> {
    edition_says.or((location.local_fixed && !location.marked).then_some(true))
}

/// **The editions whose missing zone answer has been said in diagnostics** — once per edition
/// per process, so a future break of the internal zone function is noticed without a line per
/// probe.
#[derive(Default)]
struct FallbackNotices(Mutex<std::collections::BTreeSet<PowerShellEdition>>);

impl FallbackNotices {
    /// The diagnostics line for this edition's first unanswered observation; `None` after it.
    fn first(&self, program: &Path, location: ProfileLocation) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(powershell_edition(program))
            .then(|| {
                format!(
                    "BT_SHELL_PROFILE {}: the edition did not answer whether RemoteSigned loads \
                     {}; using the fallback",
                    program.display(),
                    location.kind()
                )
            })
    }
}

static FALLBACK_NOTICES: OnceLock<FallbackNotices> = OnceLock::new();

static PROFILE_OBSERVATIONS: OnceLock<
    Mutex<BTreeMap<PowerShellEdition, Heard<ProfileObservation>>>,
> = OnceLock::new();

/// **What has been heard from a question asked in the background**: nothing yet, a failure (the
/// question was asked and got no answer), or the answer. A failure is not an answer about the
/// row, and the row says so (release read m2).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Heard<T> {
    NotYet,
    Failed,
    Answer(T),
}

/// What an uncomposable PowerShell row can honestly offer on the Profiles page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerShellProfileFallback {
    /// The argv can receive process-scoped integration; no persistent fallback is needed.
    NotNeeded,
    /// A command-bearing row is still waiting for its asynchronous parse fact, or the edition's
    /// first `$PROFILE` and policy observation has not landed yet.
    Pending,
    /// The managed line can be added with one press.
    Offer,
    /// This edition's profile already carries an active Folio line.
    Enabled,
    /// The row explicitly tells PowerShell not to load profiles.
    NoProfile,
    /// The policy refuses `$PROFILE` and [`POLICY_COMMAND`] would change that
    /// ([`PolicyCause::Changeable`]).
    PolicyChangeable,
    /// Group Policy refuses `$PROFILE` ([`PolicyCause::Organisation`]).
    PolicyManaged,
    /// The process scope refuses `$PROFILE` ([`PolicyCause::Process`]).
    PolicyProcess,
    /// `RemoteSigned` decides and `$PROFILE`'s zone is one it refuses or one this edition cannot
    /// name ([`PolicyCause::Location`]).
    PolicyLocation,
    /// The row's arguments cannot be read, so nothing is claimed and nothing offered
    /// ([`PolicyCause::Unreadable`]).
    Unreadable,
    /// A question the row depends on — the parse answer, or the edition's observation — was
    /// asked and got no answer; the next visit to the Profiles page asks again
    /// ([`PolicyCause::Undetermined`]).
    Undetermined,
    /// This platform has no `$PROFILE` probe ([`PROFILE_PROBE_EXISTS`]), so nothing is observed
    /// and nothing is offered.
    Unsupported,
}

fn asks_for_no_profile(program: &Path, arguments: &[OsString]) -> bool {
    classify_powershell_arguments(program, arguments).is_some_and(|parsed| {
        parsed
            .non_terminal
            .iter()
            .any(|option| option.parameter == "noprofile")
    })
}

/// The Profiles page's row model, kept pure apart from reading the latest
/// worker-published edition fact.
#[must_use]
pub fn powershell_profile_fallback(
    program: &Path,
    arguments: &[OsString],
    integration_enabled: bool,
) -> PowerShellProfileFallback {
    let composable = integration_enabled && powershell_arguments_are_safe(program, arguments);
    let no_profile = asks_for_no_profile(program, arguments);
    let parse = if classify_powershell_arguments(program, arguments).is_some_and(|parsed| {
        matches!(
            parsed.terminal,
            PowerShellTerminal::Command { .. } | PowerShellTerminal::EncodedCommand { .. }
        )
    }) {
        heard_parse(program, arguments)
    } else {
        Heard::Answer(())
    };
    let observed = PROFILE_OBSERVATIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&powershell_edition(program))
        .cloned()
        .unwrap_or(Heard::NotYet);
    let row = row_process_scope(program, arguments);
    profile_fallback_from_parts(
        PROFILE_PROBE_EXISTS,
        integration_enabled,
        composable,
        no_profile,
        parse,
        match observed {
            Heard::NotYet => Heard::NotYet,
            Heard::Failed => Heard::Failed,
            Heard::Answer(observed) => {
                Heard::Answer((edition_cause(&observed, row), observed.line_present))
            }
        },
    )
}

/// The row model's one decision. `Offer` needs an observation of the row's edition: before the
/// first one lands the row is `Pending`, and on a platform with no probe it never lands, which
/// is `Unsupported` rather than a wait. A question that was asked and got no answer — the parse
/// probe, or the observation — is `Undetermined`, never the "not provided" of a refusal.
///
/// **A line that is present is `Enabled` only when both sessions load it** ([`edition_cause`]):
/// a line an ordinary session refuses — written by an older Folio, or by hand, before the policy
/// tightened — is the policy's row, with its sentence and, where the user can change it, its
/// Copy button; the line's removal is the Settings row "PowerShell $PROFILE line", shown while
/// the line is present.
fn profile_fallback_from_parts(
    probe_exists: bool,
    integration_enabled: bool,
    composable: bool,
    no_profile: bool,
    parse: Heard<()>,
    observed: Heard<(PolicyCause, bool)>,
) -> PowerShellProfileFallback {
    if composable {
        return PowerShellProfileFallback::NotNeeded;
    }
    if !integration_enabled || no_profile {
        return PowerShellProfileFallback::NoProfile;
    }
    if !probe_exists {
        return PowerShellProfileFallback::Unsupported;
    }
    match (parse, observed) {
        (Heard::NotYet, _) | (Heard::Answer(()), Heard::NotYet) => {
            PowerShellProfileFallback::Pending
        }
        (Heard::Failed, _) | (Heard::Answer(()), Heard::Failed) => {
            PowerShellProfileFallback::Undetermined
        }
        (Heard::Answer(()), Heard::Answer((cause, line_present))) => match cause {
            PolicyCause::Changeable => PowerShellProfileFallback::PolicyChangeable,
            PolicyCause::Organisation => PowerShellProfileFallback::PolicyManaged,
            PolicyCause::Process => PowerShellProfileFallback::PolicyProcess,
            PolicyCause::Location => PowerShellProfileFallback::PolicyLocation,
            PolicyCause::Unreadable => PowerShellProfileFallback::Unreadable,
            PolicyCause::Undetermined => PowerShellProfileFallback::Undetermined,
            PolicyCause::Allows if line_present => PowerShellProfileFallback::Enabled,
            PolicyCause::Allows => PowerShellProfileFallback::Offer,
        },
    }
}

fn publish_profile_observation(program: &Path, observed: ProfileObservation) {
    if observed.edition_says.is_none()
        && let Some(line) = FALLBACK_NOTICES
            .get_or_init(Default::default)
            .first(program, observed.location)
    {
        eprintln!("{line}");
    }
    PROFILE_OBSERVATIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(powershell_edition(program), Heard::Answer(observed));
}

/// **The edition was asked and did not answer** (release read m2): its row says so instead of
/// keeping an older answer or reading as a refusal, until the next visit asks again.
fn publish_profile_observation_failed(program: &Path) {
    PROFILE_OBSERVATIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(powershell_edition(program), Heard::Failed);
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ParseKey {
    program: PathBuf,
    arguments: Vec<OsString>,
}

const PARSE_PROBE_ATTEMPT_LIMIT: u8 = 3;
const PROBE_OUTPUT_PREFIX_LIMIT: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ParseAnswer {
    Pending {
        attempt: u8,
    },
    Valid,
    Invalid,
    Failed {
        attempts: u8,
        failure: ParseProbeFailure,
    },
}

/// Process-owned answers from the target PowerShell parser.
///
/// The executable in the key is the already-resolved [`profiles::ProfilePrograms`]
/// answer and the argv is exact except for a starting-directory launcher pair.
/// That pair is supplied only at birth and cannot change the command grammar;
/// removing it makes the startup question and the birth name the same fact.
/// Editing any command row therefore still makes a new key and a new question
/// without invalidating an answer another row may still use.
type ParseAnswers = BTreeMap<ParseKey, ParseAnswer>;
static PARSE_ANSWERS: OnceLock<Mutex<ParseAnswers>> = OnceLock::new();

fn parse_key(program: &Path, arguments: &[OsString]) -> ParseKey {
    let mut arguments = arguments.to_vec();
    if let Some(parsed) = classify_powershell_arguments(program, &arguments) {
        let mut launcher = parsed
            .non_terminal
            .iter()
            .filter(|option| option.parameter == "workingdirectory")
            .filter_map(|option| option.value.map(|value| (option.option, value)))
            .collect::<Vec<_>>();
        launcher.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        for (option, value) in launcher {
            arguments.drain(option..=value);
        }
    }
    ParseKey {
        program: profile_key(program),
        arguments,
    }
}

fn cached_parse_answer(program: &Path, arguments: &[OsString]) -> Option<bool> {
    match PARSE_ANSWERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&parse_key(program, arguments))
    {
        Some(ParseAnswer::Valid) => Some(true),
        Some(ParseAnswer::Invalid) => Some(false),
        Some(ParseAnswer::Pending { .. } | ParseAnswer::Failed { .. }) | None => None,
    }
}

/// What the Profiles row has heard from the parse question about this exact row: an answer
/// (either way), a failure (the last attempt got no answer), or nothing yet.
fn heard_parse(program: &Path, arguments: &[OsString]) -> Heard<()> {
    match PARSE_ANSWERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&parse_key(program, arguments))
    {
        Some(ParseAnswer::Valid | ParseAnswer::Invalid) => Heard::Answer(()),
        Some(ParseAnswer::Failed { .. }) => Heard::Failed,
        Some(ParseAnswer::Pending { .. }) | None => Heard::NotYet,
    }
}

fn command_text(program: &Path, arguments: &[OsString]) -> Option<String> {
    let parsed = classify_powershell_arguments(program, arguments)?;
    if parsed.non_interactive {
        return None;
    }
    match parsed.terminal {
        PowerShellTerminal::Command { text, .. }
        | PowerShellTerminal::EncodedCommand { text, .. } => Some(text),
        PowerShellTerminal::None
        | PowerShellTerminal::File
        | PowerShellTerminal::CommandWithArgs
        | PowerShellTerminal::Stdin => None,
    }
}

#[derive(Clone)]
struct ParseQuestion {
    key: ParseKey,
    program: PathBuf,
    text: String,
}

impl ParseQuestion {
    fn new(program: &Path, arguments: &[OsString], text: String) -> Self {
        Self {
            key: parse_key(program, arguments),
            program: program.to_path_buf(),
            text,
        }
    }
}

/// Every command-bearing PowerShell row of `rows` that `programs` can start, as the question its
/// target parser is asked.
fn parse_questions(
    rows: &[profiles::Profile],
    programs: &profiles::ProfilePrograms,
) -> Vec<ParseQuestion> {
    rows.iter()
        .filter_map(|profile| {
            let program = PathBuf::from(programs.program(&profile.id)?);
            if !is_powershell(&program) {
                return None;
            }
            let arguments = profiles::launch_args(profile)
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>();
            let text = command_text(&program, &arguments)?;
            Some(ParseQuestion::new(&program, &arguments, text))
        })
        .collect()
}

#[derive(Clone)]
struct ParseAttempt {
    question: ParseQuestion,
    number: u8,
}

/// **Who asks a parse question**, and so whether the attempt limit applies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParseAsker {
    /// Startup, a profile-table change or a birth: at most [`PARSE_PROBE_ATTEMPT_LIMIT`] attempts
    /// per exact row in a process, so a host that keeps failing goes quiet.
    Background,
    /// A visit to the Profiles page — an edge a person makes, never a loop: a failed question is
    /// asked once more whatever the count (release read m2), as the page's observation is.
    Visit,
}

/// Claim one bounded attempt before starting any worker or process. A failed
/// answer remains unknown, and the next birth may claim the next attempt; a
/// pending or grammatical answer starts nothing, and neither does an exhausted one unless a
/// visit asks ([`ParseAsker::Visit`]).
fn claim_parse_attempt(question: ParseQuestion, asker: ParseAsker) -> Option<ParseAttempt> {
    let number = {
        let mut answers = PARSE_ANSWERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let number = match answers.get(&question.key) {
            None => 1,
            Some(ParseAnswer::Failed { attempts, .. })
                if *attempts < PARSE_PROBE_ATTEMPT_LIMIT || asker == ParseAsker::Visit =>
            {
                attempts.saturating_add(1)
            }
            Some(
                ParseAnswer::Pending { .. }
                | ParseAnswer::Valid
                | ParseAnswer::Invalid
                | ParseAnswer::Failed { .. },
            ) => return None,
        };
        answers.insert(
            question.key.clone(),
            ParseAnswer::Pending { attempt: number },
        );
        number
    };
    Some(ParseAttempt { question, number })
}

fn publish_parse_attempt(key: ParseKey, number: u8, result: Result<bool, ParseProbeFailure>) {
    let answer = match result {
        Ok(true) => ParseAnswer::Valid,
        Ok(false) => ParseAnswer::Invalid,
        Err(failure) => ParseAnswer::Failed {
            attempts: number,
            failure,
        },
    };
    let mut answers = PARSE_ANSWERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if matches!(
        answers.get(&key),
        Some(ParseAnswer::Pending { attempt }) if *attempt == number
    ) {
        answers.insert(key, answer);
    }
    drop(answers);
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

fn run_parse_attempt(attempt: ParseAttempt, parse: ParseProbe) {
    let result = parse(&attempt.question.program, &attempt.question.text);
    publish_parse_attempt(attempt.question.key, attempt.number, result);
}

fn ask_parse_question(question: ParseQuestion, asker: ParseAsker, parse: ParseProbe) {
    if let Some(attempt) = claim_parse_attempt(question, asker) {
        run_parse_attempt(attempt, parse);
    }
}

/// **The Profiles page's visit asks every failed parse question again** (release read m2), on
/// the visit's own observation worker; a question with an answer, or one in flight, is not asked.
fn ask_failed_parse_questions_again(questions: Vec<ParseQuestion>) {
    for question in questions {
        ask_parse_question(question, ParseAsker::Visit, run_parse_probe);
    }
}

/// A birth that meets a transient failure starts at most one background retry
/// for the exact row and then continues with the original argv. Claiming before
/// the thread door coalesces concurrent births; the attempt limit makes a
/// permanently failing host quiet for the rest of this process.
fn request_parse_retry(
    question: ParseQuestion,
    start: impl FnOnce(ParseAttempt) -> Result<(), String>,
) {
    let Some(attempt) = claim_parse_attempt(question, ParseAsker::Background) else {
        return;
    };
    let key = attempt.question.key.clone();
    let number = attempt.number;
    if let Err(error) = start(attempt) {
        publish_parse_attempt(key, number, Err(ParseProbeFailure::WorkerSpawn(error)));
    }
}

fn schedule_parse_retry(question: ParseQuestion) {
    request_parse_retry(question, |attempt| {
        spawn_powershell_preparation(
            PowershellPreparation::ParseAttempt(attempt),
            PreparationEffects::MACHINE,
        )
    });
}

const PARSE_COMMAND: &str = "$enc=[Text.UTF8Encoding]::new($false);[Console]::InputEncoding=$enc;[Console]::OutputEncoding=$enc;$source=[Console]::In.ReadToEnd();$tokens=$null;$errors=$null;[System.Management.Automation.Language.Parser]::ParseInput($source,[ref]$tokens,[ref]$errors)>$null;if($errors.Count -eq 0){[Console]::Out.Write('1')}else{[Console]::Out.Write('0')}";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProbeOutput {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ParseProbeFailure {
    WorkerSpawn(String),
    Spawn(String),
    Stdin {
        error: String,
        stdout: String,
        stderr: String,
    },
    Deadline {
        stdout: String,
        stderr: String,
    },
    Wait {
        error: String,
        stdout: String,
        stderr: String,
    },
    Exit {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    OutputNotRecognised {
        stdout: String,
        stderr: String,
    },
}

/// The deadline shared by every PowerShell machine probe.
pub(crate) const POWERSHELL_PROBE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

impl std::fmt::Display for ParseProbeFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WorkerSpawn(error) => write!(formatter, "worker spawn: {error}"),
            Self::Spawn(error) => write!(formatter, "process spawn: {error}"),
            Self::Stdin {
                error,
                stdout,
                stderr,
            } => write!(
                formatter,
                "stdin write: {error}; stdout first bytes {stdout}; stderr first bytes {stderr}"
            ),
            Self::Deadline { stdout, stderr } => write!(
                formatter,
                "five-second deadline; stdout first bytes {stdout}; stderr first bytes {stderr}"
            ),
            Self::Wait {
                error,
                stdout,
                stderr,
            } => write!(
                formatter,
                "process wait: {error}; stdout first bytes {stdout}; stderr first bytes {stderr}"
            ),
            Self::Exit {
                code,
                stdout,
                stderr,
            } => write!(
                formatter,
                "exit status {code:?}; stdout first bytes {stdout}; stderr first bytes {stderr}"
            ),
            Self::OutputNotRecognised { stdout, stderr } => write!(
                formatter,
                "output not recognised; stdout first bytes {stdout}; stderr first bytes {stderr}"
            ),
        }
    }
}

fn output_prefix(bytes: &[u8]) -> String {
    let mut rendered = bytes
        .iter()
        .take(PROBE_OUTPUT_PREFIX_LIMIT)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > PROBE_OUTPUT_PREFIX_LIMIT {
        rendered.push_str(" …");
    }
    format!("[{rendered}]")
}

/// How a parse question is put to a PowerShell: [`run_parse_probe`], or a test's stand-in.
type ParseProbe = fn(&Path, &str) -> Result<bool, ParseProbeFailure>;

fn run_parse_probe(program: &Path, text: &str) -> Result<bool, ParseProbeFailure> {
    let output = run_powershell_probe(
        program,
        PARSE_COMMAND,
        Some(text),
        POWERSHELL_PROBE_DEADLINE,
    )?;
    parse_probe_answer(output)
}

fn parse_probe_answer(output: ProbeOutput) -> Result<bool, ParseProbeFailure> {
    match output.stdout.as_slice() {
        b"1" => Ok(true),
        b"0" => Ok(false),
        _ => Err(ParseProbeFailure::OutputNotRecognised {
            stdout: output_prefix(&output.stdout),
            stderr: output_prefix(&output.stderr),
        }),
    }
}

fn profile_key(program: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(
            program
                .as_os_str()
                .to_string_lossy()
                .replace('/', "\\")
                .to_lowercase(),
        )
    }
    #[cfg(not(windows))]
    {
        program.to_path_buf()
    }
}

/// Resolving executable aliases can touch disk, so it happens only on workers.
/// All aliases point at the same slot before any worker asks PowerShell. `deadline` is the
/// asker's patience ([`POWERSHELL_PROBE_DEADLINE`] for the Profiles page, [`REMOVAL_PROBE_DEADLINE`] for a
/// removal somebody asked for).
fn cached_profile_answer(program: &Path, deadline: std::time::Duration) -> Option<PathBuf> {
    let resolved = bt_platform::program_on_path(program).unwrap_or_else(|| program.to_path_buf());
    let canonical = std::fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
    let slot = {
        let mut answers = PROFILE_ANSWERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let original = answers.entry(profile_key(program)).or_default().clone();
        let slot = answers
            .entry(profile_key(&canonical))
            .or_insert(original)
            .clone();
        answers.insert(profile_key(program), slot.clone());
        slot
    };
    answer_once(&slot, || run_profile_path_probe(&resolved, deadline))
}

/// The slot's answer, asking for it only when there is none: an answer is kept, a failure is
/// not, and the question is asked with the slot held, so an asker that arrives meanwhile reads
/// the answer instead of asking again.
fn answer_once(
    slot: &Mutex<Option<PathBuf>>,
    ask: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    let mut held = slot.lock().unwrap_or_else(|error| error.into_inner());
    if held.is_none() {
        *held = ask();
    }
    held.clone()
}

#[cfg(windows)]
fn installed_powershells() -> Vec<PathBuf> {
    let mut programs = Vec::new();
    // Use the same installation discovery as the shipped panes, including
    // PowerShell 7 installed outside PATH. No Documents paths are composed.
    let rows = profiles::shipped_for(
        profiles::SeedPlatform::Windows,
        &bt_pty::SystemShellEnvironment,
    );
    let resolved = profiles::ProfilePrograms::probe_rows(&rows, &bt_pty::SystemShellEnvironment);
    for id in ["pwsh", "winps"] {
        if let Some(program) = resolved.program(id).map(PathBuf::from)
            && is_powershell(&program)
            && !programs.contains(&program)
        {
            programs.push(program);
        }
    }
    for name in ["powershell.exe", "pwsh.exe"] {
        if let Some(program) = bt_platform::program_on_path(Path::new(name))
            && !programs.contains(&program)
        {
            programs.push(program);
        }
    }
    programs
}

#[cfg(not(windows))]
fn installed_powershells() -> Vec<PathBuf> {
    Vec::new()
}

/// Every answer this module publishes out of band is published on a thread with
/// no window, so the window has to be told to come and read it.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Teach the probe how to bring the event loop round when an answer lands.
///
/// [`crate::psreadline::install_wake`]'s twin and for its reason: a pane that
/// has started, printed its prompt and gone quiet produces no further frame on
/// its own, so an answer that landed after that frame would sit unread until the
/// reader typed something.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

/// **The probe's PowerShell finds its modules where a pane's would, not where Folio's parent
/// put them.** A pane's environment is the fresh logon block on Windows (T-ENV-REFRESH), but a
/// child of this door inherits Folio's own — and a Folio started from a PowerShell 7 session
/// carries 7's `PSModulePath`, with which a Windows PowerShell probe loads 7's
/// `Microsoft.PowerShell.Security` and finds no `Get-ExecutionPolicy` (measured 2026-10-05). Without
/// the variable each edition computes its own module path; every command this door runs (the
/// parser, `$PROFILE`, the policy cmdlets, the zone question) is answered by the edition's inbox
/// modules, which that path always holds. Off Windows a pane inherits, and so does the probe.
pub(crate) fn powershell_probe_command(
    program: &Path,
) -> Result<std::process::Command, ParseProbeFailure> {
    #[cfg(windows)]
    {
        bt_platform::quiet_command_named(program)
            .map(|mut command| {
                command.env_remove("PSModulePath");
                command
            })
            .ok_or_else(|| {
                ParseProbeFailure::Spawn(format!(
                    "the named child-process door could not resolve {}",
                    program.display()
                ))
            })
    }
    #[cfg(not(windows))]
    {
        Ok(bt_platform::quiet_command(program))
    }
}

fn stopped_output(mut child: bt_platform::ProbeChild) -> ProbeOutput {
    let _ = child.kill();
    child.wait_with_output().map_or_else(
        |_| ProbeOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
        |output| {
            bt_platform::file_reads::pipe_output(bt_platform::file_reads::Lane::Settings, &output);
            ProbeOutput {
                stdout: output.stdout,
                stderr: output.stderr,
            }
        },
    )
}

/// **How long the one question a removal needs may take** — where a PowerShell keeps `$PROFILE`,
/// asked by the Settings remover, `--remove-shell-integration` and both uninstall verbs: an
/// operation somebody asked for once, which is not asked again by itself. A Windows PowerShell
/// whose module analysis cache is cold can take tens of seconds before its first command
/// (measured 22–46 s on a CI image, T-INTEGRATION-INJECT-4 round 7); the path question runs no
/// command, so this is the bound on a start, not on a lookup (release read M1).
const REMOVAL_PROBE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn run_powershell_probe(
    program: &Path,
    command: &str,
    input: Option<&str>,
    deadline_after: std::time::Duration,
) -> Result<ProbeOutput, ParseProbeFailure> {
    // Through the quiet door (§7.40 ①): without `CREATE_NO_WINDOW` a console
    // window opens on screen the first time a PowerShell pane is opened.
    //
    // **And through the named door** (R1-17): a profile may spell its shell
    // `pwsh.exe` with no path at all, and a bare name is resolved by
    // `CreateProcess` out of the working directory before `PATH`. The probe
    // asks about the program a pane will run, so it asks about the one an
    // administrator installed.
    let mut probe = powershell_probe_command(program)?;
    probe.args(["-NoProfile", "-NonInteractive", "-Command", command]);
    let stdio = if input.is_some() {
        bt_platform::ProbeStdio::FED
    } else {
        bt_platform::ProbeStdio::ANSWER
    };
    let mut child = bt_platform::spawn_probe(&mut probe, stdio)
        .map_err(|error| ParseProbeFailure::Spawn(error.to_string()))?;
    if let Some(input) = input {
        let written = child.take_stdin().map_or_else(
            || Err("the piped stdin handle was absent".to_owned()),
            |mut stdin| {
                stdin
                    .write_all(input.as_bytes())
                    .map_err(|error| error.to_string())
            },
        );
        if let Err(error) = written {
            let output = stopped_output(child);
            return Err(ParseProbeFailure::Stdin {
                error,
                stdout: output_prefix(&output.stdout),
                stderr: output_prefix(&output.stderr),
            });
        }
    }
    let _started_pid = child.id(); // Only this owned child may be stopped.
    let deadline = std::time::Instant::now() + deadline_after;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Ok(None) => {
                let output = stopped_output(child);
                return Err(ParseProbeFailure::Deadline {
                    stdout: output_prefix(&output.stdout),
                    stderr: output_prefix(&output.stderr),
                });
            }
            Err(error) => {
                let output = stopped_output(child);
                return Err(ParseProbeFailure::Wait {
                    error: error.to_string(),
                    stdout: output_prefix(&output.stdout),
                    stderr: output_prefix(&output.stderr),
                });
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| ParseProbeFailure::Wait {
            error: error.to_string(),
            stdout: "[]".to_owned(),
            stderr: "[]".to_owned(),
        })?;
    bt_platform::file_reads::pipe_output(bt_platform::file_reads::Lane::Settings, &output);
    if !output.status.success() {
        return Err(ParseProbeFailure::Exit {
            code: output.status.code(),
            stdout: output_prefix(&output.stdout),
            stderr: output_prefix(&output.stderr),
        });
    }
    Ok(ProbeOutput {
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// **Where this PowerShell keeps `$PROFILE`, and nothing else** — the one question a removal
/// needs (release read M1). No cmdlet runs: a cmdlet is found through command discovery, which is
/// what a cold module analysis cache makes slow, while `$PROFILE` is a variable the host sets
/// before its first command. Windows only, like [`PROFILE_COMMAND`].
#[cfg(windows)]
const PROFILE_PATH_COMMAND: &str =
    "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); $PROFILE.CurrentUserCurrentHost";

#[cfg(windows)]
fn run_profile_path_probe(program: &Path, patience: std::time::Duration) -> Option<PathBuf> {
    if profile_sandboxed() {
        return None;
    }
    let output = run_powershell_probe(program, PROFILE_PATH_COMMAND, None, patience).ok()?;
    parse_profile_path(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(not(windows))]
fn run_profile_path_probe(_program: &Path, _patience: std::time::Duration) -> Option<PathBuf> {
    None
}

/// The path [`PROFILE_PATH_COMMAND`] prints: its first line, taken verbatim, and only when it is
/// absolute — whatever the shell names is the file, wherever it is.
#[cfg(windows)]
fn parse_profile_path(stdout: &str) -> Option<PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// **Whether this platform can ask a PowerShell for its `$PROFILE` and execution policy** — the
/// one place that platform fact lives, beside the probe it describes. Off Windows
/// [`probe_profile_observation`] answers nothing, so the Profiles row model
/// ([`profile_fallback_from_parts`]) offers no fallback there.
const PROFILE_PROBE_EXISTS: bool = cfg!(windows);

fn probe_profile_observation(
    worker: &bt_platform::admission::WorkerCtx,
    program: &Path,
) -> Option<ProfileObservation> {
    probe_profile_observation_unless(profile_sandboxed(), || {
        #[cfg(windows)]
        {
            let output =
                run_powershell_probe(program, PROFILE_COMMAND, None, POWERSHELL_PROBE_DEADLINE)
                    .ok()?;
            let mut observed =
                parse_profile_observation(std::str::from_utf8(&output.stdout).ok()?)?;
            let fresh = bt_platform::environment::fresh_logon_environment(worker)
                .ok()
                .flatten();
            observed.ordinary_process = ordinary_process_scope(fresh.as_deref());
            Some(observed)
        }
        #[cfg(not(windows))]
        {
            let _ = (worker, program);
            None
        }
    })
}

/// **The `$PROFILE` sandbox replaces every real profile, so a sandboxed run asks no shell about
/// one** (`BT_POWERSHELL_PROFILE`, the test door — `warm_profile_answers` already kept that
/// promise; the edition observation added in this ticket did not, and a sandboxed test's child
/// started a real PowerShell per edition and read the real `$PROFILE`'s path inside a 3-second
/// wait: U-35's `a_start_that_is_no_trial_writes_as_it_always_has` on CI, round 6).
fn probe_profile_observation_unless(
    sandboxed: bool,
    ask: impl FnOnce() -> Option<ProfileObservation>,
) -> Option<ProfileObservation> {
    if sandboxed { None } else { ask() }
}

/// Whether the `$PROFILE` sandbox is set: the question the probes and `warm_profile_answers` ask
/// before starting a shell. The variable is read twice more, each for its value: by
/// `profile_runtime::sandbox_profile` (the sandboxed profile's path) and on the uninstaller's own
/// road (`uninstall.rs`).
fn profile_sandboxed() -> bool {
    std::env::var_os("BT_POWERSHELL_PROFILE").is_some()
}

/// Read what [`PROFILE_COMMAND`] writes: the profile path, then the effective execution policy.
/// Windows only, like the probe that is its one reader and the test that pins it.
#[cfg(windows)]
fn parse_profile_observation(stdout: &str) -> Option<ProfileObservation> {
    use crate::psreadline::ExecutionPolicy;
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let path = PathBuf::from(lines.next()?);
    if !path.is_absolute() {
        return None;
    }
    let _effective = lines.next()?;
    let mut reported = BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once('=')?;
        reported.insert(key.trim().to_owned(), value.trim().to_owned());
    }
    let scope = |name: &str| {
        reported
            .get(name)
            .map(|value| ExecutionPolicy::parse(value))
    };
    // The edition's `SecurityZone`, by name; anything else — `NoZone`, `Unknown`, an answer
    // this build has no name for — is no answer.
    let edition_says = match reported.get("Zone")?.as_str() {
        "MyComputer" | "Intranet" | "Trusted" => Some(true),
        "Internet" | "Untrusted" => Some(false),
        _ => None,
    };
    let fact = |name: &str| {
        reported
            .get(name)
            .map(|value| value.eq_ignore_ascii_case("True"))
    };
    let location = ProfileLocation {
        local_fixed: fact("Local")?,
        marked: fact("Marked")?,
    };
    Some(ProfileObservation {
        scopes: PolicyScopes {
            machine_policy: scope("MachinePolicy")?,
            user_policy: scope("UserPolicy")?,
            process: scope("Process")?,
            current_user: scope("CurrentUser")?,
            local_machine: scope("LocalMachine")?,
            default: scope("NoProcess")?,
        },
        path,
        edition_says,
        location,
        ordinary_process: None,
        line_present: false,
    })
}

/// **The Process scope every ordinary session of the account starts with**: the
/// `PSExecutionPolicyPreference` named in `fresh` (the account's fresh logon block), `Undefined`
/// when the block names none, and `None` when there is no block to read. The name is matched
/// without case, as Windows matches environment names. Windows only, like the probe that reads it.
#[cfg(windows)]
fn ordinary_process_scope(
    fresh: Option<&[(OsString, OsString)]>,
) -> Option<crate::psreadline::ExecutionPolicy> {
    let fresh = fresh?;
    Some(
        fresh
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PSExecutionPolicyPreference"))
            .map(|(_, value)| value.to_string_lossy())
            .filter(|value| !value.is_empty())
            .map_or(crate::psreadline::ExecutionPolicy::Undefined, |value| {
                process_scope_value(&value)
            }),
    )
}

/// Whether this profile's text already dot-sources the script.
///
/// **A string criterion, and deliberately a loose one**: it recognises the line
/// this product writes *and* the line a reader wrote themselves, pointing at a
/// checkout, a copy, or anywhere else — because what the offer must not do is
/// appear in front of somebody who has already installed the integration their
/// own way. The file name is the whole of the evidence; the path in front of it
/// is theirs.
///
/// **A commented line is not an installation.** The script's own header carries
/// a worked example of the line behind a `#`, so a reader who pasted the header
/// into their profile would otherwise read as installed while their shell went
/// on emitting nothing — silence about the one machine that needs the offer.
///
/// Offer evidence only; editing and recording must use `profile_marks::Forms::owns`.
#[must_use]
pub fn profile_suppresses_integration_offer(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#') && line.to_ascii_lowercase().contains(SCRIPT_FILE_PS1)
    })
}

/// Default managed form for injected profile fixtures. Production resolves the
/// account's data root before selecting either admissible spelling.
#[cfg(test)]
#[must_use]
pub fn integration_line() -> String {
    profile_marks::MANAGED_LINE.to_owned()
}

/// **The `folio.ps1` a PowerShell birth names** — called on a birth worker or the
/// preparation worker, never on the window thread.
///
/// The rule, by whether this process's durable writes are held back (`update_trial`, F-7):
///
/// - **Not a trial, or a committed one**: the durable copy under Folio's data folder, compared
///   with the bytes this build carries and rewritten when it differs ([`install_script_at`]).
/// - **A trial not yet committed**: a read, never a write, of the durable copy — named when it
///   already holds this build's bytes. When it is stale or missing, the trial writes this build's
///   bytes into its own folder under the system temporary directory (one per transaction,
///   [`trial_script_directory`]) and names that, leaving the durable copy exactly as the old build
///   left it. The commit releases [`update_trial::Writer::PowerShellScript`], which repairs the
///   durable copy; births after it name the durable copy.
///
/// [`update_trial::Writer::PowerShellScript`]: crate::update_trial::Writer::PowerShellScript
pub fn powershell_script_for_birth() -> Option<PathBuf> {
    let durable = persist::storage_dir().join(SCRIPT_DIRECTORY);
    if crate::update_trial::defer(crate::update_trial::Writer::PowerShellScript) {
        if let Some(path) = TRIAL_POWERSHELL_SCRIPT.get().filter(|path| path.is_file()) {
            return Some(path.clone());
        }
        let prepared =
            powershell_script_in_trial(&durable, &trial_script_directory()?, SCRIPT_PS1)?;
        if !prepared.is_file() {
            return None;
        }
        let _ = TRIAL_POWERSHELL_SCRIPT.set(prepared.clone());
        return Some(prepared);
    }
    if let Some(path) = DURABLE_POWERSHELL_SCRIPT
        .get()
        .filter(|path| path.is_file())
    {
        return Some(path.clone());
    }
    let prepared = install_script_at(&durable, SCRIPT_FILE_PS1, SCRIPT_PS1)?;
    if !prepared.is_file() {
        return None;
    }
    let _ = DURABLE_POWERSHELL_SCRIPT.set(prepared.clone());
    Some(prepared)
}

/// A trial's own folder for the script: under the system temporary directory, which holds none
/// of the old build's data, named by the transaction so two trials never share one. A start that
/// continues with its writes held (`update_startup::is_held`) uses its transaction's the same way.
fn trial_script_directory() -> Option<PathBuf> {
    let txn = crate::update_startup::trial()
        .map(|(txn, _)| txn)
        .or_else(crate::update_startup::held)?;
    Some(
        std::env::temp_dir()
            .join(format!("folio-trial-{txn}"))
            .join(SCRIPT_DIRECTORY),
    )
}

/// Remove the script copy owned by a transaction after that transaction's
/// own directory has been retired. The target is reconstructed from the same
/// fixed prefix as [`trial_script_directory`] and checked before the recursive
/// removal; no durable Folio or user directory is beneath this path.
pub(crate) fn remove_trial_script(txn: crate::update_txn::TxnId) {
    let temporary = std::env::temp_dir();
    let root = temporary.join(format!("folio-trial-{txn}"));
    if root.parent() == Some(temporary.as_path())
        && root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("folio-trial-"))
    {
        let _ = std::fs::remove_dir_all(root.join(SCRIPT_DIRECTORY));
        let _ = std::fs::remove_dir(root);
    }
}

/// The trial half of [`powershell_script_for_birth`]'s rule, over directories a test can name:
/// the durable copy when it already holds `text` (read only), otherwise `text` written into
/// `trial_directory`. Never writes into `durable_directory`.
fn powershell_script_in_trial(
    durable_directory: &Path,
    trial_directory: &Path,
    text: &str,
) -> Option<PathBuf> {
    let durable = durable_directory.join(SCRIPT_FILE_PS1);
    if bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Settings, &durable)
        .is_ok_and(|existing| existing == text)
    {
        return Some(durable);
    }
    install_script_at(trial_directory, SCRIPT_FILE_PS1, text)
}

/// **Prepare the script ahead of the first PowerShell birth, on a worker nobody waits for.**
///
/// Started at launch and again when an update's trial is committed (the release of
/// [`update_trial::Writer::PowerShellScript`], which is then the durable write). A birth that
/// arrives first prepares it itself on its own worker; the two meet in a `OnceLock`, so the file
/// is compared once.
///
/// [`update_trial::Writer::PowerShellScript`]: crate::update_trial::Writer::PowerShellScript
enum PowershellPreparation {
    ScriptAndQuestions(Vec<ParseQuestion>),
    ParseAttempt(ParseAttempt),
}

/// **What the preparation worker does to the machine**: prepare `folio.ps1`, and put a parse
/// question to a PowerShell. Everything else on the worker — claiming the attempt, publishing the
/// answer, the thread door itself — is the same whoever does those two.
#[derive(Clone, Copy)]
struct PreparationEffects {
    script: fn() -> Option<PathBuf>,
    parse: ParseProbe,
}

impl PreparationEffects {
    /// The product's: the durable script under the data folder, and a real PowerShell.
    const MACHINE: Self = Self {
        script: powershell_script_for_birth,
        parse: run_parse_probe,
    };
}

fn spawn_powershell_preparation(
    work: PowershellPreparation,
    effects: PreparationEffects,
) -> Result<(), String> {
    bt_platform::spawn_at_priority(
        "powershell-script-prepare",
        bt_platform::ThreadPriority::BelowNormal,
        move |_ctx| match work {
            PowershellPreparation::ScriptAndQuestions(questions) => {
                let _ = (effects.script)();
                for question in questions {
                    ask_parse_question(question, ParseAsker::Background, effects.parse);
                }
            }
            PowershellPreparation::ParseAttempt(attempt) => {
                run_parse_attempt(attempt, effects.parse);
            }
        },
    )
    .map(drop)
    .map_err(|error| error.to_string())
}

fn begin_powershell_preparation(questions: Vec<ParseQuestion>, effects: PreparationEffects) {
    let _ = spawn_powershell_preparation(
        PowershellPreparation::ScriptAndQuestions(questions),
        effects,
    );
}

/// Prepare the script and ask the target parser about every command-bearing
/// PowerShell row in this profile snapshot. Called at startup and after a table
/// change; exact argv keys make unchanged rows free and changed rows new work.
pub fn begin_powershell_preparation_for(programs: &profiles::ProfilePrograms) {
    begin_powershell_preparation_over(
        profiles::table().profiles(),
        programs,
        PreparationEffects::MACHINE,
    );
}

/// [`begin_powershell_preparation_for`] over rows and effects handed in: the questions are
/// formed here, on the caller's thread, and asked on the preparation worker, which nobody joins.
fn begin_powershell_preparation_over(
    rows: &[profiles::Profile],
    programs: &profiles::ProfilePrograms,
    effects: PreparationEffects,
) {
    begin_powershell_preparation(parse_questions(rows, programs), effects);
}

/// Re-prepare only the script when a trial releases its durable write.
pub fn begin_powershell_script_preparation() {
    begin_powershell_preparation(Vec::new(), PreparationEffects::MACHINE);
}

/// What one write into a profile did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileWrite {
    /// The file that now carries the line.
    pub profile: PathBuf,
    /// The copy taken first, or `None` when there was no file to copy.
    pub backup: Option<PathBuf>,
}

/// Low-level injected-path writer. Production uses install_recorded, which
/// records intent before the profile write. Existing marks are idempotent.
#[cfg(test)]
pub fn add_to_profile(
    profile: &Path,
    line: &str,
    at: std::time::SystemTime,
) -> std::io::Result<ProfileWrite> {
    add_profile_with_forms(
        profile,
        line,
        &profile_marks::Forms::new(&[]),
        Some(&free_backup_path(profile, at)),
    )
}

/// `backup` is where the file as it stood is copied first, when there is a file; `None` takes no
/// copy. Whether a write takes one is the caller's decision (`profile_marks::ProfileFiles`: one
/// copy, before Folio's first write).
fn add_profile_with_forms(
    profile: &Path,
    line: &str,
    forms: &profile_marks::Forms,
    backup: Option<&Path>,
) -> std::io::Result<ProfileWrite> {
    let existing = read_profile_for_edit(profile)?;
    let original = existing.as_deref().unwrap_or_default();
    let decoded = profile_marks::Decoded::read(original)?;
    let changed = profile_marks::rewrite(original, forms, profile_marks::Action::Migrate)?;
    let bytes = if let Some(bytes) = changed {
        bytes
    } else if decoded.text.lines().any(|text| forms.owns(text)) {
        return Ok(ProfileWrite {
            profile: profile.to_path_buf(),
            backup: None,
        });
    } else {
        let newline = if decoded.text.contains("\r\n") {
            "\r\n"
        } else if decoded.text.contains('\n') {
            "\n"
        } else {
            "\r\n"
        };
        let mut text = decoded.text.clone();
        if !text.trim().is_empty() {
            if !text.ends_with('\n') {
                text.push_str(newline);
            }
            text.push_str(newline);
        } else if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(newline);
        }
        text.push_str(line);
        text.push_str(newline);
        decoded.encode(&text)
    };
    let backup = replace_profile(profile, original, &bytes, backup)?;
    Ok(ProfileWrite {
        profile: profile.to_path_buf(),
        backup,
    })
}

/// Refuse links/reparse points, including ancestors. PowerShell already
/// resolved Documents; the writer never follows a different target.
#[derive(Clone, Copy)]
enum ProfileAccess {
    Missing,
    WritableFile { links: u64 },
    ReadOnlyFile,
    Directory,
    Link,
}

pub(crate) fn refuse_profile_path(path: &Path) -> std::io::Result<()> {
    match profile_path_reason(path)? {
        Some(reason) => Err(std::io::Error::other(reason.text())),
        None => Ok(()),
    }
}

/// **The same predicate, with its answer rather than an error.**
///
/// Which of the three the filesystem said is a fact a caller may need: the agents' installers
/// resolve a link they can account for and say which refusal they are standing on, and a sentence
/// flattened to "this build cannot read it" was true of none of them (closure review R1). The
/// profile writer keeps the refusal it always had, by asking this and throwing the answer away.
pub(crate) fn profile_path_reason(path: &Path) -> std::io::Result<Option<crate::i18n::Text>> {
    profile_path_reason_with(path, |component| {
        let metadata = match std::fs::symlink_metadata(component) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProfileAccess::Missing);
            }
            Err(e) => return Err(e),
        };
        let linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            linked || metadata.file_attributes() & 0x400 != 0
        };
        Ok(if linked {
            ProfileAccess::Link
        } else if !metadata.is_file() {
            ProfileAccess::Directory
        } else if metadata.permissions().readonly() {
            ProfileAccess::ReadOnlyFile
        } else {
            ProfileAccess::WritableFile {
                links: bt_platform::file_link_count(&std::fs::File::open(component)?)?,
            }
        })
    })
}

/// The injected-metadata seam, in the refusing shape the two fixtures below press.
///
/// Test-only since the agents' installers began asking for the answer rather than for an error:
/// production has one caller of the decision and it is [`profile_path_reason`].
#[cfg(test)]
fn refuse_profile_path_with(
    path: &Path,
    inspect: impl Fn(&Path) -> std::io::Result<ProfileAccess>,
) -> std::io::Result<()> {
    match profile_path_reason_with(path, inspect)? {
        Some(reason) => Err(std::io::Error::other(reason.text())),
        None => Ok(()),
    }
}

fn profile_path_reason_with(
    path: &Path,
    inspect: impl Fn(&Path) -> std::io::Result<ProfileAccess>,
) -> std::io::Result<Option<crate::i18n::Text>> {
    for component in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let reason = match inspect(component)? {
            ProfileAccess::WritableFile { links } if links > 1 => {
                Some(crate::i18n::Text::ShellProfileHardLink)
            }
            ProfileAccess::Link => Some(crate::i18n::Text::ShellProfileLink),
            ProfileAccess::Directory | ProfileAccess::ReadOnlyFile if component == path => {
                Some(crate::i18n::Text::ShellProfileReadOnly)
            }
            _ => None,
        };
        if reason.is_some() {
            return Ok(reason);
        }
    }
    Ok(None)
}

fn read_profile_for_edit(profile: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    refuse_profile_path(profile)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1 | 4); // FILE_SHARE_READ | FILE_SHARE_DELETE
    }
    let mut file = match options.open(profile) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    bt_platform::file_reads::Reader::new(
        &mut file,
        bt_platform::file_reads::Lane::Settings,
        Some(profile),
    )
    .read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Replace the profile's bytes, atomically, after checking it still holds `original`. A file
/// that exists is copied to `backup` first when one is named (never over an existing file); a
/// file that does not is created, with its folders. Answers the copy it wrote.
fn replace_profile(
    profile: &Path,
    original: &[u8],
    bytes: &[u8],
    backup: Option<&Path>,
) -> std::io::Result<Option<PathBuf>> {
    use std::io::Write;
    let existing = read_profile_for_edit(profile)?;
    if existing.as_deref().unwrap_or_default() != original {
        return Err(std::io::Error::other(
            crate::i18n::Text::ShellProfileChanged.text(),
        ));
    }
    let existed = existing.is_some();
    let backup = if let Some(before) = existing {
        if let Some(path) = backup {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(&before)?;
            file.sync_all()?;
        }
        backup.map(Path::to_path_buf)
    } else {
        if let Some(parent) = profile.parent() {
            std::fs::create_dir_all(parent)?;
        }
        None
    };
    refuse_profile_path(profile)?;
    if existed {
        bt_persist::atomic_replace_preserving(profile, bytes).map_err(std::io::Error::other)?;
    } else {
        bt_persist::atomic_write(profile, bytes).map_err(std::io::Error::other)?;
    }
    Ok(backup)
}

/// `<profile>.bak-<YYYYMMDD>`, beside the file it copies.
///
/// The day in UTC, which is the calendar this workspace already keeps
/// (`seed::format_iso8601_utc`): it has no time-zone source, and a backup whose
/// name disagreed with the timestamp beside it in Explorer by a few hours is a
/// smaller problem than one whose name was invented from a guess.
/// The first `<profile>.bak-<YYYYMMDD>` name that is free, counting up.
///
/// The day alone would collide on the second write of one day (review row
/// R4-4), and a collision there used to mean no copy at all. Counting up keeps
/// the first copy of the day where somebody looking for "before Folio touched
/// this" will find it, and still gives every later write one of its own.
///
/// The count is bounded: a hundred writes into one profile in one day is not a
/// reader, and the hundred-and-first is given the plain day name — which by then
/// exists, so create_new refuses the write rather than replacing a backup.
fn free_backup_path(profile: &Path, at: std::time::SystemTime) -> PathBuf {
    let base = backup_path(profile, at);
    if !base.exists() {
        return base;
    }
    for attempt in 1..100u32 {
        let mut name = base.file_name().unwrap_or_default().to_os_string();
        name.push(format!("-{attempt}"));
        let candidate = base.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

fn backup_path(profile: &Path, at: std::time::SystemTime) -> PathBuf {
    let seconds = match at.duration_since(std::time::UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_secs()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let (year, month, day) = crate::seed::civil_from_days(seconds.div_euclid(86_400));
    let mut name = profile.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".bak-{year:04}{month:02}{day:02}"));
    profile.with_file_name(name)
}

/// The script's own text, for the tests that check what ships.
#[cfg(test)]
pub(crate) const fn script_source() -> &'static str {
    SCRIPT
}

/// PowerShell's script, which this module never installs but does depend on for
/// one declaration — see [`hyperlink_declaration`].
///
/// Readable from outside for one more reason since: the script's parting OSC 0
/// carries the two PowerShell profiles' own titles, and the pin that keeps the
/// two files in step (`profiles::tests`) has to read the bytes that ship rather
/// than a copy of them.
#[cfg(test)]
pub(crate) const fn script_source_ps1() -> &'static str {
    SCRIPT_PS1
}

/// zsh's script, for the tests that check what the three names in `ZDOTDIR` hold.
#[cfg(test)]
pub(crate) const fn script_source_zsh() -> &'static str {
    SCRIPT_ZSH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{Origin, ProgramSource};
    use bt_source::{Index, Pattern, Search, View, needle};

    /// PIN (release read m2) — **concurrent askers share one answer, and a failure is not an
    /// answer**: eight askers of a profile path that is answered make one query; after a query
    /// that got no answer, the next asker asks again, and its answer is then kept.
    ///
    /// RED (mutation: `failure_kept` — `answer_once` keeps the first result, `None` included,
    /// as the `OnceLock` it replaced did: the second asker never asks).
    #[test]
    fn shell_integration_profile_path_askers_share_an_answer_and_ask_again_after_a_failure() {
        let answered = Some(PathBuf::from("D:/redirected Documents/资料/profile.ps1"));
        let slot = std::sync::Arc::new(Mutex::new(None));
        let queries = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let slot = slot.clone();
                let queries = queries.clone();
                let answered = answered.clone();
                scope.spawn(move || {
                    assert_eq!(
                        answer_once(&slot, || {
                            queries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            answered.clone()
                        }),
                        answered
                    );
                });
            }
        });
        assert_eq!(queries.load(std::sync::atomic::Ordering::SeqCst), 1);

        let slot = Mutex::new(None);
        assert_eq!(
            answer_once(&slot, || None),
            None,
            "the probe ran out of time"
        );
        let asked_again = std::cell::Cell::new(false);
        assert_eq!(
            answer_once(&slot, || {
                asked_again.set(true);
                answered.clone()
            }),
            answered
        );
        assert!(asked_again.get(), "a failure is asked again");
        assert_eq!(answer_once(&slot, || panic!("an answer is kept")), answered);
    }

    #[test]
    fn shell_integration_followup_literal_current_account_is_recognised() {
        let script = persist::storage_dir()
            .join(SCRIPT_DIRECTORY)
            .join(SCRIPT_FILE_PS1);
        assert!(
            profile_marks::Forms::new(std::slice::from_ref(&script))
                .owns(&format!(". \"{}\"", script.display()))
        );
    }

    #[test]
    fn shell_integration_followup_hardlink_metadata_refuses_without_edit() {
        let file = temp_dir("followup-hardlink").join("profile.ps1");
        let original = profile_marks::LEGACY_LINE.as_bytes();
        std::fs::write(&file, original).unwrap();
        for links in [2, 3, 100] {
            let result = refuse_profile_path_with(&file, |path| {
                Ok(if path == file {
                    ProfileAccess::WritableFile { links }
                } else {
                    ProfileAccess::Directory
                })
            })
            .and_then(|()| {
                add_to_profile(&file, profile_marks::MANAGED_LINE, std::time::UNIX_EPOCH)
            });
            assert!(result.is_err(), "hardlink count {links} must refuse");
            assert_eq!(std::fs::read(&file).unwrap(), original);
        }
        assert!(
            refuse_profile_path_with(&file, |_| Ok(ProfileAccess::WritableFile { links: 1 }))
                .is_ok()
        );
    }

    #[test]
    fn shell_integration_refuses_links_at_every_path_component_without_following_them() {
        let file = Path::new("sandbox").join("redirected").join("profile.ps1");
        for linked in file.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            let result = refuse_profile_path_with(&file, |path| {
                Ok(if path == linked {
                    ProfileAccess::Link
                } else if path == file {
                    ProfileAccess::WritableFile { links: 1 }
                } else {
                    ProfileAccess::Directory
                })
            });
            assert!(result.is_err(), "{}", linked.display());
        }
        assert!(
            refuse_profile_path_with(&file, |path| Ok(if path == file {
                ProfileAccess::ReadOnlyFile
            } else {
                ProfileAccess::Directory
            }))
            .is_err()
        );
        assert!(refuse_profile_path_with(&file, |_| Ok(ProfileAccess::Missing)).is_ok());
    }

    /// [`profile_marks::apply_recorded`] with no owned-edit record, which is what the two
    /// Windows-only profile tests below (a file locked by a share mode, a symlinked profile) hand
    /// their files to. Gated like its only readers.
    #[cfg(windows)]
    fn apply_unrecorded(
        paths: &[PathBuf],
        forms: &profile_marks::Forms,
        action: profile_marks::Action,
    ) -> profile_marks::Report {
        profile_marks::apply_recorded(
            paths,
            forms,
            action,
            &mut profile_marks::ProfileFiles::default(),
            |_| Ok(()),
        )
    }

    #[cfg(windows)]
    #[test]
    fn shell_integration_locked_profile_is_byte_identical_and_other_file_is_removed() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = temp_dir("locked-removal");
        let locked = root.join("locked.ps1");
        let other = root.join("other.ps1");
        for path in [&locked, &other] {
            std::fs::write(path, LINE).unwrap();
        }
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();
        let report = apply_unrecorded(
            &[locked.clone(), other.clone()],
            &profile_marks::Forms::new(&[]),
            profile_marks::Action::Remove,
        );
        assert_eq!(report.exit_code(), 1);
        assert!(report.text(true).contains("locked.ps1"));
        assert_eq!(std::fs::read(other).unwrap(), b"");
        drop(handle);
        assert_eq!(std::fs::read(locked).unwrap(), LINE.as_bytes());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires Windows symlink privilege; injected ancestor refusal runs without it"]
    fn shell_integration_symlink_profile_and_linked_parent_are_refused() {
        let root = temp_dir("link-removal");
        let target = root.join("real.ps1");
        let link = root.join("linked.ps1");
        std::fs::write(&target, LINE).unwrap();
        std::os::windows::fs::symlink_file(&target, &link)
            .expect("developer mode permits sandbox symlinks");
        let report = apply_unrecorded(
            std::slice::from_ref(&link),
            &profile_marks::Forms::new(&[]),
            profile_marks::Action::Remove,
        );
        assert_eq!(report.exit_code(), 1);
        assert_eq!(std::fs::read(&target).unwrap(), LINE.as_bytes());
        assert!(
            std::fs::symlink_metadata(link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let parent_link = root.join("parent-link");
        let parent = root.join("real-parent");
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("profile.ps1"), LINE).unwrap();
        std::os::windows::fs::symlink_dir(&parent, &parent_link).unwrap();
        assert!(read_profile_for_edit(&parent_link.join("profile.ps1")).is_err());
        assert_eq!(
            std::fs::read(parent.join("profile.ps1")).unwrap(),
            LINE.as_bytes()
        );
    }

    #[test]
    fn shell_integration_startup_and_removal_doors_are_above_window_work() {
        let source = include_str!("main.rs");
        let main = source.rsplit_once("fn main()").unwrap().1;
        let removed = main
            .find("shell_integration::remove_shell_integration(")
            .unwrap();
        for later in [
            "cli::parse(",
            "launch_wire::hand_over(",
            "diagnostics::enter_resident_run(",
            "EventLoop::<AppEvent>::with_user_event()",
        ] {
            assert!(removed < main.find(later).unwrap(), "{later}");
        }
        let prepare = source
            .find("shell_integration::begin_startup_migration();")
            .expect("startup prepares the PowerShell script");
        assert!(prepare < source.find("opening_window_attributes(").unwrap());
        assert!(source.contains("shell_integration::begin_removal();"));
        let runtime = include_str!("shell_integration/profile_runtime.rs");
        let worker = runtime
            .split_once("pub fn begin_startup_migration() {")
            .unwrap()
            .1
            .split_once("fn candidates")
            .unwrap()
            .0;
        assert!(worker.contains("\"powershell-profile-observation\""));
        assert!(
            !worker.contains(".join()"),
            "the window thread waits for no startup worker"
        );
        // The script is prepared before the window; the parser questions about the PowerShell
        // rows follow the program walk's answers (T-PROGRAMS-REFRESH), read before the panes.
        let warmed = source
            .find("shell_integration::begin_powershell_script_preparation();")
            .expect("launch starts the script's preparation");
        assert!(warmed < source.find("opening_window_attributes(").unwrap());
        let asked = source
            .find("shell_integration::begin_powershell_preparation_for(&profile_programs);")
            .expect("launch asks about the PowerShell rows the walk answered");
        assert!(
            asked
                < source
                    .find("let (tab, conpty_source) = create_tab_state(")
                    .unwrap()
        );
        let probe = source_for_profile_probe();
        let command = source_for_probe_command();
        assert!(command.contains("quiet_command_named"));
        assert!(command.contains("quiet_command(program)"));
        assert!(probe.contains("-NoProfile"));
        assert!(include_str!("shell_integration.rs").contains("const POWERSHELL_PROBE_DEADLINE:"));
        assert!(REMOVAL_PROBE_DEADLINE > POWERSHELL_PROBE_DEADLINE);
        let parse_probe = include_str!("shell_integration.rs")
            .split_once("fn run_parse_probe(program: &Path, text: &str)")
            .expect("the Windows target parser probe")
            .1
            .split_once("fn profile_key")
            .expect("the item after that probe")
            .0;
        assert!(parse_probe.contains("POWERSHELL_PROBE_DEADLINE"));
        assert!(probe.contains("ProbeStdio::FED"));
        assert!(probe.contains("input.as_bytes()"));
        assert!(!probe.contains(".arg(input)"));
    }

    /// PIN — **the window hears a removal only through the report's own answer,
    /// and the console door keeps its sentence.**
    ///
    /// Two doors read the same `Report` and they owe different things. The
    /// window is not owed a toast about a `$PROFILE` line that was never there.
    /// Somebody who typed
    /// `--remove-shell-integration` *is* owed an answer, so that door still
    /// prints `ShellProfileNothing`.
    ///
    /// MUTATIONS:
    /// ① put the empty-text substitution back on the window's door and the
    ///    corner speaks about nothing again;
    /// ② take it off the console door and a command answers with silence and an
    ///    exit code.
    #[test]
    fn shell_integration_removal_speaks_to_a_window_only_through_the_report() {
        // **P3's deletion commit for this pin** (`docs/plans/bt-app-split-prep.md`
        // §6.3, and §6.0 rule 3). The commit before this one cut each of the two
        // doors twice — once out of `include_str!("main.rs")`, once out of the
        // body of the item that owns it — and asserted the two cuts were the
        // same bytes; this one removes the older of the two, because two
        // implementations of one judgement do not vouch for each other
        // (`docs/CONVENTIONS.md` §十 rule 4). The pattern is
        // `main.rs::pty_drain_budget_tests`', not re-derived here.
        //
        // The cuts themselves are unchanged; what changes is where they are
        // taken. Both doors are arms of one item apiece — the window's
        // `user_event` and the program's `main` — so the item is named and the
        // arm is cut inside its body, which is the same door wherever either
        // one comes to be written.
        let index = bt_source::Index::of_package("bt-app");
        let item_body = |query: bt_source::ItemQuery| {
            index
                .body_of(&query)
                .unwrap_or_else(|failure| panic!("{failure}"))
        };
        let probed = item_body(
            bt_source::ItemQuery::method("FolioApp", "user_event").of_trait("ApplicationHandler"),
        )
        .split_once("AppEvent::PowerShellProfileProbed => {")
        .expect("the window's user-event road carries the probe's arm")
        .1
        .split_once("AppEvent::UpdateChecked")
        .expect("and the arm after it")
        .0;
        assert!(probed.contains("report.window_text()"));
        assert!(
            !probed.contains("ShellProfileNothing"),
            "the window is being told about a removal that removed nothing"
        );
        let console = item_body(bt_source::ItemQuery::function("main"))
            .split_once("if cli::remove_shell_integration(std::env::args_os().skip(1)) {")
            .expect("the program's own entry carries the removal door")
            .1
            .split_once("if cli::remove_explorer_menu(")
            .expect("and the door after it")
            .0;
        assert!(
            console.contains("ShellProfileNothing"),
            "a person who typed the command is owed an answer"
        );
    }

    fn source_for_profile_probe() -> &'static str {
        include_str!("shell_integration.rs")
            .split_once("fn run_powershell_probe(")
            .unwrap()
            .1
            .split_once("/// **Where this PowerShell keeps `$PROFILE`, and nothing else**")
            .unwrap()
            .0
    }

    fn source_for_probe_command() -> &'static str {
        include_str!("shell_integration.rs")
            .split_once("fn powershell_probe_command(")
            .unwrap()
            .1
            .split_once("fn stopped_output")
            .unwrap()
            .0
    }

    #[test]
    fn shell_integration_managed_line_is_guarded_and_user_code_is_not_ours() {
        assert!(!profile_marks::Forms::new(&[]).owns(". D:\\tools\\folio.ps1"));
        assert!(!profile_marks::Forms::new(&[]).owns("# my note about folio.ps1"));
        let line = integration_line();
        assert!(line.starts_with("if (Test-Path -LiteralPath "));
        assert!(line.ends_with("# Folio shell integration v1"));
    }

    /// One shipped row, whole — what the spawn path is handed.
    fn row(id: &str) -> Profile {
        profiles::row_of(id).expect("a shipped id")
    }

    /// That row with an environment of its own.
    fn row_with(id: &str, env: &[(&str, &str)]) -> Profile {
        Profile {
            env: env
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            ..row(id)
        }
    }

    /// PIN — **a profile row whose name an environment block cannot spell sets
    /// nothing** (review row R2-22).
    ///
    /// A block ends a name at its first `=`, so a row named `A=B` does not make
    /// a variable of that name: it writes `A=B=...`, which the child reads as
    /// `A` set to `B=...` — the row quietly overwriting a variable it does not
    /// mention. A name with a NUL in it ends the entry there and takes every
    /// entry after it away from the child.
    ///
    /// MUTATION: layer the row anyway and the first assertion below is `PATH`
    /// belonging to somebody who typed `PATH=C:\x` into the name box.
    #[test]
    fn a_profile_row_naming_something_a_block_cannot_carry_sets_nothing() {
        assert!(a_child_can_be_given("PATH"));
        assert!(!a_child_can_be_given(""));
        assert!(!a_child_can_be_given("A=B"));
        assert!(!a_child_can_be_given("=C:"));
        assert!(!a_child_can_be_given("A\0B"));

        let mut environment = vec![(OsString::from("PATH"), OsString::from("C:\\real"))];
        layer_profile_environment(
            &mut environment,
            &[
                ("PATH=C:\\hijacked".to_owned(), "anything".to_owned()),
                ("KEPT".to_owned(), "value".to_owned()),
            ],
        );
        assert_eq!(
            environment,
            vec![
                (OsString::from("PATH"), OsString::from("C:\\real")),
                (OsString::from("KEPT"), OsString::from("value")),
            ],
            "the row that cannot be spelled changed nothing, and the one beside it did"
        );
    }

    fn args(command: &ShellCommand) -> Vec<String> {
        command
            .arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    /// An environment holding exactly the variables a case is about.
    struct Env(Vec<(&'static str, &'static str)>);

    impl ShellEnvironment for Env {
        fn var_os(&self, key: &str) -> Option<OsString> {
            self.0
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsString::from(*value))
        }

        fn is_file(&self, _path: &Path) -> bool {
            false
        }
    }

    fn bare() -> Env {
        Env(Vec::new())
    }

    #[test]
    fn powershell_spawn_appends_the_exact_loader_suffix_and_escapes_every_apostrophe() {
        let script =
            Path::new("C:/Folio/用户 目录/a'b\u{2018}c\u{2019}d\u{201a}e\u{201b}f/folio.ps1");
        let row_words = [OsString::from("-NoProfile")];
        let actual =
            compose_powershell_arguments(Path::new("pwsh.exe"), &row_words, Some(script), true);
        assert_eq!(&actual[..1], row_words.as_slice());
        assert_eq!(actual[1], "-NoExit");
        assert_eq!(actual[2], "-Command");
        let load = actual[3].to_string_lossy();
        for doubled in ["a''b", "b‘ ‘c", "c’ ’d", "d‚ ‚e", "e‛ ‛f"] {
            let expected = doubled.replace(' ', "");
            assert!(
                load.contains(&expected),
                "{expected:?} was not doubled in {load:?}"
            );
        }
        assert!(load.contains("用户 目录"), "{load}");
        assert!(load.contains("[IO.File]::ReadAllText("), "{load}");
        assert!(load.contains("[scriptblock]::Create"), "{load}");
        assert!(!actual.iter().skip(1).any(|word| word == "-NoLogo"));
    }

    fn os_words(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// RED (mutation: shorten any `minimum`, remove an alias row, or share the
    /// pwsh table with Windows PowerShell) — each edition's one table is the
    /// parser's ordered `MatchSwitch` contract, including its non-generic
    /// prefix exceptions.
    #[test]
    fn powershell_classifier_enumerates_each_editions_names_aliases_and_prefixes() {
        for (program, table) in [
            (Path::new("powershell.exe"), WINPS_OPTIONS),
            (Path::new("pwsh.exe"), PWSH_OPTIONS),
        ] {
            for option in table {
                let spelling = format!("-{}", option.name);
                let mut words = vec![OsString::from(spelling)];
                if option.kind == PowerShellOptionKind::Value {
                    words.push(OsString::from("value"));
                } else if matches!(
                    option.kind,
                    PowerShellOptionKind::Command
                        | PowerShellOptionKind::EncodedCommand
                        | PowerShellOptionKind::File
                        | PowerShellOptionKind::CommandWithArgs
                ) {
                    words.push(OsString::from(
                        if option.kind == PowerShellOptionKind::EncodedCommand {
                            encode_command("Get-Date")
                        } else {
                            "Get-Date".to_owned()
                        },
                    ));
                }
                let parsed = classify_powershell_arguments(program, &words);
                if option.kind == PowerShellOptionKind::Unsupported {
                    assert_eq!(parsed, None, "{program:?} {}", option.name);
                } else {
                    assert!(parsed.is_some(), "{program:?} {}", option.name);
                }

                if option.minimum != option.name {
                    let mut minimum = words.clone();
                    minimum[0] = OsString::from(format!("/{}", option.minimum));
                    let parsed = classify_powershell_arguments(program, &minimum);
                    if option.kind == PowerShellOptionKind::Unsupported {
                        assert_eq!(parsed, None, "{program:?} {}", option.minimum);
                    } else {
                        assert!(parsed.is_some(), "{program:?} {}", option.minimum);
                    }
                }
            }
        }

        for (program, words) in [
            ("powershell.exe", vec!["-n"]),
            ("powershell.exe", vec!["-en"]),
            ("pwsh.exe", vec!["-n"]),
            ("pwsh.exe", vec!["-set"]),
            ("pwsh.exe", vec!["-NoProfileL"]),
        ] {
            assert_eq!(
                classify_powershell_arguments(Path::new(program), &os_words(&words)),
                None,
                "ambiguous or below-minimum spelling {program} {words:?}"
            );
        }
        assert!(
            classify_powershell_arguments(Path::new("powershell.exe"), &os_words(&["-NoP"]))
                .is_some()
        );
        assert!(
            classify_powershell_arguments(
                Path::new("pwsh.exe"),
                &os_words(&["-NoP", "-NoProfileLoadTime", "-i"])
            )
            .is_some()
        );

        for (program, words, terminal, non_interactive) in [
            ("powershell.exe", vec!["-nol"], "none", false),
            ("powershell.exe", vec!["-ep", "Bypass"], "none", false),
            ("powershell.exe", vec!["-f", "script.ps1"], "file", false),
            (
                "powershell.exe",
                vec!["-noni", "-c", "Get-Date"],
                "command",
                true,
            ),
            ("pwsh.exe", vec!["-wd", "C:/"], "none", false),
            ("pwsh.exe", vec!["-cwa", "Get-Date"], "cwa", false),
        ] {
            let parsed = classify_powershell_arguments(Path::new(program), &os_words(&words))
                .unwrap_or_else(|| panic!("measured spelling {program} {words:?}"));
            assert_eq!(parsed.non_interactive, non_interactive);
            assert_eq!(
                match parsed.terminal {
                    PowerShellTerminal::None => "none",
                    PowerShellTerminal::Command { .. } => "command",
                    PowerShellTerminal::File => "file",
                    PowerShellTerminal::CommandWithArgs => "cwa",
                    PowerShellTerminal::EncodedCommand { .. } => "encoded",
                    PowerShellTerminal::Stdin => "stdin",
                },
                terminal,
                "{program} {words:?}"
            );
        }
    }

    /// RED (mutation: map `-s` to STA) — 5.1 measured it as server mode.
    #[test]
    fn inject4_winps_s_is_not_sta() {
        let winps = Path::new("powershell.exe");
        assert_eq!(
            classify_powershell_arguments(winps, &os_words(&["-s"])),
            None
        );
        assert!(classify_powershell_arguments(winps, &os_words(&["-sta"])).is_some());
    }

    /// RED (mutation: accept 5.1 `-Version`) — it selects another engine.
    #[test]
    fn inject4_winps_version_is_not_composable() {
        let winps = Path::new("powershell.exe");
        assert_eq!(
            classify_powershell_arguments(winps, &os_words(&["-Version", "2"])),
            None
        );
    }

    /// RED (mutation: allow a late `-PSConsoleFile`) — 5.1 binds it only at argv zero.
    #[test]
    fn inject4_winps_psconsolefile_is_position_zero_only() {
        let winps = Path::new("powershell.exe");
        assert!(
            classify_powershell_arguments(
                winps,
                &os_words(&["-PSConsoleFile", "legacy.psc1", "-NoLogo"])
            )
            .is_some()
        );
        assert_eq!(
            classify_powershell_arguments(
                winps,
                &os_words(&["-NoLogo", "-PSConsoleFile", "legacy.psc1"])
            ),
            None
        );
    }

    /// RED (mutation: require the encoded payload to be last) — both hosts
    /// continue far enough to accept the trailing NoExit.
    #[test]
    fn inject4_encoded_command_accepts_trailing_noexit() {
        let encoded = encode_command("Get-Date");
        for program in [Path::new("powershell.exe"), Path::new("pwsh.exe")] {
            let words = ["-EncodedCommand", encoded.as_str(), "-NoExit"];
            let parsed = classify_powershell_arguments(program, &os_words(&words))
                .expect("the hosts continue option parsing after the encoded payload");
            assert!(parsed.no_exit);
            assert!(matches!(
                parsed.terminal,
                PowerShellTerminal::EncodedCommand { text, .. } if text == "Get-Date"
            ));
        }
    }

    /// RED (mutations: offer beside a composable row, ignore `-NoProfile`,
    /// ignore a blocking policy, or key the installed fact by row) — the
    /// Profiles page's fallback is one edition fact applied to every row.
    #[test]
    fn inject4_powershell_profile_fallback_row_model_covers_every_state() {
        use PolicyCause::{Allows, Changeable, Location, Organisation, Process, Unreadable};
        use PowerShellProfileFallback::{
            Enabled, NoProfile, NotNeeded, Offer, Pending, PolicyChangeable, PolicyLocation,
            PolicyManaged, PolicyProcess,
        };
        let decide =
            |composable, pending: bool, no_profile, observed: Option<(PolicyCause, bool)>| {
                profile_fallback_from_parts(
                    true,
                    true,
                    composable,
                    no_profile,
                    if pending {
                        Heard::NotYet
                    } else {
                        Heard::Answer(())
                    },
                    observed.map_or(Heard::NotYet, Heard::Answer),
                )
            };
        assert_eq!(decide(true, false, false, None), NotNeeded);
        assert_eq!(decide(false, false, false, Some((Allows, false))), Offer);
        assert_eq!(decide(false, true, false, None), Pending);
        assert_eq!(decide(false, false, true, None), NoProfile);
        for (cause, state) in [
            (Changeable, PolicyChangeable),
            (Organisation, PolicyManaged),
            (Process, PolicyProcess),
            (Location, PolicyLocation),
            (Unreadable, PowerShellProfileFallback::Unreadable),
        ] {
            assert_eq!(decide(false, false, false, Some((cause, false))), state);
            assert_eq!(decide(false, false, false, Some((cause, true))), state);
        }
        assert_eq!(decide(false, false, false, Some((Allows, true))), Enabled);

        let two_rows = |present| {
            [
                decide(false, false, false, Some((Allows, present))),
                decide(false, false, false, Some((Allows, present))),
            ]
        };
        assert_eq!(two_rows(false), [Offer, Offer]);
        assert_eq!(two_rows(true), [Enabled, Enabled]);

        for (program, spelling) in [("powershell.exe", "-NoP"), ("pwsh.exe", "-nop")] {
            assert_eq!(
                powershell_profile_fallback(
                    Path::new(program),
                    &os_words(&[spelling, "-File", "enter.ps1"]),
                    true,
                ),
                NoProfile,
                "{program} must honour its accepted NoProfile prefix"
            );
        }
    }

    #[cfg(windows)]
    fn probe_answer(path: &str, current_user: &str, zone: &str, no_process: &str) -> String {
        format!(
            "\r\n{path}\r\nRemoteSigned\r\n\
             MachinePolicy=Undefined\r\nUserPolicy=Undefined\r\nProcess=Undefined\r\n\
             CurrentUser={current_user}\r\nLocalMachine=Undefined\r\n\
             Zone={zone}\r\nLocal=True\r\nMarked=False\r\nNoProcess={no_process}\r\n"
        )
    }

    /// PIN — **the profile probe's answer is the path the shell names, its effective policy,
    /// the five scopes, the profile's zone and the policy with no Process scope** (rounds 4–5).
    /// The default is the `NoProcess=` answer — PowerShell's own resolution, never a constant.
    /// The zone is the edition's own `SecurityZone` name, read as whether `RemoteSigned` loads
    /// from there; `NoZone` or anything unnamed is no answer, never a guess. A relative path, or
    /// an answer missing a scope, the zone or `NoProcess=`, is no answer.
    ///
    /// RED (mutations: drop the `is_absolute` check; take the default from a constant
    /// `Restricted`; read `NoZone` as this computer).
    #[cfg(windows)]
    #[test]
    fn the_profile_probe_answer_is_a_path_a_policy_and_its_scopes() {
        use crate::psreadline::ExecutionPolicy::{RemoteSigned, Undefined};
        let local = r"D:\Documents\PowerShell\Microsoft.PowerShell_profile.ps1";
        let share = r"\\files.example.com\home\me\Microsoft.PowerShell_profile.ps1";
        let observed = parse_profile_observation(&probe_answer(
            local,
            "Undefined",
            "MyComputer",
            "RemoteSigned",
        ))
        .expect("an answer");
        assert_eq!(observed.path, PathBuf::from(local));
        assert_eq!(observed.scopes.current_user, Undefined);
        assert_eq!(
            observed.scopes.default, RemoteSigned,
            "the probe's own resolution"
        );
        assert_eq!(observed.edition_says, Some(true));
        assert_eq!(
            observed.location,
            ProfileLocation {
                local_fixed: true,
                marked: false
            }
        );
        let facts = probe_answer(local, "Undefined", "NoZone", "Restricted")
            .replace("Local=True", "Local=False")
            .replace("Marked=False", "Marked=True");
        let facts = parse_profile_observation(&facts).expect("an answer");
        assert_eq!(
            facts.location,
            ProfileLocation {
                local_fixed: false,
                marked: true
            }
        );
        assert!(!observed.line_present, "presence is read from the file");
        for (zone, path, expected) in [
            ("Internet", local, Some(false)),
            ("Untrusted", share, Some(false)),
            ("Intranet", share, Some(true)),
            ("Trusted", share, Some(true)),
            ("MyComputer", share, Some(true)),
            ("NoZone", local, None),
            ("Unknown", share, None),
            ("3", local, None),
        ] {
            let parsed =
                parse_profile_observation(&probe_answer(path, "Undefined", zone, "Restricted"))
                    .expect("an answer");
            assert_eq!(parsed.edition_says, expected, "Zone={zone} for {path}");
        }
        assert!(
            parse_profile_observation("C:\\p\\profile.ps1\r\nRestricted\r\n").is_none(),
            "no scope list, no answer"
        );
        let without_zone = probe_answer(local, "Undefined", "MyComputer", "Restricted")
            .replace("Zone=MyComputer\r\n", "");
        assert!(parse_profile_observation(&without_zone).is_none());
        assert!(parse_profile_observation("profile.ps1\r\nBypass\r\n").is_none());
        assert!(parse_profile_observation("  \r\n").is_none());
    }

    fn scopes(
        machine_policy: crate::psreadline::ExecutionPolicy,
        user_policy: crate::psreadline::ExecutionPolicy,
        process: crate::psreadline::ExecutionPolicy,
        current_user: crate::psreadline::ExecutionPolicy,
        local_machine: crate::psreadline::ExecutionPolicy,
    ) -> PolicyScopes {
        PolicyScopes {
            machine_policy,
            user_policy,
            process,
            current_user,
            local_machine,
            default: crate::psreadline::ExecutionPolicy::Restricted,
        }
    }

    /// PIN — **who set a refusing policy, read in PowerShell's precedence order** (round 4,
    /// the owner's ruling of 2026-10-04). Group Policy is the organisation's; a process scope
    /// is the row's or the environment's; CurrentUser, LocalMachine or the default is the user's
    /// to change — and then, and only then, [`POLICY_COMMAND`] decides the effective policy.
    ///
    /// RED (mutations: read Process before Group Policy; treat a refusing LocalMachine as the
    /// organisation's; let a set CurrentUser fall through to LocalMachine).
    #[test]
    fn who_set_the_policy_is_read_in_precedence_order() {
        use crate::psreadline::ExecutionPolicy::{
            AllSigned, Bypass, RemoteSigned, Restricted, Undefined,
        };
        use PolicyCause::{Allows, Changeable, Organisation, Process};
        use RowProcessScope::{Absent, Set};
        let none = Undefined;
        let here = Some(true);
        for (name, list, row, cause) in [
            (
                "Group Policy refuses over a permissive CurrentUser",
                scopes(Restricted, none, none, RemoteSigned, none),
                Absent,
                Organisation,
            ),
            (
                "UserPolicy refuses",
                scopes(none, AllSigned, none, none, none),
                Absent,
                Organisation,
            ),
            (
                "Group Policy allows over the row's own refusal",
                scopes(RemoteSigned, none, none, none, none),
                Set(Restricted),
                Allows,
            ),
            (
                "the row's own -ExecutionPolicy Restricted",
                scopes(none, none, none, RemoteSigned, none),
                Set(Restricted),
                Process,
            ),
            (
                "a refusing process scope from the environment",
                scopes(none, none, AllSigned, none, none),
                Absent,
                Process,
            ),
            (
                "the row's own -ExecutionPolicy Bypass over a refusing CurrentUser",
                scopes(none, none, none, Restricted, none),
                Set(Bypass),
                Allows,
            ),
            (
                "CurrentUser Restricted, the rest Undefined",
                scopes(none, none, none, Restricted, none),
                Absent,
                Changeable,
            ),
            (
                "LocalMachine AllSigned, CurrentUser Undefined",
                scopes(none, none, none, none, AllSigned),
                Absent,
                Changeable,
            ),
            (
                "a set CurrentUser wins over LocalMachine",
                scopes(none, none, none, RemoteSigned, AllSigned),
                Absent,
                Allows,
            ),
            (
                "nothing set: the default decides",
                scopes(none, none, none, none, none),
                Absent,
                Changeable,
            ),
        ] {
            assert_eq!(policy_cause(list, here, row), cause, "{name}");
        }
        // And `Changeable` means the command works: the CurrentUser scope it writes, then read.
        let command_policy = crate::psreadline::ExecutionPolicy::parse(
            POLICY_COMMAND.rsplit(' ').next().expect("a policy word"),
        );
        assert_eq!(command_policy, POLICY_COMMAND_POLICY);
        assert!(POLICY_COMMAND.contains("-Scope CurrentUser"));
        for list in [
            scopes(none, none, none, Restricted, none),
            scopes(none, none, none, none, AllSigned),
            scopes(none, none, none, none, none),
        ] {
            assert_eq!(policy_cause(list, here, Absent), Changeable);
            let after = PolicyScopes {
                current_user: command_policy,
                ..list
            };
            assert_eq!(policy_cause(after, here, Absent), Allows, "{list:?}");
        }
    }

    /// PIN — **the default is PowerShell's own answer** (review of round 4, item 7). Nothing is set
    /// in any scope — the Process scope `Undefined`, which is an ordinary session with no
    /// `PSExecutionPolicyPreference` (a row's own `-ExecutionPolicy Undefined` is *not* this: both
    /// editions read it as `Restricted`, [`process_scope_value`]) — so the machine's default
    /// decides, and the probe's `NoProcess=` answer is what that default is, on a server
    /// (`RemoteSigned`) as on a client (`Restricted`).
    ///
    /// RED (mutation: `.unwrap_or(scopes.default)` → `.unwrap_or(Restricted)`).
    #[test]
    fn a_cleared_process_scope_falls_to_the_probes_own_default() {
        use crate::psreadline::ExecutionPolicy::{AllSigned, RemoteSigned, Restricted, Undefined};
        let none = Undefined;
        for (default, cause) in [
            (RemoteSigned, PolicyCause::Allows),
            (Restricted, PolicyCause::Changeable),
        ] {
            let list = PolicyScopes {
                default,
                ..scopes(none, none, AllSigned, none, none)
            };
            assert_eq!(
                policy_cause(list, Some(true), RowProcessScope::Set(Undefined)),
                cause,
                "default {default:?}"
            );
        }
    }

    /// PIN — **`RemoteSigned` and where `$PROFILE` lives, by the edition's own word** (review of
    /// round 4 item 6; review of round 5 (a), (d)). Whichever policy will decide — the one deciding
    /// now, or `RemoteSigned` once [`POLICY_COMMAND`] has run — when it is `RemoteSigned`, the row
    /// is promised only where the edition said its `RemoteSigned` loads from there. A PowerShell 7
    /// profile on a redirected share (the edition says yes) keeps Enable and Copy; a Windows
    /// PowerShell one on a share it maps to the Internet zone (the edition says no), or an edition
    /// that gave no answer, gets the Location sentence. `Bypass` and `Unrestricted` load from
    /// anywhere. And a line already installed where it loads is Enabled, not the Location sentence.
    ///
    /// RED (mutations: skip the zone check; judge the changeable case by the refusing policy
    /// instead of `POLICY_COMMAND_POLICY`; treat no answer as loadable).
    #[test]
    fn remote_signed_is_judged_against_the_profiles_zone() {
        use crate::psreadline::ExecutionPolicy::{Bypass, RemoteSigned, Restricted, Undefined};
        use PolicyCause::{Allows, Changeable, Location};
        let none = Undefined;
        let remote_signed = scopes(none, none, none, RemoteSigned, none);
        let refusing = scopes(none, none, none, Restricted, none);
        let bypassed = scopes(none, none, Bypass, none, none);
        for (zone, now, after_command, bypass) in [
            (Some(true), Allows, Changeable, Allows),
            (Some(false), Location, Location, Allows),
            (None, Location, Location, Allows),
        ] {
            let row = RowProcessScope::Absent;
            assert_eq!(policy_cause(remote_signed, zone, row), now, "{zone:?}");
            assert_eq!(policy_cause(refusing, zone, row), after_command, "{zone:?}");
            assert_eq!(policy_cause(bypassed, zone, row), bypass, "{zone:?}");
        }
        // (d): installed where the edition says it loads — Enabled.
        assert_eq!(
            profile_fallback_from_parts(
                true,
                true,
                false,
                false,
                Heard::Answer(()),
                Heard::Answer((
                    policy_cause(remote_signed, Some(true), RowProcessScope::Absent),
                    true
                )),
            ),
            PowerShellProfileFallback::Enabled
        );
    }

    /// PIN — **a private API is not a single point of failure for the ordinary case** (review of
    /// round 7, item 1). The edition's own zone answer wins in every combination. Without it, an
    /// unmarked profile on a local fixed drive keeps its road (Offer, or Copy where the policy is
    /// the user's), while a marked file or a network path cannot be known and gets the Location
    /// sentence.
    ///
    /// RED (mutations: the fallback answers `None` always — the local unmarked rows lose the
    /// road; it ignores the mark; it ignores the drive; it wins over the edition's answer).
    #[test]
    fn the_editions_answer_wins_and_the_fallback_covers_only_what_can_be_known() {
        use crate::psreadline::ExecutionPolicy::{RemoteSigned, Restricted, Undefined};
        let at = |local_fixed, marked| ProfileLocation {
            local_fixed,
            marked,
        };
        let every = [
            at(true, false),
            at(true, true),
            at(false, false),
            at(false, true),
        ];
        for answer in [Some(true), Some(false)] {
            for location in every {
                assert_eq!(
                    remote_signed_loads(answer, location),
                    answer,
                    "{answer:?} at {location:?}"
                );
            }
        }
        assert_eq!(remote_signed_loads(None, at(true, false)), Some(true));
        assert_eq!(remote_signed_loads(None, at(true, true)), None);
        assert_eq!(remote_signed_loads(None, at(false, false)), None);
        assert_eq!(remote_signed_loads(None, at(false, true)), None);

        // Through the row: no answer + local unmarked is the road it was before round 6.
        let none = Undefined;
        let permissive = scopes(none, none, none, RemoteSigned, none);
        let refusing = scopes(none, none, none, Restricted, none);
        let row = RowProcessScope::Absent;
        let cause = |list, location| policy_cause(list, remote_signed_loads(None, location), row);
        assert_eq!(cause(permissive, at(true, false)), PolicyCause::Allows);
        assert_eq!(cause(refusing, at(true, false)), PolicyCause::Changeable);
        assert_eq!(cause(permissive, at(true, true)), PolicyCause::Location);
        assert_eq!(cause(permissive, at(false, false)), PolicyCause::Location);
        assert_eq!(cause(refusing, at(false, false)), PolicyCause::Location);
    }

    /// PIN — **a missing zone answer is said in diagnostics once per edition**, naming the kind of
    /// path (review of round 7, item 1), so a future break of the internal zone function shows.
    ///
    /// RED (mutation: `FallbackNotices::first` answers every time).
    #[test]
    fn a_missing_zone_answer_is_said_once_per_edition() {
        let notices = FallbackNotices::default();
        let local = ProfileLocation {
            local_fixed: true,
            marked: false,
        };
        let first = notices
            .first(Path::new("powershell.exe"), local)
            .expect("the first unanswered observation is said");
        assert!(first.contains("did not answer whether RemoteSigned loads"));
        assert!(first.contains("a profile on a local fixed drive"));
        assert!(first.contains("using the fallback"));
        assert_eq!(notices.first(Path::new("powershell.exe"), local), None);
        assert!(
            notices.first(Path::new("pwsh.exe"), local).is_some(),
            "the other edition is said once too"
        );
    }

    /// PIN — **a run inside the `$PROFILE` sandbox asks no shell** (round 6; U-35's ordinary-start
    /// test went red on CI because a sandboxed child started real PowerShell probes).
    ///
    /// RED (mutation: `probe_profile_observation_unless` asks whatever `sandboxed` says).
    #[test]
    fn a_sandboxed_run_asks_no_shell_about_the_real_profile() {
        assert!(
            probe_profile_observation_unless(true, || panic!("a sandboxed run asked a shell"))
                .is_none()
        );
        let asked = std::cell::Cell::new(false);
        let _ = probe_profile_observation_unless(false, || {
            asked.set(true);
            None
        });
        assert!(asked.get(), "an ordinary run asks");
    }

    /// PIN — **a probe's PowerShell computes its own module path** (round 6): the door does not
    /// hand it the module path of whatever session started Folio.
    ///
    /// RED (mutation: drop the `env_remove("PSModulePath")` in `powershell_probe_command`).
    #[cfg(windows)]
    #[test]
    fn a_probe_powershell_computes_its_own_module_path() {
        let command = powershell_probe_command(Path::new("powershell.exe"))
            .expect("Windows PowerShell is on every supported Windows");
        assert!(
            command
                .get_envs()
                .any(|(key, value)| key.eq_ignore_ascii_case("PSModulePath") && value.is_none()),
            "the probe's module path is the edition's own"
        );
    }

    /// PIN — **each refusing cause is its own row state and its own sentence, and only the
    /// changeable one has a button** (rounds 4–5). The first sentence of each is the shipped
    /// "not provided" one; an unreadable row says that sentence alone.
    ///
    /// RED (mutations: map `Organisation` to `PolicyChangeable`; give `PolicyManaged` the Copy
    /// button; give `Unreadable` the Enable button).
    #[test]
    fn each_policy_cause_is_its_own_row_and_sentence() {
        use crate::i18n::Text;
        use crate::settings::ProfileButton;
        for (cause, state, sentence, button) in [
            (
                PolicyCause::Changeable,
                PowerShellProfileFallback::PolicyChangeable,
                Text::CapPowerShellPolicyChangeable,
                Some(ProfileButton::CopyPolicyCommand),
            ),
            (
                PolicyCause::Organisation,
                PowerShellProfileFallback::PolicyManaged,
                Text::CapPowerShellPolicyManaged,
                None,
            ),
            (
                PolicyCause::Process,
                PowerShellProfileFallback::PolicyProcess,
                Text::CapPowerShellPolicyProcess,
                None,
            ),
            (
                PolicyCause::Location,
                PowerShellProfileFallback::PolicyLocation,
                Text::CapPowerShellPolicyLocation,
                None,
            ),
            (
                PolicyCause::Unreadable,
                PowerShellProfileFallback::Unreadable,
                Text::CapPowerShellNotProvided,
                None,
            ),
        ] {
            assert_eq!(
                profile_fallback_from_parts(
                    true,
                    true,
                    false,
                    false,
                    Heard::Answer(()),
                    Heard::Answer((cause, false))
                ),
                state
            );
            assert_eq!(state.policy_cause(), Some(cause));
            assert_eq!(cause.sentence(), Some(sentence));
            assert_eq!(ProfileButton::of(state), button);
            assert!(
                sentence
                    .text()
                    .starts_with(Text::CapPowerShellNotProvided.text()),
                "{sentence:?} starts with the shipped sentence"
            );
        }
        assert_eq!(PolicyCause::Allows.sentence(), None);
        assert_eq!(
            ProfileButton::of(PowerShellProfileFallback::Offer),
            Some(ProfileButton::Enable)
        );
    }

    /// PIN — **a policy changed while Folio runs is read again, and the row follows** (round 4).
    /// What the page-entry edge starts (`begin_profile_observation_for`, pinned as one edge per
    /// visit by `inject4_profiles_page_observation_is_one_edge_per_visit`) publishes a new
    /// observation; the row is read from the latest one, so the next frame shows `Offer`.
    ///
    /// RED (mutation: `publish_profile_observation` keeps the first observation of an edition).
    #[test]
    fn a_policy_changed_since_the_last_visit_flips_the_row_to_offer() {
        use crate::psreadline::ExecutionPolicy::{RemoteSigned, Restricted, Undefined};
        // Windows PowerShell's edition, which no other test publishes for; restored after.
        let program = Path::new("powershell.exe");
        let row = os_words(&["-NoExit", "-File", "enter.ps1"]);
        let observed = |current_user| ProfileObservation {
            path: PathBuf::from("profile.ps1"),
            scopes: scopes(Undefined, Undefined, Undefined, current_user, Undefined),
            edition_says: Some(true),
            location: ProfileLocation {
                local_fixed: true,
                marked: false,
            },
            ordinary_process: Some(Undefined),
            line_present: false,
        };
        publish_profile_observation(program, observed(Restricted));
        let before = powershell_profile_fallback(program, &row, true);
        publish_profile_observation(program, observed(RemoteSigned));
        let after = powershell_profile_fallback(program, &row, true);
        PROFILE_OBSERVATIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&powershell_edition(program));
        if PROFILE_PROBE_EXISTS {
            assert_eq!(before, PowerShellProfileFallback::PolicyChangeable);
            assert_eq!(after, PowerShellProfileFallback::Offer);
        } else {
            assert_eq!(before, PowerShellProfileFallback::Unsupported);
            assert_eq!(after, PowerShellProfileFallback::Unsupported);
        }
    }

    /// PIN — **a row's own `-ExecutionPolicy` is read by the classifier, the last one wins, and
    /// a row the classifier cannot read makes no claim** (rounds 4–5). Any accepted spelling
    /// and case; a repeated option takes its last value, as both editions' binders do (pinned
    /// against them by `the_real_binders_read_the_execution_policy_as_the_classifier_does`); a
    /// colon form (`-ep:Bypass`) is not an option to either binder and not one to the
    /// classifier, so the row is `Unreadable`.
    ///
    /// RED (mutations: `row_process_scope` takes the first occurrence; answers `Absent` for an
    /// argv the classifier cannot read).
    #[test]
    fn a_rows_own_execution_policy_is_its_process_scope() {
        use crate::psreadline::ExecutionPolicy::{Bypass, Restricted};
        use RowProcessScope::{Absent, Set, Unreadable};
        for (program, words, expected) in [
            (
                "pwsh.exe",
                vec!["-ExecutionPolicy", "Restricted", "-File", "a.ps1"],
                Set(Restricted),
            ),
            (
                "powershell.exe",
                vec!["-ep", "bypass", "-NoExit", "-File", "a.ps1"],
                Set(Bypass),
            ),
            (
                "pwsh.exe",
                vec!["-ep", "Restricted", "-ep", "Bypass", "-File", "a.ps1"],
                Set(Bypass),
            ),
            (
                "powershell.exe",
                vec![
                    "-ep",
                    "Bypass",
                    "-ExecutionPolicy",
                    "Restricted",
                    "-File",
                    "a.ps1",
                ],
                Set(Restricted),
            ),
            ("pwsh.exe", vec!["-NoExit", "-File", "a.ps1"], Absent),
            ("pwsh.exe", vec!["-ep:Bypass", "-File", "a.ps1"], Unreadable),
            (
                "powershell.exe",
                vec!["-ExecutionPolicy:Bypass", "-File", "a.ps1"],
                Unreadable,
            ),
            (
                "pwsh.exe",
                vec!["-NoProfile:$false", "-File", "a.ps1"],
                Unreadable,
            ),
        ] {
            assert_eq!(
                row_process_scope(Path::new(program), &os_words(&words)),
                expected,
                "{program} {words:?}"
            );
        }
        // An unreadable row offers nothing, whatever the edition's policy.
        assert_eq!(
            policy_cause(
                scopes(
                    crate::psreadline::ExecutionPolicy::Undefined,
                    crate::psreadline::ExecutionPolicy::Undefined,
                    crate::psreadline::ExecutionPolicy::Undefined,
                    crate::psreadline::ExecutionPolicy::RemoteSigned,
                    crate::psreadline::ExecutionPolicy::Undefined,
                ),
                Some(true),
                Unreadable,
            ),
            PolicyCause::Unreadable
        );
    }

    /// PIN — **the classifier reads `-ExecutionPolicy` exactly as the real binders do** (review
    /// of round 4, items 4–5). On each installed edition, started through `bt_pty::test_shell`
    /// (no profile, no history, a one-shot `-Command`): a repeated option is bound to its last
    /// value, and a colon form is not bound at all — PowerShell 7 takes `-ep:Bypass` for a
    /// script file's name, Windows PowerShell for the command's own text; either way the process
    /// scope is not `Bypass`.
    ///
    /// RED (mutation: the classifier's `row_process_scope` takes the first occurrence — the
    /// classifier's answer then disagrees with the binder's).
    #[cfg(windows)]
    #[test]
    fn the_real_binders_read_the_execution_policy_as_the_classifier_does() {
        use bt_pty::test_shell::Hygiene;
        let query = "(Get-ExecutionPolicy -Scope Process).ToString()";
        for name in ["powershell.exe", "pwsh.exe"] {
            let Some(program) = bt_platform::program_on_path(Path::new(name)) else {
                eprintln!("{name}: not installed; real binder arm skipped");
                continue;
            };
            let bound = |words: &[&str]| -> String {
                let hygiene = Hygiene::new();
                let output = hygiene
                    .command(&program, bt_platform::quiet_command)
                    .args(["-NonInteractive"])
                    .args(words)
                    .args(["-Command", query])
                    .output()
                    .expect("the shell starts");
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            };
            for words in [
                ["-ep", "Restricted", "-ep", "Bypass"],
                ["-ep", "Bypass", "-ep", "Restricted"],
            ] {
                let mut row = os_words(&words);
                row.extend(os_words(&["-Command", query]));
                let RowProcessScope::Set(classified) = row_process_scope(&program, &row) else {
                    panic!("{name} {words:?}: the classifier reads a repeated -ep");
                };
                assert_eq!(bound(&words), classified.name(), "{name} {words:?}");
            }
            let colon = bound(&["-ep:Bypass"]);
            assert_ne!(
                colon, "Bypass",
                "{name}: -ep:Bypass is not bound as an option"
            );
            let mut row = os_words(&["-ep:Bypass"]);
            row.extend(os_words(&["-Command", query]));
            assert_eq!(
                row_process_scope(&program, &row),
                RowProcessScope::Unreadable,
                "{name}"
            );
        }
    }

    /// PIN (release read B1) — **the managed line is offered only where an ordinary session of
    /// the edition and the clicked row both load it**, and the row says the cause a user can act
    /// on otherwise. The full table: who set the ordinary session's policy (nothing refusing, the
    /// client default, CurrentUser, LocalMachine, MachinePolicy, UserPolicy, the account's
    /// environment, Group Policy's `RemoteSigned` against an Internet-zone profile, an
    /// environment that could not be read) × what the row's own arguments say (nothing,
    /// `-ExecutionPolicy Bypass`, `-ExecutionPolicy Restricted`, an unreadable `-ep:Bypass`,
    /// `-NoProfile`) × whether the line is already there. A line that is there is `Enabled` only
    /// where it would be offered; under a policy an ordinary session refuses, it is the policy's
    /// row (with Copy where the user can change it) — the old Folio's or the hand-written line is
    /// removed by the Settings "PowerShell $PROFILE line" row. And where the row is
    /// `PolicyChangeable`, the copied command makes it `Offer`: it fixes every session.
    ///
    /// RED (mutations: `row_only` — `edition_cause` answers the row's own cause: the Bypass row
    /// on a default client is offered, the blocker; `ordinary_only` — it answers the ordinary
    /// session's: the row's own `Restricted` is offered; `changeable_over_process` — `both_sessions`
    /// prefers the ordinary session's `Changeable` to the row's own `Process`: Copy offered on a
    /// row the command cannot fix; `probe_process_is_ordinary` — the ordinary session reads the
    /// probe's Process scope instead of the account's).
    #[test]
    fn the_line_is_offered_only_where_an_ordinary_session_and_the_row_both_load_it() {
        use crate::psreadline::ExecutionPolicy::{
            AllSigned, Bypass, RemoteSigned, Restricted, Undefined,
        };
        use PowerShellProfileFallback::{
            NoProfile, Offer, PolicyChangeable, PolicyLocation, PolicyManaged, PolicyProcess,
            Undetermined, Unreadable,
        };
        let none = Undefined;
        let observed = |scopes: PolicyScopes, ordinary, edition_says| ProfileObservation {
            path: PathBuf::from(r"C:\Users\me\Documents\WindowsPowerShell\profile.ps1"),
            scopes,
            edition_says,
            location: ProfileLocation {
                local_fixed: true,
                marked: false,
            },
            ordinary_process: ordinary,
            line_present: false,
        };
        // The probe's own Process scope is Folio's: Bypass here, which no ordinary session has.
        let with_probe_bypass = |scopes: PolicyScopes| PolicyScopes {
            process: Bypass,
            ..scopes
        };
        let here = Some(true);
        let sessions = [
            (
                "nothing refuses",
                observed(
                    scopes(none, none, none, RemoteSigned, none),
                    Some(none),
                    here,
                ),
            ),
            (
                "the client default",
                observed(scopes(none, none, none, none, none), Some(none), here),
            ),
            (
                "CurrentUser Restricted",
                observed(scopes(none, none, none, Restricted, none), Some(none), here),
            ),
            (
                "LocalMachine AllSigned",
                observed(scopes(none, none, none, none, AllSigned), Some(none), here),
            ),
            (
                "MachinePolicy Restricted",
                observed(
                    scopes(Restricted, none, none, RemoteSigned, none),
                    Some(none),
                    here,
                ),
            ),
            (
                "UserPolicy AllSigned",
                observed(
                    scopes(none, AllSigned, none, RemoteSigned, none),
                    Some(none),
                    here,
                ),
            ),
            (
                "the account's environment sets Restricted",
                observed(
                    with_probe_bypass(scopes(none, none, none, none, none)),
                    Some(Restricted),
                    here,
                ),
            ),
            (
                "Group Policy RemoteSigned, an Internet-zone profile",
                observed(
                    scopes(RemoteSigned, none, none, none, none),
                    Some(none),
                    Some(false),
                ),
            ),
            (
                "the account's environment could not be read",
                observed(scopes(none, none, none, RemoteSigned, none), None, here),
            ),
            (
                "the account's environment sets an invalid preference",
                observed(
                    scopes(none, none, none, RemoteSigned, none),
                    Some(process_scope_value("Garbage")),
                    here,
                ),
            ),
            (
                "a scope in a policy name this build does not know",
                observed(
                    scopes(
                        none,
                        none,
                        none,
                        crate::psreadline::ExecutionPolicy::Unknown,
                        none,
                    ),
                    Some(none),
                    here,
                ),
            ),
        ];
        let rows: [(&str, Option<RowProcessScope>); 6] = [
            ("no -ExecutionPolicy", Some(RowProcessScope::Absent)),
            (
                "-ExecutionPolicy Bypass",
                Some(RowProcessScope::Set(Bypass)),
            ),
            (
                "-ExecutionPolicy Restricted",
                Some(RowProcessScope::Set(Restricted)),
            ),
            ("-ep:Bypass", Some(RowProcessScope::Unreadable)),
            ("-NoProfile", None),
            (
                "-ExecutionPolicy Undefined",
                Some(RowProcessScope::Set(process_scope_value("Undefined"))),
            ),
        ];
        #[rustfmt::skip]
        let expected = [
            //                     nothing           Bypass            Restricted     -ep:Bypass  -NoProfile -ep Undefined
            /* nothing refuses */ [Offer,            Offer,            PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* client default  */ [PolicyChangeable, PolicyChangeable, PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* CurrentUser     */ [PolicyChangeable, PolicyChangeable, PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* LocalMachine    */ [PolicyChangeable, PolicyChangeable, PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* MachinePolicy   */ [PolicyManaged,    PolicyManaged,    PolicyManaged, Unreadable, NoProfile, PolicyManaged],
            /* UserPolicy      */ [PolicyManaged,    PolicyManaged,    PolicyManaged, Unreadable, NoProfile, PolicyManaged],
            /* account's env   */ [PolicyProcess,    PolicyProcess,    PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* GP RemoteSigned */ [PolicyLocation,   PolicyLocation,   PolicyLocation, Unreadable, NoProfile, PolicyLocation],
            /* env unreadable  */ [Undetermined,     Undetermined,     Undetermined,  Unreadable, NoProfile, Undetermined],
            /* env garbage     */ [PolicyProcess,    PolicyProcess,    PolicyProcess, Unreadable, NoProfile, PolicyProcess],
            /* unknown name    */ [Undetermined,     Undetermined,     Undetermined,  Unreadable, NoProfile, Undetermined],
        ];
        let state = |session: &ProfileObservation, row: Option<RowProcessScope>, present| {
            profile_fallback_from_parts(
                true,
                true,
                false,
                row.is_none(),
                Heard::Answer(()),
                Heard::Answer((
                    edition_cause(session, row.unwrap_or(RowProcessScope::Absent)),
                    present,
                )),
            )
        };
        for ((who, session), expected) in sessions.iter().zip(expected) {
            for ((what, row), expected) in rows.iter().zip(expected) {
                assert_eq!(state(session, *row, false), expected, "{who} × {what}");
                // The line already there: Enabled only where it would be offered.
                let present = if expected == Offer {
                    PowerShellProfileFallback::Enabled
                } else {
                    expected
                };
                assert_eq!(
                    state(session, *row, true),
                    present,
                    "{who} × {what}, line present"
                );
                // Copy's command fixes it for every session, this row included.
                if expected == PolicyChangeable {
                    let after = ProfileObservation {
                        scopes: PolicyScopes {
                            current_user: POLICY_COMMAND_POLICY,
                            ..session.scopes
                        },
                        ..session.clone()
                    };
                    assert_eq!(
                        state(&after, *row, false),
                        Offer,
                        "{who} × {what}, after Copy"
                    );
                }
            }
        }
        // `both_sessions` over every pair of causes it can be handed: one session that allows
        // takes the other's cause; Group Policy before the row's own Process scope before the
        // ordinary session's cause.
        use PolicyCause::{Allows, Changeable, Location, Organisation, Process};
        for own in [Allows, Changeable, Organisation, Process, Location] {
            assert_eq!(both_sessions(Allows, own), own);
            assert_eq!(both_sessions(own, Allows), own);
            assert_eq!(
                both_sessions(own, PolicyCause::Unreadable),
                PolicyCause::Unreadable
            );
            assert_eq!(
                both_sessions(PolicyCause::Undetermined, own),
                PolicyCause::Undetermined
            );
        }
        assert_eq!(both_sessions(Changeable, Process), Process);
        assert_eq!(both_sessions(Location, Process), Process);
        assert_eq!(both_sessions(Process, Changeable), Process);
        assert_eq!(both_sessions(Organisation, Process), Organisation);
        assert_eq!(both_sessions(Changeable, Location), Changeable);
        assert_eq!(both_sessions(Location, Changeable), Location);
    }

    /// PIN (release read B1) — **an ordinary session's Process scope is the account's fresh
    /// environment's `PSExecutionPolicyPreference`**, any case of its name, `Undefined` when the
    /// account sets none or sets it empty, and unknown when there is no block to read.
    ///
    /// RED (mutation: `case_sensitive_name` — the name compared exactly: a block that spells it
    /// `PSEXECUTIONPOLICYPREFERENCE` reads as unset).
    #[cfg(windows)]
    #[test]
    fn an_ordinary_sessions_process_scope_is_the_accounts_environment() {
        use crate::psreadline::ExecutionPolicy::{AllSigned, Bypass, Restricted, Undefined};
        let block = |pairs: &[(&str, &str)]| -> Vec<(OsString, OsString)> {
            pairs
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect()
        };
        assert_eq!(ordinary_process_scope(None), None);
        assert_eq!(
            ordinary_process_scope(Some(&block(&[("Path", r"C:\工具")]))),
            Some(Undefined)
        );
        assert_eq!(
            ordinary_process_scope(Some(&block(&[(
                "PSEXECUTIONPOLICYPREFERENCE",
                "allsigned"
            )]))),
            Some(AllSigned)
        );
        assert_eq!(
            ordinary_process_scope(Some(&block(&[("PSExecutionPolicyPreference", "Bypass")]))),
            Some(Bypass)
        );
        // PowerShell's own reading, measured on both editions: anything but one of the five names
        // exactly (any case) is `Restricted` — garbage, a padded name, `Undefined` itself.
        for value in [" ", "Garbage", " Bypass ", "Undefined", "无效"] {
            assert_eq!(
                ordinary_process_scope(Some(&block(&[("PSExecutionPolicyPreference", value)]))),
                Some(Restricted),
                "{value:?}"
            );
        }
        assert_eq!(
            ordinary_process_scope(Some(&block(&[("PSExecutionPolicyPreference", "")]))),
            Some(Undefined),
            "an empty value is no value"
        );
    }

    /// PIN (release read B1) — **after Enable and Undo on a machine that had no `$PROFILE`, an
    /// ordinary Windows PowerShell 5.1 session under `Restricted` prints nothing**, through a real
    /// 5.1 child (`bt_pty::test_shell`, no profile, no history). The child stands in for an
    /// ordinary session's profile load at the sandbox's path — PowerShell's host loads a profile
    /// that exists, under the session's policy — so a file left behind, even an empty one, is the
    /// red "cannot be loaded because running scripts is disabled" every session would print.
    /// Never the user's real `$PROFILE`: the profile and the data root are a temporary directory.
    ///
    /// RED (mutation: `retire_keeps_file` — `ProfileFiles::retire` skips the file's deletion: the
    /// empty file stays and the child prints the refusal).
    #[cfg(windows)]
    #[test]
    fn after_enable_and_undo_an_ordinary_restricted_session_prints_nothing() {
        use bt_pty::test_shell::Hygiene;
        let Some(program) = bt_platform::program_on_path(Path::new("powershell.exe")) else {
            eprintln!("powershell.exe: not installed; real-session arm skipped");
            return;
        };
        let root = temp_dir("enable-undo-restricted");
        let data = root.join("data");
        let profile = root
            .join("文档")
            .join("WindowsPowerShell")
            .join("profile.ps1");
        let script = data.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        let ordinary = |profile: &Path| {
            let hygiene = Hygiene::new();
            let literal = profile.display().to_string().replace('\'', "''");
            let output = hygiene
                .command(&program, bt_platform::quiet_command)
                .args([
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Restricted",
                    "-Command",
                ])
                .arg(format!(
                    "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); \
                     if (Test-Path -LiteralPath '{literal}') {{ . '{literal}' }}; 'session done'"
                ))
                .output()
                .expect("Windows PowerShell starts");
            (
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        };
        // The stand-in is faithful: a file there, even an empty one, is refused.
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, b"").unwrap();
        let (_, refused) = ordinary(&profile);
        assert!(
            !refused.trim().is_empty(),
            "an empty profile under Restricted is an error in every session"
        );
        std::fs::remove_dir_all(root.join("文档")).unwrap();

        profile_runtime::install_recorded(
            &profile,
            &data,
            &script,
            profile_marks::MANAGED_LINE,
            std::time::UNIX_EPOCH,
            Some(PowerShellEdition::WindowsPowerShell),
        )
        .expect("Enable writes the line");
        assert!(profile.is_file());
        profile_runtime::undo_profile_install(&profile, &data).expect("Undo");
        let (stdout, stderr) = ordinary(&profile);
        assert_eq!(
            stderr.trim(),
            "",
            "an ordinary session prints nothing: {stdout}"
        );
        assert_eq!(stdout.trim(), "session done");
        assert!(!root.join("文档").exists(), "nothing Folio created is left");
        let _ = std::fs::remove_dir_all(root);
    }

    /// PIN — **no button before the edition has been looked at** (review C-5 of
    /// T-INTEGRATION-INJECT-4). An uncomposable row whose edition has no observation yet is
    /// `Pending`: the line may already be there, or the policy may refuse it.
    ///
    /// RED (mutation: `None => Offer` in `profile_fallback_from_parts`).
    #[test]
    fn an_unobserved_edition_is_pending_not_offered() {
        assert_eq!(
            profile_fallback_from_parts(true, true, false, false, Heard::Answer(()), Heard::NotYet),
            PowerShellProfileFallback::Pending
        );
    }

    /// PIN — **where the profile probe does not exist, the row model never offers** (review G-6
    /// of T-INTEGRATION-INJECT-4). Whatever else is known, a platform without the probe is
    /// `Unsupported`, and the public model passes the probe's own platform fact.
    ///
    /// RED (mutations: delete the `!probe_exists` arm — the observed, line-less, permissive case
    /// is `Offer`; pass `true` instead of `PROFILE_PROBE_EXISTS` in
    /// `powershell_profile_fallback` — red on a build without the probe).
    #[test]
    fn a_platform_without_the_profile_probe_never_offers() {
        for observed in [
            Heard::NotYet,
            Heard::Failed,
            Heard::Answer((PolicyCause::Allows, false)),
            Heard::Answer((PolicyCause::Allows, true)),
            Heard::Answer((PolicyCause::Changeable, false)),
        ] {
            for parse in [Heard::Answer(()), Heard::NotYet, Heard::Failed] {
                assert_eq!(
                    profile_fallback_from_parts(false, true, false, false, parse, observed),
                    PowerShellProfileFallback::Unsupported,
                    "{observed:?} parse={parse:?}"
                );
            }
        }
        // No test publishes an observation, so on this build an uncomposable row is waiting for
        // one where the probe exists and is unsupported where it does not.
        let answer = powershell_profile_fallback(
            Path::new("pwsh.exe"),
            &os_words(&["-NoExit", "-File", "enter.ps1"]),
            true,
        );
        assert_eq!(
            answer,
            if PROFILE_PROBE_EXISTS {
                PowerShellProfileFallback::Pending
            } else {
                PowerShellProfileFallback::Unsupported
            }
        );
    }

    /// RED (mutation: treat `-c` as a literal name, discard `-NoExit`, or let
    /// the next option parse after Command) — the motivating generated rows are
    /// the host's Command terminal, not an unsafe suffix point.
    #[test]
    fn visual_studio_and_conda_rows_classify_as_commands_with_noexit() {
        let vs_text =
            "&{Import-Module 'Microsoft.VisualStudio.DevShell.dll'; Enter-VsDevShell a2ec33a6}";
        let conda_text = "& 'C:/Miniconda/shell/condabin/conda-hook.ps1' ; conda activate 'base'";
        for (program, words, expected) in [
            ("powershell.exe", vec!["-noe", "-c", vs_text], vs_text),
            ("powershell.exe", vec!["-noe", "/c", vs_text], vs_text),
            (
                "pwsh.exe",
                vec![
                    "-ExecutionPolicy",
                    "ByPass",
                    "-NoExit",
                    "-Command",
                    conda_text,
                ],
                conda_text,
            ),
        ] {
            let parsed = classify_powershell_arguments(Path::new(program), &os_words(&words))
                .expect("a real host line");
            assert!(parsed.no_exit, "{program} {words:?}");
            assert!(
                parsed
                    .non_terminal
                    .iter()
                    .any(|option| option.parameter == "noexit" && option.value.is_none()),
                "the parsed non-terminal list names NoExit: {program} {words:?}"
            );
            assert!(
                matches!(parsed.terminal, PowerShellTerminal::Command { text, .. } if text == expected)
            );
        }
        let conda = classify_powershell_arguments(
            Path::new("pwsh.exe"),
            &os_words(&[
                "-ExecutionPolicy",
                "ByPass",
                "-NoExit",
                "-Command",
                "Get-Date",
            ]),
        )
        .unwrap();
        assert_eq!(conda.non_terminal[0].parameter, "executionpolicy");
        assert_eq!(conda.non_terminal[0].option, 0);
        assert_eq!(conda.non_terminal[0].value, Some(1));
    }

    /// RED (mutations: join with no space, use `;`, add `-NoExit`, accept an
    /// unknown parse, rewrite a terminal form, or omit the rendered-line cap).
    #[test]
    fn powershell_composition_table_preserves_every_terminal_contract() {
        let script = Path::new("C:/Folio/folio.ps1");
        let program = Path::new("pwsh.exe");
        let loader = powershell_load_command(program, script);
        let compose = |words: &[&str], answer| {
            composed_powershell_arguments(program, &os_words(words), script, answer)
        };

        let plain = compose(&["-NoLogo"], None).expect("non-terminal");
        assert_eq!(plain[..3], os_words(&["-NoLogo", "-NoExit", "-Command"]));
        assert_eq!(plain[3], OsString::from(&loader));

        let command = compose(&["-NoExit", "-c", "$x=", "'several' # comment"], Some(true))
            .expect("parse-valid command");
        assert_eq!(command.len(), 3);
        assert_eq!(command[..2], os_words(&["-NoExit", "-c"]));
        assert_eq!(
            command[2],
            OsString::from(format!("$x= 'several' # comment\r\n{loader}"))
        );
        assert_eq!(command.iter().filter(|word| *word == "-NoExit").count(), 1);

        let unicode = "$global:answer='雪'";
        let encoded = encode_command(unicode);
        let encoded_words = ["-NoExit", "-ec", encoded.as_str()];
        let result = compose(&encoded_words, Some(true)).expect("encoded command");
        assert_eq!(
            decode_encoded_command(result[2].to_str().unwrap()),
            Some(format!("{unicode}\r\n{loader}"))
        );
        assert_eq!(result[..2], os_words(&["-NoExit", "-ec"]));

        let trailing_noexit = ["-ec", encoded.as_str(), "-NoExit"];
        let result = compose(&trailing_noexit, Some(true)).expect("encoded command with suffix");
        assert_eq!(result[2], "-NoExit");
        assert_eq!(
            decode_encoded_command(result[1].to_str().unwrap()),
            Some(format!("{unicode}\r\n{loader}"))
        );

        for words in [
            vec!["-File", "x.ps1"],
            vec!["-cwa", "Get-Date"],
            vec!["-Command", "-"],
            vec!["-NonInteractive", "-Command", "Get-Date"],
            vec!["-unknown"],
        ] {
            assert_eq!(compose(&words, Some(true)), None, "{words:?}");
        }
        assert_eq!(compose(&["-Command", "Get-Date"], None), None);
        assert_eq!(compose(&["-Command", "Get-Date"], Some(false)), None);
        let huge = "x".repeat(40_000);
        assert_eq!(
            composed_powershell_arguments(
                program,
                &[OsString::from("-Command"), OsString::from(huge)],
                script,
                Some(true)
            ),
            None
        );
    }

    /// RED (mutations: treat Pending/Failed as valid or key on the executable
    /// without argv) — an exact changed row has no inherited grammar answer.
    #[test]
    fn parse_gate_cache_distinguishes_unknown_valid_invalid_failure_and_row_change() {
        let program = Path::new("C:/unique/parser-gate/pwsh.exe");
        let first = os_words(&["-Command", "Get-Date"]);
        let changed = os_words(&["-Command", "Get-Location"]);
        assert_eq!(cached_parse_answer(program, &first), None);
        let mut answers = PARSE_ANSWERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        answers.insert(
            parse_key(program, &first),
            ParseAnswer::Pending { attempt: 1 },
        );
        assert_eq!(
            answers.get(&parse_key(program, &first)),
            Some(&ParseAnswer::Pending { attempt: 1 })
        );
        drop(answers);
        assert_eq!(cached_parse_answer(program, &first), None);
        PARSE_ANSWERS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(parse_key(program, &first), ParseAnswer::Valid);
        assert_eq!(cached_parse_answer(program, &first), Some(true));
        assert_eq!(cached_parse_answer(program, &changed), None);
        for (arguments, answer) in [
            (os_words(&["-Command", "broken'"]), ParseAnswer::Invalid),
            (
                os_words(&["-Command", "probe failed"]),
                ParseAnswer::Failed {
                    attempts: 1,
                    failure: ParseProbeFailure::Deadline {
                        stdout: "[]".to_owned(),
                        stderr: "[]".to_owned(),
                    },
                },
            ),
        ] {
            let expected = matches!(answer, ParseAnswer::Invalid).then_some(false);
            PARSE_ANSWERS
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert(parse_key(program, &arguments), answer);
            assert_eq!(cached_parse_answer(program, &arguments), expected);
        }
    }

    /// RED (mutation: key the launcher's working-directory pair) — startup asks
    /// about the row before a pane has a concrete place, while birth adds that
    /// non-terminal pair. Both name the same command grammar fact.
    #[test]
    fn inject4_launcher_flag_profile_matches_the_startup_parse_cache_key() {
        let program = Path::new("pwsh.exe");
        let startup = os_words(&["-Command", "Get-Date"]);
        let birth = os_words(&[
            "-WorkingDirectory",
            "C:/reader/project",
            "-Command",
            "Get-Date",
        ]);
        assert_eq!(parse_key(program, &startup), parse_key(program, &birth));
        assert_eq!(
            command_text(program, &startup),
            command_text(program, &birth)
        );
    }

    /// PIN — **every spelling of the working-directory launcher flag leaves the parse-cache
    /// identity** (review E of T-INTEGRATION-INJECT-4). The spellings are the table's own: each
    /// call whose parameter is `workingdirectory`, every accepted length of it, with each
    /// prefix and in two cases; those the classifier reads as that parameter all key the same
    /// fact as the row without the pair. The alias `wd` is among them.
    ///
    /// RED (mutation: the `wd` entry's `parameter: "wd"`, i.e. identity by spelled name) — `-wd`
    /// is no longer read as the working directory, and the set loses it.
    #[test]
    fn every_spelling_of_the_working_directory_leaves_the_parse_key() {
        let program = Path::new("pwsh.exe");
        let startup = os_words(&["-Command", "Get-Date"]);
        let mut spellings = Vec::new();
        for option in option_table(program)
            .iter()
            .filter(|option| option.parameter == "workingdirectory")
        {
            for length in option.minimum.len()..=option.name.len() {
                let key = &option.name[..length];
                for prefix in ["-", "--", "/"] {
                    for spelled in [key.to_owned(), key.to_ascii_uppercase()] {
                        spellings.push(format!("{prefix}{spelled}"));
                    }
                }
            }
        }
        let read_as_working_directory = spellings
            .into_iter()
            .filter(|spelling| {
                classify_powershell_arguments(
                    program,
                    &os_words(&[
                        spelling.as_str(),
                        "C:/reader/project",
                        "-Command",
                        "Get-Date",
                    ]),
                )
                .is_some_and(|parsed| parsed.non_terminal[0].parameter == "workingdirectory")
            })
            .collect::<Vec<_>>();
        for expected in ["-wd", "/WD", "-WorkingDirectory", "--wo"] {
            assert!(
                read_as_working_directory
                    .iter()
                    .any(|spelling| spelling.eq_ignore_ascii_case(expected)),
                "{expected} is a working-directory spelling: {read_as_working_directory:?}"
            );
        }
        for spelling in &read_as_working_directory {
            let birth = os_words(&[
                spelling.as_str(),
                "C:/reader/project",
                "-Command",
                "Get-Date",
            ]);
            assert_eq!(
                parse_key(program, &startup),
                parse_key(program, &birth),
                "{spelling}"
            );
        }
    }

    /// RED (mutations: collapse a nonzero exit into `None`, accept arbitrary
    /// stdout, or omit either output prefix) — the diagnostic names the stage
    /// that failed and carries the first bytes needed to distinguish encoding,
    /// host output and a silent child.
    #[test]
    fn a_parse_probe_failure_names_its_stage_and_first_output_bytes() {
        let malformed = parse_probe_answer(ProbeOutput {
            stdout: vec![0xef, 0xbb, 0xbf, b'1'],
            stderr: "错误".as_bytes().to_vec(),
        })
        .expect_err("a BOM is not the probe's one-byte protocol");
        let rendered = malformed.to_string();
        assert!(rendered.starts_with("output not recognised"));
        assert!(rendered.contains("[ef bb bf 31]"));
        assert!(rendered.contains("[e9 94 99 e8 af af]"));

        let exited = ParseProbeFailure::Exit {
            code: Some(23),
            stdout: "[31]".to_owned(),
            stderr: "[]".to_owned(),
        }
        .to_string();
        assert!(exited.starts_with("exit status Some(23)"));
        assert!(exited.contains("stdout first bytes [31]"));
        assert!(exited.contains("stderr first bytes []"));
    }

    /// RED (mutation: make `Failed` an occupied terminal cache entry again) —
    /// a transient probe failure remains unknown and the next request can turn
    /// the exact row into a valid answer.
    #[test]
    fn a_failed_parse_probe_is_unknown_and_the_next_request_can_answer() {
        let program = Path::new("C:/unique/parser-retry/pwsh.exe");
        let arguments = os_words(&["-Command", "'天下為公'"]);
        let question = ParseQuestion::new(program, &arguments, "'天下為公'".to_owned());

        let first =
            claim_parse_attempt(question.clone(), ParseAsker::Background).expect("first attempt");
        publish_parse_attempt(
            first.question.key,
            first.number,
            Err(ParseProbeFailure::Deadline {
                stdout: "[]".to_owned(),
                stderr: "[]".to_owned(),
            }),
        );
        assert_eq!(cached_parse_answer(program, &arguments), None);

        let second =
            claim_parse_attempt(question, ParseAsker::Background).expect("failure is retryable");
        assert_eq!(second.number, 2);
        publish_parse_attempt(second.question.key, second.number, Ok(true));
        assert_eq!(cached_parse_answer(program, &arguments), Some(true));
    }

    /// RED (mutation: raise `PARSE_PROBE_ATTEMPT_LIMIT` from three to four) — a permanently
    /// failing parser starts three children for an exact row, never one per
    /// pane birth for the rest of the process.
    #[test]
    fn a_permanently_failed_parse_probe_has_a_process_attempt_limit() {
        let program = Path::new("C:/unique/parser-attempt-limit/pwsh.exe");
        let arguments = os_words(&["-Command", "Write-Output 雪"]);
        let question = ParseQuestion::new(program, &arguments, "Write-Output 雪".to_owned());

        assert_eq!(PARSE_PROBE_ATTEMPT_LIMIT, 3);
        for expected in 1..=3 {
            let attempt = claim_parse_attempt(question.clone(), ParseAsker::Background)
                .expect("bounded attempt");
            assert_eq!(attempt.number, expected);
            publish_parse_attempt(
                attempt.question.key,
                attempt.number,
                Err(ParseProbeFailure::Exit {
                    code: Some(17),
                    stdout: "[31]".to_owned(),
                    stderr: "[e9 9b aa]".to_owned(),
                }),
            );
        }
        assert!(claim_parse_attempt(question.clone(), ParseAsker::Background).is_none());
        assert_eq!(cached_parse_answer(program, &arguments), None);
    }

    /// PIN (release read m2) — **a parse question that got no answer says so in the row, and a
    /// visit to the Profiles page asks it again past the attempt limit**; births still do not.
    /// What the row says while the question is in flight is `Pending`; after a failure it is
    /// `Undetermined` ("could not be determined … asks again"), never the "not provided" of an
    /// answer.
    ///
    /// RED (mutations: `visit_limited` — the attempt limit also binds `ParseAsker::Visit`;
    /// `failed_is_pending` — `heard_parse` reads `Failed` as `NotYet`).
    #[test]
    fn a_failed_parse_question_is_undetermined_and_a_visit_asks_it_again() {
        let program = Path::new("C:/unique/parser-visit/pwsh.exe");
        let arguments = os_words(&["-NoExit", "-Command", "Write-Output 雨"]);
        let question = ParseQuestion::new(program, &arguments, "Write-Output 雨".to_owned());
        let fail = |attempt: ParseAttempt| {
            publish_parse_attempt(
                attempt.question.key,
                attempt.number,
                Err(ParseProbeFailure::Deadline {
                    stdout: "[]".to_owned(),
                    stderr: "[]".to_owned(),
                }),
            );
        };
        let first =
            claim_parse_attempt(question.clone(), ParseAsker::Background).expect("first attempt");
        assert_eq!(heard_parse(program, &arguments), Heard::NotYet, "in flight");
        fail(first);
        assert_eq!(heard_parse(program, &arguments), Heard::Failed);
        for _ in 1..PARSE_PROBE_ATTEMPT_LIMIT {
            fail(claim_parse_attempt(question.clone(), ParseAsker::Background).expect("bounded"));
        }
        assert!(claim_parse_attempt(question.clone(), ParseAsker::Background).is_none());
        let visit = claim_parse_attempt(question.clone(), ParseAsker::Visit)
            .expect("a visit asks a failed question again");
        assert_eq!(visit.number, PARSE_PROBE_ATTEMPT_LIMIT + 1);
        assert!(
            claim_parse_attempt(question.clone(), ParseAsker::Visit).is_none(),
            "a question in flight is not asked twice"
        );
        publish_parse_attempt(visit.question.key, visit.number, Ok(false));
        assert_eq!(heard_parse(program, &arguments), Heard::Answer(()));
        assert!(
            claim_parse_attempt(question, ParseAsker::Visit).is_none(),
            "an answered question is not asked again"
        );
        assert_eq!(
            profile_fallback_from_parts(true, true, false, false, Heard::Failed, Heard::NotYet),
            PowerShellProfileFallback::Undetermined
        );
        assert_eq!(
            PowerShellProfileFallback::Undetermined.policy_cause(),
            Some(PolicyCause::Undetermined)
        );
        assert_eq!(
            PolicyCause::Undetermined.sentence(),
            Some(crate::i18n::Text::CapPowerShellUndetermined)
        );
    }

    /// RED (mutation: execute the claimed attempt inside `request_parse_retry`
    /// before handing it to `start`) — a birth with an unknown answer returns
    /// its original argv after handing one job to the scheduler; it neither
    /// prepares the script nor executes the parser job itself. A second birth
    /// sees Pending and does not hand over another job.
    #[test]
    fn a_birth_schedules_a_parse_retry_without_waiting_for_it() {
        let program = Path::new("C:/unique/parser-birth-retry/pwsh.exe");
        let arguments = os_words(&["-Command", "Write-Output '混合 script'"]);
        let question =
            ParseQuestion::new(program, &arguments, "Write-Output '混合 script'".to_owned());
        let failed = claim_parse_attempt(question, ParseAsker::Background).expect("first attempt");
        publish_parse_attempt(
            failed.question.key,
            failed.number,
            Err(ParseProbeFailure::Stdin {
                error: "fixture".to_owned(),
                stdout: "[]".to_owned(),
                stderr: "[]".to_owned(),
            }),
        );

        let prepared = std::cell::Cell::new(false);
        let scheduled = std::cell::RefCell::new(None);
        let actual = compose_with_prepared_and_retry(
            program,
            &arguments,
            true,
            || {
                prepared.set(true);
                None
            },
            |retry| {
                request_parse_retry(retry, |attempt| {
                    *scheduled.borrow_mut() = Some(attempt);
                    Ok(())
                });
            },
        );
        assert_eq!(actual, arguments);
        assert!(!prepared.get());
        assert_eq!(
            scheduled.borrow().as_ref().map(|attempt| attempt.number),
            Some(2)
        );
        request_parse_retry(
            ParseQuestion::new(program, &arguments, "Write-Output '混合 script'".to_owned()),
            |_| panic!("a concurrent birth scheduled a second parser child"),
        );
    }

    /// RED (mutations: `compose_with_prepared` asks `prepare` before its gate; or
    /// `last_resort_arguments` composes for some program other than `bt_pty::LAST_RESORT_SHELL`)
    /// — a birth prepares the script only for an argv the composer will load into, and the
    /// last-resort retry is composed like every other PowerShell birth: on Windows it is a
    /// PowerShell and is handed the same load; off Windows `/bin/sh` is handed nothing.
    #[test]
    fn a_birth_prepares_the_script_only_for_an_argv_that_loads_it() {
        let root = temp_dir("birth-prepares");
        let script = root.join("folio.ps1");
        std::fs::write(&script, "# fixture").unwrap();
        let asked = std::cell::Cell::new(0);
        let prepare = || {
            asked.set(asked.get() + 1);
            Some(script.clone())
        };
        for (program, words, enabled) in [
            ("cmd.exe", vec![], true),
            ("bash.exe", vec!["--login"], true),
            ("pwsh.exe", vec!["-Command", "Get-Date"], true),
            ("pwsh.exe", vec![], false),
        ] {
            let own = words.iter().map(OsString::from).collect::<Vec<_>>();
            assert_eq!(
                compose_with_prepared(Path::new(program), &own, enabled, prepare),
                own,
                "{program} {words:?} {enabled}"
            );
        }
        assert_eq!(
            asked.get(),
            0,
            "nothing prepared for an argv that does not load it"
        );

        let own = vec![OsString::from("-NoLogo")];
        let composed = compose_with_prepared(Path::new("pwsh.exe"), &own, true, prepare);
        assert_eq!(asked.get(), 1);
        assert_eq!(
            composed,
            compose_powershell_arguments(Path::new("pwsh.exe"), &own, Some(&script), true)
        );

        let retry = compose_with_prepared(
            Path::new(bt_pty::LAST_RESORT_SHELL),
            &bt_pty::last_resort_arguments(),
            true,
            prepare,
        );
        if cfg!(windows) {
            assert_eq!(retry[..1], [OsString::from("-NoLogo")]);
            assert_eq!(
                retry[1..3],
                [OsString::from("-NoExit"), OsString::from("-Command")]
            );
            assert_eq!(
                retry[3],
                OsString::from(powershell_load_command(
                    Path::new(bt_pty::LAST_RESORT_SHELL),
                    &script
                ))
            );
        } else {
            assert_eq!(retry, bt_pty::last_resort_arguments());
        }
        let source = Index::of_package("bt-app")
            .body_of(&bt_source::ItemQuery::function("last_resort_arguments"))
            .expect("the retry's composition");
        assert!(
            source.contains("Path::new(bt_pty::LAST_RESORT_SHELL)")
                && source.contains("&bt_pty::last_resort_arguments()")
                && source.contains("compose_powershell_birth("),
            "the retry's argv is the last-resort shell's own words through the birth composer"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (mutation: trust `prepare`'s path without checking it, or memoize its
    /// first failure) — a missing copy is a plain birth and the following birth
    /// asks preparation again and can integrate.
    #[test]
    fn a_failed_or_missing_preparation_is_retried_at_the_next_birth() {
        let root = temp_dir("birth-retries");
        let script = root.join("folio.ps1");
        let words = os_words(&["-NoLogo"]);
        let attempts = std::cell::Cell::new(0);
        let first = compose_with_prepared(Path::new("pwsh.exe"), &words, true, || {
            attempts.set(attempts.get() + 1);
            None
        });
        assert_eq!(first, words);
        let missing = compose_with_prepared(Path::new("pwsh.exe"), &words, true, || {
            attempts.set(attempts.get() + 1);
            Some(script.clone())
        });
        assert_eq!(missing, words);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&script, "# fixture").unwrap();
        let ready = compose_with_prepared(Path::new("pwsh.exe"), &words, true, || {
            attempts.set(attempts.get() + 1);
            Some(script.clone())
        });
        assert_eq!(attempts.get(), 3);
        assert_eq!(ready[..3], os_words(&["-NoLogo", "-NoExit", "-Command"]));
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (mutations: `powershell_script_in_trial` installs into the durable directory; or names
    /// the trial copy even when the durable one already holds this build's bytes; or
    /// `powershell_script_for_birth` stops asking the trial gate) — **the trial rule**: a trial
    /// whose durable script already holds this build's bytes names the durable path and writes
    /// nothing; a trial facing a stale or missing durable script writes its own copy and leaves
    /// the durable file byte for byte as it was; outside a trial (or after the commit) the durable
    /// file is the one written.
    #[test]
    fn a_trial_names_a_matching_durable_script_and_otherwise_its_own_copy() {
        let root = temp_dir("trial-script");
        let durable = root.join("data").join(SCRIPT_DIRECTORY);
        let trial = root.join("trial").join(SCRIPT_DIRECTORY);
        let shipped = "# this build\n";

        // Matching durable copy: named, read only.
        std::fs::create_dir_all(&durable).unwrap();
        std::fs::write(durable.join(SCRIPT_FILE_PS1), shipped).unwrap();
        let before = std::fs::metadata(durable.join(SCRIPT_FILE_PS1))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            powershell_script_in_trial(&durable, &trial, shipped),
            Some(durable.join(SCRIPT_FILE_PS1))
        );
        assert!(
            !trial.exists(),
            "nothing written for a matching durable copy"
        );
        assert_eq!(
            std::fs::metadata(durable.join(SCRIPT_FILE_PS1))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );

        // Stale, then missing: the trial's own copy, the durable one untouched.
        for stale in [Some("# an older build\n"), None] {
            let _ = std::fs::remove_dir_all(&trial);
            match stale {
                Some(text) => std::fs::write(durable.join(SCRIPT_FILE_PS1), text).unwrap(),
                None => std::fs::remove_file(durable.join(SCRIPT_FILE_PS1)).unwrap(),
            }
            let named = powershell_script_in_trial(&durable, &trial, shipped);
            assert_eq!(named, Some(trial.join(SCRIPT_FILE_PS1)), "{stale:?}");
            assert_eq!(
                std::fs::read_to_string(trial.join(SCRIPT_FILE_PS1)).unwrap(),
                shipped
            );
            match stale {
                Some(text) => assert_eq!(
                    std::fs::read_to_string(durable.join(SCRIPT_FILE_PS1)).unwrap(),
                    text,
                    "the old build's copy is left as it was"
                ),
                None => assert!(!durable.join(SCRIPT_FILE_PS1).exists()),
            }
        }

        // The commit's write, and every write outside a trial: the durable copy, repaired.
        std::fs::write(durable.join(SCRIPT_FILE_PS1), "# an older build\n").unwrap();
        assert_eq!(
            install_script_at(&durable, SCRIPT_FILE_PS1, shipped),
            Some(durable.join(SCRIPT_FILE_PS1))
        );
        assert_eq!(
            std::fs::read_to_string(durable.join(SCRIPT_FILE_PS1)).unwrap(),
            shipped
        );

        // And the birth asks the trial gate by this writer, so a commit releases it.
        let body = Index::of_package("bt-app")
            .body_of(&bt_source::ItemQuery::function(
                "powershell_script_for_birth",
            ))
            .expect("the birth's script");
        assert!(
            body.contains("update_trial::defer(crate::update_trial::Writer::PowerShellScript)")
        );
        assert!(body.contains("powershell_script_in_trial("));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn powershell_loader_is_guarded_and_pwsh_revives_only_the_loaded_assembly() {
        let path = Path::new("C:/Folio/folio.ps1");
        let winps = powershell_load_command(Path::new("powershell.exe"), path);
        let pwsh = powershell_load_command(Path::new("pwsh.exe"), path);
        for load in [&winps, &pwsh] {
            assert!(load.starts_with(
                "if ($ExecutionContext.SessionState.LanguageMode -eq 'FullLanguage')"
            ));
            assert!(load.contains("Get-Module PSReadLine, Microsoft.PowerShell.PSReadLine"));
            assert!(load.contains("function global:PSConsoleHostReadLine"));
            assert!(load.contains("Microsoft.PowerShell.Core\\Set-StrictMode -Off"));
            assert!(load.contains("try { $folioScript = [IO.File]::ReadAllText("));
            assert!(load.contains("catch {}"));
            assert!(load.contains("$Global:__FolioShellIntegration.ReadLineType"));
            assert!(!load.contains("$Global:__FolioReadLineType"));
        }
        assert!(!winps.contains("GetAssemblies"));
        assert_eq!(pwsh.contains("GetAssemblies"), cfg!(windows));
        assert_eq!(
            pwsh.contains("Microsoft.PowerShell.PSReadLine.dll"),
            cfg!(windows)
        );
        assert!(SCRIPT_PS1.contains("ReadLineType = $readLineType"));
        assert!(!SCRIPT_PS1.contains("$Global:__FolioReadLineType"));
    }

    /// RED (mutation: omit the retirement hook or remove only the script
    /// file) — rollback/discard retirement removes the transaction-owned
    /// PowerShell staging root together with the update transaction.
    #[test]
    fn retired_trial_removes_its_powershell_script_root() {
        // Minted as the product mints one, so no other test's transaction is this one.
        let txn = crate::update_job::mint_txn();
        let root = std::env::temp_dir().join(format!("folio-trial-{txn}"));
        let script = root.join(SCRIPT_DIRECTORY).join(SCRIPT_FILE_PS1);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, SCRIPT_PS1).unwrap();

        remove_trial_script(txn);

        assert!(!root.exists(), "the transaction-owned staging root remains");
        let delete = Index::of_package("bt-app")
            .body_of(&bt_source::ItemQuery::function("delete").in_module("crate::update_startup"))
            .expect("update-start retirement delete");
        assert!(delete.contains("remove_trial_script(txn)"));
    }

    /// RED (mutation: replace atomically only on one caller, or write the
    /// destination directly) — two preparations leave one complete script,
    /// never a splice or a truncated file.
    #[test]
    fn two_script_preparations_replace_atomically() {
        let root = temp_dir("atomic-script");
        std::fs::create_dir_all(&root).unwrap();
        let name = "folio.ps1";
        std::fs::write(root.join(name), "standing").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let texts = ["a".repeat(128 * 1024), "b".repeat(128 * 1024)];
        let joins = texts
            .into_iter()
            .map(|text| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    install_script_at(&root, name, &text)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let outcomes = joins
            .into_iter()
            .map(|join| join.join().unwrap())
            .collect::<Vec<_>>();
        let installed = root.join(name);
        assert!(
            outcomes.iter().any(Option::is_some),
            "at least one competing replacement lands"
        );
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.is_none() || outcome.as_deref() == Some(installed.as_path())),
            "a refusal names nothing else: {outcomes:?}"
        );
        let final_text = std::fs::read_to_string(root.join(name)).unwrap();
        assert_eq!(final_text.len(), 128 * 1024);
        assert!(
            final_text.bytes().all(|byte| byte == b'a')
                || final_text.bytes().all(|byte| byte == b'b')
        );
        let body = Index::of_package("bt-app")
            .body_of(&bt_source::ItemQuery::function("install_script_at"))
            .expect("the script writer");
        assert!(body.contains("bt_persist::atomic_replace_preserving"));
        assert!(body.contains("bt_persist::atomic_write"));
        assert!(!body.contains("std::fs::write"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    fn real_powershell_output_until(
        session: &mut bt_pty::test_shell::TestShell,
        needle: &[u8],
        silent_for: std::time::Duration,
    ) -> Vec<u8> {
        let started = std::time::Instant::now();
        let absolute_deadline = started + std::time::Duration::from_secs(180);
        let mut last_output = started;
        let mut output = Vec::new();
        while !output.windows(needle.len()).any(|window| window == needle) {
            let now = std::time::Instant::now();
            if now >= absolute_deadline || now.duration_since(last_output) >= silent_for {
                panic!(
                    "PowerShell never emitted {:?}; elapsed {:?}, silent {:?}, {} bytes; {}; \
                     output was {:?}",
                    String::from_utf8_lossy(needle),
                    now.duration_since(started),
                    now.duration_since(last_output),
                    output.len(),
                    session.account(),
                    String::from_utf8_lossy(&output)
                );
            }
            let chunk = session.read_output();
            if chunk.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            } else {
                last_output = std::time::Instant::now();
            }
            output.extend(chunk);
        }
        output
    }

    #[cfg(windows)]
    fn real_parse_answer(program: &Path, text: &str) -> bool {
        let mut failures = Vec::new();
        for attempt in 1..=PARSE_PROBE_ATTEMPT_LIMIT {
            match run_parse_probe(program, text) {
                Ok(answer) => return answer,
                Err(failure) => {
                    eprintln!(
                        "{} target parser attempt {attempt}/{PARSE_PROBE_ATTEMPT_LIMIT} failed: {failure}",
                        program.display()
                    );
                    failures.push(failure);
                }
            }
        }
        panic!(
            "{} target parser gave no answer in {PARSE_PROBE_ATTEMPT_LIMIT} attempts: {failures:?}",
            program.display()
        );
    }

    /// The shape of a composed row as this module's own classifier reads it — handed to
    /// `bt_pty::test_shell` so the test shell does not read the row a second, different way
    /// (`-noe -c` is the genuine Visual Studio row, and the subject here).
    #[cfg(windows)]
    fn test_shell_shape(
        program: &Path,
        arguments: &[OsString],
    ) -> bt_pty::test_shell::PowerShellShape {
        let parsed = classify_powershell_arguments(program, arguments)
            .unwrap_or_else(|| panic!("the row is classified: {arguments:?}"));
        bt_pty::test_shell::PowerShellShape {
            draws_a_prompt: parsed.no_exit || matches!(parsed.terminal, PowerShellTerminal::None),
            startup_text: match parsed.terminal {
                PowerShellTerminal::Command { option, .. } => Some(option + 1),
                _ => None,
            },
            has_no_profile: parsed
                .non_terminal
                .iter()
                .any(|option| option.parameter == "noprofile"),
        }
    }

    /// One composed row in its own test shell: `bt_pty::test_shell` with the row's shape, so the
    /// argv is the composed one and the hygiene — `-NoProfile`, a temporary HOME, USERPROFILE,
    /// APPDATA and LOCALAPPDATA, and the history refusal verified before a byte is typed — is
    /// the one every real-shell test in the workspace has.
    #[cfg(windows)]
    fn real_row(
        hygiene: bt_pty::test_shell::Hygiene,
        program: &Path,
        arguments: &[OsString],
        columns: u16,
        rows: u16,
    ) -> bt_pty::test_shell::TestShell {
        let home = hygiene.home();
        let hygiene = hygiene.with_powershell_shape(test_shell_shape(program, arguments));
        bt_pty::test_shell::TestShell::spawn_shell_in(
            hygiene,
            program.to_path_buf(),
            arguments,
            &|| last_resort_arguments(false),
            &[],
            bt_pty::PtySize::cells(
                std::num::NonZeroU16::new(columns).unwrap(),
                std::num::NonZeroU16::new(rows).unwrap(),
            ),
            Some(home),
        )
        .unwrap()
    }

    /// Real-shell acceptance through the same headless ConPTY as a pane.
    ///
    /// Each row starts in its own `bt_pty::test_shell` (T-TEST-SHELL-HYGIENE), handed the row's
    /// shape as `classify_powershell_arguments` reads it, so the composed argv is what runs:
    /// the rows carry `-NoProfile` already (the Windows Known Folder that supplies `$PROFILE`
    /// cannot be redirected by HOME/USERPROFILE — Part B measured that explicitly), and the
    /// hygiene's history refusal stands where this test's own used to — at the head of the
    /// user's `-Command` text (`HYGIENE`, replaced before the start; the parser is asked about
    /// the text that runs), and as the first line of the `-NoExit -File` row's script, whose argv
    /// stays exactly as written. The one-shot row draws no prompt and is not touched. Nothing
    /// is typed into a row whose refusal was not read back.
    ///
    /// RED (mutation: drop the CRLF composition, add `-NoExit` to the one-shot
    /// row, or compose File) — prompt marks/effect, exit, and plain-file arms
    /// fail independently.
    #[cfg(windows)]
    #[test]
    fn real_command_rows_integrate_without_touching_account_state() {
        use bt_pty::test_shell::{HYGIENE, Hygiene, powershell_hygiene};

        let root = temp_dir("real-command-rows");
        std::fs::create_dir_all(&root).unwrap();
        let integration = root.join(SCRIPT_FILE_PS1);
        std::fs::write(&integration, SCRIPT_PS1).unwrap();

        for name in ["powershell.exe", "pwsh.exe"] {
            let Some(program) = bt_platform::program_on_path(Path::new(name)) else {
                eprintln!("{name}: not installed; real command-row arm skipped");
                continue;
            };
            let hygiene = Hygiene::new();
            let user = format!("{HYGIENE}; $Global:BT_FOLIO_EFFECT = 'persisted'");
            let runs = user.replace(HYGIENE, &powershell_hygiene(&hygiene));
            let row = os_words(&["-NoProfile", "-NoExit", "-Command", &user]);
            let arguments = composed_powershell_arguments(
                &program,
                &row,
                &integration,
                Some(real_parse_answer(&program, &runs)),
            )
            .expect("the valid command composes");
            let mut session = real_row(hygiene, &program, &arguments, 100, 30);
            let mut output = real_powershell_output_until(
                &mut session,
                b"\x1b]133;B",
                std::time::Duration::from_secs(20),
            );
            assert!(
                output
                    .windows(b"\x1b]7;".len())
                    .any(|window| window == b"\x1b]7;")
            );
            session
                .write(b"Write-Output ('BT_EFFECT|' + $Global:BT_FOLIO_EFFECT); exit\r")
                .unwrap();
            output.extend(real_powershell_output_until(
                &mut session,
                b"BT_EFFECT|persisted",
                std::time::Duration::from_secs(20),
            ));
            assert!(
                output
                    .windows(b"\x1b]133".len())
                    .any(|window| window == b"\x1b]133")
            );
            assert!(
                !session.hygiene().history_file().exists(),
                "SaveNothing wrote no scratch history either"
            );
            session.shutdown().unwrap();

            let one_shot = "Write-Output INJECT3_ONESHOT";
            let row = os_words(&["-NoProfile", "-Command", one_shot]);
            let arguments = composed_powershell_arguments(
                &program,
                &row,
                &integration,
                Some(real_parse_answer(&program, one_shot)),
            )
            .expect("one-shot command composes without changing its lifetime");
            assert!(!arguments.iter().any(|argument| argument == "-NoExit"));
            let mut session = real_row(Hygiene::new(), &program, &arguments, 100, 30);
            let output = real_powershell_output_until(
                &mut session,
                b"INJECT3_ONESHOT",
                std::time::Duration::from_secs(20),
            );
            assert!(
                !output
                    .windows(b"\x1b]133".len())
                    .any(|window| window == b"\x1b]133")
            );
            session.shutdown().unwrap();

            let hygiene = Hygiene::new();
            let file = hygiene.root().join(format!("{name}-plain.ps1"));
            std::fs::write(
                &file,
                format!(
                    "{}\r\nWrite-Output INJECT3_FILE_PLAIN\r\n",
                    powershell_hygiene(&hygiene)
                ),
            )
            .unwrap();
            let row = vec![
                OsString::from("-NoProfile"),
                OsString::from("-NoExit"),
                OsString::from("-File"),
                file.into_os_string(),
            ];
            assert_eq!(
                composed_powershell_arguments(&program, &row, &integration, Some(true)),
                None
            );
            let mut session = real_row(hygiene, &program, &row, 100, 30);
            let output = real_powershell_output_until(
                &mut session,
                b"INJECT3_FILE_PLAIN",
                std::time::Duration::from_secs(20),
            );
            assert!(
                !output
                    .windows(b"\x1b]133".len())
                    .any(|window| window == b"\x1b]133")
            );
            session.write(b"exit\r").unwrap();
            session.shutdown().unwrap();
        }

        let vs_program =
            PathBuf::from(r"C:\Windows\SysWOW64\WindowsPowerShell\v1.0\powershell.exe");
        let vs_module = PathBuf::from(
            r"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\Microsoft.VisualStudio.DevShell.dll",
        );
        if vs_program.is_file() && vs_module.is_file() {
            let hygiene = Hygiene::new();
            let vs = format!(
                "{HYGIENE}; &{{Import-Module {}; Enter-VsDevShell a2ec33a6}}; Write-Output INJECT3_VS_READY",
                powershell_single_quoted(&vs_module)
            );
            let runs = vs.replace(HYGIENE, &powershell_hygiene(&hygiene));
            let row = vec![
                OsString::from("-NoProfile"),
                OsString::from("-noe"),
                OsString::from("-c"),
                OsString::from(&vs),
            ];
            let arguments = composed_powershell_arguments(
                &vs_program,
                &row,
                &integration,
                Some(real_parse_answer(&vs_program, &runs)),
            )
            .expect("the installed Developer PowerShell row composes");
            let mut session = real_row(hygiene, &vs_program, &arguments, 120, 35);
            let output = real_powershell_output_until(
                &mut session,
                b"\x1b]133;B",
                std::time::Duration::from_secs(30),
            );
            assert!(
                output
                    .windows(16)
                    .any(|window| window == b"INJECT3_VS_READY")
            );
            assert!(
                output
                    .windows(b"\x1b]133".len())
                    .any(|window| window == b"\x1b]133")
            );
            session
                .write(b"if (Get-Command cl.exe -ErrorAction Ignore) { Write-Output INJECT3_CL_OK }; exit\r")
                .unwrap();
            let _output = real_powershell_output_until(
                &mut session,
                b"INJECT3_CL_OK",
                std::time::Duration::from_secs(20),
            );
            assert!(
                !session.hygiene().history_file().exists(),
                "SaveNothing wrote no scratch history either"
            );
            session.shutdown().unwrap();
        } else {
            eprintln!("Visual Studio Build Tools Developer PowerShell is absent; arm skipped");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn every_terminal_birth_uses_the_one_powershell_composition() {
        let index = Index::of_package("bt-app");
        let constructor = index
            .body_of(&bt_source::ItemQuery::function("create_leaf_session"))
            .expect("the one terminal-leaf constructor");
        // RED (mutations: compose on the window thread in the constructor; or drop the switch
        // from the birth; or have the birth worker hand bt-pty `bt_pty::last_resort_arguments`) —
        // the constructor reads the switch once and hands it to the birth, and the birth worker
        // composes both the pane's argv and the last-resort retry's with the one composer.
        assert_eq!(
            constructor
                .matches("shell_integration::powershell_integration_enabled()")
                .count(),
            1,
            "the leaf constructor reads the switch exactly once"
        );
        assert!(
            !constructor.contains("compose_powershell"),
            "the window thread composes no PowerShell load"
        );
        let birth = index
            .body_of(&bt_source::ItemQuery::function("bear"))
            .expect("the birth worker's body");
        assert_eq!(
            birth
                .matches("shell_integration::compose_powershell_birth(")
                .count(),
            1,
            "the birth worker composes the pane's argv"
        );
        assert!(
            birth.contains("shell_integration::last_resort_arguments(powershell_integration)"),
            "and the retry's, with the same switch"
        );

        let found = index
            .search(&Search::new(
                needle!(Pattern::identifier("create_leaf_session")),
                View::Identifiers,
            ))
            .expect("the constructor's complete call census")
            .in_the_product(index);
        let mut owners = found
            .owners(index)
            .into_keys()
            .map(|identity| match identity.type_owner {
                Some(owner) => format!("{owner}::{}", identity.name),
                None => identity.name,
            })
            .collect::<Vec<_>>();
        owners.sort();
        owners.dedup();
        assert_eq!(
            owners,
            [
                "Runtime::land_pane_birth",
                "Runtime::pop_out_preview",
                "Runtime::restart_shell",
                "Runtime::split_seat",
                "create_leaf_session",
                "create_tab_state",
            ],
            "new tab, restore, Recent and Duplicate enter through create_tab_state; split, \
             Restart shell and preview stand-in enter directly; every terminal birth still \
             reaches the constructor that composes PowerShell"
        );
    }

    fn prompt_of(command: &ShellCommand) -> String {
        command
            .environment
            .iter()
            .find(|(key, _)| key == "PROMPT")
            .map(|(_, value)| value.to_string_lossy().into_owned())
            .expect("the cmd profile must carry a PROMPT")
    }

    /// PIN — the script is a POSIX file and must ship as one.
    ///
    /// Red gate, and it is a *checkout* that would break it rather than an edit:
    /// with `core.autocrlf=true` — the Git for Windows installer's own default —
    /// every line of this file arrives ending `\r\n`, `include_str!` embeds the
    /// carriage returns, and bash reads `__bt_pwd_style=windows\r`, carrying the
    /// `\r` into the value. The symptom is not a parse error; it is a prompt that
    /// prints stray characters and comparisons that quietly never match.
    #[test]
    fn the_bash_script_ships_with_unix_line_endings() {
        assert!(
            !script_source().contains('\r'),
            "folio.bash must be checked out with LF endings — see .gitattributes"
        );
        // And it is the script, not an empty file that would inject nothing.
        for marker in ["133;A", "133;B", "133;C", "133;D", "]7;", "file://"] {
            assert!(
                script_source().contains(marker),
                "the script must emit {marker}"
            );
        }
    }

    /// PIN — the name this terminal announces itself under and the name the
    /// PowerShell script recognises are one string.
    ///
    /// `PtyCommand` puts `TERM_PROGRAM=<name>` in every child's environment;
    /// `folio.ps1` turns `FORCE_HYPERLINK` on for exactly the sessions whose
    /// `TERM_PROGRAM` it recognises as ours. Neither half can tell that the other
    /// has moved: a script comparing against a name nobody declares simply never
    /// takes the branch, and the only symptom is that `OSC 8` links in
    /// hyperlink-gated CLIs stop being links. There is no error, no warning, and
    /// nothing in the pane that says why.
    ///
    /// It reads the bytes that ship rather than a copy of them, for the reason
    /// `profiles::tests::the_integration_script_names_the_profiles_own_titles`
    /// gives: a constant restated here would agree with this file forever and
    /// with the script never.
    ///
    /// Red gate: rename [`bt_pty::TERM_PROGRAM`] without the script's literal, or
    /// the script's literal without the constant, and this fails.
    ///
    /// The script names this terminal twice — the guard that returns outside a
    /// Folio session, and the hyperlink declaration — and both are pinned: every
    /// `$env:TERM_PROGRAM` comparison in it is one of those two, spelled with the
    /// declared name. The script suite's session-scope case declares the same
    /// name, and is pinned here likewise.
    ///
    /// Red gate: rename [`bt_pty::TERM_PROGRAM`] without the script's literals,
    /// either literal without the constant, or the session-scope case's
    /// declaration without the constant, and this fails.
    #[test]
    fn the_integration_script_knows_the_name_this_terminal_announces() {
        let declared = bt_pty::TERM_PROGRAM;
        let guard = format!("if ($env:TERM_PROGRAM -ne '{declared}') {{");
        let hyperlink = format!("if ($env:TERM_PROGRAM -eq '{declared}' -and");
        for comparison in [&guard, &hyperlink] {
            assert_eq!(
                script_source_ps1().matches(comparison.as_str()).count(),
                1,
                "the terminal declares TERM_PROGRAM={declared:?}, so the script must \
                 test for it verbatim, once; folio.ps1 does not contain {comparison:?}"
            );
        }
        // And those are the *only* places the script compares the name, so a
        // rename cannot pass by leaving the old literal in a third branch.
        assert_eq!(
            script_source_ps1().matches("$env:TERM_PROGRAM -").count(),
            2,
            "the script recognises this terminal in exactly the guard and the \
             hyperlink declaration"
        );
        let session_scope =
            include_str!("../../../scripts/shell-integration/tests/session-scope.ps1");
        let declaration = format!("$env:TERM_PROGRAM = '{declared}'");
        assert_eq!(
            session_scope.matches("$env:TERM_PROGRAM =").count(),
            1,
            "the session-scope case declares this terminal in one place"
        );
        assert!(
            session_scope.contains(&declaration),
            "the session-scope case must declare {declaration:?}"
        );
    }

    /// PIN — **every variable this build announces to a pane by name is one a test shell does not
    /// inherit** (T-INTEGRATION-INJECT-4 round 3). A test run from inside a Folio pane inherits
    /// these and CI does not; `bt_pty::test_shell` removes exactly
    /// [`bt_pty::test_shell::PANE_ANNOUNCEMENTS`] from every child it builds, and the script
    /// suites' runner clears the same list. A name added to a pane here without being added
    /// there is a local gate that can pass for a reason CI does not have.
    ///
    /// RED (mutations: drop `FORCE_HYPERLINK` from `PANE_ANNOUNCEMENTS`; drop a name from
    /// `run.ps1`'s `$paneAnnouncements`).
    #[test]
    fn every_pane_announcement_is_one_the_test_shell_strips() {
        let stripped = bt_pty::test_shell::PANE_ANNOUNCEMENTS;
        for announced in [
            "TERM_PROGRAM",
            "TERM_PROGRAM_VERSION",
            "COLORTERM",
            "TERM",
            FORCE_HYPERLINK,
            INSTALLED_MARKER,
            USER_ZDOTDIR,
            crate::attention_wire::PANE_VARIABLE,
            crate::attention_wire::ENDPOINT_VARIABLE,
            crate::attention_wire::CAPABILITY_VARIABLE,
        ] {
            assert!(
                stripped.contains(&announced),
                "{announced} is announced to a pane and must be stripped from a test shell"
            );
        }
        // `FORWARDED` is what crosses into WSL; every name in it is an announcement.
        for forwarded in FORWARDED {
            let name = forwarded.split('/').next().unwrap_or_default();
            assert!(stripped.contains(&name), "{name} crosses into WSL");
        }
        let runner = include_str!("../../../scripts/shell-integration/tests/run.ps1");
        let line = runner
            .lines()
            .find(|line| line.starts_with("$paneAnnouncements = @("))
            .expect("the suites' runner clears the pane announcements");
        let listed = line.split('\'').skip(1).step_by(2).collect::<Vec<_>>();
        assert_eq!(
            listed, stripped,
            "run.ps1 clears exactly the test shell's list"
        );
    }

    /// PIN — **the bash and zsh scripts act wherever they are sourced** (owner's
    /// ruling 2026-10-04). A hand-installed copy sourced from an rc file on a host
    /// reached by ssh from a Folio pane, or in a `sudo -i` / `su -` shell inside
    /// one, sees no `TERM_PROGRAM`, and its standard `OSC 133` / `OSC 7` are what
    /// the pane reads; only `folio.ps1`, whose line Folio itself adds to
    /// `$PROFILE`, is scoped to Folio.
    ///
    /// RED (mutation: restore `[ "${TERM_PROGRAM-}" = Folio ] || return 0` in
    /// either script).
    #[test]
    fn the_bash_and_zsh_scripts_do_not_ask_which_terminal_runs_them() {
        for (name, source) in [
            ("folio.bash", script_source()),
            ("folio.zsh", script_source_zsh()),
        ] {
            assert!(
                !source.contains("TERM_PROGRAM"),
                "{name} must act in any terminal that sources it"
            );
        }
    }

    /// **A PowerShell that never answers a parse question until it is let go**, for
    /// [`inject4_startup_preasks_power_shell_rows_without_a_birth_waiting`]: it says which
    /// program it was asked about, then holds until the test lets it go — or until
    /// [`NEVER_ANSWERS_CEILING`], which only a door or a birth that waits for it reaches, and
    /// which it records so that such a run is red rather than slow.
    struct NeverAnswers {
        state: Mutex<NeverAnswersState>,
        changed: std::sync::Condvar,
    }

    #[derive(Default)]
    struct NeverAnswersState {
        arrived: Vec<PathBuf>,
        let_go: bool,
        ceiling_reached: bool,
    }

    /// Far above any honest cost of a worker reaching its first question; a working run never
    /// waits for it.
    const NEVER_ANSWERS_CEILING: std::time::Duration = std::time::Duration::from_secs(120);

    static NEVER_ANSWERS: NeverAnswers = NeverAnswers {
        state: Mutex::new(NeverAnswersState {
            arrived: Vec::new(),
            let_go: false,
            ceiling_reached: false,
        }),
        changed: std::sync::Condvar::new(),
    };

    impl NeverAnswers {
        fn state(&self) -> std::sync::MutexGuard<'_, NeverAnswersState> {
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }

        /// The [`ParseProbe`] stand-in.
        fn probe(program: &Path, _text: &str) -> Result<bool, ParseProbeFailure> {
            let mut state = NEVER_ANSWERS.state();
            state.arrived.push(program.to_path_buf());
            NEVER_ANSWERS.changed.notify_all();
            let (mut state, waited) = NEVER_ANSWERS
                .changed
                .wait_timeout_while(state, NEVER_ANSWERS_CEILING, |state| !state.let_go)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.ceiling_reached |= waited.timed_out();
            Ok(true)
        }

        /// Until the probe has been asked about `program`, or the ceiling.
        fn arrival_of(&self, program: &Path) -> bool {
            let (state, _) = self
                .changed
                .wait_timeout_while(self.state(), NEVER_ANSWERS_CEILING, |state| {
                    !state.arrived.iter().any(|arrived| arrived == program)
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.arrived.iter().any(|arrived| arrived == program)
        }

        fn let_go(&self) -> bool {
            let mut state = self.state();
            state.let_go = true;
            self.changed.notify_all();
            !state.ceiling_reached
        }
    }

    /// RED (T-TEST-HYGIENE-048; it read `main.rs`, this file and `pty_door.rs` before) —
    /// **the startup door asks a command-bearing PowerShell row's parse question on a worker it
    /// does not wait for, and a birth of that row while the answer has not arrived starts the row
    /// as written without waiting either.**
    ///
    /// Through `begin_powershell_preparation_over` — `begin_powershell_preparation_for` with the
    /// rows and the two machine effects handed in — so the thread door, the claim, the cache and
    /// the birth's composer are the product's. The row is a real file named `pwsh.exe` carrying
    /// `-NoExit -Command` text with CJK in it; the probe is [`NeverAnswers`] and the script is
    /// none. The door returns, the question is seen arriving at the probe, the answer is still
    /// not heard, the birth composes the row's own words, and only then is the probe let go.
    ///
    /// MUTATIONS: `.join()` the worker in `spawn_powershell_preparation` — the door returns only
    /// at the ceiling, with the answer already heard; drop the questions in
    /// `begin_powershell_preparation_over` — the probe is never asked.
    #[test]
    fn inject4_startup_preasks_power_shell_rows_without_a_birth_waiting() {
        let root = bt_testpath::temp_path("folio-inject4-preask");
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("pwsh.exe");
        std::fs::write(&program, b"").unwrap();
        // A program this machine could start: on Unix that is a file with an execute bit.
        #[cfg(unix)]
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        // The Windows seed's PowerShell 7 row as the template on every platform: the row is
        // data, and what is under test is the door and the composer, not this machine's seed.
        let template = profiles::shipped_for(
            profiles::SeedPlatform::Windows,
            &bt_pty::SystemShellEnvironment,
        )
        .into_iter()
        .find(|profile| profile.id == "pwsh")
        .expect("the Windows seed ships a PowerShell 7 row");
        let theirs = Profile {
            id: "inject4-preask".to_owned(),
            program: ProgramSource::Path(program.clone()),
            args: vec![
                "-NoExit".to_owned(),
                "-Command".to_owned(),
                "Write-Host '你好, 世界'".to_owned(),
            ],
            ..template
        };
        let programs = profiles::ProfilePrograms::probe_rows(
            std::slice::from_ref(&theirs),
            &bt_pty::SystemShellEnvironment,
        );
        assert_eq!(
            programs.program(&theirs.id),
            Some(program.as_os_str()),
            "the row resolves to its own file"
        );
        let arguments: Vec<OsString> = profiles::launch_args(&theirs)
            .into_iter()
            .map(OsString::from)
            .collect();

        fn no_script() -> Option<PathBuf> {
            None
        }
        begin_powershell_preparation_over(
            std::slice::from_ref(&theirs),
            &programs,
            PreparationEffects {
                script: no_script,
                parse: NeverAnswers::probe,
            },
        );
        assert!(
            NEVER_ANSWERS.arrival_of(&program),
            "the startup door asked the row's question"
        );
        assert_eq!(
            heard_parse(&program, &arguments),
            Heard::NotYet,
            "the door returned before the answer"
        );
        assert_eq!(
            compose_powershell_birth(&program, &arguments, true),
            arguments,
            "a birth before the answer starts the row as written"
        );
        assert!(
            NEVER_ANSWERS.let_go(),
            "the probe was let go by this test, not by its ceiling"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// PIN — **what a pane is told it is, is one answer for every platform**
    /// (M1-5, plan §2 M1).
    ///
    /// `TERM`, `COLORTERM`, `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` are the
    /// four declarations that tell a child what it is running inside, and the
    /// port does not get to have an opinion about them: a zsh on a Mac must read
    /// `TERM=xterm-256color` and `TERM_PROGRAM=Folio` exactly as a PowerShell on
    /// Windows does, because every program that consults them — `less`, `vim`,
    /// `git`, every CLI that gates hyperlinks — is the same program on both.
    ///
    /// They already were platform-free, and that is what this pins: they live in
    /// one function of `bt_pty::PtyCommand` with no `cfg` in it, and neither this
    /// module nor the profile table adds a fifth answer beside them. What *is*
    /// platform-specific is one layer below and must be: matching an existing
    /// name is case-insensitive on Windows and exact off it, which is a fact
    /// about environment blocks rather than about what is declared.
    ///
    /// RED GATE: give the macOS spawn its own `TERM` — the shape this port would
    /// take if somebody "fixed" a terminfo complaint at the platform arm — and
    /// the second half fails naming the variable.
    #[test]
    fn the_term_variables_a_pane_is_given_are_the_same_on_every_platform() {
        const DECLARED: [&str; 4] = ["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION"];
        // One function, in the crate that spawns, naming all four and no platform.
        let region = crate::source_pin::code_of(crate::source_pin::source_region(
            include_str!("../../bt-pty/src/lib.rs"),
            "fn resolved_environment_layers(&self)",
        ));
        for name in DECLARED {
            assert!(
                region.contains(&format!("\"{name}\"")),
                "`resolved_environment_layers` no longer declares {name}"
            );
        }
        assert!(
            !region.contains("cfg"),
            "the four declarations took a platform arm: {region}"
        );
        assert_eq!(bt_pty::TERM_PROGRAM, "Folio");
        // And this module leaves them alone: the floor profile is a row every
        // platform has, and what it is handed here names none of the four.
        let command = shell_command(
            &row(profiles::fallback_profile_id()),
            &[],
            Scripts::default(),
            &bare(),
        );
        for (name, _) in &command.environment {
            for declared in DECLARED {
                assert!(
                    !environment_name_eq(name, OsStr::new(declared)),
                    "the spawn path declared {declared} a second time, beside `bt-pty`'s"
                );
            }
        }
    }

    /// PIN — **the system's locale is declared for a pane that would otherwise
    /// have none, and only then** (M1-5, plan §8 Q8; T-MAC-LOCALE).
    ///
    /// A Finder-launched app inherits `launchd`'s environment, which sets no
    /// `LC_*`, and a shell started in it runs in the `C` locale where a UTF-8
    /// filename lists as question marks and `天下为公` comes back out of the line
    /// editor a character at a time. Off macOS
    /// `bt_platform::system_locale_declaration` answers `None` and this declares
    /// nothing at all, which is what keeps the Windows spawn byte for byte what
    /// it was.
    ///
    /// **Both shapes are here**, because the suppression rule is about the
    /// question rather than about the variable: a machine whose own locale is
    /// installed is declared in `LANG`, one whose is not gets Apple's pair
    /// (`LANG=C.UTF-8`, `LC_CTYPE=UTF-8`), and *any* of the three locale
    /// variables already answered silences either of them whole. Declaring half
    /// of a pair under an inherited `LC_CTYPE` would be the worst of both: a
    /// `LANG` that the inherited variable outranks for the one category a pane
    /// cares about.
    ///
    /// RED GATE: declare it unconditionally and the `LC_ALL` case puts a `LANG`
    /// under a variable that outranks it — one in the record that changes
    /// nothing — while the profile-row case overrules a reader who answered the
    /// question themselves.
    #[test]
    fn a_pane_with_no_locale_at_all_is_told_the_systems_one() {
        let none: &[(String, String)] = &[];
        let system = LocaleDeclaration::lang("ja_JP.UTF-8");
        let fallback = LocaleDeclaration::apple_fallback(true, true)
            .expect("a machine with both halves declares both");
        let said = |declaration: Vec<(OsString, OsString)>| {
            declaration
                .into_iter()
                .map(|(name, value)| {
                    (
                        name.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            said(locale_declaration(Some(&system), &bare(), none)),
            [("LANG".to_owned(), "ja_JP.UTF-8".to_owned())],
            "nothing had answered, so the system's own setting does"
        );
        assert_eq!(
            said(locale_declaration(Some(&fallback), &bare(), none)),
            [
                ("LANG".to_owned(), "C.UTF-8".to_owned()),
                ("LC_CTYPE".to_owned(), "UTF-8".to_owned())
            ],
            "and where the system names no installed locale, Apple's own pair"
        );
        assert!(
            locale_declaration(None, &bare(), none).is_empty(),
            "a platform with no such setting declares nothing rather than a guess"
        );
        for declaration in [&system, &fallback] {
            for answered in ["LANG", "LC_ALL", "LC_CTYPE"] {
                assert!(
                    locale_declaration(
                        Some(declaration),
                        &Env(vec![(answered, "de_DE.UTF-8")]),
                        none
                    )
                    .is_empty(),
                    "{answered} had answered the question already"
                );
                assert!(
                    locale_declaration(
                        Some(declaration),
                        &bare(),
                        &[(answered.to_owned(), "de_DE.UTF-8".to_owned())]
                    )
                    .is_empty(),
                    "and a {answered} row of the reader's own is them answering outright"
                );
            }
        }
        // An inherited name with an empty value is the platform taking the
        // variable away rather than setting it — see `layer_profile_environment`
        // on what an empty value does to a child's block.
        assert_eq!(
            said(locale_declaration(
                Some(&system),
                &Env(vec![("LANG", "")]),
                none
            )),
            [("LANG".to_owned(), "ja_JP.UTF-8".to_owned())]
        );
    }

    /// PIN — Git Bash is handed the init file *instead of* `--login`.
    ///
    /// Red gate: appending `--init-file` to the profile's own `--login -i`
    /// produces a command line bash accepts and a shell that never reads the
    /// script, because bash consults the init file only for a non-login shell.
    /// Nothing about that is visible — the shell starts, the prompt is right,
    /// and no marker ever arrives.
    #[test]
    fn git_bash_trades_its_login_flag_for_the_init_file() {
        let script = Path::new(r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\folio.bash");
        let command = shell_command(&row("gitbash"), &[], bash_only(script), &bare());
        assert_eq!(
            args(&command),
            [
                "--init-file",
                r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\folio.bash",
                "-i"
            ]
        );
        assert!(
            !args(&command).iter().any(|argument| argument == "--login"),
            "`--login` and `--init-file` cannot both be honoured, so only one is passed"
        );
        assert!(
            command.environment.contains(&(
                OsString::from("BT_SHELL_INTEGRATION"),
                OsString::from(MODE_LOGIN)
            )),
            "the marker is what makes the script run the chain it displaced, and this row's own              arguments say which chain that is"
        );
        // No script on this machine, and Git Bash is the shell it always was.
        assert_eq!(
            args(&shell_command(
                &row("gitbash"),
                &[],
                Scripts::default(),
                &bare()
            )),
            ["--login", "-i"]
        );
    }

    /// RED — **the first WSL pane of a run carries the init file, because there
    /// is only one shape of WSL command line and it carries it.**
    ///
    /// The 2026-09-07 fix, stated as the property that was false before it
    /// (`docs/plans/shell-matrix-2026-09-07.md` T-2). The first WSL pane of
    /// every process was started as a bare `wsl.exe --cd <dir>`: the login-shell
    /// probe was armed *from* that spawn and never waited for, so
    /// `integrated_login_shell()` answered `None` and the command line went out
    /// without `--init-file`. Its dump contained no `OSC 133` and no `OSC 7` at
    /// all, and on a machine whose default profile is WSL that first pane is the
    /// only one there is.
    ///
    /// What makes it fixed is not a faster probe: it is that **the spawn asks
    /// nothing whose answer could be outstanding.** The question travels *in*
    /// the command line, and the distribution answers it about itself. So the
    /// assertion is that two spawns in a row are the same eight arguments, and
    /// that the first of them names the script.
    ///
    /// MUTATIONS:
    /// ① go back to `wsl.integrated_login_shell()` gating the branch and the
    ///    first call is `["--cd", "/mnt/d/Developer"]` — two arguments, no
    ///    script;
    /// ② splice the script into the text of the question instead of passing it
    ///    as `$1` and a reader whose account name has a space in it gets a
    ///    filename this shell re-parses into two words.
    ///
    /// The other half of the red gate is unchanged and older: passing the
    /// *Windows* path of the script to `wsl.exe` gives the distribution a
    /// filename with a drive letter and backslashes, which it cannot open — so
    /// `--init-file` names nothing, bash starts with no startup file at all, and
    /// the user loses their own `~/.bashrc` as well as our markers.
    #[test]
    fn the_first_wsl_pane_is_told_the_place_the_question_and_the_script_in_wsls_own_spelling() {
        let script = Path::new(r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\folio.bash");
        let place = [OsString::from("--cd"), OsString::from("/mnt/d/Developer")];
        let expected = [
            "--cd",
            "/mnt/d/Developer",
            "-e",
            "sh",
            "-c",
            WSL_LOGIN_SHELL,
            "folio",
            "/mnt/c/Users/dev/AppData/Roaming/Folio/shell-integration/folio.bash",
            "/mnt/c/Users/dev/AppData/Roaming/Folio/shell-integration/zdotdir",
        ];
        let zdotdir = Path::new(r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\zdotdir");
        let doors = Scripts {
            bash: Some(script),
            zdotdir: Some(zdotdir),
        };
        let first = shell_command(&row("wsl"), &place, doors, &bare());
        assert_eq!(
            args(&first),
            expected,
            "`--cd` is the launcher's own flag and must precede the command it is to run, and \
             the script is named in the spelling the distribution can open"
        );
        // **`-e` and not `--`.** `wsl.exe --` joins what follows into one
        // command line and hands it to the login shell, which re-parses it —
        // and the question below is one argument full of spaces, quotes, `$`,
        // `|` and `;`. `-e` executes argv for argv. Measured on Ubuntu-24.04,
        // 2026-09-07: under `--` the question arrived as fragments and `$1` was
        // empty; under `-e`, `$0` is `folio` and `$1` is the init file with the
        // space in the account name intact.
        assert!(
            !args(&first).contains(&"--".to_owned()),
            "the launcher must execute the question, not hand it to a shell to re-read"
        );
        // The whole of the defect, said as a sentence: the second pane of a run
        // used to be the first one that worked.
        let second = shell_command(&row("wsl"), &place, doors, &bare());
        assert_eq!(
            args(&second),
            args(&first),
            "no WSL pane of a run is composed from an answer an earlier pane's probe brought \
             back, because there is no probe and no answer"
        );
        // The script travels as `$1`, never spliced into the text: a path is
        // data, and a shell that re-read it would split an account name with a
        // space in it into two words.
        assert!(
            !WSL_LOGIN_SHELL.contains("/mnt/"),
            "the question names no path of its own: {WSL_LOGIN_SHELL}"
        );
        assert!(
            value_of(&first, "WSLENV").is_some_and(|listed| listed.contains("TERM_PROGRAM/u")),
            "a variable that is not listed in WSLENV does not cross into the distribution"
        );
        assert_eq!(
            value_of(&first, INSTALLED_MARKER),
            None,
            "`BT_SHELL_INTEGRATION` is set inside the distribution, by the one branch that \
             reads the init file — a zsh session carrying it would tell every nested bash that \
             its startup files had already been run"
        );
    }

    /// RED — **the question the pane puts to its own distribution keeps both
    /// branches the probe had.**
    ///
    /// A shape test rather than a round trip, and the round trip is
    /// `crates/bt-pty/tests/shell_integration_wsl.rs`, which runs this exact
    /// string through a real POSIX `sh` against a password database it wrote.
    /// What is checked here is that the two branches are still *there*, because
    /// the failure they guard is silent in each direction: without the `bash`
    /// branch every WSL pane is back to no markers, and without the default one
    /// a zsh reader has their shell replaced by bash every time they open a tab
    /// and the symptom — "my prompt is gone" — names neither this terminal nor
    /// the flag that did it.
    #[test]
    fn the_question_hands_bash_the_init_file_and_leaves_every_other_shell_alone() {
        // The password database is the source a login would read. `$SHELL` is
        // not: this is not a login shell, so it is either unset or inherited
        // from the Win32 side of the boundary.
        assert!(WSL_LOGIN_SHELL.contains("getent passwd"));
        assert!(!WSL_LOGIN_SHELL.contains("$SHELL"));
        // bash, by the name it is invoked under rather than by where it lives:
        // `/bin/bash` and `/usr/bin/bash` are one shell on two machines.
        assert!(WSL_LOGIN_SHELL.contains(r#"case "${shell##*/}" in"#));
        assert!(WSL_LOGIN_SHELL.contains(r#"BT_SHELL_INTEGRATION=login"#));
        assert!(WSL_LOGIN_SHELL.contains(r#"exec "$shell" --init-file "$1" -i"#));
        // zsh by its own door, because it has no `--init-file` and refuses the
        // flag: `ZDOTDIR`, with the reader's own carried beside it, and still
        // the login shell `wsl.exe` would have started.
        assert!(WSL_LOGIN_SHELL.contains(r#"zsh) "#));
        assert!(WSL_LOGIN_SHELL.contains(r#"ZDOTDIR="$2""#));
        assert!(WSL_LOGIN_SHELL.contains("BT_USER_ZDOTDIR"));
        // Everything else keeps its own shell, started as the login shell
        // `wsl.exe` would have started.
        assert!(WSL_LOGIN_SHELL.contains(r#"*) exec "$shell" -l"#));
        // And a distribution that cannot say gets the one shell POSIX requires,
        // rather than `exec ""`.
        assert!(WSL_LOGIN_SHELL.contains(r#"[ -n "$shell" ] || shell=/bin/sh"#));
        // One argument on the far side of `wsl.exe`, so no newline may creep in.
        assert!(!WSL_LOGIN_SHELL.contains('\n'));
    }

    /// PIN — a script this machine keeps where WSL cannot name it is not handed
    /// over at all.
    ///
    /// `%APPDATA%` on a network share, or on a substituted drive: the launcher
    /// is told only where to stand, which is what every WSL pane did before
    /// there was a script to hand over. Injecting a path the distribution cannot
    /// open is strictly worse than not injecting.
    #[test]
    fn a_script_wsl_cannot_name_is_not_handed_to_it() {
        let place = [OsString::from("--cd"), OsString::from("/mnt/d/Developer")];
        for unreachable in [r"\\nas\home\Folio\folio.bash", "relative\\folio.bash"] {
            let command = shell_command(
                &row("wsl"),
                &place,
                Scripts {
                    bash: Some(Path::new(unreachable)),
                    zdotdir: Some(Path::new(unreachable)),
                },
                &bare(),
            );
            assert_eq!(
                args(&command),
                ["--cd", "/mnt/d/Developer"],
                "{unreachable}"
            );
        }
    }

    /// The bash half alone, which is what most of these cases are about.
    fn bash_only(script: &Path) -> Scripts<'_> {
        Scripts {
            bash: Some(script),
            zdotdir: None,
        }
    }

    /// Both halves, spelled the way the two installers spell them.
    fn both(bash: &'static str, zdotdir: &'static str) -> Scripts<'static> {
        Scripts {
            bash: Some(Path::new(bash)),
            zdotdir: Some(Path::new(zdotdir)),
        }
    }

    /// PIN — **a profile's own arguments reach the bash it names** (review row
    /// R3-7).
    ///
    /// This arm used to build the whole command line out of three literals, so a
    /// row that carried anything of the reader's — `--noediting`, `-O globstar`,
    /// a `--rcfile` of their own — lost it the moment the row had integration.
    /// The profile said one thing and the spawn did another, and the only
    /// symptom was a shell that did not behave the way the row described.
    ///
    /// What *is* dropped is the login flag, and only because bash will not read
    /// an init file for a login shell — the chain it stood for is put back by the
    /// script, which is told which one it owes.
    ///
    /// MUTATION: build the list from literals again and every word a reader
    /// added to a Git Bash row is silently thrown away.
    #[test]
    fn a_bash_given_an_init_file_still_gets_the_words_its_profile_carries() {
        let theirs = Profile {
            args: vec![
                "--login".to_owned(),
                "-i".to_owned(),
                "--noediting".to_owned(),
            ],
            ..row("gitbash")
        };
        let command = shell_command(
            &theirs,
            &[OsString::from("--rcfile-is-not-here")],
            bash_only(Path::new(r"C:\Folio\folio.bash")),
            &bare(),
        );
        assert_eq!(
            args(&command),
            [
                "--init-file",
                r"C:\Folio\folio.bash",
                "-i",
                "--noediting",
                "--rcfile-is-not-here"
            ],
            "the reader's words are kept and only the login flag is dropped"
        );
        // A cluster is one argument carrying several flags, and only the `l`
        // comes out of it.
        let clustered = Profile {
            args: vec!["-li".to_owned()],
            ..row("gitbash")
        };
        let command = shell_command(
            &clustered,
            &[],
            bash_only(Path::new(r"C:\Folio\folio.bash")),
            &bare(),
        );
        assert_eq!(
            args(&command),
            ["--init-file", r"C:\Folio\folio.bash", "-i"]
        );
        // And a row that never said `-i` still gets one: `-i` is what makes bash
        // read the init file at all.
        let bare_row = Profile {
            args: Vec::new(),
            ..row("gitbash")
        };
        let command = shell_command(
            &bare_row,
            &[],
            bash_only(Path::new(r"C:\Folio\folio.bash")),
            &bare(),
        );
        assert_eq!(
            args(&command),
            ["--init-file", r"C:\Folio\folio.bash", "-i"]
        );
    }

    /// PIN — **the startup chain a pane is owed is the one its profile asked
    /// for** (review row R3-8).
    ///
    /// bash has two chains and reads exactly one of them, and which is a fact
    /// about the profile: a row carrying `--login` wants `/etc/profile` and the
    /// first of the three profile files, and a row that does not wants
    /// `~/.bashrc` and nothing else. The script emulates whichever it is told,
    /// and this side is the only half that can read the row.
    ///
    /// MUTATION: send `1` again and every pane emulates a login shell, so a
    /// reader who kept their aliases in `~/.bashrc` — which is where bash's own
    /// documentation puts them — has none of them.
    #[test]
    fn the_startup_chain_a_pane_emulates_is_the_one_its_profile_asked_for() {
        let login = shell_command(
            &row("gitbash"),
            &[],
            bash_only(Path::new(r"C:\Folio\folio.bash")),
            &bare(),
        );
        assert_eq!(
            value_of(&login, INSTALLED_MARKER).as_deref(),
            Some(MODE_LOGIN),
            "the shipped Git Bash row says `--login`"
        );
        // A row switched off its login shell (0.4.6 ticket 74: the switch, and no
        // login word left in its arguments).
        let plain = Profile {
            args: vec!["-i".to_owned()],
            login: false,
            ..row("gitbash")
        };
        let plain = shell_command(
            &plain,
            &[],
            bash_only(Path::new(r"C:\Folio\folio.bash")),
            &bare(),
        );
        assert_eq!(
            value_of(&plain, INSTALLED_MARKER).as_deref(),
            Some(MODE_INTERACTIVE)
        );
        // The script reads both words, and neither of them is a bare `1`.
        let source = script_source();
        assert!(source.contains(MODE_INTERACTIVE), "{MODE_INTERACTIVE}");
        assert!(
            source.contains("$HOME/.bashrc"),
            "the interactive chain is the one file bash reads"
        );
        assert!(
            source.contains("/etc/profile") && source.contains("$HOME/.bash_login"),
            "and the login chain is still all of bash's own order"
        );
    }

    /// PIN — **zsh is pointed at a directory and never handed bash's flag**
    /// (review row R3-6).
    ///
    /// `--init-file` is bash's, and zsh refuses it: a `zsh` row used to be sent
    /// through the bash door, so the shell either would not start or started
    /// with the flag treated as a file of its own. zsh's door is `ZDOTDIR`, and
    /// nothing is added to the command line at all — the profile's own words go
    /// through untouched.
    ///
    /// MUTATION: map `zsh` back to the init file and every zsh pane is handed an
    /// argument its shell does not take.
    #[test]
    fn zsh_is_pointed_at_a_directory_and_never_handed_bashs_flag() {
        assert_eq!(
            profiles::derive_integration(&ProgramSource::Path(PathBuf::from("/usr/bin/zsh"))),
            Integration::ZshDotDir
        );
        let theirs = Profile {
            program: ProgramSource::Path(PathBuf::from(r"C:\msys64\usr\bin\zsh.exe")),
            args: vec!["-l".to_owned()],
            paths: profiles::PathNamespace::Windows,
            ..row("gitbash")
        };
        let command = shell_command(
            &theirs,
            &[],
            both(r"C:\Folio\folio.bash", r"C:\Folio\zdotdir"),
            &bare(),
        );
        assert_eq!(args(&command), ["-l"], "zsh's door adds no argument");
        assert_eq!(
            value_of(&command, "ZDOTDIR").as_deref(),
            Some(r"C:\Folio\zdotdir")
        );
        assert_eq!(value_of(&command, "BT_USER_ZDOTDIR"), None);
        // A reader who keeps their own startup files somewhere else has said so
        // in that variable, and the script needs it to put them back.
        let inherited = shell_command(
            &theirs,
            &[],
            both(r"C:\Folio\folio.bash", r"C:\Folio\zdotdir"),
            &Env(vec![("ZDOTDIR", r"D:\dotfiles\zsh")]),
        );
        assert_eq!(
            value_of(&inherited, "BT_USER_ZDOTDIR").as_deref(),
            Some(r"D:\dotfiles\zsh")
        );
        // And the script it will read is the one that puts them back.
        let script = script_source_zsh();
        assert!(script.contains("BT_USER_ZDOTDIR"));
        for name in ZDOTDIR_FILES {
            assert!(script.contains(name), "{name} is one of the three");
        }
    }

    /// RED (74) — **the three shipped macOS rows are handed their login flag
    /// through each one's own door**: `-l` beside zsh's `ZDOTDIR`, bash's
    /// `--login` traded for the init file and the login chain it owes, `-l` to a
    /// `sh` that has no door.
    ///
    /// The portable half of `a_login_row_starts_a_login_shell`, asserted on every
    /// runner: what the spawn is told, for the rows a Mac ships.
    ///
    /// MUTATION: build `own` from `profile.args` again in `shell_command` — the
    /// zsh and sh rows lose their `-l` and bash is told `interactive`.
    #[test]
    fn the_shipped_macos_rows_are_handed_their_login_flag_through_their_own_door() {
        struct Mac;
        impl ShellEnvironment for Mac {
            fn var_os(&self, key: &str) -> Option<OsString> {
                (key == "SHELL").then(|| OsString::from("/bin/zsh"))
            }
            fn is_file(&self, path: &Path) -> bool {
                ["/bin/zsh", "/bin/bash", "/bin/sh"]
                    .iter()
                    .any(|shell| path == Path::new(shell))
            }
        }
        let rows = profiles::shipped_for(profiles::SeedPlatform::MacOs, &Mac);
        let scripts = both("/F/folio.bash", "/F/zdotdir");
        let zsh = shell_command(&rows[0], &[], scripts, &bare());
        assert_eq!(args(&zsh), ["-l"]);
        assert_eq!(value_of(&zsh, "ZDOTDIR").as_deref(), Some("/F/zdotdir"));
        let bash = shell_command(&rows[1], &[], scripts, &bare());
        assert_eq!(args(&bash), ["--init-file", "/F/folio.bash", "-i"]);
        assert_eq!(
            value_of(&bash, INSTALLED_MARKER).as_deref(),
            Some(MODE_LOGIN),
            "the script replays the login chain bash will not run beside an init file"
        );
        let sh = shell_command(&rows[2], &[], scripts, &bare());
        assert_eq!(args(&sh), ["-l"]);
    }

    /// RED (74) — **a login row starts a login shell: `ps` shows it, and the
    /// `.zprofile` Homebrew writes its `PATH` into reaches the pane** (issue #12).
    ///
    /// The real producer end to end: this build's macOS seed, through
    /// [`shell_command`] with the real scripts written into a sandbox, spawned
    /// over a real pty, and asked by the shell itself. The reader's home is a
    /// `bt_pty::test_shell::Hygiene` directory under `std::env::temp_dir()` —
    /// `TMPDIR` decides where, so a run on a shared machine points it inside its
    /// own work tree — holding a `.zprofile`, a `.bash_profile` and a `.profile`
    /// that each put a marker at the front of `PATH`. No file of the account
    /// running the test is read or written: `HOME`, `HISTFILE` and `XDG_*` are the
    /// sandbox's and `ZDOTDIR` is Folio's copy in it.
    ///
    /// `ps -ww -o command= -p $$` is the reporter's own question, widened so a
    /// long init-file path is not cut at the terminal's width. zsh and sh answer
    /// with their `-l`; bash answers with the init file, because its login chain
    /// is replayed by `folio.bash` rather than by `--login`
    /// (`docs/shell-integration.md`), and the marker is how that is seen.
    ///
    /// MUTATION: in `unix_shipped`, hand the system rows `false` — zsh prints
    /// `argv=/bin/zsh` and no marker.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_login_row_starts_a_login_shell() {
        use std::time::{Duration, Instant};

        struct Mac;
        impl ShellEnvironment for Mac {
            fn var_os(&self, key: &str) -> Option<OsString> {
                (key == "SHELL").then(|| OsString::from("/bin/zsh"))
            }
            fn is_file(&self, path: &Path) -> bool {
                bt_pty::SystemShellEnvironment.is_file(path)
            }
        }

        const MARKER: &str = "/folio-login-marker-74";
        // One `bt_pty::test_shell::Hygiene` per shell: its own temporary `HOME`, `ZDOTDIR`,
        // `HISTFILE` and `XDG_*`, holding the startup files this test writes — the files this
        // test is about, so the shell is not told to skip them (`reading_startup_files`).
        let sandbox = || {
            let hygiene = bt_pty::test_shell::Hygiene::new().reading_startup_files();
            let home = hygiene.home();
            let zdotdir = hygiene.root().join("zdotdir");
            std::fs::create_dir_all(&zdotdir).unwrap();
            for name in [".zprofile", ".bash_profile", ".profile"] {
                std::fs::write(home.join(name), format!("export PATH=\"{MARKER}:$PATH\"\n"))
                    .unwrap();
            }
            for name in ZDOTDIR_FILES {
                std::fs::write(zdotdir.join(name), SCRIPT_ZSH).unwrap();
            }
            let script = hygiene.root().join(SCRIPT_FILE);
            std::fs::write(&script, SCRIPT).unwrap();
            (hygiene, home, zdotdir, script)
        };

        let rows = profiles::shipped_for(profiles::SeedPlatform::MacOs, &Mac);
        let expected = [
            ("zsh", "argv=/bin/zsh -l"),
            ("bash", "argv=/bin/bash --init-file "),
            ("sh", "argv=/bin/sh -l"),
        ];
        for (row, (id, argv)) in rows.iter().zip(expected) {
            assert_eq!(row.id, id);
            let profiles::ProgramSource::Path(program) = &row.program else {
                panic!("{id} names one path");
            };
            let (hygiene, home, zdotdir, script) = sandbox();
            let scripts = Scripts {
                bash: Some(&script),
                zdotdir: Some(&zdotdir),
            };
            let mut command = shell_command(row, &[], scripts, &bare());
            command
                .environment
                .push((OsString::from("HOME"), home.clone().into_os_string()));
            let mut session = bt_pty::test_shell::TestShell::spawn_shell_in(
                hygiene,
                program.clone(),
                &command.arguments,
                &|| last_resort_arguments(false),
                &command.environment,
                bt_pty::PtySize::cells(
                    std::num::NonZeroU16::new(200).unwrap(),
                    std::num::NonZeroU16::new(24).unwrap(),
                ),
                Some(home.clone()),
            )
            .unwrap();
            session
                .write(
                    b"printf 'argv=%s\\n' \"$(ps -ww -o command= -p $$)\"; \
                      printf 'path=%s\\n' \"$PATH\"; echo folio-done-$((6*7))\n",
                )
                .unwrap();
            let mut seen = String::new();
            let deadline = Instant::now() + Duration::from_secs(20);
            while !seen.contains("folio-done-42") {
                assert!(
                    Instant::now() < deadline,
                    "{id} never answered; it printed {seen:?}"
                );
                let chunk = session.read_output();
                if chunk.is_empty() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                seen.push_str(&String::from_utf8_lossy(&chunk));
            }
            assert!(seen.contains(argv), "{id}: expected {argv:?} in {seen:?}");
            // The shell's own answer, for a reader running this with `--nocapture`.
            for line in seen
                .lines()
                .filter_map(|line| line.find("argv=/").map(|at| &line[at..]))
            {
                eprintln!("{id}: {}", line.trim_end());
            }
            assert!(
                seen.contains(&format!("path={MARKER}:")),
                "{id}: the login file's PATH reached the pane: {seen:?}"
            );
            session.shutdown().unwrap();
        }
    }

    /// PIN — **a Bourne shell is told it has no integration rather than handed
    /// one it will ignore** (review row R3-6).
    ///
    /// `sh` accepts `--init-file` and does nothing with it — no error, no
    /// marker, no directory — which is the one failure that looks exactly like a
    /// shell with no integration at all. Now it *is* one, and the pane's own
    /// capability sentence says so.
    ///
    /// MUTATION: map `sh` back to the init file and the pane claims a capability
    /// it silently does not have.
    #[test]
    fn a_bourne_shell_is_told_it_has_no_integration_rather_than_handed_one() {
        for program in ["/bin/sh", "/usr/bin/dash", r"C:\msys64\usr\bin\sh.exe"] {
            assert_eq!(
                profiles::derive_integration(&ProgramSource::Path(PathBuf::from(program))),
                Integration::None,
                "{program}"
            );
        }
        let theirs = Profile {
            program: ProgramSource::Path(PathBuf::from("/bin/sh")),
            args: vec!["-i".to_owned()],
            login: false,
            ..row("gitbash")
        };
        let command = shell_command(
            &theirs,
            &[],
            both(r"C:\Folio\folio.bash", r"C:\Folio\zdotdir"),
            &bare(),
        );
        assert_eq!(args(&command), ["-i"]);
        assert_eq!(value_of(&command, INSTALLED_MARKER), None);
        assert_eq!(value_of(&command, "ZDOTDIR"), None);
        assert_eq!(
            profiles::capability_of_parts(profiles::served_by(&theirs), theirs.paths, true, false),
            crate::i18n::Text::CapNone
        );
    }

    /// PIN — **the question asked inside the distribution names both doors.**
    ///
    /// Which shell a `wsl.exe` logs the reader into is a fact about a Linux
    /// password database, so the answer is reached in there — and each branch
    /// needs a different thing: bash needs the init file, zsh needs the
    /// directory. Both travel as arguments, so a path with a space in it arrives
    /// whole.
    ///
    /// MUTATION: hand the zsh branch nothing and a WSL login that lands in zsh
    /// is back to a pane with no marks and no directory.
    #[test]
    fn the_question_asked_inside_the_distribution_names_both_doors() {
        let place = [OsString::from("--cd"), OsString::from("/mnt/d/Developer")];
        let command = shell_command(
            &row("wsl"),
            &place,
            both(r"C:\Folio\folio.bash", r"C:\Folio\zdotdir"),
            &bare(),
        );
        let words = args(&command);
        assert_eq!(
            words.last().map(String::as_str),
            windows_to_wsl(Path::new(r"C:\Folio\zdotdir"))
                .as_deref()
                .and_then(Path::to_str),
            "the directory is the last argument, which is `$2`"
        );
        assert!(words.contains(&WSL_ARGV0.to_owned()));
        assert!(WSL_LOGIN_SHELL.contains("zsh)"), "{WSL_LOGIN_SHELL}");
        assert!(WSL_LOGIN_SHELL.contains("ZDOTDIR=\"$2\""));
        assert!(
            WSL_LOGIN_SHELL.contains("BT_SHELL_INTEGRATION=login"),
            "a wsl.exe starts a login shell, and the script owes that chain"
        );
    }

    /// PIN — **the line written into a PowerShell profile survives the path it
    /// names** (review row R3-14).
    ///
    /// A double-quoted PowerShell string is interpolated, and `$` and a backtick
    /// are both legal in a Windows directory name — so the fallback line, the one
    /// written when the script is not under `%APPDATA%`, used to hand the shell
    /// something other than the path. A single-quoted string is not interpolated
    /// at all.
    ///
    /// MUTATION: quote the literal path with `"` again and a reader whose folder
    /// is called `C:\$dev` dot-sources a path that is missing a component.
    #[test]
    fn the_profile_line_survives_a_path_powershell_would_have_read() {
        // The current writer has one form even if an old installation used a
        // literal operand. Literal historical forms require a known script path.
        for awkward in [
            r"C:\$dev\Folio\folio.ps1",
            "C:\\dev`n\\Folio\\folio.ps1",
            r"C:\it's here\folio.ps1",
        ] {
            assert_eq!(integration_line(), profile_marks::MANAGED_LINE);
            let forms = profile_marks::Forms::new(&[PathBuf::from(awkward)]);
            assert!(forms.owns(&format!(". '{}'", awkward.replace('\'', "''"))));
        }
    }

    /// PIN — PowerShell is not injected into, by any door.
    #[test]
    fn powershell_is_not_injected_into() {
        // **Both of them.** They are two profiles and one script: 5.1 and 7 read
        // the same `$PROFILE` mechanism, `folio.ps1` is written for
        // both, and neither is written into by this product.
        for id in ["pwsh", "winps"] {
            let profile = row(id);
            assert_eq!(
                profiles::served_by(&profile),
                Integration::PowerShellOptIn,
                "{id}: PowerShell's script is the user's to install"
            );
            let command = shell_command(
                &profile,
                &[],
                bash_only(Path::new(r"C:\script.bash")),
                &bare(),
            );
            assert_eq!(
                command.arguments,
                profile.args.iter().map(OsString::from).collect::<Vec<_>>(),
                "{id}"
            );
            assert!(command.environment.is_empty(), "{id}");
        }
    }

    /// PIN — Command Prompt marks where each of its commands begins and ends,
    /// reports where it is standing, and claims **neither** of the two markers
    /// that would open a region it can never close.
    ///
    /// Both halves are red gates, in opposite directions.
    ///
    /// * Dropping `133;D`/`133;A` is the state this profile shipped in until
    ///   2026-09-07: every other capability works and the command rail is
    ///   simply empty forever, in the one profile whose reader has no other way
    ///   to find the top of a command's output.
    /// * Adding `$e]133;B$e\` at the end of this string is the one-token edit
    ///   that looks like more capability and is less. `B` opens an input region
    ///   whose only closers are `C` and the *next* `A`, `PROMPT` is expanded
    ///   once — just before a line is read — so `C` has nowhere to be sent
    ///   from, and every command's own output would then spend its whole run
    ///   inside the region that means "this is what the reader is typing":
    ///   undecorated, and holding the resize path's `InvokePrompt` chord over a
    ///   shell with no such binding. See
    ///   [`profiles::Integration::CmdPrompt`].
    #[test]
    fn command_prompt_marks_its_command_boundaries_and_reports_its_directory() {
        let command = shell_command(
            &row("cmd"),
            &[],
            bash_only(Path::new(r"C:\script.bash")),
            &bare(),
        );
        assert!(
            command.arguments.is_empty(),
            "cmd takes no argument that would leave it interactive"
        );
        let prompt = prompt_of(&command);
        assert_eq!(
            prompt, r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$P$G",
            "the previous command's end, the report, this prompt's start, then the prompt cmd \
             would have printed on its own"
        );
        for absent in ["133;B", "133;C"] {
            assert!(
                !prompt.contains(absent),
                "a shell that cannot close a region must not open one: {prompt}"
            );
        }
        assert!(
            !prompt.contains("133;D;"),
            "`PROMPT` cannot read ERRORLEVEL, so `D` carries no status rather than a wrong one: \
             {prompt}"
        );
    }

    /// PIN — every shell this terminal starts is told that it renders
    /// hyperlinks, and none is told over the top of an answer already given.
    ///
    /// This is R-d settled (`docs/M2-persistence-schema-v1.md` §296-299), and
    /// the red gate is the *coverage*: with the declaration back inside
    /// `folio.ps1` where it used to live, `OSC 8` links worked in a
    /// PowerShell whose owner had installed the opt-in script and in no other
    /// pane in the window — a capability of the terminal reachable only through
    /// one profile's optional file.
    #[test]
    fn every_shell_is_told_this_terminal_renders_hyperlinks_unless_it_was_already_told() {
        let forced = |id: &str, environment: &dyn ShellEnvironment| {
            shell_command(
                &row(id),
                &[],
                bash_only(Path::new(r"C:\s.bash")),
                environment,
            )
            .environment
            .into_iter()
            .find(|(key, _)| key == "FORCE_HYPERLINK")
            .map(|(_, value)| value.to_string_lossy().into_owned())
        };
        for id in ["wsl", "gitbash", "cmd"] {
            assert_eq!(forced(id, &bare()).as_deref(), Some("1"), "{id}");
            // Any answer already in the environment is the user's, `0` very
            // much included: this is a declaration, not an override.
            for theirs in ["0", "1", ""] {
                assert_eq!(
                    forced(id, &Env(vec![("FORCE_HYPERLINK", theirs)])),
                    None,
                    "{id} must not overwrite an inherited {theirs:?}"
                );
            }
        }
        // PowerShell is the exception, and only because its own script is still
        // the half that says this — stating it twice would be two places to
        // change and one silently redundant.
        assert_eq!(forced("pwsh", &bare()), None);
        assert!(
            script_source_ps1().contains("FORCE_HYPERLINK"),
            "…so the PowerShell script must still be the one that says it"
        );
        // And across the WSL boundary a variable that is not listed does not
        // travel, so the declaration is listed whether or not we set it.
        let wsl = shell_command(
            &row("wsl"),
            &[],
            bash_only(Path::new(r"C:\s.bash")),
            &Env(vec![("FORCE_HYPERLINK", "0")]),
        );
        assert!(
            wsl.environment.iter().any(|(key, value)| key == "WSLENV"
                && value.to_string_lossy().contains("FORCE_HYPERLINK/u")),
            "the user's own answer has to cross too"
        );
    }

    /// PIN — a prompt the user wrote survives, and is not doubled.
    ///
    /// Red gate on the first half: assigning `PROMPT` instead of prefixing it
    /// passes every other test here and silently deletes a prompt somebody set
    /// with `setx`. Red gate on the second: a `cmd` pane exports `PROMPT` to
    /// its children, so without the idempotence check a Folio started
    /// from a `cmd` pane prints the directory twice, and one started from
    /// *that* prints it three times.
    #[test]
    fn a_prompt_the_user_already_set_is_kept_and_reported_in_front_of_exactly_once() {
        let theirs = shell_command(
            &row("cmd"),
            &[],
            Scripts::default(),
            &Env(vec![("PROMPT", "$T$S$P$G")]),
        );
        assert_eq!(
            prompt_of(&theirs),
            r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$T$S$P$G"
        );

        let again = shell_command(
            &row("cmd"),
            &[],
            Scripts::default(),
            &Env(vec![(
                "PROMPT",
                r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$T$S$P$G",
            )]),
        );
        assert_eq!(
            prompt_of(&again),
            r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$T$S$P$G",
            "an inherited prompt that already reports is left alone"
        );

        // An empty `PROMPT` is not a prompt the user chose to have; it is what
        // `cmd` reads as "use the default", and the default is what it gets.
        let empty = shell_command(
            &row("cmd"),
            &[],
            Scripts::default(),
            &Env(vec![("PROMPT", "")]),
        );
        assert_eq!(
            prompt_of(&empty),
            r"$e]133;D$e\$e]7;file:///$P$e\$e]133;A$e\$P$G"
        );
    }

    // -- the profile's own environment (7.1.6c-6c) ---------------------------

    fn value_of(command: &ShellCommand, name: &str) -> Option<String> {
        command
            .profile_environment
            .iter()
            .chain(&command.environment)
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    fn spelled(command: &ShellCommand, name: &str) -> usize {
        command
            .profile_environment
            .iter()
            .chain(&command.environment)
            .filter(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .count()
    }

    /// PIN - **a profile's own environment reaches the child, and it is written
    /// last** (plan 1.7, red gates 1 and 2).
    ///
    /// Three layers: what this window inherited, then what this terminal
    /// declares, then this. A profile's environment is the most specific
    /// sentence anybody says about its sessions, so it wins - `TERM_PROGRAM`
    /// included, which is the honest reading and not an oversight: a person who
    /// writes that row has told programs what to think this terminal is, and
    /// they are entitled to.
    ///
    /// Red gate: layer the profile *before* the declarations and a row named
    /// `TERM_PROGRAM` is silently the one thing in the table that cannot be
    /// overridden - with no symptom except that the row appears not to work.
    #[test]
    fn a_profiles_own_rows_are_written_over_what_this_terminal_says() {
        let command = shell_command(
            &row_with(
                "gitbash",
                &[("FOO", "bar"), ("TERM_PROGRAM", "xterm"), ("EMPTY", "")],
            ),
            &[],
            bash_only(Path::new(r"C:\s.bash")),
            &bare(),
        );
        assert_eq!(value_of(&command, "FOO").as_deref(), Some("bar"));
        assert_eq!(value_of(&command, "TERM_PROGRAM").as_deref(), Some("xterm"));
        // An empty value is carried through as an empty value. What the child
        // then has is *no such variable*, which is Windows' answer rather than
        // this module's - measured on the real machine (7.1.6c-6c evidence,
        // `21-empty-value.png`): a profile carrying `EMPTY=` takes it away from
        // its sessions even when this window inherited one. What is pinned here
        // is the layer this module owns, which is that the row reaches the spawn.
        assert_eq!(value_of(&command, "EMPTY").as_deref(), Some(""));
        // And it is this row's sentence and no other's: the shipped table is
        // untouched, so a sibling profile still hears what the terminal says.
        let sibling = shell_command(
            &row("gitbash"),
            &[],
            bash_only(Path::new(r"C:\s.bash")),
            &bare(),
        );
        assert_eq!(value_of(&sibling, "FOO"), None);
        assert_eq!(value_of(&sibling, "TERM_PROGRAM"), None);
    }

    /// PIN - **a row with no name is not a variable.**
    ///
    /// It is exactly what the editor's `Add` produces before anybody types, it
    /// round-trips through `profiles.json` as a key of `""`, and it is the one
    /// shape a child's environment block genuinely cannot carry. Dropped at the
    /// boundary and nowhere earlier, because the half-typed row is a real state
    /// of the editor and deleting it under the caret would be the dialog
    /// throwing away what somebody is in the middle of writing.
    #[test]
    fn a_nameless_row_never_reaches_a_child() {
        let command = shell_command(
            &row_with("cmd", &[("", "orphan"), ("KEPT", "1")]),
            &[],
            Scripts::default(),
            &bare(),
        );
        assert!(
            command.environment.iter().all(|(key, _)| !key.is_empty()),
            "{:?}",
            command.environment
        );
        assert_eq!(value_of(&command, "KEPT").as_deref(), Some("1"));
    }

    /// PIN - **`Force hyperlinks` is one question with one storage** (red gates
    /// 1 and 6).
    ///
    /// `Auto` is the profile saying nothing, so the terminal's own declaration
    /// stands byte for byte; `On` and `Off` are a row in that same environment
    /// with that name, and the row is the whole answer - a declaration pushed on
    /// top of it would leave two contradictory entries in one list, and the fact
    /// that the right one still wins at the far end would not make the record
    /// true.
    #[test]
    fn the_hyperlink_answer_a_profile_gives_replaces_the_declaration_rather_than_joining_it() {
        for answer in ["0", "1"] {
            let command = shell_command(
                &row_with("gitbash", &[(FORCE_HYPERLINK, answer)]),
                &[],
                bash_only(Path::new(r"C:\s.bash")),
                &bare(),
            );
            assert_eq!(spelled(&command, FORCE_HYPERLINK), 1, "{answer}");
            assert_eq!(value_of(&command, FORCE_HYPERLINK).as_deref(), Some(answer));
        }
        // `Auto` - no row of that name - is the behaviour that shipped before
        // the picker existed, unchanged.
        let auto = shell_command(
            &row("gitbash"),
            &[],
            bash_only(Path::new(r"C:\s.bash")),
            &bare(),
        );
        assert_eq!(value_of(&auto, FORCE_HYPERLINK).as_deref(), Some("1"));
        // And a profile's own `0` beats an inherited `1`, which the declaration
        // would have left alone: this is not a declaration, it is the answer.
        let over_inherited = shell_command(
            &row_with("gitbash", &[(FORCE_HYPERLINK, "0")]),
            &[],
            bash_only(Path::new(r"C:\s.bash")),
            &Env(vec![(FORCE_HYPERLINK, "1")]),
        );
        assert_eq!(
            value_of(&over_inherited, FORCE_HYPERLINK).as_deref(),
            Some("0")
        );
    }

    /// PIN - **a profile served by no door is handed nothing** (red gate 3).
    ///
    /// No `--init-file`, no `PROMPT`, nothing dot-sourced - and the degradation
    /// needs no invention: a screen that never sees OSC 133 keeps the
    /// cursor/WRAPLINE heuristics byte for byte, and a session that never sees
    /// OSC 7 leaves the relative path undetected rather than guessing.
    #[test]
    fn a_profile_with_no_door_is_handed_no_script_no_flag_and_no_prompt() {
        for id in ["gitbash", "cmd", "wsl"] {
            let shut = Profile {
                integration: profiles::IntegrationChoice::Named(Integration::None),
                paths: profiles::PathNamespace::Windows,
                ..row(id)
            };
            let command = shell_command(&shut, &[], bash_only(Path::new(r"C:\s.bash")), &bare());
            assert_eq!(
                command.arguments,
                // The row's own words, its login switch spelled among them
                // (0.4.6 ticket 74: Git Bash's `--login` is that switch now).
                profiles::launch_args(&shut)
                    .iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>(),
                "{id}: the profile's own words and no flag of ours"
            );
            assert_eq!(value_of(&command, "PROMPT"), None, "{id}");
            assert_eq!(value_of(&command, INSTALLED_MARKER), None, "{id}");
            // Links are still declared: they are a fact about this terminal and
            // not about a script, which is the whole of R-d.
            assert_eq!(value_of(&command, FORCE_HYPERLINK).as_deref(), Some("1"));
        }
    }

    /// PIN - **`Auto` derives the door every shipped profile has always had**
    /// (red gate 4).
    ///
    /// The five rows carry the rule and not an answer, so this is what keeps
    /// `pwsh` a PowerShell and `cmd` a `cmd`. Red gate: break the derivation and
    /// every shipped profile silently loses its integration at once, with no
    /// symptom but the absence of markers.
    #[test]
    fn auto_derives_the_door_every_shipped_profile_has_always_had() {
        for (id, door) in [
            ("pwsh", Integration::PowerShellOptIn),
            ("winps", Integration::PowerShellOptIn),
            ("wsl", Integration::BashInitFile),
            ("gitbash", Integration::BashInitFile),
            ("cmd", Integration::CmdPrompt),
        ] {
            let profile = row(id);
            assert_eq!(
                profile.integration,
                profiles::IntegrationChoice::Auto,
                "{id}"
            );
            assert_eq!(profiles::served_by(&profile), door, "{id}");
        }
        // And a program this list has not heard of gets no door at all, which is
        // the honest answer: `--init-file` handed to something that is not a
        // bash is a filename it will try to open.
        for (program, door) in [
            (r"C:\Users\me\.local\bin\claude.exe", Integration::None),
            (
                r"C:\Program Files\Git\bin\bash.exe",
                Integration::BashInitFile,
            ),
            (r"C:\Windows\System32\wsl.exe", Integration::BashInitFile),
            // A zsh has a door of its own: `ZDOTDIR`, because `--init-file` is
            // bash's flag and zsh refuses it (review row R3-6).
            ("/usr/bin/zsh", Integration::ZshDotDir),
            // And a Bourne shell has none, rather than one it accepts and
            // ignores in silence.
            ("/bin/sh", Integration::None),
            ("/usr/bin/dash", Integration::None),
            (r"C:\Windows\System32\cmd.exe", Integration::CmdPrompt),
            (r"D:\pwsh.exe", Integration::PowerShellOptIn),
        ] {
            assert_eq!(
                profiles::derive_integration(&ProgramSource::Path(PathBuf::from(program))),
                door,
                "{program}"
            );
        }
    }

    /// PIN - **a WSL profile's own variables are listed so that they cross.**
    ///
    /// A variable set on `wsl.exe` is set on a *Win32* process, and the
    /// distribution behind it sees nothing that was not named in `WSLENV`. Red
    /// gate, and it is the one failure with no symptom on this side: the row is
    /// stored, written to the launcher and honoured by every check except the
    /// only one that matters, which is `echo $FOO` inside the distribution.
    #[test]
    fn a_wsl_profiles_own_variables_are_listed_in_wslenv() {
        let script = Path::new(r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\folio.bash");
        let listed = |profile: &Profile| {
            value_of(
                &shell_command(profile, &[], bash_only(script), &bare()),
                "WSLENV",
            )
        };
        let carried = listed(&row_with("wsl", &[("FOO", "bar")]))
            .expect("a WSL profile is told what to carry");
        assert!(carried.contains("FOO/u"), "{carried}");
        assert!(
            carried.contains("TERM_PROGRAM/u"),
            "and the terminal's own listing is untouched: {carried}"
        );
        // A name already listed is not listed twice - `FORCE_HYPERLINK` is in
        // the terminal's own four, and a profile that answers the hyperlink
        // question would otherwise put it in the list a second time.
        let answered = listed(&row_with("wsl", &[(FORCE_HYPERLINK, "0")]))
            .expect("a WSL profile is told what to carry");
        assert_eq!(answered.matches("FORCE_HYPERLINK").count(), 1, "{answered}");
        // **The terminal's own declarations cross whatever the login shell
        // turns out to be** (2026-09-07). They used to be listed only where this
        // side had established that the shell was a bash, which made
        // `FORCE_HYPERLINK` — a fact about what this *terminal* renders — a
        // property of the reader's choice of shell: a zsh pane drew the same
        // hyperlinks as any other and told the programs in it that it did not.
        // Which shell answers is the pane's own business now, so the listing
        // asks the question it was always really asking.
        assert!(
            listed(&row("wsl")).is_some_and(|listed| listed.contains("FORCE_HYPERLINK/u")),
            "a WSL pane crosses the boundary whichever shell is behind it"
        );
        // The one WSL pane that lists nothing new is the one handed no script,
        // which is the pane that was never injected into.
        assert_eq!(
            value_of(
                &shell_command(&row("wsl"), &[], Scripts::default(), &bare()),
                "WSLENV"
            ),
            None
        );
    }

    /// PIN - **the capability sentence knows what the environment did to it**
    /// (J85, closed).
    ///
    /// Derived from the door, the namespace *and* the two environment rows that
    /// can silence a link: `FORCE_HYPERLINK=0` outright, and - on a PowerShell -
    /// a `TERM_PROGRAM` override, because `folio.ps1` declares links only for a
    /// session whose `TERM_PROGRAM` it recognises as this terminal's.
    #[test]
    fn a_profile_that_switched_links_off_no_longer_says_it_has_them() {
        assert!(declares_hyperlinks(&row("gitbash")));
        assert!(!declares_hyperlinks(&row_with(
            "gitbash",
            &[(FORCE_HYPERLINK, "0")]
        )));
        assert!(declares_hyperlinks(&row_with(
            "gitbash",
            &[(FORCE_HYPERLINK, "1")]
        )));
        // PowerShell's come from its own script, and the script asks who it is
        // talking to.
        assert!(declares_hyperlinks(&row("pwsh")));
        assert!(!declares_hyperlinks(&row_with(
            "pwsh",
            &[("TERM_PROGRAM", "xterm")]
        )));
        assert!(
            declares_hyperlinks(&row_with("pwsh", &[("TERM_PROGRAM", bt_pty::TERM_PROGRAM)])),
            "a row that restates what this terminal already says changes nothing"
        );
        // The same override on a profile this module declares for is nothing to
        // do with links: the declaration is ours and does not consult a name.
        assert!(declares_hyperlinks(&row_with(
            "gitbash",
            &[("TERM_PROGRAM", "xterm")]
        )));
    }

    /// PIN - **the editor's ghosts are what this terminal will actually say.**
    ///
    /// A constant list of three was already wrong in one place: PowerShell is
    /// the one door this module does not declare `FORCE_HYPERLINK` through, so a
    /// third ghost drawn on that page would be the page pretending - which is
    /// the thing the page exists to stop.
    #[test]
    fn the_ghosts_a_profile_shows_are_the_declarations_it_will_get() {
        assert_eq!(
            declared_environment(Integration::BashInitFile),
            [
                ("TERM_PROGRAM", bt_pty::TERM_PROGRAM),
                ("COLORTERM", "truecolor"),
                (FORCE_HYPERLINK, "1"),
            ]
        );
        assert_eq!(
            declared_environment(Integration::PowerShellOptIn),
            [
                ("TERM_PROGRAM", bt_pty::TERM_PROGRAM),
                ("COLORTERM", "truecolor"),
            ],
            "folio.ps1 is the half that says it, and saying it twice would be \
             two places to change with one silently redundant"
        );
    }

    /// PIN - a profile of the reader's own reaches the spawn path whole, which
    /// is what makes every case above a case about *any* profile and not about
    /// the five this build ships.
    #[test]
    fn a_profile_the_reader_wrote_is_spawned_by_the_same_arithmetic() {
        let theirs = Profile {
            id: "claude-7f3a".to_owned(),
            compared_title: None,
            display_title: "Claude".to_owned(),
            program: ProgramSource::Path(PathBuf::from(r"C:\Users\me\.local\bin\claude.exe")),
            args: vec!["--verbose".to_owned()],
            env: vec![("ANTHROPIC_LOG".to_owned(), "debug".to_owned())],
            integration: profiles::IntegrationChoice::Auto,
            origin: Origin::User,
            ..row("cmd")
        };
        assert_eq!(profiles::served_by(&theirs), Integration::None);
        let command = shell_command(&theirs, &[], Scripts::default(), &bare());
        assert_eq!(command.arguments, [OsString::from("--verbose")]);
        assert_eq!(
            value_of(&command, "ANTHROPIC_LOG").as_deref(),
            Some("debug")
        );
        assert_eq!(value_of(&command, "PROMPT"), None);
        assert_eq!(value_of(&command, FORCE_HYPERLINK).as_deref(), Some("1"));
    }

    // ── the PowerShell profile (§7.1.6j) ───────────────────────────────────

    pub(super) fn temp_dir(tag: &str) -> PathBuf {
        let dir = bt_testpath::temp_path(&format!("folio-ps-profile-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// **The path is whatever the shell says, and this test is the whole of that
    /// rule.** A build that composed `<Documents>\PowerShell\…` would pass every
    /// other test in this file and fail here, because on the machine this was
    /// written on the Documents known folder and the answer PowerShell gives sit
    /// on two different drives — and the file the shell names is the one with
    /// the integration in it while the composed one is two bytes of nothing.
    #[cfg(windows)]
    #[test]
    fn the_profile_is_the_file_the_shell_names_and_never_one_this_build_composed() {
        let composed_root = Path::new(r"C:\Users\me\Documents");
        let answer =
            parse_profile_path("D:\\Documents\\PowerShell\\Microsoft.PowerShell_profile.ps1\r\n")
                .expect("the shell answered");
        assert_eq!(
            answer,
            PathBuf::from(r"D:\Documents\PowerShell\Microsoft.PowerShell_profile.ps1")
        );
        assert!(
            !answer.starts_with(composed_root),
            "the answer is not under the folder a composing build would have used"
        );
        assert_eq!(parse_profile_path("   \r\n"), None);
        assert_eq!(parse_profile_path(""), None);
        assert_eq!(
            parse_profile_path("profile.ps1\r\n"),
            None,
            "a relative answer"
        );
    }

    /// Which programs are asked about at all.
    #[test]
    fn the_program_name_says_whether_this_is_a_powershell() {
        assert!(is_powershell(Path::new(
            r"C:\Program Files\PowerShell\7\pwsh.exe"
        )));
        assert!(is_powershell(Path::new(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        )));
        assert!(!is_powershell(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(!is_powershell(Path::new(r"C:\Windows\System32\cmd.exe")));
    }

    /// The criterion, and the whole of it: a line that dot-sources the script.
    /// A commented example — the script's own header carries one — is not an
    /// installation, and a build that read one as an installation would go
    /// silent about the very machine that needs the offer.
    #[test]
    fn only_a_live_line_counts_as_an_installed_integration() {
        assert!(profile_suppresses_integration_offer(
            profile_marks::LEGACY_LINE
        ));
        assert!(profile_suppresses_integration_offer(
            profile_marks::MANAGED_LINE
        ));
        assert!(profile_suppresses_integration_offer(
            r". 'D:\Developer\folio-terminal\scripts\shell-integration\FOLIO.PS1'"
        ));
        for user in ["# my note about folio.ps1", "\r\n", ""] {
            assert!(!profile_suppresses_integration_offer(user), "{user}");
        }
    }

    /// The line names the script through `$env:APPDATA` when that is where it
    /// is, so the profile keeps working for a user whose account is renamed or
    /// whose machine is rebuilt.
    #[test]
    fn the_line_spells_the_script_the_way_the_shell_can_re_derive_it() {
        assert_eq!(integration_line(), profile_marks::MANAGED_LINE);
    }

    /// A profile that is not there yet is created, directories and all, and it
    /// gets the line and nothing else — no blank line above the first thing in
    /// a file.
    #[test]
    fn a_profile_that_does_not_exist_is_created_holding_only_the_line() {
        let documents = temp_dir("absent");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        let written = add_to_profile(&profile, LINE, EPOCH_DAY).expect("the write");
        assert_eq!(written.backup, None, "there was nothing to back up");
        assert_eq!(
            std::fs::read_to_string(&profile).unwrap(),
            format!("{LINE}\r\n")
        );
    }

    /// The user's own two profiles are a bare `\r\n` apiece. An empty file has
    /// nothing to be separated from, so the line goes in at the top.
    #[test]
    fn an_empty_profile_gets_the_line_with_no_blank_line_above_it() {
        let documents = temp_dir("empty");
        let profile = documents.join("WindowsPowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, b"\r\n").unwrap();
        let written = add_to_profile(&profile, LINE, EPOCH_DAY).expect("the write");
        assert_eq!(
            std::fs::read_to_string(&profile).unwrap(),
            format!("\r\n{LINE}\r\n"),
            "what was there stays there; the line follows it"
        );
        let backup = written.backup.expect("a file that existed was backed up");
        assert_eq!(std::fs::read(&backup).unwrap(), b"\r\n");
    }

    /// A profile with something in it keeps every byte of it, gets a blank line
    /// and then the line — and the copy taken first is byte for byte what was
    /// there.
    #[test]
    fn a_profile_with_content_is_backed_up_byte_for_byte_before_the_append() {
        let documents = temp_dir("content");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        let before = "# mine\nfunction prompt { 'PS> ' }\n";
        std::fs::write(&profile, before).unwrap();
        let written = add_to_profile(&profile, LINE, EPOCH_DAY).expect("the write");
        let backup = written.backup.expect("a backup");
        assert_eq!(
            backup.file_name().unwrap().to_string_lossy(),
            "Microsoft.PowerShell_profile.ps1.bak-19700101",
            "beside the file it copies, named by the day it was taken"
        );
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(&profile).unwrap(),
            format!("{before}\n{LINE}\n"),
            "one blank line between what was theirs and what is ours, in the \
             line ending the file already uses"
        );
        assert!(profile_suppresses_integration_offer(
            &std::fs::read_to_string(&profile).unwrap()
        ));
    }

    /// A file with no trailing newline still gets one before the blank line, or
    /// the last thing the reader wrote and the line we add would be one
    /// statement.
    #[test]
    fn a_profile_that_does_not_end_in_a_newline_is_closed_before_the_append() {
        let documents = temp_dir("unterminated");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, "Set-Alias ll Get-ChildItem").unwrap();
        add_to_profile(&profile, LINE, EPOCH_DAY).expect("the write");
        assert_eq!(
            std::fs::read_to_string(&profile).unwrap(),
            format!("Set-Alias ll Get-ChildItem\r\n\r\n{LINE}\r\n")
        );
    }

    /// UTF-16LE with a BOM is edited in the original encoding; the backup
    /// retains the original bytes and the append does not mix encodings.
    #[test]
    fn a_utf16le_profile_keeps_its_encoding_and_bom() {
        let documents = temp_dir("utf16");
        let profile = documents.join(PROFILE_LEAF);
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend("# mine\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&profile, &bytes).unwrap();
        let written = add_to_profile(&profile, LINE, EPOCH_DAY).unwrap();
        assert_eq!(std::fs::read(written.backup.unwrap()).unwrap(), bytes);
        let mut expected = bytes;
        expected.extend(
            format!("\r\n{LINE}\r\n")
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert_eq!(std::fs::read(profile).unwrap(), expected);
    }

    /// RED (review row R4-4) — **the first backup of a day is kept, and the
    /// second write still takes one.**
    ///
    /// Half of this test is the old one and stays: the pristine copy — from
    /// before this product touched the file at all — is not replaced by one that
    /// already carries our line. What was missing is the other half. The rule
    /// used to be "a backup already taken today is not overwritten", implemented
    /// as *no backup at all*, so the second write into somebody's `$PROFILE`
    /// went in with no copy of what they had typed into it since the first.
    ///
    /// Red gate: put back the `if !path.exists()` around the copy and the second
    /// write's `backup` is the first one's path, holding the wrong bytes.
    #[test]
    fn a_second_write_the_same_day_keeps_the_first_copy_and_takes_one_of_its_own() {
        let documents = temp_dir("twice");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, "# mine\n").unwrap();

        let first = add_to_profile(&profile, LINE, EPOCH_DAY)
            .expect("the write")
            .backup
            .expect("a backup");
        // What the reader typed between the two writes, which is exactly what a
        // skipped second backup loses.
        let between = "# mine\n# and this is mine too\n".to_owned();
        std::fs::write(&profile, &between).unwrap();

        let second = add_to_profile(&profile, LINE, EPOCH_DAY)
            .expect("the second write")
            .backup
            .expect("a second backup");

        assert_ne!(first, second, "two writes, two copies");
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            "# mine\n",
            "the pristine copy is never written over"
        );
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            between,
            "and the second copy is the file as it stood a moment before"
        );
    }

    /// RED (review row R4-4) — **the profile is never observably empty.**
    ///
    /// `std::fs::write` truncates and then writes, so between those two calls the
    /// reader's `$PROFILE` is a zero-byte file — and this is the one file this
    /// product writes that belongs to somebody else's shell. The property is
    /// asserted the only way it can be from one thread: the write goes through
    /// `bt_persist::atomic_write`, so what appears at the profile's own name is a
    /// rename of a file that was already complete, and no temp file survives it.
    ///
    /// Red gate: put `std::fs::write(profile, &bytes)?` back and the directory
    /// listing is the same — but `add_to_profile` no longer goes through the
    /// atomic path, which the second assertion pins by name.
    #[test]
    fn writing_the_profile_leaves_no_half_written_file_in_the_folder() {
        let documents = temp_dir("atomic");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, "# mine\n").unwrap();

        add_to_profile(&profile, LINE, EPOCH_DAY).expect("the write");

        let folder = profile.parent().unwrap();
        let leftovers: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "no temp file survives: {leftovers:?}");
        assert!(
            std::fs::read_to_string(&profile).unwrap().contains(LINE),
            "and the line is in the file the shell will read"
        );

        let source = include_str!("shell_integration.rs");
        let body = source
            .split_once("fn replace_profile(")
            .expect("the writer")
            .1;
        let end = body.find("\n}\n").expect("its end");
        assert!(
            body[..end].contains("bt_persist::atomic_replace_preserving(profile"),
            "the profile is replaced atomically, not truncated and rewritten"
        );
        assert!(
            !body[..end].contains("std::fs::write(profile"),
            "and the truncating write is gone"
        );
    }

    /// RED (review row R4-6) — **an installed script that is not this build's is
    /// rewritten, and a `$PROFILE` that declares one is what triggers the
    /// comparison.**
    ///
    /// Two halves, because the defect is in the join between them. The first is
    /// the repair itself, over a temp directory rather than the machine's own
    /// `%APPDATA%`: a stale copy (an upgrade), a truncated one (a half-finished
    /// write) and a missing one (a cleaner) are one finding and get one answer.
    /// The second is that startup asks before any PowerShell pane can spawn. The repair existed
    /// for bash, which names its script on every spawn, and previously had no unconditional caller
    /// for PowerShell, whose script was named only by an optional profile line.
    ///
    /// Red gate: name the durable path without `install_script_at` in `powershell_script_for_birth`
    /// and the second half fails; make `install_script_at` return early whenever the file merely
    /// exists and the first half fails.
    #[test]
    fn an_installed_script_that_is_not_this_builds_is_rewritten() {
        let directory = temp_dir("stale-script");
        let name = "folio.ps1";
        let shipped = "# this build\n";

        // Nothing there at all.
        let path = install_script_at(&directory, name, shipped).expect("written");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), shipped);

        // An older build's copy, and a truncated one.
        for existing in ["# an older build\n", ""] {
            std::fs::write(&path, existing).unwrap();
            install_script_at(&directory, name, shipped).expect("repaired");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                shipped,
                "an installed copy that is not this build's is replaced by this build's"
            );
        }

        // And one that is already right is left alone rather than rewritten
        // under an open shell that may be reading it.
        std::fs::write(&path, shipped).unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        install_script_at(&directory, name, shipped).expect("unchanged");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before,
            "an identical file is read, not written"
        );

        // The second half: the durable copy every birth outside a trial names is the one
        // `install_script_at` compares against what this build ships.
        let body = Index::of_package("bt-app")
            .body_of(&bt_source::ItemQuery::function(
                "powershell_script_for_birth",
            ))
            .expect("the birth's script");
        assert!(
            body.contains("install_script_at(&durable, SCRIPT_FILE_PS1, SCRIPT_PS1)"),
            "a birth compares the installed integration against what this build ships"
        );
    }

    /// What the two constants above stand for: one day, and one line.
    const EPOCH_DAY: std::time::SystemTime = std::time::UNIX_EPOCH;
    const LINE: &str = profile_marks::MANAGED_LINE;
    /// The name both PowerShells give the file, for the tests that stand one up
    /// rather than asking a shell where it is.
    const PROFILE_LEAF: &str = "Microsoft.PowerShell_profile.ps1";

    /// A stand-in for a PowerShell that the parse probe asks: it reads the
    /// whole of standard input and answers `1`, as the parse command does for
    /// text that parses.
    #[cfg(unix)]
    fn parse_stand_in(tag: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = temp_dir(tag);
        let program = directory.join("pwsh");
        std::fs::write(&program, "#!/bin/sh\ncat >/dev/null\nprintf 1\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (directory, program)
    }

    /// RED mutation: in `bt_platform::ProbeChild::settle`, drop the early
    /// return of the kept status; the probe's poll reaps the child, the
    /// collection after it is refused, and the answer is `Wait`.
    #[cfg(unix)]
    #[test]
    fn a_parse_probe_on_unix_answers_after_its_poll_settled_the_child() {
        let (directory, program) = parse_stand_in("unix-parse");
        let answer = run_parse_probe(
            &program,
            "Write-Output 'na\u{ef}ve \u{3a9}\u{3bc}\u{3ad}\u{3b3}\u{3b1}'",
        );
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(answer, Ok(true));
    }

    /// RED mutation: as above, drop the early return of the kept status in
    /// `bt_platform::ProbeChild::settle`; the collection after `kill` is
    /// refused and the diagnostics lose what the probe had written.
    #[cfg(unix)]
    #[test]
    fn a_stopped_probe_on_unix_keeps_what_it_wrote() {
        use std::io::BufRead as _;
        let said = "partial na\u{ef}ve \u{3a9}\u{3bc}\u{3ad}\u{3b3}\u{3b1}";
        let hygiene = bt_pty::test_shell::Hygiene::new();
        let mut command = hygiene.command("/bin/sh", bt_platform::quiet_command);
        command.arg("-c").arg(format!(
            "printf '%s' '{said}'; printf 'ready\\n' >&2; exec sleep 30"
        ));
        let mut child = bt_platform::spawn_probe(&mut command, bt_platform::ProbeStdio::ANSWER)
            .expect("start the stand-in");
        let mut line = String::new();
        std::io::BufReader::new(child.take_stderr().expect("stderr was piped"))
            .read_line(&mut line)
            .expect("read readiness");
        assert_eq!(line, "ready\n");
        let output = stopped_output(child);
        assert_eq!(String::from_utf8_lossy(&output.stdout), said);
    }

    /// An environment whose home is a test shell's own.
    #[cfg(windows)]
    struct HomeIs(PathBuf);

    #[cfg(windows)]
    impl ShellEnvironment for HomeIs {
        fn var_os(&self, key: &str) -> Option<OsString> {
            (key == profiles::home_variable(profiles::SeedPlatform::of_this_build()))
                .then(|| self.0.clone().into_os_string())
        }

        fn is_file(&self, _path: &Path) -> bool {
            false
        }
    }

    /// Feed `shell`'s output to `pane` until `done` holds, failing after a minute of it not.
    #[cfg(windows)]
    fn feed_until(
        shell: &mut bt_pty::test_shell::TestShell,
        pane: &mut bt_term::DualPlaneSession,
        what: &str,
        done: impl Fn(&bt_term::DualPlaneSession) -> bool,
    ) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut seen = Vec::new();
        while !done(pane) {
            assert!(
                std::time::Instant::now() < deadline,
                "{what}: never reached; {}; output was {:?}",
                shell.account(),
                String::from_utf8_lossy(&seen)
            );
            let chunk = shell.read_output();
            if chunk.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            pane.feed(&chunk).unwrap();
            seen.extend(chunk);
        }
    }

    /// Issue #28 through a real shell: a profile row whose starting folder is a fixed folder
    /// (`profiles::place_for`, as `create_leaf_session` asks it), the shell started there by
    /// `start`, its bytes read by a real session told its spawn directory as a pane is, and the
    /// files card's folder (`crate::files_root_of`, the folder button's and `Ctrl+Shift+B`'s one
    /// reader) asked at birth, at the first prompt and after `cd` into a subfolder.
    ///
    /// **What the card is compared with is what the product holds.** The pane keeps every folder
    /// in the spelling it was given — the profile's folder as the profile names it, a report as
    /// the shell spelled it — and does not canonicalise. A shell does not have to spell its folder
    /// the way it was handed it: PowerShell answers `$PWD` with long names where its working
    /// directory was given an 8.3 component (CI run 37739213188: handed the runner's account
    /// folder by its 8.3 name, it reported the long name). So the card is the
    /// profile's folder *as given* before the first report, and *the report itself* after it,
    /// which must name the same directory (`bt_platform::same_file`). The folder is handed over
    /// through an 8.3 component on every machine — a long-named folder, referred to by its short
    /// name — so a comparison of spellings is red here and not only on a runner.
    #[cfg(windows)]
    fn files_card_follows_a_real_shell_from_its_fixed_starting_folder(
        start: impl FnOnce(
            bt_pty::test_shell::Hygiene,
            Option<PathBuf>,
        ) -> bt_pty::test_shell::TestShell,
    ) {
        let hygiene = bt_pty::test_shell::Hygiene::new();
        let home = hygiene.home();
        let long = hygiene.root().join("a long folder name 长名");
        std::fs::create_dir_all(long.join("sandbox 沙盒").join("sub 子")).unwrap();
        let short = bt_platform::short_path_name(&long).unwrap();
        assert_ne!(
            short, long,
            "the test folder's volume keeps 8.3 names, which the spelling under test needs"
        );
        let fixed = short.join("sandbox 沙盒");
        let sub = fixed.join("sub 子");
        let environment = HomeIs(home.clone());
        let place = profiles::place_for(
            &profiles::StartAt::Fixed(fixed.clone()),
            &profiles::StartingDir::AccountHome,
            profiles::PathNamespace::Windows,
            None,
            &environment,
        );
        assert_eq!(place.working_directory.as_deref(), Some(fixed.as_path()));
        let mut shell = start(hygiene, place.working_directory.clone());
        crate::test_support::install_this_machines_names();
        let mut pane = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(120).unwrap(),
            std::num::NonZeroU32::new(30).unwrap(),
        );
        pane.set_spawn_directory(place.directory.clone());
        let card = |pane: &bt_term::DualPlaneSession| {
            PathBuf::from(crate::files_root_of(Some(pane), &environment))
        };
        assert_ne!(home, fixed);
        assert_eq!(
            card(&pane),
            fixed,
            "before the first prompt: the fixed folder"
        );

        feed_until(&mut shell, &mut pane, "the first report", |pane| {
            pane.working_directory().is_some()
        });
        let reported = pane.working_directory().unwrap().to_path_buf();
        assert!(
            bt_platform::same_file(&reported, &fixed),
            "the first report {reported:?} names the fixed folder {fixed:?}"
        );
        assert_eq!(
            card(&pane),
            reported,
            "after the first prompt: the reported folder"
        );

        shell.write("cd \"sub 子\"\r".as_bytes()).unwrap();
        feed_until(&mut shell, &mut pane, "the report after cd", |pane| {
            pane.working_directory()
                .is_some_and(|reported| bt_platform::same_file(reported, &sub))
        });
        let reported = pane.working_directory().unwrap().to_path_buf();
        assert_eq!(card(&pane), reported, "after cd: the subfolder");
        shell.write(b"exit\r").unwrap();
        shell.shutdown().unwrap();
    }

    /// RED (issue #28) — **the reporter's configuration through a real PowerShell**: a PowerShell
    /// row with Starting directory = a fixed folder and `-NoLogo` as its only argument, composed as
    /// a birth composes it (`-NoLogo -NoExit -Command <loader>`). The files card shows the fixed
    /// folder before the first prompt, the reported folder after it, and the subfolder after `cd`.
    ///
    /// Both editions, by the rule `bt-pty`'s `shell_integration_osc133` keeps: Windows PowerShell
    /// is part of Windows and is never skipped, so this test cannot pass with no shell under it;
    /// PowerShell 7 — the reporter's edition — is optional on a machine and is resolved the way the
    /// product resolves it (`bt_pty::resolve_powershell_seven`), its arm skipped with a line on
    /// stderr when there is none.
    ///
    /// MUTATION, observed red: `files_root_of` reads `DualPlaneSession::working_directory`
    /// instead of `standing_folder` — the birth row answers the home folder.
    #[cfg(windows)]
    #[test]
    fn the_files_card_follows_a_powershell_pane_from_its_fixed_starting_folder() {
        let windows_powershell =
            bt_platform::program_on_path(Path::new(bt_pty::WINDOWS_POWERSHELL))
                .expect("Windows PowerShell is part of Windows");
        let mut editions = vec![windows_powershell];
        match bt_pty::resolve_powershell_seven(&bt_pty::SystemShellEnvironment) {
            Some(pwsh) => editions.push(PathBuf::from(pwsh)),
            None => eprintln!(
                "files card, PowerShell 7 arm skipped: pwsh.exe is not installed \
                 (the product's own resolver found none)"
            ),
        }
        let root = temp_dir("files-card-powershell");
        std::fs::create_dir_all(&root).unwrap();
        let integration = root.join(SCRIPT_FILE_PS1);
        std::fs::write(&integration, SCRIPT_PS1).unwrap();
        for program in editions {
            let arguments = composed_powershell_arguments(
                &program,
                &os_words(&["-NoLogo"]),
                &integration,
                None,
            )
            .expect("a `-NoLogo` row receives the loader");
            assert_eq!(
                arguments[..3],
                os_words(&["-NoLogo", "-NoExit", "-Command"])
            );
            files_card_follows_a_real_shell_from_its_fixed_starting_folder(|hygiene, directory| {
                let hygiene = hygiene.with_powershell_shape(test_shell_shape(&program, &arguments));
                bt_pty::test_shell::TestShell::spawn_shell_in(
                    hygiene,
                    program.clone(),
                    &arguments,
                    &|| last_resort_arguments(false),
                    &[],
                    bt_pty::PtySize::cells(
                        std::num::NonZeroU16::new(120).unwrap(),
                        std::num::NonZeroU16::new(30).unwrap(),
                    ),
                    directory,
                )
                .unwrap()
            });
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// RED (issue #28) — **the same through Command Prompt's `PROMPT`, unchanged**: `cmd.exe` with
    /// the `PROMPT` a pane gives it, started in a fixed folder.
    ///
    /// MUTATION, observed red: the one named on
    /// `the_files_card_follows_a_powershell_pane_from_its_fixed_starting_folder`.
    #[cfg(windows)]
    #[test]
    fn the_files_card_follows_a_command_prompt_pane_from_its_fixed_starting_folder() {
        let cmd = bt_platform::program_on_path(Path::new("cmd.exe")).expect("cmd.exe");
        files_card_follows_a_real_shell_from_its_fixed_starting_folder(|hygiene, directory| {
            bt_pty::test_shell::TestShell::spawn_shell_in(
                hygiene,
                cmd,
                &[],
                &|| last_resort_arguments(false),
                &[(OsString::from(CMD_PROMPT), cmd_prompt(None))],
                bt_pty::PtySize::cells(
                    std::num::NonZeroU16::new(120).unwrap(),
                    std::num::NonZeroU16::new(30).unwrap(),
                ),
                directory,
            )
            .unwrap()
        });
    }
}
