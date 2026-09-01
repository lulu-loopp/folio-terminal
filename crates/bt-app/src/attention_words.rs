//! **The sentence a turn ended with**, for the toast that says the turn ended.
//!
//! `docs/plans/attention/plan.md` §11.7's wording clause has said since it was written that *a
//! program that wrote a sentence has said this better than anything composed from a pane name* —
//! and until now the only lane that could act on it was `OSC 9`, where the program hands over its
//! own words in the bytes. A hook does not. Claude Code's `Stop` carries a session id and a path;
//! codex's `notify` carries a payload nobody was reading. So every turn that ended over the pipe
//! reached the desktop as the same three words, and a user of the released build called the result
//! by its right name: a notification with no information in it.
//!
//! This module is the missing half — **not a summariser and not a guess**. Everything here quotes
//! something the agent actually wrote and stops the moment there is nothing to quote:
//!
//! * [`lede`] takes prose the agent wrote and returns its first sentence, bounded.
//! * [`transcript_lede`] finds that prose at the end of a Claude Code transcript.
//!
//! Both answer `None` freely, and that is the load-bearing property: the caller's contract is that
//! a missing sentence costs the reader nothing but the old wording. Nothing on this path may fail
//! a notification.
//!
//! # Why the *first* sentence
//!
//! Measured, on 35 real transcripts of this project and four others (2026-09-01, the run recorded
//! in `docs/DESIGN.md` §7.1.5q). A Claude Code turn's last message is written lede-first: one
//! sentence that says what happened, then the detail — bullets, a table, a file list. Taking the
//! **first** sentence quoted the outcome in 33 of 35; taking the **last** quoted a trailing offer
//! or aside in more than half of them ("想再展开哪部分？", "现在可以 /clear 了。", "说一声就行。")
//! and, when the message ended on a table, quoted one cell of it. The lede is the summary the
//! author already wrote; the tail is the part addressed to somebody who has read the rest.
//!
//! # Why the markdown is parsed rather than scrubbed
//!
//! [`crate::preview::parse_markdown`] is this build's CommonMark reader, and the alternative — a
//! pass that deletes `*`, `` ` `` and `#` from a line — is a second, worse one. It would keep the
//! `*` in `3 * 4`, break a link into its destination, and read a fenced block's contents as prose.
//! Asking the parser which block is the first paragraph is both correct and free: a message whose
//! first block is a table, a fence or a rule falls through to whichever block is prose, and a
//! message with no prose at all quotes nothing.
//!
//! # The transcript is read from its end
//!
//! These files reach hundreds of megabytes — 287 MB in the measured set — and this runs inside a
//! hook, where the whole call has to be over before the user notices it happened. So the read is a
//! bounded tail that grows only when it has to, and the measured cost across that set was 0.1 ms
//! to 0.8 ms, the largest file included.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

use crate::preview::{MarkdownBlock, Span, parse_markdown};

/// **How many characters of somebody else's sentence a toast carries.**
///
/// Counted in characters and not in bytes, because the sentence is as likely to be Chinese as
/// English and a byte bound would cut one of them at a quarter of the length of the other. Eighty
/// is about two lines of a Windows toast at the width Windows gives one; what is cut is marked, so
/// the reader can tell a short sentence from a shortened one.
pub(crate) const LIMIT: usize = 80;

/// What a shortened sentence ends on. One character, and it is **not** counted against [`LIMIT`]:
/// the limit is how much of the agent's words are carried, and this is not one of them.
const ELLIPSIS: char = '…';

/// The tail of a transcript read on the first attempt.
///
/// Sixty-four kilobytes holds the last message of every transcript in the measured set. It is the
/// common case and it is one seek and one read.
const FIRST_TAIL: u64 = 64 * 1024;

/// How much more is read each time the tail held no answer.
const TAIL_GROWTH: u64 = 8;

/// **Where the search stops.** A transcript whose last eight megabytes contain no message from the
/// main agent is a transcript this has nothing to quote from, and reading further would trade the
/// hook's whole time budget for a sentence that is very likely not there.
const WIDEST_TAIL: u64 = 8 * 1024 * 1024;

/// **The first sentence of what somebody wrote**, cleaned of its markdown and bounded to `limit`
/// characters.
///
/// `None` when there is no prose in it at all — a message that is one code fence, one table, one
/// image — which is the answer that leaves the caller saying what it said before.
#[must_use]
pub(crate) fn lede(said: &str, limit: usize) -> Option<String> {
    if limit == 0 {
        return None;
    }
    let prose = first_prose(&parse_markdown(said))?;
    let sentence = first_sentence(&prose);
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return None;
    }
    Some(shortened(sentence, limit))
}

/// **The lede of the last thing the main agent said**, out of a Claude Code transcript.
///
/// The file is JSONL, one entry per line, and the entry wanted is the last `"type":"assistant"`
/// that is not a sidechain — a sidechain entry is a subagent's, and a subagent's last word is not
/// this turn's.
///
/// Every failure here is `None`: no file, a file that is not this format, a last message that is
/// all tool calls. **None of them is an error**, because the caller's fallback is the sentence it
/// would have said anyway.
#[must_use]
pub(crate) fn transcript_lede(path: &Path, limit: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut window = FIRST_TAIL;
    loop {
        let read = window.min(size);
        if let Some(words) = lede_in_tail(&mut file, size - read, read < size, limit) {
            return Some(words);
        }
        if read == size || window >= WIDEST_TAIL {
            return None;
        }
        window = window.saturating_mul(TAIL_GROWTH);
    }
}

/// One pass over one tail: read it, walk its lines backwards, answer with the first lede found.
///
/// `partial` says the read began inside a line, in which case the first line of what came back is
/// the tail of a line whose start was not read and is dropped. Dropping it is not an optimisation:
/// half a JSON object parses as nothing, but half a *string* can parse as a whole object with the
/// wrong contents, and the entry it would answer with is one nobody wrote.
fn lede_in_tail(file: &mut File, from: u64, partial: bool, limit: usize) -> Option<String> {
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if partial && !lines.is_empty() {
        lines.remove(0);
    }
    lines
        .iter()
        .rev()
        .filter_map(|line| said_by_the_agent(line))
        .find_map(|said| lede(&said, limit))
}

/// What one transcript line says the main agent said, or `None` for every other kind of line.
///
/// Tolerant of the shape and strict about the identity: the fields read are `type`, `isSidechain`
/// and `message.content`, and a `content` that is a bare string is read as well as one that is an
/// array of blocks, because both spellings have shipped. What it will not do is read a line that
/// does not say it is an assistant's, or one that says it is a sidechain's.
fn said_by_the_agent(line: &str) -> Option<String> {
    let line = line.trim();
    // The cheap gate. Most lines in a transcript's tail are somebody else's and are large; asking
    // whether the first character could begin an object costs one comparison instead of a parse.
    if !line.starts_with('{') {
        return None;
    }
    let entry = serde_json::from_str::<Value>(line).ok()?;
    let entry = entry.as_object()?;
    if entry.get("type")?.as_str()? != "assistant" {
        return None;
    }
    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let content = entry.get("message")?.get("content")?;
    let said = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    (!said.trim().is_empty()).then_some(said)
}

/// The first block of a parsed message that is prose, as one line of text.
///
/// A heading counts and a table does not, and the line between them is whether the block is a
/// sentence somebody wrote or a structure they built. A message that opens on a fence, a rule, a
/// picture or a table has its prose further down, and this walks to it rather than giving up —
/// which is the case §7.1.5q's measurement kept meeting: "改动汇总" over a table.
fn first_prose(blocks: &[MarkdownBlock]) -> Option<String> {
    blocks.iter().find_map(|block| match block {
        MarkdownBlock::Heading { spans, .. } | MarkdownBlock::Paragraph(spans) => joined(spans),
        MarkdownBlock::List { items, .. } => items.iter().find_map(|spans| joined(spans)),
        MarkdownBlock::Quote(lines) => lines.iter().find_map(|spans| joined(spans)),
        MarkdownBlock::Code { .. }
        | MarkdownBlock::Table { .. }
        | MarkdownBlock::Math { .. }
        | MarkdownBlock::Image(_)
        | MarkdownBlock::Rule => None,
    })
}

/// One block's spans as one line: the text the parser kept, with every run of whitespace — the
/// line breaks inside a paragraph included — collapsed to one space.
fn joined(spans: &[Span]) -> Option<String> {
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one_line.is_empty()).then_some(one_line)
}

/// Everything up to and including the first sentence's end, or the whole of it when it ends none.
///
/// The Latin stops are ends only where a space or the end of the text follows, which is what keeps
/// `0.1.1`, `folio.exe` and `§7.1` whole; the CJK stops are ends wherever they are, because that
/// is how they are written — a full stop in Chinese is never an abbreviation mark.
fn first_sentence(prose: &str) -> &str {
    for (at, character) in prose.char_indices() {
        let end = at + character.len_utf8();
        let stops = match character {
            '。' | '！' | '？' | '…' => true,
            '.' | '!' | '?' => prose[end..].chars().next().is_none_or(char::is_whitespace),
            _ => false,
        };
        if stops {
            return &prose[..end];
        }
    }
    prose
}

/// `sentence` if it fits in `limit` characters, and its first `limit` characters with an
/// [`ELLIPSIS`] after them if it does not.
///
/// **Counted and cut in characters**, so the cut never lands inside one — the property a byte
/// slice of the same length would break on the first Chinese sentence, and would break by
/// panicking rather than by looking wrong.
fn shortened(sentence: &str, limit: usize) -> String {
    if sentence.chars().count() <= limit {
        return sentence.to_owned();
    }
    let mut kept = sentence.chars().take(limit).collect::<String>();
    while kept.ends_with(char::is_whitespace) {
        kept.pop();
    }
    kept.push(ELLIPSIS);
    kept
}

#[cfg(test)]
mod tests;
