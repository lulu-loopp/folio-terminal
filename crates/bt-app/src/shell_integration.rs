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
//! | Command Prompt | none — `cmd.exe` has no pre/post-command hook to install |
//!
//! PowerShell's absence from that list is not an omission. `pwsh` has one
//! startup file at one well-known path and no argument that would source a
//! second one after it, so the only automatic injection available would be
//! writing into `$PROFILE` — editing a file that belongs to the user. bash has
//! `--init-file`, which names the startup file for one interactive shell and
//! touches nothing on disk, so bash gets the automatic install and PowerShell
//! keeps the manual one. The asymmetry is the shells', not a preference.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
};

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

/// The variables a WSL shell is given, listed for `WSLENV` so that they cross
/// the Win32/Linux boundary.
///
/// The last three are the identity declarations `PtyCommand` already puts in
/// every child's environment; a WSL shell is the one child that could not see
/// them, because `wsl.exe` forwards nothing it was not told to. Forwarding them
/// is not this ticket inventing a capability — it is the same declaration every
/// other profile has always received, finally reaching the one that could not.
const FORWARDED: [&str; 4] = [
    INSTALLED_MARKER,
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
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
#[must_use]
pub fn shell_command(
    profile: usize,
    place_arguments: &[OsString],
    script: Option<&Path>,
    wsl: &WslFacts,
) -> ShellCommand {
    let own = || {
        PROFILES[profile]
            .args
            .iter()
            .map(OsString::from)
            .chain(place_arguments.iter().cloned())
            .collect::<Vec<_>>()
    };
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
        _ => ShellCommand {
            arguments: own(),
            environment: Vec::new(),
        },
    }
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
        let command = shell_command(index_of_id("gitbash"), &[], Some(script), &bash_wsl());
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
        assert_eq!(
            command.environment,
            [(OsString::from("BT_SHELL_INTEGRATION"), OsString::from("1"))],
            "the marker is what makes the script run the login chain it displaced"
        );
        // No script on this machine, and Git Bash is the shell it always was.
        assert_eq!(
            args(&shell_command(
                index_of_id("gitbash"),
                &[],
                None,
                &bash_wsl()
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
        let command = shell_command(index_of_id("wsl"), &place, Some(script), &bash_wsl());
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
                &zsh
            )),
            ["--cd", "/mnt/d/Developer"]
        );
    }

    /// PIN — the two profiles with no script are left exactly as they were.
    #[test]
    fn powershell_and_cmd_are_not_injected_into() {
        for id in ["pwsh", "cmd"] {
            let profile = index_of_id(id);
            assert_ne!(PROFILES[profile].integration, Integration::BashInitFile);
            let command = shell_command(
                profile,
                &[],
                Some(Path::new(r"C:\script.bash")),
                &bash_wsl(),
            );
            assert_eq!(
                command.arguments,
                PROFILES[profile]
                    .args
                    .iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>()
            );
            assert!(command.environment.is_empty());
        }
    }
}
