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
//!   { "async": true, "command": "\"<folio.exe>\" attention codex:stop --json -",
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
//! - **One string run through a shell**, codex's only form for a command hook: cmd.exe on Windows,
//!   where a `%` in Folio's path would be expanded inside the quotes — so an install from such a path
//!   is refused and said — and `$SHELL -lc` elsewhere, where the single-quoted form keeps `$`,
//!   backticks and spaces literal ([`hook_command`]).
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
//! marked twice. A `notify` Folio did not write is the person's and is never touched. The line
//! writer stays here for the codex builds that have no asynchronous hooks, which plan row 69c
//! chooses it for.
//!
//! # Where the files are, and why it is asked for rather than composed
//!
//! §7.1.6j's lesson, the same one [`attention_hooks`](crate::attention_hooks) states: a machine with
//! a redirected profile folder gets told "not installed" about something installed for months, if
//! the path was *composed* rather than asked for. `CODEX_HOME` is the variable codex itself reads —
//! the survey's own harness set it to point the recordings at the real installation — so it is what
//! is read here, and only otherwise the documented default beside the user's profile.

pub(crate) use crate::attention_ownership::Outcome;
use crate::attention_ownership::{self as ownership, Decision};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bt_platform::HostPlatform;
use serde_json::{Map, Value as Json};
use toml_edit::{Array, DocumentMut, ImDocument, Item, Table, Value};

use crate::attention_hooks::{self as hooks, Config, Family, SPLICE_REFUSED, Standing, Wanted};

use crate::attention_map::CODEX;
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

/// What this module says about a configuration file it could not read, whichever half could not
/// read it.
///
/// One sentence for the byte that is not UTF-8 and for the document that is not TOML, because to
/// the reader they are one fact: there is a file there, and this build is not going to guess at it.
const UNREADABLE: &str = "the codex configuration file is not one this build can read";

/// The same sentence about `hooks.json`.
const HOOKS_UNREADABLE: &str = "the codex hooks file is not one this build can read";

/// What an object on the way to Folio's hook said when it named one key twice — the reason
/// [`attention_hooks`](crate::attention_hooks) gives about Claude Code's file, about this one.
const HOOKS_DUPLICATE: &str =
    "the codex hooks file names one key twice where Folio's hook goes, so nothing was written";

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

/// **The keys codex reads a `hooks.json` with.** Its `HooksFile` refuses any other
/// (`deny_unknown_fields`), and a file it refuses is a file it ignores whole, with a warning.
const HOOKS_FILE_KEYS: [&str; 2] = ["description", "hooks"];

/// **The hook events codex reads under config.toml's `[hooks]`** (`HookEventsToml` at
/// `rust-v0.166.0`). `[hooks.state]`, where codex keeps the person's approvals, is not one of them,
/// so a table holding only that is not a second representation.
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

/// **codex's `hooks.json`, as [`attention_hooks`](crate::attention_hooks) reads and writes it.**
pub(crate) const HOOKS: Family = Family {
    owner: hook_owner,
    wanted: hook_wanted,
    positional: true,
    unreadable: HOOKS_UNREADABLE,
    duplicate: HOOKS_DUPLICATE,
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

/// The user-level configuration file — read, and written only to take out the `notify` line an
/// earlier Folio wrote.
#[must_use]
pub(crate) fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

/// The user-level hooks file. **The file this module writes Folio's hook in.**
#[must_use]
pub(crate) fn hooks_path() -> Option<PathBuf> {
    Some(config_dir()?.join(HOOKS_FILE))
}

/// The same file, spelled for a reader who is being asked to consent to it —
/// [`crate::attention_hooks::settings_path_shown`]'s rule over `CODEX_HOME`.
#[must_use]
pub(crate) fn config_path_shown() -> String {
    config_path_shown_from(
        crate::attention_hooks::agent_variable(HOME_VARIABLE),
        crate::attention_hooks::agent_variable(bt_platform::home_variable()),
    )
}

/// The same decision, with the environment handed in.
#[must_use]
fn config_path_shown_from(named: Option<OsString>, home: Option<OsString>) -> String {
    let default = || format!("~/{DEFAULT_DIRECTORY}/{CONFIG_FILE}");
    if named.as_ref().is_none_or(|named| named.is_empty()) {
        return default();
    }
    config_dir_from(named, home)
        .map(|dir| dir.join(CONFIG_FILE).display().to_string())
        .unwrap_or_else(default)
}

/// What the two files say today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(dir) = config_dir() else {
        return State::Absent;
    };
    state_in(&dir, std::env::current_exe().ok().as_deref())
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
/// **Installed when either file names this copy**: the hook, or a `notify` line an earlier Folio
/// wrote and that stays until Folio writes its entry again (R18). The refusal the row says is
/// `hooks.json`'s, the one file a press writes Folio's entry in.
#[must_use]
fn state_in(dir: &Path, exe: Option<&Path>) -> State {
    let hook = hooks::state_in(&HOOKS, &dir.join(HOOKS_FILE), exe);
    let notify = match beside(&dir.join(CONFIG_FILE)) {
        Ok(toml) => match toml.text.parse::<DocumentMut>() {
            Ok(document) => {
                let ours = notify_owner(&document).ok().flatten();
                if exe.is_some_and(|exe| {
                    ours.is_some_and(|owner| crate::explorer_menu::same_path(&owner, exe))
                }) {
                    State::Installed
                } else {
                    State::Absent
                }
            }
            Err(_) => State::Unreadable,
        },
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

/// **The command Folio's hook runs**, for the shell codex runs it through on this machine.
#[must_use]
pub(crate) fn hook_command(exe: &Path) -> String {
    hook_command_on(exe, bt_platform::host_platform())
}

/// The same, for a named platform: **cmd.exe's double quotes on Windows**, inside which nothing but
/// `%` is special and no file name can hold a `"`; **sh's single quotes elsewhere**, inside which
/// nothing is special and a `'` of the path's own is spelled `'\''`. Both are the spellings
/// [`ownership::legacy_path`] reads back, which is what makes the entry provably Folio's.
#[must_use]
fn hook_command_on(exe: &Path, platform: HostPlatform) -> String {
    let path = exe.to_string_lossy();
    let program = if platform == HostPlatform::Windows {
        format!("\"{path}\"")
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    };
    format!("{program} {ATTENTION_VERB} {CODEX}:{STOP} {JSON_FLAG} {STDIN_PAYLOAD}")
}

/// **Whose one `hooks.json` entry is** — the [`HOOKS`] family's decoder.
///
/// A copy of Folio's when its `command` is Folio's template ([`ownership::legacy_path`]: a quoted
/// executable whose own file name is Folio's, speaking `attention codex:<event>`) and it carries no
/// key Folio does not write ([`HOOK_KEYS`]). Folio's verb in any other shape is an error, which the
/// splice reads as nobody's: the entry is left where it is, untouched.
fn hook_owner(hook: &Json) -> Result<Option<PathBuf>, &'static str> {
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
    let mut hook = Map::new();
    hook.insert("type".to_owned(), "command".into());
    hook.insert("command".to_owned(), hook_command(exe).into());
    hook.insert("async".to_owned(), true.into());
    hook.insert("timeout".to_owned(), HOOK_TIMEOUT.into());
    vec![Wanted {
        event: STOP_EVENT,
        matcher: None,
        hook: Json::Object(hook),
    }]
}

/// **Whether codex would read `hooks.json` with this text in it**, once Folio's hook is added.
///
/// A byte-order mark, a top-level key other than [`HOOKS_FILE_KEYS`], or a `description` that is
/// not a string makes codex ignore the whole file with a warning — so a hook written into it would
/// never run, and the press would be a lie. The splice keeps every one of those bytes, so they
/// would still be there after the write. A text that is not JSON at all is
/// [`hooks::hook_owners`]'s refusal, said before this is asked.
fn read_by_codex(existing: &str) -> bool {
    if existing.starts_with('\u{feff}') {
        return false;
    }
    if existing.trim().is_empty() {
        return true;
    }
    let Ok(Json::Object(file)) = serde_json::from_str::<Json>(existing) else {
        return true;
    };
    file.keys()
        .all(|key| HOOKS_FILE_KEYS.contains(&key.as_str()))
        && file
            .get("description")
            .is_none_or(|description| description.is_string() || description.is_null())
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
    let words = words_of(document).ok_or(UNREADABLE)?;
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
    let document = ImDocument::parse(existing).map_err(|_| UNREADABLE)?;
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
                let span = item.span().ok_or(SPLICE_REFUSED)?;
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
        let (key, item) = table.get_key_value(NOTIFY_KEY).ok_or(SPLICE_REFUSED)?;
        let key = key.span().ok_or(SPLICE_REFUSED)?;
        let value = item.span().ok_or(SPLICE_REFUSED)?;
        let start = existing[..key.start]
            .rfind('\n')
            .map_or(body, |ending| ending + 1)
            .max(body);
        // Nothing but indentation before the key on its line: the line is the key's.
        if !existing[start..key.start]
            .trim_matches([' ', '\t'])
            .is_empty()
        {
            return Err(SPLICE_REFUSED);
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
        Err(SPLICE_REFUSED)
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

/// **`config.toml` as an operation on `hooks.json` reads it**: its text, and the resolution a write
/// of it needs — or, for a file this build will read but not edit, the reason it will not be
/// written.
struct Toml {
    text: String,
    config: Result<Config, &'static str>,
}

/// Read the `config.toml` at `path`, resolved **once**, for this operation.
///
/// A file Folio will not edit — a link out of codex's folder, one shared by hard links, a read-only
/// one — is still read: what it says decides whether a hook may be added beside it (R15), and the
/// operation is refused only when a line of Folio's in it would have to go.
fn beside(path: &Path) -> Result<Toml, &'static str> {
    match Config::resolve(path) {
        Ok(config) => match config.standing() {
            Standing::Text(text) => Ok(Toml {
                text,
                config: Ok(config),
            }),
            Standing::Nothing => Ok(Toml {
                text: String::new(),
                config: Ok(config),
            }),
            Standing::Unreadable => Err(UNREADABLE),
        },
        Err(hooks::Unresolved::Refused(reason)) => {
            match bt_platform::file_reads::read_to_string(
                bt_platform::file_reads::Lane::Attention,
                path,
            ) {
                Ok(text) => Ok(Toml {
                    text,
                    config: Err(reason),
                }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Toml {
                    text: String::new(),
                    config: Err(reason),
                }),
                Err(_) => Err(UNREADABLE),
            }
        }
        Err(hooks::Unresolved::Unreadable) => Err(UNREADABLE),
    }
}

/// Put Folio's hook in, or take it out, on this machine.
///
/// **The files are read and written whole, and only Folio's own entries in them change**
/// ([`hooks::hooks_edited`], [`edited`]), and a copy of what was there is kept beside each the first
/// time each day — `attention_hooks`'s rule, for its reason: these are somebody's own configuration
/// files, and a build that could damage one had better be able to hand it back.
pub(crate) fn apply(decision: Decision, exe: &Path) -> Outcome {
    let Some(path) = hooks_path() else {
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
        Err(hooks::Unresolved::Unreadable) => Outcome::Refused(HOOKS_UNREADABLE),
    }
}

/// The same act on a `hooks.json` this operation has already resolved.
///
/// **Everything below the entry takes this value.** The path was resolved once, at the top; the
/// read, the dated copy and the replace all name that one answer, and there is no second
/// resolution on this path for a repointed link to slip through (re-review B1). `config.toml`, the
/// other file, is resolved once too ([`beside`]).
///
/// One operation over two files, in this order: both are read, **both edits are made and pass
/// their self-checks** ([`hooks::hooks_edited`], [`edited`]), every copy either names is asked
/// about, and a `config.toml` this build will not edit is refused — and only then is anything
/// written, `config.toml` first, then `hooks.json`. So every refusal leaves both files as they
/// were. The one window left is a second write that fails after the first landed: the `notify`
/// line is gone and the hook not yet written, a Folio that marks nothing until the next press or
/// row 67's repair finishes it — never a turn marked twice.
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
    let install = decision.installs();
    if install
        && bt_platform::host_platform() == HostPlatform::Windows
        && exe.to_string_lossy().contains('%')
    {
        return Outcome::Refused(crate::i18n::Text::CodexHookPathPercent.text());
    }
    let folder = config.named().parent().expect("absolute config");
    let existing = match config.standing() {
        Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        Standing::Nothing => String::new(),
        // **A file that could not be read is never written over** — release audit 2026-09-16 (C-3).
        Standing::Unreadable => return Outcome::Refused(HOOKS_UNREADABLE),
    };
    let toml = match beside(&folder.join(CONFIG_FILE)) {
        Ok(toml) => toml,
        Err(reason) => return Outcome::Refused(reason),
    };
    let Ok(document) = toml.text.parse::<DocumentMut>() else {
        // Refused rather than guessed at. A configuration file this build cannot read is a file
        // somebody wrote, and whether it carries Folio's line is not something to assume.
        return Outcome::Refused(UNREADABLE);
    };
    if document.get(NOTIFY_KEY).is_some() && words_of(&document).is_none() {
        return Outcome::Refused(UNREADABLE);
    }
    if install && declares_toml_hooks(&document) {
        return Outcome::Refused(crate::i18n::Text::CodexHooksInToml.text());
    }
    let mut paths = match hooks::hook_owners(&HOOKS, &existing) {
        Ok(paths) => paths,
        Err(reason) => return Outcome::Refused(reason),
    };
    if install && !read_by_codex(&existing) {
        return Outcome::Refused(crate::i18n::Text::CodexHooksFileIgnored.text());
    }
    // **Ownership is per entry** (closure review R4): a `notify` wearing Folio's verb in a shape
    // this build cannot decode is not Folio's, and it is left alone.
    let line = notify_owner(&document).ok().flatten();
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
    let taken_out = match line {
        Some(_) => match edited(&toml.text, exe, false, install) {
            Ok(taken_out) => taken_out,
            Err(reason) => return Outcome::Refused(reason),
        },
        None => None,
    };
    let toml_config = match (&taken_out, &toml.config) {
        (Some(_), Err(reason)) => return Outcome::Refused(*reason),
        (_, config) => config.as_ref().ok(),
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
        return if others.is_empty() {
            Outcome::Unchanged
        } else {
            Outcome::LeftOther(others)
        };
    }
    // The atomic write, mandatory backup, and unsafe-path refusal are all
    // `attention_hooks::land`'s, said once for all three installers. `config.toml` first: see
    // above.
    if let (Some(taken_out), Some(toml_config)) = (taken_out, toml_config)
        && let Err(reason) = landed(toml_config, &toml.text, "toml", &taken_out)
    {
        return Outcome::Refused(reason);
    }
    if let Some(written) = written
        && let Err(reason) = landed(config, &existing, "json", &written)
    {
        return Outcome::Refused(reason);
    }
    if install {
        Outcome::Installed
    } else if others.is_empty() {
        Outcome::Removed
    } else {
        Outcome::LeftOther(others)
    }
}

/// One file's write through [`Config::land`], and the sentence for each way it can fail.
fn landed(
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
        hooks::Landing::NoBackup => Err(hooks::NO_BACKUP),
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
        assert_eq!(
            config_path_shown_from(None, named(r"C:\Users\someone")),
            "~/.codex/config.toml"
        );
        assert_eq!(
            config_path_shown_from(named(""), named(r"C:\Users\someone")),
            "~/.codex/config.toml"
        );
        assert_eq!(config_path_shown_from(None, None), "~/.codex/config.toml");
        assert_eq!(
            config_path_shown_from(named(r"D:\scratch\codex-home"), named(r"C:\Users\someone")),
            PathBuf::from(r"D:\scratch\codex-home")
                .join(CONFIG_FILE)
                .display()
                .to_string()
        );
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
        assert!(config_path().is_none_or(|path| path.ends_with(CONFIG_FILE)));
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
        assert_eq!(installing, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            names(&dir),
            vec![CONFIG_FILE.to_owned()],
            "nothing was written beside it either"
        );
        // And the row drawn from it says so, rather than offering to write over it.
        assert_eq!(state_in(&dir, Some(&exe())), State::Unreadable);
        // Taking it back out is refused for the same reason: this build cannot tell whose it is.
        let taking_it_out = apply_to(&dir.join(HOOKS_FILE), false, &exe());
        assert_eq!(taking_it_out, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
    }

    /// **A file that is not there is the one state in which writing a fresh one loses nothing.**
    #[test]
    fn a_hooks_file_that_is_not_there_is_the_one_that_gets_created() {
        let dir = scratch("absent");
        let path = dir.join(HOOKS_FILE);
        assert_eq!(state_in(&dir, Some(&exe())), State::Absent);

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(state_in(&dir, Some(&exe())), State::Installed);
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
                format!("@@@@@@\"command\": {},", Json::from(hook_command(&exe()))),
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
        for (platform, exe, command) in [
            (
                HostPlatform::Windows,
                r"C:\Program Files\Folio\folio.exe",
                r#""C:\Program Files\Folio\folio.exe" attention codex:stop --json -"#,
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
            let written = hook_command_on(Path::new(exe), platform);
            assert_eq!(written, command, "{platform:?}");
            // **And it reads back as this copy's**: the decoder that decides what Folio may edit
            // is the one that decodes what it wrote.
            assert_eq!(
                ownership::legacy_path(&written, CODEX),
                Ok(Some(PathBuf::from(exe))),
                "{platform:?}"
            );
        }
    }

    /// **What codex runs reaches the verb as a `Stop` that reads its payload from stdin**, and the
    /// event is one this build maps (plan row 69b): its words and its session are declared.
    #[test]
    fn the_command_codex_runs_is_a_call_the_verb_reads() {
        let command = hook_command_on(Path::new(r"C:\folio\folio.exe"), HostPlatform::Windows);
        let words = command
            .strip_prefix(r#""C:\folio\folio.exe" "#)
            .expect("the program, then the verb's words");
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
                Json::from(hook_command(&exe())),
                "{name}: Folio's is the last group"
            );
            // Laid out as their file is: its line endings, its indentation.
            assert_eq!(written.contains("\r\n"), newline == "\r\n", "{name}");
            assert!(
                written.contains(&format!("{newline}{}\"async\": true,", unit.repeat(6))),
                "{name}: {written}"
            );
            assert!(written.contains(r#""statusMessage": "\u00e9""#), "{name}");
            assert_eq!(state_in(&dir, Some(&exe())), State::Installed, "{name}");
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
            Json::from(hook_command(&exe()))
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

    /// **A `hooks.json` codex would ignore is never written into**, and one this build cannot read
    /// is never written at all: a hook added to a file codex refuses whole would never run.
    #[test]
    fn a_hooks_file_codex_would_ignore_is_never_written() {
        let ignored = crate::i18n::Text::CodexHooksFileIgnored.text();
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
                Outcome::Refused(HOOKS_UNREADABLE),
                Outcome::Refused(HOOKS_UNREADABLE),
            ),
            (
                "{\"hooks\": {\"Stop\": [], \"Stop\": []}}\n",
                Outcome::Refused(HOOKS_DUPLICATE),
                Outcome::Refused(HOOKS_DUPLICATE),
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
            assert_eq!(state_in(&dir, Some(&exe())), State::Installed, "{name}");
            // A take-over press is no licence either: the line is nobody's to take over.
            assert_eq!(
                apply_with(&path, Decision::TakeOver(Vec::new()), &exe()),
                Outcome::Unchanged,
                "{name}"
            );
            assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed, "{name}");
            assert_eq!(std::fs::read_to_string(&config).expect("there"), theirs);
            assert_eq!(state_in(&dir, Some(&exe())), State::Absent, "{name}");
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
        assert_eq!(state_in(&dir, Some(&exe())), State::Installed);
        // A press writes the hook and takes the line out.
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
        assert_eq!(state_in(&dir, Some(&exe())), State::Installed);
        // Both of Folio's entries, by hand or across an upgrade: off takes out both.
        std::fs::write(&config, line(&exe())).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(std::fs::read_to_string(&config).expect("there"), without);
        assert_eq!(state_in(&dir, Some(&exe())), State::Absent);

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
            .find("landed(toml_config,&toml.text,\"toml\",&taken_out)")
            .expect("config.toml is landed here");
        let hooks_land = body
            .find("landed(config,&existing,\"json\",&written)")
            .expect("hooks.json is landed here");
        assert!(toml_lands < hooks_land, "config.toml is written first");
        let judged = body
            .find("(Some(_),Err(reason))=>returnOutcome::Refused(*reason),")
            .expect("a config.toml Folio will not edit is refused here");
        assert!(judged < toml_lands, "and refused before either write");
    }

    /// **A `%` in Folio's path is refused on Windows**, where cmd.exe would expand it inside the
    /// quotes codex runs the hook in; a removal is never refused for it.
    #[test]
    fn a_percent_in_the_path_is_refused_where_cmd_runs_the_hook() {
        let dir = scratch("percent");
        let exe = dir.join("100%").join("folio.exe");
        let path = dir.join(HOOKS_FILE);
        let installing = apply_to(&path, true, &exe);
        if bt_platform::host_platform() == HostPlatform::Windows {
            assert_eq!(
                installing,
                Outcome::Refused(crate::i18n::Text::CodexHookPathPercent.text())
            );
            assert!(!path.exists());
        } else {
            assert_eq!(installing, Outcome::Installed);
        }
        let own = own_stop(&exe);
        std::fs::write(
            &path,
            format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{own}]}}]}}}}"#),
        )
        .expect("a hook of this copy's");
        assert_eq!(apply_to(&path, false, &exe), Outcome::Removed);
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
}
