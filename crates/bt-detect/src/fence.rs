//! **CommonMark fenced code blocks** (spec 0.31.2 §4.5), as one grammar for the terminal and the
//! markdown preview.
//!
//! # Why this is exported
//!
//! A fence decides what everything below it is: inside one, a `$$` is not mathematics, a pipe row
//! is not a table, a `#` is not a heading. The terminal's detector asks it of every printed line
//! (`structural_line_step`, the pipe-table pass, the screen-fence pass); `bt-app`'s preview asks it
//! of a document's own lines. Two grammars for the one question are a block that is code in a pane
//! and prose in the preview beside it — which is what the preview's private `strip_prefix("```")`
//! was until T-FENCE-GRAMMAR-SHARED (2026-10-21): no `~~~`, no indented opener or closer, and a
//! close on any line that began with three backticks. So the grammar lives here, and both readers
//! ask it; the state machine that carries a fence from line to line stays each reader's own.
//!
//! # The grammar (every shape these functions answer for)
//!
//! Each function reads **one line without its terminator**. Indentation is counted in columns, a
//! tab advancing to the next multiple of [`TAB_STOP`] (§2.2).
//!
//! | The line | [`fence_opening`] | [`fence_closes`] (against an open fence) |
//! |---|---|---|
//! | 0–3 spaces, then 3 or more `` ` ``, then text holding no `` ` `` (§4.5 ex. 119, 131–133, 142, 144) | an opening; its info string is the text | closes when the run is the opening's character, at least as long, and the text is only spaces and tabs (ex. 124, 135, 136) |
//! | 0–3 spaces, then 3 or more `~`, then any text (ex. 120, 143, 146) | an opening; the text may hold backticks | as above, with `~` (ex. 125) |
//! | a backtick run whose rest holds a `` ` `` (ex. 138, 145) | `None` — it is a code span (§6.1), not a fence | does not close (ex. 147: a closer has no info string) |
//! | fewer than 3 of the character (ex. 121) | `None` | does not close |
//! | 4 or more columns of indentation, a tab counting to its stop (ex. 134, 137) | `None` — it is indented code (§4.4), [`is_indented_code`] | does not close |
//! | the other fence character, or a shorter run (ex. 122, 123, 124) | an opening, read as above | does not close |
//! | a run followed by anything that is not a space or a tab — `~~~ ~~` (ex. 139), U+3000, U+00A0, a `\r` left in the line | as above | does not close: the spec allows "only spaces or tabs" after a closer |
//! | any other first character after the indentation | `None` | does not close |
//!
//! **The info string** is the text after the opening run, trimmed of spaces and tabs (and only
//! those). [`FenceOpening::language`] is its first word, split at a space or a tab — what the
//! spec's renderers turn into `language-…`. The spec decodes backslash escapes and entity
//! references in an info string; this grammar does not, so a first word holding a `\` or a `&`
//! names no language (`None`) rather than a spelling nobody decoded.
//!
//! **The body**: [`strip_fence_indent`] removes up to the opening's indentation from a body line
//! (ex. 131–133). A tab that straddles that width is split the way §2.2 splits it: the columns it
//! still spans past the width become spaces, so the answer is owned exactly then.
//!
//! **A container** (a list item) is the caller's: it knows which container a line is in and how
//! many columns that container's content is indented by, and [`strip_container_indent`] subtracts
//! them **first** — the opening, the closing and the body are then read off what is left, as if
//! the container were not there (§5.2: "a list item … with the same content indented"). A line
//! indented by fewer columns than the container is not the container's (`None`), unless it is
//! blank. A tab in what is left's indentation is counted from its real column, not from the
//! remainder's start: where the container's column is not a tab stop it is spelled as the spaces
//! it spans, so the readers below see the indentation the line really has.
//!
//! **Not here, and the caller's:** which container a line belongs to (a list marker's width, a
//! `>`), whether a fence may interrupt the block above it (it may interrupt a paragraph, §4.5 ex.
//! 140), and that the end of the document or of the container ends an unclosed fence (ex. 126,
//! 127). This module carries no state from one line to the next.

use std::borrow::Cow;

/// §2.2: "tabs in lines are not expanded to spaces. However, in contexts where spaces help to
/// define block structure, tabs behave as if they were replaced by spaces with a tab stop of 4
/// characters."
pub const TAB_STOP: usize = 4;

/// §4.4/§4.5: an opening or closing fence "may be indented up to three spaces"; four is indented
/// code.
pub const MAX_FENCE_INDENT: usize = 3;

/// §4.5: "a sequence of at least three consecutive backtick characters (`` ` ``) or tildes (`~`)".
pub const MIN_FENCE_RUN: usize = 3;

/// Which of the two fence characters a fence is drawn with.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FenceMarker {
    Backtick,
    Tilde,
}

impl FenceMarker {
    #[must_use]
    pub fn character(self) -> char {
        match self {
            Self::Backtick => '`',
            Self::Tilde => '~',
        }
    }

    fn of(character: char) -> Option<Self> {
        match character {
            '`' => Some(Self::Backtick),
            '~' => Some(Self::Tilde),
            _ => None,
        }
    }
}

/// An open fence, as much of it as the lines below it are read against: its character, its run
/// length, and the columns its opening was indented by. Owned and `Copy`, because the terminal
/// carries it across lines long after the opening line's text is gone.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Fence {
    pub marker: FenceMarker,
    /// At least [`MIN_FENCE_RUN`].
    pub count: usize,
    /// `0..=MAX_FENCE_INDENT`.
    pub indent: usize,
}

/// An opening fence line: the fence it opens and its info string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FenceOpening<'a> {
    pub fence: Fence,
    /// Trimmed of spaces and tabs; empty when the opening has none.
    pub info: &'a str,
}

impl<'a> FenceOpening<'a> {
    /// The info string's first word, which names the block's language; `None` when there is none,
    /// or when the word holds a `\` or a `&` this grammar would have to decode (module doc).
    #[must_use]
    pub fn language(&self) -> Option<&'a str> {
        let word = self.info.split([' ', '\t']).next()?;
        (!word.is_empty() && !word.contains(['\\', '&'])).then_some(word)
    }
}

/// Four or more columns of leading indentation (§4.4), a tab counting to its stop.
#[must_use]
pub fn is_indented_code(line: &str) -> bool {
    let mut columns = 0usize;
    for character in line.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += TAB_STOP - columns % TAB_STOP,
            _ => break,
        }
        if columns > MAX_FENCE_INDENT {
            return true;
        }
    }
    false
}

/// The fence `line` opens, or `None` when it opens none (the module doc's table).
#[must_use]
pub fn fence_opening(line: &str) -> Option<FenceOpening<'_>> {
    if is_indented_code(line) {
        return None;
    }
    // Under four columns a tab cannot have stood in the indentation: one tab reaches the fourth
    // column on its own. So what is left of the indentation is spaces, and their count is its
    // width.
    let rest = line.trim_start_matches(' ');
    let indent = line.len() - rest.len();
    let marker = FenceMarker::of(rest.chars().next()?)?;
    let count = run_length(rest, marker);
    if count < MIN_FENCE_RUN {
        return None;
    }
    // The fence characters are ASCII, so the run is `count` bytes.
    let after = &rest[count..];
    if marker == FenceMarker::Backtick && after.contains('`') {
        return None;
    }
    Some(FenceOpening {
        fence: Fence {
            marker,
            count,
            indent,
        },
        info: after.trim_matches([' ', '\t']),
    })
}

/// Whether `line` closes `fence` (the module doc's table).
#[must_use]
pub fn fence_closes(line: &str, fence: &Fence) -> bool {
    if is_indented_code(line) {
        return false;
    }
    let rest = line.trim_start_matches(' ');
    let count = run_length(rest, fence.marker);
    count >= fence.count
        && rest[count..]
            .chars()
            .all(|character| matches!(character, ' ' | '\t'))
}

/// A body line of `fence` as the block holds it: up to the opening's indentation removed.
///
/// The answer (here and from [`strip_container_indent`]) is always a suffix of `line`, with
/// spaces in front of it only when the indentation it stands in held a tab that was split or
/// spelled out — which is what lets a caller map every byte of it back to the line it came from.
#[must_use]
pub fn strip_fence_indent<'a>(line: &'a str, fence: &Fence) -> Cow<'a, str> {
    strip_columns(line, fence.indent).0
}

/// `line` with a container's `columns` of content indentation removed, or `None` when the line is
/// indented by fewer and so is not that container's. A blank line is every container's, and is
/// blank in it.
///
/// What is left starts at column `columns` of the real line, and every reader here counts tab
/// stops from the start of the text it is handed. So a tab in what is left's indentation is
/// spelled as the spaces it spans **from its real column** whenever `columns` is not itself a
/// stop: `- a` over `  <TAB>```` is a fence indented two columns in the item (the tab runs from
/// column 2 to 4), not indented code. The spec keeps such a tab a tab; spelling it is what lets
/// the readers below count it where it stands.
#[must_use]
pub fn strip_container_indent(line: &str, columns: usize) -> Option<Cow<'_, str>> {
    let (rest, reached) = strip_columns(line, columns);
    (reached || rest.is_empty()).then(|| spell_leading_tabs_from(rest, columns))
}

/// `text`'s leading tabs as the spaces they span when `text` starts at column `start`; `text`
/// itself when `start` is a tab stop (the stops then fall where the readers count them) or its
/// indentation holds no tab.
fn spell_leading_tabs_from(text: Cow<'_, str>, start: usize) -> Cow<'_, str> {
    let indentation = text.len() - text.trim_start_matches([' ', '\t']).len();
    if start.is_multiple_of(TAB_STOP) || !text[..indentation].contains('\t') {
        return text;
    }
    let mut column = start;
    let mut spelled = String::with_capacity(text.len() + TAB_STOP);
    for character in text[..indentation].chars() {
        let stop = if character == '\t' {
            column + TAB_STOP - column % TAB_STOP
        } else {
            column + 1
        };
        spelled.extend(std::iter::repeat_n(' ', stop - column));
        column = stop;
    }
    spelled.push_str(&text[indentation..]);
    Cow::Owned(spelled)
}

fn run_length(text: &str, marker: FenceMarker) -> usize {
    let character = marker.character();
    text.chars().take_while(|&next| next == character).count()
}

/// Up to `columns` of leading indentation removed, and whether all of them were there. A tab
/// that crosses `columns` leaves the columns it spans beyond them as spaces (§2.2).
fn strip_columns(line: &str, columns: usize) -> (Cow<'_, str>, bool) {
    let mut column = 0usize;
    for (byte, character) in line.char_indices() {
        if column >= columns {
            return (Cow::Borrowed(&line[byte..]), true);
        }
        match character {
            ' ' => column += 1,
            '\t' => {
                let stop = column + TAB_STOP - column % TAB_STOP;
                if stop > columns {
                    let mut split = " ".repeat(stop - columns);
                    split.push_str(&line[byte + 1..]);
                    return (Cow::Owned(split), true);
                }
                column = stop;
            }
            _ => return (Cow::Borrowed(&line[byte..]), false),
        }
    }
    (Cow::Borrowed(""), column >= columns)
}

#[cfg(test)]
mod tests {
    //! One test per row of the module doc's table, each named for the CommonMark example it
    //! holds. MUTATION notes say which change to the grammar turns each red.

    use super::*;

    fn opening(line: &str) -> Option<(FenceMarker, usize, usize, &str)> {
        fence_opening(line).map(|opening| {
            (
                opening.fence.marker,
                opening.fence.count,
                opening.fence.indent,
                opening.info,
            )
        })
    }

    fn fence(line: &str) -> Fence {
        fence_opening(line).expect("an opening").fence
    }

    /// Ex. 119/120: both characters open a fence, and close their own kind.
    /// MUTATION: drop `'~'` from `FenceMarker::of` and the tilde row goes `None`.
    #[test]
    fn both_fence_characters_open_and_close_their_own_kind() {
        assert_eq!(opening("```"), Some((FenceMarker::Backtick, 3, 0, "")));
        assert_eq!(opening("~~~"), Some((FenceMarker::Tilde, 3, 0, "")));
        assert!(fence_closes("```", &fence("```")));
        assert!(fence_closes("~~~", &fence("~~~")));
    }

    /// Ex. 121: two backticks are not a fence.
    /// MUTATION: `MIN_FENCE_RUN = 2` and the pair opens one.
    #[test]
    fn fewer_than_three_is_not_a_fence() {
        assert_eq!(opening("``"), None);
        assert_eq!(opening("~~"), None);
        assert!(!fence_closes("``", &fence("```")));
    }

    /// Ex. 122/123: the other character never closes.
    /// MUTATION: compare runs of either character in `fence_closes` and ex. 122 closes on `~~~`.
    #[test]
    fn the_other_character_does_not_close() {
        assert!(!fence_closes("~~~", &fence("```")));
        assert!(!fence_closes("```", &fence("~~~")));
    }

    /// Ex. 124/125: a closer at least as long as the opener; a shorter one is body.
    /// MUTATION: `count == fence.count` and ex. 124's six-backtick closer stops closing;
    /// `count >= MIN_FENCE_RUN` and its three-backtick body line closes.
    #[test]
    fn a_closer_is_at_least_as_long_as_its_opener() {
        let four = fence("````");
        assert!(!fence_closes("```", &four));
        assert!(fence_closes("``````", &four));
        let tildes = fence("~~~~");
        assert!(!fence_closes("~~~", &tildes));
        assert!(fence_closes("~~~~", &tildes));
    }

    /// Ex. 131–133: the opener's indentation is its width, and the body loses up to that much.
    /// MUTATION: `strip_fence_indent` returning the line whole keeps ex. 133's three spaces.
    #[test]
    fn an_indented_opener_strips_its_width_off_the_body() {
        assert_eq!(opening(" ```"), Some((FenceMarker::Backtick, 3, 1, "")));
        assert_eq!(opening("  ```"), Some((FenceMarker::Backtick, 3, 2, "")));
        let three = fence("   ```");
        assert_eq!(three.indent, 3);
        assert_eq!(strip_fence_indent("   aaa", &three), "aaa");
        assert_eq!(strip_fence_indent("    aaa", &three), " aaa");
        assert_eq!(strip_fence_indent("  aaa", &three), "aaa");
        assert_eq!(strip_fence_indent("aaa", &three), "aaa");
        assert!(matches!(
            strip_fence_indent("   aaa", &three),
            Cow::Borrowed(_)
        ));
    }

    /// §2.2 under ex. 133: a tab straddling the opener's width leaves its far columns as spaces.
    /// MUTATION: strip the tab whole and the line comes back with no space in front of it.
    #[test]
    fn a_tab_straddling_the_width_is_split_into_spaces() {
        let three = fence("   ```");
        let split = strip_fence_indent("  \taaa", &three);
        assert_eq!(split, " aaa");
        assert!(matches!(split, Cow::Owned(_)));
        let two = fence("  ```");
        assert_eq!(strip_fence_indent("\taaa", &two), "  aaa");
        assert_eq!(strip_fence_indent("\t\taaa", &two), "  \taaa");
        let none = fence("```");
        assert_eq!(strip_fence_indent("\taaa", &none), "\taaa");
    }

    /// Ex. 134/137: four columns are indented code — no opener, and no closer either.
    /// MUTATION: `columns > 4` in `is_indented_code` and both lines read as fences.
    #[test]
    fn four_columns_are_indented_code_not_a_fence() {
        assert_eq!(opening("    ```"), None);
        assert_eq!(opening("\t```"), None);
        assert_eq!(opening(" \t```"), None);
        assert!(!fence_closes("    ```", &fence("```")));
        assert!(is_indented_code("    aaa"));
        assert!(!is_indented_code("   aaa"));
    }

    /// Ex. 135/136: a closer may be indented up to three spaces, whatever the opener's was.
    /// MUTATION: require the closer's indentation to equal the opener's and ex. 135 stays open.
    #[test]
    fn a_closer_may_be_indented_independently_of_its_opener() {
        assert!(fence_closes("  ```", &fence("```")));
        assert!(fence_closes("  ```", &fence("   ```")));
        assert!(fence_closes("   ```", &fence("```")));
    }

    /// Ex. 138/145/146: a backtick opener's text may hold no backtick; a tilde opener's may.
    /// MUTATION: drop the backtick check and ex. 145 opens a fence with info `aa ```.
    #[test]
    fn a_backtick_in_the_info_string_refuses_only_a_backtick_fence() {
        assert_eq!(opening("``` ```"), None);
        assert_eq!(opening("``` aa ```"), None);
        assert_eq!(
            opening("~~~ aa ``` ~~~"),
            Some((FenceMarker::Tilde, 3, 0, "aa ``` ~~~"))
        );
    }

    /// Ex. 139/147, and the spec's "only spaces or tabs": anything else after the run keeps the
    /// fence open — an info string, an internal space, an ideographic or a no-break space.
    /// MUTATION: `char::is_whitespace` in `fence_closes` and the U+3000 and U+00A0 lines close.
    #[test]
    fn a_closer_is_followed_only_by_spaces_and_tabs() {
        let backticks = fence("```");
        assert!(fence_closes("```  \t ", &backticks));
        assert!(!fence_closes("``` aaa", &backticks));
        assert!(!fence_closes("```\u{3000}", &backticks));
        assert!(!fence_closes("```\u{a0}", &backticks));
        assert!(!fence_closes("```\r", &backticks));
        assert!(!fence_closes("~~~ ~~", &fence("~~~~~~")));
    }

    /// Ex. 142–144/146: the info string is trimmed text; the language is its first word.
    /// MUTATION: `language` returning the whole info string gives ex. 143 `ruby startline=3 $%@#$`.
    #[test]
    fn the_language_is_the_info_strings_first_word() {
        let ruby = fence_opening("```ruby").expect("an opening");
        assert_eq!(ruby.language(), Some("ruby"));
        // Ex. 143's four spaces before the info string, built rather than spelled in a literal.
        let opener = format!("~~~~{}ruby startline=3 $%@#$", " ".repeat(4));
        let long = fence_opening(&opener).expect("an opening");
        assert_eq!(long.info, "ruby startline=3 $%@#$");
        assert_eq!(long.language(), Some("ruby"));
        let semicolon = fence_opening("````;").expect("an opening");
        assert_eq!(semicolon.info, ";");
        assert_eq!(semicolon.language(), Some(";"));
        assert_eq!(fence_opening("```").expect("an opening").language(), None);
        assert_eq!(
            fence_opening("~~~ aa ``` ~~~")
                .expect("an opening")
                .language(),
            Some("aa")
        );
        assert_eq!(
            fence_opening("```\trust\t").expect("an opening").info,
            "rust"
        );
    }

    /// The module doc's one refusal of a language: a word the spec would decode.
    /// MUTATION: drop the `\`/`&` check and `c\+\+` is named as it is spelled.
    #[test]
    fn a_language_holding_an_escape_or_an_entity_is_refused() {
        assert_eq!(
            fence_opening(r"```c\+\+").expect("an opening").language(),
            None
        );
        assert_eq!(
            fence_opening("```f&#35;").expect("an opening").language(),
            None
        );
    }

    /// Any other first character after the indentation is no fence.
    #[test]
    fn any_other_line_is_not_a_fence() {
        for line in ["", "   ", "aaa", "> ```", "- ```", "1. ```", "\u{3000}```"] {
            assert_eq!(opening(line), None, "{line:?}");
        }
    }

    /// §5.2 ex. 318/321/324: the container's columns come off first, then the fence is read.
    /// MUTATION: `strip_container_indent` returning a short line whole and ex. 318's `- c` stays
    /// inside the item.
    #[test]
    fn a_container_subtracts_its_columns_first() {
        assert_eq!(strip_container_indent("  ```", 2).as_deref(), Some("```"));
        assert_eq!(strip_container_indent("   ```", 3).as_deref(), Some("```"));
        assert_eq!(
            strip_container_indent("     ```", 2).as_deref(),
            Some("   ```")
        );
        assert_eq!(strip_container_indent("- c", 2), None);
        assert_eq!(strip_container_indent(" c", 2), None);
        assert_eq!(strip_container_indent("", 2).as_deref(), Some(""));
        assert_eq!(strip_container_indent(" ", 2).as_deref(), Some(""));
        assert_eq!(strip_container_indent("\tb", 2).as_deref(), Some("  b"));
        assert_eq!(strip_container_indent("abc", 0).as_deref(), Some("abc"));
    }

    /// §2.2 inside §5.2: a tab in an item's line is counted from its real column. `- a` over
    /// `  <TAB>```` is a fence indented two columns in the item (the tab runs from column 2 to
    /// 4), so it opens and it closes; counted from the remainder's start it would be indented code.
    /// MUTATION: return the remainder unspelled and the opener is `None` and the closer stays open.
    #[test]
    fn a_tab_in_an_items_line_is_counted_from_its_real_column() {
        let opener = strip_container_indent("  \t```", 2).expect("the item's line");
        assert_eq!(opener, "  ```");
        let open = fence_opening(&opener).expect("an opening");
        assert_eq!(open.fence.indent, 2);
        let body = strip_container_indent("  \tb", 2).expect("the item's line");
        assert_eq!(strip_fence_indent(&body, &open.fence), "b");
        let closer = strip_container_indent("  \t```", 2).expect("the item's line");
        assert!(fence_closes(&closer, &open.fence));
        // At a tab stop the remainder's own stops are the real ones, and it is lent as it is.
        assert!(matches!(
            strip_container_indent("    \t```", 4),
            Some(Cow::Borrowed("\t```"))
        ));
        // A space before the tab moves the stop it reaches: column 3 to 4 is one column.
        assert_eq!(
            strip_container_indent("   \t```", 2).as_deref(),
            Some("  ```")
        );
    }
}
