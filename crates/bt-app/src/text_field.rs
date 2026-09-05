//! **One line of editable text**, and everything a caret does to it (T4, v2 ③).
//!
//! # Why this is a module and not a field on the graph
//!
//! This window already had two single-line editors when the graph asked for a
//! third: the tab-name editor in `main.rs`, and the preview's own edit surface.
//! Both grew their caret arithmetic where they stood, which is how a build ends
//! up with three answers to "what does `Ctrl+←` do at the start of a word" — and
//! two of them will be wrong in a different way.
//!
//! So the third one is written here instead, with no idea what it is inside:
//! it holds a `String`, a caret, a selection anchor and an IME composition, and
//! it answers commands. It does not know about fonts, rectangles, seats or git.
//! What that buys immediately is a search field with real word-jump and real
//! shift-selection for the cost of the tests below; what it buys later is that
//! the in-pane search the preview is owed can have the same field rather than a
//! fourth one.
//!
//! **The tab rename should migrate to this**, and is deliberately not migrated
//! by this slice — recorded in `docs/DESIGN.md` §7.1.3g rather than left as a
//! comment nobody will find. Moving it is a change to a surface with its own
//! tests and its own IME path, and doing it in the same breath as introducing
//! the type would mean the type's first proof was a refactor rather than a
//! feature.
//!
//! # Bytes, not characters
//!
//! The caret is a **byte index into a `String`**, always on a character
//! boundary. The alternative — a character count — is a number that has to walk
//! the string to be used for anything, and every use here is either a slice or a
//! comparison. Every motion below lands on a boundary by construction, because
//! every one of them is computed from `char_indices`.

/// Where a caret can be asked to go.
///
/// **A closed list and not a key code**, for the reason `crate::files`'s own
/// `TreeCommand` is one: what `Home` does to a line is a fact about the line,
/// and it has to be assertable without a keyboard. The translation from winit
/// lives beside the window that reads winit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextMove {
    Left,
    Right,
    /// To the start of the word behind the caret — `Ctrl+←`.
    WordLeft,
    /// To the end of the word ahead of it — `Ctrl+→`.
    WordRight,
    Home,
    End,
}

/// One line of text with a caret in it.
///
/// `Default` is an empty field with the caret at zero, which is what a field
/// nobody has typed into is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextField {
    text: String,
    /// Byte index, on a character boundary.
    caret: usize,
    /// The other end of the selection, also a byte index. **Equal to the caret
    /// when there is no selection**, rather than an `Option`: every motion moves
    /// the caret and then either drags the anchor along or leaves it, and a
    /// separate "is there a selection" flag is a second thing to keep in step
    /// with a fact the two numbers already state.
    anchor: usize,
    /// What the IME is composing, which is **not part of [`Self::text`]**.
    ///
    /// A composition is text the reader has not committed: it is drawn at the
    /// caret and it disappears if they press Escape, so folding it into the
    /// buffer would mean un-typing it on cancel — which is the bug every
    /// hand-rolled IME field has. It arrives through [`Self::set_preedit`] and
    /// leaves through the commit, which is an ordinary [`Self::insert`].
    preedit: String,
}

impl TextField {
    /// A field already holding this text, caret at the end.
    ///
    /// At the end and not at the start, because what it is for is "restore what
    /// was typed here" and a caret at zero would put the next keystroke in front
    /// of it.
    ///
    /// Only the tests build one this way today — the graph's field is born empty
    /// and typed into. It is kept because it is the one constructor a *restore*
    /// would use, and because every test below would otherwise open with four
    /// lines of `insert`.
    #[allow(dead_code)]
    #[must_use]
    pub fn holding(text: &str) -> Self {
        Self {
            caret: text.len(),
            anchor: text.len(),
            text: text.to_owned(),
            preedit: String::new(),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Where the caret is, as a byte index — what the tests assert against and
    /// what a second reader of this field would place a candidate window from.
    #[allow(dead_code)]
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected range, low end first. Empty when there is no selection.
    #[must_use]
    pub fn selection(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// The text in front of the caret — what a painter measures to place it.
    ///
    /// The **caret's own** side and not the selection's, because a caret is
    /// where the next character goes however the selection was dragged.
    #[must_use]
    pub fn before_caret(&self) -> &str {
        &self.text[..self.caret]
    }

    #[must_use]
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// **What the field reads as right now** — the committed text with the
    /// composition spliced in where it is drawn.
    ///
    /// This is the string a reader is looking at, and it is deliberately *not*
    /// [`Self::text`]: the buffer is what survives an Escape, and what is on
    /// the glass is the buffer with the pre-edit standing at the caret. A
    /// surface that filters a list as you type has to filter against what is on
    /// the glass, or a Chinese query narrows nothing until the moment it is
    /// committed — which is a box that appears to ignore half of what was typed
    /// into it.
    ///
    /// Spliced at the caret rather than over the selection, on the painter's own
    /// arithmetic: a composition pushes the caret along and does not consume the
    /// selection until it commits, and one reading is what keeps the string that
    /// is measured and the string that is queried the same string.
    ///
    /// Borrowed when there is no composition, which is the ordinary case and
    /// every keystroke of it.
    #[must_use]
    pub fn composed(&self) -> std::borrow::Cow<'_, str> {
        if self.preedit.is_empty() {
            return std::borrow::Cow::Borrowed(&self.text);
        }
        let mut composed = String::with_capacity(self.text.len() + self.preedit.len());
        composed.push_str(&self.text[..self.caret]);
        composed.push_str(&self.preedit);
        composed.push_str(&self.text[self.caret..]);
        std::borrow::Cow::Owned(composed)
    }

    /// What the IME is composing right now, or nothing when it has finished.
    pub fn set_preedit(&mut self, text: &str) {
        self.preedit.clear();
        self.preedit.push_str(text);
    }

    /// Put text in, replacing whatever is selected.
    ///
    /// The one door every character comes through — a keystroke, an IME commit,
    /// a paste — so "typing replaces the selection" is one rule written once.
    pub fn insert(&mut self, text: &str) {
        let range = self.selection();
        self.text.replace_range(range.clone(), text);
        self.caret = range.start + text.len();
        self.anchor = self.caret;
        self.preedit.clear();
    }

    /// Backspace: the selection if there is one, otherwise the character behind.
    ///
    /// Returns whether anything changed, so a caller can tell "nothing happened"
    /// from "something did" without comparing strings — which is what decides
    /// whether the search is re-asked.
    pub fn backspace(&mut self) -> bool {
        if self.selection().is_empty() {
            let Some(at) = self.previous_boundary(self.caret) else {
                return false;
            };
            self.anchor = at;
        }
        self.delete_selection()
    }

    /// Delete: the selection if there is one, otherwise the character ahead.
    pub fn delete(&mut self) -> bool {
        if self.selection().is_empty() {
            let Some(at) = self.next_boundary(self.caret) else {
                return false;
            };
            self.anchor = at;
        }
        self.delete_selection()
    }

    /// `Ctrl+Backspace`: the selection if there is one, otherwise the word
    /// behind the caret.
    ///
    /// The word is [`Self::word_boundary`]'s, which is the same edge `Ctrl+←`
    /// stops at — so "delete a word" and "walk a word" cannot disagree about
    /// where a word starts, which is the way a hand-rolled pair of these ends up
    /// eating a hyphen in one direction and not the other.
    pub fn delete_word_back(&mut self) -> bool {
        if self.selection().is_empty() {
            self.anchor = self.word_boundary(false);
        }
        self.delete_selection()
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.caret = range.start;
        self.anchor = range.start;
        self.preedit.clear();
        true
    }

    /// Everything, selected — `Ctrl+A`.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// Empty the field and the composition with it.
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        self.preedit.clear();
    }

    /// Move the caret, dragging the selection's far end along when `select`.
    ///
    /// **An unselecting motion collapses to the edge it moved towards**, which
    /// is what every text field on this platform does and is not the same as
    /// "move one character from the caret": with `abc` selected, pressing `←`
    /// puts the caret at `a` and pressing `→` puts it after `c`, and neither
    /// moves it a character further. Written out because the naive version — drop
    /// the selection, then move — eats a character every time.
    pub fn step(&mut self, motion: TextMove, select: bool) {
        let selection = self.selection();
        if !select && !selection.is_empty() && matches!(motion, TextMove::Left | TextMove::Right) {
            self.caret = match motion {
                TextMove::Left => selection.start,
                _ => selection.end,
            };
            self.anchor = self.caret;
            return;
        }
        self.caret = match motion {
            TextMove::Left => self.previous_boundary(self.caret).unwrap_or(0),
            TextMove::Right => self.next_boundary(self.caret).unwrap_or(self.text.len()),
            TextMove::WordLeft => self.word_boundary(false),
            TextMove::WordRight => self.word_boundary(true),
            TextMove::Home => 0,
            TextMove::End => self.text.len(),
        };
        if !select {
            self.anchor = self.caret;
        }
    }

    fn previous_boundary(&self, from: usize) -> Option<usize> {
        self.text[..from]
            .char_indices()
            .next_back()
            .map(|(at, _)| at)
    }

    fn next_boundary(&self, from: usize) -> Option<usize> {
        self.text[from..]
            .chars()
            .next()
            .map(|first| from + first.len_utf8())
    }

    /// The next word edge in one direction.
    ///
    /// **Two classes and not a dictionary**: a run of word characters, or a run
    /// of anything else, with whitespace skipped on the way out. That is what
    /// every editor's `Ctrl+←` does, and it is the rule that makes
    /// `refactor(git): fix` walk in the five steps a reader expects rather than
    /// in one.
    fn word_boundary(&self, forwards: bool) -> usize {
        let chars: Vec<(usize, char)> = self.text.char_indices().collect();
        let end = self.text.len();
        // The caret's position in `chars`, as an index into the list of starts.
        let at = chars
            .iter()
            .position(|(start, _)| *start >= self.caret)
            .unwrap_or(chars.len());
        if forwards {
            let mut index = at;
            while index < chars.len() && chars[index].1.is_whitespace() {
                index += 1;
            }
            let word = index < chars.len() && is_word(chars[index].1);
            while index < chars.len()
                && !chars[index].1.is_whitespace()
                && is_word(chars[index].1) == word
            {
                index += 1;
            }
            chars.get(index).map_or(end, |(start, _)| *start)
        } else {
            let mut index = at;
            while index > 0 && chars[index - 1].1.is_whitespace() {
                index -= 1;
            }
            let word = index > 0 && is_word(chars[index - 1].1);
            while index > 0
                && !chars[index - 1].1.is_whitespace()
                && is_word(chars[index - 1].1) == word
            {
                index -= 1;
            }
            chars.get(index).map_or(end, |(start, _)| *start)
        }
    }
}

/// Whether a character belongs to a word, for the word walk.
///
/// Alphanumeric or `_`, which is the rule the shell's own word motions use and
/// the one a reader typing `fix_the_thing` means. Deliberately not "not
/// punctuation": a hyphenated branch name is two words to every editor there is,
/// and pretending otherwise here would make one field disagree with the terminal
/// beside it.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// **What a clipboard can put into a field that holds one line.**
///
/// The first line and nothing after it, with the control characters taken out
/// of what is left. Two decisions, and each has an alternative worth naming:
///
/// * **The first line, not every line joined.** A field with one row can only
///   show one, so a multi-line paste has to lose something either way; losing
///   the rows after the first is a loss the reader can see and undo, while
///   joining them puts a query on the glass that matches nothing and looks like
///   one thing they typed.
/// * **Control characters removed, not escaped or kept.** A tab, a NUL or the
///   `\u{1b}` at the head of an escape sequence are not characters anybody meant
///   to search for; kept, they are invisible glyphs that make a query fail for a
///   reason nothing on the glass explains.
///
/// A free function because it is a fact about the clipboard and a one-line
/// field, not about any particular one — the caller inserts the answer through
/// the same [`TextField::insert`] a keystroke goes through.
#[must_use]
pub fn one_line(text: &str) -> String {
    text.split(['\r', '\n'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|ch| !ch.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T4 — typing goes in at the caret, and a selection is what it replaces.
    #[test]
    fn typing_lands_at_the_caret_and_replaces_whatever_is_selected() {
        let mut field = TextField::default();
        field.insert("fix");
        assert_eq!(field.text(), "fix");
        assert_eq!(field.caret(), 3);
        field.step(TextMove::Home, false);
        field.insert("hot");
        assert_eq!(field.text(), "hotfix");
        assert_eq!(field.caret(), 3, "the caret follows what was typed");

        field.select_all();
        assert_eq!(field.selection(), 0..6);
        field.insert("x");
        assert_eq!(field.text(), "x", "typing over a selection replaces it");
        assert_eq!(field.selection(), 1..1, "and leaves nothing selected");
    }

    /// T4 — backspace and delete take a character, or the selection when there
    /// is one, and say whether they took anything.
    #[test]
    fn backspace_and_delete_take_one_character_or_the_whole_selection() {
        let mut field = TextField::holding("abc");
        assert!(field.backspace());
        assert_eq!(field.text(), "ab");
        field.step(TextMove::Home, false);
        assert!(
            !field.backspace(),
            "there is nothing behind the start of the line"
        );
        assert!(field.delete());
        assert_eq!(field.text(), "b");
        assert!(field.delete(), "the last character still goes");
        assert!(field.is_empty());
        assert!(!field.delete(), "and an empty line has nothing to take");

        let mut field = TextField::holding("abcdef");
        field.step(TextMove::Home, false);
        field.step(TextMove::Right, true);
        field.step(TextMove::Right, true);
        assert_eq!(field.selection(), 0..2);
        assert!(field.backspace());
        assert_eq!(field.text(), "cdef", "the selection, not one character");
    }

    /// T4 — Home and End, and shift-selection to each.
    #[test]
    fn home_and_end_reach_the_ends_and_shift_selects_on_the_way() {
        let mut field = TextField::holding("branch name");
        field.step(TextMove::Home, false);
        assert_eq!(field.caret(), 0);
        assert!(field.selection().is_empty());
        field.step(TextMove::End, true);
        assert_eq!(field.selection(), 0..11, "shift dragged the far end along");
        field.step(TextMove::Home, false);
        assert!(
            field.selection().is_empty(),
            "a motion without shift drops the selection"
        );
    }

    /// T4 — an unselecting arrow collapses to the edge it moved towards rather
    /// than stepping a character past it.
    #[test]
    fn an_arrow_out_of_a_selection_lands_on_its_edge() {
        let mut field = TextField::holding("abcdef");
        field.step(TextMove::Home, false);
        field.step(TextMove::Right, true);
        field.step(TextMove::Right, true);
        field.step(TextMove::Right, true);
        assert_eq!(field.selection(), 0..3);
        field.step(TextMove::Left, false);
        assert_eq!(field.caret(), 0, "left goes to the selection's own start");

        field.select_all();
        field.step(TextMove::Right, false);
        assert_eq!(field.caret(), 6, "and right to its end");
    }

    /// T4 — the word walk crosses runs of word characters and runs of
    /// punctuation separately, and skips the whitespace between them.
    #[test]
    fn the_word_walk_stops_at_every_edge_a_reader_expects() {
        let mut field = TextField::holding("fix(git): the thing");
        field.step(TextMove::Home, false);
        let mut stops = Vec::new();
        for _ in 0..8 {
            field.step(TextMove::WordRight, false);
            stops.push(field.caret());
        }
        assert_eq!(
            stops,
            vec![3, 4, 7, 9, 13, 19, 19, 19],
            "fix | ( | git | ): | the | thing, and then the end holds"
        );

        let mut back = Vec::new();
        for _ in 0..8 {
            field.step(TextMove::WordLeft, false);
            back.push(field.caret());
        }
        assert_eq!(
            back,
            vec![14, 10, 7, 4, 3, 0, 0, 0],
            "the same edges, walked the other way"
        );
    }

    /// T4 — a word jump with shift selects what it crossed.
    #[test]
    fn a_word_jump_with_shift_selects_what_it_crossed() {
        let mut field = TextField::holding("one two");
        field.step(TextMove::WordLeft, true);
        assert_eq!(field.selection(), 4..7);
        field.insert("three");
        assert_eq!(field.text(), "one three");
    }

    /// T4 — a composition is drawn but is not in the text until it is committed,
    /// and a cancelled one leaves nothing behind.
    #[test]
    fn a_composition_is_not_in_the_text_until_it_is_committed() {
        let mut field = TextField::holding("v");
        field.set_preedit("ni'hao");
        assert_eq!(field.text(), "v", "the buffer is untouched while composing");
        assert_eq!(field.preedit(), "ni'hao");
        field.set_preedit("");
        assert_eq!(
            field.text(),
            "v",
            "and a cancelled composition un-types nothing"
        );
        field.set_preedit("ni");
        field.insert("\u{4f60}");
        assert_eq!(field.text(), "v\u{4f60}");
        assert!(
            field.preedit().is_empty(),
            "a commit ends the composition it came out of"
        );
    }

    /// T4 — every motion lands on a character boundary, whatever is in the line.
    #[test]
    fn every_motion_lands_on_a_character_boundary() {
        let mut field = TextField::holding("a\u{4f60}\u{597d}b");
        field.step(TextMove::Home, false);
        for _ in 0..6 {
            field.step(TextMove::Right, false);
            assert!(field.text().is_char_boundary(field.caret()));
        }
        assert_eq!(field.caret(), field.text().len());
        for _ in 0..6 {
            field.step(TextMove::Left, false);
            assert!(field.text().is_char_boundary(field.caret()));
        }
        assert_eq!(field.caret(), 0);
        assert_eq!(field.before_caret(), "");
    }

    /// PIN — **what the field reads as is the buffer with the composition
    /// standing in it** (user ruling 2026-09-05).
    ///
    /// The half of the palette's live narrowing that is a fact about a field
    /// rather than about a list: `composed` is the string on the glass, `text`
    /// is the string that survives an Escape, and they are two different
    /// questions with two different answers.
    ///
    /// MUTATIONS:
    /// (1) return `text` from `composed` — the second assertion goes red, and a
    ///     composed query narrows nothing until it commits;
    /// (2) splice the pre-edit at the end instead of the caret — the third goes
    ///     red, and the string that is filtered stops being the string that is
    ///     drawn;
    /// (3) fold the pre-edit into `text` — the fourth goes red, and cancelling
    ///     a composition would have to un-type it.
    #[test]
    fn the_composed_reading_is_the_buffer_with_the_composition_in_it() {
        let mut field = TextField::holding("git");
        assert_eq!(
            field.composed(),
            "git",
            "with nothing composing, it is the text"
        );

        field.set_preedit("ni");
        assert_eq!(
            field.composed(),
            "gitni",
            "a composition is part of the reading"
        );

        field.step(TextMove::Home, false);
        assert_eq!(
            field.composed(),
            "nigit",
            "and it stands where the caret is, which is where it is drawn"
        );

        assert_eq!(field.text(), "git", "the buffer never saw it");
        field.set_preedit("");
        assert_eq!(
            field.composed(),
            "git",
            "so cancelling restores the reading"
        );
    }

    /// PIN — **`Ctrl+Backspace` deletes to the same edge `Ctrl+←` walks to.**
    ///
    /// MUTATIONS:
    /// (1) delete to the previous whitespace instead — the second case keeps
    ///     `(` and this goes red;
    /// (2) ignore the selection and always eat a word — the last case goes red,
    ///     and a selected line loses a word beyond it.
    #[test]
    fn ctrl_backspace_takes_the_word_the_word_walk_would_have_crossed() {
        let mut field = TextField::holding("fix the thing");
        assert!(field.delete_word_back());
        assert_eq!(field.text(), "fix the ");

        let mut punctuated = TextField::holding("fix(git");
        assert!(punctuated.delete_word_back());
        assert_eq!(punctuated.text(), "fix(", "punctuation is its own run");
        assert!(punctuated.delete_word_back());
        assert_eq!(punctuated.text(), "fix");

        let mut empty = TextField::default();
        assert!(!empty.delete_word_back(), "nothing behind, nothing deleted");

        let mut selected = TextField::holding("fix the thing");
        selected.select_all();
        assert!(selected.delete_word_back());
        assert_eq!(selected.text(), "", "a selection is what a delete takes");
    }

    /// PIN — **a clipboard reaches a one-line field as one line of printable
    /// text** (user ruling 2026-09-05).
    ///
    /// MUTATIONS:
    /// (1) join the lines with a space instead of cutting — the second
    ///     assertion goes red;
    /// (2) keep the control characters — the third goes red, and a query holds
    ///     glyphs nothing on the glass explains.
    #[test]
    fn a_pasted_clipboard_is_cut_to_its_first_line_and_stripped() {
        assert_eq!(one_line("settings"), "settings");
        assert_eq!(
            one_line("first\r\nsecond\r\nthird"),
            "first",
            "the rows after the first are not this field's to show"
        );
        assert_eq!(
            one_line("a\tb\u{0}c\u{1b}d"),
            "abcd",
            "and a control character is not a character anybody searched for"
        );
        assert_eq!(one_line(""), "");
        assert_eq!(one_line("\nsecond"), "", "the first line is the first line");
        assert_eq!(
            one_line("\u{4f60}\u{597d}\nthere"),
            "\u{4f60}\u{597d}",
            "the cut is by line and not by byte"
        );
    }
}
