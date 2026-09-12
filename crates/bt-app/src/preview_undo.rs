//! The preview editor's undo log: what changed, in what order, and how much of
//! it one press takes back.
//!
//! **On the buffer, never on the pane** (research §9.2, ticket T3). A file open
//! in two panes is *one* buffer by ruling (§7.1.3 「同一文件在两个 pane 打开即
//! 同一份缓冲,编辑不可能分叉」), so a stack of changes kept on a pane would fork
//! the one thing the pool exists to keep unforked: the second pane's undo would
//! either replay changes the first pane never made or refuse to see the ones it
//! did. The caret is the other way round — it is the *view's* (ruling 8⑧) — so
//! an entry carries the caret of whoever typed it and the press that undoes it
//! gets that caret back. The other pane's caret is not touched: it is clamped
//! into the body the next time it is used ([`crate::preview_edit::EditCaret::heal`],
//! which every edit and every motion begins with) and otherwise left where its
//! reader left it, exactly as it already is when the *other* pane types.
//!
//! **Everything here is a pure function of a `String`, an offset and a caret**,
//! in [`crate::preview_edit`]'s style: no window, no disk, no clock. What the
//! buffer owns is one [`UndoLog`]; what this module owns is the arithmetic of
//! putting bytes back.
//!
//! **A run of typing is one entry.** The granularity is the one open question 6
//! ruled: consecutive single-character insertions, or consecutive
//! single-character deletions in the same direction, coalesce into one entry, and
//! the run is broken by a caret jump, a newline, a change of direction, a save,
//! or the arrival of the disk's copy. A paste is one entry however long it is.
//! There is no depth cap short of [`UndoLog::DEPTH`], and the log is **not
//! persisted**: a session file that restored an undo stack would let a person
//! undo past a save they cannot see.

use crate::preview_edit::EditCaret;

/// A caret with nothing selected, at one offset.
///
/// **Test-only, with [`Change::implied`]**: every edit this window makes to a
/// body arrives through the keyboard and carries a real caret, so the only
/// callers that have to invent one are the ones asserting the arithmetic.
#[cfg(test)]
fn collapsed(offset: usize) -> EditCaret {
    EditCaret {
        anchor: offset,
        caret: offset,
        desired_column: None,
        desired_x: None,
    }
}

/// **What shape of edit made a change**, and therefore what it may join.
///
/// A column of the entry rather than something re-derived at merge time: the
/// direction a deletion ran in is a fact about the keystroke, and once the two
/// changes have been folded into one entry there is nothing left to read it off.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Grain {
    /// One character typed in.
    Insert,
    /// One character taken back — Backspace, the caret walking left.
    DeleteBack,
    /// One character taken forward — Delete, the caret standing still.
    DeleteForward,
    /// Everything else, and it stands alone: a paste, a newline, a selection
    /// replaced, an edit no keyboard made. The entry before it is closed and the
    /// entry after it starts fresh.
    Solid,
}

/// One change to a body, before it is filed.
///
/// The five things an undo needs and no more: where the change starts, what came
/// out, what went in, and the caret on each side of it. The byte range removed is
/// `at .. at + removed.len()` and the range inserted is `at .. at + inserted.len()`;
/// spelling one offset and two strings rather than two ranges is what keeps the
/// two from ever disagreeing about where they begin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    /// The first byte the change touched.
    pub at: usize,
    /// The bytes that were there.
    pub removed: String,
    /// The bytes that are there now.
    pub inserted: String,
    /// Where the caret was before the keystroke.
    pub before: EditCaret,
    /// Where it ended up.
    pub after: EditCaret,
}

impl Change {
    /// **The change between two bodies**, with the caret the keystroke reported.
    ///
    /// Computed by comparison rather than described by the caller, and that is
    /// deliberate: the buffer's edit door takes a closure, so the only thing that
    /// knows for certain what a keystroke did to the bytes is the bytes. A
    /// closure that reported its own edit would be a second account of it, and
    /// the two accounts would drift the first time somebody wrote a third kind of
    /// edit.
    ///
    /// The common head and the common tail are trimmed off, so a change is the
    /// smallest span that differs — which is what makes a run of typing look like
    /// a run of one-character inserts at ascending offsets, and what lets the
    /// coalescing below be arithmetic rather than a guess. Both ends are pulled
    /// back to a character boundary in **both** bodies, because a change that
    /// began in the middle of a character would name text that is not there.
    ///
    /// `None` when the bodies are identical: an edit that changed no bytes is not
    /// a change, however loudly it returned `true`.
    #[must_use]
    pub fn between(was: &str, now: &str, before: EditCaret, after: EditCaret) -> Option<Self> {
        let head = common_head(was, now);
        let tail = common_tail(was, now, head);
        let removed = was[head..was.len() - tail].to_owned();
        let inserted = now[head..now.len() - tail].to_owned();
        if removed.is_empty() && inserted.is_empty() {
            return None;
        }
        Some(Self {
            at: head,
            removed,
            inserted,
            before,
            after,
        })
    }

    /// The same, for a door that has **no caret to tell it** — every edit that is
    /// not a keystroke.
    ///
    /// The caret filed is the one the change itself implies: collapsed at the end
    /// of what came out before, and at the end of what went in after. It is what
    /// a caret would be if a person had typed this, so an undo of it lands
    /// somewhere a reader can make sense of instead of at the top of the file.
    #[cfg(test)]
    #[must_use]
    pub fn implied(was: &str, now: &str) -> Option<Self> {
        let mut change = Self::between(was, now, EditCaret::default(), EditCaret::default())?;
        change.before = collapsed(change.at + change.removed.len());
        change.after = collapsed(change.at + change.inserted.len());
        Some(change)
    }

    /// Which run, if any, this change belongs to.
    ///
    /// **A break in either direction is [`Grain::Solid`]**, which is how "broken
    /// by a newline" is enforced: Enter is its own entry, it closes the run above
    /// it, and the next character opens a new one. A `\r\n` is one break and is
    /// caught by the same clause.
    fn grain(&self) -> Grain {
        let breaks = |text: &str| text.contains(['\n', '\r']);
        if breaks(&self.removed) || breaks(&self.inserted) {
            return Grain::Solid;
        }
        match (self.removed.is_empty(), self.inserted.is_empty()) {
            // Typed in. A paste is the same shape and is told apart by its
            // length, which is the whole of "a paste is one entry".
            (true, false) if self.inserted.chars().count() == 1 => Grain::Insert,
            (false, true) if self.removed.chars().count() == 1 => {
                // A deletion that swallowed a selection is one entry: the
                // gesture that made it was a drag, not a run of keys.
                if !self.before.is_empty() {
                    return Grain::Solid;
                }
                let end = self.at + self.removed.len();
                if self.before.caret == end {
                    Grain::DeleteBack
                } else if self.before.caret == self.at {
                    Grain::DeleteForward
                } else {
                    Grain::Solid
                }
            }
            _ => Grain::Solid,
        }
    }
}

/// One change, as the log remembers it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    at: usize,
    removed: String,
    inserted: String,
    before: EditCaret,
    after: EditCaret,
    grain: Grain,
}

impl Entry {
    fn new(change: Change, grain: Grain) -> Self {
        Self {
            at: change.at,
            removed: change.removed,
            inserted: change.inserted,
            before: change.before,
            after: change.after,
            grain,
        }
    }

    /// **Take one more keystroke into this entry**, or refuse and let it start
    /// its own.
    ///
    /// Three questions, and each of them is one of the ruled breakers. The grain
    /// must match, which is "a change of direction breaks the run". The caret
    /// must have started where this entry left it, which is "a caret jump breaks
    /// the run" — and it is asked of the *positions* and not of
    /// [`EditCaret::desired_column`], which is a memory of a vertical walk and
    /// not a place. And the bytes must abut, in the direction the run is running.
    fn absorb(&mut self, change: &Change, grain: Grain) -> bool {
        if self.grain != grain {
            return false;
        }
        if (self.after.anchor, self.after.caret) != (change.before.anchor, change.before.caret) {
            return false;
        }
        match grain {
            Grain::Insert => {
                if !self.removed.is_empty() || change.at != self.at + self.inserted.len() {
                    return false;
                }
                self.inserted.push_str(&change.inserted);
            }
            // Backspace walks left, so each keystroke's bytes go on the *front*
            // of what the run has taken and the entry's own start moves with it.
            Grain::DeleteBack => {
                if !self.inserted.is_empty() || change.at + change.removed.len() != self.at {
                    return false;
                }
                self.at = change.at;
                self.removed.insert_str(0, &change.removed);
            }
            // Delete stands still and the text comes to it, so the run grows out
            // of its own far end and its start never moves.
            Grain::DeleteForward => {
                if !self.inserted.is_empty() || change.at != self.at {
                    return false;
                }
                self.removed.push_str(&change.removed);
            }
            Grain::Solid => return false,
        }
        self.after = change.after;
        true
    }
}

/// **A body's changes, in the order they were made.**
///
/// `entries[..cursor]` have been applied to the body and `entries[cursor..]` are
/// the redo tail — changes that were made and then taken back, kept until a new
/// edit arrives to discard them. There is no snapshot of the text anywhere: the
/// body is the truth and this is the road back through it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoLog {
    entries: Vec<Entry>,
    /// How many entries stand applied to the body.
    cursor: usize,
    /// The cursor as it stood at the last save, while that position is still
    /// reachable. `None` once it has been discarded — by a redo tail thrown away
    /// or by the oldest entries falling off the front — and a body whose saved
    /// state cannot be returned to is dirty whatever else is true of it.
    saved: Option<usize>,
    /// Whether the newest entry may still take another keystroke. A save, a disk
    /// copy, an undo and a redo all close it, which is three of the ruled
    /// breakers spelled in one bit.
    open: bool,
}

impl Default for UndoLog {
    /// An empty log over a body that matches its file — which is what a body
    /// just read off a disk is.
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            saved: Some(0),
            open: false,
        }
    }
}

impl UndoLog {
    /// **How far back the log reaches.**
    ///
    /// Open question 6 asks for "no depth cap below a few thousand entries", and
    /// this is the number that honours it. It is a cap on *entries* and not on
    /// bytes, and an entry is a whole typing run, so four thousand of them is
    /// several hours of typing rather than four thousand keystrokes. Past it the
    /// oldest entry falls off the front, which is the one thing that can put the
    /// saved state out of reach — see [`Self::saved`].
    pub const DEPTH: usize = 4096;

    /// File a change.
    ///
    /// **A new edit discards the redo tail**, which is the rule every editor
    /// has: once the road has forked, the branch nobody took is gone. If the
    /// saved state was on that branch it goes with it, and the body is dirty from
    /// then on however far back it is undone — which is honest, because the
    /// bytes the disk holds are no longer anywhere in this log.
    pub fn record(&mut self, change: Change) {
        if self.cursor < self.entries.len() {
            self.entries.truncate(self.cursor);
            if self.saved.is_some_and(|at| at > self.cursor) {
                self.saved = None;
            }
            self.open = false;
        }
        let grain = change.grain();
        if self.open
            && grain != Grain::Solid
            && let Some(last) = self.entries.last_mut()
            && last.absorb(&change, grain)
        {
            return;
        }
        self.entries.push(Entry::new(change, grain));
        self.cursor = self.entries.len();
        self.open = grain != Grain::Solid;
        self.trim();
    }

    /// Put the newest entry's bytes back, and answer with the caret of whoever
    /// typed it.
    pub fn undo(&mut self, content: &mut String) -> Option<EditCaret> {
        let index = self.cursor.checked_sub(1)?;
        let entry = self.entries.get(index)?;
        content.replace_range(entry.at..entry.at + entry.inserted.len(), &entry.removed);
        let mut caret = entry.before;
        self.cursor = index;
        self.open = false;
        caret.heal(content);
        Some(caret)
    }

    /// Play the next entry forward again.
    pub fn redo(&mut self, content: &mut String) -> Option<EditCaret> {
        let entry = self.entries.get(self.cursor)?;
        content.replace_range(entry.at..entry.at + entry.removed.len(), &entry.inserted);
        let mut caret = entry.after;
        self.cursor += 1;
        self.open = false;
        caret.heal(content);
        Some(caret)
    }

    /// **The body was written to its file here.**
    ///
    /// The whole of the honest dirty bit: the saved state is a *position in this
    /// log*, so undoing back to it is being clean again and editing away from it
    /// is being dirty again — where before a save was the only thing that could
    /// clean a buffer, because there was no history to compare against.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.cursor);
        self.open = false;
    }

    /// **Forget everything** — the body under this log has been replaced.
    ///
    /// The disk's copy arriving is not a change to the body, it is a different
    /// body, and every offset in here names a place in the old one. The new body
    /// is the file's, so the log starts empty and clean, exactly as it does on a
    /// first read.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    /// Whether the body has moved away from the last state the disk saw.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.cursor)
    }

    /// Drop the oldest entries once the log is over its cap.
    fn trim(&mut self) {
        let Some(over) = self.entries.len().checked_sub(Self::DEPTH) else {
            return;
        };
        if over == 0 {
            return;
        }
        self.entries.drain(..over);
        self.cursor = self.cursor.saturating_sub(over);
        // A saved position that has just fallen off the front is a state this
        // log can no longer return to, and saying so is the point: the
        // alternative is a buffer that reports itself clean at a place the file
        // was never in.
        self.saved = self.saved.and_then(|at| at.checked_sub(over));
    }
}

/// **What the log is holding**, asked by the tests that assert the shape of it
/// and by nothing on the glass.
///
/// A press with nothing to take back is answered by [`UndoLog::undo`] returning
/// `None`, which is one question and not two — so the window never has to ask
/// whether there is anything there before asking for it, and these stay where
/// they are useful.
#[cfg(test)]
impl UndoLog {
    fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    /// How many entries it holds, redo tail included.
    fn depth(&self) -> usize {
        self.entries.len()
    }

    /// Close the run, the way a save or a reload does.
    fn seal(&mut self) {
        self.open = false;
    }
}

/// How many leading bytes the two bodies share, ending on a character boundary
/// in both.
fn common_head(was: &str, now: &str) -> usize {
    let limit = was.len().min(now.len());
    let mut head = 0;
    while head < limit && was.as_bytes()[head] == now.as_bytes()[head] {
        head += 1;
    }
    while head > 0 && !(was.is_char_boundary(head) && now.is_char_boundary(head)) {
        head -= 1;
    }
    head
}

/// How many trailing bytes they share, never reaching back past `head`.
fn common_tail(was: &str, now: &str, head: usize) -> usize {
    let limit = was.len().min(now.len()) - head;
    let mut tail = 0;
    while tail < limit
        && was.as_bytes()[was.len() - 1 - tail] == now.as_bytes()[now.len() - 1 - tail]
    {
        tail += 1;
    }
    while tail > 0
        && !(was.is_char_boundary(was.len() - tail) && now.is_char_boundary(now.len() - tail))
    {
        tail -= 1;
    }
    tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview_edit;

    /// Type one character where the caret is, the way the surface does, and file
    /// what happened.
    fn type_in(log: &mut UndoLog, content: &mut String, caret: &mut EditCaret, text: &str) {
        let was = content.clone();
        let before = *caret;
        preview_edit::insert(content, caret, text);
        if let Some(change) = Change::between(&was, content, before, *caret) {
            log.record(change);
        }
    }

    fn backspace(log: &mut UndoLog, content: &mut String, caret: &mut EditCaret) {
        let was = content.clone();
        let before = *caret;
        preview_edit::backspace(content, caret);
        if let Some(change) = Change::between(&was, content, before, *caret) {
            log.record(change);
        }
    }

    fn delete(log: &mut UndoLog, content: &mut String, caret: &mut EditCaret) {
        let was = content.clone();
        let before = *caret;
        preview_edit::delete_forward(content, caret);
        if let Some(change) = Change::between(&was, content, before, *caret) {
            log.record(change);
        }
    }

    fn typed(log: &mut UndoLog, content: &mut String, caret: &mut EditCaret, word: &str) {
        for glyph in word.chars() {
            type_in(log, content, caret, &glyph.to_string());
        }
    }

    /// RED — **a run of typing is one entry, and one press takes the run back.**
    ///
    /// The granularity open question 6 ruled. An undo that gave back one
    /// character per press is the undo a `<textarea>` has and the one every
    /// editor in §8 replaced.
    ///
    /// MUTATION: return `Grain::Solid` from `Change::grain` for an insert — the
    /// depth goes to five and the first undo gives back one letter.
    #[test]
    fn a_run_of_typing_is_one_entry() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "hello");
        assert_eq!(content, "hello");
        assert_eq!(log.depth(), 1, "five keystrokes, one run");
        assert_eq!(log.undo(&mut content), Some(collapsed(0)));
        assert_eq!(content, "");
        assert!(!log.can_undo());
    }

    /// RED — **a caret jump breaks the run.**
    ///
    /// Clicking somewhere else and typing is a second thought, and an undo that
    /// took back both thoughts at once would be taking back work the person
    /// never joined up.
    ///
    /// MUTATION: drop the caret clause from `Entry::absorb` — the depth falls to
    /// one and the first press empties the whole line.
    #[test]
    fn a_caret_jump_starts_a_new_entry() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "world");
        caret.place(&content, 0, false);
        typed(&mut log, &mut content, &mut caret, "hi");
        assert_eq!(content, "hiworld");
        assert_eq!(log.depth(), 2);
        log.undo(&mut content);
        assert_eq!(content, "world", "the second thought, and only it");
        log.undo(&mut content);
        assert_eq!(content, "");
    }

    /// RED — **a newline is its own entry, and it breaks the run on both sides.**
    ///
    /// MUTATION: stop asking `breaks()` in `Change::grain` — the three entries
    /// become one and Enter stops being a place an undo can stop at.
    #[test]
    fn a_newline_stands_alone() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "ab");
        type_in(&mut log, &mut content, &mut caret, "\n");
        typed(&mut log, &mut content, &mut caret, "cd");
        assert_eq!(content, "ab\ncd");
        assert_eq!(log.depth(), 3);
        log.undo(&mut content);
        assert_eq!(content, "ab\n");
        log.undo(&mut content);
        assert_eq!(content, "ab");
    }

    /// RED — **a change of direction breaks the run**, both ways round.
    ///
    /// Typing after deleting is not more deleting, and a Delete after a
    /// Backspace is a different gesture even though both take a character away.
    ///
    /// MUTATION: stop comparing `self.grain` in `Entry::absorb` — the deletions
    /// fold into the typing and one press undoes a correction the person made in
    /// two.
    #[test]
    fn a_change_of_direction_starts_a_new_entry() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "abcd");
        backspace(&mut log, &mut content, &mut caret);
        backspace(&mut log, &mut content, &mut caret);
        assert_eq!(content, "ab");
        assert_eq!(log.depth(), 2, "the typing, then the taking back");
        typed(&mut log, &mut content, &mut caret, "xy");
        assert_eq!(log.depth(), 3, "and typing again is a third");

        // The two deletions are two directions, not one run.
        let mut log = UndoLog::default();
        let mut content = "abcd".to_owned();
        let mut caret = collapsed(2);
        backspace(&mut log, &mut content, &mut caret);
        delete(&mut log, &mut content, &mut caret);
        assert_eq!(content, "ad");
        assert_eq!(log.depth(), 2);
    }

    /// RED — **a run of Backspace is one entry and gives every byte back at
    /// once**, and so is a run of Delete.
    ///
    /// The two directions grow an entry from opposite ends, which is the one
    /// piece of arithmetic in `Entry::absorb` that a reader cannot guess.
    ///
    /// MUTATION: push instead of prepending in the `DeleteBack` arm — the letters
    /// come back reversed.
    #[test]
    fn a_run_of_deletions_is_one_entry_in_either_direction() {
        let mut log = UndoLog::default();
        let mut content = "abcdef".to_owned();
        let mut caret = collapsed(6);
        for _ in 0..3 {
            backspace(&mut log, &mut content, &mut caret);
        }
        assert_eq!(content, "abc");
        assert_eq!(log.depth(), 1);
        assert_eq!(log.undo(&mut content), Some(collapsed(6)));
        assert_eq!(content, "abcdef");

        let mut log = UndoLog::default();
        let mut content = "abcdef".to_owned();
        let mut caret = collapsed(0);
        for _ in 0..3 {
            delete(&mut log, &mut content, &mut caret);
        }
        assert_eq!(content, "def");
        assert_eq!(log.depth(), 1);
        assert_eq!(log.undo(&mut content), Some(collapsed(0)));
        assert_eq!(content, "abcdef");
    }

    /// RED — **a paste is one entry however long it is**, and it closes the run
    /// before it and the run after it.
    ///
    /// MUTATION: classify by "the change is an insert" rather than by its length
    /// — the paste joins the typing before it and one press takes back both.
    #[test]
    fn a_paste_is_one_entry() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "ab");
        type_in(&mut log, &mut content, &mut caret, "a whole clipboard");
        typed(&mut log, &mut content, &mut caret, "cd");
        assert_eq!(log.depth(), 3);
        log.undo(&mut content);
        assert_eq!(content, "aba whole clipboard");
        log.undo(&mut content);
        assert_eq!(content, "ab");
    }

    /// RED — **undo and redo round-trip the bytes and the caret**, over a mixed
    /// session, all the way down and all the way back.
    ///
    /// MUTATION: file `after` where `before` belongs in `UndoLog::undo` — the
    /// caret comes back at the far end of the change it just took away.
    #[test]
    fn undo_and_redo_round_trip_the_bytes_and_the_caret() {
        let mut log = UndoLog::default();
        let mut content = "one\n".to_owned();
        let mut caret = collapsed(4);
        typed(&mut log, &mut content, &mut caret, "two");
        type_in(&mut log, &mut content, &mut caret, "\n");
        typed(&mut log, &mut content, &mut caret, "three");
        backspace(&mut log, &mut content, &mut caret);
        let full = content.clone();
        let ended = caret;

        // The body as it stood **before** each undo, newest first: the road out,
        // recorded on the way back down it.
        let mut states = Vec::new();
        while log.can_undo() {
            states.push(content.clone());
            log.undo(&mut content).expect("the log said there was one");
        }
        assert_eq!(content, "one\n", "back to the body it started from");

        // Each redo puts the body back into the state one more entry makes, so
        // the road out is that list read from its far end.
        while log.can_redo() {
            let caret = log.redo(&mut content).expect("the log said there was one");
            let wanted = states.pop().expect("one state per entry");
            assert_eq!(content, wanted, "the road back is the road out");
            if !log.can_redo() {
                assert_eq!(caret, ended, "and the caret the last keystroke left");
            }
        }
        assert!(states.is_empty(), "one redo per undo");
        assert_eq!(content, full);
    }

    /// RED — **a new edit after an undo drops the redo tail.**
    ///
    /// MUTATION: drop the `truncate` in `UndoLog::record` — a redo replays a
    /// change against bytes that are no longer there.
    #[test]
    fn a_new_edit_after_an_undo_drops_the_redo_tail() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "first");
        caret.place(&content, content.len(), false);
        type_in(&mut log, &mut content, &mut caret, "\n");
        typed(&mut log, &mut content, &mut caret, "second");
        assert_eq!(log.depth(), 3);

        log.undo(&mut content);
        log.undo(&mut content);
        assert_eq!(content, "first");
        assert!(log.can_redo());

        let mut caret = collapsed(content.len());
        typed(&mut log, &mut content, &mut caret, "!");
        assert_eq!(content, "first!");
        assert!(!log.can_redo(), "the branch nobody took is gone");
        assert_eq!(log.depth(), 2);
    }

    /// RED — **the dirty bit is a position in this log** (ticket T3 ③).
    ///
    /// Undoing back to the last save is being clean again; editing away from it
    /// is being dirty again; and a saved state the log can no longer reach — here
    /// because a new edit threw the branch it was on away — is dirty for ever
    /// after, because the bytes on the disk are nowhere in this history.
    ///
    /// MUTATION: return `self.cursor != 0` from `is_dirty` — the buffer reports
    /// itself clean at the top of the file instead of at the file.
    #[test]
    fn the_dirty_bit_is_the_log_position_at_the_last_save() {
        let mut log = UndoLog::default();
        let mut content = "body\n".to_owned();
        let mut caret = collapsed(5);
        assert!(!log.is_dirty(), "a body just read is clean");

        typed(&mut log, &mut content, &mut caret, "more");
        assert!(log.is_dirty());
        log.undo(&mut content);
        assert!(!log.is_dirty(), "back where the disk left it");
        log.redo(&mut content);
        assert!(log.is_dirty());

        log.mark_saved();
        assert!(!log.is_dirty(), "and a save is the other way to be clean");
        let mut caret = collapsed(content.len());
        typed(&mut log, &mut content, &mut caret, "!");
        assert!(log.is_dirty());
        log.undo(&mut content);
        assert!(!log.is_dirty());

        // Undo past the save, then edit: the saved position was on the branch
        // that has just been discarded.
        log.undo(&mut content);
        assert!(log.is_dirty(), "before the save is not the save");
        let mut caret = collapsed(content.len());
        typed(&mut log, &mut content, &mut caret, "?");
        assert!(log.is_dirty());
        while log.can_undo() {
            log.undo(&mut content);
            assert!(log.is_dirty(), "the file's bytes are no longer in this log");
        }
    }

    /// RED — **a save breaks the run**, so the entry either side of it can be
    /// undone on its own.
    ///
    /// MUTATION: leave `open` alone in `mark_saved` — the keystrokes after the
    /// save join the ones before it and one press undoes across a save the person
    /// watched happen.
    #[test]
    fn a_save_breaks_the_run() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "ab");
        log.mark_saved();
        typed(&mut log, &mut content, &mut caret, "cd");
        assert_eq!(log.depth(), 2);
        log.undo(&mut content);
        assert_eq!(content, "ab");
        assert!(!log.is_dirty());
    }

    /// RED — **an undo breaks the run too**, so typing after one is its own
    /// entry rather than an addition to the entry that was just taken back.
    ///
    /// MUTATION: leave `open` alone in `UndoLog::undo` — after undoing a run, one
    /// character typed joins the *older* entry and the two can never be separated
    /// again.
    #[test]
    fn typing_after_an_undo_starts_its_own_entry() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "ab");
        caret.place(&content, content.len(), false);
        typed(&mut log, &mut content, &mut caret, "cd");
        assert_eq!(log.depth(), 1, "one uninterrupted run");
        log.undo(&mut content);
        assert_eq!(content, "");

        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "z");
        assert_eq!(log.depth(), 1, "the tail went, and this is the new head");
        assert_eq!(content, "z");
    }

    /// RED — **the disk's copy empties the log**, because the body it described
    /// is gone.
    ///
    /// MUTATION: keep the entries and only clear `cursor` — an undo then replays
    /// an offset from the old body into the file's own bytes.
    #[test]
    fn the_disks_copy_empties_the_log() {
        let mut log = UndoLog::default();
        let mut content = "mine".to_owned();
        let mut caret = collapsed(4);
        typed(&mut log, &mut content, &mut caret, " and more");
        assert!(log.is_dirty());
        log.forget();
        assert_eq!(log.depth(), 0);
        assert!(!log.can_undo());
        assert!(!log.can_redo());
        assert!(!log.is_dirty(), "the body is the file's again");
    }

    /// RED — **a CRLF file's undo never leaves the caret between the `\r` and the
    /// `\n`.**
    ///
    /// The one seam `preview_edit::normalize` exists to keep a caret off, asked
    /// of the one door that can put an offset back into a body it was not
    /// measured against.
    ///
    /// MUTATION: return the entry's caret without healing it — the caret comes
    /// back inside the break and the very next Up or Down reads a column from the
    /// middle of a line ending.
    #[test]
    fn an_undo_over_crlf_never_lands_the_caret_inside_the_break() {
        let mut log = UndoLog::default();
        let mut content = "one\r\ntwo\r\n".to_owned();
        // The caret sits at the end of "one", one byte in front of the break.
        let mut caret = collapsed(3);
        typed(&mut log, &mut content, &mut caret, "xy");
        assert_eq!(content, "onexy\r\ntwo\r\n");
        let caret = log.undo(&mut content).expect("one entry");
        assert_eq!(content, "one\r\ntwo\r\n");
        assert_eq!(caret, collapsed(3));
        assert_eq!(
            caret.caret,
            preview_edit::normalize(&content, caret.caret),
            "the caret an undo hands back is one a caret may stand on"
        );

        // And a Backspace that swallows a whole break comes back whole, with the
        // caret where the keystroke found it — **the far side of the break, and
        // never the byte in the middle of it**, which is the offset a caret
        // restored without healing could land on the moment the two bytes are
        // back in the body.
        let mut log = UndoLog::default();
        let mut content = "one\r\ntwo".to_owned();
        let mut caret = collapsed(5);
        backspace(&mut log, &mut content, &mut caret);
        assert_eq!(content, "onetwo", "half a break is not a character");
        assert_eq!(caret.caret, 3);
        let caret = log.undo(&mut content).expect("one entry");
        assert_eq!(content, "one\r\ntwo");
        assert_eq!(caret.caret, 5, "where the hand was, and not 4");
        assert_eq!(caret.caret, preview_edit::normalize(&content, caret.caret));
    }

    /// RED — **a change is the smallest span that differs, on a character
    /// boundary in both bodies.**
    ///
    /// The multi-byte case is the one a byte-wise trim gets wrong: two
    /// characters that share a leading byte would otherwise leave a change
    /// starting in the middle of one of them.
    ///
    /// MUTATION: stop pulling `common_head` back to a boundary — the assertion on
    /// the accented pair panics on a slice that is not a boundary.
    #[test]
    fn a_change_is_the_smallest_span_that_differs() {
        let change = Change::implied("hello", "hello").is_none();
        assert!(change, "identical bodies are not a change");

        let change = Change::implied("abcdef", "abcXef").expect("one letter moved");
        assert_eq!(change.at, 3);
        assert_eq!(change.removed, "d");
        assert_eq!(change.inserted, "X");
        assert_eq!(change.before, collapsed(4));
        assert_eq!(change.after, collapsed(4));

        let change = Change::implied("aé", "aê").expect("the accent moved");
        assert_eq!(change.at, 1, "the shared lead byte is not a boundary");
        assert_eq!(change.removed, "é");
        assert_eq!(change.inserted, "ê");

        // A pure insertion in the middle names no removed bytes at all.
        let change = Change::implied("ac", "abc").expect("a letter arrived");
        assert_eq!((change.at, change.removed.as_str()), (1, ""));
        assert_eq!(change.inserted, "b");
    }

    /// RED — **the log stops growing at its cap, and says so about the save it
    /// can no longer reach.**
    ///
    /// MUTATION: drop the `saved` adjustment in `trim` — the log reports itself
    /// clean at a position that is now some other entry.
    #[test]
    fn the_log_stops_at_its_cap_and_forgets_the_oldest_first() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        // Each line is its own entry: the break is solid, and the letter after it
        // starts a fresh run.
        let mut caret = EditCaret::default();
        for _ in 0..(UndoLog::DEPTH + 8) {
            let was = content.clone();
            let before = caret;
            preview_edit::insert(&mut content, &mut caret, "x\n");
            let change = Change::between(&was, &content, before, caret).expect("an edit");
            log.record(change);
        }
        assert_eq!(log.depth(), UndoLog::DEPTH);
        assert!(log.is_dirty());
        // The save this body started from is off the front, so it can no longer
        // be undone back to.
        while log.can_undo() {
            log.undo(&mut content);
        }
        assert!(log.is_dirty(), "the saved state is out of reach");
        assert!(!content.is_empty(), "and the oldest lines stay in the body");
    }

    /// The run is closed on demand as well as by the ruled breakers — the door
    /// the buffer uses when something it cannot describe has happened to a body.
    #[test]
    fn a_sealed_run_takes_nothing_more() {
        let mut log = UndoLog::default();
        let mut content = String::new();
        let mut caret = EditCaret::default();
        typed(&mut log, &mut content, &mut caret, "ab");
        log.seal();
        typed(&mut log, &mut content, &mut caret, "cd");
        assert_eq!(log.depth(), 2);
    }
}
