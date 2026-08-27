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
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use bt_pty::ShellEnvironment;

use crate::{
    persist,
    profiles::{self, Integration, Profile, windows_to_wsl},
    wsl::WslFacts,
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
    profile: &Profile,
    place_arguments: &[OsString],
    script: Option<&Path>,
    wsl: &WslFacts,
    environment: &dyn ShellEnvironment,
) -> ShellCommand {
    let own = || {
        profile
            .args
            .iter()
            .map(OsString::from)
            .chain(place_arguments.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut command = shell_command_for(profile, place_arguments, script, wsl, environment, &own);
    let mine = &profile.env;
    command.environment.extend(hyperlink_declaration(
        profiles::served_by(profile),
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
    layer_profile_environment(&mut command.environment, mine);
    command
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
        if name.is_empty() {
            continue;
        }
        let name = OsString::from(name);
        let value = OsString::from(value);
        match environment
            .iter_mut()
            .find(|(existing, _)| environment_name_eq(existing, &name))
        {
            Some((_, existing)) => *existing = value,
            None => environment.push((name, value)),
        }
    }
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
    place_arguments: &[OsString],
    script: Option<&Path>,
    wsl: &WslFacts,
    environment: &dyn ShellEnvironment,
    own: &dyn Fn() -> Vec<OsString>,
) -> ShellCommand {
    match (profiles::served_by(profile), script) {
        (Integration::BashInitFile, Some(script)) => match profile.paths {
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
    // Folio launched from one — or a `cmd` started inside a `cmd` —
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

// ── PowerShell's own door: the profile, and the one line that opens it ──────
//
// The table at the top of this file says PowerShell's mechanism is "none — the
// user dot-sources it into `$PROFILE` themselves", and that stays true: nothing
// below runs on its own. What it adds is the ability to *offer* — to read the
// file, see that the line is not in it, and write the line if the reader asks
// for it in so many words. The asymmetry with bash is unchanged, because
// `--init-file` touches no file at all and this touches one that belongs to
// somebody, which is why it happens only on a press and never on a spawn.

/// PowerShell's script, under the same roof as bash's.
const SCRIPT_PS1: &str = include_str!("../../../scripts/shell-integration/folio.ps1");

/// The name it is written under, beside [`SCRIPT_FILE`].
const SCRIPT_FILE_PS1: &str = "folio.ps1";

/// The sub-directory of `%APPDATA%\Folio\` both scripts live in.
const SCRIPT_DIRECTORY: &str = "shell-integration";

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
const PROFILE_COMMAND: &str = "$PROFILE.CurrentUserCurrentHost";

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
type ProfileAnswers = std::collections::BTreeMap<PathBuf, Option<PathBuf>>;
static PROFILE_ANSWERS: OnceLock<std::sync::Mutex<ProfileAnswers>> = OnceLock::new();
static PROFILE_ASKED: OnceLock<std::sync::Mutex<std::collections::BTreeSet<PathBuf>>> =
    OnceLock::new();

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

/// Where `program` keeps its `$PROFILE`, or `None` while it is still being
/// asked.
///
/// The outer `Option` is "has the machine answered"; the inner one is the
/// answer, which is `None` for a shell that could not be started or said
/// nothing. Asking is started by the first call and never repeated: a
/// `$PROFILE` path is a property of an installation, and a build that re-asked
/// per pane would start a process every time somebody split a window.
#[must_use]
pub fn profile_probe(program: &Path) -> Option<Option<PathBuf>> {
    // **The diagnostics door**, in the family of `BT_PSREADLINE_DOCUMENTS` and
    // for exactly its reason: the two verbs below write into a file that belongs
    // to the reader's shell, and exercising them on a development machine must
    // not mean editing that developer's own `$PROFILE`. `BT_POWERSHELL_PROFILE=
    // <file>` moves the whole path — read and write together, so a run cannot
    // report "installed" about one file while writing another.
    if let Some(sandbox) = std::env::var_os("BT_POWERSHELL_PROFILE") {
        let sandbox = PathBuf::from(sandbox);
        if !sandbox.as_os_str().is_empty() {
            return Some(Some(sandbox));
        }
    }
    if let Some(answer) = PROFILE_ANSWERS
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .get(program)
    {
        return Some(answer.clone());
    }
    begin_profile_probe(program);
    None
}

/// Start one program's probe, once per process.
fn begin_profile_probe(program: &Path) {
    let Ok(mut asked) = PROFILE_ASKED.get_or_init(Default::default).lock() else {
        return;
    };
    if !asked.insert(program.to_path_buf()) {
        return;
    }
    drop(asked);
    let program = program.to_path_buf();
    // In the workers' band, beside the PSReadLine probe: this starts a
    // PowerShell to ask it a question, and it must never be the reason a frame
    // was late.
    bt_platform::spawn_at_priority(
        "powershell-profile-probe",
        bt_platform::ThreadPriority::BelowNormal,
        move || {
            let answer = run_profile_probe(&program);
            if let Ok(mut answers) = PROFILE_ANSWERS.get_or_init(Default::default).lock() {
                answers.insert(program, answer);
            }
            // After the answer is published, never before.
            if let Some(wake) = WAKE.get() {
                wake();
            }
        },
    )
    .ok();
}

#[cfg(windows)]
fn run_profile_probe(program: &Path) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    // `CREATE_NO_WINDOW`, for the PSReadLine probe's reason: without it a
    // console flashes on screen the first time a PowerShell pane is opened.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new(program)
        .args(["-NoProfile", "-NonInteractive", "-Command", PROFILE_COMMAND])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_profile_answer(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn run_profile_probe(_program: &Path) -> Option<PathBuf> {
    None
}

/// Read the one line the probe command writes.
///
/// Split out so the reading is testable without a PowerShell, and taken
/// **verbatim**: whatever the shell said is the path, whichever drive it is on
/// and whichever folder — there is no shape this function is entitled to expect,
/// because the answer is exactly the thing this build must not think it knows.
#[must_use]
pub fn parse_profile_answer(stdout: &str) -> Option<PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
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
#[must_use]
pub fn profile_declares_integration(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#') && line.to_ascii_lowercase().contains(SCRIPT_FILE_PS1)
    })
}

/// The line to add, spelled the way the shell can re-derive it.
///
/// `$env:APPDATA` when the script really is under it, and the literal path when
/// it is not. The variable is not decoration: a profile is a file that outlives
/// the account name it was written under, and a line naming
/// `C:\Users\<name>\AppData\Roaming\…` is a line that breaks on a rename or a
/// rebuild while the reader is left looking at a shell with no markers and no
/// error. Where the script is *not* under `%APPDATA%` — a build run with the
/// variable redirected — the literal path is the only true thing to write.
#[must_use]
pub fn integration_line(script: &Path, appdata: Option<&Path>) -> String {
    let spelled = appdata
        .and_then(|appdata| script.strip_prefix(appdata).ok())
        .map_or_else(
            || script.display().to_string(),
            |tail| format!(r"$env:APPDATA\{}", tail.display()),
        );
    format!(". \"{spelled}\"")
}

/// Where PowerShell's script is on this machine, written out on first use.
///
/// [`script_path`]'s sibling, down to the compare-before-write: the two scripts
/// are two halves of one agreement with this build, they ship inside the same
/// executable, and they land in the same directory. What differs is *when* —
/// bash's is written because a bash is starting, and this one is written
/// because somebody pressed the verb that is about to name it.
pub fn script_path_ps1() -> Option<PathBuf> {
    let directory = persist::storage_dir().join(SCRIPT_DIRECTORY);
    let path = directory.join(SCRIPT_FILE_PS1);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == SCRIPT_PS1) {
        return Some(path);
    }
    std::fs::create_dir_all(&directory).ok()?;
    std::fs::write(&path, SCRIPT_PS1).ok()?;
    Some(path)
}

/// What one write into a profile did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileWrite {
    /// The file that now carries the line.
    pub profile: PathBuf,
    /// The copy taken first, or `None` when there was no file to copy.
    pub backup: Option<PathBuf>,
}

/// Add `line` to the end of `profile`, keeping a copy of what was there.
///
/// **The backup is the whole of the discipline.** This is the one place this
/// product writes into a file that belongs to the user's shell rather than to
/// itself, and the answer to "what if I did not want that" has to be a file and
/// not an apology — the same rule this project already follows when it writes
/// `~/.claude/settings.json`. It is named `<profile>.bak-<YYYYMMDD>` and it sits
/// beside the file it copies, so it is found by looking where the change was
/// made rather than by being told where backups go.
///
/// **A backup already taken today is not overwritten.** The copy worth keeping
/// is the first one — the one from before this product touched the file at all —
/// and a second write on the same day would otherwise replace it with a copy
/// that already carries our line.
///
/// The line ending is the file's own: a profile written with LF keeps LF, and a
/// file with neither gets CRLF, which is what every Windows editor puts in a
/// new `.ps1`. A blank line separates what was theirs from what is ours, unless
/// the file is empty — nothing needs separating from nothing.
///
/// **UTF-16 is refused.** A profile saved by an editor that writes UTF-16 is not
/// bytes an ASCII line can be appended to, and a file half in one encoding is a
/// profile that no longer loads. The refusal leaves it exactly as it was, which
/// is the only outcome better than a corrupted one.
pub fn add_to_profile(
    profile: &Path,
    line: &str,
    at: std::time::SystemTime,
) -> std::io::Result<ProfileWrite> {
    let existing = match std::fs::read(profile) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if existing
        .as_deref()
        .is_some_and(|bytes| matches!(bytes, [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..]))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the profile is UTF-16; a line of ASCII cannot be appended to it",
        ));
    }
    let backup = match existing.as_deref() {
        Some(bytes) => {
            let path = backup_path(profile, at);
            if !path.exists() {
                std::fs::write(&path, bytes)?;
            }
            Some(path)
        }
        None => {
            if let Some(parent) = profile.parent() {
                std::fs::create_dir_all(parent)?;
            }
            None
        }
    };
    let mut bytes = existing.unwrap_or_default();
    let newline: &[u8] = if bytes.windows(2).any(|pair| pair == b"\r\n") {
        b"\r\n"
    } else if bytes.contains(&b'\n') {
        b"\n"
    } else {
        b"\r\n"
    };
    // **Blank is empty**, and the user's own two profiles are why: both are a
    // bare `\r\n`, which is what an editor leaves behind when a file is created
    // and never typed into. A blank line separates our line from *theirs*, and
    // there is nothing there to be separated from — but the bytes stay, because
    // nothing here is allowed to delete any part of a file it did not write.
    if bytes.iter().any(|byte| !byte.is_ascii_whitespace()) {
        if !bytes.ends_with(b"\n") {
            bytes.extend_from_slice(newline);
        }
        bytes.extend_from_slice(newline);
    } else if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.extend_from_slice(newline);
    }
    bytes.extend_from_slice(line.as_bytes());
    bytes.extend_from_slice(newline);
    std::fs::write(profile, &bytes)?;
    Ok(ProfileWrite {
        profile: profile.to_path_buf(),
        backup,
    })
}

/// `<profile>.bak-<YYYYMMDD>`, beside the file it copies.
///
/// The day in UTC, which is the calendar this workspace already keeps
/// (`seed::format_iso8601_utc`): it has no time-zone source, and a backup whose
/// name disagreed with the timestamp beside it in Explorer by a few hours is a
/// smaller problem than one whose name was invented from a guess.
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

/// What one pane owes the reader about its shell integration, and what has been
/// done about it (§7.1.6j).
///
/// **On the leaf**, beside the profile and the program, for their reason: it is
/// a fact about *this process* — which PowerShell it is and what its startup
/// file says — so a tear-out carries it with the shell rather than re-deriving
/// it from whichever tab the pane lands in. The strip drawn from it is a
/// projection ([`crate::seats::Seats::set_notices`]); this is the record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Offer {
    /// Nothing to say. This pane is not a PowerShell, or its `$PROFILE` already
    /// dot-sources the script, or the reader has ended the asking.
    #[default]
    Silent,
    /// Owed, and this is the file it is about. Shown once the shell has spoken —
    /// see [`Offer::showing`].
    Owed(PathBuf),
    /// The line has been written into that file.
    Added,
    /// Dismissed with the `×` or with Esc. **Not** the same as [`Self::Silent`]:
    /// nothing was decided, so the next PowerShell is asked again.
    Closed,
}

impl Offer {
    /// Whether a pane holding this offer draws a strip, and which one.
    ///
    /// `spoken` is whether the shell has put anything on its screen at all,
    /// which is the honest reading of "the first prompt has been drawn" for the
    /// one pane that cannot report a prompt: a shell with no integration sends
    /// no `OSC 133;A`, so waiting for one would be waiting forever in exactly
    /// the case the offer exists for.
    ///
    /// `markers_seen` retracts it outright. A `133` on the primary screen is the
    /// integration answering for itself, and it can arrive in a pane this
    /// function said was owed one — a reader who installed the script some other
    /// way, a `$PROFILE` that sources a copy under a name this build does not
    /// recognise, a shell that was restarted after the line was written. The
    /// offer is a claim about what is missing, and evidence that nothing is
    /// missing ends it without anybody pressing anything.
    #[must_use]
    pub fn showing(&self, spoken: bool, markers_seen: bool) -> Option<crate::notice::Notice> {
        if markers_seen {
            return None;
        }
        match self {
            Self::Owed(_) if spoken => Some(crate::notice::Notice::Offer),
            Self::Added => Some(crate::notice::Notice::Added),
            Self::Silent | Self::Owed(_) | Self::Closed => None,
        }
    }

    /// The `$PROFILE` this offer is about, while it is still an offer.
    #[must_use]
    pub fn profile(&self) -> Option<&Path> {
        match self {
            Self::Owed(profile) => Some(profile.as_path()),
            Self::Silent | Self::Added | Self::Closed => None,
        }
    }
}

/// What a pane whose `$PROFILE` is `profile` owes.
///
/// **The file is read here and once.** A `$PROFILE` is read by the shell at
/// startup and by nothing afterwards, so its contents at the moment this pane
/// started are the contents this pane is running under; re-reading it every
/// frame would be asking the disk a question whose answer cannot change what the
/// running shell does. The one thing that *can* change — somebody installing the
/// integration elsewhere — is caught by the marker instead ([`Offer::showing`]),
/// which is evidence rather than a guess.
///
/// The path is passed rather than derived, and that is the whole seam: it comes
/// from [`profile_probe`], which asks the shell, and a test can put any path in
/// front of this function including one on a drive the Documents known folder
/// has never heard of.
#[must_use]
pub fn offer_for(profile: &Path) -> Offer {
    match std::fs::read_to_string(profile) {
        Ok(text) if profile_declares_integration(&text) => Offer::Silent,
        // A profile that is not there is a profile with no line in it, which is
        // the case this offer was written for: a reader who has never had one.
        Ok(_) | Err(_) => Offer::Owed(profile.to_path_buf()),
    }
}

/// Write the line into `profile`, installing the script first if it is not on
/// disk yet.
///
/// Returns what was written and what was copied, or the reason nothing was.
pub fn install_into_profile(
    profile: &Path,
    at: std::time::SystemTime,
) -> std::io::Result<ProfileWrite> {
    let script = script_path_ps1().ok_or_else(|| {
        std::io::Error::other("the integration script could not be written to %APPDATA%")
    })?;
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let line = integration_line(&script, appdata.as_deref());
    add_to_profile(profile, &line, at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{Origin, ProgramSource, index_of_id};

    /// One shipped row, whole — what the spawn path is handed.
    fn row(id: &str) -> Profile {
        profiles::row(index_of_id(id)).expect("a shipped id")
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
    #[test]
    fn the_integration_script_knows_the_name_this_terminal_announces() {
        let declared = bt_pty::TERM_PROGRAM;
        let comparison = format!("$env:TERM_PROGRAM -eq '{declared}'");
        assert!(
            script_source_ps1().contains(&comparison),
            "the terminal declares TERM_PROGRAM={declared:?}, so the script must \
             test for it verbatim; folio.ps1 does not contain {comparison:?}"
        );
        // And it is the *only* spelling the script compares against, so a rename
        // cannot pass by leaving the old literal in a second branch beside it.
        assert_eq!(
            script_source_ps1().matches("$env:TERM_PROGRAM -eq").count(),
            1,
            "the script recognises this terminal in one place, not two"
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
        let command = shell_command(&row("gitbash"), &[], Some(script), &bash_wsl(), &bare());
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
            command
                .environment
                .contains(&(OsString::from("BT_SHELL_INTEGRATION"), OsString::from("1"))),
            "the marker is what makes the script run the login chain it displaced"
        );
        // No script on this machine, and Git Bash is the shell it always was.
        assert_eq!(
            args(&shell_command(
                &row("gitbash"),
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
        let script = Path::new(r"C:\Users\dev\AppData\Roaming\Folio\shell-integration\folio.bash");
        let place = [OsString::from("--cd"), OsString::from("/mnt/d/Developer")];
        let command = shell_command(&row("wsl"), &place, Some(script), &bash_wsl(), &bare());
        assert_eq!(
            args(&command),
            [
                "--cd",
                "/mnt/d/Developer",
                "--",
                "/bin/bash",
                "--init-file",
                "/mnt/c/Users/dev/AppData/Roaming/Folio/shell-integration/folio.bash",
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
                &row("wsl"),
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
                Some(Path::new(r"C:\script.bash")),
                &bash_wsl(),
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
            &row("cmd"),
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
            &row("wsl"),
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
    /// its children, so without the idempotence check a Folio started
    /// from a `cmd` pane prints the directory twice, and one started from
    /// *that* prints it three times.
    #[test]
    fn a_prompt_the_user_already_set_is_kept_and_reported_in_front_of_exactly_once() {
        let theirs = shell_command(
            &row("cmd"),
            &[],
            None,
            &bash_wsl(),
            &Env(vec![("PROMPT", "$T$S$P$G")]),
        );
        assert_eq!(prompt_of(&theirs), r"$e]7;file:///$P$e\$T$S$P$G");

        let again = shell_command(
            &row("cmd"),
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
            &row("cmd"),
            &[],
            None,
            &bash_wsl(),
            &Env(vec![("PROMPT", "")]),
        );
        assert_eq!(prompt_of(&empty), r"$e]7;file:///$P$e\$P$G");
    }

    // -- the profile's own environment (7.1.6c-6c) ---------------------------

    fn value_of(command: &ShellCommand, name: &str) -> Option<String> {
        command
            .environment
            .iter()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    fn spelled(command: &ShellCommand, name: &str) -> usize {
        command
            .environment
            .iter()
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
            Some(Path::new(r"C:\s.bash")),
            &bash_wsl(),
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
            Some(Path::new(r"C:\s.bash")),
            &bash_wsl(),
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
            None,
            &bash_wsl(),
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
                Some(Path::new(r"C:\s.bash")),
                &bash_wsl(),
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
            Some(Path::new(r"C:\s.bash")),
            &bash_wsl(),
            &bare(),
        );
        assert_eq!(value_of(&auto, FORCE_HYPERLINK).as_deref(), Some("1"));
        // And a profile's own `0` beats an inherited `1`, which the declaration
        // would have left alone: this is not a declaration, it is the answer.
        let over_inherited = shell_command(
            &row_with("gitbash", &[(FORCE_HYPERLINK, "0")]),
            &[],
            Some(Path::new(r"C:\s.bash")),
            &bash_wsl(),
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
            let command = shell_command(
                &shut,
                &[],
                Some(Path::new(r"C:\s.bash")),
                &bash_wsl(),
                &bare(),
            );
            assert_eq!(
                command.arguments,
                shut.args.iter().map(OsString::from).collect::<Vec<_>>(),
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
            ("/usr/bin/zsh", Integration::BashInitFile),
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
        let listed = |profile: &Profile, wsl: &WslFacts| {
            value_of(
                &shell_command(profile, &[], Some(script), wsl, &bare()),
                "WSLENV",
            )
        };
        let carried = listed(&row_with("wsl", &[("FOO", "bar")]), &bash_wsl())
            .expect("a WSL profile is told what to carry");
        assert!(carried.contains("FOO/u"), "{carried}");
        assert!(
            carried.contains("BT_SHELL_INTEGRATION/u"),
            "and the terminal's own listing is untouched: {carried}"
        );
        // A name already listed is not listed twice - `FORCE_HYPERLINK` is in
        // the terminal's own five, and a profile that answers the hyperlink
        // question would otherwise put it in the list a second time.
        let answered = listed(&row_with("wsl", &[(FORCE_HYPERLINK, "0")]), &bash_wsl())
            .expect("a WSL profile is told what to carry");
        assert_eq!(answered.matches("FORCE_HYPERLINK").count(), 1, "{answered}");
        // A login that lands in `zsh` reads no init file, so this terminal's own
        // five stay unforwarded - `docs/shell-integration.md` says so in as many
        // words - but the reader's own row is their instruction and crosses
        // anyway.
        let zsh = crate::wsl::test_facts("Ubuntu-24.04", Some("/usr/bin/zsh"));
        let theirs =
            listed(&row_with("wsl", &[("FOO", "bar")]), &zsh).expect("their row still crosses");
        assert!(theirs.contains("FOO/u"), "{theirs}");
        assert!(!theirs.contains("BT_SHELL_INTEGRATION"), "{theirs}");
        // A profile with nothing of its own says nothing new, so nothing is
        // listed that was not listed before this slice.
        assert!(listed(&row("wsl"), &zsh).is_none());
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
        let command = shell_command(&theirs, &[], None, &bash_wsl(), &bare());
        assert_eq!(command.arguments, [OsString::from("--verbose")]);
        assert_eq!(
            value_of(&command, "ANTHROPIC_LOG").as_deref(),
            Some("debug")
        );
        assert_eq!(value_of(&command, "PROMPT"), None);
        assert_eq!(value_of(&command, FORCE_HYPERLINK).as_deref(), Some("1"));
    }

    // ── the PowerShell profile (§7.1.6j) ───────────────────────────────────

    fn temp_dir(tag: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("folio-ps-profile-{tag}-{}-{n}", std::process::id()));
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
    #[test]
    fn the_profile_is_the_file_the_shell_names_and_never_one_this_build_composed() {
        let composed_root = Path::new(r"C:\Users\me\Documents");
        let answer =
            parse_profile_answer("D:\\Documents\\PowerShell\\Microsoft.PowerShell_profile.ps1\r\n")
                .expect("the shell answered");
        assert_eq!(
            answer,
            PathBuf::from(r"D:\Documents\PowerShell\Microsoft.PowerShell_profile.ps1")
        );
        assert!(
            !answer.starts_with(composed_root),
            "the answer is not under the folder a composing build would have used"
        );
        assert_eq!(parse_profile_answer("   \r\n"), None);
        assert_eq!(parse_profile_answer(""), None);
    }

    /// A shell whose file already carries the line is asked nothing. This is the
    /// state the machine this was written on is in, and a build that offered
    /// here would be offering to install something that is installed.
    #[test]
    fn a_profile_that_already_loads_the_script_owes_nothing() {
        let documents = temp_dir("installed");
        let profile = documents.join(PROFILE_LEAF);
        std::fs::write(
            &profile,
            "\n# Folio OSC 133 shell integration (opt-in)\n. \
             'D:\\Developer\\folio-terminal\\scripts\\shell-integration\\folio.ps1'\n",
        )
        .unwrap();
        assert_eq!(offer_for(&profile), Offer::Silent);
        let absent = documents.join("never-written.ps1");
        assert_eq!(
            offer_for(&absent),
            Offer::Owed(absent.clone()),
            "a profile that is not there is a profile with no line in it"
        );
    }

    /// A strip is drawn once the shell has spoken, and it takes itself down the
    /// moment the integration speaks for itself — however it came to be
    /// installed.
    #[test]
    fn the_offer_waits_for_the_shell_and_retires_when_a_marker_arrives() {
        let owed = Offer::Owed(PathBuf::from(r"D:\Documents\PowerShell\p.ps1"));
        assert_eq!(
            owed.showing(false, false),
            None,
            "nothing is said over a pane that has not drawn a prompt yet"
        );
        assert_eq!(
            owed.showing(true, false),
            Some(crate::notice::Notice::Offer)
        );
        assert_eq!(
            owed.showing(true, true),
            None,
            "a 133 on the primary screen is the integration answering for itself"
        );
        assert_eq!(
            Offer::Added.showing(false, false),
            Some(crate::notice::Notice::Added)
        );
        assert_eq!(Offer::Added.showing(true, true), None);
        assert_eq!(Offer::Closed.showing(true, false), None);
        assert_eq!(Offer::Silent.showing(true, false), None);
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
        assert!(profile_declares_integration(
            "Set-Alias ll Get-ChildItem\r\n. \"$env:APPDATA\\Folio\\shell-integration\\folio.ps1\"\r\n"
        ));
        assert!(
            profile_declares_integration(
                ". 'D:\\Developer\\folio-terminal\\scripts\\shell-integration\\FOLIO.PS1'\n"
            ),
            "the machine is case-insensitive about file names and so is this"
        );
        assert!(!profile_declares_integration(
            "#   . 'D:\\path\\to\\folio\\scripts\\shell-integration\\folio.ps1'\n"
        ));
        assert!(!profile_declares_integration("\r\n"));
        assert!(!profile_declares_integration(""));
    }

    /// The line names the script through `$env:APPDATA` when that is where it
    /// is, so the profile keeps working for a user whose account is renamed or
    /// whose machine is rebuilt.
    #[test]
    fn the_line_spells_the_script_the_way_the_shell_can_re_derive_it() {
        assert_eq!(
            integration_line(
                Path::new(r"C:\Users\me\AppData\Roaming\Folio\shell-integration\folio.ps1"),
                Some(Path::new(r"C:\Users\me\AppData\Roaming"))
            ),
            r#". "$env:APPDATA\Folio\shell-integration\folio.ps1""#
        );
        assert_eq!(
            integration_line(Path::new(r"D:\scratch\folio.ps1"), None),
            r#". "D:\scratch\folio.ps1""#
        );
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
        assert!(profile_declares_integration(
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

    /// A profile in UTF-16 is refused rather than appended to: these bytes are
    /// not text this function can add a line of ASCII to, and a file half in one
    /// encoding is a profile that no longer loads.
    #[test]
    fn a_utf16_profile_is_refused_rather_than_corrupted() {
        let documents = temp_dir("utf16");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(b"#\0 \0m\0i\0n\0e\0");
        std::fs::write(&profile, &bytes).unwrap();
        assert!(add_to_profile(&profile, LINE, EPOCH_DAY).is_err());
        assert_eq!(
            std::fs::read(&profile).unwrap(),
            bytes,
            "a refusal leaves the file exactly as it was"
        );
    }

    /// The first backup of a day is the one worth keeping: a second write must
    /// not overwrite the pristine copy with one that already carries our line.
    #[test]
    fn a_backup_already_taken_today_is_not_overwritten() {
        let documents = temp_dir("twice");
        let profile = documents.join("PowerShell").join(PROFILE_LEAF);
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(&profile, "# mine\n").unwrap();
        let first = add_to_profile(&profile, LINE, EPOCH_DAY)
            .expect("the write")
            .backup
            .expect("a backup");
        add_to_profile(&profile, LINE, EPOCH_DAY).expect("the second write");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "# mine\n");
    }

    /// What the two constants above stand for: one day, and one line.
    const EPOCH_DAY: std::time::SystemTime = std::time::UNIX_EPOCH;
    const LINE: &str = r#". "$env:APPDATA\Folio\shell-integration\folio.ps1""#;
    /// The name both PowerShells give the file, for the tests that stand one up
    /// rather than asking a shell where it is.
    const PROFILE_LEAF: &str = "Microsoft.PowerShell_profile.ps1";
}
