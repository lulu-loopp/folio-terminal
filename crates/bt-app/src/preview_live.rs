//! **The live preview's one rule: the block whose source range contains the
//! caret is drawn as source, and every other block is drawn rendered** (ticket
//! T4, research `docs/plans/markdown-edit/research-2026-09-10.md` §9.1;
//! `docs/DESIGN.md` §7.1.3q).
//!
//! Leaving the block renders it again. There is no third state and no block is
//! exempt — a table under the caret shows its pipes, a fence shows its markers
//! and keeps its highlighting, display mathematics shows its delimiters and its
//! picture stands down until the caret leaves. That is not a compromise: every
//! line of arithmetic [`crate::preview_edit`] owns — a column is x over the
//! monospace advance, wrapped rows, tab stops, the composition's caret box —
//! assumes a monospace grid, so turning the caret's block into a monospace body
//! is the one move that lets the editor this window already has be *reused*
//! rather than rewritten for proportional text.
//!
//! # What is drawn when the caret is in no block at all
//!
//! A block's range runs from the first byte of its first line to the end of its
//! last line, that line's own ending included (§7.1.3o), so the bytes *between*
//! two ranges — the blank line that ends a paragraph, the run of them somebody
//! left between two sections, the empty last line of a file that ends in a break
//! — belong to no block and never will: nothing is built out of them. A caret
//! standing there has no block to turn into source, and the rule as written
//! would leave it with nowhere to be.
//!
//! **So a gap is drawn as one empty source line, standing immediately under the
//! block in front of it** (at the top of the page when there is no block in
//! front of it), whatever number of blank lines the gap actually holds. Three
//! things decide it. The caret arrives in a gap by *leaving the block above it*
//! — Enter at the end of a paragraph, or Down out of its last row — so a caret
//! that appeared a collapsed margin away from the letters it was just beside
//! would read as a jump. Nothing below moves, because the empty line is drawn
//! into the margin the page already reserved between two blocks rather than
//! measured into the layout: a caret is not a block and must not push the
//! document around. And the several blank lines a gap may hold are one place as
//! far as this page is concerned — the file keeps every one of those bytes, and
//! the page is not a spelling of the file.
//!
//! Typing there re-parses, the bytes typed become a block of their own, and the
//! ordinary rule takes over on the very next parse: the caret is inside that new
//! block's range, so the new block is the source block. This module rules only
//! the *drawing*; where a keystroke goes and how the caret gets there is the
//! next ticket's (T5), and it builds on this.
//!
//! # Pure, in `preview_edit`'s style
//!
//! Every function here is a function of the ranges, the bytes and an offset —
//! no window, no pane, no geometry. A width is deliberately absent: which block
//! is the source block is a fact about the document and the caret, and it must
//! not be able to change because somebody dragged a window edge.

use std::ops::Range;

use crate::preview_edit;

/// **Where the caret is standing, in the document's own terms.**
///
/// One answer rather than two functions, because "which block is source" and
/// "which gap is the caret in" are one question asked once per rebuild: the two
/// are the two arms of a single walk, and a caller that asked them separately
/// could be told the caret is in block 4 *and* in the gap after block 7.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaretSeat {
    /// The caret is inside this block's source range. **This is the source
    /// block** — drawn as the file's own bytes, in the text face.
    Block(usize),
    /// The caret is in the tissue between two blocks, and no block is source.
    /// `after` is the block the gap's empty line stands under, or `None` when
    /// the caret is in front of the first block — which is also the answer for
    /// a document with no blocks at all.
    Gap { after: Option<usize> },
}

impl CaretSeat {
    /// The block drawn as source, when there is one.
    pub fn block(self) -> Option<usize> {
        match self {
            Self::Block(index) => Some(index),
            Self::Gap { .. } => None,
        }
    }
}

/// **Which block a caret stands in** — the one rule, as a function.
///
/// `ranges` is [`crate::preview::parse_markdown_ranged`]'s second half: one
/// byte range per block, in block order, ordered and never overlapping.
///
/// **A range is half-open, and that decides both boundaries.** A caret at a
/// range's `start` is on the block's first line and is in that block. A caret at
/// a range's `end` is one past the last byte the block was parsed from — which
/// is the first byte of whatever comes next — so it belongs to the block that
/// begins there when two blocks are adjacent, and to the gap when they are not.
/// A caret at the very end of a file that ends in a break is therefore in a gap
/// and not in the last block, which is right: that is the empty line the editor's
/// own line model insists a body ending in a break has ([`preview_edit::line_starts`]),
/// and a caret may stand on it.
pub fn caret_seat(ranges: &[Range<usize>], caret: usize) -> CaretSeat {
    // The ranges are ordered and non-overlapping, so the first one that has not
    // already ended is the only one that can hold this byte. `partition_point`
    // rather than a scan because a megabyte of markdown is thousands of blocks
    // and this is asked once per rebuild, on the path a keystroke waits behind.
    let next = ranges.partition_point(|range| range.end <= caret);
    match ranges.get(next) {
        Some(range) if range.start <= caret => CaretSeat::Block(next),
        _ => CaretSeat::Gap {
            after: next.checked_sub(1),
        },
    }
}

/// **The bytes one block is drawn from**, its own trailing line ending off.
///
/// The ending is dropped and nothing else is: a block's range covers it
/// (§7.1.3o) but no caret can stand past it — an offset at `range.end` is in the
/// next block or in the gap by [`caret_seat`]'s own rule — so a source block that
/// drew it would draw an empty line nothing can ever reach. Both bytes of a CRLF
/// go together, for [`preview_edit::normalize`]'s reason: `\r\n` is one line
/// ending and a caret inside it is a caret in the middle of a character.
///
/// Everything else survives untouched, trailing whitespace and blank lines
/// inside a fence included, because this is the file and not a rendering of it.
pub fn block_source<'a>(content: &'a str, range: &Range<usize>) -> &'a str {
    let start = range.start.min(content.len());
    let end = range.end.min(content.len()).max(start);
    let text = &content[start..end];
    match text.strip_suffix('\n') {
        Some(head) => head.strip_suffix('\r').unwrap_or(head),
        None => text,
    }
}

/// **Where a byte of the file sits inside its block's source**: the line of the
/// block it is on, and how far into that line it is in drawn columns.
///
/// The line is the block's own — zero is the block's first line, whatever line
/// of the file that is — and the column is the coordinate the whole source face
/// is measured in ([`preview_edit::column_of`]): tabs are the stops they draw
/// as, and a wide character counts the two cells it occupies.
///
/// `None` when the offset is not in that block, which is the same answer
/// [`caret_seat`] gives and is a caller asking about the wrong block rather than
/// a failure.
///
/// **CRLF is honoured twice over.** The offset is put somewhere a caret may
/// actually stand first ([`preview_edit::normalize`], which refuses to sit
/// between the two bytes of a break), and the line's text ends before its `\r`
/// ([`preview_edit::line_bounds`]), so the last column of a line on this
/// platform is the same column it would be on any other.
pub fn place_in_block(
    content: &str,
    ranges: &[Range<usize>],
    block: usize,
    offset: usize,
) -> Option<(usize, usize)> {
    let range = ranges.get(block)?;
    let offset = preview_edit::normalize(content, offset);
    if offset < range.start || offset >= range.end {
        return None;
    }
    let text = block_source(content, range);
    let local = offset - range.start;
    // An offset inside the block's own trailing break — there is exactly one
    // byte of it a normalised caret can be at, the `\n` of a bare LF — is the
    // end of the block's last line, which is where a caret at the end of a
    // paragraph stands.
    let local = local.min(text.len());
    let starts = preview_edit::line_starts(text);
    let line = preview_edit::line_index(&starts, local);
    let (start, _) = preview_edit::line_bounds(text, &starts, line);
    let column =
        preview_edit::column_of(preview_edit::line_text(text, &starts, line), local - start);
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks at the ranges a document with a blank line between every pair
    /// would have.
    fn ranges(spans: &[(usize, usize)]) -> Vec<Range<usize>> {
        spans.iter().map(|(from, to)| *from..*to).collect()
    }

    /// **The one rule, on every boundary it has** (research §9.1).
    ///
    /// A block's range is half-open and covers its last line's ending, so the
    /// three interesting offsets are its first byte, its last byte and the byte
    /// one past it — and the third one belongs to the *next* thing, whether that
    /// is a block or a gap.
    ///
    /// MUTATION: use `range.end < caret` in `caret_seat` and a caret at the end
    /// of a paragraph claims the paragraph *and* the blank line after it, so
    /// pressing Enter twice would leave the old block drawn as source with the
    /// caret nowhere in it.
    #[test]
    fn the_block_that_owns_a_caret_is_the_one_whose_range_holds_it() {
        // "one\n\ntwo\n" — two paragraphs with a blank line between them.
        let spans = ranges(&[(0, 4), (5, 9)]);
        assert_eq!(caret_seat(&spans, 0), CaretSeat::Block(0), "at the start");
        assert_eq!(caret_seat(&spans, 2), CaretSeat::Block(0), "inside");
        assert_eq!(
            caret_seat(&spans, 3),
            CaretSeat::Block(0),
            "the last byte of its own text is still its own",
        );
        assert_eq!(
            caret_seat(&spans, 4),
            CaretSeat::Gap { after: Some(0) },
            "one past the range is the blank line, and the blank line is nobody's",
        );
        assert_eq!(caret_seat(&spans, 5), CaretSeat::Block(1), "the next block");
        assert_eq!(
            caret_seat(&spans, 9),
            CaretSeat::Gap { after: Some(1) },
            "and the empty line a file ending in a break has is a gap, which is \
             exactly where a caret at the end of a document stands",
        );
    }

    /// **Two blocks with no blank line between them share a byte, and the byte
    /// belongs to the one that begins there.**
    ///
    /// A heading and the paragraph under it are written this way in every file
    /// this window was built to read, so this is the common case and not the
    /// corner.
    #[test]
    fn adjacent_blocks_hand_the_caret_straight_over() {
        // "# head\ntext\n"
        let spans = ranges(&[(0, 7), (7, 12)]);
        assert_eq!(caret_seat(&spans, 6), CaretSeat::Block(0));
        assert_eq!(
            caret_seat(&spans, 7),
            CaretSeat::Block(1),
            "no gap to fall into: the next block starts on this very byte",
        );
        assert_eq!(
            caret_seat(&[], 0),
            CaretSeat::Gap { after: None },
            "an empty document is one gap, and the caret is in front of nothing",
        );
        assert_eq!(
            caret_seat(&spans, 0),
            CaretSeat::Block(0),
            "and a caret in front of the first block of a document that has one \
             is in that block, because the block starts at the first byte",
        );
    }

    /// **A caret before the first block stands in a gap under nothing.**
    ///
    /// Front matter this parser does not model, or a file that opens on blank
    /// lines: the bytes are there, no block was made of them, and the empty line
    /// is drawn at the top of the page.
    #[test]
    fn a_caret_in_front_of_every_block_is_a_gap_under_nothing() {
        let spans = ranges(&[(4, 8)]);
        assert_eq!(caret_seat(&spans, 0), CaretSeat::Gap { after: None });
        assert_eq!(caret_seat(&spans, 3), CaretSeat::Gap { after: None });
        assert_eq!(caret_seat(&spans, 4), CaretSeat::Block(0));
    }

    /// **Row and column inside a block, on both line endings** (§7.1.3o's own
    /// warning: `str::lines` strips the `\r` as well, so a walk that added line
    /// lengths up would drift by a byte a line).
    ///
    /// MUTATION: build the line model over `content` instead of over the block's
    /// own bytes and every row is the file's line number, so the second block of
    /// a document draws its caret off the bottom of itself.
    #[test]
    fn a_byte_of_a_block_answers_with_its_row_and_column() {
        let content = "# head\r\n\r\none\r\ntwo\r\n";
        let spans = ranges(&[(0, 8), (10, 20)]);
        assert_eq!(
            place_in_block(content, &spans, 0, 2),
            Some((0, 2)),
            "two columns into the heading's own line",
        );
        assert_eq!(
            place_in_block(content, &spans, 0, 6),
            Some((0, 6)),
            "the end of a CRLF line is the column before the carriage return",
        );
        assert_eq!(
            place_in_block(content, &spans, 1, 15),
            Some((1, 0)),
            "the second line of the second block, at its start",
        );
        assert_eq!(
            place_in_block(content, &spans, 1, 18),
            Some((1, 3)),
            "and at its end",
        );
        assert_eq!(
            place_in_block(content, &spans, 0, 12),
            None,
            "a byte of another block is not this block's to place",
        );
        assert_eq!(place_in_block(content, &spans, 9, 2), None, "no such block");
    }

    /// **A caret between the two bytes of a CRLF is not a place**, so it is
    /// pulled back to the one before it — the same rule the caret itself is held
    /// to everywhere else on this surface.
    #[test]
    fn a_caret_inside_a_break_is_pulled_off_it() {
        let content = "one\r\ntwo\r\n";
        let spans = ranges(&[(0, 10)]);
        assert_eq!(
            place_in_block(content, &spans, 0, 4),
            Some((0, 3)),
            "between the carriage return and the newline is the end of line one",
        );
    }

    /// **A block's own bytes stop at its text**, the ending it was given off.
    ///
    /// MUTATION: return `&content[range]` whole and every source block draws a
    /// phantom empty line under itself that no caret can ever reach, which is a
    /// block one row taller than the file says it is.
    #[test]
    fn a_blocks_source_drops_its_own_ending_and_nothing_else() {
        let lf = "para\n";
        assert_eq!(block_source(lf, &(0..5)), "para");
        let crlf = "para\r\n";
        assert_eq!(block_source(crlf, &(0..6)), "para");
        let bare = "para";
        assert_eq!(
            block_source(bare, &(0..4)),
            "para",
            "a last line with no break"
        );
        let fence = "```\n\na\n```\n";
        assert_eq!(
            block_source(fence, &(0..11)),
            "```\n\na\n```",
            "a blank line inside a fence is the file's and stays",
        );
        let trailing = "para  \n";
        assert_eq!(
            block_source(trailing, &(0..7)),
            "para  ",
            "trailing whitespace is the file's too",
        );
    }
}
