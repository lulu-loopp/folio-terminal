//! **What a toast is allowed to quote**, pinned on the two failures this block exists to prevent:
//! a notification that says nothing, and a notification that says something nobody wrote.

use std::path::PathBuf;

use super::*;

/// A scratch file with a name of its own, removed when the test that made it ends.
///
/// `std::env::temp_dir` and a counter rather than a crate: this workspace has no tempfile
/// dependency and one transcript fixture is not a reason to add one.
struct Scratch(PathBuf);

impl Scratch {
    fn holding(name: &str, text: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "folio-attention-words-{}-{name}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, text).expect("a scratch transcript");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// One transcript line, in the shape Claude Code writes.
fn entry(kind: &str, said: &str, sidechain: bool) -> String {
    serde_json::json!({
        "type": kind,
        "isSidechain": sidechain,
        "uuid": "0000",
        "message": {
            "role": if kind == "assistant" { "assistant" } else { "user" },
            "content": [{ "type": "text", "text": said }],
        },
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// The quotation
// ---------------------------------------------------------------------------

/// **The lede is the first sentence, and it arrives without its markdown.**
#[test]
fn what_is_quoted_is_the_first_sentence_of_what_was_written() {
    assert_eq!(
        lede("Done. The rest is detail.", LIMIT).as_deref(),
        Some("Done.")
    );
    // Bold, code and a link are marks around words, and the words are what is quoted.
    assert_eq!(
        lede(
            "**Shipped** `folio.exe` to [dist](file:///D:/dist). Two files changed.",
            LIMIT
        )
        .as_deref(),
        Some("Shipped folio.exe to dist.")
    );
    // A heading is prose. A message that opens on one is quoted from it.
    assert_eq!(
        lede("## 改动汇总\n\n| a | b |\n|---|---|\n| 1 | 2 |", LIMIT).as_deref(),
        Some("改动汇总")
    );
    // A paragraph broken across source lines is one sentence, not two lines.
    assert_eq!(
        lede("The build is green\nand the tests pass. Detail.", LIMIT).as_deref(),
        Some("The build is green and the tests pass.")
    );
    // A sentence that never ends is quoted whole.
    assert_eq!(
        lede("本轮全部落位:", LIMIT).as_deref(),
        Some("本轮全部落位:")
    );
}

/// **A dot inside a word is not the end of a sentence.**
///
/// The three spellings this meets every day — a version, a file name, a section number — and each
/// of them would be cut in the middle by a rule that stopped at every `.`.
#[test]
fn a_full_stop_ends_a_sentence_only_where_a_sentence_ends() {
    assert_eq!(
        lede("Released 0.1.1 as folio.exe per §7.1. Nothing else.", LIMIT).as_deref(),
        Some("Released 0.1.1 as folio.exe per §7.1.")
    );
    // The CJK full stop is an end wherever it is: it is never an abbreviation mark.
    assert_eq!(
        lede("收官了。剩下的明天再说。", LIMIT).as_deref(),
        Some("收官了。")
    );
    assert_eq!(lede("好了吗？是的。", LIMIT).as_deref(), Some("好了吗？"));
}

/// **A cut lands between characters and never inside one** — and it is marked.
///
/// The red form of the bug a byte slice would have: `&text[..80]` on a Chinese sentence is a panic
/// on a character boundary, and on a sentence of mixed scripts it is a panic that only some inputs
/// reach. Counting characters makes the cut correct for every script by construction.
#[test]
fn a_shortened_sentence_is_cut_between_characters() {
    let long = "远".repeat(200);
    let quoted = lede(&long, LIMIT).expect("a sentence");
    assert_eq!(
        quoted.chars().count(),
        LIMIT + 1,
        "eighty characters of theirs and one mark of ours: {quoted}"
    );
    assert!(quoted.ends_with(ELLIPSIS));
    assert_eq!(
        quoted.chars().take(LIMIT).collect::<String>(),
        "远".repeat(LIMIT),
        "the characters kept are the first eighty and each of them is whole"
    );
    // The same in bytes, said the way the failure would be seen: every kept character is intact,
    // so the string round-trips as UTF-8 with the character count it claims.
    assert_eq!(quoted.len(), 3 * LIMIT + ELLIPSIS.len_utf8());
    // A sentence that fits is not marked, because a mark on it would be a lie about what was said.
    let short = lede("Done.", LIMIT).expect("a sentence");
    assert!(!short.ends_with(ELLIPSIS));
    // Mixed scripts, where a byte cut goes wrong for only some inputs.
    let mixed = format!("{}{}", "ab".repeat(60), "远".repeat(60));
    let quoted = lede(&mixed, LIMIT).expect("a sentence");
    assert_eq!(quoted.chars().count(), LIMIT + 1);
    // And a cut that would have landed on a space does not leave one hanging before the mark.
    let spaced = format!("{} tail", "w ".repeat(LIMIT));
    let quoted = lede(&spaced, LIMIT).expect("a sentence");
    assert!(!quoted.contains(" …"), "a space before the mark: {quoted}");
}

/// **Nothing to quote is an answer**, and it is the answer that keeps the old wording.
#[test]
fn a_message_with_no_prose_in_it_quotes_nothing() {
    for silent in [
        "",
        "   \n\n  ",
        "```\ncargo test\n```",
        "| a | b |\n|---|---|\n| 1 | 2 |",
        "---",
        "![a picture](shot.png)",
    ] {
        assert_eq!(
            lede(silent, LIMIT),
            None,
            "this was quoted and should not have been: {silent:?}"
        );
    }
    // A limit of nothing quotes nothing rather than quoting a bare mark.
    assert_eq!(lede("Done.", 0), None);
    // And a message whose prose is *under* a fence is quoted from the prose.
    assert_eq!(
        lede("```\ncargo test\n```\n\nAll green.", LIMIT).as_deref(),
        Some("All green.")
    );
}

// ---------------------------------------------------------------------------
// The transcript
// ---------------------------------------------------------------------------

/// **The last thing the main agent said, out of a file of everything anyone said.**
#[test]
fn the_transcript_is_quoted_from_its_last_main_thread_message() {
    let file = Scratch::holding(
        "ordinary",
        &[
            entry("assistant", "An older answer. With detail.", false),
            entry("user", "and then?", false),
            entry("assistant", "**Done.** The rest is detail.", false),
            // A subagent's last word, written after the main agent's, is not this turn's.
            entry("assistant", "Subagent finished its search.", true),
            // And the user's own line is the last one in the file, as it usually is not — but a
            // scan that took the last *entry* rather than the last assistant's would find it.
            entry("user", "thanks", false),
            String::new(),
        ]
        .join("\n"),
    );
    assert_eq!(
        transcript_lede(file.path(), LIMIT).as_deref(),
        Some("Done.")
    );
}

/// **A transcript this cannot read says nothing, and says it without failing.**
///
/// Every one of these is a state a real machine reaches — a path from a stale payload, a file
/// still being written, a turn that ended on a tool call — and the contract at every one of them
/// is the same: the caller goes on and says what it said before.
#[test]
fn a_transcript_that_answers_nothing_is_not_a_failure() {
    let missing = std::env::temp_dir().join("folio-no-such-transcript-9db2.jsonl");
    assert_eq!(transcript_lede(&missing, LIMIT), None);

    let empty = Scratch::holding("empty", "");
    assert_eq!(transcript_lede(empty.path(), LIMIT), None);

    let junk = Scratch::holding("junk", "not json\n{\n[1,2,3]\n\u{0}\u{1}\n");
    assert_eq!(transcript_lede(junk.path(), LIMIT), None);

    // Only a subagent ever spoke: nothing here is this turn's.
    let sidechain = Scratch::holding(
        "sidechain",
        &format!("{}\n", entry("assistant", "Subagent says hello.", true)),
    );
    assert_eq!(transcript_lede(sidechain.path(), LIMIT), None);

    // The turn ended on a tool call, so the last assistant entry has no words in it.
    let tools = Scratch::holding(
        "tools",
        &format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant",
                "message": { "role": "assistant", "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {} }
                ]},
            })
        ),
    );
    assert_eq!(transcript_lede(tools.path(), LIMIT), None);

    // A directory is not a transcript, and asking is not an error either.
    assert_eq!(transcript_lede(&std::env::temp_dir(), LIMIT), None);
}

/// **`content` as a bare string is read as well as `content` as blocks.**
///
/// Both spellings have shipped, and a reader that knew only one of them would go quiet on a
/// transcript written by the other with no way for anybody to tell why.
#[test]
fn both_spellings_of_a_message_body_are_read() {
    let file = Scratch::holding(
        "bare",
        &format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant",
                "message": { "role": "assistant", "content": "Done. Detail." },
            })
        ),
    );
    assert_eq!(
        transcript_lede(file.path(), LIMIT).as_deref(),
        Some("Done.")
    );
}

/// **The tail grows until the answer is in it.**
///
/// A transcript's last entries are the largest ones in it — a tool result holding a whole file is
/// one line — so the message being looked for can sit far behind the end. This is the case a fixed
/// window would answer `None` to while the sentence was sitting one byte outside it.
#[test]
fn a_message_buried_behind_a_large_entry_is_still_found() {
    let buried = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": "x".repeat(FIRST_TAIL as usize * 2) },
    })
    .to_string();
    let file = Scratch::holding(
        "buried",
        &format!(
            "{}\n{buried}\n",
            entry("assistant", "Found me. And the detail.", false)
        ),
    );
    assert_eq!(
        transcript_lede(file.path(), LIMIT).as_deref(),
        Some("Found me."),
        "the first window missed it, so a second and larger one had to be read"
    );
}

/// **Half a line is not a line.**
///
/// Dropping the first line of a partial read is not an optimisation — it is what tail-reading a
/// line-oriented file *is*. The alternative rests on a coincidence: that a fragment of one of these
/// lines is not valid JSON and would be refused anyway. That coincidence holds for every line
/// Claude Code writes today and is not a property of anything, so the rule is asked about directly,
/// with the read placed exactly where one whole object sits in the middle of a line — the shape a
/// torn or interleaved append leaves behind.
#[test]
fn a_line_the_read_began_inside_is_never_quoted() {
    let mine = entry("assistant", "The real answer. Detail.", false);
    let theirs = entry("assistant", "Half a line.", false);
    let prefix = "something else was appended in front of it:";
    let file = Scratch::holding("partial", &format!("{mine}\n{prefix}{theirs}\n"));
    let mut handle = File::open(file.path()).expect("the fixture");
    let inside = (mine.len() + 1 + prefix.len()) as u64;

    // The fixture is only worth anything if the fragment really does parse — otherwise this would
    // be asserting that JSON is picky rather than that the read drops what it began inside.
    assert_eq!(
        lede_in_tail(&mut handle, inside, false, LIMIT).as_deref(),
        Some("Half a line.")
    );
    assert_eq!(
        lede_in_tail(&mut handle, inside, true, LIMIT),
        None,
        "a read that began inside a line holds nothing but that line's tail"
    );
    // And read as a whole file — where the second line is a line, and is not one of ours — the
    // answer is the entry that was written as an entry.
    assert_eq!(
        transcript_lede(file.path(), LIMIT).as_deref(),
        Some("The real answer.")
    );
}
