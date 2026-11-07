//! **Installing codex's `Stop` hook, into the user's own configuration and nowhere else.**
//!
//! One settings row presses this, and it is [`attention_hooks`](crate::attention_hooks)'s contract
//! over a second upstream: only the user-level files are ever written, a file this build cannot
//! read is never written over, and installing and then uninstalling hands back the document that
//! was there — every byte that is not Folio's, comments, key order and blank lines included.
//!
//! # What is installed, and why it is a hook (0.4.10 plan row 69)
//!
//! `docs/plans/attention/evidence-cli-survey-2026-08-25.md` §2.6 surveyed codex's two surfaces. The
//! older one, `notify`, is a fire-and-forget external program on **one key**: a person who already
//! runs a program of their own there — Codex's own computer use writes one — left Folio nothing to
//! install but a refusal, and a terminal that wrote over that line would delete a program it cannot
//! give back. So Folio installs on the other surface, codex's lifecycle **hooks**, where every
//! program has an entry of its own (the coordinator's rulings R12–R19 of 2026-11-04, row 69's
//! report §1b):
//!
//! ```json
//! { "hooks": { "Stop": [ { "hooks": [
//!   { "async": true, "command": "& '<folio.exe>' attention codex:stop --json -",
//!     "timeout": 10, "type": "command" }
//! ] } ] } }
//! ```
//!
//! - **`hooks.json`, beside `config.toml`**, which has Claude Code's nesting — so the reader, the
//!   edits and the self-check are [`attention_hooks`](crate::attention_hooks)'s, through the
//!   [`HOOKS`] family. Never config.toml's own `[hooks]` list (R15): when that file already holds
//!   hook events, codex would warn about two representations on every start, so nothing is written
//!   and the refusal says why ([`declares_toml_hooks`]).
//! - **`Stop`**, which fires once per root turn with `session_id`, `cwd` and
//!   `last_assistant_message` on stdin — what the turn-end mark, its words and row 195's resume read
//!   ([`attention_map::TURN_END`](crate::attention_map::TURN_END)). It reaches the event lane and
//!   never the queue, as `notify` did.
//! - **`async`**, because a synchronous `Stop` hook holds the turn's end on Folio's verb, and on one
//!   an exit of 2 with text on stderr is fed back to the model as a continuation (R12). codex runs
//!   asynchronous command hooks from 0.148.0.
//! - **One string run through the session's shell**, codex's only form for a command hook
//!   (`build_hooks_config` at `rust-v0.166.0`, from `default_user_shell`): PowerShell on Windows —
//!   `pwsh`, else `powershell.exe` — as `-NoProfile -Command <string>`, so the program is called
//!   with `&` and its path is one single-quoted literal, inside which `%`, `$`, backticks and `&`
//!   are plain; the person's login shell elsewhere, as `-c <string>`, with the path in sh's single
//!   quotes ([`hook_command`]). Codex falls back to `%COMSPEC% /C` and `$SHELL -lc` only for a
//!   session with no shell of its own; that launcher does not run the PowerShell form, and nor does
//!   a Unix account whose login shell is PowerShell (`docs/DESIGN.md`, row 69's entry).
//!
//! **codex runs a hook only once the person has approved it** in codex, which asks at its next start;
//! a changed command asks again. That approval is codex's record of the person's decision, keyed by
//! the hook's place in the file, and Folio never writes it (R13). What Folio owes it is the place:
//! the hook is appended after the person's groups, and taken out without moving theirs (R16, see
//! [`attention_hooks::Family::positional`](crate::attention_hooks::Family::positional)).
//!
//! # The `notify` line an earlier Folio wrote
//!
//! ```toml
//! notify = ["<folio.exe>", "attention", "codex:agent-turn-complete", "--json"]
//! ```
//!
//! Folio 0.2–0.4.9 installed that line, and a machine still carrying it keeps it until Folio has to
//! write its entry anyway (R18): the switch pressed off or on, or a repair. Then the line goes in the
//! same operation as the hook is written, by the same byte-keeping rule — so one turn is never
//! marked twice. A `notify` Folio did not write is the person's and is never touched.
//!
//! # Which of the two, by codex's version (0.4.10 plan row 69c)
//!
//! codex runs an asynchronous command hook from 0.148.0; before it, `discovery.rs` skips one
//! ("async hooks are not supported yet", at `rust-v0.147.0`), so a hook written there would never
//! run. So the version decides the entry (R12): **the hook on codex 0.148 and later, the `notify`
//! line below it** — and on a version nobody could learn, the hook (R17). The version is asked of
//! the program the program walk found for the `codex` row, as `codex --version`, on the start-time
//! scan's worker and never on the window thread, and the answer is kept against that program's
//! path, size and modified time, so it is asked again only when codex changed ([`settle_version`]).
//!
//! The answer picks the **file** an install is pressed on ([`entry_path`]): `hooks.json` for the
//! hook, `config.toml` for the line. Everything after that follows the file it was handed — the
//! scan ([`scan_resolved`]) and the writer ([`apply_resolved`]) — so one operation never reads one
//! road and writes the other. A removal is the same on every version: it takes Folio's hook out of
//! `hooks.json` and an earlier Folio's line out of `config.toml` together, because a codex that
//! is upgraded later would run whatever was left.
//!
//! A hook codex will not run is never reported as on (review N6): not below 0.148, not in a
//! `hooks.json` codex would ignore ([`read_by_codex`]), and not where `config.toml` switches hooks
//! off ([`hooks_switched_off`]). In each of those the install writes nothing and says why, and so
//! nothing takes out a working `notify` line in their place either.
//!
//! # Where the files are, and why it is asked for rather than composed
//!
//! §7.1.6j's lesson, the same one [`attention_hooks`](crate::attention_hooks) states: a machine with
//! a redirected profile folder gets told "not installed" about something installed for months, if
//! the path was *composed* rather than asked for. `CODEX_HOME` is the variable codex itself reads —
//! the survey's own harness set it to point the recordings at the real installation — so it is what
//! is read here, and only otherwise the documented default beside the user's profile.

pub(crate) use crate::attention_ownership::Outcome;
use crate::attention_ownership::{self as ownership, Decision, Form, HooksFound, RowSentence};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use bt_platform::HostPlatform;
use serde_json::{Map, Value as Json};
use toml_edit::{Array, DocumentMut, ImDocument, Item, Table, Value};

use crate::attention_copilot::Version;
use crate::attention_hooks::{self as hooks, Config, Family, Standing, Wanted};

use crate::attention_map::{self, CODEX};
use crate::cli::{ATTENTION_VERB, STDIN_PAYLOAD};

/// The variable codex reads for its configuration directory.
const HOME_VARIABLE: &str = "CODEX_HOME";

/// The file inside it that holds **user-level** configuration.
const CONFIG_FILE: &str = "config.toml";

/// The file beside it that codex reads the user's lifecycle hooks from — the one Folio's hook is
/// written in.
const HOOKS_FILE: &str = "hooks.json";

/// The default directory's name beside the user's profile, for when the variable says nothing.
const DEFAULT_DIRECTORY: &str = ".codex";

/// The key codex spawns a program from at the end of a turn.
const NOTIFY_KEY: &str = "notify";

/// The event a `notify` program is told about, in the wire's `<family>:<event>` spelling.
///
/// The only `type` the survey ever observed in a `notify` payload, quoted from
/// `evidence-cli-survey-2026-08-25.md` §2.6 ①, and already a row of
/// [`attention_map::TURN_END`](crate::attention_map::TURN_END).
const EVENT: &str = "agent-turn-complete";

/// The flag the payload lands on. See the module header.
const JSON_FLAG: &str = "--json";

/// The hook event Folio's entry hangs on, as `hooks.json` spells it.
const STOP_EVENT: &str = "Stop";

/// The same event in the wire's `<family>:<event>` spelling — a row of both of
/// [`attention_map`](crate::attention_map)'s tables.
const STOP: &str = "stop";

/// **How long codex lets Folio's hook run, in seconds.** The verb reads its payload for at most a
/// fifth of a second and writes one line; codex's own default is ten minutes, and a hook that hung
/// would hold a process for all of them.
const HOOK_TIMEOUT: u64 = 10;

/// The keys of the entry Folio writes, and so the only keys an entry of Folio's may carry. One
/// with any other — `commandWindows`, which codex runs in place of `command` on Windows, or
/// `statusMessage` — is somebody's edit, and it is left as it is.
const HOOK_KEYS: [&str; 4] = ["type", "command", "async", "timeout"];

/// **The hook events codex reads under config.toml's `[hooks]`, and in `hooks.json`**
/// (`HookEventsToml` at `rust-v0.166.0`, [`CodexHookEvents`]). `[hooks.state]`, where codex keeps
/// the person's approvals, is not one of them, so a table holding only that is not a second
/// representation.
const TOML_HOOK_EVENTS: [&str; 14] = [
    "PreModelRequest",
    "PostModelRequest",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "SubagentStart",
    "SubagentStop",
    "Stop",
    "Interrupt",
];

/// The built-in profile row whose program is codex (`profiles::windows_shipped`): the program the
/// version is asked of (R17), and the program walk moving it is a reason to ask again.
pub(crate) const PROFILE_ID: &str = "codex";

/// **The oldest codex that runs an asynchronous command hook** (R12): `discovery.rs` at
/// `rust-v0.147.0` skips one, and `rust-v0.148.0` runs it. Below it — a prerelease of it too, which
/// semver orders before it ([`CodexRelease::runs_hooks`]) — Folio's entry is the `notify` line.
const HOOKS_FLOOR: Version = Version::new(0, 148, 0);

/// **How long `codex --version` may take** before it is stopped and the version is unknown for
/// this round (row 69's report §1b.6): copilot's [`PROBE_DEADLINE`](crate::attention_copilot)
/// (the 69c review's C2), because both are a start of a program under whatever load the machine
/// has, and an answer stopped early is asked again ([`Asked::Overran`]).
const VERSION_DEADLINE: Duration = Duration::from_secs(10);

/// The key of `config.toml`'s feature switches, and the two names codex reads its hooks switch by:
/// `hooks`, and the `codex_hooks` it was called before (`features/src/lib.rs` at `rust-v0.166.0`,
/// `Feature::CodexHooks`).
const FEATURES_KEY: &str = "features";
const HOOKS_FEATURE: &str = "hooks";
const HOOKS_FEATURE_FORMERLY: &str = "codex_hooks";

/// **What is known about this machine's codex version.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CodexVersion {
    /// Nothing has been asked yet: the program walk has not said where codex is, or the program
    /// it named has not been asked.
    Unasked,
    /// No codex where the walk looked, or one that would not say — it failed to start, ran past
    /// [`VERSION_DEADLINE`], exited with a failure, or printed no version line
    /// ([`CodexRelease::parse`]).
    Unknown,
    /// What `codex --version` printed.
    Known(CodexRelease),
}

impl CodexVersion {
    /// **Whether Folio's entry is the hook** (R12): on every answer but a release proved older than
    /// [`HOOKS_FLOOR`] — one nobody could learn is the current one (R17).
    #[must_use]
    pub(crate) fn runs_hooks(&self) -> bool {
        match self {
            Self::Known(release) => release.runs_hooks(),
            Self::Unasked | Self::Unknown => true,
        }
    }
}

/// **A codex release, as `codex --version` names it** (the 69c review's C1): its version, and the
/// prerelease identifiers after a `-`, which semver orders before the release they lead up to —
/// `0.148.0-alpha.5` is older than `0.148.0` (semver 2.0.0 §11). Build metadata after a `+` orders
/// nothing and is not kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexRelease {
    version: Version,
    prerelease: Option<String>,
}

impl CodexRelease {
    /// **Whether this release runs Folio's hook**: one at or past [`HOOKS_FLOOR`] by semver's
    /// order — a prerelease of 0.148.0 comes before 0.148.0 and is held to the `notify` line, and
    /// a prerelease of a later version (`0.149.0-alpha.1`) is past it.
    #[must_use]
    fn runs_hooks(&self) -> bool {
        self.version > HOOKS_FLOOR || (self.version == HOOKS_FLOOR && self.prerelease.is_none())
    }

    /// Whether this release is older than [`HOOKS_FLOOR`] — what the row's sentence names.
    #[must_use]
    fn below_floor(&self) -> bool {
        !self.runs_hooks()
    }

    /// **The version line codex prints, and nothing else** (the 69c review's C5): the first line of
    /// the answer that is not blank must be `codex-cli <semver>` whole — two words, the second a
    /// version by [`semver`](Self::semver). A launcher or wrapper that prints its own version
    /// first, or a sentence with a number in it, is not believed: the version is then unknown,
    /// which runs the hook (R17), rather than a stranger's number read as codex's.
    #[must_use]
    fn parse(stdout: &str) -> Option<Self> {
        let line = stdout.lines().find(|line| !line.trim().is_empty())?;
        let mut words = line.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("codex-cli"), Some(release), None) => Self::semver(release),
            _ => None,
        }
    }

    /// `major.minor.patch`, then an optional `-prerelease` and `+build` (semver 2.0.0 §2, §9, §10):
    /// numbers without leading zeros, identifiers of ASCII letters, digits and hyphens, none empty,
    /// and no numeric prerelease identifier with a leading zero.
    #[must_use]
    fn semver(text: &str) -> Option<Self> {
        let (rest, build) = match text.split_once('+') {
            Some((rest, build)) => (rest, Some(build)),
            None => (text, None),
        };
        let (core, prerelease) = match rest.split_once('-') {
            Some((core, prerelease)) => (core, Some(prerelease)),
            None => (rest, None),
        };
        if build.is_some_and(|build| !Self::identifiers(build, false))
            || prerelease.is_some_and(|prerelease| !Self::identifiers(prerelease, true))
        {
            return None;
        }
        let mut parts = core.split('.').map(Self::number);
        let version = Version::new(parts.next()??, parts.next()??, parts.next()??);
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            version,
            prerelease: prerelease.map(str::to_owned),
        })
    }

    /// One of semver's numbers: digits only, and no leading zero unless it is `0`.
    #[must_use]
    fn number(part: &str) -> Option<u32> {
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        if !digits || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        part.parse().ok()
    }

    /// Dot-separated identifiers, none empty, of ASCII letters, digits and hyphens — and, in a
    /// prerelease (`numbers_bare`), a numeric one without a leading zero.
    #[must_use]
    fn identifiers(text: &str, numbers_bare: bool) -> bool {
        text.split('.').all(|identifier| {
            let allowed = !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
            let numeric = identifier.bytes().all(|byte| byte.is_ascii_digit());
            allowed
                && !(numbers_bare && numeric && identifier.len() > 1 && identifier.starts_with('0'))
        })
    }
}

/// `major.minor.patch`, and `-<prerelease>` when there is one: what the row's sentence and the
/// diagnostics line name. No spaces, so the diagnostics line stays one field.
impl std::fmt::Display for CodexRelease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.prerelease {
            Some(prerelease) => write!(formatter, "{}-{prerelease}", self.version),
            None => write!(formatter, "{}", self.version),
        }
    }
}

/// **The answer a version question came back with** (the 69c review's C2): one codex gave —
/// a release, or none — or a question stopped at [`VERSION_DEADLINE`], which says nothing about
/// the program and is not kept, so the next round asks again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Asked {
    /// It ran to its end: the release it named, or `None` for a failure or no version line.
    Answered(Option<CodexRelease>),
    /// It was stopped at the deadline.
    Overran,
}

impl Asked {
    /// **How `codex --version` ended, read by codex's rule** (C2, C5): a run that finished with
    /// success, and only then its version line ([`CodexRelease::parse`]); a failed exit is no
    /// version whatever it printed. Not asked at all (off Windows, a `cmd.exe` that would not
    /// start) is an answer of none.
    #[must_use]
    fn of(ending: Option<bt_platform::ProbeEnding>) -> Self {
        match ending {
            Some(bt_platform::ProbeEnding::Finished(output)) => Self::Answered(Self::release_from(
                output.status.success(),
                &String::from_utf8_lossy(&output.stdout),
            )),
            Some(bt_platform::ProbeEnding::Overran) => Self::Overran,
            None => Self::Answered(None),
        }
    }

    /// The release a finished run names: its version line, on a successful exit only.
    #[must_use]
    fn release_from(succeeded: bool, stdout: &str) -> Option<CodexRelease> {
        if succeeded {
            CodexRelease::parse(stdout)
        } else {
            None
        }
    }
}

/// The program a version was asked of, as it stood when it was asked — the cache's key (R17).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramStamp {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

/// **The version question's book**: the walk's answer about the `codex` row, and the last program
/// asked with what it said.
pub(crate) struct VersionBook {
    /// `None` until the walk has answered about the row; then its program, `None` for none.
    walk: Option<Option<PathBuf>>,
    /// The program asked last, stamped as it stood, and its answer.
    asked: Option<(ProgramStamp, Asked)>,
}

impl VersionBook {
    pub(crate) const fn new() -> Self {
        Self {
            walk: None,
            asked: None,
        }
    }

    /// The version as this book has it: the answer held for the walk's program, if that is the
    /// program it was asked of — unknown for a question that overran, until it is asked again.
    fn version(&self) -> CodexVersion {
        match &self.walk {
            None => CodexVersion::Unasked,
            Some(None) => CodexVersion::Unknown,
            Some(Some(program)) => match &self.asked {
                Some((stamp, asked)) if &stamp.path == program => asked.version(),
                _ => CodexVersion::Unasked,
            },
        }
    }

    /// **Whether the version question has an answer** (the final review of row 69, F1): the walk
    /// has answered about the row, and either it found no codex or the program it named was asked
    /// and the question ran to its end. A question stopped at [`VERSION_DEADLINE`] is not an
    /// answer — [`Self::version`] reads it as unknown for presses and rows (R17), but nothing
    /// unattended is written on it.
    fn answered(&self) -> bool {
        match &self.walk {
            None => false,
            Some(None) => true,
            Some(Some(program)) => matches!(
                &self.asked,
                Some((stamp, Asked::Answered(_))) if &stamp.path == program
            ),
        }
    }
}

impl Asked {
    /// The version this answer gives.
    fn version(&self) -> CodexVersion {
        match self {
            Self::Answered(Some(release)) => CodexVersion::Known(release.clone()),
            Self::Answered(None) | Self::Overran => CodexVersion::Unknown,
        }
    }
}

/// This process's book.
static VERSION_BOOK: Mutex<VersionBook> = Mutex::new(VersionBook::new());

fn book(book: &Mutex<VersionBook>) -> std::sync::MutexGuard<'_, VersionBook> {
    book.lock().unwrap_or_else(PoisonError::into_inner)
}

/// **The program walk's answer about the `codex` row, adopted** (`ProfilePrograms::answer`'s
/// shape: `None` while the row has no answer). `true` when it differs from the one held — the
/// version may be another now, and the start-time scan runs again for it.
pub(crate) fn adopt_walk_program(answer: Option<Option<&OsStr>>) -> bool {
    adopt_walk_program_in(&VERSION_BOOK, answer)
}

/// The same, into a named book.
pub(crate) fn adopt_walk_program_in(
    held: &Mutex<VersionBook>,
    answer: Option<Option<&OsStr>>,
) -> bool {
    let walk = answer.map(|program| program.map(PathBuf::from));
    let mut held = book(held);
    if held.walk == walk {
        return false;
    }
    held.walk = walk;
    true
}

/// **The version, as the newest scan settled it** — what a press, a row and the first-run card
/// read. Never runs a program: the window thread does not wait on codex.
#[must_use]
pub(crate) fn version() -> CodexVersion {
    book(&VERSION_BOOK).version()
}

/// **Settle the version, on the start-time scan's worker** (R17): `codex --version` asked of the
/// walk's program unless the answer held is of the same file — the same path, size and modified
/// time — and an answer that overran its deadline is never the one held, so the next round asks
/// again (the 69c review's C2). Each answer it asks for is one diagnostics line,
/// `BT_AGENT_HOOKS codex version=<it>`, and `unknown` when there was none to read.
pub(crate) fn settle_version(ctx: &bt_platform::admission::WorkerCtx) -> CodexVersion {
    settle_version_in(
        &VERSION_BOOK,
        &|program| stamp_of(ctx, program),
        &|program| {
            Asked::of(crate::attention_copilot::program_version(
                ctx,
                program,
                VERSION_DEADLINE,
                "codex",
            ))
        },
        &crate::diagnostics::stderr_line,
    )
}

/// [`settle_version`] with the book, the machine and the voice handed in — the seam the tests
/// press. The question is asked with the book let go: it may take [`VERSION_DEADLINE`], and a
/// press reading [`version`] meanwhile is answered from what was held.
pub(crate) fn settle_version_in(
    held: &Mutex<VersionBook>,
    stamp: &dyn Fn(&Path) -> Option<ProgramStamp>,
    ask: &dyn Fn(&Path) -> Asked,
    say: &dyn Fn(String),
) -> CodexVersion {
    let program = match &book(held).walk {
        None => return CodexVersion::Unasked,
        Some(None) => return CodexVersion::Unknown,
        Some(Some(program)) => program.clone(),
    };
    // A program that cannot be looked at cannot be asked either: unknown, and nothing kept.
    let Some(stamp) = stamp(&program) else {
        book(held).asked = None;
        return CodexVersion::Unknown;
    };
    // An answer of the same file is kept; a question stopped at the deadline is not an answer.
    if let Some((asked, answer @ Asked::Answered(_))) = &book(held).asked
        && asked == &stamp
    {
        return answer.version();
    }
    let answer = ask(&program);
    say(format!(
        "BT_AGENT_HOOKS codex version={}",
        match &answer {
            Asked::Answered(Some(release)) => release.to_string(),
            Asked::Answered(None) | Asked::Overran => "unknown".to_owned(),
        }
    ));
    let mut held = book(held);
    held.asked = Some((stamp, answer));
    held.version()
}

/// The program as it stands on disk, or `None` when it cannot be looked at: a worker's door, the
/// one look at the disk the version book takes on the carry worker.
fn stamp_of(_worker: &bt_platform::admission::WorkerCtx, program: &Path) -> Option<ProgramStamp> {
    let metadata = std::fs::metadata(program).ok()?;
    Some(ProgramStamp {
        path: program.to_path_buf(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// **Whether the start-time repair may write codex's entry now** (0.4.10 plan row 69c; row 67b's
/// R9 for Copilot, over this family): only once the version has been asked and answered — a
/// repair on a version nobody asked yet could write a hook a codex below 0.148 skips, in place of
/// the `notify` line that works there. A question that overran is not an answer (the final review
/// of row 69, F1): the gate stays closed until a later round's question runs to its end.
pub(crate) fn repair_within_the_gate() -> bool {
    repair_within_the_gate_in(&VERSION_BOOK)
}

/// The same question of a named book — the seam the step's test presses (the 69c review's C4).
pub(crate) fn repair_within_the_gate_in(held: &Mutex<VersionBook>) -> bool {
    book(held).answered()
}

/// **codex's `hooks.json`, as [`attention_hooks`](crate::attention_hooks) reads and writes it.**
pub(crate) const HOOKS: Family = Family {
    owner: codex_hook_owner,
    wanted: hook_wanted,
    todays: hook_entry,
    // The program and its arguments are one line here; `async` and `timeout` are the person's to
    // set (ruling N1).
    program_keys: &["command"],
    positional: true,
    agent: CODEX,
};

/// Whether this machine's user configuration already calls Folio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    /// Neither file names this copy — or there are no files at all, which is the same answer to
    /// the only question being asked.
    Absent,
    /// This copy's hook is there, or the `notify` line an earlier Folio wrote.
    Installed,
    /// There is a file and it could not be read. **Not** "absent": writing over a file this build
    /// cannot parse would destroy configuration somebody wrote by hand.
    Unreadable,
    /// `hooks.json` is readable and **this build will not edit it**, for the reason carried: a
    /// link out of the agent's own folder, a file shared by hard links, or a read-only one. See
    /// `attention_hooks::Standing::Refused`.
    Refused(&'static str),
}

/// The directory codex keeps user configuration in, as **this environment** says it.
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        crate::attention_hooks::agent_variable(HOME_VARIABLE),
        crate::attention_hooks::agent_variable(bt_platform::home_variable()),
    )
}

/// The same decision, with the environment handed in.
///
/// **`home` is `%USERPROFILE%` on Windows and `$HOME` everywhere else**, and it is
/// [`bt_platform::home_variable`] that decides which — M2-6's audit finding and M4-7's to fix: a
/// path composed out of `%USERPROFILE%` alone is `None` on a Mac, so this whole module answered
/// "not installed" on every machine where the hooks were installed. The question is asked of
/// `bt-platform` rather than of a `cfg` here, which is the rule
/// `only_the_named_files_decide_what_platform_this_is` keeps and the reason this file is not on
/// that list.
///
/// Split out for [`attention_hooks`](crate::attention_hooks)'s reason: a process-wide variable
/// changed from a test is changed for every other test running beside it, and this crate refuses
/// `unsafe`, which is what `set_var` now is.
#[must_use]
fn config_dir_from(named: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    Some(PathBuf::from(home.filter(|home| !home.is_empty())?).join(DEFAULT_DIRECTORY))
}

/// The user-level hooks file. **The file this module writes Folio's hook in.**
#[must_use]
pub(crate) fn hooks_path() -> Option<PathBuf> {
    Some(config_dir()?.join(HOOKS_FILE))
}

/// The user-level configuration file: where the `notify` line is, on a codex below 0.148.
#[must_use]
pub(crate) fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

/// **The file Folio's entry goes in on this machine's codex**: `hooks.json`, or `config.toml` on a
/// version proved below 0.148 (R12, R17) — the file an install is pressed on, and the one the
/// start-time scan reads.
#[must_use]
pub(crate) fn entry_path() -> Option<PathBuf> {
    if version().runs_hooks() {
        hooks_path()
    } else {
        config_path()
    }
}

/// **The file Folio's entry goes in, spelled for a reader who is being asked to consent to it** —
/// [`crate::attention_hooks::settings_path_shown`]'s rule over `CODEX_HOME`, for the file
/// [`entry_path`] names.
#[must_use]
pub(crate) fn entry_path_shown() -> String {
    let file = if version().runs_hooks() {
        HOOKS_FILE
    } else {
        CONFIG_FILE
    };
    path_shown_from(
        file,
        crate::attention_hooks::agent_variable(HOME_VARIABLE),
        crate::attention_hooks::agent_variable(bt_platform::home_variable()),
    )
}

/// The same decision, with the file and the environment handed in.
#[must_use]
fn path_shown_from(file: &str, named: Option<OsString>, home: Option<OsString>) -> String {
    let default = || format!("~/{DEFAULT_DIRECTORY}/{file}");
    if named.as_ref().is_none_or(|named| named.is_empty()) {
        return default();
    }
    config_dir_from(named, home)
        .map(|dir| dir.join(file).display().to_string())
        .unwrap_or_else(default)
}

/// **The first-run card's tooltip for the Codex row**: the file this machine will write, and what.
#[must_use]
pub(crate) fn first_run_tip() -> String {
    if version().runs_hooks() {
        crate::i18n::first_run_tip_codex(&entry_path_shown())
    } else {
        crate::i18n::first_run_tip_codex_notify(&entry_path_shown())
    }
}

/// **The Agents row's sentence**: the hook's, or — on a codex proved below 0.148 — the one that
/// names that version and the `notify` line written in its place.
#[must_use]
pub(crate) fn row_description() -> &'static str {
    match version() {
        CodexVersion::Known(release) if release.below_floor() => {
            crate::i18n::codex_row_old_version(&release.to_string())
        }
        _ => crate::i18n::Text::DescCodexHooks.text(),
    }
}

/// **The notice a repair of a gone copy's entry raises**, by the file it wrote: the hook's says
/// codex asks once to trust the new path; the line's needs no approval.
#[must_use]
pub(crate) fn repaired_notice(file: &Path) -> crate::i18n::Text {
    if file.file_name() == Some(OsStr::new(CONFIG_FILE)) {
        crate::i18n::Text::AgentHooksRepairedCodex
    } else {
        crate::i18n::Text::AgentHooksRepairedCodexHook
    }
}

/// **The file a press writes Folio's entry in** — and the key its take-over's consent is kept by:
/// an install's [`entry_path`], and `hooks.json` for a removal, which takes out both entries.
#[must_use]
pub(crate) fn pressed_path(install: bool) -> Option<PathBuf> {
    if install { entry_path() } else { hooks_path() }
}

/// **What an install that wrote says**, by the file it wrote: the hook's toast tells the reader
/// codex asks once to trust it (R13); the line's needs no approval.
#[must_use]
pub(crate) fn added_toast(file: Option<&Path>) -> crate::i18n::Text {
    if file.is_some_and(|file| file.file_name() == Some(OsStr::new(CONFIG_FILE))) {
        crate::i18n::Text::CodexNotifyAddedToast
    } else {
        crate::i18n::Text::CodexHookAddedToast
    }
}

/// **Whether the first-run card may offer the Codex row** (row 69's report §1.7, R19): nothing of
/// Folio's is there yet, and an install of this copy's entry in the file this codex runs it from
/// ([`entry_path`]) would not be refused — [`install_refusal_at`], the install's own refusals
/// asked of the same files (the 69c review's C3), so the card offers nothing a press would then
/// refuse. Another live copy's entry is not a refusal: the press asks about it.
#[must_use]
pub(crate) fn first_run_installable() -> bool {
    state() == State::Absent
        && entry_path()
            .zip(ownership::this_copy())
            .is_some_and(|(path, exe)| install_refusal_at(&path, &exe, &version()).is_none())
}

/// What the two files say today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(dir) = config_dir() else {
        return State::Absent;
    };
    codex_state_in(
        &dir,
        ownership::this_copy().as_deref(),
        version().runs_hooks(),
    )
}

/// **The settings row's two facts, out of one read of the files.**
///
/// Whether this copy's marks are in them, and — when this build will not edit `hooks.json` at all
/// — the reason the row says in place of `Off`. Two answers to "what does the row show" derived
/// from one `State` rather than two reads, because a second read is a second answer (closure
/// review R1).
#[must_use]
pub(crate) fn row_state() -> (bool, Option<&'static str>) {
    match state() {
        State::Installed => (true, None),
        State::Refused(reason) => (false, Some(reason)),
        State::Absent | State::Unreadable => (false, None),
    }
}

/// The same question about a named folder and a named executable, so a test can ask it without a
/// codex installation on the machine it runs on.
///
/// **Installed when either file names this copy and codex runs it**: the hook, or a `notify` line
/// an earlier Folio wrote and that stays until Folio writes its entry again (R18). The hook counts
/// only where codex will run it (review N6): on a codex that runs hooks (`hooks_run`, 0.148 and
/// later), in a `hooks.json` codex reads ([`read_by_codex`]), with hooks not switched off in
/// `config.toml` ([`hooks_switched_off`]). The refusal the row says is `hooks.json`'s, the one
/// file a press writes Folio's entry in.
#[must_use]
fn codex_state_in(dir: &Path, exe: Option<&Path>, hooks_run: bool) -> State {
    let toml = toml_beside(&dir.join(CONFIG_FILE));
    let hook = match hooks::state_in(&HOOKS, &dir.join(HOOKS_FILE), exe) {
        hooks::State::Installed
            if !hooks_run
                || toml
                    .as_ref()
                    .is_ok_and(|toml| hooks_switched_off(&toml.document))
                || !hooks_file_read_by_codex(&dir.join(HOOKS_FILE)) =>
        {
            hooks::State::Absent
        }
        hook => hook,
    };
    let notify = match toml {
        Ok(toml) => {
            let ours = notify_owner(&toml.document).ok().flatten();
            if exe.is_some_and(|exe| {
                ours.is_some_and(|owner| crate::explorer_menu::same_path(&owner, exe))
            }) {
                State::Installed
            } else {
                State::Absent
            }
        }
        Err(_) => State::Unreadable,
    };
    match (hook, notify) {
        (hooks::State::Installed, _) | (_, State::Installed) => State::Installed,
        (hooks::State::Refused(reason), _) => State::Refused(reason),
        (hooks::State::Unreadable, _) | (_, State::Unreadable | State::Refused(_)) => {
            State::Unreadable
        }
        (hooks::State::Absent, State::Absent) => State::Absent,
    }
}

/// **The command Folio's hook runs** for one row of this family (`codex:<row>`), for the shell
/// codex runs it through on this machine.
#[must_use]
fn hook_command(exe: &Path, row: &str) -> String {
    hook_command_on(exe, row, bt_platform::host_platform())
}

/// The same, for a named platform: **PowerShell's call operator and single quotes on Windows**
/// ([`ownership::powershell_quoted`]) — a quoted string followed by a bare word is a parse error
/// there, so the path is called with `&`, and inside the quotes nothing is special but a quote,
/// which is doubled; **sh's single quotes elsewhere**, inside which nothing is special and a `'` of
/// the path's own is spelled `'\''`. Both are the spellings [`ownership::legacy_path`] reads back,
/// which is what makes the entry provably Folio's.
#[must_use]
fn hook_command_on(exe: &Path, row: &str, platform: HostPlatform) -> String {
    let path = exe.to_string_lossy();
    let program = if platform == HostPlatform::Windows {
        format!("& {}", ownership::powershell_quoted(&path))
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    };
    format!("{program} {ATTENTION_VERB} {CODEX}:{row} {JSON_FLAG} {STDIN_PAYLOAD}")
}

/// **Whether Windows PowerShell 5.1 can call Folio at this path with `& '<path>'`** (re-review R1).
/// 5.1, the PowerShell every Windows ships, reads each component of the call's path as a wildcard
/// pattern before it looks for the file, and throws on one it cannot parse, so the hook would exit 1
/// on every turn. pwsh 7 does not, and the backtick spelling 5.1 would take is a path pwsh 7 does
/// not find: no one spelling serves both, so an install from such a path is refused and said.
///
/// The pattern, as 5.1 parses it (measured on 2026-11-05 through Windows PowerShell 5.1.26100 and
/// pwsh 7.6.9 over some seventy folder names, row 69's report): outside a set a backtick makes the
/// next unit literal, and an unescaped `[` opens a set; the set's first unit is its own, a `]`
/// too, and the set ends at the next `]`, none being a parse error, and a backtick inside is a
/// character of the set. In the set, `x-y` is a range, taken left to right, and the range becomes
/// one of .NET's character classes: a range whose end is below its start, or whose end is `[`,
/// which .NET reads as the start of a class subtraction, is a parse error too. Units are UTF-16,
/// which is what both PowerShell and .NET compare.
#[must_use]
fn windows_powershell_finds(path: &str) -> bool {
    path.split(['\\', '/']).all(|component| {
        let units: Vec<u16> = component.encode_utf16().collect();
        let mut at = units.iter().copied();
        let mut escaped = false;
        while let Some(unit) = at.next() {
            if unit == u16::from(b'[') && !escaped {
                let Some(first) = at.next() else {
                    return false;
                };
                let mut set = vec![first];
                loop {
                    match at.next() {
                        None => return false,
                        Some(unit) if unit == u16::from(b']') => break,
                        Some(unit) => set.push(unit),
                    }
                }
                let mut index = 0;
                while index < set.len() {
                    if index + 2 < set.len() && set[index + 1] == u16::from(b'-') {
                        if set[index] > set[index + 2] || set[index + 2] == u16::from(b'[') {
                            return false;
                        }
                        index += 3;
                    } else {
                        index += 1;
                    }
                }
                escaped = false;
            } else {
                escaped = unit == u16::from(b'`');
            }
        }
        true
    })
}

/// **Whose one `hooks.json` entry is** — the [`HOOKS`] family's decoder.
///
/// A copy of Folio's when its `command` is Folio's template ([`ownership::legacy_path`]: a quoted
/// executable whose own file name is Folio's, speaking `attention codex:<event>`) and it carries no
/// key Folio does not write ([`HOOK_KEYS`]). Folio's verb in any other shape is an error, which the
/// splice reads as nobody's: the entry is left where it is, untouched.
fn codex_hook_owner(hook: &Json) -> Result<Option<PathBuf>, &'static str> {
    let Some(command) = hook.get("command").and_then(Json::as_str) else {
        return Ok(None);
    };
    let owner = ownership::legacy_path(command, CODEX)?;
    if owner.is_some()
        && (hook.get("type").and_then(Json::as_str) != Some("command")
            || hook
                .as_object()
                .is_none_or(|o| o.keys().any(|key| !HOOK_KEYS.contains(&key.as_str())))
            || hook.get("async").is_some_and(|value| !value.is_boolean())
            || hook.get("timeout").is_some_and(|value| !value.is_u64()))
    {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(owner)
}

/// **The one entry an install writes**, in a group of its own under `Stop`. `Stop` reads no
/// matcher, so the group carries none.
fn hook_wanted(exe: &Path) -> Vec<Wanted> {
    vec![Wanted {
        event: STOP_EVENT,
        matcher: None,
        row: STOP,
        hook: hook_entry(STOP, exe),
        carries_resume: attention_map::turn_end_row(CODEX, STOP)
            .is_some_and(|row| row.resume.is_declared()),
    }]
}

/// **The entry this build writes for one row** — [`HOOKS`]'s
/// [`Family::todays`](crate::attention_hooks::Family::todays): what an install points another
/// copy's entry at, where it stands (ruling N1).
fn hook_entry(row: &str, exe: &Path) -> Json {
    let mut hook = Map::new();
    hook.insert("type".to_owned(), "command".into());
    hook.insert("command".to_owned(), hook_command(exe, row).into());
    hook.insert("async".to_owned(), true.into());
    hook.insert("timeout".to_owned(), HOOK_TIMEOUT.into());
    Json::Object(hook)
}

/// **Whether codex would read `hooks.json` with this text in it** — the text the file will hold
/// once Folio's hook is in it.
///
/// codex reads the file with `serde_json::from_str::<HooksFile>` (`hooks/src/engine/discovery.rs`
/// at `rust-v0.166.0`), and any error anywhere in it — a byte-order mark, a top-level key other
/// than `description` and `hooks`, a key named twice in one object it reads into a struct, a handler of
/// a type it does not know, a `matcher` that is not a string, a `timeout` that is not a whole
/// number — is a warning and **the whole file ignored**: a hook written into it would never run,
/// and the press would be a lie. The splice keeps every one of those bytes, so they would still be
/// there after the write. So the question is asked of codex's own types, mirrored attribute for
/// attribute ([`CodexHooksFile`]), and of the text as written.
fn read_by_codex(text: &str) -> bool {
    serde_json::from_str::<CodexHooksFile>(text).is_ok()
}

/// [`read_by_codex`] of the `hooks.json` at `path` as it stands — `false` when it cannot be read.
fn hooks_file_read_by_codex(path: &Path) -> bool {
    bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Attention, path)
        .is_ok_and(|text| read_by_codex(&text))
}

/// **The refusal over hooks switched off**, naming the `config.toml` that says so (the 69c
/// review's C6): the file this operation read, which is under `CODEX_HOME` when that is set.
fn hooks_off(toml_path: &Path) -> &'static str {
    crate::i18n::codex_hooks_off(&toml_path.display().to_string())
}

/// **Whether `config.toml` switches codex's hooks off** (row 69's report §1b.3): `[features]`'s
/// `hooks`, or the `codex_hooks` it was called before, set to `false`. codex applies the table's
/// switches in key order, so where both are written `hooks` is the one that stands. A
/// `[profiles.<name>.features]` table applies only to a session started with that profile, which
/// Folio cannot know, and is not read (`docs/DESIGN.md`, row 69c's entry).
fn hooks_switched_off(document: &Table) -> bool {
    let Some(features) = document.get(FEATURES_KEY).and_then(Item::as_table_like) else {
        return false;
    };
    let switch = |name: &str| features.get(name).and_then(Item::as_bool);
    switch(HOOKS_FEATURE)
        .or_else(|| switch(HOOKS_FEATURE_FORMERLY))
        .is_some_and(|on| !on)
}

/// **Why codex would not run a hook of Folio's in these files**, when it would not — the refusal an
/// install gives, and the row's line over them: hooks switched off in `config.toml`, or a
/// `hooks.json` codex ignores whole ([`read_by_codex`]). Hook events in `config.toml` (R15) are not
/// one: codex loads both files, with a warning, so a hook already there runs.
fn hook_kept_from_running(toml_path: &Path, toml: &Table, hooks_text: &str) -> Option<RowSentence> {
    if hooks_switched_off(toml) {
        Some(RowSentence::CodexHooksOff(toml_path.to_path_buf()))
    } else if !hooks_text.trim().is_empty() && !read_by_codex(hooks_text) {
        Some(crate::i18n::Text::CodexHooksFileIgnored.into())
    } else {
        None
    }
}

// ── codex's `HooksFile`, as codex reads it ─────────────────────────────────────
//
// `config/src/hook_config.rs` and `protocol/src/hooks.rs` at `rust-v0.166.0`: the same fields,
// the same types, the same serde attributes — `deny_unknown_fields` on the file and nowhere else,
// `default` where codex says it, the handler internally tagged by `type`. Only the field names
// differ, each a `_` before codex's own, since this reads the file to judge it and keeps nothing.

/// codex's `HooksFile`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CodexHooksFile {
    #[serde(default, rename = "description")]
    _description: Option<String>,
    #[serde(default, rename = "hooks")]
    _hooks: CodexHookEvents,
}

/// codex's `HookEventsToml`: its 14 events ([`TOML_HOOK_EVENTS`]), each a list of groups. A key
/// that is none of them is passed over, as codex passes it over.
#[derive(Default, serde::Deserialize)]
struct CodexHookEvents {
    #[serde(rename = "PreModelRequest", default)]
    _pre_model_request: Vec<CodexGroup>,
    #[serde(rename = "PostModelRequest", default)]
    _post_model_request: Vec<CodexGroup>,
    #[serde(rename = "PreToolUse", default)]
    _pre_tool_use: Vec<CodexGroup>,
    #[serde(rename = "PermissionRequest", default)]
    _permission_request: Vec<CodexGroup>,
    #[serde(rename = "PostToolUse", default)]
    _post_tool_use: Vec<CodexGroup>,
    #[serde(rename = "PreCompact", default)]
    _pre_compact: Vec<CodexGroup>,
    #[serde(rename = "PostCompact", default)]
    _post_compact: Vec<CodexGroup>,
    #[serde(rename = "SessionStart", default)]
    _session_start: Vec<CodexGroup>,
    #[serde(rename = "SessionEnd", default)]
    _session_end: Vec<CodexGroup>,
    #[serde(rename = "UserPromptSubmit", default)]
    _user_prompt_submit: Vec<CodexGroup>,
    #[serde(rename = "SubagentStart", default)]
    _subagent_start: Vec<CodexGroup>,
    #[serde(rename = "SubagentStop", default)]
    _subagent_stop: Vec<CodexGroup>,
    #[serde(rename = "Stop", default)]
    _stop: Vec<CodexGroup>,
    #[serde(rename = "Interrupt", default)]
    _interrupt: Vec<CodexGroup>,
}

/// codex's `MatcherGroup`.
#[derive(serde::Deserialize)]
struct CodexGroup {
    #[serde(default, rename = "matcher")]
    _matcher: Option<String>,
    #[serde(default, rename = "hooks")]
    _hooks: Vec<CodexHandler>,
}

/// codex's `HookHandlerConfig`.
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum CodexHandler {
    #[serde(rename = "command")]
    Command {
        #[serde(rename = "command")]
        _command: String,
        #[serde(default, rename = "commandWindows", alias = "command_windows")]
        _command_windows: Option<String>,
        #[serde(default, rename = "timeout")]
        _timeout_sec: Option<u64>,
        #[serde(default, rename = "async")]
        _async: bool,
        #[serde(default, rename = "statusMessage")]
        _status_message: Option<String>,
        #[serde(default, rename = "additionalContextLimit")]
        _additional_context_limit: Option<usize>,
    },
    #[serde(rename = "mcp_tool")]
    McpTool {
        #[serde(rename = "server")]
        _server: String,
        #[serde(rename = "tool")]
        _tool: String,
        #[serde(default, rename = "input", deserialize_with = "toml_representable")]
        _input: Map<String, Json>,
        #[serde(default, rename = "timeout")]
        _timeout_sec: Option<u64>,
        #[serde(default, rename = "statusMessage")]
        _status_message: Option<String>,
    },
    #[serde(rename = "prompt")]
    Prompt {},
    #[serde(rename = "agent")]
    Agent {},
}

/// codex's `deserialize_mcp_tool_input`: an `mcp_tool` handler's `input` is an object every value
/// of which TOML can hold — no `null`, no whole number past `i64` — or the file is refused.
fn toml_representable<'de, D>(deserializer: D) -> Result<Map<String, Json>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    fn representable(value: &Json) -> bool {
        match value {
            Json::Null => false,
            Json::Number(number) => !number.is_u64() || number.as_i64().is_some(),
            Json::Array(items) => items.iter().all(representable),
            Json::Object(items) => items.values().all(representable),
            Json::Bool(_) | Json::String(_) => true,
        }
    }
    let input = <Map<String, Json> as serde::Deserialize>::deserialize(deserializer)?;
    if input.values().all(representable) {
        Ok(input)
    } else {
        Err(serde::de::Error::custom(
            "MCP hook input must be representable as TOML",
        ))
    }
}

/// **Whether config.toml carries hook events of its own** (R15): a non-empty list under one of
/// codex's event names in its `[hooks]` table — written as `[[hooks.Stop]]` blocks, an inline
/// array, or dotted keys alike. Anything else under one of those names is not a list codex can
/// read, and a file codex would refuse to load is not one Folio adds a second representation
/// beside either.
fn declares_toml_hooks(document: &Table) -> bool {
    document
        .get("hooks")
        .and_then(Item::as_table_like)
        .is_some_and(|hooks| {
            TOML_HOOK_EVENTS
                .iter()
                .filter_map(|event| hooks.get(event))
                .any(|groups| match groups {
                    Item::ArrayOfTables(tables) => !tables.is_empty(),
                    Item::Value(Value::Array(array)) => !array.is_empty(),
                    _ => true,
                })
        })
}

/// The array an install of the `notify` line writes, given where this build lives.
#[must_use]
pub(crate) fn program_for(exe: &Path) -> Vec<String> {
    vec![
        exe.display().to_string(),
        ATTENTION_VERB.to_owned(),
        format!("{CODEX}:{EVENT}"),
        JSON_FLAG.to_owned(),
    ]
}

/// The words of a `notify` value, or nothing when the key is absent or is not an array of strings.
fn words_of(document: &Table) -> Option<Vec<String>> {
    document
        .get(NOTIFY_KEY)?
        .as_array()?
        .iter()
        .map(|word| word.as_str().map(str::to_owned))
        .collect()
}

/// Whether the exact Folio argv shape is present. `notify_owner` is the
/// ownership decoder; all mutations additionally check its executable path.
fn declares_folio(document: &Table) -> bool {
    notify_owner(document).is_ok_and(|owner| owner.is_some())
}

/// The sole `notify` attribution site: argv[0], with the exact Folio verb/event shape.
fn notify_owner(document: &Table) -> Result<Option<PathBuf>, &'static str> {
    if document.get(NOTIFY_KEY).is_none() {
        return Ok(None);
    }
    let words = words_of(document).ok_or(crate::i18n::Text::CodexConfigUnreadable.text())?;
    let folio = words.get(1).is_some_and(|word| word == ATTENTION_VERB)
        && words
            .get(2)
            .is_some_and(|word| word.starts_with(&format!("{CODEX}:")));
    if !folio {
        return Ok(None);
    }
    // The verb said *a* Folio; the program's own name says whether it is one at all. A `notify` of
    // somebody's own that speaks this verb is theirs, and it is never touched (re-review B2).
    let Some(owner) = ownership::folio_operand(&words[0]) else {
        return Ok(None);
    };
    if words.len() != 4 || words[2] != format!("{CODEX}:{EVENT}") || words[3] != JSON_FLAG {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(Some(owner))
}

/// **Whether somebody else's program is on the `notify` key.**
///
/// The one state the line writer refuses to act on: there is one `notify` key, so writing Folio's
/// over theirs deletes a program this build cannot give back.
fn declares_somebody_else(document: &Table) -> bool {
    document.get(NOTIFY_KEY).is_some() && !declares_folio(document)
}

/// **What codex's configuration holds, as the start-time scan reads it** (0.4.10 plan row 67b, over
/// row 69's two files): the file the scan was handed says which entry is Folio's on this codex
/// ([`entry_path`]) — `config.toml`'s `notify` line ([`notify_scan_resolved`]), or the hook in
/// `hooks.json` ([`hook_scan_resolved`]). Nothing is written here.
pub(crate) fn scan_resolved(
    config: &crate::attention_hooks::Config,
    exe: &Path,
    look: &dyn ownership::Look,
) -> HooksFound {
    if is_config_file(config) {
        notify_scan_resolved(config, exe, look)
    } else {
        hook_scan_resolved(config, exe, look)
    }
}

/// Whether an operation was handed `config.toml` — the `notify` line's road — rather than
/// `hooks.json`.
fn is_config_file(config: &Config) -> bool {
    config.named().file_name() == Some(OsStr::new(CONFIG_FILE))
}

/// **`hooks.json`, with the `config.toml` beside it, as the scan reads them on a codex that runs
/// hooks.** The owners are both files' — the hook's, and an earlier Folio's `notify` line's — so a
/// gone copy's line is repaired with the hook, and this copy's line beside its own hook is an old
/// shape the repair takes out. **This copy's line alone is current** (R18): it works, and moving it
/// would trade it for an entry codex asks the person to trust; it is replaced only when Folio writes
/// its entry anyway.
///
/// Where codex would not run a hook of Folio's ([`hook_kept_from_running`]) the reason is the row's
/// line and nothing is repaired — unless this copy's line is there and works, which is the
/// integration on such a machine. Where `config.toml` holds hook events of its own (R15) an install
/// writes nothing, so a file with nothing of Folio's, or one a repair would have to write, says
/// that instead.
pub(crate) fn hook_scan_resolved(
    config: &crate::attention_hooks::Config,
    exe: &Path,
    look: &dyn ownership::Look,
) -> HooksFound {
    let folder = config.named().parent().expect("absolute hooks file");
    let toml = match toml_beside(&folder.join(CONFIG_FILE)) {
        Ok(toml) => toml,
        Err(reason) => return HooksFound::Declined(reason.into()),
    };
    if toml.document.get(NOTIFY_KEY).is_some() && words_of(&toml.document).is_none() {
        return HooksFound::Declined(crate::i18n::Text::CodexConfigUnreadable.into());
    }
    let line = match notify_owner(&toml.document) {
        Ok(line) => line,
        Err(_) => return HooksFound::Unclassified,
    };
    let reading = match hooks::family_reading(&HOOKS, config, exe) {
        Ok(reading) => reading,
        Err(found) => return found,
    };
    let hooks_text = match config.standing() {
        Standing::Text(text) => text,
        Standing::Nothing | Standing::Unreadable => String::new(),
    };
    let line_form = || {
        if words_of(&toml.document) == Some(program_for(exe)) {
            Form::Today
        } else {
            Form::Old
        }
    };
    if let Some(reason) = hook_kept_from_running(&toml.path, &toml.document, &hooks_text) {
        return match line {
            Some(owner) => match ownership::classify_owners(&[owner], exe, look, line_form) {
                found if found.wants_repair() => HooksFound::Declined(reason),
                found => found,
            },
            None => HooksFound::Declined(reason),
        };
    }
    let found = if reading.owners.is_empty() {
        match line {
            Some(owner) => ownership::classify_owners(&[owner], exe, look, line_form),
            None => HooksFound::Nothing,
        }
    } else {
        let mut owners = reading.owners;
        owners.extend(line.iter().cloned());
        // This copy's line beside its own hook is an old shape the repair takes out.
        let form = if line.is_some() {
            Form::Old
        } else {
            reading.form
        };
        ownership::classify_owners(&owners, exe, look, || form)
    };
    match found {
        found
            if (found == HooksFound::Nothing || found.wants_repair())
                && declares_toml_hooks(&toml.document) =>
        {
            HooksFound::Declined(crate::i18n::Text::CodexHooksInToml.into())
        }
        found => found,
    }
}

/// **What codex's configuration holds, as the start-time scan reads it** (0.4.10 plan row 67b):
/// `attention_hooks::scan_resolved`'s answer over this family's one entry, the `notify` key.
/// Nothing is written here.
///
/// Today's form is the one shape there has been (X1): [`program_for`] this copy. The same program
/// spelled another way — the link a copy was started through, where the scan names the file it
/// leads to — is this copy's in an older spelling, and drifted.
///
/// **A hook of Folio's in the `hooks.json` beside it is an entry in the wrong file** on a codex
/// below 0.148, which skips it (0.4.10 plan row 69c; the coordinator's ruling on 69c's choice 1): a
/// press made before the version was known wrote it. Its owners are classified with the line's,
/// and this copy's hook is drifted, so the start-time repair writes the line and takes the hook out
/// ([`notify_install_resolved`]) without a second press. In a `hooks.json` Folio reads but will not
/// edit, a hook the repair would have to take out says that file's reason instead.
pub(crate) fn notify_scan_resolved(
    config: &crate::attention_hooks::Config,
    exe: &Path,
    look: &dyn ownership::Look,
) -> HooksFound {
    let text = match config.standing() {
        Standing::Nothing => String::new(),
        Standing::Unreadable => return HooksFound::Unreadable,
        Standing::Text(text) => text,
    };
    let Ok(document) = text.parse::<DocumentMut>() else {
        return HooksFound::Unreadable;
    };
    let words = match document.get(NOTIFY_KEY) {
        None => None,
        Some(_) => match words_of(&document) {
            Some(words) => Some(words),
            None => return HooksFound::Unreadable,
        },
    };
    let line = match notify_owner(&document) {
        Ok(owner) => owner,
        Err(_) => return HooksFound::Unclassified,
    };
    if let (Some(words), None) = (&words, &line) {
        return HooksFound::Declined(held_slot(&version(), words));
    }
    let folder = config.named().parent().expect("absolute config");
    let hooks = match hooks_beside(&folder.join(HOOKS_FILE)) {
        Ok(hooks) => hooks,
        Err(reason) => return HooksFound::Declined(reason.into()),
    };
    let mut owners: Vec<PathBuf> = line.into_iter().collect();
    owners.extend(hooks.owners.iter().cloned());
    let found = ownership::classify_owners(&owners, exe, look, || {
        if hooks.owners.is_empty() && words == Some(program_for(exe)) {
            Form::Today
        } else {
            Form::Old
        }
    });
    match (&hooks.config, found) {
        (Err(reason), found) if found.wants_repair() && !hooks.owners.is_empty() => {
            HooksFound::Declined(RowSentence::Said(reason))
        }
        (_, found) => found,
    }
}

/// **`hooks.json` as an operation on `config.toml` reads it** (0.4.10 plan row 69c): its text,
/// the owners Folio's hooks in it name, and the resolution a write of it needs — or, for a file
/// this build reads but will not edit, the reason it will not be written.
struct HooksBeside {
    text: String,
    owners: Vec<PathBuf>,
    config: Result<Config, &'static str>,
}

/// Read the `hooks.json` at `path`, resolved **once**, for this operation — [`toml_beside`]'s rule
/// for the other file: a file Folio will not edit is still read, and refused only where a hook of
/// Folio's in it would have to go. One this build cannot read is
/// [`Text::AgentHooksUnreadable`](crate::i18n::Text::AgentHooksUnreadable).
fn hooks_beside(path: &Path) -> Result<HooksBeside, crate::i18n::Text> {
    let (text, config) = match Config::resolve(path) {
        Ok(config) => match config.standing() {
            Standing::Text(text) => (text, Ok(config)),
            Standing::Nothing => (String::new(), Ok(config)),
            Standing::Unreadable => return Err(crate::i18n::Text::AgentHooksUnreadable),
        },
        Err(hooks::Unresolved::Refused(reason)) => {
            match bt_platform::file_reads::read_to_string(
                bt_platform::file_reads::Lane::Attention,
                path,
            ) {
                Ok(text) => (text, Err(reason)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    (String::new(), Err(reason))
                }
                Err(_) => return Err(crate::i18n::Text::AgentHooksUnreadable),
            }
        }
        Err(hooks::Unresolved::Unreadable) => {
            return Err(crate::i18n::Text::AgentHooksUnreadable);
        }
    };
    let owners = hooks::hook_owners(&HOOKS, &text)?;
    Ok(HooksBeside {
        text,
        owners,
        config,
    })
}

/// **The refusal over somebody else's `notify`, on a codex below 0.148** (R12): it names the
/// version and their program, and what a newer codex would let Folio do. Without a version to
/// name, the plain sentence.
fn held_slot(version: &CodexVersion, words: &[String]) -> RowSentence {
    match (version, words.first()) {
        (CodexVersion::Known(release), Some(program)) => RowSentence::CodexNotifyHeld {
            version: release.to_string(),
            program: Path::new(program).file_name().map_or_else(
                || program.clone(),
                |name| name.to_string_lossy().into_owned(),
            ),
        },
        _ => crate::i18n::Text::AgentCodexNotifyTaken.into(),
    }
}

/// **`existing` with Folio's `notify` line put in (`install`) or taken out, and every other byte as
/// it was** (the owner's rule of 2026-11-04; 0.4.10 plan row 67a). `Ok(None)` when there is nothing
/// to change.
///
/// A removal takes out the line of this copy or of a copy that is gone — or, with `any_copy`, of
/// whichever copy wrote it: the hook install that replaces it has asked about every other live copy
/// first (ruling R18).
///
/// `toml_edit` keeps comments and key order through a round trip, but not every byte: it drops a
/// byte-order mark and ends every line it writes in `\n`, so a file saved with Windows line endings
/// came back with Unix ones. So the document is read for its **spans** (`ImDocument` keeps them) and
/// the edit is made on the text, one line, and nothing else:
///
/// - **an own `notify` with other words** has its value replaced where it stands — a comment at the
///   end of the line stays with it;
/// - **no `notify`** gets one line after the last key-value at the top level (where codex reads
///   it, never under a table header), or at the top of a file that has none;
/// - **removing** takes the `notify` line out, its line ending with it, and nothing above it: a
///   comment the person wrote over the key stays where they wrote it.
///
/// The result is read back before anybody may write it ([`kept`]).
fn edited(
    existing: &str,
    exe: &Path,
    install: bool,
    any_copy: bool,
) -> Result<Option<String>, &'static str> {
    let document =
        ImDocument::parse(existing).map_err(|_| crate::i18n::Text::CodexConfigUnreadable.text())?;
    let table = document.as_table();
    let body = if existing.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let newline = match existing.find('\n') {
        Some(at) if at > 0 && existing.as_bytes()[at - 1] == b'\r' => "\r\n",
        _ => "\n",
    };
    let edit = if install {
        if declares_somebody_else(table) || words_of(table) == Some(program_for(exe)) {
            return Ok(None);
        }
        let mut array = Array::new();
        for word in program_for(exe) {
            array.push(word);
        }
        let value = Value::Array(array).to_string();
        match table.get(NOTIFY_KEY) {
            Some(item) => {
                let span = item
                    .span()
                    .ok_or(crate::i18n::Text::AgentHooksSpliceRefused.text())?;
                (span.start, span.end, value)
            }
            None => {
                let line = format!("{NOTIFY_KEY} = {value}");
                let last = table
                    .get_values()
                    .iter()
                    .filter_map(|(_, value)| value.span())
                    .map(|span| span.end)
                    .max();
                match last.map(|end| (end, existing[end..].find('\n'))) {
                    Some((end, Some(ending))) => {
                        let at = end + ending + 1;
                        (at, at, format!("{line}{newline}"))
                    }
                    Some((_, None)) => (existing.len(), existing.len(), format!("{newline}{line}")),
                    None => (body, body, format!("{line}{newline}")),
                }
            }
        }
    } else {
        if notify_owner(table)
            .ok()
            .flatten()
            .is_none_or(|owner| !any_copy && ownership::other_live(&owner, exe) != Ok(false))
        {
            return Ok(None);
        }
        let (key, item) = table
            .get_key_value(NOTIFY_KEY)
            .ok_or(crate::i18n::Text::AgentHooksSpliceRefused.text())?;
        let key = key
            .span()
            .ok_or(crate::i18n::Text::AgentHooksSpliceRefused.text())?;
        let value = item
            .span()
            .ok_or(crate::i18n::Text::AgentHooksSpliceRefused.text())?;
        let start = existing[..key.start]
            .rfind('\n')
            .map_or(body, |ending| ending + 1)
            .max(body);
        // Nothing but indentation before the key on its line: the line is the key's.
        if !existing[start..key.start]
            .trim_matches([' ', '\t'])
            .is_empty()
        {
            return Err(crate::i18n::Text::AgentHooksSpliceRefused.text());
        }
        match existing[value.end..].find('\n') {
            Some(ending) => (start, value.end + ending + 1, String::new()),
            // The last line, with no line ending of its own: the one before it goes instead, so a
            // file that ended without one still does.
            None => {
                let before = if existing[..start].ends_with("\r\n") {
                    2
                } else {
                    usize::from(existing[..start].ends_with('\n'))
                };
                (start - before, existing.len(), String::new())
            }
        }
    };
    let (start, end, text) = edit;
    let written = format!("{}{text}{}", &existing[..start], &existing[end..]);
    if kept(existing, &written, exe, install) {
        Ok(Some(written))
    } else {
        Err(crate::i18n::Text::AgentHooksSpliceRefused.text())
    }
}

/// **The write-time self-check**: the edited text is a configuration codex can read, its `notify`
/// is what the edit meant it to be, and every other value of the file — by its key path, in its
/// order — reads exactly as it did.
fn kept(existing: &str, written: &str, exe: &Path, install: bool) -> bool {
    let Ok(after) = written.parse::<DocumentMut>() else {
        return false;
    };
    let notify = if install {
        words_of(&after) == Some(program_for(exe))
    } else {
        after.get(NOTIFY_KEY).is_none()
    };
    let before = values_but_notify(existing);
    notify && before.is_some() && before == values_but_notify(written)
}

/// Every value of a document but its top-level `notify`, by key path and in order, and every table
/// — what codex reads out of the file, whatever the comments and blank lines around it say.
fn values_but_notify(text: &str) -> Option<Vec<(String, String)>> {
    let mut document = text.parse::<DocumentMut>().ok()?;
    document.remove(NOTIFY_KEY);
    let mut found = Vec::new();
    flattened(document.as_table(), "", &mut found);
    Some(found)
}

fn flattened(table: &toml_edit::Table, path: &str, found: &mut Vec<(String, String)>) {
    for (key, item) in table.iter() {
        let at = format!("{path}{key:?}.");
        match item {
            Item::Value(value) => {
                found.push((at, value.clone().decorated("", "").to_string()));
            }
            Item::Table(table) => {
                found.push((at.clone(), "table".to_owned()));
                flattened(table, &at, found);
            }
            Item::ArrayOfTables(tables) => {
                for (index, table) in tables.iter().enumerate() {
                    let at = format!("{at}{index}.");
                    found.push((at.clone(), "table".to_owned()));
                    flattened(table, &at, found);
                }
            }
            Item::None => {}
        }
    }
}

/// [`edited`] over a document, for the tests that ask about the edit rather than about a file.
#[cfg(test)]
fn install_into(document: &mut DocumentMut, exe: &Path) -> bool {
    edited_document(document, exe, true)
}

/// See [`install_into`].
#[cfg(test)]
fn remove_from(document: &mut DocumentMut, exe: &Path) -> bool {
    edited_document(document, exe, false)
}

#[cfg(test)]
fn edited_document(document: &mut DocumentMut, exe: &Path, install: bool) -> bool {
    match edited(&document.to_string(), exe, install, false) {
        Ok(Some(text)) => {
            *document = text.parse().expect("the edit writes TOML");
            true
        }
        Ok(None) | Err(_) => false,
    }
}

/// **`config.toml` as an operation on `hooks.json` reads it**: the path it was read at, its text,
/// the document it parses to, and the resolution a write of it needs — or, for a file this build
/// will read but not edit, the reason it will not be written.
struct Toml {
    path: PathBuf,
    text: String,
    document: DocumentMut,
    config: Result<Config, &'static str>,
}

/// Read the `config.toml` at `path`, resolved **once**, for this operation —
/// [`Text::CodexConfigUnreadable`](crate::i18n::Text::CodexConfigUnreadable) when its bytes are not
/// UTF-8 or its text is not TOML.
///
/// A file Folio will not edit — a link out of codex's folder, one shared by hard links, a read-only
/// one — is still read: what it says decides whether a hook may be added beside it (R15), and the
/// operation is refused only when a line of Folio's in it would have to go.
fn toml_beside(path: &Path) -> Result<Toml, crate::i18n::Text> {
    let (text, config) = match Config::resolve(path) {
        Ok(config) => match config.standing() {
            Standing::Text(text) => (text, Ok(config)),
            Standing::Nothing => (String::new(), Ok(config)),
            Standing::Unreadable => return Err(crate::i18n::Text::CodexConfigUnreadable),
        },
        Err(hooks::Unresolved::Refused(reason)) => {
            match bt_platform::file_reads::read_to_string(
                bt_platform::file_reads::Lane::Attention,
                path,
            ) {
                Ok(text) => (text, Err(reason)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    (String::new(), Err(reason))
                }
                Err(_) => return Err(crate::i18n::Text::CodexConfigUnreadable),
            }
        }
        Err(hooks::Unresolved::Unreadable) => {
            return Err(crate::i18n::Text::CodexConfigUnreadable);
        }
    };
    let document = text
        .parse::<DocumentMut>()
        .map_err(|_| crate::i18n::Text::CodexConfigUnreadable)?;
    Ok(Toml {
        path: path.to_path_buf(),
        text,
        document,
        config,
    })
}

/// [`toml_beside`], for the hook road: a `notify` this build cannot decode is a file it cannot read
/// — whether it carries Folio's line is not something to assume.
fn toml_for_hooks(path: &Path) -> Result<Toml, &'static str> {
    toml_beside(path)
        .map_err(crate::i18n::Text::text)
        .and_then(|toml| {
            if toml.document.get(NOTIFY_KEY).is_some() && words_of(&toml.document).is_none() {
                Err(crate::i18n::Text::CodexConfigUnreadable.text())
            } else {
                Ok(toml)
            }
        })
}

/// **The two files an install reads, as it has read them** — the file it writes Folio's entry in,
/// and the one beside it whose entry it may take out.
#[derive(Clone, Copy)]
enum InstallRoad<'a> {
    /// `hooks.json`'s text, and `config.toml` beside it ([`toml_for_hooks`]).
    Hook {
        existing: &'a str,
        toml: Result<&'a Toml, &'static str>,
    },
    /// `config.toml`'s text, and `hooks.json` beside it ([`hooks_beside`]).
    Notify {
        existing: &'a str,
        hooks: Result<&'a HooksBeside, &'static str>,
    },
}

/// **Why an install of `exe`'s entry would be refused, over files already read** — the one list
/// both install roads and the first-run card ask (the 69c review's C3), so the card never offers a
/// row the press then refuses. Everything here is decided from the texts and the version; asking
/// about another live copy's entry ([`ownership::check`]) is a question, not a refusal, and is the
/// press's alone.
///
/// - **The hook** is refused for a path Windows PowerShell 5.1 cannot call (re-review R1), a
///   `config.toml` this build cannot read (R15 and R18 both need it), hook events declared in
///   `config.toml` (R15), hooks switched off there (N6), a `hooks.json` whose edit fails its
///   self-check or that codex would ignore whole once written ([`read_by_codex`]), and an earlier
///   Folio's `notify` line that would have to come out of a `config.toml` this build will not
///   edit (read-only, a link out of the folder, a hard link).
/// - **The `notify` line** is refused for a `config.toml` this build cannot read, somebody else's
///   program on the key ([`held_slot`], naming `version`), a `hooks.json` it cannot read, an edit
///   that fails its self-check, and a hook of Folio's that would have to come out of a `hooks.json`
///   this build will not edit.
fn install_refusal(
    road: InstallRoad<'_>,
    exe: &Path,
    version: &CodexVersion,
) -> Option<&'static str> {
    match road {
        InstallRoad::Hook { existing, toml } => {
            if bt_platform::host_platform() == HostPlatform::Windows
                && !windows_powershell_finds(&exe.to_string_lossy())
            {
                return Some(crate::i18n::Text::CodexHookPathBrackets.text());
            }
            let toml = match toml {
                Ok(toml) => toml,
                Err(reason) => return Some(reason),
            };
            if declares_toml_hooks(&toml.document) {
                return Some(crate::i18n::Text::CodexHooksInToml.text());
            }
            if hooks_switched_off(&toml.document) {
                return Some(hooks_off(&toml.path));
            }
            if let Err(reason) = hooks::hook_owners(&HOOKS, existing) {
                return Some(reason.text());
            }
            let written = match hooks::hooks_edited(&HOOKS, existing, exe, true) {
                Ok(written) => written,
                Err(reason) => return Some(reason),
            };
            if !read_by_codex(written.as_deref().unwrap_or(existing)) {
                return Some(crate::i18n::Text::CodexHooksFileIgnored.text());
            }
            if notify_owner(&toml.document).ok().flatten().is_some() {
                match edited(&toml.text, exe, false, true) {
                    Err(reason) => return Some(reason),
                    Ok(Some(_)) => return toml.config.as_ref().err().copied(),
                    Ok(None) => {}
                }
            }
            None
        }
        InstallRoad::Notify { existing, hooks } => {
            let Ok(document) = existing.parse::<DocumentMut>() else {
                return Some(crate::i18n::Text::CodexConfigUnreadable.text());
            };
            if document.get(NOTIFY_KEY).is_some() && words_of(&document).is_none() {
                return Some(crate::i18n::Text::CodexConfigUnreadable.text());
            }
            if declares_somebody_else(&document) {
                return Some(held_slot(version, &words_of(&document).unwrap_or_default()).text());
            }
            let hooks = match hooks {
                Ok(hooks) => hooks,
                Err(reason) => return Some(reason),
            };
            if let Err(reason) = edited(existing, exe, true, false) {
                return Some(reason);
            }
            if !hooks.owners.is_empty() {
                match hooks::hooks_cleared(&HOOKS, &hooks.text, exe) {
                    Err(reason) => return Some(reason),
                    Ok(Some(_)) => return hooks.config.as_ref().err().copied(),
                    Ok(None) => {}
                }
            }
            None
        }
    }
}

/// **[`install_refusal`] of the files at `path` and beside it, read now** — what the first-run card
/// asks: the press's checks before the files, too (an executable that can be named stably, a file
/// this build will resolve and read, an absolute path), then the files' own.
fn install_refusal_at(path: &Path, exe: &Path, version: &CodexVersion) -> Option<&'static str> {
    if let Err(reason) = ownership::stable_executable(Some(exe)) {
        return Some(reason);
    }
    let config = match Config::resolve(path) {
        Ok(config) => config,
        Err(hooks::Unresolved::Refused(reason)) => return Some(reason),
        Err(hooks::Unresolved::Unreadable) => {
            return Some(crate::i18n::Text::AgentHooksUnreadable.text());
        }
    };
    if !config.named().is_absolute() {
        return Some(crate::i18n::Text::AgentHooksRootUnstable.text());
    }
    let folder = config.named().parent().expect("absolute config");
    let notify = is_config_file(&config);
    let existing = match config.standing() {
        Standing::Text(text) => text,
        Standing::Nothing => String::new(),
        Standing::Unreadable if notify => {
            return Some(crate::i18n::Text::CodexConfigUnreadable.text());
        }
        Standing::Unreadable => return Some(crate::i18n::Text::AgentHooksUnreadable.text()),
    };
    if notify {
        let hooks = hooks_beside(&folder.join(HOOKS_FILE));
        install_refusal(
            InstallRoad::Notify {
                existing: &existing,
                hooks: hooks.as_ref().map_err(|reason| reason.text()),
            },
            exe,
            version,
        )
    } else {
        let toml = toml_for_hooks(&folder.join(CONFIG_FILE));
        install_refusal(
            InstallRoad::Hook {
                existing: &existing,
                toml: toml.as_ref().map_err(|reason| *reason),
            },
            exe,
            version,
        )
    }
}

/// Put Folio's hook in, or take it out, on this machine.
///
/// **The files are read and written whole, and only Folio's own entries in them change**
/// ([`hooks::hooks_edited`], [`edited`]), and a copy of what was there is kept beside each the first
/// time each day — `attention_hooks`'s rule, for its reason: these are somebody's own configuration
/// files, and a build that could damage one had better be able to hand it back.
///
/// **An install goes in the file this codex runs Folio's entry from** ([`entry_path`]); a removal is
/// pressed on `hooks.json` on every version, and takes the line out of `config.toml` with it.
pub(crate) fn apply(decision: Decision, exe: &Path) -> Outcome {
    let Some(path) = pressed_path(decision.installs()) else {
        return Outcome::Refused("no codex configuration directory to write into");
    };
    apply_at(&path, decision, exe, &crate::persist::storage_dir())
}

/// The same act on a named `hooks.json` — the seam the tests press, so that what they pin is this
/// function and not a codex folder belonging to whoever runs them. The `config.toml` it reads, and
/// may take Folio's `notify` line out of, is the one beside it.
pub(crate) fn apply_at(path: &Path, decision: Decision, exe: &Path, data: &Path) -> Outcome {
    match Config::resolve(path) {
        Ok(config) => apply_resolved(&config, decision, exe, data),
        Err(hooks::Unresolved::Refused(reason)) => Outcome::Refused(reason),
        Err(hooks::Unresolved::Unreadable) => {
            Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text())
        }
    }
}

/// The same act on a `hooks.json` this operation has already resolved.
///
/// **Everything below the entry takes this value.** The path was resolved once, at the top; the
/// read, the dated copy and the replace all name that one answer, and there is no second
/// resolution on this path for a repointed link to slip through (re-review B1). `config.toml`, the
/// other file, is resolved once too ([`toml_beside`]).
///
/// One operation over two files, in this order: both are read; an install's refusals are asked
/// ([`install_refusal`], the first-run card's list too — among them a `config.toml` this build will
/// not edit holding the line that must go); every copy either names is asked about; **both edits
/// are made and pass their self-checks** ([`hooks::hooks_edited`], [`edited`], and on an install
/// [`read_by_codex`] of the text `hooks.json` will hold) — and only then is anything written,
/// `config.toml` first, then `hooks.json`. So every refusal leaves both files as they were. The one
/// window left is a second write that fails after the first landed: the `notify` line is gone and
/// the hook not yet written, a Folio that marks nothing until the next press or row 67's repair
/// finishes it — never a turn marked twice.
///
/// **A `config.toml` this build cannot read refuses an install and not a removal** (review N5). An
/// install needs both of its answers: whether it declares hook events (R15) and whether an earlier
/// Folio's line is in it (R18). A removal needs neither to take Folio's hook out of `hooks.json`,
/// and refusing it would leave a hook naming a Folio that is gone — through an uninstall too. So
/// the removal goes on without that file, leaves it as it was, and says so
/// ([`Outcome::LeftUnread`]): whether a line of Folio's is in it is exactly what could not be read.
/// A `config.toml` read and holding Folio's line, but one this build will not edit (read-only, a
/// link out of the folder, a hard link), is the same for a removal (review R2): the hook goes, the
/// line stays, and the outcome says why. Only an install, which could mark a turn twice, is
/// refused whole over it.
///
/// **Handed `config.toml`, the operation is the `notify` line's** (0.4.10 plan row 69c): an install
/// writes the line ([`notify_install_resolved`]), and a removal is the one above, pressed on the
/// `hooks.json` beside it.
pub(crate) fn apply_resolved(
    config: &Config,
    decision: Decision,
    exe: &Path,
    data: &Path,
) -> Outcome {
    if let Err(reason) = ownership::stable_executable(Some(exe)) {
        return Outcome::Refused(reason);
    }
    if !config.named().is_absolute() {
        return Outcome::Refused(crate::i18n::Text::AgentHooksRootUnstable.text());
    }
    if is_config_file(config) {
        return if decision.installs() {
            notify_install_resolved(config, decision, exe, data)
        } else {
            let folder = config.named().parent().expect("absolute config");
            apply_at(&folder.join(HOOKS_FILE), decision, exe, data)
        };
    }
    let install = decision.installs();
    let folder = config.named().parent().expect("absolute config");
    let existing = match config.standing() {
        Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        Standing::Nothing => String::new(),
        // **A file that could not be read is never written over** — release audit 2026-09-16 (C-3).
        Standing::Unreadable => {
            return Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text());
        }
    };
    let toml = toml_for_hooks(&folder.join(CONFIG_FILE));
    // **An install's refusals are [`install_refusal`]'s**, the list the first-run card asks too, and
    // all of them before anybody is asked about another copy's entry. A removal is never refused for
    // what only an install needs (a path PowerShell can call, hook events or hooks switched off in
    // `config.toml`, a file codex reads), and goes on without a `config.toml` it cannot read.
    if install
        && let Some(reason) = install_refusal(
            InstallRoad::Hook {
                existing: &existing,
                toml: toml.as_ref().map_err(|reason| *reason),
            },
            exe,
            &version(),
        )
    {
        return Outcome::Refused(reason);
    }
    let (toml, unread) = match toml {
        Ok(toml) => (Some(toml), None),
        Err(reason) => (None, Some(reason)),
    };
    let mut paths = match hooks::hook_owners(&HOOKS, &existing) {
        Ok(paths) => paths,
        Err(reason) => return Outcome::Refused(reason.text()),
    };
    // **Ownership is per entry** (closure review R4): a `notify` wearing Folio's verb in a shape
    // this build cannot decode is not Folio's, and it is left alone.
    let line = toml
        .as_ref()
        .and_then(|toml| notify_owner(&toml.document).ok().flatten());
    paths.extend(line.iter().cloned());
    let others = match ownership::check(&paths, exe, &decision) {
        Ok(others) => others,
        Err(outcome) => return outcome,
    };
    let written = match hooks::hooks_edited(&HOOKS, &existing, exe, install) {
        Ok(written) => written,
        Err(reason) => return Outcome::Refused(reason),
    };
    // **The line an earlier Folio wrote goes in the same operation** (R18): on an install whoever's
    // it is — `check` has asked about every other live copy — so one turn is not marked twice; on a
    // removal this copy's and a gone one's.
    let taken_out = match (&line, &toml) {
        (Some(_), Some(toml)) => match edited(&toml.text, exe, false, install) {
            Ok(taken_out) => taken_out.map(|text| (toml, text)),
            Err(reason) => return Outcome::Refused(reason),
        },
        _ => None,
    };
    // A `config.toml` Folio reads and will not edit (read-only, a link out of the folder, a hard
    // link) refuses an install whole, which would otherwise mark one turn twice
    // ([`install_refusal`]). A removal marks nothing: it takes the hook out all the same, leaves
    // the line, and says so (review R2).
    let (taken_out, unread) = match taken_out {
        Some((toml, text)) => match toml.config.as_ref().map_err(|reason| *reason) {
            Ok(toml_config) => (Some((toml_config, &toml.text, text)), unread),
            Err(reason) => (None, Some(reason)),
        },
        None => (None, unread),
    };
    let _record = if install {
        match ownership::record(data, folder, "codex") {
            Ok(lock) => Some(lock),
            Err(reason) => return Outcome::Refused(reason),
        }
    } else {
        None
    };
    if written.is_none() && taken_out.is_none() {
        return removal_outcome(false, others, unread);
    }
    // The atomic write, mandatory backup, and unsafe-path refusal are all
    // `attention_hooks::land`'s, said once for all three installers. `config.toml` first: see
    // above.
    if let Some((toml_config, toml_text, taken_out)) = taken_out
        && let Err(reason) = land_codex_file(toml_config, toml_text, "toml", &taken_out)
    {
        return Outcome::Refused(reason);
    }
    if let Some(written) = written
        && let Err(reason) = land_codex_file(config, &existing, "json", &written)
    {
        return Outcome::Refused(reason);
    }
    if install {
        Outcome::Installed
    } else {
        removal_outcome(true, others, unread)
    }
}

/// **Folio's `notify` line written in `config.toml`, on a codex below 0.148** (0.4.10 plan row 69c,
/// R12) — the writer every Folio before 0.4.10 used, by 67a's byte-keeping rule ([`edited`]).
///
/// Somebody else's program on the key is refused, naming the version ([`held_slot`]): there is one
/// `notify` key, and writing Folio's over theirs deletes a program this build cannot give back.
/// A `notify` wearing Folio's verb in a shape this build cannot decode is not Folio's (closure
/// review R4) and is refused the same way.
///
/// **A hook of Folio's in the `hooks.json` beside it goes in the same operation**, whichever copy
/// wrote it — [`ownership::check`] has asked about every other live copy first — so a codex that is
/// updated past 0.148 later does not mark one turn twice (R18, the other way round; the ruling on
/// 69c's choice 1). Both edits are made and pass their self-checks, and a `hooks.json` Folio will
/// not edit holding a hook of Folio's is refused, before either file is written; then the line is
/// written first: a second write that fails leaves both entries, of which this codex runs the line
/// alone.
///
/// Every refusal is [`install_refusal`]'s, asked before anybody is asked about another copy's
/// entry; the reads below it take what it has already let through.
fn notify_install_resolved(
    config: &Config,
    decision: Decision,
    exe: &Path,
    data: &Path,
) -> Outcome {
    let existing = match config.standing() {
        Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        Standing::Nothing => String::new(),
        // **A file that could not be read is never written over** — release audit 2026-09-16 (C-3).
        Standing::Unreadable => {
            return Outcome::Refused(crate::i18n::Text::CodexConfigUnreadable.text());
        }
    };
    let folder = config.named().parent().expect("absolute config");
    let hooks = hooks_beside(&folder.join(HOOKS_FILE));
    if let Some(reason) = install_refusal(
        InstallRoad::Notify {
            existing: &existing,
            hooks: hooks.as_ref().map_err(|reason| reason.text()),
        },
        exe,
        &version(),
    ) {
        return Outcome::Refused(reason);
    }
    let Ok(document) = existing.parse::<DocumentMut>() else {
        return Outcome::Refused(crate::i18n::Text::CodexConfigUnreadable.text());
    };
    let hooks = match hooks {
        Ok(hooks) => hooks,
        Err(reason) => return Outcome::Refused(reason.text()),
    };
    let mut paths = notify_owner(&document)
        .ok()
        .flatten()
        .into_iter()
        .collect::<Vec<_>>();
    paths.extend(hooks.owners.iter().cloned());
    if let Err(outcome) = ownership::check(&paths, exe, &decision) {
        return outcome;
    }
    let written = match edited(&existing, exe, true, false) {
        Ok(written) => written,
        Err(reason) => return Outcome::Refused(reason),
    };
    let cleared = if hooks.owners.is_empty() {
        None
    } else {
        match hooks::hooks_cleared(&HOOKS, &hooks.text, exe) {
            Ok(cleared) => cleared,
            Err(reason) => return Outcome::Refused(reason),
        }
    };
    let cleared = match (cleared, &hooks.config) {
        (Some(text), Ok(hooks_config)) => Some((hooks_config, text)),
        (Some(_), Err(reason)) => return Outcome::Refused(reason),
        (None, _) => None,
    };
    let _record = match ownership::record(data, folder, "codex") {
        Ok(lock) => lock,
        Err(reason) => return Outcome::Refused(reason),
    };
    if written.is_none() && cleared.is_none() {
        return Outcome::Unchanged;
    }
    if let Some(written) = written
        && let Err(reason) = land_codex_file(config, &existing, "toml", &written)
    {
        return Outcome::Refused(reason);
    }
    if let Some((hooks_config, cleared)) = cleared
        && let Err(reason) = land_codex_file(hooks_config, &hooks.text, "json", &cleared)
    {
        return Outcome::Refused(reason);
    }
    Outcome::Installed
}

/// **What a press that wrote nothing, or a removal that wrote (`removed`), says** — in this
/// order: another live copy's entry that stays ([`Outcome::LeftOther`]); a `config.toml` the
/// removal went on without ([`Outcome::LeftUnread`]), which only a removal can; Folio's entries
/// taken out, or nothing to do.
fn removal_outcome(removed: bool, others: Vec<PathBuf>, unread: Option<&'static str>) -> Outcome {
    if !others.is_empty() {
        Outcome::LeftOther(others)
    } else if let Some(reason) = unread {
        Outcome::LeftUnread { removed, reason }
    } else if removed {
        Outcome::Removed
    } else {
        Outcome::Unchanged
    }
}

/// One file's write through [`Config::land`], and the sentence for each way it can fail.
fn land_codex_file(
    config: &Config,
    existing: &str,
    extension: &str,
    written: &str,
) -> Result<(), &'static str> {
    match config.land(existing, extension, written.as_bytes()) {
        hooks::Landing::Landed => Ok(()),
        hooks::Landing::NoDirectory => {
            Err("the codex configuration directory could not be created")
        }
        hooks::Landing::NoBackup => Err(crate::i18n::Text::AgentConfigNoBackup.text()),
        hooks::Landing::Changed => Err(crate::i18n::Text::AgentConfigChanged.text()),
        hooks::Landing::NotWritten => Err("the codex configuration file could not be written"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{host_path, host_spelling};
    use bt_source::{Needle, Pattern, Scope, View};

    #[test]
    fn attention_two_live_copies_require_takeover_and_preserve_bytes() {
        // The test's own folders, removed when it lets go of them (T-TEST-ORDER-STRAYS): the
        // root, and the marks folder `apply_to` names after it.
        let root = crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(concat!("codex-", "two-copies"))),
        );
        let marks = crate::test_support::attention_marks_folder("codex", &root);
        let root = root.also_removing(marks);
        // **Two folders, one file name.** An operand is Folio's only if its file name is one
        // Folio installs itself under, which is how two real copies differ: same program, two
        // places. The odd characters this fixture exists for move to the folder.
        let a = root.join("A space $ ` ' 中文").join("folio.exe");
        let b = root.join("B").join("folio.exe");
        for copy in [&a, &b] {
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(copy, b"a copy of Folio").unwrap();
        }
        let path = root.join(HOOKS_FILE);
        let _ = std::fs::remove_file(&path);
        assert_eq!(apply_to(&path, true, &a), Outcome::Installed);
        let before = std::fs::read(&path).unwrap();
        let result = apply_to(&path, true, &b);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "another live executable must require explicit take-over: {result:?}"
        );
        assert_ne!(result, Outcome::Installed);
        let result = apply_to(&path, false, &b);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "cleanup must leave a live other owner: {result:?}"
        );
    }

    /// One press, on the `hooks.json` at `path`, with its own marks folder.
    fn apply_to(path: &Path, install: bool, exe: &Path) -> Outcome {
        apply_with(path, install.into(), exe)
    }

    fn apply_with(path: &Path, decision: Decision, exe: &Path) -> Outcome {
        apply_at(
            path,
            decision,
            exe,
            &crate::test_support::attention_marks_folder("codex", path),
        )
    }

    fn exe() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-fixtures/Program Files/Folio/folio.exe")
    }

    /// **The consent disclosure names the file this machine will write** —
    /// [`crate::attention_hooks`]'s pin over `CODEX_HOME`, both halves.
    #[test]
    fn the_tip_names_the_default_spelling_until_the_variable_moves_it() {
        let named = |text: &str| Some(OsString::from(text));
        for file in [HOOKS_FILE, CONFIG_FILE] {
            let default = format!("~/.codex/{file}");
            assert_eq!(
                path_shown_from(file, None, named(r"C:\Users\someone")),
                default
            );
            assert_eq!(
                path_shown_from(file, named(""), named(r"C:\Users\someone")),
                default
            );
            assert_eq!(path_shown_from(file, None, None), default);
            assert_eq!(
                path_shown_from(
                    file,
                    named(r"D:\scratch\codex-home"),
                    named(r"C:\Users\someone")
                ),
                PathBuf::from(r"D:\scratch\codex-home")
                    .join(file)
                    .display()
                    .to_string()
            );
        }
    }

    fn installed(text: &str) -> DocumentMut {
        let mut document = text.parse::<DocumentMut>().expect("fixture");
        assert!(install_into(&mut document, &exe()));
        document
    }

    /// **Only the user's own files, and never a repository's.**
    ///
    /// The red form of plan §10.4.3's clause, said about codex: the survey found `.codex/` beside a
    /// working directory to be one of the four places codex itself looks, and named it the attack
    /// surface. There is exactly one folder this module can produce, it comes from the variable
    /// codex itself reads, and no working directory reaches it.
    #[test]
    fn the_only_file_this_writes_is_the_one_the_environment_names() {
        let named = |text: &str| Some(OsString::from(text));
        assert_eq!(
            config_dir_from(
                named(&host_spelling(r"D:\scratch\codex-home")),
                named(&host_spelling(r"C:\Users\someone"))
            ),
            Some(host_path(r"D:\scratch\codex-home"))
        );
        // Set-but-empty is not set.
        assert_eq!(
            config_dir_from(named(""), named(&host_spelling(r"C:\Users\someone"))),
            Some(host_path(r"C:\Users\someone").join(DEFAULT_DIRECTORY))
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, named("")), None);
        assert!(hooks_path().is_none_or(|path| path.ends_with(HOOKS_FILE)));
        // **Nothing in this module can be steered by where the process happens to be standing**,
        // asserted over the source itself because that is the only form the rule has. The names are
        // spelled in halves so this assertion is not its own counter-example.
        for reaching in ["current_dir", "project_dir"] {
            let found = crate::test_support::found_in(
                Needle::new(Pattern::identifier(reaching)),
                View::Identifiers,
                Scope::Module("crate::attention_codex".to_owned()),
            )
            .in_the_product(crate::test_support::source());
            assert!(
                found.is_empty(),
                "`{reaching}` would let a working directory decide where automation is written"
            );
        }
        assert!(
            config_dir_from(named(&host_spelling(r"D:\scratch\codex-home")), None)
                .is_some_and(|path| path.is_absolute())
        );
    }

    /// **The line an earlier Folio wrote into somebody's own file, spelled out** — and the one the
    /// line writer still writes, for the codex builds plan row 69c chooses it for.
    ///
    /// `attention_hooks::the_block_this_installs_is_this`'s reason, over a second document: what is
    /// written is *not ours*. It lands in a file the user owns, that another program reads, and that
    /// nobody will look at again — so a change to any of it should have to be typed here on purpose.
    #[test]
    fn the_line_this_installs_is_this() {
        let mut document = DocumentMut::new();
        assert!(install_into(
            &mut document,
            Path::new(r"C:\folio\folio.exe")
        ));
        assert_eq!(document.to_string(), EXPECTED_LINE);
    }

    /// See [`the_line_this_installs_is_this`].
    const EXPECTED_LINE: &str = include_str!("../../../docs/plans/attention/codex-notify.toml");

    /// **The payload lands on `--json` and not on a second bare word.**
    ///
    /// codex appends one argument before spawning, and the verb's grammar reads a second positional
    /// as a second event and refuses the call. This is the pin on the arrangement that makes the two
    /// agree — asserted through `cli::attention` itself, so it cannot be kept true by a comment.
    #[test]
    fn what_codex_appends_is_read_by_the_verb_as_a_payload() {
        let payload = r#"{"type":"agent-turn-complete","last-assistant-message":"ok"}"#;
        let mut argv = program_for(&exe());
        // The executable is `argv[0]`; what reaches `cli::attention` is everything after it.
        argv.remove(0);
        argv.push(payload.to_owned());
        let call = crate::cli::attention(argv.into_iter().map(OsString::from))
            .expect("the verb")
            .expect("a call and not a fault");
        assert_eq!(call.event, format!("{CODEX}:{EVENT}"));
        assert_eq!(
            call.payload,
            Some(crate::cli::AttentionPayload::Inline(payload.to_owned())),
            "codex hands its payload over as an argument, and it is read as one"
        );
        // And the event named is one this build has a turn-end row for, so an install that
        // succeeded is an install that reaches something.
        assert_eq!(
            crate::attention_map::turn_end(CODEX, EVENT),
            Some(crate::attention::Via::Notify)
        );
    }

    /// **Install then remove is the identity**, byte for byte, on a file somebody wrote by hand.
    ///
    /// Comments, blank lines, key order and a table that comes after the key are all things a value
    /// model would have quietly rearranged; this is the assertion that says they are not rearranged.
    #[test]
    fn taking_it_back_out_hands_back_the_file_that_was_there() {
        let original = "# my codex\nmodel = \"gpt-5\"\n\n[tui]\n# a bell, not a toast\nnotification_method = \"bel\"\n";
        let mut document = installed(original);
        assert_eq!(state_of(&document), State::Installed);
        assert!(document.to_string().contains("# a bell, not a toast"));
        // **And the key landed at the top level**, which is the one thing a
        // rendering could get wrong without any assertion above noticing: a
        // `notify` emitted after the `[tui]` header is a `tui.notify`, and codex
        // would never run it. Asserted by reading the rendered bytes back the way
        // codex would, rather than by trusting the document in hand.
        let written = document
            .to_string()
            .parse::<DocumentMut>()
            .expect("what this writes is TOML");
        assert!(
            declares_folio(&written),
            "the line has to be a top-level `notify`: {}",
            document
        );
        assert!(
            written["tui"]["notification_method"].as_str() == Some("bel"),
            "and the table that was there is still the table that was there"
        );
        assert!(remove_from(&mut document, &exe()));
        assert_eq!(
            document.to_string(),
            original,
            "uninstalling must hand back the file that was there, byte for byte"
        );
    }

    /// **A file that ended without a newline gets it back without one** (real-machine finding,
    /// 2026-08-25).
    ///
    /// The unit test above rounds a file that *does* end in a newline, and it passed while the
    /// real machine handed back 98 bytes for 97: `toml_edit` normalises a document to end in a line
    /// ending, and the one byte was the whole of the difference between the promise and what was
    /// written. Stated over the bytes actually written rather than over the document, because the
    /// document is not where it happens.
    #[test]
    fn a_file_that_ended_without_a_newline_is_handed_back_without_one() {
        for original in [
            "model = \"gpt-5\"",
            "model = \"gpt-5\"\n",
            "model = \"gpt-5\"\n\n",
        ] {
            let installed = edited(original, &exe(), true, false)
                .expect("an edit")
                .expect("a change");
            assert!(
                installed.contains(NOTIFY_KEY),
                "the line is in what gets written"
            );
            let back = edited(&installed, &exe(), false, false)
                .expect("an edit")
                .expect("a change");
            assert_eq!(
                back, original,
                "install then uninstall is the identity on the bytes, not only on the document"
            );
        }
    }

    /// A second press writes nothing.
    #[test]
    fn installing_twice_changes_nothing_the_second_time() {
        let mut document = installed("model = \"gpt-5\"\n");
        assert!(
            !install_into(&mut document, &exe()),
            "an install that was already done is not a change"
        );
        assert!(remove_from(&mut document, &exe()));
        assert!(
            !remove_from(&mut document, &exe()),
            "and neither is a removal of what is not there"
        );
    }

    /// Moving the executable is not a reason to lose track of the entry.
    #[test]
    fn a_dead_owner_can_be_replaced_after_moving_folio() {
        let mut document = installed("");
        let moved = exe().parent().unwrap().join("portable/folio.exe");
        assert!(
            install_into(&mut document, &moved),
            "a rewrite from a new location is a change"
        );
        assert!(document.to_string().contains("portable"));
        assert_eq!(state_of(&document), State::Installed);
        assert!(remove_from(&mut document, &exe()));
        assert!(!document.to_string().contains(NOTIFY_KEY));
    }

    /// **Somebody else's `notify` is never taken away** by the line writer.
    #[test]
    fn a_notify_that_is_not_ours_is_left_exactly_where_it_is() {
        let theirs = "notify = [\"C:\\\\bin\\\\ding.exe\"]\n";
        let mut document = theirs.parse::<DocumentMut>().expect("fixture");
        assert_eq!(state_of(&document), State::Absent);
        assert!(
            !install_into(&mut document, &exe()),
            "installing over somebody's own program is a deletion this build cannot undo"
        );
        assert!(!remove_from(&mut document, &exe()));
        assert_eq!(document.to_string(), theirs);
    }

    /// RED — **a configuration file that could not be read is left byte for byte**, and no hook is
    /// written beside it.
    ///
    /// Release audit 2026-09-16 (C-3): `read_to_string(&path).unwrap_or_default()` took a file this
    /// build was not allowed to read — or one holding a single byte that is not UTF-8 — for a file
    /// that was not there. The operation reads `config.toml` for what it says about hooks (R15) and
    /// for a line of Folio's that would have to go (R18); a file it cannot read answers neither.
    ///
    /// RED GATE: put the `unwrap_or_default` back and a hook is written beside a file nobody read.
    #[test]
    fn a_configuration_file_that_could_not_be_read_is_never_written_over() {
        assert!("model = = \"gpt-5\"\n".parse::<DocumentMut>().is_err());
        let dir = scratch("unreadable");
        let path = dir.join(CONFIG_FILE);
        // A Latin-1 comment: an ordinary line on an ordinary machine, and not UTF-8.
        let theirs: &[u8] = b"# caf\xe9\nmodel = \"gpt-5\"\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        let installing = apply_to(&dir.join(HOOKS_FILE), true, &exe());
        assert_eq!(
            installing,
            Outcome::Refused(crate::i18n::Text::CodexConfigUnreadable.text())
        );
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            names(&dir),
            vec![CONFIG_FILE.to_owned()],
            "nothing was written beside it either"
        );
        // And the row drawn from it says so, rather than offering to write over it.
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Unreadable);
        // Turning it off has nothing to take out of `hooks.json`, and goes on without the file it
        // cannot read: left as it was, and said (review N5).
        let taking_it_out = apply_to(&dir.join(HOOKS_FILE), false, &exe());
        assert_eq!(
            taking_it_out,
            Outcome::LeftUnread {
                removed: false,
                reason: crate::i18n::Text::CodexConfigUnreadable.text()
            }
        );
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned()]);
    }

    /// RED (0.4.10 plan row 69, review N5) — **a `config.toml` this build cannot read does not keep
    /// Folio's hook in `hooks.json`**: switching off, and so an uninstall, takes the hook out and
    /// leaves `config.toml` as it was, saying why — for bytes that are not UTF-8, a text that is not
    /// TOML, and a `notify` that is not a list of words alike. Before, each refused the removal,
    /// and an uninstalled Folio left a hook naming itself behind.
    ///
    /// MUTATION: `plant-69-removal-refused-beside.py` refuses a removal over such a file again.
    #[test]
    fn a_config_toml_that_cannot_be_read_does_not_keep_folios_hook_in() {
        for (name, theirs) in [
            ("latin-1", &b"# caf\xe9\nmodel = \"gpt-5\"\n"[..]),
            ("not-toml", &b"model = = \"gpt-5\"\n"[..]),
            ("notify-not-words", &b"notify = \"ding\"\n"[..]),
        ] {
            let dir = scratch(&format!("unread-{name}"));
            let config = dir.join(CONFIG_FILE);
            let path = dir.join(HOOKS_FILE);
            std::fs::write(&config, theirs).expect("their own file");
            let with_ours = format!(
                r#"{{"hooks":{{"Stop":[{{"hooks":[{}]}}]}}}}"#,
                own_stop(&exe())
            );
            std::fs::write(&path, &with_ours).expect("a hook of this copy's");
            assert_eq!(
                apply_to(&path, true, &exe()),
                Outcome::Refused(crate::i18n::Text::CodexConfigUnreadable.text()),
                "{name}: an install needs both of its answers"
            );
            assert_eq!(
                apply_to(&path, false, &exe()),
                Outcome::LeftUnread {
                    removed: true,
                    reason: crate::i18n::Text::CodexConfigUnreadable.text()
                },
                "{name}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("there"),
                "{}",
                "{name}: Folio's hook is out"
            );
            assert_eq!(std::fs::read(&config).expect("there"), theirs, "{name}");
        }
    }

    /// **A file that is not there is the one state in which writing a fresh one loses nothing.**
    #[test]
    fn a_hooks_file_that_is_not_there_is_the_one_that_gets_created() {
        let dir = scratch("absent");
        let path = dir.join(HOOKS_FILE);
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Absent);

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Installed);
        assert_eq!(
            names(&dir),
            vec![HOOKS_FILE.to_owned()],
            "a first install leaves one file: no copy of nothing, no temporary, no config.toml"
        );
    }

    /// **A file that is there is copied before it is changed**, and the copy is what was there.
    #[test]
    fn a_hooks_file_that_is_there_is_copied_before_it_is_changed() {
        let dir = scratch("copied");
        let path = dir.join(HOOKS_FILE);
        let theirs = "{\n  \"description\": \"mine\"\n}\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(written.contains("\"description\": \"mine\""), "{written}");
        assert!(written.contains("attention codex:stop"), "{written}");
        let backup = format!("{HOOKS_FILE}.bak-{}", crate::attention_hooks::today());
        assert_eq!(names(&dir), vec![HOOKS_FILE.to_owned(), backup.clone()]);
        assert_eq!(
            std::fs::read_to_string(dir.join(&backup)).expect("the copy"),
            theirs,
            "the copy beside it is the file as it was"
        );
    }

    /// A scratch folder of this test's own — never anywhere near a real `~/.codex` — removed with
    /// its marks folder when the test lets go.
    fn scratch(name: &str) -> crate::test_support::ScratchFolder {
        let dir = crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(&format!("folio-codex-{name}"))),
        );
        let marks = crate::test_support::attention_marks_folder("codex", &dir);
        dir.also_removing(marks)
    }

    /// Every name in a directory, sorted — what a reader who opened it would find.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read the directory")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn state_of(document: &DocumentMut) -> State {
        if declares_folio(document) {
            State::Installed
        } else {
            State::Absent
        }
    }

    /// The value of the `notify` this build writes for a copy at `exe`, as it is spelled in the file.
    fn notify_value(exe: &Path) -> String {
        let mut array = Array::new();
        for word in program_for(exe) {
            array.push(word);
        }
        Value::Array(array).to_string()
    }

    /// RED (0.4.10 plan row 67a; ruling R12) — **a comment the person wrote over Folio's `notify`
    /// stays when the switch is turned off**, and the one at the end of Folio's own line stays with
    /// the line when a copy that is gone is replaced.
    ///
    /// `toml_edit`'s `remove` takes the comment lines above a key with the key; the edit on the text
    /// takes the one line and nothing above it.
    #[test]
    fn a_comment_over_the_line_stays_when_the_line_goes() {
        let ours = notify_value(&exe());
        let theirs = format!("# the bell, from Folio\nnotify = {ours}\nmodel = \"gpt-5\"\n");
        assert_eq!(
            edited(&theirs, &exe(), false, false),
            Ok(Some(
                "# the bell, from Folio\nmodel = \"gpt-5\"\n".to_owned()
            ))
        );
        let gone = exe().parent().unwrap().join("gone").join("folio.exe");
        let theirs = format!(
            "model = \"gpt-5\"\nnotify = {} # Folio's\n",
            notify_value(&gone)
        );
        assert_eq!(
            edited(&theirs, &exe(), true, false),
            Ok(Some(format!(
                "model = \"gpt-5\"\nnotify = {ours} # Folio's\n"
            )))
        );
    }

    /// RED (0.4.10 plan row 67a; the owner's rule of 2026-11-04) — **the file's line endings and its
    /// byte-order mark are the file's**: a file saved with Windows line endings gets its line in
    /// them, keeps its mark, and comes back to the byte when the line goes again. `toml_edit`'s
    /// rendering ended every line it wrote in `\n` and dropped the mark.
    #[test]
    fn the_line_endings_and_the_byte_order_mark_are_the_files() {
        let theirs =
            "\u{feff}# mine\r\nmodel = \"gpt-5\"\r\n\r\n[tui]\r\nnotification_method = \"bel\"\r\n";
        let installed = edited(theirs, &exe(), true, false)
            .expect("an edit")
            .expect("a change");
        assert_eq!(
            installed,
            theirs.replacen(
                "model = \"gpt-5\"\r\n",
                &format!("model = \"gpt-5\"\r\nnotify = {}\r\n", notify_value(&exe())),
                1
            )
        );
        assert_eq!(
            edited(&installed, &exe(), false, false),
            Ok(Some(theirs.to_owned()))
        );
    }

    /// **A file of tables only gets the line at its top**, where codex reads a top-level key — a
    /// `notify` written after a header would be that table's — and after a byte-order mark.
    #[test]
    fn a_file_of_tables_only_gets_the_line_at_its_top() {
        for mark in ["", "\u{feff}"] {
            let theirs = format!("{mark}[tui]\nnotification_method = \"bel\"\n");
            let installed = edited(&theirs, &exe(), true, false)
                .expect("an edit")
                .expect("a change");
            assert_eq!(
                installed,
                format!(
                    "{mark}notify = {}\n[tui]\nnotification_method = \"bel\"\n",
                    notify_value(&exe())
                )
            );
            let read = installed.parse::<DocumentMut>().expect("TOML");
            assert!(declares_folio(&read));
            assert_eq!(read["tui"]["notification_method"].as_str(), Some("bel"));
            assert_eq!(edited(&installed, &exe(), false, false), Ok(Some(theirs)));
        }
    }

    /// RED (0.4.10 plan row 67a) — **the write-time self-check refuses an edit that changed
    /// anything but Folio's line**: another value, a value dropped, Folio's line under a table
    /// header (a `tui.notify` codex would never run), a table's value changed.
    #[test]
    fn the_self_check_refuses_an_edit_that_changed_anything_else() {
        let ours = format!("notify = {}", notify_value(&exe()));
        let theirs = "model = \"gpt-5\"\n[tui]\nx = 1\n";
        assert!(kept(
            theirs,
            &format!("model = \"gpt-5\"\n{ours}\n[tui]\nx = 1\n"),
            &exe(),
            true
        ));
        for changed in [
            format!("model = \"gpt-6\"\n{ours}\n[tui]\nx = 1\n"),
            format!("{ours}\n[tui]\nx = 1\n"),
            format!("model = \"gpt-5\"\n[tui]\n{ours}\nx = 1\n"),
            format!("model = \"gpt-5\"\n{ours}\n[tui]\nx = 2\n"),
            format!("model = \"gpt-5\"\n{ours}\n"),
        ] {
            assert!(!kept(theirs, &changed, &exe(), true), "{changed}");
        }
        assert!(kept(
            &format!("model = \"gpt-5\"\n{ours}\n[tui]\nx = 1\n"),
            theirs,
            &exe(),
            false
        ));
    }

    // ── The hook (0.4.10 plan row 69a) ──────────────────────────────────────────

    /// Folio's entry for a copy at `exe`, as the compact JSON a fixture spells it in.
    fn own_stop(exe: &Path) -> String {
        serde_json::to_string(&hook_wanted(exe)[0].hook).expect("an entry renders")
    }

    /// A file of `lines` in LF, each leading `@` two spaces of indentation — so a fixture's layout
    /// is spelled without a run of spaces inside a literal.
    fn spaced(lines: &[String]) -> String {
        lines
            .iter()
            .map(|line| {
                let text = line.trim_start_matches('@');
                format!("{}{text}\n", "  ".repeat(line.len() - text.len()))
            })
            .collect()
    }

    /// The parsed `hooks.json` at `path`.
    fn hooks_of(path: &Path) -> Json {
        serde_json::from_str(
            std::fs::read_to_string(path)
                .expect("read back")
                .trim_start_matches('\u{feff}'),
        )
        .expect("JSON")
    }

    /// **The entry this writes into somebody's own `hooks.json`, spelled out** — the whole file a
    /// first install creates, and the command for both shells codex runs it through.
    #[test]
    fn the_hook_this_installs_is_this() {
        let dir = scratch("the-hook");
        let path = dir.join(HOOKS_FILE);
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            spaced(&[
                "{".to_owned(),
                "@\"hooks\": {".to_owned(),
                "@@\"Stop\": [".to_owned(),
                "@@@{".to_owned(),
                "@@@@\"hooks\": [".to_owned(),
                "@@@@@{".to_owned(),
                "@@@@@@\"async\": true,".to_owned(),
                format!(
                    "@@@@@@\"command\": {},",
                    Json::from(hook_command(&exe(), STOP))
                ),
                "@@@@@@\"timeout\": 10,".to_owned(),
                "@@@@@@\"type\": \"command\"".to_owned(),
                "@@@@@}".to_owned(),
                "@@@@]".to_owned(),
                "@@@}".to_owned(),
                "@@]".to_owned(),
                "@}".to_owned(),
                "}".to_owned(),
            ])
        );
        // Windows: PowerShell, as codex runs the hook (`-NoProfile -Command`); `%`, `$`, backticks
        // and `&` are plain inside the quotes, and every quote PowerShell reads as one is doubled
        // (review N1). Run through pwsh 7.6 and Windows PowerShell 5.1 on 2026-11-05 with these
        // paths, each passed its argv and a megabyte of stdin byte for byte (row 69's report).
        for (platform, exe, command) in [
            (
                HostPlatform::Windows,
                r"C:\Program Files\Folio\folio.exe",
                r"& 'C:\Program Files\Folio\folio.exe' attention codex:stop --json -",
            ),
            (
                HostPlatform::Windows,
                r"C:\Users\someone\Folio's & 100% $HOME `x`\folio.exe",
                r"& 'C:\Users\someone\Folio''s & 100% $HOME `x`\folio.exe' attention codex:stop --json -",
            ),
            // Matched brackets: Windows PowerShell 5.1 reads them as a set and still finds the file
            // (re-review R1); a path it cannot read is refused before this is written.
            (
                HostPlatform::Windows,
                r"C:\Tools\Folio [portable]\folio.exe",
                r"& 'C:\Tools\Folio [portable]\folio.exe' attention codex:stop --json -",
            ),
            (
                HostPlatform::Windows,
                "C:\\Users\\someone\\Folio\u{2019}s \u{2018}\u{201a}\u{201b} 文件夹\\folio.exe",
                "& 'C:\\Users\\someone\\Folio\u{2019}\u{2019}s \u{2018}\u{2018}\u{201a}\u{201a}\u{201b}\u{201b} 文件夹\\folio.exe' attention codex:stop --json -",
            ),
            (
                HostPlatform::MacOs,
                "/Applications/Folio.app/Contents/MacOS/folio",
                "'/Applications/Folio.app/Contents/MacOS/folio' attention codex:stop --json -",
            ),
            (
                HostPlatform::OtherUnix,
                "/home/someone/o'neil $HOME `x`/folio",
                r"'/home/someone/o'\''neil $HOME `x`/folio' attention codex:stop --json -",
            ),
        ] {
            let written = hook_command_on(Path::new(exe), STOP, platform);
            assert_eq!(written, command, "{platform:?}");
            // **And it reads back as this copy's**: the decoder that decides what Folio may edit
            // is the one that decodes what it wrote.
            assert_eq!(
                ownership::legacy_path(&written, CODEX),
                Ok(Some(PathBuf::from(exe))),
                "{platform:?}"
            );
        }
        // **And a PowerShell literal in any other spelling is not Folio's**: a quote left single
        // ends the literal there, and PowerShell would run a different command.
        for spelled in [
            r"& 'C:\Folio's\folio.exe' attention codex:stop --json -",
            "& 'C:\\Folio\u{2019}s\\folio.exe' attention codex:stop --json -",
            r"& 'C:\Folio\folio.exe attention codex:stop --json -",
            r"& 'C:\Folio\folio.exe'' attention codex:stop --json -",
        ] {
            assert!(ownership::legacy_path(spelled, CODEX).is_err(), "{spelled}");
        }
    }

    /// **What codex runs reaches the verb as a `Stop` that reads its payload from stdin**, and the
    /// event is one this build maps (plan row 69b): its words and its session are declared.
    #[test]
    fn the_command_codex_runs_is_a_call_the_verb_reads() {
        let command = hook_command_on(
            Path::new(r"C:\folio\folio.exe"),
            STOP,
            HostPlatform::Windows,
        );
        let words = command
            .strip_prefix(r"& 'C:\folio\folio.exe' ")
            .expect("the call operator and the program, then the verb's words");
        let call = crate::cli::attention(words.split(' ').map(OsString::from))
            .expect("the verb")
            .expect("a call and not a fault");
        assert_eq!(call.event, format!("{CODEX}:{STOP}"));
        assert_eq!(call.payload, Some(crate::cli::AttentionPayload::Stdin));
        let row = crate::attention_map::turn_end_row(CODEX, STOP).expect("a turn-end row");
        assert!(row.words.are_somewhere() && row.resume.is_declared());
        assert_eq!(row.nowhere, crate::attention_map::Nowhere::Quiet);
    }

    /// The person's own `hooks.json`, in the layout `newline` and `unit` say: a `description`, an
    /// event of theirs, and two `Stop` groups of theirs — the second with two entries.
    fn their_hooks(newline: &str, unit: &str) -> String {
        let lines = [
            "{",
            "@\"description\": \"mine\",",
            "@\"hooks\": {",
            "@@\"PreToolUse\": [",
            "@@@{ \"matcher\": \"Bash\", \"hooks\": [ { \"type\": \"command\", \"command\": \"guard\" } ] }",
            "@@],",
            "@@\"Stop\": [",
            "@@@{ \"hooks\": [ { \"type\": \"command\", \"command\": \"ding\" } ] },",
            "@@@{",
            "@@@@\"hooks\": [",
            "@@@@@{ \"type\": \"command\", \"command\": \"log\", \"timeout\": 5 },",
            "@@@@@{ \"type\": \"command\", \"command\": \"two\", \"statusMessage\": \"\\u00e9\" }",
            "@@@@]",
            "@@@}",
            "@@]",
            "@}",
            "}",
        ];
        let mut text = lines
            .iter()
            .map(|line| line.replace('@', unit))
            .collect::<Vec<_>>()
            .join(newline);
        text.push_str(newline);
        text
    }

    /// RED (0.4.10 plan row 69a; ruling R16) — **Folio's hook is appended after the person's
    /// groups, so every approval they gave one of theirs stays attached to it**, whatever the
    /// layout: every byte outside Folio's group is the file's, and every entry of theirs stands at
    /// the event, group and place it stood at. Taking the hook back out hands back the file.
    ///
    /// RED GATE: `plant-69-hook-prepended.py` writes Folio's group first in `Stop`: every place of
    /// theirs in it moves, the self-check refuses the write, and the install is not `Installed`.
    #[test]
    fn the_codex_stop_hook_is_appended_after_the_persons_groups() {
        for (name, newline, unit) in [
            ("lf", "\n", "  "),
            ("crlf", "\r\n", "    "),
            ("tabs", "\n", "\t"),
        ] {
            let dir = scratch(&format!("appended-{name}"));
            let path = dir.join(HOOKS_FILE);
            let theirs = their_hooks(newline, unit);
            std::fs::write(&path, &theirs).expect("their own file");
            let before: Json = serde_json::from_str(&theirs).expect("JSON");
            assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed, "{name}");
            let written = std::fs::read_to_string(&path).expect("read back");
            let after = hooks_of(&path);
            // Every entry of theirs, at the place codex keys their approval of it by.
            for (event, group, entry) in [
                ("PreToolUse", 0, 0),
                ("Stop", 0, 0),
                ("Stop", 1, 0),
                ("Stop", 1, 1),
            ] {
                assert_eq!(
                    after["hooks"][event][group]["hooks"][entry],
                    before["hooks"][event][group]["hooks"][entry],
                    "{name}: {event}:{group}:{entry}"
                );
            }
            assert_eq!(after["hooks"]["Stop"].as_array().map(Vec::len), Some(3));
            assert_eq!(
                after["hooks"]["Stop"][2]["hooks"][0]["command"],
                Json::from(hook_command(&exe(), STOP)),
                "{name}: Folio's is the last group"
            );
            // Laid out as their file is: its line endings, its indentation.
            assert_eq!(written.contains("\r\n"), newline == "\r\n", "{name}");
            assert!(
                written.contains(&format!("{newline}{}\"async\": true,", unit.repeat(6))),
                "{name}: {written}"
            );
            assert!(written.contains(r#""statusMessage": "\u00e9""#), "{name}");
            assert_eq!(
                codex_state_in(&dir, Some(&exe()), true),
                State::Installed,
                "{name}"
            );
            assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed, "{name}");
            assert_eq!(
                std::fs::read_to_string(&path).expect("read back"),
                theirs,
                "{name}: turning the switch off hands back the file that was there"
            );
        }
    }

    /// RED (0.4.10 plan row 69a; ruling R16, which amends 67a's D1 for this file alone) —
    /// **taking Folio's hook out from in front of somebody else's group empties it where it
    /// stands**, so their group keeps its index; with nothing after it, it goes. A copy that is
    /// gone is repaired in its own place.
    ///
    /// RED GATE: `plant-69-removal-shifts.py` takes the group out whatever follows it: the group
    /// after it moves up one, the self-check refuses the write, and the middle case is not
    /// `Removed`.
    #[test]
    fn removing_it_before_someone_elses_group_keeps_their_index() {
        let dir = scratch("keeps-index");
        let path = dir.join(HOOKS_FILE);
        let own = own_stop(&exe());
        let p0 = r#"{"type":"command","command":"p0"}"#;
        let p1 = r#"{"type":"command","command":"p1"}"#;
        let middle = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{p0}]}},{{"hooks":[{own}]}},{{"hooks":[{p1}]}}]}}}}"#
        );
        std::fs::write(&path, &middle).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        let emptied = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{p0}]}},{{"hooks":[]}},{{"hooks":[{p1}]}}]}}}}"#
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), emptied);

        // Over lines: the group stays, its `hooks` emptied, every line of theirs as it was.
        let over_lines = |entry: Option<&str>| {
            let mut lines = vec![
                "{".to_owned(),
                "@\"hooks\": {".to_owned(),
                "@@\"Stop\": [".to_owned(),
                "@@@{".to_owned(),
                "@@@@\"hooks\": [".to_owned(),
                format!("@@@@@{p0}"),
                "@@@@]".to_owned(),
                "@@@},".to_owned(),
                "@@@{".to_owned(),
            ];
            match entry {
                Some(entry) => lines.extend([
                    "@@@@\"hooks\": [".to_owned(),
                    format!("@@@@@{entry}"),
                    "@@@@]".to_owned(),
                ]),
                None => lines.push("@@@@\"hooks\": []".to_owned()),
            }
            lines.extend([
                "@@@},".to_owned(),
                "@@@{".to_owned(),
                "@@@@\"hooks\": [".to_owned(),
                format!("@@@@@{p1}"),
                "@@@@]".to_owned(),
                "@@@}".to_owned(),
                "@@]".to_owned(),
                "@}".to_owned(),
                "}".to_owned(),
            ]);
            spaced(&lines)
        };
        std::fs::write(&path, over_lines(Some(&own))).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            over_lines(None)
        );

        // Last in `Stop`: nothing after it to keep in place, so it goes, with its comma.
        let last = format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{p0}]}},{{"hooks":[{own}]}}]}}}}"#);
        std::fs::write(&path, &last).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{p0}]}}]}}}}"#)
        );

        // The group left empty is nobody's to claim: the next install appends after it, and the
        // removal after that takes only what that install wrote.
        std::fs::write(&path, &emptied).expect("their own file");
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(
            hooks_of(&path)["hooks"]["Stop"][3]["hooks"][0]["command"],
            Json::from(hook_command(&exe(), STOP))
        );
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), emptied);

        // A copy that is gone, in the middle: an install claims its entry where it stands.
        let gone = exe().parent().unwrap().join("gone").join("folio.exe");
        let repaired = middle.replace(&own, &own_stop(&gone));
        std::fs::write(&path, &repaired).expect("their own file");
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), middle);
    }

    /// **An entry of Folio's shape standing beside the person's in one group is not Folio's** under
    /// codex's keying: taking it out would move theirs. It is left; an install adds Folio's own
    /// group after it.
    #[test]
    fn an_entry_sharing_a_group_with_the_persons_is_left_where_it_is() {
        let dir = scratch("shared-group");
        let path = dir.join(HOOKS_FILE);
        let own = own_stop(&exe());
        let theirs = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"p0"}},{own}]}}]}}}}"#
        );
        std::fs::write(&path, &theirs).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), theirs);
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let after = hooks_of(&path);
        assert_eq!(
            after["hooks"]["Stop"][0],
            serde_json::from_str::<Json>(&theirs).expect("JSON")["hooks"]["Stop"][0]
        );
        assert_eq!(after["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
    }

    /// **Row 67b's ruling N1 holds for codex's hook** (merged 2026-11-06): a `timeout` or `async`
    /// the person set on this copy's entry is theirs, not drift; a copy of the entry they put under
    /// another event is their placing, kept and said; and a gone copy's entry is pointed at this
    /// copy where it stands, with the person's keys on it and the person's group after it at the
    /// index it had, so no approval is detached.
    #[test]
    fn a_persons_edit_of_folios_codex_entry_is_theirs() {
        let dir = scratch("n1-edit");
        let path = dir.join(HOOKS_FILE);
        let edited = |exe: &Path| {
            let mut entry = hook_entry(STOP, exe);
            entry["timeout"] = 30.into();
            entry["async"] = false.into();
            entry.to_string()
        };
        let mine = edited(&exe());
        let file = format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{mine}]}}]}}}}"#);
        std::fs::write(&path, &file).expect("this copy's entry, edited");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).expect("there"), file);

        let group = format!(r#"[{{"hooks":[{mine}]}}]"#);
        let placed = format!(r#"{{"hooks":{{"Stop":{group},"SessionStart":{group}}}}}"#);
        std::fs::write(&path, &placed).expect("a copy under another event");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Placed(vec!["SessionStart".to_owned()])
        );
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Unchanged);
        assert_eq!(std::fs::read_to_string(&path).expect("there"), placed);

        let gone = dir.join("gone").join("folio.exe");
        let theirs = edited(&gone);
        let person = r#"{"hooks":[{"type":"command","command":"p0"}]}"#;
        let file = format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{theirs}]}},{person}]}}}}"#);
        std::fs::write(&path, &file).expect("a gone copy's entry, edited");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Dead(vec![gone.clone()])
        );
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let after = hooks_of(&path);
        let stop = after["hooks"]["Stop"].as_array().expect("Stop");
        assert_eq!(stop.len(), 2, "{after}");
        assert_eq!(
            stop[0]["hooks"][0],
            serde_json::from_str::<Json>(&mine).expect("JSON")
        );
        assert_eq!(stop[1]["hooks"][0]["command"], "p0");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
    }

    /// **A `hooks.json` codex would ignore is never written into**, and one this build cannot read
    /// is never written at all: a hook added to a file codex refuses whole would never run. The
    /// last five are review N2's: a key named twice where codex reads a struct, a handler type
    /// codex does not know, a `matcher` that is not a string, a `timeout` that is a string.
    ///
    /// MUTATION: `plant-69-hooks-file-loosely-read.py` judges only the file's top-level keys again,
    /// and those five install.
    #[test]
    fn a_hooks_file_codex_would_ignore_is_never_written() {
        let ignored = crate::i18n::Text::CodexHooksFileIgnored.text();
        for theirs in [
            r#"{"description":"a","description":"b"}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"http","url":"x"}]}]}}"#,
            r#"{"hooks":{"PreToolUse":[{"matcher":5,"hooks":[]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a","command":"b"}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"x","timeout":"5"}]}]}}"#,
        ] {
            let dir = scratch("ignored-strict");
            let path = dir.join(HOOKS_FILE);
            std::fs::write(&path, theirs).expect("their own file");
            assert_eq!(
                apply_to(&path, true, &exe()),
                Outcome::Refused(ignored),
                "{theirs}"
            );
            assert_eq!(
                apply_to(&path, false, &exe()),
                Outcome::Unchanged,
                "{theirs}"
            );
            assert_eq!(std::fs::read_to_string(&path).expect("there"), theirs);
            assert_eq!(names(&dir), vec![HOOKS_FILE.to_owned()], "{theirs}");
        }
        for (theirs, installing, removing) in [
            (
                "\u{feff}{\"hooks\": {}}\n",
                Outcome::Refused(ignored),
                Outcome::Unchanged,
            ),
            (
                "{\"model\": \"gpt-5\"}\n",
                Outcome::Refused(ignored),
                Outcome::Unchanged,
            ),
            (
                "{\"description\": 5}\n",
                Outcome::Refused(ignored),
                Outcome::Unchanged,
            ),
            (
                "{\n  // mine\n  \"hooks\": {}\n}\n",
                Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text()),
                Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text()),
            ),
            (
                "{\"hooks\": {\"Stop\": [], \"Stop\": []}}\n",
                Outcome::Refused(crate::i18n::Text::AgentHooksDuplicateKey.text()),
                Outcome::Refused(crate::i18n::Text::AgentHooksDuplicateKey.text()),
            ),
        ] {
            let dir = scratch("ignored");
            let path = dir.join(HOOKS_FILE);
            std::fs::write(&path, theirs).expect("their own file");
            assert_eq!(apply_to(&path, true, &exe()), installing, "{theirs}");
            assert_eq!(apply_to(&path, false, &exe()), removing, "{theirs}");
            assert_eq!(std::fs::read_to_string(&path).expect("there"), theirs);
            assert_eq!(names(&dir), vec![HOOKS_FILE.to_owned()], "{theirs}");
        }
        // **Taking Folio's hook out of such a file still works**, byte for byte: what it removes is
        // only Folio's, and what codex refuses about the file is the person's to fix.
        let dir = scratch("ignored-removal");
        let path = dir.join(HOOKS_FILE);
        let theirs = "\u{feff}{\"model\": \"gpt-5\"}\r\n";
        let with_ours = format!(
            "\u{feff}{{\"model\": \"gpt-5\", \"hooks\": {{\"Stop\": [{{\"hooks\": [{}]}}]}}}}\r\n",
            own_stop(&exe())
        );
        std::fs::write(&path, &with_ours).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), theirs);
    }

    /// **The mirror of codex's `HooksFile` reads what codex reads** (review N2): every handler
    /// codex knows, in every spelling it takes, and an event it does not know passed over; and
    /// refuses what codex refuses — each of its 14 events ([`TOML_HOOK_EVENTS`], so the mirror
    /// and the list are one) holding something that is not a list, a key named twice through the
    /// alias codex accepts, an `mcp_tool` input TOML cannot hold, an `async` that is not a boolean.
    #[test]
    fn the_hooks_file_is_judged_by_codexs_own_types() {
        let own = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{}]}}]}}}}"#,
            own_stop(&exe())
        );
        for read in [
            "{}",
            "{}\n",
            r#"{"description":null,"hooks":{}}"#,
            own.as_str(),
            r#"{"hooks":{"NotAnEvent":5}}"#,
            r#"{"hooks":{"PreToolUse":[{"matcher":null,"x":1,"hooks":[{"type":"command","command":"a","commandWindows":"b","timeout":5,"async":false,"statusMessage":"s","additionalContextLimit":0,"extra":true}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a","command_windows":"b"}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"mcp_tool","server":"s","tool":"t","input":{"a":[1,"x",{"b":true}]}}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"prompt","prompt":"x"},{"type":"agent"}]}]}}"#,
        ] {
            assert!(read_by_codex(read), "{read}");
        }
        for event in TOML_HOOK_EVENTS {
            let refused = format!(r#"{{"hooks":{{"{event}":5}}}}"#);
            assert!(!read_by_codex(&refused), "{refused}");
        }
        for refused in [
            "",
            "\u{feff}{}",
            r#"{"hooks":{"Stop":[],"Stop":[]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a","commandWindows":"b","command_windows":"c"}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command"}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a","async":null}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a","timeout":-1}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"mcp_tool","server":"s","tool":"t","input":{"a":null}}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"mcp_tool","server":"s","tool":"t","input":{"a":18446744073709551615}}]}]}}"#,
            r#"{"hooks":{"Stop":[{"hooks":[{"command":"a"}]}]}}"#,
        ] {
            assert!(!read_by_codex(refused), "{refused}");
        }
    }

    /// RED (0.4.10 plan row 69a; ruling R15) — **hook events in config.toml keep Folio out of
    /// `hooks.json`**: codex would warn about two representations on every start, so nothing is
    /// written, and the refusal says why. codex's own approvals table alone is not one, and
    /// neither is an empty list.
    #[test]
    fn toml_hook_events_keep_folio_out_of_hooks_json() {
        let in_toml = crate::i18n::Text::CodexHooksInToml.text();
        for (theirs, expected) in [
            (
                "[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"ding\"\n",
                Outcome::Refused(in_toml),
            ),
            (
                "[hooks]\nPreToolUse = [{ matcher = \"Bash\", hooks = [] }]\n",
                Outcome::Refused(in_toml),
            ),
            (
                "model = \"gpt-5\"\nhooks.SessionStart = [{ hooks = [] }]\n",
                Outcome::Refused(in_toml),
            ),
            (
                "[hooks.state.\"x:stop:0:0\"]\ntrusted_hash = \"sha256:0\"\n",
                Outcome::Installed,
            ),
            ("[hooks]\nStop = []\n", Outcome::Installed),
        ] {
            let dir = scratch("toml-events");
            let config = dir.join(CONFIG_FILE);
            std::fs::write(&config, theirs).expect("their own file");
            assert_eq!(
                apply_to(&dir.join(HOOKS_FILE), true, &exe()),
                expected,
                "{theirs}"
            );
            assert_eq!(std::fs::read_to_string(&config).expect("there"), theirs);
            if expected != Outcome::Installed {
                assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned()], "{theirs}");
                // A removal is never refused for it: what it takes out is only Folio's.
                assert_eq!(
                    apply_to(&dir.join(HOOKS_FILE), false, &exe()),
                    Outcome::Unchanged
                );
            }
        }
    }

    /// The owner's own `config.toml` (row 69's brief): Codex's computer use on `notify`, in its
    /// two-word shape, with keys before and after it and a table.
    fn owners_config(newline: &str, mark: &str) -> String {
        [
            "model = \"gpt-5.5\"",
            r#"notify = [ "C:\\Users\\someone\\AppData\\Local\\OpenAI\\Codex\\runtimes\\cua_node\\0a1b2c3d\\bin\\node_modules\\@oai\\sky\\bin\\windows\\codex-computer-use.exe", "turn-ended" ] # theirs"#,
            "approval_policy = \"on-request\"",
            "",
            "[tui]",
            "notifications = true",
            "",
        ]
        .join(newline)
        .replacen("model", &format!("{mark}model"), 1)
    }

    /// RED (0.4.10 plan row 69; the owner's case) — **a `notify` line of somebody else's no longer
    /// keeps Folio out**: the hook is written beside it, and the line — Codex's own computer use,
    /// in the owner's shape — is never touched, on the way in or out.
    #[test]
    fn the_owners_notify_line_stays_and_the_hook_goes_beside_it() {
        for (name, newline, mark) in [("lf", "\n", ""), ("crlf-bom", "\r\n", "\u{feff}")] {
            let dir = scratch(&format!("owners-{name}"));
            let config = dir.join(CONFIG_FILE);
            let theirs = owners_config(newline, mark);
            std::fs::write(&config, &theirs).expect("their own file");
            let path = dir.join(HOOKS_FILE);
            assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed, "{name}");
            assert_eq!(std::fs::read_to_string(&config).expect("there"), theirs);
            assert_eq!(
                codex_state_in(&dir, Some(&exe()), true),
                State::Installed,
                "{name}"
            );
            // A take-over press is no licence either: the line is nobody's to take over.
            assert_eq!(
                apply_with(&path, Decision::TakeOver(Vec::new()), &exe()),
                Outcome::Unchanged,
                "{name}"
            );
            assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed, "{name}");
            assert_eq!(std::fs::read_to_string(&config).expect("there"), theirs);
            assert_eq!(
                codex_state_in(&dir, Some(&exe()), true),
                State::Absent,
                "{name}"
            );
        }
    }

    /// RED (0.4.10 plan row 69; ruling R18) — **the `notify` line an earlier Folio wrote stays
    /// until Folio writes its entry again, and then goes in the same operation**, byte for byte,
    /// so one turn is never marked twice. A live other copy's line is a take-over; a gone copy's is
    /// replaced without one; turning the switch off takes out both.
    ///
    /// RED GATE: `plant-69-both-entries.py` leaves the line beside the hook an install writes, and
    /// `config.toml` still holds it after the first press.
    #[test]
    fn an_earlier_folios_notify_line_goes_when_the_hook_is_written() {
        let dir = scratch("earlier-line");
        let config = dir.join(CONFIG_FILE);
        let path = dir.join(HOOKS_FILE);
        let line = |exe: &Path| {
            format!(
                "# mine\r\nmodel = \"gpt-5\"\r\nnotify = {}\r\n[tui]\r\nx = 1\r\n",
                notify_value(exe)
            )
        };
        let without = "# mine\r\nmodel = \"gpt-5\"\r\n[tui]\r\nx = 1\r\n";
        // Kept and read while nothing is pressed: the row is on.
        std::fs::write(&config, line(&exe())).expect("their own file");
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Installed);
        // A press writes the hook and takes the line out.
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Installed);
        // Both of Folio's entries, by hand or across an upgrade: off takes out both.
        std::fs::write(&config, line(&exe())).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Absent);

        // A live other copy's line asks for a take-over, and the consent covers the line.
        let other = dir.join("other").join("folio.exe");
        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
        std::fs::write(&other, b"another copy").unwrap();
        std::fs::write(&config, line(&other)).expect("their own file");
        assert_eq!(
            apply_to(&path, true, &exe()),
            Outcome::TakeOverRequired(vec![other.clone()])
        );
        assert_eq!(
            std::fs::read_to_string(&config).expect("there"),
            line(&other)
        );
        assert_eq!(
            apply_with(&path, Decision::TakeOver(vec![other.clone()]), &exe()),
            Outcome::Installed
        );
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
        // And a removal leaves a live other copy's line where it is.
        std::fs::write(&config, line(&other)).expect("their own file");
        assert_eq!(
            apply_to(&path, false, &exe()),
            Outcome::LeftOther(vec![other.clone()])
        );
        assert_eq!(
            std::fs::read_to_string(&config).expect("there"),
            line(&other)
        );

        // A copy that is gone needs no consent.
        let gone = dir.join("gone").join("folio.exe");
        std::fs::write(&config, line(&gone)).expect("their own file");
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
    }

    /// RED (0.4.10 plan row 69; the coordinator, 2026-11-04) — **a `config.toml` the operation must
    /// refuse leaves `hooks.json` as it was**: both edits are made and judged, and every refusal
    /// said, before either file is written; then `config.toml` is written first. No refusal is
    /// half done, and the one window left — a second write that fails — loses the `notify` line
    /// before the hook is written, so a turn is never marked twice.
    ///
    /// RED GATE: `plant-69-hook-before-refusal.py` writes `hooks.json` before the `config.toml`
    /// judgement, and the hooks file comes back with Folio's group in it;
    /// `plant-69-hooks-landed-first.py` writes the two files the other way round, and the reading of
    /// `apply_resolved` below finds it.
    #[test]
    fn a_config_toml_the_writer_refuses_leaves_hooks_json_untouched() {
        let dir = scratch("refused-toml");
        let config = dir.join(CONFIG_FILE);
        let path = dir.join(HOOKS_FILE);
        let theirs = their_hooks("\n", "  ");
        std::fs::write(&path, &theirs).expect("their own file");
        // This copy's `notify` line, in a file Folio reads and will not edit: the install has to
        // take the line out, so the press is refused.
        let line = format!("model = \"gpt-5\"\nnotify = {}\n", notify_value(&exe()));
        std::fs::write(&config, &line).expect("their own file");
        let writable = std::fs::metadata(&config).expect("there").permissions();
        let mut readonly = writable.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&config, readonly).expect("read-only");
        let outcome = apply_to(&path, true, &exe());
        let hooks_after = std::fs::read_to_string(&path).expect("there");
        let config_after = std::fs::read_to_string(&config).expect("there");
        let after = names(&dir);
        // Restored before the assertions, not after them: a panic in between would leave the
        // fixture read-only (attention_ownership's review R14).
        std::fs::set_permissions(&config, writable).expect("writable again");
        assert_eq!(
            outcome,
            Outcome::Refused(crate::i18n::Text::AgentConfigReadOnly.text())
        );
        assert_eq!(hooks_after, theirs, "the hook was not written");
        assert_eq!(config_after, line, "the line was not taken out");
        assert_eq!(
            after,
            vec![CONFIG_FILE.to_owned(), HOOKS_FILE.to_owned()],
            "no copy was kept of either: nothing was about to be written"
        );
        // Put right, the same press writes both.
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(
            std::fs::read_to_string(&config).expect("there"),
            "model = \"gpt-5\"\n"
        );

        // **`config.toml` lands first**: the order is the property, and no fixture can make the
        // second of two atomic replaces fail, so it is read off the operation itself.
        let body = crate::test_support::squeezed(crate::test_support::item_body(
            &bt_source::ItemQuery::function("apply_resolved").in_module("crate::attention_codex"),
        ));
        let toml_lands = body
            .find("land_codex_file(toml_config,toml_text,\"toml\",&taken_out)")
            .expect("config.toml is landed here");
        let hooks_land = body
            .find("land_codex_file(config,&existing,\"json\",&written)")
            .expect("hooks.json is landed here");
        assert!(toml_lands < hooks_land, "config.toml is written first");
        // An install's refusals are `install_refusal`'s (the 69c review's C3), asked before either
        // write — among them a config.toml Folio will not edit, and the text hooks.json will hold
        // judged by codex's types.
        let judged = body
            .find("ifinstall&&letSome(reason)=install_refusal(InstallRoad::Hook{")
            .expect("an install's refusals are asked here");
        assert!(judged < toml_lands, "and refused before either write");
        let refusals = crate::test_support::squeezed(crate::test_support::item_body(
            &bt_source::ItemQuery::function("install_refusal").in_module("crate::attention_codex"),
        ));
        assert!(
            refusals.contains("Ok(Some(_))=>returntoml.config.as_ref().err().copied(),"),
            "a config.toml Folio will not edit, holding the line, is refused: {refusals}"
        );
        assert!(
            refusals.contains("if!read_by_codex(written.as_deref().unwrap_or(existing)){"),
            "the text hooks.json will hold is judged by codex's types: {refusals}"
        );
    }

    /// RED (0.4.10 plan row 69, re-review R2) — **a `config.toml` Folio reads and will not edit
    /// does not keep Folio's hook in `hooks.json` either**: switching off, and so an uninstall,
    /// takes the hook out, leaves the file holding this copy's `notify` line as it was, and says
    /// why. A removal cannot mark a turn twice, so the install's whole refusal is not its rule.
    /// Before, the removal was refused, and an uninstalled Folio left both behind.
    ///
    /// MUTATION: `plant-69-removal-refused-unwritable.py` refuses the removal over it again.
    #[test]
    fn a_config_toml_folio_will_not_edit_does_not_keep_folios_hook_in() {
        let dir = scratch("unwritable-toml-off");
        let config = dir.join(CONFIG_FILE);
        let path = dir.join(HOOKS_FILE);
        let with_ours = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{}]}}]}}}}"#,
            own_stop(&exe())
        );
        std::fs::write(&path, &with_ours).expect("a hook of this copy's");
        let line = format!("model = \"gpt-5\"\nnotify = {}\n", notify_value(&exe()));
        std::fs::write(&config, &line).expect("their own file");
        let writable = std::fs::metadata(&config).expect("there").permissions();
        let mut readonly = writable.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&config, readonly).expect("read-only");
        let outcome = apply_to(&path, false, &exe());
        let hooks_after = std::fs::read_to_string(&path).expect("there");
        let config_after = std::fs::read_to_string(&config).expect("there");
        // Restored before the assertions, not after them: a panic in between would leave the
        // fixture read-only (attention_ownership's review R14).
        std::fs::set_permissions(&config, writable).expect("writable again");
        assert_eq!(
            outcome,
            Outcome::LeftUnread {
                removed: true,
                reason: crate::i18n::Text::AgentConfigReadOnly.text()
            }
        );
        assert_eq!(hooks_after, "{}", "Folio's hook is out");
        assert_eq!(config_after, line, "the line stays, as the file was");
        // Put right, the same press takes the line out too.
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&config).expect("there"),
            "model = \"gpt-5\"\n"
        );
    }

    /// RED (0.4.10 plan row 69, re-review R1) — **an install from a path Windows PowerShell 5.1
    /// cannot call is refused on Windows**, where codex runs the hook through it and the call would
    /// fail on every turn; a path with matched brackets installs; a removal is never refused for
    /// it; off Windows the hook is sh's and both install.
    ///
    /// MUTATION: `plant-69-brackets-unrefused.py` drops the refusal.
    #[test]
    fn a_path_windows_powershell_cannot_call_is_refused_there() {
        let dir = scratch("brackets");
        let unmatched = dir.join("a[b").join("folio.exe");
        let path = dir.join(HOOKS_FILE);
        let installing = apply_to(&path, true, &unmatched);
        if bt_platform::host_platform() == HostPlatform::Windows {
            assert_eq!(
                installing,
                Outcome::Refused(crate::i18n::Text::CodexHookPathBrackets.text())
            );
            assert!(!path.exists(), "nothing was written");
        } else {
            assert_eq!(installing, Outcome::Installed);
        }
        let own = own_stop(&unmatched);
        std::fs::write(
            &path,
            format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{own}]}}]}}}}"#),
        )
        .expect("a hook of this copy's");
        assert_eq!(apply_to(&path, false, &unmatched), Outcome::Removed);
        let matched = dir.join("Folio [portable]").join("folio.exe");
        assert_eq!(apply_to(&path, true, &matched), Outcome::Installed);
    }

    /// **The pattern Windows PowerShell 5.1 reads a call's path as** — every folder name of the
    /// 2026-11-05 measurement (row 69's report, re-review R1), each run as `& '<path>' a` with piped
    /// stdin through 5.1 and pwsh 7: the first list ran under both, the second failed under 5.1
    /// alone. A `\` inside a name stands for two folders.
    #[test]
    fn windows_powershell_takes_the_paths_it_was_measured_to_take() {
        let runs = [
            "plain",
            "a]b",
            "x [portable] y",
            "a[[b]",
            "a[b]]c",
            "a`[b",
            "a`b",
            "tail`",
            "a[`]b",
            "a[b`]c",
            "]",
            "a[b[c]d]e",
            "a[]]b",
            "a[]b]c",
            "a``[b",
            "a`]b",
            "a`[b]",
            "a[`]",
            "[a]",
            "a [] b ]",
            "a[b`]",
            "x[a-c]y",
            "x[-]y",
            "x[a-]y",
            "x[!a]y",
            "x[^a]y",
            "tail`[",
            "x[]-a]y",
            "x[a-c-e]y",
            "x[0-9]y",
            "x[$]y",
            "x[.]y",
            "x[(]y",
            "x[{]y",
            "x[a b]y",
            "x[!]y",
            "x[^]y",
            "x[a-a]y",
            "x[文]y",
            "x[a-文]y",
            "x[---]y",
            "x[--a]y",
            "x[`-a]y",
            "x[a-c-[d]y",
            "x[a-b-c]y",
            "x[-[]y",
            "x[a-c-]y",
            "x[a-c--]y",
            "x[a-c-a]y",
            "y[Z-a]y",
            "y[[-a]y",
            "y[a-b-[c]y",
            "y[-a-]y",
            "y[!-#]y",
            "y[\u{1F600}]y",
        ];
        let fails = [
            "a[b",
            "a[]b",
            "a[b]c[d",
            "a]b[c",
            "[",
            "[]",
            "][",
            "a[`",
            "a[b`",
            "a[`[b",
            "x[c-a]y",
            "x[e-c-a]y",
            "x[a-c][z-a]y",
            "x[9-0]y",
            "x[文-a]y",
            "x[a--]y",
            r"p[b\c]q",
            r"p[x\y]q",
            r"p[a\d]q",
            r"p[a\]q",
            "x[A-[]y",
            "x[A-[b]y",
            "x[!-[]y",
            "y[a-Z]y",
            "y[a-c-[d-a]y",
            "y[b-[-]y",
            "y[--[]y",
        ];
        for name in runs {
            let path = format!(r"C:\Users\someone\{name}\folio.exe");
            assert!(windows_powershell_finds(&path), "{name} ran under 5.1");
        }
        for name in fails {
            let path = format!(r"C:\Users\someone\{name}\folio.exe");
            assert!(!windows_powershell_finds(&path), "{name} failed under 5.1");
        }
    }

    /// RED (ruling R13) — **Folio never writes codex's record of the person's approval.** Said over
    /// the module's product code, because the rule's only form is an absence: no literal of it
    /// names the table codex keeps approvals in, or the hash it keeps there.
    #[test]
    fn folio_never_writes_codexs_trust_ledger() {
        for name in ["trusted_hash", "hooks.state", "\"state\""] {
            let found = crate::test_support::found_in(
                Needle::new(Pattern::text(name)),
                View::CodeKeepingLiterals,
                Scope::Module("crate::attention_codex".to_owned()),
            )
            .in_the_product(crate::test_support::source());
            assert!(found.is_empty(), "`{name}` is codex's to write");
        }
    }

    // ── 0.4.10 plan row 69c: the entry by codex's version ──────────────────

    fn codex_at(major: u32, minor: u32, patch: u32) -> CodexVersion {
        CodexVersion::Known(release_at(major, minor, patch, None))
    }

    fn release_at(major: u32, minor: u32, patch: u32, prerelease: Option<&str>) -> CodexRelease {
        CodexRelease {
            version: Version::new(major, minor, patch),
            prerelease: prerelease.map(str::to_owned),
        }
    }

    /// A prerelease of `major.minor.patch`.
    fn codex_pre(major: u32, minor: u32, patch: u32, prerelease: &str) -> CodexVersion {
        CodexVersion::Known(release_at(major, minor, patch, Some(prerelease)))
    }

    /// A press on the file a version chooses, with its own marks folder — what [`apply`] does once
    /// [`pressed_path`] has named the file.
    fn press_by_version(dir: &Path, version: &CodexVersion, install: bool) -> Outcome {
        let file = if install && !version.runs_hooks() {
            CONFIG_FILE
        } else {
            HOOKS_FILE
        };
        apply_to(&dir.join(file), install, &exe())
    }

    /// One scan of the file a version chooses, the machine's own answer about what stands.
    fn scan_by_version(dir: &Path, version: &CodexVersion) -> HooksFound {
        let file = if version.runs_hooks() {
            HOOKS_FILE
        } else {
            CONFIG_FILE
        };
        let config = Config::resolve(&dir.join(file))
            .unwrap_or_else(|_| panic!("a fixture resolves"))
            .with_one_read();
        scan_resolved(&config, &exe(), &ownership::what_stands)
    }

    /// RED (0.4.10 plan row 69c; rulings R12, R17) — **one entry per codex version**: from 0.148 the
    /// hook in `hooks.json`; below it the `notify` line in `config.toml`, as every Folio before
    /// 0.4.10 wrote it; below it with somebody else's program on the key, the refusal names the
    /// version; and on a version nobody could learn, the hook. A Folio line beside this copy's hook
    /// is an old shape the scan wants repaired, and the repair takes the line out.
    ///
    /// RED GATE: `plant-69c-version-gate-skipped.py` answers every version as one that runs hooks,
    /// and the press on 0.147 writes a hook codex skips; `plant-69c-prerelease-runs-hooks.py` reads
    /// a prerelease of 0.148.0 as 0.148.0 (the 69c review's C1).
    #[test]
    fn one_codex_entry_per_version() {
        assert!(!codex_at(0, 147, 9).runs_hooks());
        assert!(!codex_at(0, 114, 0).runs_hooks());
        assert!(codex_at(0, 148, 0).runs_hooks());
        assert!(codex_at(0, 166, 0).runs_hooks());
        assert!(
            CodexVersion::Unknown.runs_hooks(),
            "R17: unknown is current"
        );
        assert!(CodexVersion::Unasked.runs_hooks());
        // The 69c review's C1: semver orders a prerelease before the release it leads up to.
        assert!(
            !codex_pre(0, 148, 0, "alpha.5").runs_hooks(),
            "0.148.0-alpha.5 is older than 0.148.0"
        );
        assert!(
            codex_pre(0, 149, 0, "alpha.1").runs_hooks(),
            "a prerelease of a later version is past 0.148.0"
        );
        assert!(!codex_pre(0, 147, 0, "alpha.2").runs_hooks());
        assert_eq!(
            release_at(0, 148, 0, Some("alpha.5")).to_string(),
            "0.148.0-alpha.5",
            "the row and the diagnostics line name the prerelease"
        );

        // 0.148 and later: the hook, and config.toml untouched.
        let dir = scratch("version-hook");
        assert_eq!(
            press_by_version(&dir, &codex_at(0, 148, 0), true),
            Outcome::Installed
        );
        assert_eq!(names(&dir), vec![HOOKS_FILE.to_owned()]);
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 148, 0)),
            HooksFound::Current
        );

        // Below 0.148, the key free: the line, and no hooks.json.
        let dir = scratch("version-line");
        assert_eq!(
            press_by_version(&dir, &codex_at(0, 147, 0), true),
            Outcome::Installed
        );
        assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned()]);
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("written"),
            format!("notify = {}\n", notify_value(&exe()))
        );
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 147, 0)),
            HooksFound::Current
        );
        // Turning it off takes the line out on every version.
        assert_eq!(
            press_by_version(&dir, &codex_at(0, 147, 0), false),
            Outcome::Removed
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            ""
        );

        // Below 0.148, the key held: refused, the version and the program named, nothing written.
        let dir = scratch("version-held");
        let theirs = owners_config("\n", "");
        std::fs::write(dir.join(CONFIG_FILE), &theirs).expect("their own file");
        assert!(matches!(
            apply_to(&dir.join(CONFIG_FILE), true, &exe()),
            Outcome::Refused(_)
        ));
        let words = words_of(&theirs.parse::<DocumentMut>().expect("TOML")).expect("words");
        assert_eq!(
            held_slot(&CodexVersion::Unknown, &words),
            crate::i18n::Text::AgentCodexNotifyTaken.into()
        );
        let named = held_slot(&codex_at(0, 147, 0), &words).text();
        assert!(named.contains("0.147.0"), "{named}");
        assert!(named.contains("codex-computer-use.exe"), "{named}");
        assert!(named.contains("0.148"), "{named}");
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            theirs
        );
        assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned()]);

        // Both of Folio's entries, this copy's: drift, and the repair leaves one.
        let dir = scratch("version-both");
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed
        );
        std::fs::write(
            dir.join(CONFIG_FILE),
            format!("notify = {}\n", notify_value(&exe())),
        )
        .expect("the line beside it");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Drifted
        );
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            ""
        );
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
    }

    /// RED (ruling R18) — **this copy's working `notify` line, alone, is current on a codex that runs
    /// hooks**: the scan does not move it into an entry codex would ask the person to trust. A gone
    /// copy's line is dead, and its repair writes the hook and takes the line out.
    ///
    /// RED GATE: `plant-69c-line-alone-drifts.py` reads a line beside no hook as an old shape, and
    /// the first assertion reads `Drifted`.
    #[test]
    fn a_working_line_stays_until_folio_writes_its_entry() {
        let dir = scratch("line-alone");
        let line = format!("model = \"gpt-5\"\nnotify = {}\n", notify_value(&exe()));
        std::fs::write(dir.join(CONFIG_FILE), &line).expect("an earlier Folio's line");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            line,
            "a scan writes nothing"
        );

        let gone = dir.join("gone").join("folio.exe");
        let theirs = format!("model = \"gpt-5\"\nnotify = {}\n", notify_value(&gone));
        std::fs::write(dir.join(CONFIG_FILE), &theirs).expect("a gone copy's line");
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Dead(vec![gone.clone()])
        );
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            "model = \"gpt-5\"\n"
        );
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
    }

    /// RED (the coordinator's ruling on 69c's choice 1) — **a hook a press wrote before the version
    /// was known heals at the next start, on a codex below 0.148**: the scan of `config.toml` reads
    /// the hook beside it as drifted (this copy's) or dead (a gone one's), and the install the
    /// repair makes writes the line and takes the hook out, so a codex updated past 0.148 later
    /// does not mark a turn twice. Somebody else's program on the key is still said, and nothing is
    /// written.
    ///
    /// RED GATE: `plant-69c-early-hook-unseen.py` drops the hooks beside `config.toml` from the
    /// scan, and the first assertion reads `Nothing`; `plant-69c-early-hook-left.py` keeps the hook
    /// through the line's install, and the hook is still there.
    #[test]
    fn an_early_hook_on_an_old_codex_is_repaired_into_the_line() {
        let old = codex_at(0, 147, 0);
        let dir = scratch("early-hook");
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed,
            "the press before the version was known"
        );
        assert_eq!(scan_by_version(&dir, &old), HooksFound::Drifted);
        assert_eq!(codex_state_in(&dir, Some(&exe()), false), State::Absent);
        assert_eq!(press_by_version(&dir, &old, true), Outcome::Installed);
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("written"),
            format!("notify = {}\n", notify_value(&exe()))
        );
        let hooks = std::fs::read_to_string(dir.join(HOOKS_FILE)).expect("there");
        assert!(!hooks.contains("attention codex:stop"), "{hooks}");
        assert_eq!(scan_by_version(&dir, &old), HooksFound::Current);
        assert_eq!(codex_state_in(&dir, Some(&exe()), false), State::Installed);

        // A gone copy's hook: dead, and the same repair.
        let dir = scratch("early-hook-gone");
        let gone = dir.join("gone").join("folio.exe");
        let mut hooks = Map::new();
        let mut stop = Map::new();
        stop.insert(
            "hooks".to_owned(),
            Json::Array(vec![hook_wanted(&gone)[0].hook.clone()]),
        );
        hooks.insert(STOP_EVENT.to_owned(), Json::Array(vec![Json::Object(stop)]));
        let mut file = Map::new();
        file.insert("hooks".to_owned(), Json::Object(hooks));
        std::fs::write(
            dir.join(HOOKS_FILE),
            serde_json::to_string_pretty(&Json::Object(file)).expect("renders"),
        )
        .expect("a gone copy's hook");
        assert_eq!(
            scan_by_version(&dir, &old),
            HooksFound::Dead(vec![gone.clone()])
        );
        assert_eq!(press_by_version(&dir, &old, true), Outcome::Installed);
        let hooks = std::fs::read_to_string(dir.join(HOOKS_FILE)).expect("there");
        assert!(!hooks.contains("attention codex:stop"), "{hooks}");
        assert_eq!(scan_by_version(&dir, &old), HooksFound::Current);

        // Somebody else's program on the key: said, and both files as they were.
        let dir = scratch("early-hook-held");
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed
        );
        let theirs = owners_config("\n", "");
        std::fs::write(dir.join(CONFIG_FILE), &theirs).expect("their own file");
        let before = std::fs::read_to_string(dir.join(HOOKS_FILE)).expect("there");
        assert!(matches!(
            scan_by_version(&dir, &old),
            HooksFound::Declined(_)
        ));
        assert!(matches!(
            press_by_version(&dir, &old, true),
            Outcome::Refused(_)
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            theirs
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(HOOKS_FILE)).expect("there"),
            before
        );
    }

    /// RED (review N6) — **a hook codex will not run is never On, and nothing takes a working line
    /// out for one**: below 0.148, in a `hooks.json` codex ignores, or with hooks switched off in
    /// `config.toml`, this copy's hook does not make the row Installed; with hooks switched off an
    /// install is refused and an earlier Folio's line stays where it works.
    ///
    /// RED GATE: `plant-69c-hook-on-when-not-run.py` drops the version's condition from
    /// `codex_state_in`, and the first `Absent` reads `Installed`; `plant-69c-hooks-off-installs.py`
    /// drops the install's refusal with hooks off, and the hook replaces the line that works;
    /// `plant-69c-hooks-off-names-home.py` names `~/.codex/config.toml` whatever was read (the 69c
    /// review's C6), and the sentence does not name the fixture's file.
    #[test]
    fn a_hook_codex_will_not_run_is_never_on() {
        let dir = scratch("not-run");
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Installed
        );
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Installed);
        assert_eq!(codex_state_in(&dir, Some(&exe()), false), State::Absent);
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );

        std::fs::write(dir.join(CONFIG_FILE), "[features]\nhooks = false\n").expect("off");
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Absent);
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Declined(RowSentence::CodexHooksOff(dir.join(CONFIG_FILE)))
        );
        // The 69c review's C6: the sentence names the file read, wherever CODEX_HOME put it.
        let shown = dir.join(CONFIG_FILE).display().to_string();
        let refusal = hooks_off(&dir.join(CONFIG_FILE));
        assert!(refusal.contains(&shown), "{refusal}");
        for lang in [crate::i18n::Lang::English, crate::i18n::Lang::Chinese] {
            let sentence = crate::i18n::codex_hooks_off_in_lang(lang, &shown);
            assert!(sentence.contains(&shown), "{sentence}");
            assert!(!sentence.contains("~/.codex"), "{sentence}");
        }

        let hooks = std::fs::read_to_string(dir.join(HOOKS_FILE)).expect("there");
        let ignored = hooks.replacen('{', "{\"model\": \"gpt-5\", ", 1);
        std::fs::write(dir.join(CONFIG_FILE), "").expect("on again");
        std::fs::write(dir.join(HOOKS_FILE), &ignored).expect("a key codex refuses");
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Absent);
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Declined(crate::i18n::Text::CodexHooksFileIgnored.into())
        );

        // Hooks off and an earlier Folio's line: the line is the integration, and it stays.
        let dir = scratch("not-run-line");
        let line = format!(
            "notify = {}\n[features]\ncodex_hooks = false\n",
            notify_value(&exe())
        );
        std::fs::write(dir.join(CONFIG_FILE), &line).expect("off, and the line");
        assert_eq!(
            apply_to(&dir.join(HOOKS_FILE), true, &exe()),
            Outcome::Refused(hooks_off(&dir.join(CONFIG_FILE)))
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(CONFIG_FILE)).expect("there"),
            line
        );
        assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned()]);
        assert_eq!(codex_state_in(&dir, Some(&exe()), true), State::Installed);
        assert_eq!(
            scan_by_version(&dir, &codex_at(0, 166, 0)),
            HooksFound::Current
        );
    }

    /// PIN (row 69's report §1b.3) — **`[features]`'s hooks switch, by both its names**: `hooks`, and
    /// the `codex_hooks` it was called before; dotted or under a header alike; `hooks` stands where
    /// both are written, as codex applies the table in key order. A profile's table is not read.
    #[test]
    fn the_hooks_switch_is_read_by_both_its_names() {
        for (text, off) in [
            ("", false),
            ("[features]\nhooks = false\n", true),
            ("[features]\ncodex_hooks = false\n", true),
            ("features.hooks = false\n", true),
            ("features = { codex_hooks = false }\n", true),
            ("[features]\nhooks = true\n", false),
            ("[features]\nhooks = true\ncodex_hooks = false\n", false),
            ("[features]\nhooks = false\ncodex_hooks = true\n", true),
            ("[features]\nhooks = \"off\"\n", false),
            ("[profiles.work.features]\nhooks = false\n", false),
        ] {
            let document = text.parse::<DocumentMut>().expect("a fixture");
            assert_eq!(hooks_switched_off(&document), off, "{text:?}");
        }
    }

    /// RED (ruling R17) — **the version is asked once per program file**: not before the walk has
    /// said where codex is, never of a codex that is not there, once for a program, again only when
    /// its size or modified time moved or the walk named another; an answer that did not come is
    /// unknown, said once in the log.
    ///
    /// RED GATE: `plant-69c-version-asked-every-scan.py` drops the stamp comparison, and the second
    /// settle asks again.
    #[test]
    fn the_version_is_asked_once_per_program_file() {
        use std::cell::{Cell, RefCell};
        let held = Mutex::new(VersionBook::new());
        let asked = Cell::new(0);
        let answer = RefCell::new(Asked::Answered(Some(release_at(0, 166, 0, None))));
        let size = Cell::new(10_u64);
        let said = RefCell::new(Vec::new());
        let stamp = |path: &Path| {
            Some(ProgramStamp {
                path: path.to_path_buf(),
                size: size.get(),
                modified: None,
            })
        };
        let ask = |_: &Path| {
            asked.set(asked.get() + 1);
            answer.borrow().clone()
        };
        let say = |line: String| said.borrow_mut().push(line);
        let settle = || settle_version_in(&held, &stamp, &ask, &say);

        assert_eq!(settle(), CodexVersion::Unasked, "the walk has not answered");
        assert!(adopt_walk_program_in(&held, Some(None)));
        assert_eq!(settle(), CodexVersion::Unknown, "no codex where it looked");
        assert_eq!(asked.get(), 0);

        let codex = OsStr::new(r"C:\Users\someone\AppData\Roaming\npm\codex.cmd");
        assert!(adopt_walk_program_in(&held, Some(Some(codex))));
        assert!(
            !adopt_walk_program_in(&held, Some(Some(codex))),
            "the same answer"
        );
        assert_eq!(held.lock().unwrap().version(), CodexVersion::Unasked);
        assert_eq!(settle(), codex_at(0, 166, 0));
        assert_eq!(settle(), codex_at(0, 166, 0));
        assert_eq!(asked.get(), 1, "the same file is not asked twice");
        assert_eq!(*said.borrow(), vec!["BT_AGENT_HOOKS codex version=0.166.0"]);

        size.set(11);
        *answer.borrow_mut() = Asked::Answered(None);
        assert_eq!(settle(), CodexVersion::Unknown, "it would not say");
        assert_eq!(asked.get(), 2, "a changed file is asked again");
        assert_eq!(
            said.borrow().last().map(String::as_str),
            Some("BT_AGENT_HOOKS codex version=unknown")
        );

        let other = OsStr::new(r"C:\Tools\codex.exe");
        *answer.borrow_mut() = Asked::Answered(Some(release_at(0, 147, 0, None)));
        assert!(adopt_walk_program_in(&held, Some(Some(other))));
        assert_eq!(held.lock().unwrap().version(), CodexVersion::Unasked);
        assert_eq!(settle(), codex_at(0, 147, 0));
        assert_eq!(asked.get(), 3, "another program is asked");
    }

    /// RED (the 69c review's C2) — **a question stopped at its deadline is not an answer**: the
    /// version is unknown for that round and the log says so, but nothing is kept, so the next
    /// round asks the same file again; the answer that then comes is the one held. Until it comes,
    /// the start-time repair's gate stays closed (the final review of row 69, F1).
    ///
    /// RED GATE: `plant-69c-overrun-kept.py` keeps an overrun like an answer, and the second settle
    /// asks nothing; `plant-69c-overrun-opens-gate.py` opens the start-time repair's gate on an
    /// overrun (the final review of row 69, F1), and the gate is open after the first settle.
    #[test]
    fn an_overrun_version_question_is_asked_again() {
        use std::cell::{Cell, RefCell};
        let held = Mutex::new(VersionBook::new());
        let asked = Cell::new(0);
        let answer = RefCell::new(Asked::Overran);
        let said = RefCell::new(Vec::new());
        let stamp = |path: &Path| {
            Some(ProgramStamp {
                path: path.to_path_buf(),
                size: 10,
                modified: None,
            })
        };
        let ask = |_: &Path| {
            asked.set(asked.get() + 1);
            answer.borrow().clone()
        };
        let say = |line: String| said.borrow_mut().push(line);
        let settle = || settle_version_in(&held, &stamp, &ask, &say);
        assert!(adopt_walk_program_in(
            &held,
            Some(Some(OsStr::new(r"C:\Tools\codex.exe")))
        ));

        assert_eq!(settle(), CodexVersion::Unknown, "an overrun is unknown");
        assert_eq!(*said.borrow(), vec!["BT_AGENT_HOOKS codex version=unknown"]);
        assert_eq!(held.lock().unwrap().version(), CodexVersion::Unknown);
        assert!(
            !repair_within_the_gate_in(&held),
            "unknown for presses and rows (R17), but an overrun is no answer: the repair's gate \
             stays closed (the final review of row 69, F1)"
        );
        assert_eq!(settle(), CodexVersion::Unknown);
        assert_eq!(
            asked.get(),
            2,
            "the same file, asked again after an overrun"
        );
        assert!(!repair_within_the_gate_in(&held), "overran again: closed");

        *answer.borrow_mut() = Asked::Answered(Some(release_at(0, 147, 0, None)));
        assert_eq!(settle(), codex_at(0, 147, 0));
        assert!(
            repair_within_the_gate_in(&held),
            "the next answered settle opens the gate"
        );
        assert_eq!(settle(), codex_at(0, 147, 0));
        assert_eq!(asked.get(), 3, "the answer that came is held");
        assert_eq!(
            VERSION_DEADLINE,
            Duration::from_secs(10),
            "copilot's deadline"
        );
    }

    /// PIN (the 69c review's C5) — **codex's version is its version line, on a successful exit**:
    /// the first line that is not blank must be `codex-cli <semver>` whole. A wrapper that prints
    /// its own version first, a sentence with a number in it, a failed exit, or no line at all is
    /// no version — unknown, which runs the hook (R17).
    ///
    /// RED GATE: `plant-69c-version-any-line.py` reads the first version anywhere in the answer,
    /// and the wrapper's line is believed; `plant-69c-failed-exit-believed.py` reads a failed run's
    /// line all the same.
    #[test]
    fn the_version_reads_off_what_codex_prints() {
        let read = |stdout: &str| Asked::release_from(true, stdout);
        assert_eq!(
            read("codex-cli 0.166.0\n"),
            Some(release_at(0, 166, 0, None))
        );
        assert_eq!(
            read("codex-cli 0.166.0\r\n"),
            Some(release_at(0, 166, 0, None))
        );
        assert_eq!(
            read("\ncodex-cli 0.148.0-alpha.5\n"),
            Some(release_at(0, 148, 0, Some("alpha.5")))
        );
        assert_eq!(
            read("codex-cli 0.148.0+build.7\n"),
            Some(release_at(0, 148, 0, None)),
            "build metadata orders nothing"
        );
        assert_eq!(
            read("codex-cli 0.147.0-alpha.2\n"),
            Some(release_at(0, 147, 0, Some("alpha.2"))),
            "a prerelease is kept as one, below 0.148 either way"
        );
        for stranger in [
            "",
            "\n\n",
            "wrapper 2.1.0\ncodex-cli 0.166.0\n",
            "codex-shim 0.166.0\n",
            "codex-cli 0.166.0 (npm)\n",
            "codex-cli version 0.166.0\n",
            "codex-cli v0.166.0\n",
            "codex-cli 0.166\n",
            "codex-cli 0.166.0.1\n",
            "codex-cli 00.166.0\n",
            "codex-cli 0.166.0-\n",
            "codex-cli 0.166.0-alpha..1\n",
            "codex-cli 0.166.0-01\n",
            "'codex' is not recognized as an internal or external command",
        ] {
            assert_eq!(read(stranger), None, "{stranger:?}");
        }
        assert_eq!(
            Asked::release_from(false, "codex-cli 0.166.0\n"),
            None,
            "a failed exit is no version, whatever it printed"
        );
        assert_eq!(Asked::of(None), Asked::Answered(None), "not asked: none");
        assert_eq!(
            Asked::of(Some(bt_platform::ProbeEnding::Overran)),
            Asked::Overran
        );
    }

    /// RED (the 69c review's C3) — **the first-run card offers only what the press would write**:
    /// each install refusal the press gives, [`install_refusal_at`] — the card's question — gives
    /// for the same files, and the press leaves them byte for byte. A `hooks.json` codex would
    /// ignore once written; a path Windows PowerShell 5.1 cannot call; a `notify` this build cannot
    /// decode, on either road; and, below 0.148, a read-only `config.toml` the line would go in.
    /// Empty folders are offered on both roads, and the press installs there.
    ///
    /// RED GATE: `plant-69c-card-reads-one-file.py` has the card read `config.toml` alone on the
    /// hook's road, and the ignored `hooks.json` is offered.
    #[test]
    fn an_install_is_refused_alike_by_the_press_and_the_card() {
        let current = codex_at(0, 166, 0);
        let old = codex_at(0, 147, 0);
        let alike = |path: &Path, version: &CodexVersion, exe: &Path, reason: &'static str| {
            let dir = path.parent().expect("a folder");
            let before = names(dir)
                .into_iter()
                .map(|name| {
                    let bytes = std::fs::read(dir.join(&name)).expect("there");
                    (name, bytes)
                })
                .collect::<Vec<_>>();
            assert_eq!(
                install_refusal_at(path, exe, version),
                Some(reason),
                "the card, {}",
                path.display()
            );
            assert_eq!(
                apply_to(path, true, exe),
                Outcome::Refused(reason),
                "the press, {}",
                path.display()
            );
            let after = names(dir)
                .into_iter()
                .map(|name| {
                    let bytes = std::fs::read(dir.join(&name)).expect("there");
                    (name, bytes)
                })
                .collect::<Vec<_>>();
            assert_eq!(after, before, "nothing written, {}", path.display());
        };

        // A hooks.json codex would ignore whole once written.
        let dir = scratch("card-ignored");
        std::fs::write(dir.join(HOOKS_FILE), r#"{"model": "gpt-5", "hooks": {}}"#)
            .expect("their own file");
        alike(
            &dir.join(HOOKS_FILE),
            &current,
            &exe(),
            crate::i18n::Text::CodexHooksFileIgnored.text(),
        );

        // A path Windows PowerShell 5.1 cannot call: on Windows only, where codex runs the hook
        // through it.
        let dir = scratch("card-brackets");
        let unmatched = dir.join("a[b").join("folio.exe");
        if bt_platform::host_platform() == HostPlatform::Windows {
            alike(
                &dir.join(HOOKS_FILE),
                &current,
                &unmatched,
                crate::i18n::Text::CodexHookPathBrackets.text(),
            );
        } else {
            assert_eq!(
                install_refusal_at(&dir.join(HOOKS_FILE), &unmatched, &current),
                None
            );
        }

        // A notify this build cannot decode, on both roads.
        let dir = scratch("card-notify-undecodable");
        std::fs::write(dir.join(CONFIG_FILE), "notify = \"x\"\n").expect("their own file");
        let unreadable = crate::i18n::Text::CodexConfigUnreadable.text();
        alike(&dir.join(HOOKS_FILE), &current, &exe(), unreadable);
        alike(&dir.join(CONFIG_FILE), &old, &exe(), unreadable);

        // Below 0.148, a config.toml the line would go in and that Folio will not edit.
        let dir = scratch("card-readonly");
        let config = dir.join(CONFIG_FILE);
        std::fs::write(&config, "model = \"gpt-5\"\n").expect("their own file");
        let writable = std::fs::metadata(&config).expect("there").permissions();
        let mut readonly = writable.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&config, readonly).expect("read-only");
        let card = install_refusal_at(&config, &exe(), &old);
        let press = apply_to(&config, true, &exe());
        let after = std::fs::read_to_string(&config).expect("there");
        // Restored before the assertions, not after them: a panic in between would leave the
        // fixture read-only (attention_ownership's review R14).
        std::fs::set_permissions(&config, writable).expect("writable again");
        let read_only = crate::i18n::Text::AgentConfigReadOnly.text();
        assert_eq!(card, Some(read_only), "the card");
        assert_eq!(press, Outcome::Refused(read_only), "the press");
        assert_eq!(after, "model = \"gpt-5\"\n");

        // Nothing in the way: offered on both roads, and the press installs.
        for (file, version) in [(HOOKS_FILE, &current), (CONFIG_FILE, &old)] {
            let dir = scratch("card-free");
            assert_eq!(
                install_refusal_at(&dir.join(file), &exe(), version),
                None,
                "{file}"
            );
            assert_eq!(
                apply_to(&dir.join(file), true, &exe()),
                Outcome::Installed,
                "{file}"
            );
        }
    }

    /// RED (re-review R4) — **a removal that left `config.toml` as it was says so in its own
    /// sentence**, naming `hooks.json`, and telling a hook taken out from none there; the off-press
    /// toast picks the one the outcome says.
    ///
    /// RED GATE: `plant-69c-left-unread-says-removed.py` gives both cases the removed sentence.
    #[test]
    fn a_removal_that_left_config_toml_says_which() {
        for lang in [crate::i18n::Lang::English, crate::i18n::Lang::Chinese] {
            let removed = crate::i18n::Text::CodexHookRemovedConfigLeft.in_lang(lang);
            let none = crate::i18n::Text::CodexNoHookConfigLeft.in_lang(lang);
            assert_ne!(removed, none);
            for sentence in [removed, none] {
                assert!(sentence.contains("hooks.json"), "{sentence}");
                assert!(sentence.contains("config.toml"), "{sentence}");
            }
        }
        let press = crate::test_support::squeezed_body("Runtime", "apply_codex_notify");
        assert!(
            press.contains(
                "Outcome::LeftUnread{removed,reason}=>{lethead=ifremoved{i18n::Text::CodexHookRemovedConfigLeft}else{i18n::Text::CodexNoHookConfigLeft};"
            ),
            "the toast is chosen by the outcome"
        );
    }
}
