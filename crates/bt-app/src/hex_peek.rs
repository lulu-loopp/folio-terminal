//! **The colour under the pointer** — a swatch beside any `#rrggbb` a text
//! preview is showing (§7.1.6c-4c).
//!
//! # Why a terminal has this at all
//!
//! Because of what the previous slice made a JSON file mean. §7.1.6c-4a made a
//! colour scheme a file in Windows Terminal's format, and §7.1.6c-4c hands the
//! user a copy of the one they are wearing, opened in this window's own editor.
//! That editor is a general text editor and nothing in it knows about schemes —
//! and it does not need to, because the two halves of "a scheme editor" are
//! already present once this module exists: **you can see the colour under the
//! pointer, and the window itself is the preview after you save.**
//!
//! So this is not a feature for schemes. It is a feature of text previews that
//! happens to make a scheme file editable, which is why it is armed for every
//! text document rather than for one folder's `*.json`.
//!
//! # What counts as a colour
//!
//! The three spellings CSS uses — `#rgb`, `#rrggbb`, `#rrggbbaa` — and no
//! others. Two things are deliberately refused:
//!
//! * **Runs of the wrong length.** `#12` and `#abcd` are not short colours to be
//!   guessed at; a card offering a swatch for the first three digits of a
//!   four-digit run would be showing a colour that is not in the file.
//! * **A run glued to a word.** `#abcg` is an identifier that begins with three
//!   hex digits. The rule is that the whole run between the `#` and the next
//!   non-hex byte has to be one of the three lengths, and that what follows it
//!   is not a word byte.
//!
//! This is a *lexical* judgement and not a semantic one: an eight-hex commit
//! prefix written `#deadbeef` gets a swatch. That is the honest answer — nothing
//! on this glass knows what the file means, and the alternative is a table of
//! exceptions per file type that would still be wrong on the next one.

use std::ops::Range;

/// A colour spelled somewhere in a line of text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HexToken {
    /// Byte range of the token *including* its `#`, within the line it was found
    /// in — what the card is drawn beside and what its text reads.
    pub range: Range<usize>,
    /// Straight (non-premultiplied) RGBA. A spelling with no alpha is opaque.
    pub rgba: [u8; 4],
}

impl HexToken {
    /// The token as it was written, out of the line it was found in.
    #[must_use]
    pub fn text<'a>(&self, line: &'a str) -> &'a str {
        &line[self.range.clone()]
    }
}

/// The colour token containing `byte`, if the pointer is inside one.
///
/// `byte` is a byte offset into `line`; an offset landing anywhere in the token,
/// `#` included, answers with it.
#[must_use]
pub fn hex_token_at(line: &str, byte: usize) -> Option<HexToken> {
    hex_tokens(line).find(|token| token.range.contains(&byte))
}

/// Every colour token in one line, left to right.
pub fn hex_tokens(line: &str) -> impl Iterator<Item = HexToken> + '_ {
    let bytes = line.as_bytes();
    line.match_indices('#')
        .filter_map(move |(start, _)| token_at_hash(bytes, start))
}

/// The token that starts at the `#` at `start`, if that `#` starts one.
fn token_at_hash(bytes: &[u8], start: usize) -> Option<HexToken> {
    if start > 0 && is_word_byte(bytes[start - 1]) {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() && hex_nibble(bytes[end]).is_some() {
        end += 1;
    }
    if bytes.get(end).is_some_and(|byte| is_word_byte(*byte)) {
        return None;
    }
    let rgba = channels(&bytes[start + 1..end])?;
    Some(HexToken {
        range: start..end,
        rgba,
    })
}

/// The four channels a run of 3, 6 or 8 hex digits spells, and nothing for any
/// other length.
///
/// The short form doubles each nibble rather than shifting it left, which is
/// CSS's rule and the only one mapping `f` to `255` — `f0` would make `#fff` a
/// shade darker than white, which is the one value everybody would notice.
fn channels(digits: &[u8]) -> Option<[u8; 4]> {
    let mut rgba = [0xffu8; 4];
    match digits.len() {
        3 => {
            for (channel, digit) in rgba.iter_mut().zip(digits) {
                *channel = hex_nibble(*digit)? * 17;
            }
        }
        6 | 8 => {
            for (channel, pair) in rgba.iter_mut().zip(digits.chunks_exact(2)) {
                *channel = hex_nibble(pair[0])? * 16 + hex_nibble(pair[1])?;
            }
        }
        _ => return None,
    }
    Some(rgba)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// What a colour token may not be glued to on either side.
///
/// ASCII only, and `_` counts. A `#` after a Han character or an em dash starts
/// a token, because in prose those are punctuation; a `#` after a letter or a
/// digit is somebody's identifier.
const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'#'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(line: &str) -> Option<HexToken> {
        let found: Vec<HexToken> = hex_tokens(line).collect();
        assert!(found.len() <= 1, "{line} produced {found:?}");
        found.into_iter().next()
    }

    /// PIN — the three spellings, and what each one means.
    #[test]
    fn the_three_css_spellings_are_read_and_the_short_one_doubles_its_nibbles() {
        assert_eq!(only("#abc").unwrap().rgba, [0xaa, 0xbb, 0xcc, 0xff]);
        assert_eq!(only("#fff").unwrap().rgba, [0xff, 0xff, 0xff, 0xff]);
        assert_eq!(only("#aabbcc").unwrap().rgba, [0xaa, 0xbb, 0xcc, 0xff]);
        assert_eq!(only("#aabbccdd").unwrap().rgba, [0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(only("#AABBCC").unwrap().rgba, [0xaa, 0xbb, 0xcc, 0xff]);
    }

    /// PIN — a run of the wrong length is not a colour to be guessed at.
    #[test]
    fn a_run_that_is_not_three_six_or_eight_digits_long_is_not_a_colour() {
        for line in [
            "#12",
            "#1",
            "#",
            "#abcd",
            "#abcde",
            "#abcdefa",
            "#abcdefabc",
        ] {
            assert_eq!(only(line), None, "{line} must not be a colour");
        }
    }

    /// PIN — non-hex digits are not colours, and the refusal is not a partial
    /// read of the digits that were hex.
    #[test]
    fn a_run_that_is_not_hex_is_not_a_colour() {
        for line in ["#ggg", "#zzzzzz", "#abg", "#12345g"] {
            assert_eq!(only(line), None, "{line} must not be a colour");
        }
    }

    /// PIN — a colour glued to a word on either side belongs to the word.
    #[test]
    fn a_token_inside_a_longer_word_is_not_a_colour() {
        for line in [
            "#abcg", "#abcdefg", "id#abc", "x#aabbcc", "9#abc", "_#abc", "##abc",
        ] {
            assert_eq!(only(line), None, "{line} must not be a colour");
        }
    }

    /// The shapes a colour actually turns up in — a scheme file's quoted value,
    /// a CSS declaration, prose, and prose in the other language.
    #[test]
    fn a_colour_is_found_in_the_places_it_is_written() {
        let json = r##"  "background": "#1b1b1b","##;
        let token = only(json).unwrap();
        assert_eq!(token.text(json), "#1b1b1b");
        assert_eq!(token.rgba, [0x1b, 0x1b, 0x1b, 0xff]);

        let css = "  color: #fff;";
        assert_eq!(only(css).unwrap().text(css), "#fff");

        let prose = "the accent is #7a99ff, which is a blue";
        assert_eq!(only(prose).unwrap().text(prose), "#7a99ff");

        let han = "背景色#7a99ff。";
        assert_eq!(only(han).unwrap().text(han), "#7a99ff");
    }

    /// Several colours on one line are several tokens, in reading order.
    #[test]
    fn a_line_may_hold_more_than_one_colour() {
        let line = "linear-gradient(#fff, #1b1b1b 40%, #7a99ff88)";
        let found: Vec<&str> = hex_tokens(line).map(|token| token.text(line)).collect();
        assert_eq!(found, ["#fff", "#1b1b1b", "#7a99ff88"]);
    }

    /// The pointer answers with the token it is standing in, `#` and last digit
    /// included, and with nothing on either side of it.
    #[test]
    fn the_pointer_answers_with_the_token_it_is_standing_in() {
        let line = r##"  "accent": "#7a99ff","##;
        let start = line.find('#').unwrap();
        let end = start + "#7a99ff".len();
        assert_eq!(hex_token_at(line, start).unwrap().text(line), "#7a99ff");
        assert_eq!(hex_token_at(line, end - 1).unwrap().text(line), "#7a99ff");
        assert_eq!(hex_token_at(line, start - 1), None);
        assert_eq!(hex_token_at(line, end), None);
    }

    /// A line with nothing in it, and one with no colour, answer with nothing
    /// rather than with a panic.
    #[test]
    fn a_line_with_no_colour_in_it_answers_with_nothing() {
        assert_eq!(hex_token_at("", 0), None);
        assert_eq!(hex_token_at("fn main() {}", 3), None);
        assert_eq!(hex_tokens("# heading").count(), 0);
    }
}
