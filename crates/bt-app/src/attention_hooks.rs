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

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::attention::MappingRow;
use crate::attention_map::{self, CLAUDE_CODE};

/// The variable Claude Code reads for its configuration directory.
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";

/// The file inside it that holds **user-level** settings.
const SETTINGS_FILE: &str = "settings.json";

/// The default directory's name beside the user's profile, for when the variable says nothing.
const DEFAULT_DIRECTORY: &str = ".claude";

/// The substring that marks a hook entry as ours.
///
/// The **verb and the family**, not the path to the executable: a user who moves Folio, or who runs
/// two builds of it, still has one set of entries that this can recognise and take back out. An
/// entry that does not contain this is somebody else's and is never touched, which is what makes
/// uninstalling safe on a machine with hooks of its own.
const MARK: &str = "attention claude-code:";

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
}

/// The directory Claude Code keeps user configuration in, as **this environment** says it.
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os(CONFIG_DIR_VARIABLE),
        std::env::var_os("USERPROFILE"),
    )
}

/// The same decision, with the environment handed in.
///
/// Split out so the rule can be pinned by a test that sets nothing: a process-wide variable changed
/// from a test is changed for every other test running beside it, and this crate refuses `unsafe`,
/// which is what `set_var` now is. The one impure input is named instead — the shape `cli::resolve`
/// uses for the filesystem, and for the same reason.
#[must_use]
fn config_dir_from(named: Option<OsString>, profile: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    Some(PathBuf::from(profile.filter(|profile| !profile.is_empty())?).join(DEFAULT_DIRECTORY))
}

/// The user-level settings file. **The only file this module ever writes.**
#[must_use]
pub(crate) fn settings_path() -> Option<PathBuf> {
    Some(config_dir()?.join(SETTINGS_FILE))
}

/// What that file says today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(path) = settings_path() else {
        return State::Absent;
    };
    match std::fs::read_to_string(&path) {
        Err(_) => State::Absent,
        Ok(text) if text.trim().is_empty() => State::Absent,
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(settings) if settings.is_object() => {
                if declares_folio(&settings) {
                    State::Installed
                } else {
                    State::Absent
                }
            }
            _ => State::Unreadable,
        },
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
    let Ok(text) = std::fs::read_to_string(&path) else {
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
            hooks.iter().any(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(MARK))
            })
        })
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

/// One hook command line.
///
/// The executable is quoted and the event is the qualified `<family>:<event>` spelling, so the
/// receiving end never has to work out which upstream a bare `Stop` came from.
///
/// **The payload is not passed.** Nothing in the shipped tables declares an identifier to take out
/// of one, so there is nothing to send — and a command line that interpolated a hook payload would
/// be a command line an upstream could put a quote character into.
#[must_use]
pub(crate) fn command_for(exe: &Path, event: &str) -> String {
    format!("\"{}\" attention {CLAUDE_CODE}:{event}", exe.display())
}

/// Write Folio's hooks into a settings value, replacing any it had before.
///
/// Returns whether anything changed, so that a press on an already-installed machine costs no write
/// at all — the same reason `shell_integration` compares before it writes.
///
/// **Everything that is not ours is preserved**, including hook entries under the same event names:
/// the removal below is by mark, and the insertion appends a group rather than replacing the array.
pub(crate) fn install_into(settings: &mut Value, exe: &Path) -> bool {
    let before = settings.clone();
    remove_from(settings);
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
        hook.insert("command".to_owned(), command_for(exe, row.event).into());
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
pub(crate) fn remove_from(settings: &mut Value) -> bool {
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
        groups.retain(|group| !group_is_ours(group));
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

/// What happened when the row was pressed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    /// Written. The file is now what [`State::Installed`] describes.
    Installed,
    /// Taken back out.
    Removed,
    /// Nothing to do — it was already in the state that was asked for.
    Unchanged,
    /// Refused, with the reason in the caller's own words.
    Refused(&'static str),
}

/// Put Folio's hooks in, or take them out, on this machine.
///
/// **The file is read, changed and written whole**, and a copy of what was there is kept beside it
/// the first time each day — `shell_integration`'s rule, for `shell_integration`'s reason: this is
/// somebody's own configuration file, and a build that could damage one had better be able to hand
/// it back.
pub(crate) fn apply(install: bool, exe: &Path) -> Outcome {
    let Some(path) = settings_path() else {
        return Outcome::Refused("no user configuration directory to write into");
    };
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut settings = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_str::<Value>(&existing) {
            Ok(value) if value.is_object() => value,
            // Refused rather than replaced. A settings file this build cannot read is a settings
            // file somebody wrote, and overwriting it to add a convenience is not a trade anyone
            // agreed to.
            _ => return Outcome::Refused("the settings file is not one this build can read"),
        }
    };
    let changed = if install {
        install_into(&mut settings, exe)
    } else {
        remove_from(&mut settings)
    };
    if !changed {
        return Outcome::Unchanged;
    }
    if !existing.is_empty() {
        let backup = path.with_extension(format!("json.bak-{}", today()));
        if !backup.exists() {
            let _ = std::fs::write(&backup, &existing);
        }
    }
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Outcome::Refused("the user configuration directory could not be created");
    }
    let text = match serde_json::to_string_pretty(&settings) {
        Ok(text) => text,
        Err(_) => return Outcome::Refused("the settings could not be written back"),
    };
    if std::fs::write(&path, format!("{text}\n")).is_err() {
        return Outcome::Refused("the settings file could not be written");
    }
    if install {
        Outcome::Installed
    } else {
        Outcome::Removed
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{MappedAction, Tier, WaitKind};

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Folio\folio.exe")
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
        assert!(remove_from(&mut settings));
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
        assert!(remove_from(&mut settings));
        assert!(
            !remove_from(&mut settings),
            "and neither is a removal of what is not there"
        );
    }

    /// Moving the executable is not a reason to lose track of the entries.
    #[test]
    fn entries_are_recognised_by_what_they_do_not_by_where_folio_lives() {
        let mut settings = installed();
        let moved = PathBuf::from(r"E:\portable\folio.exe");
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
        assert!(!remove_from(&mut array));
    }

    /// The command carries the family, so a bare `Stop` is never ambiguous.
    #[test]
    fn every_command_says_which_upstream_it_speaks_for() {
        for row in rows_to_install() {
            let command = command_for(&exe(), row.event);
            assert!(command.contains(MARK), "{command}");
            assert!(
                command.contains(&format!("{CLAUDE_CODE}:{}", row.event)),
                "{command}"
            );
            assert!(
                !command.contains("--json"),
                "no shipped row declares an identifier, so no payload is passed: {command}"
            );
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
        assert!(install_into(
            &mut settings,
            Path::new(r"C:\folio\folio.exe")
        ));
        assert_eq!(
            serde_json::to_string_pretty(&settings).expect("render"),
            EXPECTED_BLOCK.trim_end()
        );
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
}
