//! **Installing codex's `notify` program, into the user's own configuration and nowhere else.**
//!
//! One settings row presses this, and it is [`attention_hooks`](crate::attention_hooks)'s contract
//! over a second upstream and a second file format: only the user-level file is ever written, a file
//! this build cannot read is never written over, and installing and then uninstalling hands back the
//! document that was there — comments, key order and blank lines included.
//!
//! # What is installed, and why it is this and not a hook
//!
//! `docs/plans/attention/evidence-cli-survey-2026-08-25.md` §2.6 surveyed codex's two configuration
//! surfaces. The one this module writes is the **older** of them, and the survey's own reading is
//! why: `notify` is a fire-and-forget external program whose payload has exactly one observed
//! `type`, `agent-turn-complete`, and which was **silent** through the whole of a recording that sat
//! on a real approval box. It says *I have finished talking*, and the block spent four recordings
//! proving that sentence is not *it is standing there waiting for you*.
//!
//! So it is installed against the **event lane** and never against the queue: the row it reaches is
//! `(CODEX, "agent-turn-complete", Via::Notify)` in [`attention_map::TURN_END`], which mints no
//! episode, takes no place in the queue and raises no `grounds`. What it does is announce the end of
//! a turn — the third of the four sources that say that sentence, beside a hooked Claude Code's
//! `Stop`, a bare bell, and pi's `agent_settled` — under the one `Turn finished` switch that governs
//! all of them.
//!
//! codex's **hooks** are the other surface, and `permission-request` there is the event that would
//! reach the queue. It is not this module: an installer for it has a trust gate to explain (codex
//! ledgers non-managed hooks by hash and the user has to authorise them once), and the survey filed
//! that as its own slice.
//!
//! # The one line
//!
//! ```toml
//! notify = ["<folio.exe>", "attention", "codex:agent-turn-complete", "--json"]
//! ```
//!
//! codex **appends one argument** — the payload, as a JSON string — to whatever this array says
//! before spawning it. Ending the array on `--json` is therefore not decoration: it puts the payload
//! exactly where `cli::attention`'s own grammar already expects a payload, so nothing about the verb
//! has to learn that one caller passes its argument positionally. Without it the payload arrives as
//! a second bare word and the verb refuses the call as two events in one.
//!
//! # Where the file is, and why it is asked for rather than composed
//!
//! §7.1.6j's lesson, the same one [`attention_hooks`](crate::attention_hooks) states: a machine with
//! a redirected profile folder gets told "not installed" about something installed for months, if
//! the path was *composed* rather than asked for. `CODEX_HOME` is the variable codex itself reads —
//! the survey's own harness set it to point the recordings at the real installation — so it is what
//! is read here, and only otherwise the documented default beside the user's profile.
//!
//! # The user's own `notify` is never taken away
//!
//! A `notify` this module did not write means somebody has a program of their own on that key.
//! There is one such key, so installing over it would delete their configuration, and uninstalling
//! could not give it back — this module keeps no memory of what it replaced, and a build that kept
//! one would be a build with a second copy of somebody else's file. So it refuses, out loud, and the
//! row stays where the machine actually is.

pub(crate) use crate::attention_ownership::Outcome;
use crate::attention_ownership::{self as ownership, Decision};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Value};

use crate::attention_map::CODEX;
use crate::cli::ATTENTION_VERB;

/// The variable codex reads for its configuration directory.
const HOME_VARIABLE: &str = "CODEX_HOME";

/// The file inside it that holds **user-level** configuration.
const CONFIG_FILE: &str = "config.toml";

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

/// The event this build asks to be told about, in the wire's `<family>:<event>` spelling.
///
/// The only `type` the survey ever observed in a `notify` payload, quoted from
/// `evidence-cli-survey-2026-08-25.md` §2.6 ①, and already a row of
/// [`attention_map::TURN_END`](crate::attention_map::TURN_END).
const EVENT: &str = "agent-turn-complete";

/// The flag the payload lands on. See the module header.
const JSON_FLAG: &str = "--json";

/// Whether this machine's user configuration already calls Folio.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    /// A configuration file whose `notify` is not ours — or no file at all, or no `notify` at all,
    /// which are the same answer to the only question being asked.
    Absent,
    /// Our program is there.
    Installed,
    /// There is a file and it could not be read as TOML. **Not** "absent": writing over a file this
    /// build cannot parse would destroy configuration somebody wrote by hand.
    Unreadable,
    /// There is a readable file and **this build will not edit it**, for the reason carried: a
    /// link out of the agent's own folder, a file shared by hard links, or a read-only one. See
    /// `attention_hooks::Standing::Refused`.
    Refused(&'static str),
}

/// The directory codex keeps user configuration in, as **this environment** says it.
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

/// The user-level configuration file. **The only file this module ever writes.**
#[must_use]
pub(crate) fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

/// The same file, spelled for a reader who is being asked to consent to it —
/// [`crate::attention_hooks::settings_path_shown`]'s rule over `CODEX_HOME`.
#[must_use]
pub(crate) fn config_path_shown() -> String {
    config_path_shown_from(
        std::env::var_os(HOME_VARIABLE),
        std::env::var_os(bt_platform::home_variable()),
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

/// What that file says today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(path) = config_path() else {
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
    let Some(value) = text.parse::<DocumentMut>().ok() else {
        return State::Unreadable;
    };
    if (notify_owner(&value)
        .ok()
        .flatten()
        .into_iter()
        .collect::<Vec<_>>())
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

/// The same question about a named file, so a test can ask it without a codex installation on the
/// machine it runs on.
#[must_use]
fn state_at(path: &Path) -> State {
    let config = match crate::attention_hooks::Config::resolve(path) {
        Ok(config) => config,
        // Read, and not ours to change. The row says which file it is looking at.
        Err(crate::attention_hooks::Unresolved::Refused(reason)) => return State::Refused(reason),
        Err(crate::attention_hooks::Unresolved::Unreadable) => return State::Unreadable,
    };
    let text = match config.standing() {
        // No file is the same answer to the only question being asked.
        crate::attention_hooks::Standing::Nothing => return State::Absent,
        // **Not `Absent`.** There is a file, and a row that said "not installed" about it would
        // offer to write over one this build never read.
        crate::attention_hooks::Standing::Unreadable => return State::Unreadable,
        crate::attention_hooks::Standing::Text(text) => text,
    };
    match text.parse::<DocumentMut>() {
        Ok(document) => {
            if declares_folio(&document) {
                State::Installed
            } else {
                State::Absent
            }
        }
        Err(_) => State::Unreadable,
    }
}

/// The array one install writes, given where this build lives.
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
fn words_of(document: &DocumentMut) -> Option<Vec<String>> {
    document
        .get(NOTIFY_KEY)?
        .as_array()?
        .iter()
        .map(|word| word.as_str().map(str::to_owned))
        .collect()
}

/// Whether the exact Folio argv shape is present. `notify_owner` is the
/// ownership decoder; all mutations additionally check its executable path.
fn declares_folio(document: &DocumentMut) -> bool {
    notify_owner(document).is_ok_and(|owner| owner.is_some())
}

/// The sole Codex attribution site: argv[0], with the exact Folio verb/event shape.
fn notify_owner(document: &DocumentMut) -> Result<Option<PathBuf>, &'static str> {
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
    // somebody's own that speaks this verb is theirs, and `declares_somebody_else` then keeps this
    // module's oldest promise about it (re-review B2).
    let Some(owner) = ownership::folio_operand(&words[0]) else {
        return Ok(None);
    };
    if words.len() != 4 || words[2] != format!("{CODEX}:{EVENT}") || words[3] != JSON_FLAG {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(Some(owner))
}

/// **Whether somebody else's program is on the key.**
///
/// The one state this module refuses to act on. See the module header: there is one `notify` key,
/// so writing ours over theirs deletes a program this build cannot give back.
fn declares_somebody_else(document: &DocumentMut) -> bool {
    document.get(NOTIFY_KEY).is_some() && !declares_folio(document)
}

/// Write Folio's program onto the key, replacing one of ours it had before.
///
/// Returns whether anything changed, so that a press on an already-installed machine costs no write
/// at all — `attention_hooks`'s rule, for `shell_integration`'s reason.
fn install_into(document: &mut DocumentMut, exe: &Path) -> bool {
    if declares_somebody_else(document) {
        return false;
    }
    let mut array = Array::new();
    for word in program_for(exe) {
        array.push(word);
    }
    let written = Item::Value(Value::Array(array));
    // Compare argv, not TOML decoration/quoting, so a disk round trip is a no-op.
    if words_of(document).as_ref() == Some(&program_for(exe)) {
        return false;
    }
    document[NOTIFY_KEY] = written;
    true
}

/// Take Folio's program back off the key, leaving everything else exactly as it was.
///
/// Symmetric with [`install_into`] and tested as such, byte for byte, on a document with the user's
/// own comments and tables in it.
fn remove_from(document: &mut DocumentMut, exe: &Path) -> bool {
    if notify_owner(document)
        .ok()
        .flatten()
        .is_none_or(|owner| ownership::other_live(&owner, exe) != Ok(false))
    {
        return false;
    }
    document.remove(NOTIFY_KEY).is_some()
}

/// Put Folio's `notify` in, or take it out, on this machine.
///
/// **The file is read, changed and written whole**, and a copy of what was there is kept beside it
/// the first time each day — `attention_hooks`'s rule, for its reason: this is somebody's own
/// configuration file, and a build that could damage one had better be able to hand it back.
pub(crate) fn apply(decision: Decision, exe: &Path) -> Outcome {
    let Some(path) = config_path() else {
        return Outcome::Refused("no codex configuration directory to write into");
    };
    apply_at(&path, decision, exe, &crate::persist::storage_dir())
}

/// The same act on a named file — the seam the tests press, so that what they pin is this function
/// and not a `config.toml` belonging to whoever runs them.
pub(crate) fn apply_at(path: &Path, decision: Decision, exe: &Path, data: &Path) -> Outcome {
    match crate::attention_hooks::Config::resolve(path) {
        Ok(config) => apply_resolved(&config, decision, exe, data),
        Err(crate::attention_hooks::Unresolved::Refused(reason)) => Outcome::Refused(reason),
        Err(crate::attention_hooks::Unresolved::Unreadable) => Outcome::Refused(UNREADABLE),
    }
}

/// The same act on a configuration this operation has already resolved.
///
/// **Everything below the entry takes this value.** The path was resolved once, at the top; the
/// read, the dated copy, the replace and the removal all name that one answer, and there is no
/// second resolution on this path for a repointed link to slip through (re-review B1).
pub(crate) fn apply_resolved(
    config: &crate::attention_hooks::Config,
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
        crate::attention_hooks::Standing::Text(text) => text,
        // Nothing there yet: the install creates the file, and there is nothing to keep beside it.
        crate::attention_hooks::Standing::Nothing => String::new(),
        // **A file that could not be read is never written over.** The empty string this used to
        // fall back to parses as an empty document, so the refusal below never fired and the write
        // went ahead over somebody's own configuration — release audit 2026-09-16 (C-3).
        crate::attention_hooks::Standing::Unreadable => return Outcome::Refused(UNREADABLE),
    };
    let mut document = match existing.parse::<DocumentMut>() {
        Ok(document) => document,
        // Refused rather than replaced. A configuration file this build cannot read is a file
        // somebody wrote, and overwriting it to add a convenience is not a trade anyone agreed to.
        Err(_) => return Outcome::Refused(UNREADABLE),
    };
    if document.get(NOTIFY_KEY).is_some() && words_of(&document).is_none() {
        return Outcome::Refused(UNREADABLE);
    }
    // **Ownership is per entry** (closure review R4), and this family's entry is the whole key: a
    // `notify` wearing Folio's verb in a shape this build cannot decode is not Folio's, which is
    // the answer [`declares_folio`] and [`remove_from`] have always given about it. An install
    // over it is refused below as somebody else's program; a removal leaves it alone.
    let paths = notify_owner(&document)
        .ok()
        .flatten()
        .into_iter()
        .collect::<Vec<_>>();
    if install && declares_somebody_else(&document) {
        return Outcome::Refused("codex already runs a notify program of your own");
    }
    let others = match ownership::check(&paths, exe, &decision) {
        Ok(others) => others,
        Err(outcome) => return outcome,
    };
    let changed = if install {
        install_into(&mut document, exe)
    } else {
        remove_from(&mut document, exe)
    };
    let _record = if install {
        match ownership::record(
            data,
            config.named().parent().expect("absolute config"),
            "codex",
        ) {
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
    // The atomic write, mandatory backup, and unsafe-path refusal are all
    // `attention_hooks::land`'s, said once for all three installers.
    match config.land(&existing, "toml", rendered(&document, &existing).as_bytes()) {
        crate::attention_hooks::Landing::Landed => {}
        crate::attention_hooks::Landing::NoDirectory => {
            return Outcome::Refused("the codex configuration directory could not be created");
        }
        crate::attention_hooks::Landing::NoBackup => {
            return Outcome::Refused(crate::attention_hooks::NO_BACKUP);
        }
        crate::attention_hooks::Landing::Changed => {
            return Outcome::Refused(crate::i18n::Text::AgentConfigChanged.text());
        }
        crate::attention_hooks::Landing::NotWritten => {
            return Outcome::Refused("the codex configuration file could not be written");
        }
    }
    if install {
        Outcome::Installed
    } else if others.is_empty() {
        Outcome::Removed
    } else {
        Outcome::LeftOther(others)
    }
}

/// The bytes to write, given what was read.
///
/// **A file that ended without a newline still ends without one** (found on a real machine,
/// 2026-08-25: install-then-uninstall handed back the file and one extra byte). `toml_edit` renders
/// a document with a line ending after its last item, which is the right default for a document it
/// is creating and a change to a document it was handed. One byte is exactly the difference between
/// "uninstalling gives the file back" and "uninstalling gives the file back and a newline", and the
/// promise this module makes is the first one.
///
/// Exactly one, and only when the reader's own file had none: a file that ended in two blank lines
/// still ends in two, because that is what `existing.ends_with` already answers.
#[must_use]
fn rendered(document: &DocumentMut, existing: &str) -> String {
    let mut text = document.to_string();
    if !existing.is_empty() && !existing.ends_with('\n') && text.ends_with('\n') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_two_live_copies_require_takeover_and_preserve_bytes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(concat!("codex-", "two-copies"));
        std::fs::create_dir_all(&root).unwrap();
        // **Two folders, one file name.** An operand is Folio's only if its file name is one
        // Folio installs itself under, which is how two real copies differ: same program, two
        // places. The odd characters this fixture exists for move to the folder.
        let a = root.join("A space $ ` ' 中文").join("folio.exe");
        let b = root.join("B").join("folio.exe");
        for copy in [&a, &b] {
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(copy, b"a copy of Folio").unwrap();
        }
        let path = root.join("config.toml");
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
                .join("../../target/tb-test-marks/codex")
                .join(path.parent().unwrap().file_name().unwrap()),
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

    /// **Only the user's own file, and never a repository's.**
    ///
    /// The red form of plan §10.4.3's clause, said about codex: the survey found `.codex/` beside a
    /// working directory to be one of the four places codex itself looks, and named it the attack
    /// surface. There is exactly one path this module can produce, it comes from the variable codex
    /// itself reads, and no working directory reaches it.
    #[test]
    fn the_only_file_this_writes_is_the_one_the_environment_names() {
        let named = |text: &str| Some(OsString::from(text));
        assert_eq!(
            config_dir_from(named(r"D:\scratch\codex-home"), named(r"C:\Users\someone")),
            Some(PathBuf::from(r"D:\scratch\codex-home"))
        );
        // Set-but-empty is not set.
        assert_eq!(
            config_dir_from(named(""), named(r"C:\Users\someone")),
            Some(PathBuf::from(r"C:\Users\someone").join(DEFAULT_DIRECTORY))
        );
        assert_eq!(config_dir_from(None, None), None);
        assert_eq!(config_dir_from(None, named("")), None);
        assert!(config_path().is_none_or(|path| path.ends_with(CONFIG_FILE)));
        // **Nothing in this module can be steered by where the process happens to be standing**,
        // asserted over the source itself because that is the only form the rule has. The names are
        // spelled in halves so this assertion is not its own counter-example.
        let source = include_str!("attention_codex.rs");
        for reaching in [concat!("current", "_dir"), concat!("project", "_dir")] {
            assert!(
                !source.contains(reaching),
                "`{reaching}` would let a working directory decide where automation is written"
            );
        }
        assert!(
            config_dir_from(named(r"D:\scratch\codex-home"), None)
                .is_some_and(|path| path.is_absolute())
        );
    }

    /// **The line this writes into somebody's own file, spelled out.**
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
            let mut document = original.parse::<DocumentMut>().expect("fixture");
            assert!(install_into(&mut document, &exe()));
            let installed = rendered(&document, original);
            assert!(
                installed.contains(NOTIFY_KEY),
                "the line is in what gets written"
            );
            let mut back = installed.parse::<DocumentMut>().expect("what we wrote");
            assert!(remove_from(&mut back, &exe()));
            assert_eq!(
                rendered(&back, &installed),
                original,
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

    /// **Somebody else's `notify` is never taken away.**
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

    /// A file this build cannot read is left alone.
    #[test]
    fn an_unreadable_configuration_file_is_never_written_over() {
        assert!("model = = \"gpt-5\"\n".parse::<DocumentMut>().is_err());
        // The refusal is `apply`'s, and it is the reason `state` has a third answer: `Absent` would
        // have made the row offer to write over it.
        let source = include_str!("attention_codex.rs");
        assert!(source.contains("is not one this build can read"));
    }

    /// RED — **a configuration file that could not be read is left byte for byte.**
    ///
    /// Release audit 2026-09-16 (C-3): `read_to_string(&path).unwrap_or_default()` took a file this
    /// build was not allowed to read — or one holding a single byte that is not UTF-8 — for a file
    /// that was not there. The empty string it fell back to parses as an empty document, so the
    /// refusal below it never fired, and `land` was handed nothing to copy: the user's own
    /// configuration was replaced by one `notify` line with no copy kept anywhere.
    ///
    /// RED GATE: put the `unwrap_or_default` back and this file comes back as Folio's own.
    #[test]
    fn a_configuration_file_that_could_not_be_read_is_never_written_over() {
        let dir = scratch("unreadable");
        let path = dir.join(CONFIG_FILE);
        // A Latin-1 comment: an ordinary line on an ordinary machine, and not UTF-8.
        let theirs: &[u8] = b"# caf\xe9\nmodel = \"gpt-5\"\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        let installing = apply_to(&path, true, &exe());
        assert_eq!(installing, Outcome::Refused(UNREADABLE));
        assert_eq!(std::fs::read(&path).expect("still there"), theirs);
        assert_eq!(
            names(&dir),
            vec![CONFIG_FILE.to_owned()],
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
    fn a_configuration_file_that_is_not_there_is_the_one_that_gets_created() {
        let dir = scratch("absent");
        let path = dir.join(CONFIG_FILE);
        assert_eq!(state_at(&path), State::Absent);

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        assert_eq!(state_at(&path), State::Installed);
        assert_eq!(
            names(&dir),
            vec![CONFIG_FILE.to_owned()],
            "a first install leaves one file: no copy of nothing, no temporary"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A file that is there is copied before it is changed**, and the copy is what was there.
    #[test]
    fn a_configuration_file_that_is_there_is_copied_before_it_is_changed() {
        let dir = scratch("copied");
        let path = dir.join(CONFIG_FILE);
        let theirs = "# mine\nmodel = \"gpt-5\"\n";
        std::fs::write(&path, theirs).expect("the user's own file");

        assert_eq!(apply_to(&path, true, &exe()), Outcome::Installed);
        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(written.contains("# mine"), "{written}");
        assert!(written.contains(NOTIFY_KEY), "{written}");
        let backup = format!("{CONFIG_FILE}.bak-{}", crate::attention_hooks::today());
        assert_eq!(names(&dir), vec![CONFIG_FILE.to_owned(), backup.clone()]);
        assert_eq!(
            std::fs::read_to_string(dir.join(&backup)).expect("the copy"),
            theirs,
            "the copy beside it is the file as it was"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch directory of this test's own. Never anywhere near a real `~/.codex`.
    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(format!(
                "folio-codex-{name}-{}-{}",
                std::process::id(),
                crate::attention_hooks::today()
            ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
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
}
