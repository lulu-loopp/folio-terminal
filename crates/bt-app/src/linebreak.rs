//! **Where a line may end** — the one rule table every wrapper in this window
//! reads (user ruling 2026-08-29).
//!
//! This file holds no wrapper. It answers one question — *given these two
//! adjacent characters, may a line end between them?* — and hands the answer
//! out as a list of the smallest runs a line may end after. The two greedy
//! fills in this crate ([`crate::restore::wrap`], which measures whole
//! candidate lines, and [`crate::tooltip::wrap`], which accumulates piece
//! widths) then differ only in how they fill, never in where they are allowed
//! to break.
//!
//! **Why it is a file of its own.** The rules were written once, for the
//! restore prompt, and the settings dialog wrapped its rows with the other
//! wrapper — so the Agent page drew 「打开后，Folio 在 Claude Code」 on a line
//! and stopped there with more than half the column empty, because the run that
//! came next carries no space and a space-only fill has to move the whole of it
//! or none of it. A rule table that lives inside one surface is a rule table the
//! next surface does without; the user ruling that followed the screenshot says
//! in as many words that there is to be one function and not one per page.
//!
//! **A character-class rule, not UAX#14.** The real algorithm has thirty-odd
//! classes, a pair table and tailoring, and it exists to serve arbitrary text in
//! any script; what is wrapped here is this product's own prose, in two
//! languages, at a handful of measures. Five rules cover it, and each is written
//! down because a rule that is not written down is a rule the next person
//! deletes:
//!
//! 1. **A space is a break opportunity, and the space is not drawn at the head
//!    of the line that follows it.** Runs of spaces collapse, which is what
//!    `white-space: pre-line` does with them; the newline the same declaration
//!    keeps is the caller's business, because only the caller knows whether its
//!    box has paragraphs in it.
//! 2. **Break between two adjacent characters when either is CJK.** Ideographs,
//!    kana and Hangul are written without spaces and a line may end after any of
//!    them; the Latin/CJK boundary is a break opportunity too, so `Folio时` may
//!    split even though nothing separates them.
//! 3. **Never break *before* closing punctuation** — `，。、！？；：）》」』` and
//!    their Latin equivalents. A line that began with `。` would be the single
//!    most obviously wrong thing this could do.
//! 4. **Never break *after* opening punctuation** — `（《「『` and `([{`. The
//!    mirror of rule 3, and it is a separate rule rather than a symmetry because
//!    the two sets are not each other's mirror image in Unicode.
//! 5. **A Latin word is never cut inside itself.** Rules 2 to 4 reach no
//!    boundary in `settings.json`, and that is deliberate: a word or a path
//!    shorter than the line is drawn whole, and one longer than the line is the
//!    wrapper's own last resort to cut — [`crate::tooltip::wrap`]'s `break_word`
//!    and [`crate::restore::wrap_anywhere`], both of which prefer a path joint
//!    to a letter boundary.
//!
//! [`PathSeparators`] is the one thing the two callers do not agree on, and it
//! is a parameter rather than a fork of the table — see its own note.
//!
//! What is knowingly given up: no line-break class for the numeric and unit
//! runs (`64 KB` is protected only by having a space in it, as it is in
//! English), no tailoring for the two Chinese conventions about `·`, and no
//! hyphenation in either language. None of the three is reachable from the
//! sentences this crate actually wraps.

/// One atom of a paragraph, and whether a space stood in front of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece<'a> {
    /// The run itself, never empty and never containing a space.
    pub text: &'a str,
    /// A space separated this run from the one before it, so a fill that keeps
    /// both on one line owes a space between them — and a fill that breaks here
    /// owes nothing, which is rule 1's second half.
    pub space_before: bool,
}

/// Whether `\` and `/` are break opportunities in their own right.
///
/// **Not a second rule table — a measure.** [`crate::restore`]'s dialog is 400
/// logical pixels wide and its sentences name places on disk (the PSReadLine
/// invitation, §7.1.6c-3b): one space-less token eighty characters long, which
/// rules 2 to 4 do not reach because nothing in it is CJK, and which ran out of
/// both edges of the dialog until the separator became an opportunity there.
/// The prose columns — a settings row's sentence, a tip, a toast — are wide
/// enough that the same path fits, and breaking it anyway would cut a Latin word
/// that had room, against rule 5. So each caller says which of the two it is,
/// and neither writes the rule out again.
///
/// *After* a separator and never before one, in both settings: a line ending in
/// `\` reads as "continues", and a line beginning with one reads as a UNC share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathSeparators {
    /// A line may end after `\` or `/` — the narrow dialog's setting.
    Break,
    /// A path is one Latin word and rule 5 holds it whole — the prose columns'
    /// setting, and the wrapper's over-wide fallback still cuts it if it must.
    Whole,
}

/// Cut `text` into the smallest runs a line may end after.
///
/// The caller has already dealt with any `\n` it wanted to honour; every other
/// run of whitespace is rule 1's break and collapses to a single
/// [`Piece::space_before`].
#[must_use]
pub fn pieces(text: &str, paths: PathSeparators) -> Vec<Piece<'_>> {
    let mut pieces = Vec::new();
    for (index, word) in text.split_whitespace().enumerate() {
        let mut start = 0usize;
        let mut previous: Option<char> = None;
        for (offset, character) in word.char_indices() {
            if let Some(previous) = previous
                && breaks_between(previous, character, paths)
            {
                pieces.push(Piece {
                    text: &word[start..offset],
                    space_before: index > 0 && start == 0,
                });
                start = offset;
            }
            previous = Some(character);
        }
        pieces.push(Piece {
            text: &word[start..],
            space_before: index > 0 && start == 0,
        });
    }
    pieces
}

/// Whether a line may end between these two characters — see this module's own
/// note for the five rules and for what they deliberately leave out.
#[must_use]
pub fn breaks_between(before: char, after: char, paths: PathSeparators) -> bool {
    // Ahead of the CJK gate because it is not about script: where it is on at
    // all, a path separator is a break opportunity in an English sentence as
    // much as in a Chinese one.
    if paths == PathSeparators::Break
        && matches!(before, '\\' | '/')
        && !matches!(after, '\\' | '/')
    {
        return true;
    }
    if !is_cjk(before) && !is_cjk(after) {
        return false;
    }
    !no_break_before(after) && !no_break_after(before)
}

/// The scripts written without spaces, plus the punctuation that belongs to
/// them.
///
/// The ranges are blocks rather than a property lookup, which is the same trade
/// `bt-unicode` makes for width: the answer only has to be right for text this
/// product writes, and every one of these blocks is unambiguous.
#[must_use]
pub fn is_cjk(character: char) -> bool {
    matches!(character,
        '\u{1100}'..='\u{11ff}'      // Hangul Jamo
        | '\u{2e80}'..='\u{2eff}'    // CJK radicals
        | '\u{3000}'..='\u{303f}'    // CJK symbols and punctuation
        | '\u{3040}'..='\u{30ff}'    // Hiragana, Katakana
        | '\u{3130}'..='\u{318f}'    // Hangul compatibility jamo
        | '\u{3400}'..='\u{4dbf}'    // CJK extension A
        | '\u{4e00}'..='\u{9fff}'    // CJK unified ideographs
        | '\u{a960}'..='\u{a97f}'    // Hangul jamo extended A
        | '\u{ac00}'..='\u{d7ff}'    // Hangul syllables
        | '\u{f900}'..='\u{faff}'    // CJK compatibility ideographs
        | '\u{fe30}'..='\u{fe4f}'    // CJK compatibility forms
        | '\u{ff00}'..='\u{ff60}'    // Fullwidth forms
        | '\u{ffe0}'..='\u{ffe6}'    // Fullwidth signs
        | '\u{20000}'..='\u{3ffff}'  // CJK extensions B and beyond
    )
}

/// Punctuation a line may never begin with.
#[must_use]
pub fn no_break_before(character: char) -> bool {
    matches!(
        character,
        '，' | '。'
            | '、'
            | '！'
            | '？'
            | '；'
            | '：'
            | '·'
            | '…'
            | '‥'
            | '）'
            | '〕'
            | '】'
            | '》'
            | '〉'
            | '」'
            | '』'
            | '〗'
            | '〙'
            | '〛'
            | '〞'
            | '＂'
            | '％'
            | '℃'
            | 'ー'
            | '～'
            | '〜'
            | ','
            | '.'
            | '!'
            | '?'
            | ';'
            | ':'
            | ')'
            | ']'
            | '}'
            | '%'
    )
}

/// Punctuation a line may never end with.
#[must_use]
pub fn no_break_after(character: char) -> bool {
    matches!(
        character,
        '（' | '〔'
            | '【'
            | '《'
            | '〈'
            | '「'
            | '『'
            | '〖'
            | '〘'
            | '〚'
            | '〝'
            | '('
            | '['
            | '{'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(text: &str, paths: PathSeparators) -> Vec<&str> {
        pieces(text, paths).iter().map(|piece| piece.text).collect()
    }

    /// PIN (user ruling 2026-08-29) — **rule 5: English is cut at spaces and
    /// nowhere else.**
    ///
    /// The whole of the CJK pass is supposed to be invisible to the language
    /// that has spaces in it, and this is the pin that says so. Every wrapper in
    /// the crate reads this list, so a rule that reached inside a Latin word
    /// would move an English line on four surfaces at once.
    #[test]
    fn a_latin_word_is_one_piece() {
        assert_eq!(runs("one two three", PathSeparators::Whole), [
            "one", "two", "three"
        ]);
        assert_eq!(runs("~/.claude/settings.json", PathSeparators::Whole), [
            "~/.claude/settings.json"
        ]);
        assert_eq!(runs("Alt+wheel, 12.5px (100%)", PathSeparators::Whole), [
            "Alt+wheel,",
            "12.5px",
            "(100%)"
        ]);
        // Rule 1: a run of spaces is one opportunity, and the head of the next
        // line carries none of it.
        let spaced = pieces("one  two", PathSeparators::Whole);
        assert_eq!(runs("one  two", PathSeparators::Whole), ["one", "two"]);
        assert!(!spaced[0].space_before && spaced[1].space_before);
    }

    /// PIN — **rule 2: every ideograph boundary is an opportunity, and so is
    /// the seam between a Han run and a Latin one.**
    #[test]
    fn chinese_breaks_between_any_two_characters() {
        assert_eq!(runs("重新打开", PathSeparators::Whole), [
            "重", "新", "打", "开"
        ]);
        assert_eq!(runs("重启Folio以切换", PathSeparators::Whole), [
            "重", "启", "Folio", "以", "切", "换"
        ]);
    }

    /// PIN — **rules 3 and 4: the punctuation that may not start a line and the
    /// punctuation that may not end one.**
    ///
    /// MUTATIONS: drop [`no_break_before`] and 「。」 becomes a piece of its own,
    /// free to open a line; drop [`no_break_after`] and 「（」 becomes the last
    /// piece of one.
    #[test]
    fn punctuation_hangs_rather_than_starting_or_ending_a_line() {
        assert_eq!(runs("你好。再见", PathSeparators::Whole), [
            "你", "好。", "再", "见"
        ]);
        // 「入」 and 「（」 are two pieces — a line may end after 入 and the
        // bracket opens the next one, which is right. What rule 4 forbids is the
        // bracket *ending* a line, and it does not: 「（例」 is one atom.
        assert_eq!(runs("写入（例如）时", PathSeparators::Whole), [
            "写", "入", "（例", "如）", "时"
        ]);
        for piece in pieces("目录，标记；说明：完（成）了「引」用", PathSeparators::Whole) {
            let first = piece.text.chars().next().expect("no piece is empty");
            let last = piece.text.chars().next_back().expect("no piece is empty");
            assert!(
                !no_break_before(first),
                "{piece:?} could open a line with closing punctuation"
            );
            assert!(
                !no_break_after(last),
                "{piece:?} could end a line with opening punctuation"
            );
        }
    }

    /// PIN — **[`PathSeparators`] is the only thing the two callers disagree
    /// about, and it disagrees in one direction.**
    #[test]
    fn a_path_is_whole_in_prose_and_jointed_in_the_narrow_dialog() {
        assert_eq!(runs(r"C:\Users\me\Documents", PathSeparators::Break), [
            r"C:\", r"Users\", r"me\", "Documents"
        ]);
        assert_eq!(runs(r"C:\Users\me\Documents", PathSeparators::Whole), [
            r"C:\Users\me\Documents"
        ]);
        // A UNC root is two separators and the opportunity is after the pair,
        // never inside it — so `\\` is one atom and no line can begin with the
        // second backslash.
        assert_eq!(runs(r"\\server\share", PathSeparators::Break), [
            r"\\",
            r"server\",
            "share"
        ]);
    }
}
