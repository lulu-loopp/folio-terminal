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
use crate::attention_ownership::{self as ownership, Decision};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// The directory Claude Code keeps user configuration in, as **this environment** says it.
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os(CONFIG_DIR_VARIABLE),
        std::env::var_os(bt_platform::home_variable()),
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
        std::env::var_os(CONFIG_DIR_VARIABLE),
        std::env::var_os(bt_platform::home_variable()),
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
    let state = state_at(&path);
    if state != State::Installed {
        return state;
    }
    let Some(exe) = std::env::current_exe().ok() else {
        return State::Unreadable;
    };
    let Ok(text) =
        bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Attention, path)
    else {
        return State::Unreadable;
    };
    let Some(value) = serde_json::from_str::<Value>(&text).ok() else {
        return State::Unreadable;
    };
    if (owners(&value).unwrap_or_default())
        .iter()
        .any(|owner| crate::explorer_menu::same_path(owner, &exe))
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
fn state_at(path: &Path) -> State {
    let text = match standing(path) {
        // Nothing there is the same answer to the only question being asked.
        Standing::Nothing => return State::Absent,
        // **Not `Absent`.** There is a file, and a row that said "not installed" about it would
        // offer to write over one this build never read. See [`standing`].
        Standing::Unreadable => return State::Unreadable,
        // Nor `Unreadable`: this one was read, and the row says why it is not ours to change.
        Standing::Refused(reason) => return State::Refused(reason),
        Standing::Text(text) => text,
    };
    if text.trim().is_empty() {
        return State::Absent;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(settings) if settings.is_object() => {
            if declares_folio(&settings) {
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
    let Ok(settings) = serde_json::from_str::<Value>(&text) else {
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
        .is_some_and(|hooks| {
            hooks
                .iter()
                .any(|hook| hook_owner(hook).is_ok_and(|owner| owner.is_some()))
        })
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

fn owners(settings: &Value) -> Result<Vec<PathBuf>, &'static str> {
    let mut found = Vec::new();
    let Some(hooks) = settings.get("hooks") else {
        return Ok(found);
    };
    for groups in hooks
        .as_object()
        .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?
        .values()
    {
        for group in groups
            .as_array()
            .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?
        {
            let rows = group
                .get("hooks")
                .and_then(Value::as_array)
                .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?;
            for hook in rows {
                if !hook.is_object() {
                    return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
                }
                // **Ownership is per entry.** An entry this build cannot decode is not Folio's: it
                // is left exactly where it is, and it has no say over the entries beside it. One
                // hand-written `"mytool attention claude-code:Stop"` used to refuse every edit to
                // the whole file, install and remove alike — a second state nobody could get out
                // of (closure review R4). The document's own shape, above and below, is still a
                // refusal: that is a file this build cannot read, not an entry it cannot claim.
                let Ok(Some(owner)) = hook_owner(hook) else {
                    continue;
                };
                if group.as_object().is_none_or(|o| {
                    o.keys()
                        .any(|k| !["matcher", "hooks"].contains(&k.as_str()))
                }) || group.get("matcher").is_some_and(|v| !v.is_string())
                {
                    return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
                }
                found.push(owner);
            }
        }
    }
    Ok(found)
}

fn declares_folio(settings: &Value) -> bool {
    !declared_events(settings).is_empty()
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
    if attention_map::turn_end_row(CLAUDE_CODE, event).is_some_and(|row| row.words.are_somewhere())
    {
        args.extend(["--json".to_owned(), crate::cli::STDIN_PAYLOAD.to_owned()]);
    }
    args
}

/// Mutate a validated document after `apply_at` has checked every owner and consent.
/// User hook entries, including entries sharing a matcher group, survive.
fn install_into(settings: &mut Value, exe: &Path) -> bool {
    install_into_on(settings, exe, bt_platform::host_platform())
}

/// Direct execution has the same shape on every platform.
fn install_into_on(settings: &mut Value, exe: &Path, _platform: HostPlatform) -> bool {
    let before = settings.clone();
    remove_from(settings, exe, true);
    let object = match settings {
        Value::Object(object) => object,
        _ => {
            *settings = Value::Object(Map::new());
            let Value::Object(object) = settings else {
                unreachable!("just assigned an object")
            };
            object
        }
    };
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let Some(hooks) = hooks.as_object_mut() else {
        return false;
    };
    for row in rows_to_install() {
        let (event, matcher) = match row.event.split_once('.') {
            Some((event, matcher)) => (event, Some(matcher)),
            None => (row.event, None),
        };
        let mut group = Map::new();
        if let Some(matcher) = matcher {
            group.insert("matcher".to_owned(), matcher.into());
        }
        let mut hook = Map::new();
        hook.insert("type".to_owned(), "command".into());
        hook.insert("command".to_owned(), exe.to_string_lossy().as_ref().into());
        hook.insert("args".to_owned(), args_for(row.event).into());
        // **Every one of these is asynchronous**, and it is not an optimisation (plan §10.4.3).
        // `PermissionRequest` is a *synchronous decision gate* with a ten-minute timeout: a signal
        // hook that made it wait would put this program between the user and every approval Claude
        // Code ever asks for. Asynchronous hooks cannot return a decision, and this one has none to
        // return — it wants the side effect and nothing else.
        hook.insert("async".to_owned(), true.into());
        group.insert("hooks".to_owned(), Value::Array(vec![Value::Object(hook)]));
        hooks
            .entry(event.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(array) = hooks.get_mut(event).and_then(Value::as_array_mut) {
            array.push(Value::Object(group));
        }
    }
    *settings != before
}

/// Take Folio's hooks back out, leaving everything else exactly as it was.
///
/// Symmetric with [`install_into`] and tested as such: install, remove, and the value is the one
/// that went in — including a user's own hooks under the same event names, an empty `hooks` object
/// they had written themselves, and the ordering of everything around it.
fn remove_from(settings: &mut Value, exe: &Path, takeover: bool) -> bool {
    let Some(object) = settings.as_object_mut() else {
        return false;
    };
    let Some(hooks) = object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    let events = hooks.keys().cloned().collect::<Vec<_>>();
    for event in events {
        let Some(groups) = hooks.get_mut(&event).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = groups.len();
        groups.retain_mut(|group| {
            let Some(rows) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let count = rows.len();
            rows.retain(|hook| {
                !hook_owner(hook).ok().flatten().is_some_and(|owner| {
                    takeover || ownership::other_live(&owner, exe) == Ok(false)
                })
            });
            changed |= rows.len() != count;
            !rows.is_empty() || count == 0
        });
        changed |= groups.len() != before;
        // An event whose only entries were ours goes with them. An event that was empty before we
        // arrived stays empty, because we did not put it there.
        if groups.is_empty() && before != 0 {
            hooks.remove(&event);
        }
    }
    // The same rule one level up: a `hooks` object that only existed to hold ours goes too.
    if changed && hooks.is_empty() {
        object.remove("hooks");
    }
    changed
}

/// Put Folio's hooks in, or take them out, on this machine.
///
/// **The file is read, changed and written whole**, and a copy of what was there is kept beside it
/// the first time each day — `shell_integration`'s rule, for `shell_integration`'s reason: this is
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
    if let Err(reason) = ownership::stable_executable(Some(exe)) {
        return Outcome::Refused(reason);
    }
    if !path.is_absolute() {
        return Outcome::Refused(crate::i18n::Text::AgentHooksRootUnstable.text());
    }
    let install = decision.installs();
    let existing = match standing(path) {
        Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        Standing::Nothing => String::new(),
        // Refused rather than replaced, exactly as an unparseable file is — see [`standing`].
        Standing::Unreadable => return Outcome::Refused(UNREADABLE),
        // The same refusal, carrying the filesystem's own reason rather than this one.
        Standing::Refused(reason) => return Outcome::Refused(reason),
    };
    let mut settings = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_str::<Value>(&existing) {
            Ok(value) if value.is_object() => value,
            // Refused rather than replaced. A settings file this build cannot read is a settings
            // file somebody wrote, and overwriting it to add a convenience is not a trade anyone
            // agreed to.
            _ => return Outcome::Refused(UNREADABLE),
        }
    };
    let paths = match owners(&settings) {
        Ok(paths) => paths,
        Err(reason) => return Outcome::Refused(reason),
    };
    let others = match ownership::check(&paths, exe, &decision) {
        Ok(others) => others,
        Err(outcome) => return outcome,
    };
    let changed = if install {
        install_into(&mut settings, exe)
    } else {
        remove_from(&mut settings, exe, false)
    };
    let _record = if install {
        match ownership::record(data, path.parent().expect("absolute config"), "claude") {
            Ok(lock) => Some(lock),
            Err(reason) => return Outcome::Refused(reason),
        }
    } else {
        None
    };
    if !changed {
        return if others.is_empty() {
            Outcome::Unchanged
        } else {
            Outcome::LeftOther(others)
        };
    }
    let text = match serde_json::to_string_pretty(&settings) {
        Ok(text) => text,
        Err(_) => return Outcome::Refused("the settings could not be written back"),
    };
    match land(path, &existing, "json", format!("{text}\n").as_bytes()) {
        Landing::Landed => {}
        Landing::NoDirectory => {
            return Outcome::Refused("the user configuration directory could not be created");
        }
        Landing::NoBackup => {
            return Outcome::Refused(NO_BACKUP);
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

/// The sentence all three installers say when the copy of the user's own file could not be kept.
///
/// One string because it is one ruling: **the copy is a precondition of the write, not a courtesy
/// beside it.** See [`land`].
pub(crate) const NO_BACKUP: &str =
    "a copy of your own file could not be kept, so nothing was written";

/// What this module says about a settings file it could not read, whichever half could not read it.
///
/// One sentence for the byte that is not UTF-8 and for the document that is not JSON, because to
/// the reader they are one fact: there is a file there, and this build is not going to guess at it.
const UNREADABLE: &str = "the settings file is not one this build can read";

/// **What is standing at a configuration file's path**, as the answer to "is there one".
///
/// The distinction between these three is the whole of the read side of [`land`]'s contract.
/// `std::fs::read_to_string` fails the same way for a file that is not there and for one this
/// build was not allowed to read, and an installer that took both for "there was nothing there"
/// writes a whole fresh file over somebody's own — with no copy kept beside it, because `land` is
/// given nothing to copy. Release audit 2026-09-16 (C-3): one non-UTF-8 byte in `config.toml` or
/// `settings.json` is enough, the file is perfectly writable, and the loss is certain rather than
/// a race. So the error kind is asked about, and everything that is not "no such file" refuses.
pub(crate) enum Standing {
    /// There is no file at that path. A write there creates one and destroys nothing.
    Nothing,
    /// The file's own text, as it reads today.
    Text(String),
    /// There is something at that path and this build could not read it. **Never written over.**
    Unreadable,
    /// There is a file there, it can be read, and **this build will not edit it** — it is a link
    /// out of the agent's own folder, it is shared by hard links, or it is read-only. The sentence
    /// is the filesystem predicate's own, because each of the three is a different thing to do
    /// about (closure review R1).
    Refused(&'static str),
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
pub(crate) fn editable_target(path: &Path) -> Result<PathBuf, Standing> {
    let reason = match crate::shell_integration::profile_path_reason(path) {
        Ok(None) => return Ok(path.to_path_buf()),
        Ok(Some(reason)) => reason,
        // The walk itself failed — an ancestor this account may not even ask about. That is not a
        // file this build declines to edit, it is one it could not look at, and `Unreadable` has
        // been the honest answer to it since the 2026-09-16 audit.
        Err(_) => return Err(Standing::Unreadable),
    };
    if reason != crate::i18n::Text::ShellProfileLink {
        return Err(Standing::Refused(agent_reason(reason)));
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
    .map_err(Standing::Refused)?;
    // The resolved target carries no link of its own, so what this can still answer is a hard
    // link, a read-only file or a directory — the refusals §6.3 keeps.
    match crate::shell_integration::profile_path_reason(&target) {
        Ok(None) => Ok(target),
        Ok(Some(reason)) => Err(Standing::Refused(agent_reason(reason))),
        Err(_) => Err(Standing::Unreadable),
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

/// Read a configuration file the way all three installers have to read one.
///
/// Anything but [`std::io::ErrorKind::NotFound`] is [`Standing::Unreadable`]: a permission the
/// user's own ACL withholds, a sharing lock somebody else's editor holds, a byte that is not
/// UTF-8, a directory standing under the file's name. None of them is a file that is not there,
/// and that is the only state in which writing a fresh one loses nothing.
pub(crate) fn standing(path: &Path) -> Standing {
    let target = match editable_target(path) {
        Ok(target) => target,
        // **Not `Nothing`, and mostly not `Unreadable` either.** There is a file, it can be read,
        // and this build will not edit it — a third thing, and the row above says which (§6.3, R1).
        Err(standing) => return standing,
    };
    match bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Attention, target)
    {
        Ok(text) => Standing::Text(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Standing::Nothing,
        Err(_) => Standing::Unreadable,
    }
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
/// `extension` is the target's own extension — `settings.json` with `"json"` gives
/// `settings.json.bak-20260827`. `existing` is what was read off the file, empty when there was
/// nothing there.
pub(crate) fn land(path: &Path, existing: &str, extension: &str, bytes: &[u8]) -> Landing {
    let Ok(target) = editable_target(path) else {
        return Landing::NotWritten;
    };
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Landing::NoDirectory;
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
        if bt_persist::atomic_replace_preserving(&target, bytes).is_err() {
            return Landing::NotWritten;
        }
        return Landing::Landed;
    }
    if bt_persist::atomic_write(&target, bytes).is_err() {
        return Landing::NotWritten;
    }
    Landing::Landed
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{MappedAction, Tier, WaitKind};

    #[test]
    fn attention_two_live_copies_require_takeover_and_preserve_bytes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(concat!("hooks-", "two-copies"));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("A space $ ` ' 中文.exe");
        let b = root.join("B.exe");
        std::fs::write(&a, b"A").unwrap();
        std::fs::write(&b, b"B").unwrap();
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
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tb-test-marks/hooks")
                .join(path.parent().unwrap().file_name().unwrap()),
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
            config_dir_from(named(r"D:\scratch\claude-home"), named(r"C:\Users\someone")),
            Some(PathBuf::from(r"D:\scratch\claude-home"))
        );
        // Set-but-empty is not set. A shell that wrote `CLAUDE_CONFIG_DIR=` said nothing, and
        // taking it literally would put this file at the root of the current drive.
        assert_eq!(
            config_dir_from(named(""), named(r"C:\Users\someone")),
            Some(PathBuf::from(r"C:\Users\someone").join(DEFAULT_DIRECTORY))
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
        let source = include_str!("attention_hooks.rs");
        for reaching in [concat!("current", "_dir"), concat!("project", "_dir")] {
            assert!(
                !source.contains(reaching),
                "`{reaching}` would let a working directory decide where automation is written"
            );
        }
        // And the other half of the same rule, said as a property rather than as an absence: given
        // an absolute answer the path is absolute, so nothing this module writes can ever be
        // resolved against wherever the process happens to be standing.
        assert!(
            config_dir_from(named(r"D:\scratch\claude-home"), None)
                .is_some_and(|path| path.is_absolute())
        );
        assert!(
            config_dir_from(None, named(r"C:\Users\someone"))
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
            settings_path_shown_from(None, named(r"C:\Users\someone")),
            "~/.claude/settings.json"
        );
        // Set-but-empty is not set, `config_dir_from`'s rule, so the spelling does not move.
        assert_eq!(
            settings_path_shown_from(named(""), named(r"C:\Users\someone")),
            "~/.claude/settings.json"
        );
        // …and with nothing to go on at all, the default spelling is still the honest answer:
        // there is no path to offer instead.
        assert_eq!(
            settings_path_shown_from(None, None),
            "~/.claude/settings.json"
        );
        assert_eq!(
            settings_path_shown_from(named(r"D:\scratch\claude-home"), named(r"C:\Users\someone")),
            PathBuf::from(r"D:\scratch\claude-home")
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
        assert!(remove_from(&mut settings, &exe(), false));
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
        assert!(remove_from(&mut settings, &exe(), false));
        assert!(
            !remove_from(&mut settings, &exe(), false),
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
        assert!(!remove_from(&mut array, &exe(), false));
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
        if declares_folio(settings) {
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
    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(format!(
                "folio-land-{name}-{}-{}",
                std::process::id(),
                today()
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        // Resolved once, here: `%TEMP%` on a real machine can be a short name or sit behind a
        // link, and neither is what any of these tests is about.
        std::fs::canonicalize(&dir).expect("scratch path")
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
        // Spelled in halves so this assertion is not its own counter-example — the source these
        // read is this file, and one whole spelling here would be found in the half it is looking
        // at. `concat!` puts them back together at compile time.
        let raw_write = concat!("fs::", "write(");
        let atomic = concat!("bt_persist::", "atomic_write(");
        let production = |source: &str| {
            source
                .split_once(concat!("#[cfg(", "test)]"))
                .map_or(source, |(before, _)| before)
                .to_owned()
        };
        // `land`'s own dated copy is the one raw write left on this path, and it is a write to a
        // file nothing else has ever read — never to the target.
        let hooks = production(include_str!("attention_hooks.rs"));
        assert_eq!(
            hooks.matches(raw_write).count(),
            1,
            "the only raw write left in this module is `land`'s dated copy"
        );
        assert_eq!(
            hooks.matches(atomic).count(),
            1,
            "and the target is replaced through the one atomic writer"
        );
        for (name, source) in [
            (
                "attention_codex",
                production(include_str!("attention_codex.rs")),
            ),
            (
                "attention_copilot",
                production(include_str!("attention_copilot.rs")),
            ),
        ] {
            assert!(
                !source.contains(raw_write),
                "{name} writes a configuration file behind `land`'s back"
            );
            assert!(
                source.contains(concat!("attention_hooks::", "land(")),
                "{name} must land its bytes through the one writer"
            );
        }
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
            land(&target, &existing, "json", b"{}\n"),
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
        assert_eq!(land(&target, "", "json", b"first\n"), Landing::Landed);
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "first\n");
        assert_eq!(
            names(&dir),
            vec!["settings.json".to_owned()],
            "a first landing leaves one file: no copy of nothing, no temporary"
        );

        // A second landing over a file that was there keeps exactly one dated copy of it.
        assert_eq!(
            land(&target, "first\n", "json", b"second\n"),
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
            land(&target, "second\n", "json", b"third\n"),
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
        assert_eq!(installing, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            names(&dir),
            vec![SETTINGS_FILE.to_owned()],
            "nothing was written beside it either"
        );
        // And the row drawn from it says so, rather than offering to write over it.
        assert_eq!(state_at(&path), State::Unreadable);
        // Taking it back out is refused for the same reason: this build cannot tell whose it is.
        let taking_it_out = apply_to(&path, false, &exe());
        assert_eq!(taking_it_out, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A file that is not there is the one state in which writing a fresh one loses nothing.**
    #[test]
    fn a_settings_file_that_is_not_there_is_the_one_that_gets_created() {
        let dir = scratch("absent");
        let path = dir.join(SETTINGS_FILE);
        assert_eq!(state_at(&path), State::Absent);

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(state_at(&path), State::Installed);
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
}
