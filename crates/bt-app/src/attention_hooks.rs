//! **Installing Claude Code's hooks, into the user's own configuration and nowhere else.**
//!
//! One settings row presses this, and `docs/plans/attention/plan.md` §10.6 clause 5 is the whole of
//! its policy: **Folio never discovers or loads automation from a working directory, a repository
//! or a `.claude/` beside one.** That is not fastidiousness — it is upstream's own security note,
//! which says that in a non-interactive session Claude Code "treats the folder as trusted, so hooks
//! committed in a repository's `.claude/settings.json` run in a folder you've never trusted". A
//! terminal that wrote hooks into a repository would be handing that gun to whoever sent the user a
//! link to it.
//!
//! # Where the file is, and why it is asked for rather than composed
//!
//! §7.1.6j's lesson, paid for once already: a machine with Documents redirected to another drive
//! reported "not installed" for a PowerShell profile that had been installed for months, because
//! the path had been *composed* from `%USERPROFILE%` instead of asked for. So the directory here is
//! `CLAUDE_CONFIG_DIR` when the environment sets one — which is the variable Claude Code itself
//! reads — and only otherwise the documented default beside the user's profile. That is also what
//! makes this testable without going anywhere near a real installation: point the variable at a
//! scratch directory and the whole of this module operates there.
//!
//! # Which layer is installed
//!
//! One, never both (§12.1 R2): the zero-delay event and the six-second notification describe the
//! *same* request, so a configuration carrying both turns one request into two credentials.
//!
//! **The primary layer is what is written, and the choice is not a version guess.** Upstream
//! publishes no interface that answers "which hook events does this build have", so there is
//! nothing to ask; what is chosen instead is the layer whose *wrong* case is harmless. A hook event
//! an older Claude Code does not know is an entry it never fires — the pane stays exactly as silent
//! as it was before any of this existed. The other way round, a fallback installed on a current
//! build is a signal that arrives six seconds late for no reason. So: primary, and the fallback
//! rows stay in the catalogue as data, waiting for the day upstream can be asked.
//!
//! And **what is installed is read back from the file** rather than remembered, which is the same
//! rule one layer down: the configuration on disk is the answer to "which rows does this machine
//! have", so a user who edits it by hand gets a Folio that agrees with them.

pub(crate) use crate::attention_ownership::Outcome;
use crate::attention_ownership::{self as ownership, Decision, HooksFound};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

#[cfg(test)]
use bt_platform::HostPlatform;
use serde_json::{Map, Value};

use crate::attention::MappingRow;
use crate::attention_map::{self, CLAUDE_CODE};

/// The variable Claude Code reads for its configuration directory.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The file inside it that holds **user-level** settings.
const SETTINGS_FILE: &str = "settings.json";

/// The default directory's name beside the user's profile, for when the variable says nothing.
const DEFAULT_DIRECTORY: &str = ".claude";

// `hook_owner` decodes the executable operand. A Folio verb identifies the
// integration family; only the operand establishes per-copy ownership.

/// Whether this machine's user configuration already calls Folio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    /// A settings file that names none of our hooks — or no settings file at all, which is the
    /// same answer to the only question being asked.
    Absent,
    /// Our hooks are there.
    Installed,
    /// There is a file and it could not be read as settings. **Not** "absent": writing over a file
    /// this build cannot parse would destroy configuration somebody wrote by hand.
    Unreadable,
    /// There is a readable file and **this build will not edit it**, for the reason carried: it is
    /// a link out of the agent's own folder, it is shared by hard links, or it is read-only. A row
    /// that read `Off` about this would be offering a press that cannot happen, over hooks that
    /// may be firing this minute (closure review R1).
    Refused(&'static str),
}

/// **Where the three agents keep their configuration, as the account says it now**
/// (T-FRESH-FACTS): `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME`, and the home each falls
/// back to — the four variables every agent module composes its file's path from.
///
/// They were read out of this process's launch environment at every call, while a pane is born
/// with the account's current environment (T-ENV-REFRESH; the owner's ruling of 2026-10-05): a
/// variable set after Folio started reached every agent started in a pane and never the folder
/// Folio read and wrote the agent's hooks in. The program walk reads the same logon block a pane
/// is born with, so it reads these four too ([`crate::programs_lane`]'s `MachineFacts`), and
/// [`AGENT_HOMES`] holds the newest walk's reading.
pub(crate) const AGENT_HOME_VARIABLES: [&str; 4] = [
    CONFIG_DIR_VARIABLE,
    "CODEX_HOME",
    "COPILOT_HOME",
    bt_platform::home_variable(),
];

/// **The four variables as one walk read them** — the set ones, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentHomes(Vec<(&'static str, OsString)>);

impl AgentHomes {
    /// Read [`AGENT_HOME_VARIABLES`] out of an environment.
    #[must_use]
    pub(crate) fn in_environment(variable: &dyn Fn(&str) -> Option<OsString>) -> Self {
        Self(
            AGENT_HOME_VARIABLES
                .iter()
                .filter_map(|name| variable(name).map(|value| (*name, value)))
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<OsString> {
        self.0
            .iter()
            .find(|(held, _)| *held == name)
            .map(|(_, value)| value.clone())
    }
}

/// **The newest walk's reading of the agent folders, with its walk's number** — one writer, the
/// program walk's adopter on the window thread (`adopt_program_answers`); read from any thread.
///
/// Until the first walk has answered (milliseconds after launch, the launch's own walk) a reader
/// is answered from this process's launch environment, which is the reading the launch had.
///
/// **Settled** ([`Self::homes_settled`]) once a walk has answered, or once the program lane has
/// said no walk will ([`Self::settle_on_the_launch_homes`]: a walk that failed, a worker that
/// ended, a request its door refused with no walk standing) — the moment the carry pass's members
/// that read these folders have their first real answer to give (0.4.10 plan row 68c, the
/// coordinator's ruling on the review's N1, 2026-11-09). No timer settles it.
pub(crate) struct AgentHomesCell(Mutex<Option<(u64, AgentHomes)>>, AtomicBool);

impl AgentHomesCell {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None), AtomicBool::new(false))
    }

    /// **Adopt walk `generation`'s reading**; an older walk's is refused. Answers whether any of
    /// the four variables now reads differently.
    pub(crate) fn adopt(&self, generation: u64, homes: AgentHomes) -> bool {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if held
            .as_ref()
            .is_some_and(|(newest, _)| *newest >= generation)
        {
            return false;
        }
        let before: Vec<Option<OsString>> = AGENT_HOME_VARIABLES
            .iter()
            .map(|name| Self::reading(held.as_ref(), name))
            .collect();
        *held = Some((generation, homes));
        AGENT_HOME_VARIABLES
            .iter()
            .zip(before)
            .any(|(name, before)| Self::reading(held.as_ref(), name) != before)
    }

    /// Whether a walk has answered — the first-run card's wait: it offers rows that write into
    /// these folders.
    pub(crate) fn answered(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// **Whether the folders have their first real answer in this process**: a walk's, or the
    /// launch environment's once no walk will answer ([`Self::settle_on_the_launch_homes`]).
    pub(crate) fn homes_settled(&self) -> bool {
        self.answered() || self.1.load(Ordering::SeqCst)
    }

    /// **No walk will answer, so the launch environment's reading is the answer** — said by the
    /// window when the program lane ends a walk with no answer, or refuses a request with no walk
    /// standing (`programs_lane::Answers::ended_with_no_walk_to_answer`). `true` when this settled
    /// the folders; `false` when a walk had answered, or they were settled already. A walk that
    /// answers later is adopted as the first walk's answer is ([`Self::adopt`]; `answered` was
    /// `false` until then), so the members read the account's folders again.
    pub(crate) fn settle_on_the_launch_homes(&self) -> bool {
        !self.answered() && !self.1.swap(true, Ordering::SeqCst)
    }

    /// One of [`AGENT_HOME_VARIABLES`] as the newest walk read it.
    pub(crate) fn variable(&self, name: &str) -> Option<OsString> {
        Self::reading(
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref(),
            name,
        )
    }

    fn reading(held: Option<&(u64, AgentHomes)>, name: &str) -> Option<OsString> {
        match held {
            Some((_, homes)) => homes.get(name),
            None => std::env::var_os(name),
        }
    }
}

/// The product's cell ([`AgentHomesCell`]).
pub(crate) static AGENT_HOMES: AgentHomesCell = AgentHomesCell::new();

/// [`AgentHomesCell::variable`] on the product's cell.
#[must_use]
pub(crate) fn agent_variable(name: &str) -> Option<OsString> {
    AGENT_HOMES.variable(name)
}

/// The directory Claude Code keeps user configuration in, as **the account's environment** says
/// it ([`AGENT_HOMES`]).
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        crate::attention_hooks::agent_variable(CONFIG_DIR_VARIABLE),
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
/// Split out so the rule can be pinned by a test that sets nothing: a process-wide variable changed
/// from a test is changed for every other test running beside it, and this crate refuses `unsafe`,
/// which is what `set_var` now is. The one impure input is named instead — the shape `cli::resolve`
/// uses for the filesystem, and for the same reason.
#[must_use]
fn config_dir_from(named: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    Some(PathBuf::from(home.filter(|home| !home.is_empty())?).join(DEFAULT_DIRECTORY))
}

/// The user-level settings file. **The only file this module ever writes.**
#[must_use]
pub(crate) fn settings_path() -> Option<PathBuf> {
    Some(config_dir()?.join(SETTINGS_FILE))
}

/// The same file, spelled for a reader who is being asked to consent to it.
///
/// **`~/.claude/settings.json` when nothing has moved the directory**, which is
/// the spelling every other surface in this window uses and the truth on every
/// machine that has not set `CLAUDE_CONFIG_DIR`. When the variable does name a
/// directory the whole path is spelled out instead: a disclosure that named a
/// file this build is not going to write is worse than no disclosure, and the
/// reader who moved the directory is exactly the reader who will check.
#[must_use]
pub(crate) fn settings_path_shown() -> String {
    settings_path_shown_from(
        crate::attention_hooks::agent_variable(CONFIG_DIR_VARIABLE),
        crate::attention_hooks::agent_variable(bt_platform::home_variable()),
    )
}

/// The same decision, with the environment handed in — [`config_dir_from`]'s
/// reason, and it is what lets a test pin both halves without touching a
/// process-wide variable.
#[must_use]
fn settings_path_shown_from(named: Option<OsString>, home: Option<OsString>) -> String {
    let default = || format!("~/{DEFAULT_DIRECTORY}/{SETTINGS_FILE}");
    if named.as_ref().is_none_or(|named| named.is_empty()) {
        return default();
    }
    config_dir_from(named, home)
        .map(|dir| dir.join(SETTINGS_FILE).display().to_string())
        .unwrap_or_else(default)
}

/// What that file says today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(path) = settings_path() else {
        return State::Absent;
    };
    state_in(&CLAUDE, &path, ownership::this_copy().as_deref())
}

/// **Whether one family's hook file at `path` holds this copy's entries** (`exe`, the running
/// executable; `None` when it could not be asked), and the states in which the question has no
/// answer — the reading [`state`] gives of Claude Code's file, for any [`Family`].
#[must_use]
pub(crate) fn state_in(family: &Family, path: &Path, exe: Option<&Path>) -> State {
    let state = state_at(family, path);
    if state != State::Installed {
        return state;
    }
    let Some(exe) = exe else {
        return State::Unreadable;
    };
    let Ok(text) =
        bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Attention, path)
    else {
        return State::Unreadable;
    };
    let Some(value) = serde_json::from_str::<Value>(splice::body(&text)).ok() else {
        return State::Unreadable;
    };
    if (owners(family, &value).unwrap_or_default())
        .iter()
        .any(|owner| crate::explorer_menu::same_path(owner, exe))
    {
        State::Installed
    } else {
        State::Absent
    }
}

/// **The settings row's two facts, out of one read of the file.**
///
/// Whether this copy's marks are in it, and — when this build will not edit it at all — the reason
/// the row says in place of `Off`. Two answers to "what does the row show" derived from one
/// `State` rather than two reads, because a second read is a second answer (closure review R1).
#[must_use]
pub(crate) fn row_state() -> (bool, Option<&'static str>) {
    match state() {
        State::Installed => (true, None),
        State::Refused(reason) => (false, Some(reason)),
        State::Absent | State::Unreadable => (false, None),
    }
}

/// The same question about a named file, so a test can ask it without a settings file on the
/// machine it runs on.
#[must_use]
fn state_at(family: &Family, path: &Path) -> State {
    let config = match Config::resolve(path) {
        Ok(config) => config,
        // Read, and not ours to change. The row says which file it is looking at.
        Err(Unresolved::Refused(reason)) => return State::Refused(reason),
        Err(Unresolved::Unreadable) => return State::Unreadable,
    };
    let text = match config.standing() {
        // Nothing there is the same answer to the only question being asked.
        Standing::Nothing => return State::Absent,
        // **Not `Absent`.** There is a file, and a row that said "not installed" about it would
        // offer to write over one this build never read. See [`standing`].
        Standing::Unreadable => return State::Unreadable,
        Standing::Text(text) => text,
    };
    // A byte-order mark is read past, as the writer reads past it and writes it back.
    let text = splice::body(&text);
    if text.trim().is_empty() {
        return State::Absent;
    }
    match serde_json::from_str::<Value>(text) {
        Ok(settings) if settings.is_object() => {
            if declares_folio(family, &settings) {
                State::Installed
            } else {
                State::Absent
            }
        }
        _ => State::Unreadable,
    }
}

/// **The rows this machine actually has installed**, read back off the file.
///
/// Empty when nothing is installed, which is what makes an arrival from an uninstalled family fall
/// through to nothing at all rather than to a row we assumed was there.
#[must_use]
pub(crate) fn installed_rows() -> Vec<MappingRow> {
    let Some(path) = settings_path() else {
        return Vec::new();
    };
    let Ok(text) =
        bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Attention, &path)
    else {
        return Vec::new();
    };
    let Ok(settings) = serde_json::from_str::<Value>(splice::body(&text)) else {
        return Vec::new();
    };
    rows_declared_by(&settings)
}

/// The same, from a settings value rather than from the disk.
#[must_use]
pub(crate) fn rows_declared_by(settings: &Value) -> Vec<MappingRow> {
    let declared = declared_events(settings);
    attention_map::ROWS
        .iter()
        .filter(|row| row.family == CLAUDE_CODE && declared.iter().any(|name| name == row.event))
        .copied()
        .collect()
}

/// Every `<event>` (matcher-qualified where it is one) that a Folio hook is registered under.
fn declared_events(settings: &Value) -> Vec<String> {
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (event, groups) in hooks {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            if !group_is_ours(group) {
                continue;
            }
            match group.get("matcher").and_then(Value::as_str) {
                Some(matcher) if !matcher.is_empty() => found.push(format!("{event}.{matcher}")),
                _ => found.push(event.clone()),
            }
        }
    }
    found
}

fn group_is_ours(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| entry_owners(&CLAUDE, hooks).iter().any(Option::is_some))
}

/// **One agent's hook file, as the splice reads and writes it** (0.4.10 plan row 69a).
///
/// Claude Code's `settings.json` and Codex's `hooks.json` nest the same way — the file's object,
/// its `hooks`, an event's array of groups, a group's `hooks` array of entries — so the reader, the
/// three edits ([`edited`]) and the write-time self-check ([`verified`]) are one, and what the two
/// files differ in is said here and nowhere else.
pub(crate) struct Family {
    /// **Whose one entry is**: the executable it runs when it is a copy of Folio speaking this
    /// family's verb, `Ok(None)` for anybody else's, and an error for an entry wearing Folio's verb
    /// in a shape this build cannot claim — which is then nobody's to edit.
    pub(crate) owner: fn(&Value) -> Result<Option<PathBuf>, &'static str>,
    /// The entries one install of a copy at this executable writes.
    pub(crate) wanted: fn(&Path) -> Vec<Wanted>,
    /// **The entry this build writes for one row** ([`Wanted::row`]), naming the copy at this
    /// executable — what an install points another copy's entry at, where it stands (ruling N1,
    /// re-review R1).
    pub(crate) todays: fn(&str, &Path) -> Value,
    /// **The keys that say which program an entry runs, and how** — what today's form is judged on
    /// ([`in_todays_form`]) and the only keys a repair rewrites ([`repaired_entry`]); every other
    /// key of an entry of Folio's is the person's to set. An entry without every one of them is in
    /// an older shape.
    pub(crate) program_keys: &'static [&'static str],
    /// **Whether the agent keys something of the person's by an entry's place in the file.**
    ///
    /// Codex records the person's approval of a hook under `<file>:<event>:<group>:<entry>`, so
    /// taking a group out from in front of theirs would detach every approval after it (ruling
    /// R16). Under such a family an entry is Folio's only when it is its group's one entry, a group
    /// of Folio's with a group after it is emptied where it stands rather than taken out, and the
    /// self-check compares every foreign entry's place as well as its bytes.
    pub(crate) positional: bool,
    /// The family's name in the wire's `<family>:<event>` spelling, which a Folio-looking entry
    /// this build cannot claim speaks (the start-time scan's "unclassified", 0.4.10 plan row 67b).
    pub(crate) agent: &'static str,
}

/// Claude Code's `settings.json`.
pub(crate) const CLAUDE: Family = Family {
    owner: hook_owner,
    wanted,
    todays: todays_hook,
    program_keys: &["command", "args"],
    positional: false,
    agent: CLAUDE_CODE,
};

/// **Whose each entry of one group is** — [`Family::owner`], read per entry, and the one place that
/// reading is made: the planner, the owners an operation checks, the state a row reads and the
/// self-check's stripped value all take it from here, so no two of them can disagree about an
/// entry.
///
/// Under a [`Family::positional`] family an entry that shares its group is nobody's: Folio writes
/// its entry alone in a group of its own, and one standing beside the person's could only be taken
/// out by moving theirs.
fn entry_owners(family: &Family, rows: &[Value]) -> Vec<Option<PathBuf>> {
    let alone = !family.positional || rows.len() == 1;
    rows.iter()
        .map(|hook| {
            if alone {
                (family.owner)(hook).ok().flatten()
            } else {
                None
            }
        })
        .collect()
}

/// The only decoder of Claude hook ownership, for both legacy and direct form.
fn hook_owner(hook: &Value) -> Result<Option<PathBuf>, &'static str> {
    let Some(command) = hook.get("command").and_then(Value::as_str) else {
        return Ok(None);
    };
    let owner = if let Some(args) = hook.get("args") {
        ownership::direct_path(command, args, CLAUDE_CODE)?
    } else {
        ownership::legacy_path(command, CLAUDE_CODE)?
    };
    if owner.is_some()
        && (hook.get("type").and_then(Value::as_str) != Some("command")
            || hook.as_object().is_none_or(|o| {
                o.keys()
                    .any(|k| !["type", "command", "args", "async"].contains(&k.as_str()))
            })
            || hook.get("async").is_some_and(|v| !v.is_boolean()))
    {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(owner)
}

fn owners(family: &Family, settings: &Value) -> Result<Vec<PathBuf>, crate::i18n::Text> {
    let mut found = Vec::new();
    let Some(hooks) = settings.get("hooks") else {
        return Ok(found);
    };
    for groups in hooks
        .as_object()
        .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown)?
        .values()
    {
        for group in groups
            .as_array()
            .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown)?
        {
            let rows = group
                .get("hooks")
                .and_then(Value::as_array)
                .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown)?;
            if rows.iter().any(|hook| !hook.is_object()) {
                return Err(crate::i18n::Text::AgentHooksSchemaUnknown);
            }
            // **Ownership is per entry.** An entry this build cannot decode is not Folio's: it
            // is left exactly where it is, and it has no say over the entries beside it. One
            // hand-written `"mytool attention claude-code:Stop"` used to refuse every edit to
            // the whole file, install and remove alike — a second state nobody could get out
            // of (closure review R4). The document's own shape, above and below, is still a
            // refusal: that is a file this build cannot read, not an entry it cannot claim.
            for owner in entry_owners(family, rows).into_iter().flatten() {
                if group.as_object().is_none_or(|o| {
                    o.keys()
                        .any(|k| !["matcher", "hooks"].contains(&k.as_str()))
                }) || group.get("matcher").is_some_and(|v| !v.is_string())
                {
                    return Err(crate::i18n::Text::AgentHooksSchemaUnknown);
                }
                found.push(owner);
            }
        }
    }
    Ok(found)
}

/// Whether any group of the file holds an entry of Folio's, of whichever copy.
fn declares_folio(family: &Family, settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| {
            hooks
                .values()
                .filter_map(Value::as_array)
                .flatten()
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .any(|rows| entry_owners(family, rows).iter().any(Option::is_some))
        })
}

/// **The rows one install writes**: every Claude Code row whose wait is primary, plus every clear.
///
/// The clears are not tiered and every one of them is written, because a machine that gets its
/// waits is a machine that has to get out of them — and because two of them (`Stop`,
/// `UserPromptSubmit`) are also what the other lane reads to announce a turn's end and to know that
/// the next one has begun.
#[must_use]
pub(crate) fn rows_to_install() -> Vec<MappingRow> {
    attention_map::installed_rows(attention_map::ROWS, CLAUDE_CODE, |_| true)
}

/// Direct argv, with stdin only for events whose mapping reads a payload.
fn args_for(event: &str) -> Vec<String> {
    let mut args = vec!["attention".to_owned(), format!("{CLAUDE_CODE}:{event}")];
    if attention_map::turn_end_row(CLAUDE_CODE, event)
        .is_some_and(|row| row.words.are_somewhere() || row.resume.is_declared())
    {
        args.extend(["--json".to_owned(), crate::cli::STDIN_PAYLOAD.to_owned()]);
    }
    args
}

/// **One entry an install wants in the file**: the event it hangs on, the matcher of its group, and
/// the entry itself.
pub(crate) struct Wanted {
    pub(crate) event: &'static str,
    pub(crate) matcher: Option<&'static str>,
    /// The row the entry speaks for — the `<row>` of its `<family>:<row>` word, which every shape
    /// of Folio's entry has carried since the first (report §1.3), so an entry is known by what it
    /// says and never by where it stands (ruling N1; [`row_of`]).
    pub(crate) row: &'static str,
    pub(crate) hook: Value,
    /// Whether its turn-end row carries resume — `Stop` — so that a file missing it is one whose
    /// resume cannot work, and an old shape to the start-time scan (0.4.10 plan row 67, R3).
    pub(crate) carries_resume: bool,
}

impl Wanted {
    /// The group a missing entry is written in: its own, holding nothing else.
    fn hooks_group(&self) -> Value {
        let mut group = Map::new();
        if let Some(matcher) = self.matcher {
            group.insert("matcher".to_owned(), matcher.into());
        }
        group.insert("hooks".to_owned(), Value::Array(vec![self.hook.clone()]));
        Value::Object(group)
    }
}

/// **The entry this build writes for one row**, naming this copy's program.
fn todays_hook(row: &str, exe: &Path) -> Value {
    let mut hook = Map::new();
    hook.insert("type".to_owned(), "command".into());
    hook.insert("command".to_owned(), exe.to_string_lossy().as_ref().into());
    hook.insert("args".to_owned(), args_for(row).into());
    // **Every one of these is asynchronous**, and it is not an optimisation (plan §10.4.3).
    // `PermissionRequest` is a *synchronous decision gate* with a ten-minute timeout: a signal hook
    // that made it wait would put this program between the user and every approval Claude Code ever
    // asks for. Asynchronous hooks cannot return a decision, and this one has none to return — it
    // wants the side effect and nothing else.
    hook.insert("async".to_owned(), true.into());
    Value::Object(hook)
}

/// Every entry one install writes, in [`rows_to_install`]'s order.
fn wanted(exe: &Path) -> Vec<Wanted> {
    rows_to_install()
        .into_iter()
        .map(|row| {
            let (event, matcher) = match row.event.split_once('.') {
                Some((event, matcher)) => (event, Some(matcher)),
                None => (row.event, None),
            };
            Wanted {
                event,
                matcher,
                row: row.event,
                hook: todays_hook(row.event, exe),
                carries_resume: attention_map::turn_end_row(CLAUDE_CODE, row.event)
                    .is_some_and(|row| row.resume.is_declared()),
            }
        })
        .collect()
}

/// **What Claude Code's settings file holds, as the start-time scan reads it** (0.4.10 plan row
/// 67b). Nothing is written here: the step repairs a [`HooksFound::wants_repair`] answer through
/// [`apply_resolved`], the switch's own writer, which asks every question again before it writes.
///
/// `look` is the machine's answer to "what stands at this path" ([`ownership::what_stands`] in the
/// product), so a test decides which owners are alive, gone or on a volume that is not there.
pub(crate) fn scan_resolved(config: &Config, exe: &Path, look: &dyn ownership::Look) -> HooksFound {
    family_scan(&CLAUDE, config, exe, look)
}

/// **What one family's hook file holds, as the start-time scan reads it** — [`scan_resolved`]'s
/// reading, for any [`Family`]: Claude Code's `settings.json`, and codex's `hooks.json` on a codex
/// that runs it (0.4.10 plan row 69c).
pub(crate) fn family_scan(
    family: &Family,
    config: &Config,
    exe: &Path,
    look: &dyn ownership::Look,
) -> HooksFound {
    match family_reading(family, config, exe) {
        Ok(reading) => ownership::classify_owners(&reading.owners, exe, look, || reading.form),
        Err(found) => found,
    }
}

/// **One family's file, read for the scan, before the machine is asked about anybody**: the owners
/// its Folio entries name, and the form of this copy's entries ([`family_form`]).
pub(crate) struct FamilyReading {
    pub(crate) owners: Vec<PathBuf>,
    pub(crate) form: ownership::Form,
}

/// [`family_scan`]'s reading of the file, or the answer it stops at — a file this build cannot
/// read, a key named twice, an entry it cannot classify. A file with no hooks at all names no
/// owner. Split out for [`attention_codex`](crate::attention_codex), whose scan adds the owner of
/// the `notify` line beside its `hooks.json` (0.4.10 plan row 69c).
pub(crate) fn family_reading(
    family: &Family,
    config: &Config,
    exe: &Path,
) -> Result<FamilyReading, HooksFound> {
    let nothing = || FamilyReading {
        owners: Vec::new(),
        form: ownership::Form::Today,
    };
    let text = match config.standing() {
        Standing::Nothing => return Ok(nothing()),
        Standing::Unreadable => return Err(HooksFound::Unreadable),
        Standing::Text(text) => text,
    };
    let Ok(document) = splice::parse(&text) else {
        return Err(HooksFound::Unreadable);
    };
    let settings = match document.whole_value() {
        Ok(value) if value.is_object() => value,
        _ => return Err(HooksFound::Unreadable),
    };
    let hooks = match mapped(family, &document) {
        Ok(Some(hooks)) => hooks,
        Ok(None) => return Ok(nothing()),
        // The row says it in its own register; a press's toast keeps the writer's sentence
        // (review N7).
        Err(crate::i18n::Text::AgentHooksDuplicateKey) => {
            return Err(HooksFound::Declined(
                crate::i18n::Text::AgentHooksDuplicateKeyRow.into(),
            ));
        }
        Err(_) => return Err(HooksFound::Unreadable),
    };
    let Ok(owners) = owners(family, &settings) else {
        return Err(HooksFound::Unreadable);
    };
    if unclassified_entry(family, &hooks) {
        return Err(HooksFound::Unclassified);
    }
    Ok(FamilyReading {
        owners,
        form: family_form(family, &hooks, exe),
    })
}

/// Whether an entry names a copy of Folio in a shape the family's [`Family::owner`] cannot read —
/// one with a key this build does not write, an argument list of another shape, a template it
/// never produced.
fn unclassified_entry(family: &Family, hooks: &Hooks<'_>) -> bool {
    hooks
        .events
        .iter()
        .flat_map(|event| &event.groups)
        .flat_map(|group| &group.entries)
        .any(|entry| {
            entry.own.is_none()
                && (family.owner)(&entry.value).is_err()
                && entry
                    .value
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| ownership::folio_like_command(command, family.agent))
        })
}

/// **The form of this copy's entries** (R3 and ruling N1, 2026-11-05; [`ownership::Form`]).
///
/// Old only in the ruled shapes: an entry an install claims ([`claims`]) that is not in today's
/// form ([`in_todays_form`]: a string form C1–C3, another spelling of the program, other
/// arguments), or no entry at all for an event that carries resume (`Stop`). Nothing else about an
/// own entry is drift — an `async` the person set is theirs, and so is the group they moved it
/// into — so the scan never names, and the repair never rewrites, a person's edit. An event the
/// person took out by hand that carries no resume is theirs to leave out. This copy's entries no
/// install claims are the person's placing, and named.
fn family_form(family: &Family, hooks: &Hooks<'_>, exe: &Path) -> ownership::Form {
    let wanted = (family.wanted)(exe);
    let claimed = claims(hooks, &wanted, exe);
    for (want, at) in wanted.iter().zip(&claimed) {
        match at {
            Some((e, g, h)) => {
                let value = &hooks.events[*e].groups[*g].entries[*h].value;
                if !in_todays_form(family, value, want) {
                    return ownership::Form::Old;
                }
            }
            None if want.carries_resume => return ownership::Form::Old,
            None => {}
        }
    }
    let mut placed: Vec<String> = Vec::new();
    for (e, event) in hooks.events.iter().enumerate() {
        for (g, group) in event.groups.iter().enumerate() {
            for (h, entry) in group.entries.iter().enumerate() {
                if entry.own.is_some()
                    && !claimed.contains(&Some((e, g, h)))
                    && !placed.contains(&event.member.key)
                {
                    placed.push(event.member.key.clone());
                }
            }
        }
    }
    if placed.is_empty() {
        ownership::Form::Today
    } else {
        ownership::Form::Placed(placed)
    }
}

/// **The row an entry of Folio's speaks for**: the `<row>` of its `<family>:<row>` word
/// ([`Family::agent`]), from its `args` or, in the string forms, from the line after ` attention `
/// (a payload's ` --json -` is not part of it). [`Family::owner`] reads an owner only off an entry
/// that names one.
fn row_of(agent: &str, value: &Value) -> Option<String> {
    let family = format!("{agent}:");
    if let Some(args) = value.get("args") {
        return args
            .get(1)
            .and_then(Value::as_str)
            .and_then(|word| word.strip_prefix(&family))
            .map(str::to_owned);
    }
    let line = value.get("command").and_then(Value::as_str)?;
    let (_, tail) = line.rsplit_once(&format!(" attention {family}"))?;
    Some(tail.strip_suffix(" --json -").unwrap_or(tail).to_owned())
}

/// **The own entry each wanted entry claims** (ruling N1, 2026-11-05), in `wanted`'s order: under
/// the wanted event, one that speaks for the same row ([`Own::row`]) — **wherever in that
/// event it stands**, in a group of Folio's own or in one of the person's, whatever its matcher —
/// this copy's first, then any copy's. Ownership is by content and never by position: an entry
/// the person moved is still the entry it was. [`family_form`] and [`edited`] both ask this.
fn claims(hooks: &Hooks<'_>, wanted: &[Wanted], exe: &Path) -> Vec<Option<(usize, usize, usize)>> {
    let mut claimed: Vec<Option<(usize, usize, usize)>> = Vec::with_capacity(wanted.len());
    for want in wanted {
        let candidates: Vec<((usize, usize, usize), &Entry<'_>)> = hooks
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.member.key == want.event)
            .flat_map(|(e, event)| {
                event.groups.iter().enumerate().flat_map(move |(g, group)| {
                    group
                        .entries
                        .iter()
                        .enumerate()
                        .map(move |(h, entry)| ((e, g, h), entry))
                })
            })
            .filter(|(at, entry)| {
                !claimed.contains(&Some(*at))
                    && entry.own.as_ref().is_some_and(|own| own.row == want.row)
            })
            .collect();
        let this_copys = candidates.iter().find(|(_, entry)| {
            entry
                .own
                .as_ref()
                .is_some_and(|own| crate::explorer_menu::same_path(&own.owner, exe))
        });
        claimed.push(this_copys.or(candidates.first()).map(|(at, _)| *at));
    }
    claimed
}

/// **Whether a claimed entry is in today's form**: today's shape ([`Family::program_keys`] all
/// there), naming this copy's program as this build spells it, with the wanted entry's arguments —
/// every program key as the wanted entry has it. Every other key of it is the person's to set —
/// `async` among them, and codex's `timeout`.
fn in_todays_form(family: &Family, value: &Value, want: &Wanted) -> bool {
    family.program_keys.iter().all(|key| {
        value
            .get(*key)
            .is_some_and(|held| Some(held) == want.hook.get(*key))
    })
}

/// **An entry of Folio's brought into today's form** (`todays`, [`Family::todays`] for its row): an
/// entry in an older shape — one without every one of the family's [`Family::program_keys`], such
/// as Claude Code's string forms — is replaced by the entry this build writes; an entry in today's
/// shape gets this copy's program and the row's arguments, and keeps every other key it has, as
/// the person left them.
fn repaired_entry(family: &Family, value: &Value, todays: &Value) -> Value {
    match value.as_object() {
        Some(entry)
            if family
                .program_keys
                .iter()
                .all(|key| entry.contains_key(*key)) =>
        {
            let mut entry = entry.clone();
            for key in family.program_keys {
                entry.insert((*key).to_owned(), todays[*key].clone());
            }
            Value::Object(entry)
        }
        _ => todays.clone(),
    }
}

/// One hook entry as the file has it, and whether it is provably Folio's.
struct Entry<'a> {
    node: &'a splice::Node,
    value: Value,
    /// Whose it is and what it speaks for, when [`entry_owners`] reads it as a Folio executable
    /// speaking `attention`. `None` is every other entry — the person's own, and one that looks
    /// like Folio's but that this build cannot read as one — and an entry that is `None` is never
    /// edited, moved or deleted.
    own: Option<Own>,
}

/// An entry of Folio's: the executable it runs, and the row it speaks for ([`row_of`]).
struct Own {
    owner: PathBuf,
    row: String,
}

/// One matcher group: its `hooks` array and the entries in it.
struct Group<'a> {
    rows: &'a splice::Node,
    entries: Vec<Entry<'a>>,
    /// Whether anything in the group is not Folio's: an entry of somebody else's, or — outside a
    /// [`Family::positional`] family — no entry at all, a group nobody but its writer put there.
    foreign: bool,
}

impl Group<'_> {
    /// See [`Group::foreign`].
    fn holds_foreign(&self) -> bool {
        self.foreign
    }
}

/// One event's member of the `hooks` object, and its groups.
struct Event<'a> {
    member: &'a splice::Property,
    groups: Vec<Group<'a>>,
}

impl Event<'_> {
    fn groups_share(&self) -> FolioShare {
        folio_share_of(self.groups.iter().map(|group| !group.holds_foreign()))
    }
}

/// The `hooks` member of the file, read down to its entries.
struct Hooks<'a> {
    member: &'a splice::Property,
    events: Vec<Event<'a>>,
}

impl Hooks<'_> {
    fn events_share(&self) -> FolioShare {
        folio_share_of(
            self.events
                .iter()
                .map(|event| event.groups_share() == FolioShare::OnlyFolios),
        )
    }
}

/// **What a container on the way to Folio's entries holds**, as the self-check reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FolioShare {
    /// Nothing at all: `{}` or `[]`, whatever whitespace stands between the brackets.
    Nothing,
    /// Folio's and nothing else.
    OnlyFolios,
    /// Something of somebody else's — and a container the person left empty inside it counts as
    /// theirs.
    Theirs,
}

/// The share of a container whose items are each Folio's alone (`true`) or not.
fn folio_share_of(items: impl IntoIterator<Item = bool>) -> FolioShare {
    let mut share = FolioShare::Nothing;
    for folios in items {
        if !folios {
            return FolioShare::Theirs;
        }
        share = FolioShare::OnlyFolios;
    }
    share
}

/// **The file's `hooks`, read down to every entry and whose it is.**
///
/// `None` when there is no `hooks` member at all. The shapes [`owners`] refuses are refused here
/// too, for the same reasons; on top of them, a key named twice anywhere on the way down to an
/// entry, or inside an entry that reads as Folio's ([`Text::AgentHooksDuplicateKey`](crate::i18n::Text::AgentHooksDuplicateKey)).
fn mapped<'a>(
    family: &Family,
    document: &'a splice::Document,
) -> Result<Option<Hooks<'a>>, crate::i18n::Text> {
    let unknown = || crate::i18n::Text::AgentHooksSchemaUnknown;
    let Some(root) = document.root.as_ref() else {
        return Ok(None);
    };
    if root.members().is_none() {
        return Err(crate::i18n::Text::AgentHooksUnreadable);
    }
    let Some(member) = root
        .member("hooks")
        .map_err(|_| crate::i18n::Text::AgentHooksDuplicateKey)?
    else {
        return Ok(None);
    };
    let members = member.value.members().ok_or_else(unknown)?;
    let mut events = Vec::with_capacity(members.len());
    for (index, event) in members.iter().enumerate() {
        if members[..index]
            .iter()
            .any(|earlier| earlier.key == event.key)
        {
            return Err(crate::i18n::Text::AgentHooksDuplicateKey);
        }
        let mut groups = Vec::new();
        for node in event.value.elements().ok_or_else(unknown)? {
            let rows = node
                .member("hooks")
                .map_err(|_| crate::i18n::Text::AgentHooksDuplicateKey)?
                .ok_or_else(unknown)?;
            // A matcher named twice is a key named twice. Which group an entry stands in says
            // nothing of whose it is (ruling N1), so the matcher is not kept beside the group.
            node.member("matcher")
                .map_err(|_| crate::i18n::Text::AgentHooksDuplicateKey)?;
            let nodes = rows.value.elements().ok_or_else(unknown)?;
            let values = nodes
                .iter()
                .map(|entry| value_at(document.text, entry))
                .collect::<Result<Vec<_>, _>>()?;
            let owners = entry_owners(family, &values);
            let mut entries = Vec::with_capacity(nodes.len());
            for ((node, value), owner) in nodes.iter().zip(values).zip(owners) {
                let own = owner.and_then(|owner| {
                    Some(Own {
                        row: row_of(family.agent, &value)?,
                        owner,
                    })
                });
                // An entry that reads as Folio's only because the value parser kept the last of two
                // keys of one name: which one the agent runs is its parser's business, and an edit
                // of the entry would take the other one's bytes with it.
                if own.is_some() && node.names_a_key_twice() {
                    return Err(crate::i18n::Text::AgentHooksDuplicateKey);
                }
                entries.push(Entry { node, value, own });
            }
            // Under a positional family a group with no entry of anybody else's is nobody's, empty
            // or not: the group Folio's removal leaves empty where it stood (ruling R16) is one.
            let foreign = entries.iter().any(|entry| entry.own.is_none())
                || (!family.positional && entries.is_empty());
            groups.push(Group {
                rows: &rows.value,
                entries,
                foreign,
            });
        }
        events.push(Event {
            member: event,
            groups,
        });
    }
    Ok(Some(Hooks { member, events }))
}

/// The value a span holds, as the classifiers read it.
fn value_at(text: &str, node: &splice::Node) -> Result<Value, crate::i18n::Text> {
    serde_json::from_str(&text[node.start..node.end])
        .map_err(|_| crate::i18n::Text::AgentHooksUnreadable)
}

/// What becomes of one entry.
#[derive(Clone)]
enum Fate {
    Kept,
    Replaced(String),
    Deleted,
}

/// **The file with Folio's entries put in (`install`) or taken out, every other byte as it was.**
///
/// `Ok(None)` when there is nothing to change. The only edits ever made:
///
/// - **an own entry brought into today's form, where it stands** ([`repaired_entry`]) — a repair
///   keeps the entry's place among the person's, and every key of it that is not the program or
///   its arguments;
/// - **an own entry deleted**, with its separator, and the group, event or `hooks` that held
///   nothing but Folio's going with it;
/// - **an entry that is missing appended**: a group at the end of its event (after the person's
///   own), an event at the end of `hooks`, `hooks` at the end of the file's object — laid out like
///   its neighbours.
///
/// An install claims, for each entry it wants, an own entry under the same event that speaks for
/// the same request, wherever in the event it stands ([`claims`]), of whichever copy of Folio —
/// `apply_resolved` has asked about every other copy before this runs — rewrites a claimed entry
/// only when it is not in today's form, deletes the other copies' entries left unclaimed and keeps
/// this copy's (ruling N1, 2026-11-05). A removal deletes the entries of this copy and of copies
/// that are gone, and no other. Then [`verified`] reads the result back, and a
/// result that changed anything else is refused whole.
///
/// **Under a [`Family::positional`] family a group of Folio's with any group after it is emptied
/// where it stands** — its entry deleted, the group left as `"hooks": []` — rather than taken out
/// (ruling R16, which amends the D1 rule above for Codex's `hooks.json` alone): every group after
/// it keeps its index, so every approval the person gave one of theirs stays attached to it. An
/// empty group is one Codex loads without a word.
fn edited(
    family: &Family,
    document: &splice::Document,
    exe: &Path,
    install: bool,
) -> Result<Option<String>, &'static str> {
    edited_as(family, document, exe, install, false)
}

/// [`edited`], and with `any_copy` a removal that deletes the entries of **every** copy of Folio
/// ([`hooks_cleared`]).
fn edited_as(
    family: &Family,
    document: &splice::Document,
    exe: &Path,
    install: bool,
    any_copy: bool,
) -> Result<Option<String>, &'static str> {
    let text = document.text;
    let style = splice::Style::of(document);
    let wanted = if install {
        (family.wanted)(exe)
    } else {
        Vec::new()
    };
    let Some(root) = document.root.as_ref() else {
        if wanted.is_empty() {
            return Ok(None);
        }
        let mut settings = Map::new();
        settings.insert(
            "hooks".to_owned(),
            Value::Object(events_value(&wanted, &[])),
        );
        let fresh = format!(
            "{}{}{}",
            &text[..document.body],
            splice::render(&Value::Object(settings), "", &style, true),
            style.newline
        );
        return verified(family, document, fresh).map(Some);
    };
    let root_members = root
        .members()
        .ok_or(crate::i18n::Text::AgentHooksUnreadable.text())?;
    // A file of one line gets its new entries on that line; a file with nothing in it yet, over
    // lines like everything else this build writes.
    let pretty = root_members.is_empty() || text[root.start..root.end].contains('\n');
    let root_items =
        splice::Container::of(text, root).ok_or(crate::i18n::Text::AgentHooksUnreadable.text())?;
    let mut edits = Vec::new();
    let Some(hooks) = mapped(family, document).map_err(crate::i18n::Text::text)? else {
        if wanted.is_empty() {
            return Ok(None);
        }
        let layout = root_items.layout(&style, pretty);
        let member = format!(
            "{}{}{}",
            Value::from("hooks"),
            style.colon,
            splice::render(
                &Value::Object(events_value(&wanted, &[])),
                &layout.indent,
                &style,
                layout.pretty
            )
        );
        edits.extend(root_items.edits(
            &vec![false; root_members.len()],
            &[member],
            &style,
            &layout,
        ));
        return splice_checked(family, document, edits);
    };

    // An entry written over where it stands, on as many lines as it stood on.
    let rewritten = |entry: &Entry<'_>, value: &Value| {
        let old = &text[entry.node.start..entry.node.end];
        Fate::Replaced(splice::render(
            value,
            splice::line_indent(text, entry.node.start),
            &style,
            old.contains('\n'),
        ))
    };
    // Every entry's fate, by event, group and entry.
    let mut fates: Vec<Vec<Vec<Fate>>> = hooks
        .events
        .iter()
        .map(|event| {
            event
                .groups
                .iter()
                .map(|group| {
                    group
                        .entries
                        .iter()
                        .map(|entry| match &entry.own {
                            None => Fate::Kept,
                            // Cleared: gone, whichever copy it names.
                            Some(_) if any_copy => Fate::Deleted,
                            // **An own entry no install claims is the person's placing** (ruling
                            // N1, 2026-11-05) — a copy of one of Folio's entries they put under
                            // another event, or a second one — and an install leaves it where it
                            // stands; a removal takes it out with this copy's others.
                            Some(own) if install => {
                                if crate::explorer_menu::same_path(&own.owner, exe) {
                                    Fate::Kept
                                } else {
                                    // Another copy's — moved, gone, or taken over by the press's
                                    // consent: `apply_resolved` has asked about every other copy —
                                    // is pointed at this copy where it stands, with every other
                                    // key the person left on it (re-review R1). A wanted entry
                                    // that claims it below decides its fate again.
                                    let todays = (family.todays)(&own.row, exe);
                                    rewritten(entry, &repaired_entry(family, &entry.value, &todays))
                                }
                            }
                            Some(own) if ownership::other_live(&own.owner, exe) == Ok(false) => {
                                Fate::Deleted
                            }
                            Some(_) => Fate::Kept,
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let mut missing: Vec<&Wanted> = Vec::new();
    for (want, at) in wanted.iter().zip(claims(&hooks, &wanted, exe)) {
        let Some((e, g, h)) = at else {
            missing.push(want);
            continue;
        };
        let entry = &hooks.events[e].groups[g].entries[h];
        // Claimed where it stands. Only an old shape is rewritten, and only what makes it old: a
        // person's edit of Folio's own entry is theirs (ruling N1).
        fates[e][g][h] = if in_todays_form(family, &entry.value, want) {
            Fate::Kept
        } else {
            rewritten(entry, &repaired_entry(family, &entry.value, &want.hook))
        };
    }

    let mut event_gone = Vec::with_capacity(hooks.events.len());
    let mut inner = Vec::new();
    for (e, event) in hooks.events.iter().enumerate() {
        let appended: Vec<Value> = missing
            .iter()
            .filter(|want| want.event == event.member.key)
            .map(|want| want.hooks_group())
            .collect();
        let deleted: Vec<Vec<bool>> = fates[e]
            .iter()
            .map(|fates| {
                fates
                    .iter()
                    .map(|fate| matches!(fate, Fate::Deleted))
                    .collect()
            })
            .collect();
        // A group goes when it held entries and every one of them goes; one the person left
        // empty stays empty.
        let mut group_gone: Vec<bool> = deleted
            .iter()
            .map(|deleted| !deleted.is_empty() && deleted.iter().all(|gone| *gone))
            .collect();
        if family.positional {
            // From the end: a group that would go, with a group after it that stays, stays
            // itself — emptied — and is then a group that stays for the ones before it.
            let mut stays_after = false;
            for gone in group_gone.iter_mut().rev() {
                if *gone && stays_after {
                    *gone = false;
                }
                stays_after |= !*gone;
            }
        }
        let mut kept = Vec::new();
        for (g, group) in event.groups.iter().enumerate() {
            if group_gone[g] {
                continue;
            }
            for (entry, fate) in group.entries.iter().zip(&fates[e][g]) {
                if let Fate::Replaced(written) = fate {
                    kept.push(splice::Edit {
                        start: entry.node.start,
                        end: entry.node.end,
                        text: written.clone(),
                    });
                }
            }
            if deleted[g].iter().any(|gone| *gone) {
                let rows = splice::Container::of(text, group.rows)
                    .ok_or(crate::i18n::Text::AgentHooksUnreadable.text())?;
                let layout = rows.layout(&style, pretty);
                kept.extend(rows.edits(&deleted[g], &[], &style, &layout));
            }
        }
        // The same rule one level up: an event whose every group goes, and to which nothing is
        // appended, goes with them. An event the person left empty stays.
        let gone =
            !group_gone.is_empty() && group_gone.iter().all(|gone| *gone) && appended.is_empty();
        event_gone.push(gone);
        if gone {
            continue;
        }
        inner.extend(kept);
        if group_gone.iter().any(|gone| *gone) || !appended.is_empty() {
            let groups = splice::Container::of(text, &event.member.value)
                .ok_or(crate::i18n::Text::AgentHooksUnreadable.text())?;
            let layout = groups.layout(&style, pretty);
            let appended: Vec<String> = appended
                .iter()
                .map(|group| splice::render(group, &layout.indent, &style, layout.pretty))
                .collect();
            inner.extend(groups.edits(&group_gone, &appended, &style, &layout));
        }
    }
    let present: Vec<&str> = hooks
        .events
        .iter()
        .map(|event| event.member.key.as_str())
        .collect();
    let added = events_value(missing.iter().copied(), &present);
    let hooks_gone =
        !event_gone.is_empty() && event_gone.iter().all(|gone| *gone) && added.is_empty();
    if hooks_gone {
        let deleted: Vec<bool> = root_members
            .iter()
            .map(|member| member.key_start == hooks.member.key_start)
            .collect();
        let layout = root_items.layout(&style, pretty);
        edits.extend(root_items.edits(&deleted, &[], &style, &layout));
        return splice_checked(family, document, edits);
    }
    edits.extend(inner);
    if event_gone.iter().any(|gone| *gone) || !added.is_empty() {
        let events = splice::Container::of(text, &hooks.member.value)
            .ok_or(crate::i18n::Text::AgentHooksUnreadable.text())?;
        let layout = events.layout(&style, pretty);
        let appended: Vec<String> = added
            .iter()
            .map(|(name, groups)| {
                format!(
                    "{}{}{}",
                    Value::from(name.as_str()),
                    style.colon,
                    splice::render(groups, &layout.indent, &style, layout.pretty)
                )
            })
            .collect();
        edits.extend(events.edits(&event_gone, &appended, &style, &layout));
    }
    splice_checked(family, document, edits)
}

/// The events the file does not have yet, each with the groups its missing entries are written in.
fn events_value<'w>(
    missing: impl IntoIterator<Item = &'w Wanted>,
    present: &[&str],
) -> Map<String, Value> {
    let mut events = Map::new();
    for want in missing {
        if present.contains(&want.event) {
            continue;
        }
        if let Value::Array(groups) = events
            .entry(want.event.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
        {
            groups.push(want.hooks_group());
        }
    }
    events
}

/// `edits` made, and the result read back by [`verified`] before anybody may write it.
fn splice_checked(
    family: &Family,
    document: &splice::Document,
    edits: Vec<splice::Edit>,
) -> Result<Option<String>, &'static str> {
    if edits.is_empty() {
        return Ok(None);
    }
    let text = splice::spliced(document.text, edits)
        .ok_or(crate::i18n::Text::AgentHooksSpliceRefused.text())?;
    verified(family, document, text).map(Some)
}

/// **The write-time self-check** (0.4.10 plan row 67a): the text an edit produced is handed on
/// only when, read back, it says everything `before` said that was not Folio's, in the same bytes.
///
/// Three things are compared, and any difference refuses the write whole
/// ([`Text::AgentHooksSpliceRefused`](crate::i18n::Text::AgentHooksSpliceRefused)):
///
/// 1. that the result is a document, and its top level an object;
/// 2. **the whole file with Folio's extents cut out, byte for byte** ([`outside_folios`]): every
///    byte that is not Folio's — the byte-order mark, the whitespace, the commas and brackets
///    between items, the keys of the person's groups and events, a container the person left
///    empty — stands in both, in the same place among the others; and under a
///    [`Family::positional`] family, **the place every entry of somebody else's stands at**
///    ([`foreign_places`], row 69a), which a cut cannot see;
/// 3. the two documents as values, with Folio's entries and the containers left empty by taking
///    them out stripped from both ([`without_folio`]).
///
/// A file that held nothing but whitespace (D4) has only its byte-order mark compared, and what
/// replaces it must be Folio's alone.
///
/// The edit plan above is written to pass this; the check is what makes a mistake in it a refusal
/// rather than somebody's lost hook. Its cuts are worked out here, apart from the plan's, so that a
/// mistake in the plan's separators is not made twice.
fn verified(
    family: &Family,
    before: &splice::Document,
    text: String,
) -> Result<String, &'static str> {
    if keeps_what_is_not_ours(family, before, &text) {
        Ok(text)
    } else {
        Err(crate::i18n::Text::AgentHooksSpliceRefused.text())
    }
}

fn keeps_what_is_not_ours(family: &Family, before: &splice::Document, text: &str) -> bool {
    let Ok(after) = splice::parse(text) else {
        return false;
    };
    let (Some(root), Ok(old), Ok(new)) = (
        after.root.as_ref(),
        mapped(family, before),
        mapped(family, &after),
    ) else {
        return false;
    };
    if root.members().is_none() {
        return false;
    }
    let outside_kept = if before.root.is_none() {
        before.text[..before.body] == after.text[..after.body]
            && root_folio_share(&after, new.as_ref()) == FolioShare::OnlyFolios
    } else {
        let filled = FilledByFolio::by_edit(before, old.as_ref(), &after, new.as_ref());
        match (
            outside_folios(before, old.as_ref(), &filled),
            outside_folios(&after, new.as_ref(), &filled),
        ) {
            (Some(old), Some(new)) => old == new,
            _ => false,
        }
    };
    outside_kept
        && (!family.positional || foreign_places(old.as_ref()) == foreign_places(new.as_ref()))
        && match (before.whole_value(), after.whole_value()) {
            (Ok(old), Ok(new)) => without_folio(family, old) == without_folio(family, new),
            _ => false,
        }
}

/// **Where every entry of somebody else's stands** — its event, its group's index in that event
/// and its own in the group — which is what a [`Family::positional`] family's agent keys the
/// person's approval of that entry by. [`outside_folios`] compares bytes with Folio's extents cut
/// out, so a group of Folio's put in front of the person's, or taken out from in front of it, cuts
/// to the same text: the places are what moved.
fn foreign_places<'a>(hooks: Option<&Hooks<'a>>) -> Vec<(&'a str, usize, usize)> {
    let mut found = Vec::new();
    for event in hooks.map_or(&[][..], |hooks| hooks.events.as_slice()) {
        for (group_at, group) in event.groups.iter().enumerate() {
            for (entry_at, entry) in group.entries.iter().enumerate() {
                if entry.own.is_none() {
                    found.push((event.member.key.as_str(), group_at, entry_at));
                }
            }
        }
    }
    found
}

/// The share of the file's own object: Folio's alone when its one member is a `hooks` that is.
fn root_folio_share(document: &splice::Document, hooks: Option<&Hooks>) -> FolioShare {
    let hooks_folios = hooks.is_some_and(|hooks| hooks.events_share() == FolioShare::OnlyFolios);
    folio_share_of(
        document
            .root
            .as_ref()
            .and_then(splice::Node::members)
            .unwrap_or_default()
            .iter()
            .map(|member| member.key == "hooks" && hooks_folios),
    )
}

/// **The containers the person left empty that the edit filled with Folio's alone** (rulings D1
/// and N2): the file's own object, `hooks`, and events by name. Such a container stands on both
/// sides of the comparison as its key and brackets, with what stands between them cut — the
/// whitespace the person left inside on one side, Folio's entries on the other. Every other
/// container the person left empty is compared whole, so an edit that took one away, or put
/// anything but Folio's into it, is refused.
struct FilledByFolio {
    root: bool,
    hooks: bool,
    events: Vec<String>,
}

impl FilledByFolio {
    fn by_edit(
        before: &splice::Document,
        old: Option<&Hooks>,
        after: &splice::Document,
        new: Option<&Hooks>,
    ) -> Self {
        let filled = |was: FolioShare, is: FolioShare| {
            was == FolioShare::Nothing && is == FolioShare::OnlyFolios
        };
        let events = match (old, new) {
            (Some(old), Some(new)) => old
                .events
                .iter()
                .filter(|event| {
                    new.events.iter().any(|now| {
                        now.member.key == event.member.key
                            && filled(event.groups_share(), now.groups_share())
                    })
                })
                .map(|event| event.member.key.clone())
                .collect(),
            _ => Vec::new(),
        };
        Self {
            root: filled(root_folio_share(before, old), root_folio_share(after, new)),
            hooks: old
                .zip(new)
                .is_some_and(|(old, new)| filled(old.events_share(), new.events_share())),
            events,
        }
    }
}

/// **The file with every extent of Folio's cut out**: Folio's entries, a group of nothing but
/// Folio's entries, an event of nothing but such groups, and a `hooks` of nothing but such
/// events, each with one separator next to it; a container all of whose items are Folio's with
/// everything between its brackets. What is left is every byte that is not Folio's, in its place.
///
/// `None` for a document whose top level is not an object.
fn outside_folios(
    document: &splice::Document,
    hooks: Option<&Hooks>,
    filled: &FilledByFolio,
) -> Option<String> {
    let text = document.text;
    let root = document.root.as_ref()?;
    let mut cuts = Vec::new();
    if filled.root {
        cuts.push((root.start + 1, root.end - 1));
    } else {
        let hooks_go = !filled.hooks
            && hooks.is_some_and(|hooks| hooks.events_share() == FolioShare::OnlyFolios);
        let members: Vec<bool> = root
            .members()?
            .iter()
            .map(|member| member.key == "hooks" && hooks_go)
            .collect();
        cut_folio_items(root, &members, &mut cuts);
        match hooks {
            Some(hooks) if filled.hooks => {
                let value = &hooks.member.value;
                cuts.push((value.start + 1, value.end - 1));
            }
            Some(hooks) if !hooks_go && hooks.events_share() == FolioShare::Theirs => {
                let events: Vec<bool> = hooks
                    .events
                    .iter()
                    .map(|event| {
                        event.groups_share() == FolioShare::OnlyFolios
                            && !filled.events.contains(&event.member.key)
                    })
                    .collect();
                cut_folio_items(&hooks.member.value, &events, &mut cuts);
                for event in &hooks.events {
                    let value = &event.member.value;
                    if filled.events.contains(&event.member.key) {
                        cuts.push((value.start + 1, value.end - 1));
                    } else if event.groups_share() == FolioShare::Theirs {
                        let groups: Vec<bool> = event
                            .groups
                            .iter()
                            .map(|group| !group.holds_foreign())
                            .collect();
                        cut_folio_items(value, &groups, &mut cuts);
                        for group in event.groups.iter().filter(|group| group.holds_foreign()) {
                            let entries: Vec<bool> = group
                                .entries
                                .iter()
                                .map(|entry| entry.own.is_some())
                                .collect();
                            cut_folio_items(group.rows, &entries, &mut cuts);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    cuts.sort_unstable();
    let mut rest = String::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in cuts {
        rest.push_str(&text[at..start]);
        at = end;
    }
    rest.push_str(&text[at..]);
    Some(rest)
}

/// The cuts that take a container's Folio items (`folios`) out, each run of them with one
/// separator: up to the next item's first byte when one follows, from the last other item's end
/// when none does; and everything between the brackets when every item is Folio's.
fn cut_folio_items(node: &splice::Node, folios: &[bool], cuts: &mut Vec<(usize, usize)>) {
    let items: Vec<(usize, usize)> = match (node.members(), node.elements()) {
        (Some(members), _) => members.iter().map(splice::Property::span).collect(),
        (_, Some(elements)) => elements.iter().map(|item| (item.start, item.end)).collect(),
        _ => return,
    };
    if !items.is_empty() && folios.iter().all(|folio| *folio) {
        cuts.push((node.start + 1, node.end - 1));
        return;
    }
    let mut index = 0;
    while index < items.len() {
        if !folios[index] {
            index += 1;
            continue;
        }
        let mut last = index;
        while last + 1 < items.len() && folios[last + 1] {
            last += 1;
        }
        cuts.push(if last + 1 < items.len() {
            (items[index].0, items[last + 1].0)
        } else {
            (items[index - 1].1, items[last].1)
        });
        index = last + 1;
    }
}

/// A settings value with every entry [`entry_owners`] reads as Folio's taken out, and then every
/// group, event and `hooks` object left empty — the person's own empty ones with them, on both
/// sides of the comparison alike.
fn without_folio(family: &Family, mut settings: Value) -> Value {
    let Some(object) = settings.as_object_mut() else {
        return settings;
    };
    if let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut() {
            let Some(groups) = groups.as_array_mut() else {
                continue;
            };
            for group in groups.iter_mut() {
                if let Some(rows) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    // `retain` visits every entry once and in order, so the owners read in that
                    // order are the entries' own.
                    let mut owners = entry_owners(family, rows).into_iter();
                    rows.retain(|_| owners.next().flatten().is_none());
                }
            }
            groups.retain(|group| {
                group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|rows| !rows.is_empty())
            });
        }
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        if hooks.is_empty() {
            object.remove("hooks");
        }
    }
    settings
}

/// [`edited`] over a value, for the tests that ask about the edit and not about a file: the value
/// is written out the way `serde_json` writes one, edited, and read back.
#[cfg(test)]
fn edited_value(settings: &mut Value, exe: &Path, install: bool) -> bool {
    let text = serde_json::to_string_pretty(settings).expect("a value renders");
    let Ok(document) = splice::parse(&text) else {
        return false;
    };
    match edited(&CLAUDE, &document, exe, install) {
        Ok(Some(text)) => {
            *settings = serde_json::from_str(&text).expect("the splice writes JSON");
            true
        }
        Ok(None) | Err(_) => false,
    }
}

/// Put Folio's entries into a value; whether anything changed.
#[cfg(test)]
fn install_into(settings: &mut Value, exe: &Path) -> bool {
    install_into_on(settings, exe, bt_platform::host_platform())
}

/// Direct execution has the same shape on every platform.
#[cfg(test)]
fn install_into_on(settings: &mut Value, exe: &Path, _platform: HostPlatform) -> bool {
    edited_value(settings, exe, true)
}

/// Take this copy's entries, and those of copies that are gone, back out of a value.
#[cfg(test)]
fn remove_from(settings: &mut Value, exe: &Path) -> bool {
    edited_value(settings, exe, false)
}

/// Put Folio's hooks in, or take them out, on this machine.
///
/// **The file is read and written whole, and only Folio's own entries in it change** ([`edited`]),
/// and a copy of what was there is kept beside it the first time each day — `shell_integration`'s rule, for `shell_integration`'s reason: this is
/// somebody's own configuration file, and a build that could damage one had better be able to hand
/// it back.
pub(crate) fn apply(decision: Decision, exe: &Path) -> Outcome {
    let Some(path) = settings_path() else {
        return Outcome::Refused("no user configuration directory to write into");
    };
    apply_at(&path, decision, exe, &crate::persist::storage_dir())
}

/// The same act on a named file — the seam the tests press, so that what they pin is this
/// function and not a settings file belonging to whoever runs them.
pub(crate) fn apply_at(path: &Path, decision: Decision, exe: &Path, data: &Path) -> Outcome {
    match Config::resolve(path) {
        Ok(config) => apply_resolved(&config, decision, exe, data),
        Err(Unresolved::Refused(reason)) => Outcome::Refused(reason),
        Err(Unresolved::Unreadable) => {
            Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text())
        }
    }
}

/// The same act on a configuration this operation has already resolved.
///
/// **Everything below the entry takes this value.** The path was resolved once, at the top; the
/// read, the dated copy and the replace all name that one answer, and there is no second
/// resolution on this path for a repointed link to slip through (re-review B1).
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
    let existing = match config.standing() {
        Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        Standing::Nothing => String::new(),
        // Refused rather than replaced, exactly as an unparseable file is.
        Standing::Unreadable => {
            return Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text());
        }
    };
    let paths = match hook_owners(&CLAUDE, &existing) {
        Ok(paths) => paths,
        Err(reason) => return Outcome::Refused(reason.text()),
    };
    let others = match ownership::check(&paths, exe, &decision) {
        Ok(others) => others,
        Err(outcome) => return outcome,
    };
    let written = match hooks_edited(&CLAUDE, &existing, exe, install) {
        Ok(written) => written,
        Err(reason) => return Outcome::Refused(reason),
    };
    let _record = if install {
        match ownership::record(
            data,
            config.named().parent().expect("absolute config"),
            "claude",
        ) {
            Ok(lock) => Some(lock),
            Err(reason) => return Outcome::Refused(reason),
        }
    } else {
        None
    };
    let Some(text) = written else {
        return if others.is_empty() {
            Outcome::Unchanged
        } else {
            Outcome::LeftOther(others)
        };
    };
    match config.land(&existing, "json", text.as_bytes()) {
        Landing::Landed => {}
        Landing::NoDirectory => {
            return Outcome::Refused("the user configuration directory could not be created");
        }
        Landing::NoBackup => {
            return Outcome::Refused(crate::i18n::Text::AgentConfigNoBackup.text());
        }
        Landing::Changed => {
            return Outcome::Refused(crate::i18n::Text::AgentConfigChanged.text());
        }
        Landing::NotWritten => return Outcome::Refused("the settings file could not be written"),
    }
    if install {
        Outcome::Installed
    } else if others.is_empty() {
        Outcome::Removed
    } else {
        Outcome::LeftOther(others)
    }
}

/// **Every copy of Folio a family's hook file names**, of whichever copy, read off the text the
/// operation read — or the refusal of a file this build cannot read.
///
/// Read as spans, for the edit, and as a value, for the classifiers: one text, two views of it.
/// Refused rather than replaced when either cannot read it. A settings file this build cannot read
/// is a settings file somebody wrote, and overwriting it to add a convenience is not a trade anyone
/// agreed to.
pub(crate) fn hook_owners(
    family: &Family,
    existing: &str,
) -> Result<Vec<PathBuf>, crate::i18n::Text> {
    let document = splice::parse(existing).map_err(|_| crate::i18n::Text::AgentHooksUnreadable)?;
    match document.whole_value() {
        Ok(value) if value.is_object() => owners(family, &value),
        _ => Err(crate::i18n::Text::AgentHooksUnreadable),
    }
}

/// **The text with this copy's entries put in (`install`) or taken out**, every byte that is not
/// Folio's as it was (the owner's rule of 2026-11-04): see [`edited`] and [`verified`]. `Ok(None)`
/// when there is nothing to change.
pub(crate) fn hooks_edited(
    family: &Family,
    existing: &str,
    exe: &Path,
    install: bool,
) -> Result<Option<String>, &'static str> {
    let document =
        splice::parse(existing).map_err(|_| crate::i18n::Text::AgentHooksUnreadable.text())?;
    edited(family, &document, exe, install)
}

/// **The text with every entry of Folio's taken out, of whichever copy** — for an operation that has
/// asked about every other live copy first: codex's `notify` line written in place of its hook, on
/// a codex below 0.148 (0.4.10 plan row 69c; R18's rule, the other way round). Every byte that is
/// not Folio's as it was, as [`hooks_edited`]; `Ok(None)` when there is nothing to take out.
pub(crate) fn hooks_cleared(
    family: &Family,
    existing: &str,
    exe: &Path,
) -> Result<Option<String>, &'static str> {
    let document =
        splice::parse(existing).map_err(|_| crate::i18n::Text::AgentHooksUnreadable.text())?;
    edited_as(family, &document, exe, false, true)
}

/// **What is standing at a configuration file's path**, as the answer to "is there one".
///
/// The distinction between these three is the whole of the read side of [`land`]'s contract.
/// `std::fs::read_to_string` fails the same way for a file that is not there and for one this
/// build was not allowed to read, and an installer that took both for "there was nothing there"
/// writes a whole fresh file over somebody's own — with no copy kept beside it, because `land` is
/// given nothing to copy. Release audit 2026-09-16 (C-3): one non-UTF-8 byte in `config.toml` or
/// `settings.json` is enough, the file is perfectly writable, and the loss is certain rather than
/// a race. So the error kind is asked about, and everything that is not "no such file" refuses.
#[derive(Clone)]
pub(crate) enum Standing {
    /// There is no file at that path. A write there creates one and destroys nothing.
    Nothing,
    /// The file's own text, as it reads today.
    Text(String),
    /// There is something at that path and this build could not read it. **Never written over.**
    Unreadable,
}

/// Why a configuration path did not resolve to a file this build may edit.
///
/// Answered once per operation, by [`Config::resolve`], and never again below it.
pub(crate) enum Unresolved {
    /// The path could not be examined at all — an ancestor this account may not ask about. Not a
    /// file this build declines to edit: one it could not look at.
    Unreadable,
    /// It resolves, it can be read, and **this build will not edit it** — it is a link out of the
    /// agent's own folder, it is shared by hard links, or it is read-only. The sentence is the
    /// filesystem predicate's own, because each of the three is a different thing to do about
    /// (closure review R1).
    Refused(&'static str),
}

/// **One resolution of one configuration path, carried through the whole of one operation.**
///
/// The re-review's blocking finding: the read resolved the path and took target A, and the write
/// resolved it again on the way out. Between the two, the user's own junction can be repointed — a
/// dotfiles `stow`, a profile switch, anything with write access to a link that is theirs — and
/// the second answer is the one that writes. Folio would land a document derived from one file
/// onto another, and keep a dated copy of the *other* file's bytes beside it.
///
/// So an operation asks **once**, at its own top, and everything below takes this value. Nothing
/// under here is handed the configuration path at all: [`editable_target`] is private to this
/// module and `Config::resolve` is its only caller, which is what makes a second resolution on
/// that path a thing that cannot be written rather than a thing nobody happened to write.
pub(crate) struct Config {
    /// The path the agent's environment named. Kept for the one removal that has to take the name
    /// away as well as the file, and for the record of where a mark was written.
    named: PathBuf,
    /// The file that path resolved to, once.
    target: PathBuf,
    /// **The file as the operation read it, when the operation reads it once**
    /// ([`Self::with_one_read`]): every read below answers this, so what was classified is what
    /// is edited, and [`land`] writes only over these bytes.
    one_read: Option<Standing>,
}

impl Config {
    /// Resolve a configuration path for one operation. **The only door.**
    pub(crate) fn resolve(path: &Path) -> Result<Self, Unresolved> {
        Self::resolve_with(path, editable_target)
    }

    /// The same, with the resolution handed in — the seam a test presses to prove that an
    /// operation resolves once and that the read, the copy and the replace all name that answer.
    /// A resolver that would answer differently the second time is never asked a second time.
    pub(crate) fn resolve_with(
        path: &Path,
        resolve: impl FnOnce(&Path) -> Result<PathBuf, Unresolved>,
    ) -> Result<Self, Unresolved> {
        Ok(Self {
            named: path.to_path_buf(),
            target: resolve(path)?,
            one_read: None,
        })
    }

    /// **This resolution, its file read once for the whole operation** (0.4.10 plan row 67b): the
    /// start-time repair classifies the file and then writes it, and a press may write between the
    /// two. Held to one read, the classification and the edit are of the same bytes, and the
    /// landing's compare refuses the write when the file no longer holds them, so a repair never
    /// undoes a press it did not see.
    pub(crate) fn with_one_read(self) -> Self {
        let read = self.standing();
        Self {
            one_read: Some(read),
            ..self
        }
    }

    /// This resolution with its one read let go: what a write left, read as it stands now.
    pub(crate) fn read_afresh(self) -> Self {
        Self {
            one_read: None,
            ..self
        }
    }

    /// The path the environment named — never resolved again, only reported.
    pub(crate) fn named(&self) -> &Path {
        &self.named
    }

    /// Read a configuration file the way all three installers have to read one.
    ///
    /// Anything but [`std::io::ErrorKind::NotFound`] is [`Standing::Unreadable`]: a permission the
    /// user's own ACL withholds, a sharing lock somebody else's editor holds, a byte that is not
    /// UTF-8, a directory standing under the file's name. None of them is a file that is not
    /// there, and that is the only state in which writing a fresh one loses nothing.
    pub(crate) fn standing(&self) -> Standing {
        if let Some(read) = &self.one_read {
            return read.clone();
        }
        match bt_platform::file_reads::read_to_string(
            bt_platform::file_reads::Lane::Attention,
            &self.target,
        ) {
            Ok(text) => Standing::Text(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Standing::Nothing,
            Err(_) => Standing::Unreadable,
        }
    }

    /// Take the resolved file out, under every name that delivers it.
    ///
    /// Copilot's whole-file removal: where this path is a link this build resolved, the target is
    /// the file upstream loads — leaving it would leave Folio's hooks firing, and leaving the link
    /// alone would leave upstream a name that loads nothing. The second removal is best effort
    /// because the first may already have taken the only file there was.
    pub(crate) fn remove_file(&self) -> std::io::Result<()> {
        std::fs::remove_file(&self.target)?;
        if self.target != self.named {
            let _ = std::fs::remove_file(&self.named);
        }
        Ok(())
    }
}

/// **Which file a configuration path names, for reading and for writing.**
///
/// Design §6.3 refuses to write *through* a link, and the T-A predicate enforces that by refusing
/// any path with a link anywhere along it. Taken alone that rule strands the ordinary dotfiles
/// machine — `~/.claude` junctioned onto a managed folder — in the one state this whole design
/// exists to abolish: hooks installed by 0.4.2 still firing, a row reading `Off`, and no press
/// that can take them out (closure review R1).
///
/// So the link is resolved **once, here**, and what comes back is bounded: the target must be a
/// regular file inside the resolved directory the path names. Then Folio is not writing through a
/// link at all — it is writing to a file it resolved itself, in the agent's own folder. A target
/// that leaves that folder, a link that resolves to nothing, and every other answer the predicate
/// gives (a hard link, a read-only file, a directory) are refused with their own reason, and the
/// file is left byte-identical.
/// **Private, and called from exactly one place** — [`Config::resolve`]. Anything below an
/// operation's entry holds a [`Config`] and has no path to resolve a second time.
fn editable_target(path: &Path) -> Result<PathBuf, Unresolved> {
    let reason = match crate::shell_integration::profile_path_reason(path) {
        Ok(None) => return Ok(path.to_path_buf()),
        Ok(Some(reason)) => reason,
        // The walk itself failed — an ancestor this account may not even ask about. That is not a
        // file this build declines to edit, it is one it could not look at, and `Unreadable` has
        // been the honest answer to it since the 2026-09-16 audit.
        Err(_) => return Err(Unresolved::Unreadable),
    };
    if reason != crate::i18n::Text::ShellProfileLink {
        return Err(Unresolved::Refused(agent_reason(reason)));
    }
    let root = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok());
    let resolved = std::fs::canonicalize(path).ok();
    let regular = resolved
        .as_deref()
        .and_then(|target| std::fs::symlink_metadata(target).ok())
        .is_some_and(|metadata| metadata.is_file());
    let target = linked_target(&Resolution {
        root: root.as_deref(),
        resolved: resolved.as_deref(),
        regular,
    })
    .map_err(Unresolved::Refused)?;
    // The resolved target carries no link of its own, so what this can still answer is a hard
    // link, a read-only file or a directory — the refusals §6.3 keeps.
    match crate::shell_integration::profile_path_reason(&target) {
        Ok(None) => Ok(target),
        Ok(Some(reason)) => Err(Unresolved::Refused(agent_reason(reason))),
        Err(_) => Err(Unresolved::Unreadable),
    }
}

/// The filesystem's answers about a linked path, handed to [`linked_target`] rather than asked for
/// inside it — so the rule can be tested on an account that is not allowed to create a link.
pub(crate) struct Resolution<'a> {
    /// The directory the path names, resolved once. `None` when it does not resolve.
    pub root: Option<&'a Path>,
    /// What the path itself resolves to. `None` when it resolves to nothing.
    pub resolved: Option<&'a Path>,
    /// Whether that target is a regular file.
    pub regular: bool,
}

/// **The file a link names, when Folio will edit it.**
///
/// One question with one answer: is the thing at the end of this link a regular file inside the
/// directory the configuration path names? Everything else — a target somewhere else on the disk,
/// a link to a directory, a link to nothing — is a link Folio will not write through, and says so.
pub(crate) fn linked_target(facts: &Resolution) -> Result<PathBuf, &'static str> {
    let refused = crate::i18n::Text::AgentConfigLink.text();
    let (Some(root), Some(resolved)) = (facts.root, facts.resolved) else {
        return Err(refused);
    };
    if facts.regular && resolved.starts_with(root) {
        Ok(resolved.to_path_buf())
    } else {
        Err(refused)
    }
}

/// The same three facts about an agent's configuration file rather than about a `$PROFILE`.
///
/// A sentence naming the wrong file is a sentence a reader acts on, so the profile's wording does
/// not travel: the mapping is one to one, and nothing is flattened on the way.
fn agent_reason(reason: crate::i18n::Text) -> &'static str {
    match reason {
        crate::i18n::Text::ShellProfileLink => crate::i18n::Text::AgentConfigLink,
        crate::i18n::Text::ShellProfileHardLink => crate::i18n::Text::AgentConfigHardLink,
        _ => crate::i18n::Text::AgentConfigReadOnly,
    }
    .text()
}

/// How far [`land`] got.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Landing {
    /// The bytes are on disk, whole, under the target's own name.
    Landed,
    /// The directory the file lives in does not exist and could not be made.
    NoDirectory,
    /// There was a file there and a copy of it could not be kept. **Nothing was written.**
    NoBackup,
    /// The file changed between the read and the replace. **Nothing was written**, and no copy was
    /// kept either: a backup of bytes nobody read is not a copy of anything.
    Changed,
    /// The write itself failed. Whatever was there is still there, whole.
    NotWritten,
}

/// **The one way all three installers put bytes into somebody else's configuration file.**
///
/// Three properties, and each of them is a ruling rather than an implementation detail:
///
/// ① **The write is atomic.** A sibling temporary in the target's own directory, `write_all`,
/// `sync_all`, then a single-operation replace — `bt_persist::atomic_write`, the same writer
/// `session.json` and `settings.json` go through (`docs/M2-persistence-schema-v1.md` §5.2). A
/// `std::fs::write` stood here until the release audit and it has a window: the file is truncated
/// first, so a process killed mid-write leaves the user's own Claude Code, Codex or Copilot
/// configuration half a document — a file the upstream tool then cannot parse, damaged by a
/// convenience it did not ask for. Nothing on this path is ours to lose.
///
/// ② **The copy is a precondition, not a courtesy.** The dated backup used to be written with its
/// error dropped on the floor, which made the promise in every one of these modules' headers —
/// "a build that could damage one had better be able to hand it back" — conditional on a write
/// nobody checked. So a backup that cannot be kept is [`Landing::NoBackup`] and the target is not
/// touched at all. Only the *first* copy of each day is kept: a second press on the same day would
/// otherwise overwrite the copy of what was there before the first one, which is the one copy that
/// matters.
///
/// ③ **The bytes go to the file [`editable_target`] resolved, and nowhere else.** A link is
/// followed exactly once, by this build, and only to a regular file inside the folder the
/// configuration path names; hard links, read-only paths and anything that is not a regular file
/// are refused by the shared T-A filesystem predicate before reading and writing. A locked file
/// fails without replacement. Revision 2 deliberately supersedes the former link-following policy,
/// and closure review R1 is what bounds the resolution rather than abolishing it.
///
/// ④ **The bytes replaced are the bytes that were read.** The target is read again here, under
/// the value this operation resolved, and a document that no longer matches what the caller was
/// given is [`Landing::Changed`] rather than a write: `shell_integration::replace_profile` has
/// compared a profile against what it read since T-A, for the same reason, and an agent's
/// configuration is edited by the agent itself as readily as by a person.
///
/// `extension` is the target's own extension — `settings.json` with `"json"` gives
/// `settings.json.bak-20260827`. `existing` is what was read off the file, empty when there was
/// nothing there.
impl Config {
    pub(crate) fn land(&self, existing: &str, extension: &str, bytes: &[u8]) -> Landing {
        let target = &self.target;
        if let Some(parent) = target.parent()
            && !parent.as_os_str().is_empty()
            && std::fs::create_dir_all(parent).is_err()
        {
            return Landing::NoDirectory;
        }
        match bt_platform::file_reads::read_to_string(
            bt_platform::file_reads::Lane::Attention,
            target,
        ) {
            Ok(now) if now != existing => return Landing::Changed,
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Landing::NotWritten;
            }
            Err(_) if !existing.is_empty() => return Landing::Changed,
            _ => {}
        }
        if !existing.is_empty() {
            let backup = target.with_extension(format!("{extension}.bak-{}", today()));
            // `is_file` rather than `exists`: today's copy is skipped because it is
            // already a copy, and anything else standing under that name is not one.
            // A directory there would make `exists` answer "kept" about a copy that
            // was never written.
            if !backup.is_file() && std::fs::write(&backup, existing).is_err() {
                return Landing::NoBackup;
            }
            // **Somebody else's file keeps its own metadata.** The rename behind `atomic_write`
            // discards the target's ACL, creation time and alternate streams on Windows and takes this
            // process's ownership and umask on Unix — which matters most for exactly the file this
            // resolution reaches, one a dotfile manager shares (review R8). `shell_integration`'s
            // writer has made this distinction since T-A: a file that exists is replaced, a file that
            // does not is created.
            if bt_persist::atomic_replace_preserving(target, bytes).is_err() {
                return Landing::NotWritten;
            }
            return Landing::Landed;
        }
        if bt_persist::atomic_write(target, bytes).is_err() {
            return Landing::NotWritten;
        }
        Landing::Landed
    }
}

/// `YYYYMMDD` for the backup's name, from the wall clock and nothing else.
///
/// Shared with [`attention_codex`](crate::attention_codex), which keeps a copy of the user's own
/// `config.toml` beside it under the same rule: one installer's backup and another's are the same
/// promise about the same day, and two civil-from-days implementations would be two answers to it.
pub(crate) fn today() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let days = seconds / 86_400;
    // Civil-from-days, the standard shift-to-March algorithm. Written out rather than reached for,
    // because this workspace has no date crate and a backup's name is not a reason to add one.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}{month:02}{day:02}")
}

/// **Somebody else's JSON, read as the text it is and edited only where Folio's own entries
/// stand** (0.4.10 plan row 67a; the owner's hard rule of 2026-11-04).
///
/// The rule is that Folio touches only what it can prove is its own, and leaves every other byte of
/// the agent's settings file as it found it: the person's other hooks, every other key, their order,
/// the indentation, the line endings, a byte-order mark, the trailing newline, the way a number or
/// an escape is spelled. A value model cannot keep that promise — `serde_json` without
/// `preserve_order` hands every object back sorted, and re-serialising a document is a rewrite of
/// all of it — so this module reads the file as **spans**: where every member and every element
/// starts and ends in the text. Folio's entries are then replaced, deleted or appended by splicing
/// those spans, and the bytes between them are copied, never regenerated.
///
/// The reader is strict JSON (RFC 8259) and nothing more: no comments, no trailing commas, no
/// single quotes. A document it cannot read is a document Folio does not write (`Malformed`),
/// which is the same answer `serde_json` gave it before. What it accepts and the value parser does
/// not (a byte-order mark) is read past and written back.
///
/// What is done with the spans is `edited`'s, beside this module, and what is checked before
/// anything is written is `verified`'s.
mod splice {
    use serde_json::Value;

    /// The byte-order mark an editor on Windows may leave at the front of the file.
    pub(super) const BOM: char = '\u{feff}';

    /// How deep this reader follows nesting before it calls a document unreadable: at least as
    /// deep as the value parser does (`serde_json` stops at its 128th nested container, this
    /// reader past element depth 128). A document only this reader accepts is refused by
    /// `Document::whole_value` and called unreadable, so nothing is ever written from a reading
    /// only one of the two made.
    const DEPTH: usize = 128;

    /// The document's text after its byte-order mark, if it has one: what a value parser reads.
    #[must_use]
    pub(super) fn body(text: &str) -> &str {
        text.strip_prefix(BOM).unwrap_or(text)
    }

    /// A document this reader cannot read as strict JSON.
    #[derive(Debug, Eq, PartialEq)]
    pub(super) struct Malformed;

    /// An object that names the same key twice, at a place Folio has to look into.
    #[derive(Debug, Eq, PartialEq)]
    pub(super) struct Twice;

    /// One value of the document and the bytes it occupies, `start..end`.
    #[derive(Debug)]
    pub(super) struct Node {
        pub(super) start: usize,
        pub(super) end: usize,
        pub(super) kind: Kind,
    }

    /// What a value is, as far as an edit needs to know.
    #[derive(Debug)]
    pub(super) enum Kind {
        Object(Vec<Property>),
        Array(Vec<Node>),
        Scalar,
    }

    /// One member of an object: its key as decoded, the key's own bytes, and its value.
    #[derive(Debug)]
    pub(super) struct Property {
        pub(super) key: String,
        pub(super) key_start: usize,
        pub(super) key_end: usize,
        pub(super) value: Node,
    }

    impl Property {
        /// The member's bytes, from its key's opening quote to the end of its value.
        #[must_use]
        pub(super) fn span(&self) -> (usize, usize) {
            (self.key_start, self.value.end)
        }
    }

    impl Node {
        #[must_use]
        pub(super) fn members(&self) -> Option<&[Property]> {
            match &self.kind {
                Kind::Object(members) => Some(members),
                _ => None,
            }
        }

        #[must_use]
        pub(super) fn elements(&self) -> Option<&[Node]> {
            match &self.kind {
                Kind::Array(elements) => Some(elements),
                _ => None,
            }
        }

        /// The one member of this object named `key`, or none; [`Twice`] when it is named twice,
        /// because then which of the two the agent reads is the agent's parser's business and not
        /// something an edit may guess at.
        pub(super) fn member(&self, key: &str) -> Result<Option<&Property>, Twice> {
            let mut found = None;
            for member in self.members().unwrap_or_default() {
                if member.key == key {
                    if found.is_some() {
                        return Err(Twice);
                    }
                    found = Some(member);
                }
            }
            Ok(found)
        }

        /// Whether this object names any key twice.
        #[must_use]
        pub(super) fn names_a_key_twice(&self) -> bool {
            let members = self.members().unwrap_or_default();
            members.iter().enumerate().any(|(index, member)| {
                members[..index]
                    .iter()
                    .any(|earlier| earlier.key == member.key)
            })
        }
    }

    /// One file's text and the spans read out of it.
    pub(super) struct Document<'a> {
        pub(super) text: &'a str,
        /// Where the JSON begins: after the byte-order mark when there is one.
        pub(super) body: usize,
        /// The top-level value; none when the file holds nothing but whitespace.
        pub(super) root: Option<Node>,
    }

    impl Document<'_> {
        /// The document as a value — the classifiers' view of the same bytes.
        pub(super) fn whole_value(&self) -> Result<Value, Malformed> {
            match &self.root {
                None => Ok(Value::Object(serde_json::Map::new())),
                Some(_) => serde_json::from_str(&self.text[self.body..]).map_err(|_| Malformed),
            }
        }
    }

    /// Parse `text` into spans, or refuse it whole.
    pub(super) fn parse(text: &str) -> Result<Document<'_>, Malformed> {
        let body = if text.starts_with(BOM) {
            BOM.len_utf8()
        } else {
            0
        };
        let mut reader = Reader {
            text,
            bytes: text.as_bytes(),
            at: body,
        };
        reader.skip_whitespace();
        if reader.at == text.len() {
            return Ok(Document {
                text,
                body,
                root: None,
            });
        }
        let root = reader.json_element(0)?;
        reader.skip_whitespace();
        if reader.at != text.len() {
            return Err(Malformed);
        }
        Ok(Document {
            text,
            body,
            root: Some(root),
        })
    }

    struct Reader<'a> {
        text: &'a str,
        bytes: &'a [u8],
        at: usize,
    }

    impl Reader<'_> {
        fn peek(&self) -> Option<u8> {
            self.bytes.get(self.at).copied()
        }

        fn skip_whitespace(&mut self) {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.at += 1;
            }
        }

        fn require_byte(&mut self, byte: u8) -> Result<(), Malformed> {
            if self.peek() == Some(byte) {
                self.at += 1;
                Ok(())
            } else {
                Err(Malformed)
            }
        }

        fn json_element(&mut self, depth: usize) -> Result<Node, Malformed> {
            if depth > DEPTH {
                return Err(Malformed);
            }
            let start = self.at;
            let kind = match self.peek() {
                Some(b'{') => self.object(depth)?,
                Some(b'[') => self.array(depth)?,
                Some(b'"') => {
                    self.string()?;
                    Kind::Scalar
                }
                Some(b't') => self.literal("true")?,
                Some(b'f') => self.literal("false")?,
                Some(b'n') => self.literal("null")?,
                Some(b'-' | b'0'..=b'9') => {
                    self.number()?;
                    Kind::Scalar
                }
                _ => return Err(Malformed),
            };
            Ok(Node {
                start,
                end: self.at,
                kind,
            })
        }

        fn object(&mut self, depth: usize) -> Result<Kind, Malformed> {
            self.require_byte(b'{')?;
            self.skip_whitespace();
            let mut members = Vec::new();
            if self.peek() == Some(b'}') {
                self.at += 1;
                return Ok(Kind::Object(members));
            }
            loop {
                let key_start = self.at;
                if self.peek() != Some(b'"') {
                    return Err(Malformed);
                }
                let key = self.string()?;
                let key_end = self.at;
                self.skip_whitespace();
                self.require_byte(b':')?;
                self.skip_whitespace();
                let value = self.json_element(depth + 1)?;
                members.push(Property {
                    key,
                    key_start,
                    key_end,
                    value,
                });
                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.at += 1;
                        self.skip_whitespace();
                    }
                    Some(b'}') => {
                        self.at += 1;
                        return Ok(Kind::Object(members));
                    }
                    _ => return Err(Malformed),
                }
            }
        }

        fn array(&mut self, depth: usize) -> Result<Kind, Malformed> {
            self.require_byte(b'[')?;
            self.skip_whitespace();
            let mut elements = Vec::new();
            if self.peek() == Some(b']') {
                self.at += 1;
                return Ok(Kind::Array(elements));
            }
            loop {
                elements.push(self.json_element(depth + 1)?);
                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.at += 1;
                        self.skip_whitespace();
                    }
                    Some(b']') => {
                        self.at += 1;
                        return Ok(Kind::Array(elements));
                    }
                    _ => return Err(Malformed),
                }
            }
        }

        fn literal(&mut self, word: &str) -> Result<Kind, Malformed> {
            if self.text[self.at..].starts_with(word) {
                self.at += word.len();
                Ok(Kind::Scalar)
            } else {
                Err(Malformed)
            }
        }

        fn digits(&mut self) -> usize {
            let from = self.at;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            self.at - from
        }

        /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`
        fn number(&mut self) -> Result<(), Malformed> {
            if self.peek() == Some(b'-') {
                self.at += 1;
            }
            match self.peek() {
                Some(b'0') => self.at += 1,
                Some(b'1'..=b'9') => {
                    self.digits();
                }
                _ => return Err(Malformed),
            }
            if self.peek() == Some(b'.') {
                self.at += 1;
                if self.digits() == 0 {
                    return Err(Malformed);
                }
            }
            if matches!(self.peek(), Some(b'e' | b'E')) {
                self.at += 1;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.at += 1;
                }
                if self.digits() == 0 {
                    return Err(Malformed);
                }
            }
            Ok(())
        }

        fn hex4(&mut self) -> Result<u32, Malformed> {
            let digits = self.text.get(self.at..self.at + 4).ok_or(Malformed)?;
            if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Malformed);
            }
            self.at += 4;
            u32::from_str_radix(digits, 16).map_err(|_| Malformed)
        }

        /// A string, decoded: the caller compares keys as the agent's parser would read them.
        fn string(&mut self) -> Result<String, Malformed> {
            self.require_byte(b'"')?;
            let mut decoded = String::new();
            loop {
                let character = self.text[self.at..].chars().next().ok_or(Malformed)?;
                match character {
                    '"' => {
                        self.at += 1;
                        return Ok(decoded);
                    }
                    '\\' => {
                        self.at += 1;
                        let escape = self.peek().ok_or(Malformed)?;
                        self.at += 1;
                        let unescaped = match escape {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => self.escaped_character()?,
                            _ => return Err(Malformed),
                        };
                        decoded.push(unescaped);
                    }
                    control if u32::from(control) < 0x20 => return Err(Malformed),
                    other => {
                        decoded.push(other);
                        self.at += other.len_utf8();
                    }
                }
            }
        }

        /// The character a `\u` escape names, a surrogate pair read whole. A lone surrogate is
        /// refused, as the value parser refuses it.
        fn escaped_character(&mut self) -> Result<char, Malformed> {
            let unit = self.hex4()?;
            let code = match unit {
                0xD800..=0xDBFF => {
                    self.require_byte(b'\\')?;
                    self.require_byte(b'u')?;
                    let low = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(Malformed);
                    }
                    0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                }
                0xDC00..=0xDFFF => return Err(Malformed),
                _ => unit,
            };
            char::from_u32(code).ok_or(Malformed)
        }
    }

    /// One replacement of `start..end` by `text`; an insertion where the two are equal.
    #[derive(Debug)]
    pub(super) struct Edit {
        pub(super) start: usize,
        pub(super) end: usize,
        pub(super) text: String,
    }

    /// `text` with `edits` made and every other byte copied. `None` when two edits overlap — a
    /// plan this module made wrongly, which writes nothing rather than something.
    #[must_use]
    pub(super) fn spliced(text: &str, mut edits: Vec<Edit>) -> Option<String> {
        edits.sort_by_key(|edit| (edit.start, edit.end));
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for edit in edits {
            if edit.start < at || edit.end < edit.start || edit.end > text.len() {
                return None;
            }
            out.push_str(&text[at..edit.start]);
            out.push_str(&edit.text);
            at = edit.end;
        }
        out.push_str(&text[at..]);
        Some(out)
    }

    /// **How the person's file is laid out**, read off the file, so that what Folio adds to it is
    /// laid out the same way: the line ending, one level of indentation, and the spelling between a
    /// key and its value.
    pub(super) struct Style {
        pub(super) newline: &'static str,
        pub(super) unit: String,
        pub(super) colon: String,
    }

    impl Style {
        /// Two spaces, `\n` and `": "` where the file says nothing — a new or empty file.
        #[must_use]
        pub(super) fn of(document: &Document) -> Self {
            let text = document.text;
            let newline = match text.find('\n') {
                Some(at) if at > 0 && text.as_bytes()[at - 1] == b'\r' => "\r\n",
                _ => "\n",
            };
            let root = document.root.as_ref();
            let first = root.and_then(Node::members).and_then(<[Property]>::first);
            let unit = root
                .zip(first)
                .filter(|(root, first)| text[root.start..first.key_start].contains('\n'))
                .map(|(_, first)| line_indent(text, first.key_start))
                .filter(|unit| !unit.is_empty())
                .unwrap_or("  ")
                .to_owned();
            let colon = first.map_or_else(
                || ": ".to_owned(),
                |first| text[first.key_end..first.value.start].to_owned(),
            );
            Self {
                newline,
                unit,
                colon,
            }
        }
    }

    /// The spaces and tabs that begin the line `at` stands on.
    #[must_use]
    pub(super) fn line_indent(text: &str, at: usize) -> &str {
        let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
        let line = &text[start..at];
        &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
    }

    /// `value` as text that begins where it is put and whose later lines stand at `indent` — over
    /// several lines when `pretty`, on one when not. Only ever used for Folio's own entries.
    #[must_use]
    pub(super) fn render(value: &Value, indent: &str, style: &Style, pretty: bool) -> String {
        let mut out = String::new();
        if pretty {
            write_pretty(&mut out, value, indent, style);
        } else {
            out.push_str(&value.to_string());
        }
        out
    }

    fn write_pretty(out: &mut String, value: &Value, indent: &str, style: &Style) {
        let inner = format!("{indent}{}", style.unit);
        match value {
            Value::Object(members) if !members.is_empty() => {
                out.push('{');
                for (index, (key, item)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(style.newline);
                    out.push_str(&inner);
                    out.push_str(&Value::from(key.as_str()).to_string());
                    out.push_str(&style.colon);
                    write_pretty(out, item, &inner, style);
                }
                out.push_str(style.newline);
                out.push_str(indent);
                out.push('}');
            }
            Value::Array(items) if !items.is_empty() => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(style.newline);
                    out.push_str(&inner);
                    write_pretty(out, item, &inner, style);
                }
                out.push_str(style.newline);
                out.push_str(indent);
                out.push(']');
            }
            scalar => out.push_str(&scalar.to_string()),
        }
    }

    /// Where an item appended to a container goes: the indentation its lines stand at, and whether
    /// it is laid out over several lines like the items already there.
    pub(super) struct Layout {
        pub(super) indent: String,
        pub(super) pretty: bool,
    }

    /// **One object's members or one array's elements, as spans**, and the edits that take some of
    /// them out and put new ones after them.
    pub(super) struct Container<'a> {
        text: &'a str,
        open: usize,
        close: usize,
        items: Vec<(usize, usize)>,
    }

    impl<'a> Container<'a> {
        /// `None` for a value that holds nothing.
        #[must_use]
        pub(super) fn of(text: &'a str, node: &Node) -> Option<Self> {
            let items = match &node.kind {
                Kind::Object(members) => members.iter().map(Property::span).collect(),
                Kind::Array(elements) => {
                    elements.iter().map(|node| (node.start, node.end)).collect()
                }
                Kind::Scalar => return None,
            };
            Some(Self {
                text,
                open: node.start,
                close: node.end - 1,
                items,
            })
        }

        /// The whitespace before item `index`: after the bracket for the first, after the comma
        /// that precedes it for every other.
        fn gap(&self, index: usize) -> &'a str {
            let from = match index
                .checked_sub(1)
                .and_then(|before| self.items.get(before))
            {
                None => self.open + 1,
                Some(&(_, end)) => self.text[end..self.items[index].0]
                    .find(',')
                    .map_or(end, |comma| end + comma + 1),
            };
            &self.text[from..self.items[index].0]
        }

        /// How an appended item is laid out: as the last item there is, or — in an empty
        /// container — one level in from the line the container opens on.
        #[must_use]
        pub(super) fn layout(&self, style: &Style, document_pretty: bool) -> Layout {
            match self.items.last() {
                Some(&(start, _)) => Layout {
                    indent: line_indent(self.text, start).to_owned(),
                    pretty: self.gap(self.items.len() - 1).contains('\n'),
                },
                None => Layout {
                    indent: format!("{}{}", line_indent(self.text, self.open), style.unit),
                    pretty: document_pretty,
                },
            }
        }

        /// **The edits that leave every item not `deleted` exactly where and as it is, followed by
        /// `appended`.**
        ///
        /// A run of deleted items takes its own separators with it and nothing else: a run with a
        /// survivor after it goes up to that survivor's first byte, so the whitespace before the run
        /// now stands before the survivor; a run at the end goes from the last survivor's last byte,
        /// so what stood before the closing bracket still does. Appended items go after the last
        /// item, each behind the comma and the whitespace the last item stood behind. A container
        /// all of whose items go, with nothing appended, is left empty — `{}` or `[]`.
        #[must_use]
        pub(super) fn edits(
            &self,
            deleted: &[bool],
            appended: &[String],
            style: &Style,
            layout: &Layout,
        ) -> Vec<Edit> {
            let count = self.items.len();
            let mut edits = Vec::new();
            let joint = || -> String {
                match count {
                    0 if layout.pretty => format!("{}{}", style.newline, layout.indent),
                    0 => String::new(),
                    _ => self.gap(count - 1).to_owned(),
                }
            };
            if count > 0 && deleted.iter().all(|gone| *gone) {
                let edit = if appended.is_empty() {
                    Edit {
                        start: self.open + 1,
                        end: self.close,
                        text: String::new(),
                    }
                } else {
                    Edit {
                        start: self.items[0].0,
                        end: self.items[count - 1].1,
                        text: appended.join(&format!(",{}", joint())),
                    }
                };
                edits.push(edit);
                return edits;
            }
            let mut index = 0;
            while index < count {
                if !deleted[index] {
                    index += 1;
                    continue;
                }
                let mut last = index;
                while last + 1 < count && deleted[last + 1] {
                    last += 1;
                }
                let (start, end) = if last + 1 < count {
                    (self.items[index].0, self.items[last + 1].0)
                } else {
                    // A run at the end has a survivor before it: the case where none survives
                    // returned above.
                    (self.items[index - 1].1, self.items[last].1)
                };
                edits.push(Edit {
                    start,
                    end,
                    text: String::new(),
                });
                index = last + 1;
            }
            if appended.is_empty() {
                return edits;
            }
            let mut inserted = String::new();
            if count == 0 {
                for (position, item) in appended.iter().enumerate() {
                    if position > 0 {
                        inserted.push(',');
                    }
                    inserted.push_str(&joint());
                    inserted.push_str(item);
                }
                // `{}` gains its closing line; whatever stood between the brackets stays.
                if layout.pretty && self.close == self.open + 1 {
                    inserted.push_str(style.newline);
                    inserted.push_str(line_indent(self.text, self.open));
                }
                edits.push(Edit {
                    start: self.open + 1,
                    end: self.open + 1,
                    text: inserted,
                });
            } else {
                let joint = joint();
                for item in appended {
                    inserted.push(',');
                    inserted.push_str(&joint);
                    inserted.push_str(item);
                }
                let at = self.items[count - 1].1;
                edits.push(Edit {
                    start: at,
                    end: at,
                    text: inserted,
                });
            }
            edits
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_source::{Index, Needle, Pattern, Scope, Search, View};

    /// RED (T-FRESH-FACTS) — **an older walk's reading of the agent folders never replaces a
    /// newer walk's**, and a walk that read the same thing moves nothing.
    ///
    /// MUTATION (observed red): `AgentHomesCell::adopt` without its generation refusal — walk 3,
    /// answering after walk 5, puts the old folder back.
    #[test]
    fn an_older_walks_agent_folders_never_overwrite_a_newer_walks() {
        let cell = AgentHomesCell::new();
        let reading = |folder: &'static str| {
            AgentHomes::in_environment(&move |name| {
                (name == CONFIG_DIR_VARIABLE).then(|| folder.into())
            })
        };
        cell.adopt(5, reading(r"D:\新的\claude"));
        assert!(
            !cell.adopt(3, reading(r"D:\旧的\claude")),
            "walk 3 answered after walk 5 and is refused"
        );
        assert_eq!(
            cell.variable(CONFIG_DIR_VARIABLE),
            Some(OsString::from(r"D:\新的\claude"))
        );
        assert!(
            !cell.adopt(6, reading(r"D:\新的\claude")),
            "a walk that read the same folder moves no reader"
        );
        assert!(
            cell.adopt(7, reading(r"E:\claude 配置")),
            "and one that read another folder does"
        );
    }

    /// RED (0.4.10 plan row 68c, the review's N1) — **the folders are settled by a walk's answer,
    /// or by the lane's word that no walk will answer, and by nothing else**: a cell no walk has
    /// answered is not settled; settling it on the launch's reading settles it once; a walk that
    /// answers afterwards is still the first walk's answer; and a cell a walk answered is settled
    /// and is not settled again.
    ///
    /// MUTATION: `homes_settled` reads the walk's answer alone
    /// (`plant-68c-give-up-not-settled.py`).
    #[test]
    fn the_agent_folders_settle_on_a_walk_or_on_no_walk_to_answer() {
        let cell = AgentHomesCell::new();
        assert!(!cell.homes_settled(), "nothing has answered");
        assert!(cell.settle_on_the_launch_homes(), "no walk will answer");
        assert!(cell.homes_settled() && !cell.answered());
        assert!(!cell.settle_on_the_launch_homes(), "settled once");
        cell.adopt(1, AgentHomes::default());
        assert!(cell.answered() && cell.homes_settled());

        let walked = AgentHomesCell::new();
        walked.adopt(1, AgentHomes::default());
        assert!(walked.homes_settled());
        assert!(
            !walked.settle_on_the_launch_homes(),
            "a walk answered: the launch's reading settles nothing"
        );
    }
    use crate::attention::{MappedAction, Tier, WaitKind};
    use crate::test_support::{host_path, host_spelling};

    #[test]
    fn attention_two_live_copies_require_takeover_and_preserve_bytes() {
        // The test's own folders, removed when it lets go of them (T-TEST-ORDER-STRAYS): the
        // root, and the marks folder `apply_to` names after it.
        let root = crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(concat!("hooks-", "two-copies"))),
        );
        let marks = crate::test_support::attention_marks_folder("hooks", &root);
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
        let path = root.join("settings.json");
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

    fn apply_to(path: &Path, install: bool, exe: &Path) -> Outcome {
        apply_at(
            path,
            install.into(),
            exe,
            &crate::test_support::attention_marks_folder("hooks", path),
        )
    }

    fn exe() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-fixtures/Program Files/Folio/folio.exe")
    }

    fn installed() -> Value {
        let mut settings = Value::Object(Map::new());
        assert!(install_into(&mut settings, &exe()));
        settings
    }

    /// **Only the user's own file, and never a repository's.**
    ///
    /// The red form of §10.6 clause 5. There is exactly one path this module can produce, it comes
    /// from the variable Claude Code itself reads, and no working directory reaches it.
    #[test]
    fn the_only_file_this_writes_is_the_one_the_environment_names() {
        let named = |text: &str| Some(OsString::from(text));
        // The variable wins, which is what makes an isolated run isolated — and what lets this be
        // verified on a real machine without touching the user's own installation.
        assert_eq!(
            config_dir_from(
                named(&host_spelling(r"D:\scratch\claude-home")),
                named(&host_spelling(r"C:\Users\someone"))
            ),
            Some(host_path(r"D:\scratch\claude-home"))
        );
        // Set-but-empty is not set. A shell that wrote `CLAUDE_CONFIG_DIR=` said nothing, and
        // taking it literally would put this file at the root of the current drive.
        assert_eq!(
            config_dir_from(named(""), named(&host_spelling(r"C:\Users\someone"))),
            Some(host_path(r"C:\Users\someone").join(DEFAULT_DIRECTORY))
        );
        // With nothing to go on, nothing is written — and never a relative path, which is what a
        // bare directory name would be, and which *would* land in a working directory.
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, named("")), None);
        assert!(settings_path().is_none_or(|path| path.ends_with(SETTINGS_FILE)));
        // **Nothing in this module can be steered by where the process happens to be standing**,
        // asserted over the source itself because that is the only form the rule has: there is no
        // value to check, only an absence to keep.
        //
        // The names are spelled in halves so that this assertion is not its own counter-example —
        // `concat!` puts them back together at compile time, and what `include_str!` reads is the
        // source, which never contains one whole.
        for reaching in ["current_dir", "project_dir"] {
            let found = crate::test_support::found_in(
                Needle::new(Pattern::identifier(reaching)),
                View::Identifiers,
                Scope::Module("crate::attention_hooks".to_owned()),
            )
            .in_the_product(crate::test_support::source());
            assert!(
                found.is_empty(),
                "`{reaching}` would let a working directory decide where automation is written"
            );
        }
        // And the other half of the same rule, said as a property rather than as an absence: given
        // an absolute answer the path is absolute, so nothing this module writes can ever be
        // resolved against wherever the process happens to be standing.
        assert!(
            config_dir_from(named(&host_spelling(r"D:\scratch\claude-home")), None)
                .is_some_and(|path| path.is_absolute())
        );
        assert!(
            config_dir_from(None, named(&host_spelling(r"C:\Users\someone")))
                .is_some_and(|path| path.is_absolute())
        );
    }

    /// **The consent disclosure names the file this machine will write.**
    ///
    /// The first-run card's Claude Code row says which file it is about to copy and then write,
    /// and that sentence is the one thing on the card that has to be true. Both halves are pinned
    /// here: the default spelling on a machine that has moved nothing — byte for byte the string
    /// the card has always drawn — and the real path on a machine that has set
    /// `CLAUDE_CONFIG_DIR`, where the old literal named a file nothing was going to touch.
    #[test]
    fn the_tip_names_the_default_spelling_until_the_variable_moves_it() {
        let named = |text: &str| Some(OsString::from(text));
        assert_eq!(
            settings_path_shown_from(None, named(&host_spelling(r"C:\Users\someone"))),
            "~/.claude/settings.json"
        );
        // Set-but-empty is not set, `config_dir_from`'s rule, so the spelling does not move.
        assert_eq!(
            settings_path_shown_from(named(""), named(&host_spelling(r"C:\Users\someone"))),
            "~/.claude/settings.json"
        );
        // …and with nothing to go on at all, the default spelling is still the honest answer:
        // there is no path to offer instead.
        assert_eq!(
            settings_path_shown_from(None, None),
            "~/.claude/settings.json"
        );
        assert_eq!(
            settings_path_shown_from(
                named(&host_spelling(r"D:\scratch\claude-home")),
                named(&host_spelling(r"C:\Users\someone"))
            ),
            host_path(r"D:\scratch\claude-home")
                .join(SETTINGS_FILE)
                .display()
                .to_string()
        );
    }

    /// **One layer per kind**, in the thing that is actually written.
    #[test]
    fn what_is_written_never_carries_two_layers_of_one_request() {
        let rows = rows_to_install();
        assert_eq!(
            crate::attention::duplicated_tier(&rows),
            None,
            "installing both layers of a kind turns one request into two credentials"
        );
        for row in rows.iter().filter(|row| row.is_wait()) {
            assert!(
                matches!(
                    row.action,
                    MappedAction::Wait {
                        tier: Tier::Primary
                    }
                ),
                "{} is a fallback, and the fallback is only for a build that cannot have the \
                 primary — which is a question with no interface to ask it today",
                row.event
            );
        }
        // And the file agrees with the table, read back.
        let declared = rows_declared_by(&installed());
        assert_eq!(declared, rows);
        assert_eq!(
            crate::attention::kind_mode(&declared, CLAUDE_CODE, WaitKind::Permission),
            crate::attention::Mode::Level
        );
    }

    /// Every hook written is asynchronous, and the reason is a ten-minute decision gate.
    #[test]
    fn no_hook_this_writes_can_hold_up_an_approval() {
        let settings = installed();
        let hooks = settings["hooks"].as_object().expect("hooks");
        let mut seen = 0;
        for groups in hooks.values() {
            for group in groups.as_array().expect("groups") {
                for hook in group["hooks"].as_array().expect("hooks") {
                    seen += 1;
                    assert_eq!(
                        hook.get("async"),
                        Some(&Value::Bool(true)),
                        "a synchronous signal hook sits between the user and every approval: \
                         {hook}"
                    );
                    assert_eq!(hook.get("type"), Some(&Value::String("command".to_owned())));
                }
            }
        }
        assert_eq!(seen, rows_to_install().len());
    }

    /// A matcher-qualified event is registered under its event with its matcher, not under a name
    /// that contains a full stop.
    #[test]
    fn a_notification_subtype_is_registered_as_a_matcher() {
        let settings = installed();
        let hooks = settings["hooks"].as_object().expect("hooks");
        assert!(
            !hooks.keys().any(|key| key.contains('.')),
            "`Notification.permission_prompt` is a Folio spelling, not an upstream event name"
        );
        let notification = hooks["Notification"].as_array().expect("array");
        let matchers = notification
            .iter()
            .filter_map(|group| group.get("matcher").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            matchers,
            [
                "agent_needs_input",
                "quota_auto_resume_stale",
                "elicitation_complete",
                "elicitation_response",
                "agent_completed",
                "quota_auto_resume_fired",
                "quota_auto_resume_disabled",
            ]
        );
        assert!(
            !matchers.contains(&"idle_prompt"),
            "`idle_prompt` is the upstream spelling of `output has been quiet for N seconds`, and \
             a ruling already refused that as evidence of waiting"
        );
    }

    /// **Install then remove is the identity**, on a file with the user's own hooks in it.
    #[test]
    fn taking_them_back_out_leaves_what_was_there() {
        let original: Value = serde_json::from_str(
            r#"{
                "model": "opus",
                "hooks": {
                    "Stop": [
                        { "hooks": [ { "type": "command", "command": "say done" } ] }
                    ],
                    "PreToolUse": [
                        { "matcher": "Bash", "hooks": [ { "type": "command", "command": "lint" } ] }
                    ]
                }
            }"#,
        )
        .expect("fixture");
        let mut settings = original.clone();
        assert!(install_into(&mut settings, &exe()));
        assert_eq!(state_of(&settings), State::Installed);
        // The user's own entries are still there, beside ours.
        let stop = settings["hooks"]["Stop"].as_array().expect("array");
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], Value::from("say done"));
        // **And ours is the one that asks for a payload**, which is what makes the removal below
        // an assertion about the *new* command line rather than about the one this test was
        // written for: an entry recognised by the mark is recognised whatever follows it, and this
        // is the entry that has something following it.
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            exe().to_string_lossy().as_ref()
        );
        assert_eq!(
            stop[1]["hooks"][0]["args"],
            serde_json::json!(["attention", "claude-code:Stop", "--json", "-"])
        );
        assert!(remove_from(&mut settings, &exe()));
        assert_eq!(
            settings, original,
            "uninstalling must hand back the file that was there, byte for byte in structure"
        );
    }

    /// A second press writes nothing.
    #[test]
    fn installing_twice_changes_nothing_the_second_time() {
        let mut settings = installed();
        let after_first = settings.clone();
        assert!(
            !install_into(&mut settings, &exe()),
            "an install that was already done is not a change"
        );
        assert_eq!(settings, after_first);
        assert!(remove_from(&mut settings, &exe()));
        assert!(
            !remove_from(&mut settings, &exe()),
            "and neither is a removal of what is not there"
        );
    }

    /// Moving the executable is not a reason to lose track of the entries.
    #[test]
    fn a_dead_owner_can_be_replaced_after_moving_folio() {
        let mut settings = installed();
        let moved = exe().parent().unwrap().join("portable/folio.exe");
        assert!(
            install_into(&mut settings, &moved),
            "a rewrite from a new location is a change"
        );
        let commands = settings["hooks"]["PermissionRequest"]
            .as_array()
            .expect("array");
        assert_eq!(commands.len(), 1, "the old entry was replaced, not doubled");
        assert!(
            commands[0]["hooks"][0]["command"]
                .as_str()
                .expect("command")
                .contains("portable"),
        );
    }

    /// A file this build cannot read is left alone.
    #[test]
    fn an_unreadable_settings_file_is_never_written_over() {
        assert_eq!(
            state_of(&serde_json::from_str::<Value>("[1,2,3]").expect("array")),
            State::Absent,
            "a non-object is not an installation; the refusal to write is `apply`'s"
        );
        let mut array: Value = serde_json::from_str("[1,2,3]").expect("array");
        assert!(!remove_from(&mut array, &exe()));
    }

    /// The command carries the family, so a bare `Stop` is never ambiguous.
    ///
    /// **And it asks for a payload exactly where the table says there is something in one.** The
    /// bar is the mapping table's own column rather than a list written here, so a row that gains
    /// or loses a [`attention_map::Words`] source is a row whose command line follows it without
    /// anybody remembering to come back — and a row that declares nothing gets the command line it
    /// always had, which is the property this half is really about: nothing is spawned holding a
    /// handle it has no reason to read.
    #[test]
    fn every_command_says_which_upstream_it_speaks_for() {
        for row in rows_to_install() {
            let args = args_for(row.event);
            assert_eq!(args[0], "attention");
            assert_eq!(args[1], format!("{CLAUDE_CODE}:{}", row.event));
            let wants_payload = attention_map::turn_end_row(CLAUDE_CODE, row.event)
                .is_some_and(|end| end.words.are_somewhere());
            assert_eq!(args.len() == 4, wants_payload);
            if wants_payload {
                assert_eq!(args[2..], ["--json", "-"]);
            }
        }
    }

    /// **The block this writes into somebody's own file, spelled out.**
    ///
    /// Every other test here asserts a property — one tier per kind, every hook asynchronous, a
    /// matcher where a matcher belongs. This one asserts the **bytes**, and it is worth having for
    /// the reason a golden file usually is not: what is being written is *not ours*. It lands in a
    /// document the user owns, that another program reads, and that nobody will look at again. A
    /// change to any of it — an event renamed, a matcher dropped, `async` lost in a refactor — is a
    /// change to something out in the world, and it should have to be typed here on purpose.
    ///
    /// It is also the text a real-machine run hands to `claude --settings`, so the thing exercised
    /// against the real hook runner is the thing this build actually installs.
    #[test]
    fn the_block_this_installs_is_this() {
        let mut settings = Value::Object(Map::new());
        assert!(install_into_on(
            &mut settings,
            Path::new(r"C:\folio\folio.exe"),
            HostPlatform::Windows
        ));
        assert_eq!(
            serde_json::to_string_pretty(&settings).expect("render"),
            EXPECTED_BLOCK.trim_end()
        );
    }

    /// Direct execution is platform independent and never shell-quotes the operand.
    #[test]
    fn the_block_a_mac_installs_is_the_same_direct_exec_block() {
        let exe = Path::new("/Applications/Folio.app/Contents/MacOS/folio");
        let mut windows = Value::Object(Map::new());
        let mut mac = Value::Object(Map::new());
        assert!(install_into_on(&mut windows, exe, HostPlatform::Windows));
        assert!(install_into_on(&mut mac, exe, HostPlatform::MacOs));
        assert_eq!(
            windows["hooks"]
                .as_object()
                .expect("hooks")
                .keys()
                .collect::<Vec<_>>(),
            mac["hooks"]
                .as_object()
                .expect("hooks")
                .keys()
                .collect::<Vec<_>>(),
            "the two machines install the same events"
        );
        assert_eq!(
            windows, mac,
            "direct exec has identical structure on every platform"
        );
        for command in commands_of(&mac) {
            assert_eq!(command, "/Applications/Folio.app/Contents/MacOS/folio");
        }
    }

    /// Every command line one install wrote, whatever event it hangs on.
    fn commands_of(settings: &Value) -> Vec<String> {
        let mut found = Vec::new();
        for groups in settings["hooks"].as_object().expect("hooks").values() {
            for group in groups.as_array().expect("array") {
                for hook in group["hooks"].as_array().expect("array") {
                    found.push(hook["command"].as_str().expect("a command").to_owned());
                }
            }
        }
        found
    }

    /// See [`the_block_this_installs_is_this`].
    const EXPECTED_BLOCK: &str = include_str!("../../../docs/plans/attention/claude-hooks.json");

    /// The date the backup is named after is a real one.
    #[test]
    fn the_backup_name_is_a_date() {
        let today = today();
        assert_eq!(today.len(), 8, "{today}");
        let year: u32 = today[..4].parse().expect("year");
        let month: u32 = today[4..6].parse().expect("month");
        let day: u32 = today[6..].parse().expect("day");
        assert!((2024..2100).contains(&year), "{today}");
        assert!((1..=12).contains(&month), "{today}");
        assert!((1..=31).contains(&day), "{today}");
    }

    fn state_of(settings: &Value) -> State {
        if declares_folio(&CLAUDE, settings) {
            State::Installed
        } else {
            State::Absent
        }
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

    /// A scratch directory of this test's own. Never anywhere near a real `~/.claude`.
    /// A scratch folder of this test's own, removed with its marks folder when the test lets go.
    /// Held by its canonical spelling ([`crate::test_support::ScratchFolder::create`]): a real
    /// machine's path can be a short name or sit behind a link, and neither is what any of these
    /// tests is about.
    fn scratch(name: &str) -> crate::test_support::ScratchFolder {
        let dir = crate::test_support::ScratchFolder::create(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-tests")
                .join(bt_testpath::unique_name(&format!("folio-land-{name}"))),
        );
        let marks = crate::test_support::attention_marks_folder("hooks", &dir);
        dir.also_removing(marks)
    }

    /// RED — **the three installers write somebody else's configuration file whole or not at all.**
    ///
    /// Release audit 2026-08-27 (Codex 错 12): all three called `std::fs::write` on the target,
    /// which truncates first. A process killed in that window leaves the user's own Claude Code,
    /// Codex or Copilot configuration half a document. The write is now [`land`]'s — a sibling
    /// temporary, `sync_all`, one replace — and this pins that there is no second way out.
    ///
    /// RED GATE: put `std::fs::write(&path, …)` back into any of the three `apply` functions and
    /// this fails naming the file. The count is taken over the production half of each source
    /// only, because a test may write whatever fixture it likes.
    #[test]
    fn a_landing_is_whole_or_it_is_nothing_at_all() {
        let index = Index::of_package("bt-app");
        let found = |module: &str, spelling: &str| {
            index
                .search(
                    &Search::new(Needle::new(Pattern::text(spelling)), View::Raw)
                        .in_scope(Scope::Module(format!("crate::{module}"))),
                )
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index)
        };
        // `land`'s own dated copy is the one raw write left on this path, and it is a write to a
        // file nothing else has ever read — never to the target.
        assert_eq!(
            found("attention_hooks", "fs::write(").len(),
            1,
            "the only raw write left in this module is `land`'s dated copy"
        );
        assert_eq!(
            found("attention_hooks", "bt_persist::atomic_write(").len(),
            1,
            "and the target is replaced through the one atomic writer"
        );
        for name in ["attention_codex", "attention_copilot"] {
            assert!(
                found(name, "fs::write(").is_empty(),
                "{name} writes a configuration file behind `land`'s back"
            );
            assert!(
                !found(name, "config.land(").is_empty(),
                "{name} must land its bytes through the one writer, on the one resolution"
            );
        }
    }

    /// One resolution, the way an operation makes one, for a fixture that only wants to write.
    fn resolved(path: &Path) -> Config {
        Config::resolve(path).unwrap_or_else(|_| panic!("a plain file resolves to itself"))
    }

    /// RED — **a copy that cannot be kept refuses the install.**
    ///
    /// Release audit 2026-08-27 (Codex 错 12): the dated backup's error was dropped on the floor and
    /// the target was overwritten anyway, which made every one of these modules' "a build that could
    /// damage one had better be able to hand it back" conditional on a write nobody checked.
    ///
    /// RED GATE: change [`land`] to ignore the backup's result and the first assertion goes to
    /// `Landed` while the second finds the user's file replaced.
    #[test]
    fn a_copy_that_cannot_be_kept_refuses_to_write() {
        let dir = scratch("nobackup");
        let target = dir.join("settings.json");
        std::fs::write(&target, "{\"model\":\"opus\"}\n").expect("the user's own file");
        let existing = std::fs::read_to_string(&target).expect("read back");
        // A directory standing where the copy would go: the copy cannot be written and cannot be
        // mistaken for one that already exists.
        let blocked = target.with_extension(format!("json.bak-{}", today()));
        std::fs::create_dir(&blocked).expect("a directory in the copy's place");

        assert_eq!(
            resolved(&target).land(&existing, "json", b"{}\n"),
            Landing::NoBackup,
            "a copy that cannot be kept is a refusal"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("still there"),
            existing,
            "and the user's own file is untouched by it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED — what lands is the whole file, under its own name, with nothing left beside it.
    ///
    /// The other half of [`a_landing_is_whole_or_it_is_nothing_at_all`]: the atomic writer leaves a
    /// sibling temporary behind on any path that does not commit, and a reader who opened this
    /// directory would find two files where the user has one.
    #[test]
    fn what_lands_is_the_file_and_nothing_beside_it() {
        let dir = scratch("landed");
        let target = dir.join("settings.json");
        assert_eq!(
            resolved(&target).land("", "json", b"first\n"),
            Landing::Landed
        );
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "first\n");
        assert_eq!(
            names(&dir),
            vec!["settings.json".to_owned()],
            "a first landing leaves one file: no copy of nothing, no temporary"
        );

        // A second landing over a file that was there keeps exactly one dated copy of it.
        assert_eq!(
            resolved(&target).land("first\n", "json", b"second\n"),
            Landing::Landed
        );
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "second\n");
        // Sorted, and `settings.json` sorts before the copy that extends its name.
        let backup = format!("settings.json.bak-{}", today());
        assert_eq!(
            names(&dir),
            vec!["settings.json".to_owned(), backup.clone()]
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(&backup)).expect("the copy"),
            "first\n"
        );

        // And a third keeps the *first* copy rather than a copy of the second, which is the one
        // that is worth having: it is what was there before this build touched anything today.
        assert_eq!(
            resolved(&target).land("second\n", "json", b"third\n"),
            Landing::Landed
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(&backup)).expect("the copy"),
            "first\n",
            "the copy kept is of what was there before the first write of the day"
        );
        assert_eq!(names(&dir), vec!["settings.json".to_owned(), backup]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RED — **a settings file that could not be read is left byte for byte.**
    ///
    /// Release audit 2026-09-16 (C-3, the verifier's second site):
    /// `read_to_string(&path).unwrap_or_default()` took a file this build was not allowed to read —
    /// or one holding a single byte that is not UTF-8 — for a file that was not there. The empty
    /// string it fell back to reads as "no settings at all", so an install wrote a whole new
    /// document over somebody's own, and [`land`] was handed nothing to keep a copy of.
    ///
    /// RED GATE: put the `unwrap_or_default` back and this file comes back as Folio's own.
    #[test]
    fn a_settings_file_that_could_not_be_read_is_never_written_over() {
        let dir = scratch("unreadable");
        let path = dir.join(SETTINGS_FILE);
        // A Latin-1 byte inside a string: an ordinary file on an ordinary machine, and not UTF-8.
        let theirs: &[u8] = b"{\"model\":\"caf\xe9\"}\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        let installing = apply_to(&path, true, &exe());
        assert_eq!(
            installing,
            Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text())
        );
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            names(&dir),
            vec![SETTINGS_FILE.to_owned()],
            "nothing was written beside it either"
        );
        // And the row drawn from it says so, rather than offering to write over it.
        assert_eq!(state_at(&CLAUDE, &path), State::Unreadable);
        // Taking it back out is refused for the same reason: this build cannot tell whose it is.
        let taking_it_out = apply_to(&path, false, &exe());
        assert_eq!(
            taking_it_out,
            Outcome::Refused(crate::i18n::Text::AgentHooksUnreadable.text())
        );
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A file that is not there is the one state in which writing a fresh one loses nothing.**
    #[test]
    fn a_settings_file_that_is_not_there_is_the_one_that_gets_created() {
        let dir = scratch("absent");
        let path = dir.join(SETTINGS_FILE);
        assert_eq!(state_at(&CLAUDE, &path), State::Absent);

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(state_at(&CLAUDE, &path), State::Installed);
        assert_eq!(
            names(&dir),
            vec![SETTINGS_FILE.to_owned()],
            "a first install leaves one file: no copy of nothing, no temporary"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A file that is there is copied before it is changed**, and the copy is what was there.
    #[test]
    fn a_settings_file_that_is_there_is_copied_before_it_is_changed() {
        let dir = scratch("copied");
        let path = dir.join(SETTINGS_FILE);
        let theirs = "{\n  \"model\": \"opus\"\n}\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(written.contains("\"model\""), "{written}");
        assert!(written.contains("claude-code:"), "{written}");
        let backup = format!("{SETTINGS_FILE}.bak-{}", today());
        assert_eq!(names(&dir), vec![SETTINGS_FILE.to_owned(), backup.clone()]);
        assert_eq!(
            std::fs::read_to_string(dir.join(&backup)).expect("the copy"),
            theirs,
            "the copy beside it is the file as it was"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The entry this build writes for `Stop`, on one line, for a copy at `exe`.
    fn own_stop(exe: &Path) -> String {
        serde_json::json!({
            "type": "command",
            "command": exe,
            "args": ["attention", "claude-code:Stop", "--json", "-"],
            "async": true,
        })
        .to_string()
    }

    /// The same entry for a copy that is not there any more.
    fn gone_stop(dir: &Path) -> String {
        own_stop(&dir.join("gone").join("folio.exe"))
    }

    /// **Files somebody wrote, in the shapes people write them**, each with the pieces of it that
    /// are not Folio's, in the order they stand (0.4.10 plan row 67a, the owner's rule of
    /// 2026-11-04). None of them holds a Folio entry, so installing and then removing must hand
    /// back every byte.
    fn their_files() -> Vec<(&'static str, String, Vec<&'static str>)> {
        let two_spaces = r#"{
  "zeta": true,
  "hooks": {
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "say done" }
        ]
      }
    ],
    "Notification": [
      { "matcher": "idle_prompt", "hooks": [ { "type": "command", "command": "beep" } ] }
    ]
  },
  "alpha": 1
}
"#;
        let fragments = vec![
            r#""zeta": true"#,
            r#"{ "type": "command", "command": "say done" }"#,
            r#""matcher": "idle_prompt""#,
            r#"{ "type": "command", "command": "beep" }"#,
            r#""alpha": 1"#,
        ];
        let tabs = two_spaces.replace("  ", "\t");
        let four_spaces_crlf = two_spaces.replace("  ", "    ").replace('\n', "\r\n");
        vec![
            ("two spaces, LF", two_spaces.to_owned(), fragments.clone()),
            ("tabs", tabs, fragments.clone()),
            ("four spaces, CRLF", four_spaces_crlf, fragments),
            (
                "a byte-order mark and no newline at the end",
                "\u{feff}{\"hooks\": {\"Stop\": [{\"hooks\": [{\"type\": \"command\", \
                 \"command\": \"say done\"}]}]}, \"model\": \"opus\"}"
                    .to_owned(),
                vec![
                    r#"{"type": "command", "command": "say done"}"#,
                    r#""model": "opus""#,
                ],
            ),
            (
                "escapes, numbers and a key named twice",
                r#"{"name": "caf\u00e9 \/ tea", "ratio": 1.50, "big": 1e3, "dup": 1, "dup": 2, "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "\u0073ay"}]}]}}
"#
                .to_owned(),
                vec![
                    r#""name": "caf\u00e9 \/ tea""#,
                    r#""ratio": 1.50"#,
                    r#""big": 1e3"#,
                    r#""dup": 1"#,
                    r#""dup": 2"#,
                    r#"{"type": "command", "command": "\u0073ay"}"#,
                ],
            ),
            (
                // Review N6: two lookalikes of Folio's that are not Folio's (an extra key; a
                // command line that is none of Folio's templates), a hook with arrays and objects
                // inside it, and a raw non-ASCII byte.
                "lookalikes, a nested hook and a raw é",
                r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {"type": "command", "command": "folio.exe", "args": ["attention", "claude-code:Stop", "--json", "-"], "async": true, "timeout": 5},
          {"type": "command", "command": "folio attention claude-code:Stop"},
          {"type": "command", "command": "say café"}
        ]
      }
    ],
    "PreToolUse": [
      {"matcher": "Bash", "hooks": [{"type": "command", "command": "lint", "args": ["--x", ["y", {"z": [1, {"w": null}]}]], "env": {"A": {"B": [true]}}}]}
    ]
  }
}
"#
                .to_owned(),
                vec![
                    r#"{"type": "command", "command": "folio.exe", "args": ["attention", "claude-code:Stop", "--json", "-"], "async": true, "timeout": 5}"#,
                    r#"{"type": "command", "command": "folio attention claude-code:Stop"}"#,
                    r#"{"type": "command", "command": "say café"}"#,
                    r#""matcher": "Bash""#,
                    r#"{"type": "command", "command": "lint", "args": ["--x", ["y", {"z": [1, {"w": null}]}]], "env": {"A": {"B": [true]}}}"#,
                ],
            ),
            (
                "no hooks at all",
                "{\n  \"model\": \"opus\"\n}\n".to_owned(),
                vec![r#""model": "opus""#],
            ),
            (
                "one line",
                r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lint"}]}]}}"#
                    .to_owned(),
                vec![
                    r#""matcher":"Bash""#,
                    r#"{"type":"command","command":"lint"}"#,
                ],
            ),
        ]
    }

    /// Whether every one of `fragments` stands in `text`, in this order.
    fn stand_in_order(text: &str, fragments: &[&str]) -> bool {
        let mut from = 0;
        fragments.iter().all(|fragment| {
            text[from..].find(fragment).is_some_and(|at| {
                from += at + fragment.len();
                true
            })
        })
    }

    /// RED (0.4.10 plan row 67a; the owner's rule of 2026-11-04) — **the switch changes only
    /// Folio's own entries, and gives back every byte when it is turned off again.**
    ///
    /// Each of [`their_files`]: installing keeps every piece that is not Folio's, byte for byte and
    /// in order, keeps the byte-order mark and the file's last byte, and the file read back says
    /// the same thing it said with Folio's entries taken out; removing hands back the file that was
    /// there, to the byte.
    ///
    /// RED GATE: `plant-67a-whole-hooks-reserialised.py` (the `hooks` object written back whole)
    /// and `plant-67a-claims-nothing.py` turn the first refusal or the first byte comparison red.
    #[test]
    fn the_switch_changes_only_folios_own_entries() {
        for (name, theirs, fragments) in their_files() {
            let dir = scratch("own-bytes");
            let path = dir.join(SETTINGS_FILE);
            std::fs::write(&path, &theirs).expect("their own file");

            assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed, "{name}");
            let written = std::fs::read_to_string(&path).expect("read back");
            assert_eq!(state_at(&CLAUDE, &path), State::Installed, "{name}");
            assert!(stand_in_order(&written, &fragments), "{name}: {written}");
            assert_eq!(
                written.starts_with('\u{feff}'),
                theirs.starts_with('\u{feff}'),
                "{name}"
            );
            assert_eq!(written.ends_with('\n'), theirs.ends_with('\n'), "{name}");
            assert_eq!(
                written.contains("\r\n"),
                theirs.contains("\r\n"),
                "{name}: the line endings are the file's"
            );
            let value = |text: &str| {
                without_folio(
                    &CLAUDE,
                    serde_json::from_str(splice::body(text)).expect("JSON"),
                )
            };
            assert_eq!(value(&written), value(&theirs), "{name}");

            assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed, "{name}");
            assert_eq!(
                std::fs::read(&path).expect("read back"),
                theirs.as_bytes(),
                "{name}: turning the switch off hands back the file that was there"
            );
        }
    }

    /// RED (0.4.10 plan row 67a) — **an entry of a copy that is gone is replaced where it stands**,
    /// between the person's own hooks in its group and between their groups in its event.
    ///
    /// RED GATE: `plant-67a-claims-nothing.py` (every own entry deleted and appended anew, today's
    /// writer's behaviour) moves the entry to the end and this goes red.
    #[test]
    fn a_gone_copys_entry_is_replaced_where_it_stands() {
        let dir = scratch("in-place");
        let path = dir.join(SETTINGS_FILE);
        let gone = gone_stop(&dir);
        let new = own_stop(&exe());
        let before = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say before"},"#;
        let after = r#",{"type":"command","command":"say after"}]},{"hooks":[{"type":"command","command":"say last"}]}]"#;
        std::fs::write(&path, format!("{before}{gone}{after}}}}}\n")).expect("their own file");

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(
            written.starts_with(&format!("{before}{new}{after}")),
            "the entry keeps its place and only it changes: {written}"
        );

        // And in a group of its own between two of theirs, over lines.
        let theirs = format!(
            r#"{{
  "hooks": {{
    "Stop": [
      {{ "hooks": [ {{ "type": "command", "command": "a" }} ] }},
      {{ "hooks": [ {gone} ] }},
      {{ "hooks": [ {{ "type": "command", "command": "b" }} ] }}
    ]
  }}
}}
"#
        );
        std::fs::write(&path, &theirs).expect("their own file");
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        let at = theirs.find(&gone).expect("the old entry");
        // Up to the line that closes the event's list, the file's last `]`.
        let closing = theirs.rfind(']').expect("the event's end");
        let end = theirs[..closing].rfind('\n').expect("its line");
        assert!(
            written.starts_with(&format!(
                "{}{new}{}",
                &theirs[..at],
                &theirs[at + gone.len()..end]
            )),
            "{written}"
        );
    }

    /// RED (0.4.10 plan row 67a) — **turning the switch off takes out exactly Folio's entries**:
    /// the entry between two of the person's in one group, with its comma, and a group of
    /// Folio's alone, with its comma — and not a byte of theirs, whatever the layout.
    ///
    /// RED GATE: `plant-67a-run-eats-a-neighbour.py` (a deletion runs one item too far) drops a
    /// foreign entry and the bytes differ.
    #[test]
    fn turning_it_off_takes_out_exactly_folios_entries() {
        let dir = scratch("off-exact");
        let path = dir.join(SETTINGS_FILE);
        let own = own_stop(&exe());
        let a = r#"{"type":"command","command":"a"}"#;
        let b = r#"{"type":"command","command":"b"}"#;
        let c = r#"{"type":"command","command":"c"}"#;
        let theirs = format!(
            r#"{{"hooks":{{"Stop":[{{"hooks":[{a},{own},{b}]}},{{"hooks":[{own}]}},{{"hooks":[{c}]}}]}},"model":"x"}}"#
        );
        std::fs::write(&path, &theirs).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            format!(
                r#"{{"hooks":{{"Stop":[{{"hooks":[{a},{b}]}},{{"hooks":[{c}]}}]}},"model":"x"}}"#
            )
        );

        // Over CRLF lines whose indentation is not the file's unit: written with `\n` here and
        // turned into the file's endings once, at the end.
        let over_lines = |own: Option<&str>| {
            let own = own.map_or(String::new(), |own| {
                format!(
                    r#"
          {own},"#
                )
            });
            format!(
                r#"{{
    "hooks": {{
        "Stop": [
            {{
                "hooks": [
          {a},{own}
          {b}
                ]
            }}
        ]
    }}
}}"#
            )
            .replace('\n', "\r\n")
        };
        std::fs::write(&path, over_lines(Some(&own))).expect("their own file");
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            over_lines(None)
        );
    }

    /// **The one place the round trip is not the identity, said** (0.4.10 plan row 67a, §2): an
    /// empty `"hooks": {}` the person wrote is filled by the install and, once Folio's entries go
    /// again, goes with them — the file does not say who put an empty object there, and the rule
    /// that takes away the `hooks` Folio created is the same rule. Every other byte comes back.
    #[test]
    fn an_empty_hooks_object_goes_with_folios_entries() {
        let dir = scratch("empty-hooks");
        let path = dir.join(SETTINGS_FILE);
        let theirs = "{\n  \"model\": \"opus\",\n  \"hooks\": {}\n}\n";
        std::fs::write(&path, theirs).expect("their own file");
        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(
            written.starts_with(
                r#"{
  "model": "opus",
  "hooks": {
    ""#
            ),
            "{written}"
        );
        assert_eq!(apply_to(&path, false, &exe()), Outcome::Removed);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "{\n  \"model\": \"opus\"\n}\n"
        );

        // Review N2, the same class one level up: an object that holds nothing but whitespace
        // comes back as `{}` once Folio's entries — its only members — go again.
        for (theirs, back) in [
            ("{ }\n", "{}\n"),
            ("{\n}\n", "{}\n"),
            ("{\r\n}\r\n", "{}\r\n"),
        ] {
            std::fs::write(&path, theirs).expect("their own file");
            assert_eq!(
                apply_to(&path, true, &exe()),
                Outcome::Installed,
                "{theirs:?}"
            );
            assert_eq!(
                apply_to(&path, false, &exe()),
                Outcome::Removed,
                "{theirs:?}"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("read back"),
                back,
                "{theirs:?}"
            );
        }
    }

    /// **A file the splice cannot read, or cannot tell which key the agent reads, is never
    /// written** — said, and byte for byte as it was, with no copy kept beside it.
    ///
    /// Review N3: an entry that reads as Folio's only because the value parser keeps the last of
    /// two `command`s — the person's first, Folio's second — is one of them. Replacing or deleting
    /// it would take the person's `command` with it.
    #[test]
    fn a_file_the_splice_cannot_be_sure_of_is_never_written() {
        let twice = own_stop(&exe()).replacen("{", r#"{"command":"notify-me","#, 1);
        assert!(
            matches!(
                hook_owner(&serde_json::from_str(&twice).expect("JSON")),
                Ok(Some(_))
            ),
            "the value parser reads the entry as Folio's: {twice}"
        );
        for (theirs, reason) in [
            (
                "{\n  // mine\n  \"model\": \"opus\"\n}\n".to_owned(),
                crate::i18n::Text::AgentHooksUnreadable.text(),
            ),
            (
                "{\"model\": \"opus\",}\n".to_owned(),
                crate::i18n::Text::AgentHooksUnreadable.text(),
            ),
            (
                "{\"hooks\": {}, \"hooks\": {}}\n".to_owned(),
                crate::i18n::Text::AgentHooksDuplicateKey.text(),
            ),
            (
                "{\"hooks\": {\"Stop\": [], \"Stop\": []}}\n".to_owned(),
                crate::i18n::Text::AgentHooksDuplicateKey.text(),
            ),
            (
                "{\"hooks\": {\"Stop\": [{\"hooks\": [], \"hooks\": []}]}}\n".to_owned(),
                crate::i18n::Text::AgentHooksDuplicateKey.text(),
            ),
            (
                format!("{{\"hooks\": {{\"Stop\": [{{\"hooks\": [{twice}]}}]}}}}\n"),
                crate::i18n::Text::AgentHooksDuplicateKey.text(),
            ),
        ] {
            let dir = scratch("unsure");
            let path = dir.join(SETTINGS_FILE);
            std::fs::write(&path, &theirs).expect("their own file");
            for install in [true, false] {
                assert_eq!(
                    apply_to(&path, install, &exe()),
                    Outcome::Refused(reason),
                    "{theirs}"
                );
                assert_eq!(std::fs::read_to_string(&path).expect("there"), theirs);
                assert_eq!(names(&dir), vec![SETTINGS_FILE.to_owned()], "{theirs}");
            }
        }
    }

    /// RED (0.4.10 plan row 67a; review N1) — **the write-time self-check refuses an edit that
    /// changed anything that is not Folio's**: a foreign entry respelled, two of the person's
    /// groups in the other order, a foreign entry dropped, a key outside `hooks` reformatted — and
    /// the bytes between the person's pieces: the whitespace between two of their groups, a
    /// group's `"hooks"` key respelled, an empty event of theirs taken away or its inside changed.
    /// An edit that only adds Folio's passes, into an empty event of theirs too (D1).
    ///
    /// RED GATE: `plant-67a-self-check-skipped.py` makes `verified` hand every text on, and every
    /// refusal below goes red; `plant-67a-outside-folios-skipped.py` skips the comparison of the
    /// file with Folio's extents cut out, and the first case — whitespace inside a foreign entry,
    /// which the value comparison cannot see — goes red, as do the whitespace and empty-event
    /// cases after it.
    #[test]
    fn the_self_check_refuses_an_edit_that_changed_anything_else() {
        let own = own_stop(&exe());
        let theirs = r#"{"model": "opus", "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "a"}]}, {"hooks": [{"type": "command", "command": "b"}]}], "Notification": [ ]}}"#;
        let document = splice::parse(theirs).expect("JSON");
        for changed in [
            theirs.replace(r#""command": "a""#, r#""command":"a""#),
            theirs.replace(r#""command": "a""#, "\"command\": \"\\u0061\""),
            theirs.replace(
                r#"{"hooks": [{"type": "command", "command": "a"}]}, {"hooks": [{"type": "command", "command": "b"}]}"#,
                r#"{"hooks": [{"type": "command", "command": "b"}]}, {"hooks": [{"type": "command", "command": "a"}]}"#,
            ),
            theirs.replace(
                r#"{"hooks": [{"type": "command", "command": "a"}]}, "#,
                "",
            ),
            theirs.replace(r#""model": "opus""#, r#""model":"opus""#),
            format!("{theirs}\n"),
            format!("\u{feff}{theirs}"),
            "[]".to_owned(),
            theirs.replace("]}, {\"hooks\"", "]},{\"hooks\""),
            theirs.replace("[{\"hooks\": [{\"type\": \"command\", \"command\": \"a\"", "[{\"hooks\" : [{\"type\": \"command\", \"command\": \"a\""),
            theirs.replace(r#", "Notification": [ ]"#, ""),
            theirs.replace(r#""Notification": [ ]"#, r#""Notification": []"#),
        ] {
            assert_eq!(
                verified(&CLAUDE, &document, changed.clone()),
                Err(crate::i18n::Text::AgentHooksSpliceRefused.text()),
                "{changed}"
            );
        }
        let added = theirs.replace(
            r#"{"type": "command", "command": "b"}]}"#,
            &format!(r#"{{"type": "command", "command": "b"}}]}}, {{"hooks": [{own}]}}"#),
        );
        assert_eq!(verified(&CLAUDE, &document, added.clone()), Ok(added));
        let filled = theirs.replace(
            r#""Notification": [ ]"#,
            &format!(r#""Notification": [{{"hooks": [{own}]}}]"#),
        );
        assert_eq!(verified(&CLAUDE, &document, filled.clone()), Ok(filled));
    }

    /// **The reader is strict JSON and reads what the value parser reads**, so the two views of a
    /// file never disagree about whether it is one.
    #[test]
    fn the_span_reader_reads_what_serde_reads() {
        for text in [
            "{}",
            " { \"a\" : [ 1 , -0.5e+3 , true , false , null , \"\\ud83d\\ude00\" ] } ",
            "[{\"\\u00e9\": {}}]",
            "\"\\t\\n\\\\\\/\"",
        ] {
            assert!(splice::parse(text).is_ok(), "{text}");
            assert!(serde_json::from_str::<Value>(text).is_ok(), "{text}");
        }
        for text in [
            "{\"a\": 1,}",
            "{a: 1}",
            "[01]",
            "[1.]",
            "[.5]",
            "\"\\ud83d\"",
            "\"\\x\"",
            "\"tab\there\"",
            "{} {}",
            "// c\n{}",
        ] {
            assert!(splice::parse(text).is_err(), "{text}");
            assert!(serde_json::from_str::<Value>(text).is_err(), "{text}");
        }
        let document = splice::parse("\u{feff} {\"é\": [1]}").expect("JSON");
        assert_eq!(document.body, 3);
        let root = document.root.as_ref().expect("a value");
        let member = root.member("é").expect("once").expect("there");
        assert_eq!(&document.text[member.key_start..member.key_end], "\"é\"");
        assert_eq!(&document.text[member.value.start..member.value.end], "[1]");
    }
}
