//! Handing a shell the script that makes it legible.
//!
//! `docs/shell-integration.md` describes what the markers mean; this module is
//! the one place that decides **how the script reaches the shell**, which is a
//! different question for every family and has exactly one right answer per
//! family:
//!
//! | profile | mechanism |
//! |---|---|
//! | PowerShell | none — the user dot-sources it into `$PROFILE` themselves |
//! | Git Bash | `bash --init-file <script> -i`, replacing `--login -i` |
//! | WSL | `wsl.exe … -- <login shell> --init-file <script> -i` |
//! | Command Prompt | the `PROMPT` variable, carrying `OSC 7` and nothing else |
//!
//! PowerShell's absence from that list is not an omission. `pwsh` has one
//! startup file at one well-known path and no argument that would source a
//! second one after it, so the only automatic injection available would be
//! writing into `$PROFILE` — editing a file that belongs to the user. bash has
//! `--init-file`, which names the startup file for one interactive shell and
//! touches nothing on disk, so bash gets the automatic install and PowerShell
//! keeps the manual one. The asymmetry is the shells', not a preference.
//!
//! `cmd.exe` has no startup file to name and no hook to install, so its whole
//! integration is a *format string* — see [`profiles::Integration::CmdPrompt`]
//! for why that string carries `OSC 7` alone and no OSC 133 marker at all.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use bt_pty::ShellEnvironment;

use crate::{
    persist,
    profiles::{self, Integration, PROFILES, windows_to_wsl},
    wsl::WslFacts,
};

/// The script, compiled in.
///
/// Embedded rather than found next to the executable, and the reason is a
/// protocol one rather than a packaging one: the script and the terminal are two
/// halves of one agreement about what `OSC 133;D` means, and a build that could
/// load an older or newer half would be a build whose markers mean whatever
/// happens to be on disk. `include_str!` makes the two halves ship as one thing.
const SCRIPT: &str = include_str!("../../../scripts/shell-integration/betterterminal.bash");

/// The name it is written under, in `%APPDATA%\BetterTerminal\`.
const SCRIPT_FILE: &str = "betterterminal.bash";

/// The variable that tells the script it is being used as an init file, and is
/// therefore responsible for the startup chain `--init-file` displaced.
///
/// Its absence is equally meaningful: a hand-installed copy dot-sourced from the
/// user's own `~/.bashrc` must **not** source the login files, because bash
/// already did.
const INSTALLED_MARKER: &str = "BT_SHELL_INTEGRATION";

/// The variable `cmd.exe` prints its prompt from, and this build's only way in.
const CMD_PROMPT: &str = "PROMPT";

/// What a Rust CLI tool asks before it will print an `OSC 8` hyperlink — see
/// [`hyperlink_declaration`].
const FORCE_HYPERLINK: &str = "FORCE_HYPERLINK";

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
/// expands to `D:\Developer\BetterTerminal`, and `PROMPT` has no substitution,
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

/// The variables a WSL shell is given, listed for `WSLENV` so that they cross
/// the Win32/Linux boundary.
///
/// The last three are the identity declarations `PtyCommand` already puts in
/// every child's environment; a WSL shell is the one child that could not see
/// them, because `wsl.exe` forwards nothing it was not told to. Forwarding them
/// is not this ticket inventing a capability — it is the same declaration every
/// other profile has always received, finally reaching the one that could not.
/// `FORCE_HYPERLINK` is listed whether or not this process sets it: the listing
/// forwards whatever value ends up on the Win32 side, so a user who set their
/// own answer has it carried into the distribution rather than overwritten by
/// its absence.
const FORWARDED: [&str; 5] = [
    INSTALLED_MARKER,
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    FORCE_HYPERLINK,
];

/// Where the script is on this machine, written out on first use.
///
/// `None` when it could not be written, and that is a whole, honest outcome
/// rather than an error to report: a shell with no init file is a shell on the
/// documented fallback path, which is where every bash pane was before this
/// existed.
pub fn script_path() -> Option<&'static Path> {
    static INSTALLED: OnceLock<Option<PathBuf>> = OnceLock::new();
    INSTALLED.get_or_init(install).as_deref()
}

fn install() -> Option<PathBuf> {
    let directory = persist::storage_dir().join("shell-integration");
    let path = directory.join(SCRIPT_FILE);
    // Rewritten only when it differs, so that the common start — the same build
    // opening a bash tab again — is one read rather than one write, and an open
    // shell reading the file at that moment is not reading a truncated one.
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == SCRIPT) {
        return Some(path);
    }
    std::fs::create_dir_all(&directory).ok()?;
    std::fs::write(&path, SCRIPT).ok()?;
    Some(path)
}

/// Everything the spawn needs to say, beyond the program itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellCommand {
    pub arguments: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
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
/// `place_arguments` are [`profiles::SpawnPlace`]'s and sit between the two,
/// which matters for WSL alone: `--cd` is a flag to the *launcher* and must come
/// before the `--` that ends the launcher's own arguments.
///
/// `environment` is read, not written: Command Prompt's integration is a
/// variable this process already has one of, and prefixing rather than replacing
/// it means the composition has to see what is there.
#[must_use]
pub fn shell_command(
    profile: usize,
    place_arguments: &[OsString],
    script: Option<&Path>,
    wsl: &WslFacts,
    environment: &dyn ShellEnvironment,
) -> ShellCommand {
    let own = || {
        PROFILES[profile]
            .args
            .iter()
            .map(OsString::from)
            .chain(place_arguments.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut command = shell_command_for(profile, place_arguments, script, wsl, environment, &own);
    command.environment.extend(hyperlink_declaration(
        PROFILES[profile].integration,
        environment,
    ));
    command
}

fn shell_command_for(
    profile: usize,
    place_arguments: &[OsString],
    script: Option<&Path>,
    wsl: &WslFacts,
    environment: &dyn ShellEnvironment,
    own: &dyn Fn() -> Vec<OsString>,
) -> ShellCommand {
    match (PROFILES[profile].integration, script) {
        (Integration::BashInitFile, Some(script)) => match PROFILES[profile].paths {
            // Git Bash: bash *is* the program, and takes the flag directly. The
            // profile's own `--login -i` is dropped — the script puts the login
            // chain back itself, which is the trade `--init-file` demands.
            profiles::PathNamespace::Windows => ShellCommand {
                arguments: [OsString::from("--init-file"), script.into(), "-i".into()]
                    .into_iter()
                    .chain(place_arguments.iter().cloned())
                    .collect(),
                environment: installed_environment(false),
            },
            // WSL: `wsl.exe` is a launcher, so the shell and its flag come after
            // `--`, and the script has to be named in the *distribution's* own
            // spelling because it is the distribution that will open it.
            profiles::PathNamespace::Wsl => {
                let Some((shell, script)) = wsl.integrated_login_shell().zip(
                    windows_to_wsl(script)
                        .and_then(|path| path.into_os_string().into_string().ok()),
                ) else {
                    return ShellCommand {
                        arguments: own(),
                        environment: Vec::new(),
                    };
                };
                ShellCommand {
                    arguments: own()
                        .into_iter()
                        .chain(
                            ["--", shell, "--init-file", &script, "-i"]
                                .into_iter()
                                .map(OsString::from),
                        )
                        .collect(),
                    environment: installed_environment(true),
                }
            }
        },
        (Integration::CmdPrompt, _) => ShellCommand {
            arguments: own(),
            environment: vec![(
                OsString::from(CMD_PROMPT),
                cmd_prompt(environment.var_os(CMD_PROMPT)),
            )],
        },
        _ => ShellCommand {
            arguments: own(),
            environment: Vec::new(),
        },
    }
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
/// It used to be `betterterminal.ps1` line 16 that said so, which made a
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
fn hyperlink_declaration(
    integration: Integration,
    environment: &dyn ShellEnvironment,
) -> Option<(OsString, OsString)> {
    (integration != Integration::PowerShellOptIn && environment.var_os(FORCE_HYPERLINK).is_none())
        .then(|| (OsString::from(FORCE_HYPERLINK), OsString::from("1")))
}

/// `existing` — whatever `PROMPT` this process inherited — with the working
/// directory report in front of it.
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
    // BetterTerminal launched from one — or a `cmd` started inside a `cmd` —
    // inherits a string that already carries the report, and prefixing again
    // would print the directory twice per prompt and go on doubling.
    if existing
        .as_ref()
        .is_some_and(|prompt| prompt.to_string_lossy().contains(CMD_OSC7))
    {
        return existing.unwrap_or_default();
    }
    let mut prompt = OsString::from(CMD_OSC7);
    prompt.push(existing.unwrap_or_else(|| OsString::from(CMD_DEFAULT_PROMPT)));
    prompt
}

/// The marker, plus — across the WSL boundary — the list of what to carry over.
///
/// `WSLENV` is appended to rather than assigned, because it is a variable the
/// user may already be using to pass their own values into the distribution, and
/// replacing it would silently stop that.
fn installed_environment(through_wsl: bool) -> Vec<(OsString, OsString)> {
    let mut environment = vec![(OsString::from(INSTALLED_MARKER), OsString::from("1"))];
    if through_wsl {
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
        environment.push((OsString::from("WSLENV"), OsString::from(list)));
    }
    environment
}

/// The script's own text, for the tests that check what ships.
#[cfg(test)]
pub(crate) const fn script_source() -> &'static str {
    SCRIPT
}

/// PowerShell's script, which this module never installs but does depend on for
/// one declaration — see [`hyperlink_declaration`].
#[cfg(test)]
const fn script_source_ps1() -> &'static str {
    include_str!("../../../scripts/shell-integration/betterterminal.ps1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::index_of_id;

    fn args(command: &ShellCommand) -> Vec<String> {
        command
            .arguments
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn bash_wsl() -> WslFacts {
        crate::wsl::test_facts("Ubuntu-24.04", Some("/bin/bash"))
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
            "betterterminal.bash must be checked out with LF endings — see .gitattributes"
        );
        // And it is the script, not an empty file that would inject nothing.
        for marker in ["133;A", "133;B", "133;C", "133;D", "]7;", "file://"] {
            assert!(
                script_source().contains(marker),
                "the script must emit {marker}"
            );
        }
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
        let script = Path::new(
            r"C:\Users\dev\AppData\Roaming\BetterTerminal\shell-integration\betterterminal.bash",
        );
        let command = shell_command(
            index_of_id("gitbash"),
            &[],
            Some(script),
            &bash_wsl(),
            &bare(),
        );
        assert_eq!(
            args(&command),
            [
                "--init-file",
                r"C:\Users\dev\AppData\Roaming\BetterTerminal\shell-integration\betterterminal.bash",
                "-i"
            ]
        );
        assert!(
            !args(&command).iter().any(|argument| argument == "--login"),
            "`--login` and `--init-file` cannot both be honoured, so only one is passed"
        );
        assert!(
            command
                .environment
                .contains(&(OsString::from("BT_SHELL_INTEGRATION"), OsString::from("1"))),
            "the marker is what makes the script run the login chain it displaced"
        );
        // No script on this machine, and Git Bash is the shell it always was.
        assert_eq!(
            args(&shell_command(
                index_of_id("gitbash"),
                &[],
                None,
                &bash_wsl(),
                &bare()
            )),
            ["--login", "-i"]
        );
    }

    /// PIN — WSL is told the place first and the shell after `--`, and the
    /// script is named in the distribution's own spelling.
    ///
    /// Red gate: passing the Windows path of the script to `wsl.exe` gives the
    /// distribution a filename with a drive letter and backslashes, which it
    /// cannot open — so `--init-file` names nothing, bash starts with no startup
    /// file at all, and the user loses their own `~/.bashrc` as well as our
    /// markers. That is strictly worse than not injecting.
    #[test]
    fn wsl_is_told_the_place_before_the_shell_and_the_script_in_its_own_spelling() {
        let script = Path::new(
            r"C:\Users\dev\AppData\Roaming\BetterTerminal\shell-integration\betterterminal.bash",
        );
        let place = [OsString::from("--cd"), OsString::from("/mnt/d/Developer")];
        let command = shell_command(
            index_of_id("wsl"),
            &place,
            Some(script),
            &bash_wsl(),
            &bare(),
        );
        assert_eq!(
            args(&command),
            [
                "--cd",
                "/mnt/d/Developer",
                "--",
                "/bin/bash",
                "--init-file",
                "/mnt/c/Users/dev/AppData/Roaming/BetterTerminal/shell-integration/betterterminal.bash",
                "-i"
            ],
            "`--cd` is the launcher's and must precede the `--` that ends its arguments"
        );
        assert!(
            command
                .environment
                .iter()
                .any(|(key, value)| key == "WSLENV"
                    && value.to_string_lossy().contains("BT_SHELL_INTEGRATION/u")),
            "a variable that is not listed in WSLENV does not cross into the distribution"
        );
        // A distribution that logs into zsh keeps its shell, and the launcher is
        // told only where to stand.
        let zsh = crate::wsl::test_facts("Ubuntu-24.04", Some("/usr/bin/zsh"));
        assert_eq!(
            args(&shell_command(
                index_of_id("wsl"),
                &place,
                Some(script),
                &zsh,
                &bare()
            )),
            ["--cd", "/mnt/d/Developer"]
        );
    }

    /// PIN — PowerShell is not injected into, by any door.
    #[test]
    fn powershell_is_not_injected_into() {
        // **Both of them.** They are two profiles and one script: 5.1 and 7 read
        // the same `$PROFILE` mechanism, `betterterminal.ps1` is written for
        // both, and neither is written into by this product.
        for id in ["pwsh", "winps"] {
            let profile = index_of_id(id);
            assert_eq!(
                PROFILES[profile].integration,
                Integration::PowerShellOptIn,
                "{id}: PowerShell's script is the user's to install"
            );
            let command = shell_command(
                profile,
                &[],
                Some(Path::new(r"C:\script.bash")),
                &bash_wsl(),
                &bare(),
            );
            assert_eq!(
                command.arguments,
                PROFILES[profile]
                    .args
                    .iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>(),
                "{id}"
            );
            assert!(command.environment.is_empty(), "{id}");
        }
    }

    /// PIN — Command Prompt reports where it is standing, and claims nothing
    /// else.
    ///
    /// Red gate, and it is the *absence* that is load-bearing: adding
    /// `$e]133;A$e\` to this string is a one-token edit that looks like more
    /// capability and is less. `133;A` turns
    /// `DualPlaneSession::shell_integration_is_authoritative` on, whose whole
    /// job is to retire the cursor-line heuristic in favour of the semantic
    /// input region — a region only `133;B`/`133;C` can build, and `cmd.exe`
    /// can emit neither, because `PROMPT` is expanded once before a line is
    /// read and there is no second moment to be called at. The pane would come
    /// out of that trade with its typed line decorated as it is typed. See
    /// [`profiles::Integration::CmdPrompt`] for the `133;B` half.
    #[test]
    fn command_prompt_reports_its_directory_and_claims_no_shell_integration() {
        let command = shell_command(
            index_of_id("cmd"),
            &[],
            Some(Path::new(r"C:\script.bash")),
            &bash_wsl(),
            &bare(),
        );
        assert!(
            command.arguments.is_empty(),
            "cmd takes no argument that would leave it interactive"
        );
        let prompt = prompt_of(&command);
        assert_eq!(
            prompt, r"$e]7;file:///$P$e\$P$G",
            "the report, then the prompt cmd would have printed on its own"
        );
        assert!(
            !prompt.contains("133"),
            "a shell that cannot close a region must not open one: {prompt}"
        );
    }

    /// PIN — every shell this terminal starts is told that it renders
    /// hyperlinks, and none is told over the top of an answer already given.
    ///
    /// This is R-d settled (`docs/M2-persistence-schema-v1.md` §296-299), and
    /// the red gate is the *coverage*: with the declaration back inside
    /// `betterterminal.ps1` where it used to live, `OSC 8` links worked in a
    /// PowerShell whose owner had installed the opt-in script and in no other
    /// pane in the window — a capability of the terminal reachable only through
    /// one profile's optional file.
    #[test]
    fn every_shell_is_told_this_terminal_renders_hyperlinks_unless_it_was_already_told() {
        let forced = |id: &str, environment: &dyn ShellEnvironment| {
            shell_command(
                index_of_id(id),
                &[],
                Some(Path::new(r"C:\s.bash")),
                &bash_wsl(),
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
            index_of_id("wsl"),
            &[],
            Some(Path::new(r"C:\s.bash")),
            &bash_wsl(),
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
    /// its children, so without the idempotence check a BetterTerminal started
    /// from a `cmd` pane prints the directory twice, and one started from
    /// *that* prints it three times.
    #[test]
    fn a_prompt_the_user_already_set_is_kept_and_reported_in_front_of_exactly_once() {
        let theirs = shell_command(
            index_of_id("cmd"),
            &[],
            None,
            &bash_wsl(),
            &Env(vec![("PROMPT", "$T$S$P$G")]),
        );
        assert_eq!(prompt_of(&theirs), r"$e]7;file:///$P$e\$T$S$P$G");

        let again = shell_command(
            index_of_id("cmd"),
            &[],
            None,
            &bash_wsl(),
            &Env(vec![("PROMPT", r"$e]7;file:///$P$e\$T$S$P$G")]),
        );
        assert_eq!(
            prompt_of(&again),
            r"$e]7;file:///$P$e\$T$S$P$G",
            "an inherited prompt that already reports is left alone"
        );

        // An empty `PROMPT` is not a prompt the user chose to have; it is what
        // `cmd` reads as "use the default", and the default is what it gets.
        let empty = shell_command(
            index_of_id("cmd"),
            &[],
            None,
            &bash_wsl(),
            &Env(vec![("PROMPT", "")]),
        );
        assert_eq!(prompt_of(&empty), r"$e]7;file:///$P$e\$P$G");
    }
}
