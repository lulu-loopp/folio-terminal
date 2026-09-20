//! **Installing copilot CLI's hooks, into the user's own configuration and nowhere else.**
//!
//! One settings row presses this, and it is [`attention_hooks`](crate::attention_hooks)'s contract
//! over a third upstream: only the user-level file is ever written, a file this build cannot read
//! is never written over, somebody else's file at the name we want is refused rather than replaced,
//! and installing and then uninstalling hands back the directory that was there.
//!
//! # Where the file is, and why it is asked for rather than composed
//!
//! §7.1.6j's lesson for the third time. Upstream's own reference, quoted in
//! `docs/plans/attention/evidence-copilot-cli-2026-08-26.md` §1.2:
//!
//! > **User-level hook files** — `*.json` files in the user-level hooks directory. By default this
//! > is `~/.copilot/hooks/` on macOS and Linux, or `%USERPROFILE%\.copilot\hooks\` on Windows. If
//! > `COPILOT_HOME` is set, it is `$COPILOT_HOME/hooks/`.
//!
//! So `COPILOT_HOME` is read first, because it is the variable copilot itself reads, and the
//! documented default beside the user's profile is only the fallback.
//!
//! # **The five other places copilot looks, and none of them is written here**
//!
//! This family has the widest repository attack surface of the three. Beside the user-level
//! directory above, upstream loads `.github/hooks/*.json`, the `hooks` block of
//! `.github/copilot/settings.json` and of `.github/copilot/settings.local.json` — **and other
//! people's**: "Cross-tool `.claude/settings.json` and `.claude/settings.local.json` files in the
//! repository are also read." Every one of those is inside whatever folder the user happened to
//! open. plan §10.4.3 / §10.6 clause 5 hold here without a word changed, and the test at the bottom
//! is the red form of it: there is exactly one path this module can produce and no working
//! directory reaches it.
//!
//! # What is installed
//!
//! One file, `folio.json`, whole and ours — which is the shape difference from
//! [`attention_hooks`](crate::attention_hooks), where hooks live in a document belonging to
//! somebody else and have to be threaded into it entry by entry. Upstream combines every hook file
//! in the directory ("all hook entries from all sources are run"), so a file of our own beside the
//! user's own files costs them nothing and can be taken away without touching a byte of theirs.
//!
//! **The two waits arrive on one hook, narrowed by a matcher.** `notification` fires for six
//! subtypes and "Omit `matcher` to receive all notification types" — four of the six are
//! completions this block refuses to read as waits, so the matcher is not decoration. Upstream
//! anchors what it is given ("anchored as `^(?:PATTERN)$`"), so the value written is the bare
//! subtype name and the anchoring is upstream's.
//!
//! Copilot CLI's `exec`/`args` form runs Folio directly, on all platforms.
//! No payload is interpolated. `timeoutSec: 5` remains unchanged. Legacy shell
//! entries are decoded only for ownership and migrated on an explicit install.
//!
//! # The version gate
//!
//! `permission_prompt` did not always mean what this build reads it as. Upstream's changelog, at
//! `1.0.26` on 2026-04-14: "Permission prompt notification hook only fires when a prompt is
//! actually shown to the user." Before that it fired for every tool execution, already-approved
//! ones included — which is the exact substitution this whole block exists to undo, and installing
//! into it would put a standing wait on a pane that is not waiting.
//!
//! So the machine is **asked**, on a thread, once per process, and the answer decides what the row
//! says and whether the press writes anything. A machine that gives no answer at all — no
//! `copilot` on the path, a spawn that failed — is **not** gated: what is refused is a version
//! proved too old, never the absence of proof.

pub(crate) use crate::attention_ownership::Outcome;
use crate::attention_ownership::{self as ownership, Decision};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bt_platform::HostPlatform;
use serde_json::{Map, Value};

use crate::attention::MappingRow;
use crate::attention_map::{self, COPILOT};

/// The variable copilot reads for its configuration directory.
const HOME_VARIABLE: &str = "COPILOT_HOME";

/// The default directory's name beside the user's profile, for when the variable says nothing.
const DEFAULT_DIRECTORY: &str = ".copilot";

/// The directory inside it that holds **user-level** hook files.
const HOOKS_DIRECTORY: &str = "hooks";

/// **The only file this module ever writes.**
const HOOKS_FILE: &str = "folio.json";

/// The user-level settings file, read and never written — see [`hooks_are_switched_off`].
const SETTINGS_FILE: &str = "settings.json";

/// The user setting that turns every hook file off at once, ours included.
const DISABLE_ALL_HOOKS: &str = "disableAllHooks";

/// What this module says about a hook file it could not read, whichever half could not read it.
///
/// One sentence for the byte that is not UTF-8 and for the document that is not JSON, because to
/// the reader they are one fact: there is a file there, and this build is not going to guess at it.
const UNREADABLE: &str = "the copilot hook file is not one this build can read";

/// The schema version upstream's own example carries: `"version": 1`.
const SCHEMA_VERSION: u64 = 1;

/// How long upstream may wait on one of these. See the module header.
const TIMEOUT_SECONDS: u64 = 5;

/// **The oldest copilot whose `permission_prompt` means "somebody was asked"** — changelog
/// `1.0.26`, quoted in the module header.
const MINIMUM: Version = Version {
    major: 1,
    minor: 0,
    patch: 26,
};

/// A `major.minor.patch`, and nothing about what it is a version of.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// **The first three-part number in whatever the program said.**
    ///
    /// Taken from the machine rather than assumed, because what `copilot --version` prints is a
    /// sentence and not a field: on 2026-08-26 a real `1.0.80` answered
    ///
    /// ```text
    /// GitHub Copilot CLI 1.0.80.
    /// Run 'copilot update' to check for updates.
    /// ```
    ///
    /// — a full stop after the number, and a second line about something else. So each whitespace
    /// word is stripped of everything that is not a digit or a dot at either end and tried; the
    /// first that yields three numbers wins, and each of the three is **the digits it starts
    /// with**, because upstream publishes prereleases on the same channel and `1.0.81-12` is that
    /// version rather than no version at all. A parser that split on the last dot, or that demanded
    /// each part parse whole, would have read one real machine as a four-part version and the other
    /// as nothing.
    #[must_use]
    fn parse(text: &str) -> Option<Self> {
        text.split_whitespace().find_map(|word| {
            let trimmed = word
                .trim_matches(|character: char| !character.is_ascii_digit() && character != '.');
            let mut parts = trimmed.split('.').map(Self::leading_number);
            Some(Self {
                major: parts.next()??,
                minor: parts.next()??,
                patch: parts.next()??,
            })
        })
    }

    /// The digits one dot-separated part begins with, or nothing when it begins with none.
    #[must_use]
    fn leading_number(part: &str) -> Option<u32> {
        let digits = part
            .find(|character: char| !character.is_ascii_digit())
            .map_or(part, |end| &part[..end]);
        digits.parse().ok()
    }
}

/// **Whether this machine can be told to speak, and what stops it if not.**
///
/// The row's sentence is chosen from this, and so is whether a press writes anything. Ordered by
/// what the reader can do about it: a version that cannot carry the signal is a different problem
/// from a switch they turned off themselves.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Readiness {
    /// Nothing is known yet, or the machine gave no answer. **Not a refusal** — see the header.
    #[default]
    Unknown,
    /// A copilot new enough for `permission_prompt` to mean what this build reads it as.
    Ready,
    /// A copilot older than [`MINIMUM`]. Installing here would write a hook that misreports.
    TooOld,
    /// New enough, but the user's own settings switch every hook file off.
    HooksDisabled,
}

/// Whether this machine's user configuration already calls Folio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    /// No file of ours — no directory, no file, or a file at that name that is somebody else's.
    Absent,
    /// Our file is there.
    Installed,
    /// There is a file at our name and it could not be read as JSON. **Not** "absent": writing over
    /// a file this build cannot parse would destroy something somebody wrote by hand.
    Unreadable,
}

/// The directory copilot keeps user configuration in, as **this environment** says it.
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os(HOME_VARIABLE),
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

/// The hook file this module writes. **The only file it ever writes.**
#[must_use]
pub(crate) fn hooks_path() -> Option<PathBuf> {
    Some(config_dir()?.join(HOOKS_DIRECTORY).join(HOOKS_FILE))
}

/// The same file, spelled for a reader who is being asked to consent to it —
/// [`crate::attention_hooks::settings_path_shown`]'s rule over `COPILOT_HOME`.
#[must_use]
pub(crate) fn hooks_path_shown() -> String {
    hooks_path_shown_from(
        std::env::var_os(HOME_VARIABLE),
        std::env::var_os(bt_platform::home_variable()),
    )
}

/// The same decision, with the environment handed in.
#[must_use]
fn hooks_path_shown_from(named: Option<OsString>, home: Option<OsString>) -> String {
    let default = || format!("~/{DEFAULT_DIRECTORY}/{HOOKS_DIRECTORY}/{HOOKS_FILE}");
    if named.as_ref().is_none_or(|named| named.is_empty()) {
        return default();
    }
    config_dir_from(named, home)
        .map(|dir| {
            dir.join(HOOKS_DIRECTORY)
                .join(HOOKS_FILE)
                .display()
                .to_string()
        })
        .unwrap_or_else(default)
}

/// The user's own settings file, which this module **reads and never writes**.
#[must_use]
fn settings_path() -> Option<PathBuf> {
    Some(config_dir()?.join(SETTINGS_FILE))
}

/// What that file says today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(path) = hooks_path() else {
        return State::Absent;
    };
    let state = state_at(&path);
    if state != State::Installed {
        return state;
    }
    let Some(exe) = std::env::current_exe().ok() else {
        return State::Unreadable;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
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

/// The same question about a named file, so a test can ask it without a copilot installation on the
/// machine it runs on.
#[must_use]
fn state_at(path: &Path) -> State {
    let text = match crate::attention_hooks::standing(path) {
        // No file is the same answer to the only question being asked.
        crate::attention_hooks::Standing::Nothing => return State::Absent,
        // **Not `Absent`.** There is a file, and a row that said "not installed" about it would
        // offer to write over one this build never read.
        crate::attention_hooks::Standing::Unreadable => return State::Unreadable,
        crate::attention_hooks::Standing::Text(text) => text,
    };
    if text.trim().is_empty() {
        return State::Absent;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(document) if document.is_object() => {
            // **The same predicate `apply` refuses on**, and it has to be: a state that said
            // `Installed` about a file the press then refuses to touch would be a switch that
            // shows On and cannot be turned Off.
            if declares_folio(&document) && owners(&document).is_ok() {
                State::Installed
            } else {
                // A file somebody else put at this name, or one they added to. `Absent` would make
                // the row offer to write over it; `Unreadable` is the state that means "there is
                // something here and it is not ours to touch".
                State::Unreadable
            }
        }
        _ => State::Unreadable,
    }
}

/// **The rows this machine actually has installed**, read back off the file.
///
/// Empty when nothing is installed. Read back rather than remembered, which is the rule one layer
/// down: the configuration on disk is the answer to "which rows does this machine have", so a user
/// who edits it by hand gets a Folio that agrees with them.
#[must_use]
pub(crate) fn installed_rows() -> Vec<MappingRow> {
    let Some(path) = hooks_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    rows_declared_by(&document)
}

/// The same, from a document rather than from the disk.
#[must_use]
pub(crate) fn rows_declared_by(document: &Value) -> Vec<MappingRow> {
    let declared = declared_events(document);
    attention_map::ROWS
        .iter()
        .filter(|row| row.family == COPILOT && declared.iter().any(|name| name == row.event))
        .copied()
        .collect()
}

/// Every `<event>` (matcher-qualified where it is one) that a Folio hook is registered under.
fn declared_events(document: &Value) -> Vec<String> {
    let Some(hooks) = document.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (event, entries) in hooks {
        let Some(entries) = entries.as_array() else {
            continue;
        };
        for entry in entries {
            if !entry_is_ours(entry) {
                continue;
            }
            match entry.get("matcher").and_then(Value::as_str) {
                Some(matcher) if !matcher.is_empty() => found.push(format!("{event}.{matcher}")),
                _ => found.push(event.clone()),
            }
        }
    }
    found
}

/// Family detection delegates to the same executable-operand decoder used for edits.
fn entry_is_ours(entry: &Value) -> bool {
    entry_owner(entry).is_ok_and(|owner| owner.is_some())
}

/// The only Copilot attribution site; mixed execution modes are refused.
fn entry_owner(entry: &Value) -> Result<Option<PathBuf>, &'static str> {
    let columns = ["exec", "bash", "powershell", "command"];
    let present = columns
        .iter()
        .filter(|key| entry.get(**key).is_some())
        .collect::<Vec<_>>();
    if present.len() != 1 {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    let column = *present[0];
    let program = entry[column]
        .as_str()
        .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?;
    let owner = if column == "exec" {
        ownership::direct_path(
            program,
            entry.get("args").unwrap_or(&serde_json::json!([])),
            COPILOT,
        )?
    } else {
        ownership::legacy_path(program, COPILOT)?
    };
    if owner.is_some()
        && (entry.as_object().is_none_or(|o| {
            o.keys().any(|k| {
                ![
                    "type",
                    "exec",
                    "args",
                    "bash",
                    "powershell",
                    "command",
                    "matcher",
                    "timeoutSec",
                ]
                .contains(&k.as_str())
            })
        }) || entry
            .get("type")
            .is_some_and(|v| v.as_str() != Some("command"))
            || entry.get("matcher").is_some_and(|v| !v.is_string())
            || entry.get("timeoutSec").is_some_and(|v| !v.is_number())
            || (column != "exec" && entry.get("args").is_some()))
    {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(owner)
}

fn owners(document: &Value) -> Result<Vec<PathBuf>, &'static str> {
    if document.get("version").and_then(Value::as_u64) != Some(SCHEMA_VERSION)
        || document.as_object().is_none_or(|o| {
            o.keys()
                .any(|k| !["version", "hooks"].contains(&k.as_str()))
        })
    {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    let mut paths = Vec::new();
    for entries in document
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?
        .values()
    {
        for entry in entries
            .as_array()
            .ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?
        {
            paths.push(
                entry_owner(entry)?
                    .ok_or("a hook file of your own already stands under that name")?,
            );
        }
    }
    Ok(paths)
}

fn declares_folio(document: &Value) -> bool {
    !declared_events(document).is_empty()
}

/// **The rows one install writes**: every copilot row. None of them is tiered — this family
/// publishes one layer of each signal — so [`attention_map::installed_rows`] has nothing to choose
/// between and the whole block goes in.
#[must_use]
pub(crate) fn rows_to_install() -> Vec<MappingRow> {
    attention_map::installed_rows(attention_map::ROWS, COPILOT, |_| true)
}

/// **The whole document one install writes**, given where this build lives.
#[must_use]
pub(crate) fn document_for(exe: &Path) -> Value {
    document_for_on(exe, bt_platform::host_platform())
}

/// The same document, for a named platform.
#[must_use]
pub(crate) fn document_for_on(exe: &Path, _platform: HostPlatform) -> Value {
    let mut hooks = Map::new();
    for row in rows_to_install() {
        let (event, matcher) = match row.event.split_once('.') {
            Some((event, matcher)) => (event, Some(matcher)),
            None => (row.event, None),
        };
        let mut entry = Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".to_owned(), matcher.into());
        }
        entry.insert("exec".to_owned(), exe.to_string_lossy().as_ref().into());
        entry.insert(
            "args".to_owned(),
            serde_json::json!(["attention", format!("{COPILOT}:{}", row.event)]),
        );
        entry.insert("timeoutSec".to_owned(), TIMEOUT_SECONDS.into());
        // Upstream defaults this to `"command"` when it is omitted. Written anyway, because a
        // default is a thing that can change and this file has to keep meaning one thing.
        entry.insert("type".to_owned(), "command".into());
        let list = hooks
            .entry(event.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Some(array) = list.as_array_mut() {
            array.push(Value::Object(entry));
        }
    }
    let mut document = Map::new();
    document.insert("hooks".to_owned(), Value::Object(hooks));
    document.insert("version".to_owned(), SCHEMA_VERSION.into());
    Value::Object(document)
}

/// **Whether the user has switched every hook file off**, theirs and ours alike.
///
/// Upstream's user setting: "Disable all hooks (both repository-level and user-level)." A machine
/// with this on can be installed into perfectly and stay silent, which is the one failure this
/// block cannot tell apart from not having installed at all — so the row says it rather than
/// letting somebody discover it by watching nothing happen.
#[must_use]
fn hooks_are_switched_off() -> bool {
    let Some(path) = settings_path() else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    settings
        .get(DISABLE_ALL_HOOKS)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// **What this machine can be told**, from the probe's answer and the user's own settings.
///
/// Read at the moments it can change rather than on every frame — the caller caches it, for
/// `attention_hooks`'s reason: this reads a file and may be drawn sixty times a second.
#[must_use]
pub(crate) fn readiness() -> Readiness {
    readiness_from(probe(), hooks_are_switched_off())
}

/// The same decision with both answers handed in.
#[must_use]
fn readiness_from(found: Option<Version>, switched_off: bool) -> Readiness {
    match found {
        // **The absence of an answer is never a refusal.** No `copilot` on the path is the ordinary
        // state of a machine whose owner is about to install one, and a probe that failed to spawn
        // is this build's problem rather than theirs.
        None => Readiness::Unknown,
        Some(version) if version < MINIMUM => Readiness::TooOld,
        Some(_) if switched_off => Readiness::HooksDisabled,
        Some(_) => Readiness::Ready,
    }
}

/// **The sentence under the row**, chosen by what this machine can be told.
///
/// `crate::context_menu::row_description`'s shape, and it earns the indirection that one does not
/// quite need: two of the four answers describe a machine rather than a switch, and a row that
/// offered to install into either of them would be offering something that does not work.
#[must_use]
pub(crate) fn row_description(readiness: Readiness) -> &'static str {
    match readiness {
        // **`Unknown` reads as the plain sentence and not as a hedge.** Not having asked yet, and
        // having asked a machine with no copilot on it, are both states in which the honest thing
        // to describe is what the switch does.
        Readiness::Unknown | Readiness::Ready => crate::i18n::Text::DescCopilotHooks.text(),
        Readiness::TooOld => crate::i18n::Text::DescCopilotHooksTooOld.text(),
        Readiness::HooksDisabled => crate::i18n::Text::DescCopilotHooksDisabled.text(),
    }
}

/// Put Folio's hook file in, or take it out, on this machine.
///
/// **The version gate is here and not only on the row**, because the row is a sentence and this is
/// the act: a machine that answers `1.0.21` gets told why, rather than a file that fires on every
/// tool call an approval rule already waved through.
pub(crate) fn apply(decision: Decision, exe: &Path) -> Outcome {
    let Some(path) = hooks_path() else {
        return Outcome::Refused("no copilot configuration directory to write into");
    };
    apply_at(&path, decision, exe, &crate::persist::storage_dir())
}

/// The same act on a named file — the seam the tests press, so that what they pin is this function
/// and not a hook file belonging to whoever runs them.
pub(crate) fn apply_at(path: &Path, decision: Decision, exe: &Path, data: &Path) -> Outcome {
    if let Err(reason) = ownership::stable_executable(Some(exe)) {
        return Outcome::Refused(reason);
    }
    if !path.is_absolute() {
        return Outcome::Refused(crate::i18n::Text::AgentHooksRootUnstable.text());
    }
    let install = decision.installs();
    let existing = match crate::attention_hooks::standing(path) {
        crate::attention_hooks::Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        crate::attention_hooks::Standing::Nothing => String::new(),
        // **A file that could not be read is never written over, and never removed either** — this
        // build cannot tell whose it is. Release audit 2026-09-16 (C-3), the same conflation the
        // other two installers held.
        crate::attention_hooks::Standing::Unreadable => return Outcome::Refused(UNREADABLE),
    };
    let standing = if existing.trim().is_empty() {
        None
    } else {
        match serde_json::from_str::<Value>(&existing) {
            Ok(document) if document.is_object() => Some(document),
            // Refused rather than replaced, for `attention_hooks`'s reason: a file this build
            // cannot read is a file somebody wrote.
            _ => return Outcome::Refused(UNREADABLE),
        }
    };
    let paths = match standing.as_ref().map(owners).transpose() {
        Ok(paths) => paths.unwrap_or_default(),
        Err(reason) => return Outcome::Refused(reason),
    };
    let others = match ownership::check(&paths, exe, &decision) {
        Ok(others) => others,
        Err(outcome) => return outcome,
    };
    if !install && !others.is_empty() {
        let mut retained = standing.clone().expect("owners imply a document");
        let hooks = retained["hooks"].as_object_mut().expect("validated hooks");
        for entries in hooks.values_mut() {
            entries
                .as_array_mut()
                .expect("validated entries")
                .retain(|entry| {
                    entry_owner(entry)
                        .ok()
                        .flatten()
                        .is_some_and(|owner| others.contains(&owner))
                });
        }
        if standing.as_ref() != Some(&retained) {
            let text = serde_json::to_string_pretty(&retained).expect("JSON value");
            if crate::attention_hooks::land(path, &existing, "json", format!("{text}\n").as_bytes())
                != crate::attention_hooks::Landing::Landed
            {
                return Outcome::Refused("the copilot hook file could not be written");
            }
        }
        return Outcome::LeftOther(others);
    }
    if install {
        if readiness() == Readiness::TooOld {
            return Outcome::Refused("copilot 1.0.26 or newer is needed for this");
        }
        let root = path
            .parent()
            .and_then(Path::parent)
            .expect("absolute hooks file");
        let _record = match ownership::record(data, root, "copilot") {
            Ok(lock) => lock,
            Err(reason) => return Outcome::Refused(reason),
        };
        let written = document_for(exe);
        if standing.as_ref() == Some(&written) {
            return Outcome::Unchanged;
        }
        let Ok(text) = serde_json::to_string_pretty(&written) else {
            return Outcome::Refused("the hook file could not be written back");
        };
        // The atomic write, mandatory backup, and unsafe-path refusal are all
        // `attention_hooks::land`'s, said once for all three installers.
        //
        // **The backup's name matters more here than it does beside the other two files.** Upstream
        // loads every `*.json` in this directory, so a backup that kept the extension would be a
        // second copy of these hooks that upstream also runs — every event fired twice, for as long
        // as the file sat there. `folio.json.bak-<date>` is not a `*.json`, which is what the
        // `"json"` below produces.
        match crate::attention_hooks::land(path, &existing, "json", format!("{text}\n").as_bytes())
        {
            crate::attention_hooks::Landing::Landed => {}
            crate::attention_hooks::Landing::NoDirectory => {
                return Outcome::Refused("the copilot hooks directory could not be created");
            }
            crate::attention_hooks::Landing::NoBackup => {
                return Outcome::Refused(crate::attention_hooks::NO_BACKUP);
            }
            crate::attention_hooks::Landing::NotWritten => {
                return Outcome::Refused("the copilot hook file could not be written");
            }
        }
        Outcome::Installed
    } else {
        if standing.is_none() {
            return Outcome::Unchanged;
        }
        // **The whole file goes**, which is what "install then uninstall is the identity" means
        // when the file is one this build created: there was nothing there, and there is nothing
        // there again. The directory stays, because copilot's directory is not ours to remove.
        if std::fs::remove_file(path).is_err() {
            return Outcome::Refused("the copilot hook file could not be removed");
        }
        Outcome::Removed
    }
}

// ---------------------------------------------------------------------------
// Asking the machine which copilot it has
// ---------------------------------------------------------------------------

static PROBE: OnceLock<Option<Version>> = OnceLock::new();
static PROBE_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Teach the probe how to bring the event loop round when its answer lands.
///
/// [`crate::psreadline::install_wake`]'s twin and for its reason: a settings dialog standing on the
/// Agents page while the probe is still out has a row that will not correct itself — a modal is up,
/// so there is no shell output, no hover and no keystroke coming to produce a frame.
pub(crate) fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

/// Start the probe, once per process, on a thread of its own.
///
/// **One trigger, and it is the page the answer is printed on.** Nothing else in this program cares
/// which copilot the machine has, and a user who never opens the Agents page never starts a node
/// process to find out. Calling it again is free.
pub(crate) fn begin_probe() {
    if PROBE.get().is_some() || PROBE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    // In the workers' band, `psreadline::begin_probe`'s rule: this starts a program written in
    // JavaScript to ask it one question, and it must never be the reason a frame was late.
    bt_platform::spawn_at_priority(
        "copilot-version-probe",
        bt_platform::ThreadPriority::BelowNormal,
        || {
            let _ = PROBE.set(run_probe());
            // After the answer is published, never before: a wake that raced the `set` would send
            // the loop to read an answer that is still missing, and there is no second wake coming.
            if let Some(wake) = WAKE.get() {
                wake();
            }
        },
    )
    .ok();
}

/// What the probe found, or `None` while it is still out — and `None` again if it came back with
/// nothing, which [`readiness_from`] treats as the same thing on purpose.
#[must_use]
pub(crate) fn probe() -> Option<Version> {
    PROBE.get().copied().flatten()
}

/// Whether the probe has come back at all, **whatever it came back with**.
///
/// [`probe`] folds "still out" and "came back with nothing" together on purpose, because
/// [`readiness_from`] treats them the same: neither is a refusal. One caller has to tell them
/// apart. The first-run card offers a row only if it can be honoured, and it cannot know that a
/// copilot is new enough until the version question has been *asked and answered* — so it waits
/// for this rather than listing a row whose install would then be refused.
#[must_use]
pub(crate) fn probe_settled() -> bool {
    PROBE.get().is_some()
}

/// **Through `cmd.exe`, and that is not a shortcut.**
///
/// npm installs this program as `copilot.cmd`; `CreateProcess` appends `.exe` and does not consult
/// `PATHEXT`, so spawning `copilot` directly finds nothing on the ordinary installation. `cmd /c`
/// is what resolves a `.cmd` the way the user's own shell would, and it resolves a `.exe` too — so
/// one call covers both ways this program gets onto a machine.
///
/// **But `cmd` is never asked to find it** (R1-17). `cmd /c copilot` resolves a bare name out of
/// its own working directory before it looks at `PATH`, and this process's working directory is
/// whatever folder the shell that started Folio was standing in. A `copilot.cmd` dropped into a
/// cloned repository therefore ran the moment somebody opened Settings ▸ Agents — a page about
/// agents, running a program nobody named, with no press on anything. So the search is this
/// window's own ([`bt_platform::program_on_path`], which never looks at a working directory) and
/// what `cmd` receives is the answer, quoted, with nothing left to resolve.
#[cfg(windows)]
fn run_probe() -> Option<Version> {
    let copilot = bt_platform::program_on_path(Path::new("copilot"))?;
    // Through the quiet door (§7.40 ①): without `CREATE_NO_WINDOW` a console
    // window opens on screen the first time somebody opens the settings dialog.
    let mut command = bt_platform::quiet_command_named(Path::new("cmd.exe"))?;
    // `raw_arg` and not `args`, because what is being built is a string `cmd`
    // parses for itself: the outer quotes are the pair it strips, and the inner
    // ones are what keep a program path holding a space one token.
    let output = {
        use std::os::windows::process::CommandExt as _;
        command
            .raw_arg(probe_command_tail(&copilot))
            .output()
            .ok()?
    };
    Version::parse(&String::from_utf8_lossy(&output.stdout))
}

/// The whole of what `cmd.exe` is handed, as one string it parses itself.
///
/// `cmd /c "<command string>"` strips the outer pair of quotes and runs what is left, so a program
/// path that contains a space needs quotes of its own **inside** that pair. Split out from the
/// spawn so the shape can be pinned without a machine that has copilot on it.
#[cfg(windows)]
#[must_use]
fn probe_command_tail(copilot: &Path) -> String {
    format!("/c \"\"{}\" --version\"", copilot.display())
}

#[cfg(not(windows))]
fn run_probe() -> Option<Version> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{MappedAction, Tier};

    #[test]
    fn attention_two_live_copies_require_takeover_and_preserve_bytes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(concat!("copilot-", "two-copies"));
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
                .join("../../target/tb-test-marks/copilot")
                .join(path.parent().unwrap().file_name().unwrap()),
        )
    }

    fn has_unowned_entries(document: &Value) -> bool {
        owners(document).is_err()
    }

    fn exe() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-fixtures/Program Files/Folio/folio.exe")
    }

    /// PIN (R1-17) — **the probe names the program it starts, absolutely.**
    ///
    /// `cmd /c copilot --version` is a bare name, and `cmd` resolves a bare
    /// name out of its own working directory before it looks at `PATH`. The
    /// working directory of this process is whatever the shell that started
    /// Folio was standing in, so a `copilot.cmd` sitting in a folder somebody
    /// cloned ran the moment Settings ▸ Agents was opened — no press on it, no
    /// mention of it, and the page that ran it is the page about agents.
    ///
    /// So the search is this window's own ([`bt_platform::program_on_path`],
    /// which never looks at a working directory) and what `cmd` is handed is
    /// the answer, quoted, with nothing left for it to resolve.
    ///
    /// MUTATION: pass the bare name and the tail below stops naming a path.
    #[cfg(windows)]
    #[test]
    fn the_probe_hands_cmd_a_path_and_never_a_name_to_look_up() {
        let tail = probe_command_tail(Path::new(r"C:\Users\a b\AppData\npm\copilot.cmd"));
        assert_eq!(
            tail, "/c \"\"C:\\Users\\a b\\AppData\\npm\\copilot.cmd\" --version\"",
            "one quoted absolute program and one argument"
        );
        assert!(
            !tail.contains(" copilot "),
            "nothing left for cmd to search for: {tail}"
        );
    }

    /// **The consent disclosure names the file this machine will write** —
    /// [`crate::attention_hooks`]'s pin over `COPILOT_HOME`, both halves.
    #[test]
    fn the_tip_names_the_default_spelling_until_the_variable_moves_it() {
        let named = |text: &str| Some(OsString::from(text));
        assert_eq!(
            hooks_path_shown_from(None, named(r"C:\Users\someone")),
            "~/.copilot/hooks/folio.json"
        );
        assert_eq!(
            hooks_path_shown_from(named(""), named(r"C:\Users\someone")),
            "~/.copilot/hooks/folio.json"
        );
        assert_eq!(
            hooks_path_shown_from(None, None),
            "~/.copilot/hooks/folio.json"
        );
        assert_eq!(
            hooks_path_shown_from(
                named(r"D:\scratch\copilot-home"),
                named(r"C:\Users\someone")
            ),
            PathBuf::from(r"D:\scratch\copilot-home")
                .join(HOOKS_DIRECTORY)
                .join(HOOKS_FILE)
                .display()
                .to_string()
        );
    }

    /// **Only the user's own file, and never a repository's.**
    ///
    /// The red form of plan §10.4.3's clause said about the family it matters most for: copilot
    /// loads hooks from `.github/hooks/`, from two `.github/copilot/settings*.json`, and from
    /// *other people's* `.claude/settings*.json` — all of them inside whatever folder happens to be
    /// open. There is exactly one path this module can produce, it comes from the variable copilot
    /// itself reads, and no working directory reaches it.
    #[test]
    fn the_only_file_this_writes_is_the_one_the_environment_names() {
        let named = |text: &str| Some(OsString::from(text));
        assert_eq!(
            config_dir_from(
                named(r"D:\scratch\copilot-home"),
                named(r"C:\Users\someone")
            ),
            Some(PathBuf::from(r"D:\scratch\copilot-home"))
        );
        // Set-but-empty is not set.
        assert_eq!(
            config_dir_from(named(""), named(r"C:\Users\someone")),
            Some(PathBuf::from(r"C:\Users\someone").join(DEFAULT_DIRECTORY))
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, named("")), None);
        assert!(hooks_path().is_none_or(|path| path.ends_with(HOOKS_FILE)));
        assert!(
            hooks_path().is_none_or(|path| path
                .parent()
                .is_some_and(|parent| parent.ends_with(HOOKS_DIRECTORY))),
            "upstream loads *.json from the hooks directory, and only from there"
        );
        // **Nothing in this module can be steered by where the process happens to be standing**,
        // asserted over the source because that is the only form the rule has. The names are
        // spelled in halves so this assertion is not its own counter-example.
        let source = include_str!("attention_copilot.rs");
        for reaching in [concat!("current", "_dir"), concat!("project", "_dir")] {
            assert!(
                !source.contains(reaching),
                "`{reaching}` would let a working directory decide where automation is written"
            );
        }
        assert!(
            config_dir_from(named(r"D:\scratch\copilot-home"), None)
                .is_some_and(|path| path.is_absolute())
        );
    }

    /// **The file this writes into somebody's own directory, spelled out.**
    ///
    /// `attention_hooks::the_block_this_installs_is_this`'s reason over a third document: what is
    /// written is *not ours*. It lands in a directory the user owns, that another program reads,
    /// and that nobody will look at again — so a matcher dropped, a column renamed or a timeout
    /// silently back at upstream's thirty should have to be typed here on purpose.
    #[test]
    fn the_file_this_installs_is_this() {
        assert_eq!(
            serde_json::to_string_pretty(&document_for_on(
                Path::new(r"C:\folio\folio.exe"),
                HostPlatform::Windows
            ))
            .expect("render"),
            EXPECTED_FILE.trim_end()
        );
    }

    /// See [`the_file_this_installs_is_this`].
    const EXPECTED_FILE: &str = include_str!("../../../docs/plans/attention/copilot-hooks.json");

    /// **The two waits are one hook narrowed by a matcher, and the four completions never arrive.**
    ///
    /// The red form of the family's own finding: `notification` fires for six subtypes and a
    /// matcher-less entry receives all six. Four of those are things that have *finished*, and this
    /// block spent four recordings proving a finished thing is not a waiting one — so the matcher
    /// is the difference between a wait row and a lie.
    ///
    /// MUTATION: drop the `matcher` key and `shell_completed` lights a pane as waiting for you
    /// every time a background command ends.
    #[test]
    fn the_notification_hook_is_narrowed_to_the_two_subtypes_that_are_waits() {
        let document = document_for(&exe());
        let hooks = document["hooks"].as_object().expect("hooks");
        assert!(
            !hooks.keys().any(|key| key.contains('.')),
            "`notification.permission_prompt` is a Folio spelling, not an upstream event name"
        );
        let notification = hooks["notification"].as_array().expect("array");
        let matchers = notification
            .iter()
            .map(|entry| entry.get("matcher").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            matchers,
            [Some("permission_prompt"), Some("elicitation_dialog")],
            "the bare subtype name, because upstream anchors it as `^(?:PATTERN)$` itself"
        );
        for refused in [
            "agent_idle",
            "agent_completed",
            "shell_completed",
            "shell_detached_completed",
        ] {
            assert!(
                !matchers.contains(&Some(refused)),
                "{refused} is a completion, and a completion is not somebody waiting for you"
            );
        }
        // And the three clears are registered under their own event names, unqualified.
        for event in ["userPromptSubmitted", "agentStop", "sessionEnd"] {
            let entries = hooks[event].as_array().expect("array");
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].get("matcher"), None, "{event} has no subtypes");
        }
        assert_eq!(hooks.len(), 4, "four upstream event names, five rows");
    }

    /// **Every command names its family and its event, and none carries a payload.**
    #[test]
    fn every_command_says_which_upstream_it_speaks_for_and_hands_over_nothing() {
        let document = document_for(&exe());
        for entries in document["hooks"].as_object().unwrap().values() {
            for entry in entries.as_array().unwrap() {
                assert_eq!(entry["exec"], exe().to_string_lossy().as_ref());
                assert_eq!(entry["type"], "command");
                assert_eq!(entry["timeoutSec"], TIMEOUT_SECONDS);
                assert_eq!(entry["args"][0], "attention");
                assert!(entry["args"][1].as_str().unwrap().starts_with("copilot:"));
                assert_eq!(entry["args"].as_array().unwrap().len(), 2);
                for shell in ["bash", "powershell", "command"] {
                    assert!(entry.get(shell).is_none());
                }
            }
        }
    }

    /// Every platform gets the same executable and argv, with metacharacters literal.
    #[test]
    fn a_mac_install_writes_direct_exec_without_shell_quoting() {
        let exe = Path::new("/Users/a $ ` ' 中文/Folio.app/Contents/MacOS/folio");
        let mac = document_for_on(exe, HostPlatform::MacOs);
        assert_eq!(mac, document_for_on(exe, HostPlatform::Windows));
        for entries in mac["hooks"].as_object().unwrap().values() {
            for entry in entries.as_array().unwrap() {
                assert_eq!(entry["exec"], exe.to_str().unwrap());
            }
        }
    }

    /// **What is written never carries two layers of one request**, and for this family that is a
    /// statement about upstream rather than about a choice: it publishes one layer of each signal.
    #[test]
    fn what_is_written_never_carries_two_layers_of_one_request() {
        let rows = rows_to_install();
        assert_eq!(crate::attention::duplicated_tier(&rows), None);
        for row in rows.iter().filter(|row| row.is_wait()) {
            assert!(matches!(
                row.action,
                MappedAction::Wait {
                    tier: Tier::Primary
                }
            ));
        }
        // And the file agrees with the table, read back the way an arrival will read it.
        assert_eq!(rows_declared_by(&document_for(&exe())), rows);
    }

    /// **Install, then uninstall, and the directory is the one that was there.**
    ///
    /// Over the real filesystem rather than over a value, because for this installer the file
    /// *itself* is the unit: there is no document of somebody else's to be handed back intact, and
    /// what has to be true instead is that nothing is left behind at all.
    #[test]
    fn taking_it_back_out_leaves_the_directory_that_was_there() {
        let home = scratch("identity");
        let hooks = home.join(HOOKS_DIRECTORY);
        let path = hooks.join(HOOKS_FILE);
        std::fs::create_dir_all(&hooks).expect("a scratch directory");
        // A file of the user's own, beside ours, which upstream also loads and this must not touch.
        let theirs = hooks.join("mine.json");
        std::fs::write(&theirs, "{\"version\":1}\n").expect("their file");

        std::fs::write(&path, format!("{}\n", render(&document_for(&exe())))).expect("install");
        assert!(declares_folio(&read(&path)));
        assert_eq!(rows_declared_by(&read(&path)), rows_to_install());
        std::fs::remove_file(&path).expect("uninstall");

        assert!(!path.exists(), "nothing of ours is left behind");
        assert_eq!(
            std::fs::read_to_string(&theirs).expect("their file"),
            "{\"version\":1}\n",
            "a hook file beside ours is one upstream also runs, and it is not ours to edit"
        );
        assert_eq!(std::fs::read_dir(&hooks).expect("the directory").count(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// RED — **a hook file that could not be read is left byte for byte.**
    ///
    /// Release audit 2026-09-16 (C-3), the third site of the same conflation:
    /// `read_to_string(&path).ok()` answered `None` both for a file that is not there and for one
    /// this build could not read, so an unreadable `folio.json` was written over on install — and
    /// on uninstall was reported as "nothing to do" about a file nobody had read.
    ///
    /// RED GATE: drop the `Standing::Unreadable` arm from `apply_to` and the install replaces it.
    #[test]
    fn a_hook_file_that_could_not_be_read_is_never_written_over() {
        let home = scratch("unreadable");
        let hooks = home.join(HOOKS_DIRECTORY);
        let path = hooks.join(HOOKS_FILE);
        std::fs::create_dir_all(&hooks).expect("a scratch directory");
        // A Latin-1 byte inside a string: an ordinary file on an ordinary machine, and not UTF-8.
        let theirs: &[u8] = b"{\"version\":1,\"note\":\"caf\xe9\"}\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        let installing = apply_to(&path, true, &exe());
        assert_eq!(installing, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            std::fs::read_dir(&hooks).expect("the directory").count(),
            1,
            "nothing was written beside it either"
        );
        // And the row drawn from it says so, rather than offering to write over it.
        assert_eq!(state_at(&path), State::Unreadable);
        // Nor is it removed: uninstalling a file this build never read deletes somebody's own.
        let taking_it_out = apply_to(&path, false, &exe());
        assert_eq!(taking_it_out, Outcome::Refused(UNREADABLE));
        assert!(path.is_file(), "it is still there");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **Somebody else's `folio.json` is never written over**, and neither is one this build
    /// cannot read.
    #[test]
    fn a_file_at_our_name_that_is_not_ours_is_left_exactly_where_it_is() {
        let theirs: Value =
            serde_json::from_str(r#"{"version":1,"hooks":{"agentStop":[{"command":"say done"}]}}"#)
                .expect("fixture");
        assert!(!declares_folio(&theirs));
        assert!(rows_declared_by(&theirs).is_empty());
        // `apply` refuses, and the sentence is the one the user reads.
        let source = include_str!("attention_copilot.rs");
        assert!(source.contains("a hook file of your own already stands under that name"));
        assert!(source.contains("is not one this build can read"));
        // And the entry test looks at all three columns, so a user who wrote their command in the
        // cross-platform `command` field is still recognised as themselves.
        for column in ["bash", "powershell", "command"] {
            let mine: Value = serde_json::from_str(&format!(
                r#"{{"{column}":"folio.exe attention copilot:agentStop"}}"#
            ))
            .expect("fixture");
            assert!(
                entry_owner(&mine).is_err(),
                "a bare program cannot prove ownership: {column}"
            );
        }
        assert!(!entry_is_ours(
            &serde_json::json!({ "powershell": "say done" })
        ));
        // **And the guard is per entry, not per file**, which is the case a name alone would have
        // missed: a file that carries our block *and* one line somebody added by hand is a file
        // where install overwrites their line and uninstall deletes it, and this module keeps no
        // memory of what it replaced.
        assert!(has_unowned_entries(&theirs));
        assert!(!has_unowned_entries(&document_for(&exe())));
        let mut mixed = document_for(&exe());
        mixed["hooks"]["agentStop"]
            .as_array_mut()
            .expect("array")
            .push(serde_json::json!({ "powershell": "say done" }));
        assert!(
            declares_folio(&mixed),
            "it does carry our block, which is exactly why the name is not enough"
        );
        assert!(has_unowned_entries(&mixed));
        // A document with no `hooks` at all is somebody's, whatever else it says.
        assert!(has_unowned_entries(&serde_json::json!({ "version": 1 })));
    }

    /// **A version proved too old is refused; an absent answer is not.**
    ///
    /// The gate the changelog bought: before `1.0.26`, `permission_prompt` "fires for every tool
    /// execution in interactive mode, even when the tool has already been approved for the
    /// session". Installing into that writes a hook that says *somebody is waiting for you* about
    /// a tool call nobody was asked about.
    ///
    /// MUTATION: make `None` refuse and every machine without `copilot` on its path loses a switch
    /// it was entitled to throw before installing one.
    #[test]
    fn only_a_version_proved_too_old_is_refused() {
        let version = |major, minor, patch| {
            Some(Version {
                major,
                minor,
                patch,
            })
        };
        assert_eq!(readiness_from(None, false), Readiness::Unknown);
        assert_eq!(readiness_from(None, true), Readiness::Unknown);
        assert_eq!(readiness_from(version(1, 0, 25), false), Readiness::TooOld);
        assert_eq!(
            readiness_from(version(1, 0, 25), true),
            Readiness::TooOld,
            "the older of two problems is the one the reader has to fix first"
        );
        assert_eq!(readiness_from(version(1, 0, 26), false), Readiness::Ready);
        assert_eq!(readiness_from(version(1, 0, 80), false), Readiness::Ready);
        assert_eq!(
            readiness_from(version(0, 9, 99), false),
            Readiness::TooOld,
            "the comparison is on all three parts and not on the last"
        );
        assert_eq!(
            readiness_from(version(1, 0, 80), true),
            Readiness::HooksDisabled,
            "installed into perfectly and silent anyway is the one failure that looks exactly \
             like not having installed"
        );
        assert_eq!(
            MINIMUM,
            Version {
                major: 1,
                minor: 0,
                patch: 26
            }
        );
    }

    /// **What the machine actually said**, parsed.
    ///
    /// The first line is verbatim from a real `1.0.80` on 2026-08-26, trailing full stop included,
    /// and it is the reason this is a scan rather than a `split('.')`.
    ///
    /// MUTATION: parse the whole word and the real machine reads as no version at all, which puts
    /// every copilot on earth into `Unknown` and silently removes the gate.
    #[test]
    fn the_version_is_read_out_of_a_sentence_and_not_out_of_a_field() {
        assert_eq!(
            Version::parse(
                "GitHub Copilot CLI 1.0.80.\nRun 'copilot update' to check for updates."
            ),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 80
            })
        );
        assert_eq!(
            Version::parse("v1.0.26"),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 26
            })
        );
        assert_eq!(
            Version::parse("1.0.81-12"),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 81
            }),
            "upstream ships prereleases on this channel and they are that version, not none"
        );
        assert_eq!(Version::parse(""), None);
        assert_eq!(
            Version::parse("'copilot' is not recognized as an internal or external command"),
            None,
            "the sentence a machine without copilot answers with is not a version"
        );
        assert_eq!(Version::parse("1.0"), None, "two parts is not three");
        assert!(
            Version {
                major: 1,
                minor: 0,
                patch: 9
            } < Version {
                major: 1,
                minor: 0,
                patch: 26
            },
            "the numbers order as numbers and not as text"
        );
    }

    /// The user's own switch, read out of their own file and never written to it.
    #[test]
    fn the_settings_file_is_read_and_never_written() {
        let source = include_str!("attention_copilot.rs");
        assert!(
            !source.contains(concat!("write(&settings", "_path")),
            "the user's settings file is somebody else's and this module only reads it"
        );
        for (text, off) in [
            (r#"{"disableAllHooks":true}"#, true),
            (r#"{"disableAllHooks":false}"#, false),
            (r#"{"beep":true}"#, false),
            ("{}", false),
        ] {
            let settings: Value = serde_json::from_str(text).expect("fixture");
            assert_eq!(
                settings
                    .get(DISABLE_ALL_HOOKS)
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                off,
                "{text}"
            );
        }
        assert!(settings_path().is_none_or(|path| path.ends_with(SETTINGS_FILE)));
    }

    fn render(document: &Value) -> String {
        serde_json::to_string_pretty(document).expect("render")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
    }

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("folio-copilot-{name}-{}", std::process::id()));
        path
    }
}
