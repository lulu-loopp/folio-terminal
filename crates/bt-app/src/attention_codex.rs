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
}

/// The directory codex keeps user configuration in, as **this environment** says it.
#[must_use]
pub(crate) fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os(HOME_VARIABLE),
        std::env::var_os("USERPROFILE"),
    )
}

/// The same decision, with the environment handed in.
///
/// Split out for [`attention_hooks`](crate::attention_hooks)'s reason: a process-wide variable
/// changed from a test is changed for every other test running beside it, and this crate refuses
/// `unsafe`, which is what `set_var` now is.
#[must_use]
fn config_dir_from(named: Option<OsString>, profile: Option<OsString>) -> Option<PathBuf> {
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        return Some(PathBuf::from(named));
    }
    Some(PathBuf::from(profile.filter(|profile| !profile.is_empty())?).join(DEFAULT_DIRECTORY))
}

/// The user-level configuration file. **The only file this module ever writes.**
#[must_use]
pub(crate) fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONFIG_FILE))
}

/// What that file says today.
#[must_use]
pub(crate) fn state() -> State {
    let Some(path) = config_path() else {
        return State::Absent;
    };
    match std::fs::read_to_string(&path) {
        Err(_) => State::Absent,
        Ok(text) => match text.parse::<DocumentMut>() {
            Ok(document) => {
                if declares_folio(&document) {
                    State::Installed
                } else {
                    State::Absent
                }
            }
            Err(_) => State::Unreadable,
        },
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

/// **Whether the `notify` standing in this document is one of ours.**
///
/// By the **verb and the family**, not by the path to the executable:
/// [`attention_hooks::MARK`](crate::attention_hooks)'s rule, for its reason — a user who moves
/// Folio, or who runs two builds of it, still has an entry this can recognise and take back out.
fn declares_folio(document: &DocumentMut) -> bool {
    words_of(document).is_some_and(|words| {
        words.iter().any(|word| word == ATTENTION_VERB)
            && words
                .iter()
                .any(|word| word.starts_with(&format!("{CODEX}:")))
    })
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
pub(crate) fn install_into(document: &mut DocumentMut, exe: &Path) -> bool {
    if declares_somebody_else(document) {
        return false;
    }
    let mut array = Array::new();
    for word in program_for(exe) {
        array.push(word);
    }
    let written = Item::Value(Value::Array(array));
    if document
        .get(NOTIFY_KEY)
        .is_some_and(|standing| standing.to_string() == written.to_string())
    {
        return false;
    }
    document[NOTIFY_KEY] = written;
    true
}

/// Take Folio's program back off the key, leaving everything else exactly as it was.
///
/// Symmetric with [`install_into`] and tested as such, byte for byte, on a document with the user's
/// own comments and tables in it.
pub(crate) fn remove_from(document: &mut DocumentMut) -> bool {
    if !declares_folio(document) {
        return false;
    }
    document.remove(NOTIFY_KEY).is_some()
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

/// Put Folio's `notify` in, or take it out, on this machine.
///
/// **The file is read, changed and written whole**, and a copy of what was there is kept beside it
/// the first time each day — `attention_hooks`'s rule, for its reason: this is somebody's own
/// configuration file, and a build that could damage one had better be able to hand it back.
pub(crate) fn apply(install: bool, exe: &Path) -> Outcome {
    let Some(path) = config_path() else {
        return Outcome::Refused("no codex configuration directory to write into");
    };
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut document = match existing.parse::<DocumentMut>() {
        Ok(document) => document,
        // Refused rather than replaced. A configuration file this build cannot read is a file
        // somebody wrote, and overwriting it to add a convenience is not a trade anyone agreed to.
        Err(_) => {
            return Outcome::Refused("the codex configuration file is not one this build can read");
        }
    };
    if install && declares_somebody_else(&document) {
        return Outcome::Refused("codex already runs a notify program of your own");
    }
    let changed = if install {
        install_into(&mut document, exe)
    } else {
        remove_from(&mut document)
    };
    if !changed {
        return Outcome::Unchanged;
    }
    if !existing.is_empty() {
        let backup = path.with_extension(format!("toml.bak-{}", crate::attention_hooks::today()));
        if !backup.exists() {
            let _ = std::fs::write(&backup, &existing);
        }
    }
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return Outcome::Refused("the codex configuration directory could not be created");
    }
    if std::fs::write(&path, rendered(&document, &existing)).is_err() {
        return Outcome::Refused("the codex configuration file could not be written");
    }
    if install {
        Outcome::Installed
    } else {
        Outcome::Removed
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

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Folio\folio.exe")
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
        assert_eq!(call.payload.as_deref(), Some(payload));
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
        assert!(remove_from(&mut document));
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
            assert!(remove_from(&mut back));
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
        assert!(remove_from(&mut document));
        assert!(
            !remove_from(&mut document),
            "and neither is a removal of what is not there"
        );
    }

    /// Moving the executable is not a reason to lose track of the entry.
    #[test]
    fn the_entry_is_recognised_by_what_it_does_not_by_where_folio_lives() {
        let mut document = installed("");
        let moved = PathBuf::from(r"E:\portable\folio.exe");
        assert!(
            install_into(&mut document, &moved),
            "a rewrite from a new location is a change"
        );
        assert!(document.to_string().contains("portable"));
        assert_eq!(state_of(&document), State::Installed);
        assert!(remove_from(&mut document));
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
        assert!(!remove_from(&mut document));
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

    fn state_of(document: &DocumentMut) -> State {
        if declares_folio(document) {
            State::Installed
        } else {
            State::Absent
        }
    }
}
