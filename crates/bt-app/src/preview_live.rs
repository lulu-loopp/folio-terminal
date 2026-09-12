//! **The live preview's one rule: the block whose source range contains the
//! caret is drawn as source, and every other block is drawn rendered** (ticket
//! T4, research `docs/plans/markdown-edit/research-2026-09-10.md` §9.1;
//! `docs/DESIGN.md` §7.1.3q).
//!
//! Leaving the block renders it again. There is no third state and no block is
//! exempt — a table under the caret shows its pipes, a fence shows its markers
//! and keeps its highlighting, display mathematics shows its delimiters and its
//! picture stands down until the caret leaves.
//!
//! **What face it is shown in is the block's kind's** (owner's ruling
//! 2026-09-11, §7.1.3w). A heading, a paragraph, a list and a quote keep the
//! body face they were read in and show their marks in it — the `#`, the `**`,
//! the `- `, the `> ` — because a paragraph you click into must not change
//! typeface. A fence, a table and a display formula turn monospace, because
//! their *alignment* is their content. The two faces are one slot and one rule;
//! what differs is the arithmetic that puts a caret on the glass, which is
//! [`BlockRows`] over a monospace grid and [`ProseRows`] over the shaper's own
//! seams.
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
//! no window and no pane. A width is deliberately absent: which block is the
//! caret's block is a fact about the document and the caret, and it must not be
//! able to change because somebody dragged a window edge.
//!
//! [`ProseRows`] is the one value in here that carries pixels, and it carries
//! them as *data*: it is what the shaper answered about the rows it drew, and
//! every rule read off it — where the caret is struck, which row a press is on,
//! what a selection bands — is a function of that value and is held to in a test
//! with no window and no GPU in the room.

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
///
/// **A file that does *not* end in a break is the exception, and it is not one**
/// (user report, 2026-09-11; audit A6). There is no empty line after `abc`: the
/// last line of that file is `abc` itself, and the end of it is where `End`, a
/// press past the last letter and every keystroke typed at the end of the
/// document put the caret. The block's range ends there too — it has no ending
/// to cover — so the rule above would read that one position as a gap and draw
/// the caret on a line that does not exist, under a paragraph that never became
/// source. So **the end of an unterminated last block belongs to that block**.
/// It is exactly one position, told apart from the genuine trailing blank line
/// by the file's own last byte, which is the only thing that distinguishes them.
pub fn caret_seat(content: &str, ranges: &[Range<usize>], caret: usize) -> CaretSeat {
    // The ranges are ordered and non-overlapping, so the first one that has not
    // already ended is the only one that can hold this byte. `partition_point`
    // rather than a scan because a megabyte of markdown is thousands of blocks
    // and this is asked once per rebuild, on the path a keystroke waits behind.
    let next = ranges.partition_point(|range| range.end <= caret);
    match ranges.get(next) {
        Some(range) if range.start <= caret => CaretSeat::Block(next),
        _ => match next.checked_sub(1) {
            Some(last)
                if last + 1 == ranges.len()
                    && ranges[last].end == caret
                    && caret == content.len()
                    && !content.ends_with('\n') =>
            {
                CaretSeat::Block(last)
            }
            after => CaretSeat::Gap { after },
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
    let text = block_source(content, range);
    // **The end of the block's last line is the block's**, which is the same
    // rule said in bytes rather than in ranges: a block that ends in a break has
    // that break past this bound (and `range.end` past *that*), while the last
    // block of a file with no trailing break ends exactly here — the position
    // `End` puts the caret at, which used to be refused as though it were the
    // next block's ([`caret_seat`], audit A6).
    let last = range.start + text.len();
    if offset < range.start || offset > last {
        return None;
    }
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

/// **The source block as something a caret can be walked through** (T5,
/// §7.1.3t): its bytes, where they begin in the file, and how they fold into the
/// width they are drawn in.
///
/// Borrowed rather than owned because both the callers hold all three already —
/// the painter built the fold to draw the rows and the hit test has to read it
/// back — and a fourth derivation of the same fold is a fourth chance to
/// disagree about which row a line is on.
#[derive(Clone, Copy, Debug)]
pub struct BlockRows<'a> {
    /// The block's own bytes, its trailing line ending off ([`block_source`]).
    pub text: &'a str,
    /// The file offset those bytes begin at — the block's `range.start`.
    pub start: usize,
    /// How they fold, at the width the block is drawn in.
    pub wrap: &'a preview_edit::WrapLayout,
}

impl BlockRows<'_> {
    /// Whether a file offset stands inside these bytes.
    ///
    /// Inclusive at both ends: `start` is the block's first byte and
    /// `start + text.len()` is the end of its last line, which is where a caret
    /// at the end of a paragraph stands. One past *that* is the block's own line
    /// ending, and [`caret_seat`] has already given it to whatever comes next.
    #[must_use]
    pub fn holds(&self, offset: usize) -> bool {
        (self.start..=self.start + self.text.len()).contains(&offset)
    }

    /// **Where a file offset is drawn**: the row of this block, and how far into
    /// that row it stands.
    ///
    /// The column is measured *inside the row* rather than inside the line,
    /// which is what [`step_by_row`] carries as the desired column — a walk down
    /// a folded paragraph that kept the line's column would leap to the far end
    /// of the second row.
    #[must_use]
    pub fn row_of(&self, offset: usize) -> Option<(usize, usize)> {
        if !self.holds(offset) {
            return None;
        }
        let local = preview_edit::normalize(self.text, offset - self.start);
        let starts = preview_edit::line_starts(self.text);
        let line = preview_edit::line_index(&starts, local);
        let (from, _) = preview_edit::line_bounds(self.text, &starts, line);
        let column = preview_edit::column_of(
            preview_edit::line_text(self.text, &starts, line),
            local.saturating_sub(from),
        );
        let (row, row_start) = self.wrap.row_of(line, column);
        Some((row, column.saturating_sub(row_start)))
    }

    /// **The file byte a drawn row and a column inside it name** — [`Self::row_of`]
    /// read backwards, and the whole of a click landing in the source block.
    ///
    /// A column past the end of the row lands at the end of the row's own text,
    /// which is what makes clicking in the space after a short line put the
    /// caret at the end of that line; a row past the end of the block is the end
    /// of the block, which is where a click below its last line lands.
    #[must_use]
    pub fn offset_at(&self, row: usize, column: usize) -> usize {
        let Some((line, from, to)) = self.wrap.row_span(row) else {
            return self.start + self.text.len();
        };
        let starts = preview_edit::line_starts(self.text);
        let text = preview_edit::line_text(self.text, &starts, line);
        // `to` runs one column past the last row of a line — the cell the break
        // is drawn in — and a caret may not stand past the end of a line.
        let column =
            (from + column.min(to.saturating_sub(from))).min(preview_edit::line_columns(text));
        let (line_start, _) = preview_edit::line_bounds(self.text, &starts, line);
        self.start + line_start + preview_edit::byte_at_column(text, column)
    }

    /// **The file byte a press inside a drawn row names** — [`Self::offset_at`]
    /// asked in the coordinate a pointer arrives in, and the same clamps.
    ///
    /// Cells rather than a cell: a press lands somewhere *inside* a character,
    /// and which side of a two-cell ideograph it belongs to is a question a
    /// whole-cell column has already thrown the answer to away. See
    /// [`preview_edit::byte_at_x`].
    #[must_use]
    pub fn offset_at_x(&self, row: usize, columns: f32) -> usize {
        let Some((line, from, to)) = self.wrap.row_span(row) else {
            return self.start + self.text.len();
        };
        let starts = preview_edit::line_starts(self.text);
        let text = preview_edit::line_text(self.text, &starts, line);
        // The row's own span, exactly as [`Self::offset_at`] cuts it: a press
        // past the end of a folded row is the end of that row and not a reach
        // into the row under it.
        #[allow(clippy::cast_precision_loss)]
        let columns = (from as f32 + columns.max(0.0)).min(to as f32);
        let (line_start, _) = preview_edit::line_bounds(self.text, &starts, line);
        self.start + line_start + preview_edit::byte_at_x(text, columns)
    }
}

/// **One place a caret may stand on a drawn row of prose**: the byte of the
/// *file* it is in front of, and the x it is struck at (§7.1.3w).
///
/// [`bt_render::PreviewTextSeam`] with the paragraph's own offsets turned into
/// the file's, which is the one thing the window adds to the shaper's answer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProseSeam {
    pub offset: usize,
    pub x: f32,
}

/// One drawn row of the caret's prose block.
#[derive(Clone, Debug, PartialEq)]
pub struct ProseRow {
    pub top: f32,
    pub height: f32,
    /// Left to right. Never empty — an empty line carries the one seam it has.
    pub seams: Vec<ProseSeam>,
}

impl ProseRow {
    /// The first file byte this row draws.
    fn start(&self) -> usize {
        self.seams.first().map_or(0, |seam| seam.offset)
    }

    /// One past the last file byte this row draws — the row's own end, where
    /// the caret at the end of a row stands.
    fn end(&self) -> usize {
        self.seams.last().map_or(0, |seam| seam.offset)
    }

    /// Where a byte of this row is struck.
    ///
    /// The seam that names it, or — for a byte inside a cluster the shaper drew
    /// as one thing, which is where a caret may not stand and an unnormalised
    /// offset may still ask about — the nearest seam in front of it.
    fn x_at(&self, offset: usize) -> f32 {
        // The greatest offset at or before it, and not the last one in the
        // row's own order: glyphs arrive in *visual* order, so a row that
        // changes direction inside itself is a row whose seams do not ascend.
        self.seams
            .iter()
            .filter(|seam| seam.offset <= offset)
            .max_by_key(|seam| seam.offset)
            .or_else(|| self.seams.first())
            .map_or(0.0, |seam| seam.x)
    }
}

/// **The caret's prose block as the shaper that drew it laid it out** — every
/// row of it, and every seam in every row (§7.1.3w).
///
/// The counterpart of [`BlockRows`] for the face that has no columns, and the
/// one geometry five passes read: the caret is struck at a seam, the IME's
/// candidate box hangs from the same seam, a selection band runs from seam to
/// seam, a press is the nearest seam to the pointer, and Up and Down step these
/// rows. §7.1.3u was paid for twice by two derivations of one caret's x; this is
/// the answer to it on a proportional face, where `column × advance` is not an
/// answer at all.
///
/// **Pure data, and deliberately.** It arrives from the shaper and is then a
/// value like any other, so every rule above is a function of it and can be
/// held to in a test with no window and no GPU in the room.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProseRows {
    /// Which block of the document these rows draw.
    pub index: usize,
    /// Top to bottom, in the order they are drawn.
    pub rows: Vec<ProseRow>,
}

impl ProseRows {
    /// How many rows the block is drawn as.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// **The row a file byte is drawn on, and where along it** — the whole of
    /// "where is the caret".
    ///
    /// **A byte that ends one row and begins the next belongs to the first**
    /// (audit A5, fixed here on this face) — the affinity a model with no
    /// affinity bit has to pick, and the two candidates are not equal.
    ///
    /// One offset, two places on the glass: the end of the row it finishes and
    /// the start of the row it begins. Giving it to the *second* is what made
    /// Down land at the far left of the row under the one the reader was aiming
    /// at, and then Up from there answer with the very same byte — a caret that
    /// sticks at every soft-wrap seam. Giving it to the first costs one thing
    /// and it is smaller: a press on the extreme left edge of a wrapped
    /// continuation row draws the caret at the end of the row above, which is
    /// the same byte of the file and the same place in the sentence.
    #[must_use]
    pub fn row_of(&self, offset: usize) -> Option<(usize, f32)> {
        let index = self
            .rows
            .iter()
            .position(|row| row.start() <= offset && offset <= row.end())?;
        Some((index, self.rows[index].x_at(offset)))
    }

    /// The rectangle the caret is struck in — a hairline at the seam, as tall as
    /// the row's own line box.
    #[must_use]
    pub fn caret(&self, offset: usize) -> Option<[f32; 4]> {
        let (index, x) = self.row_of(offset)?;
        let row = &self.rows[index];
        Some([x, row.top, x, row.top + row.height])
    }

    /// The row a y is level with, clamped: above the block is its first row and
    /// below it its last, because a press has already been judged to belong to
    /// this block by the time it arrives.
    #[must_use]
    pub fn row_at_y(&self, y: f32) -> usize {
        let last = self.rows.len().saturating_sub(1);
        self.rows
            .iter()
            .position(|row| y < row.top + row.height)
            .unwrap_or(last)
    }

    /// **The file byte a point on a row names** — [`Self::row_of`] backwards,
    /// and the whole of a press landing in the prose block.
    ///
    /// The *nearest seam* and not the letter the pointer is inside, which is
    /// this window's rule on every face it has (§7.1.3q): a click on the right
    /// half of a character puts the caret after it.
    #[must_use]
    pub fn offset_at(&self, row: usize, x: f32) -> usize {
        let Some(row) = self.rows.get(row) else {
            return self.rows.last().map_or(0, ProseRow::end);
        };
        row.seams
            .iter()
            .min_by(|one, two| {
                (one.x - x)
                    .abs()
                    .partial_cmp(&(two.x - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map_or_else(|| row.end(), |seam| seam.offset)
    }

    /// The byte a press at a point in the block names.
    #[must_use]
    pub fn press(&self, x: f32, y: f32) -> usize {
        self.offset_at(self.row_at_y(y), x)
    }

    /// **The bands a range of the file draws over this block** — one per row it
    /// touches, from seam to seam, because a row is what the reader sees and a
    /// band per *line* would run off the right edge of a folded one.
    #[must_use]
    pub fn bands(&self, range: &Range<usize>) -> Vec<[f32; 4]> {
        let mut bands = Vec::new();
        for row in &self.rows {
            let from = range.start.max(row.start());
            let to = range.end.min(row.end());
            if from >= to {
                continue;
            }
            let (one, two) = (row.x_at(from), row.x_at(to));
            bands.push([one.min(two), row.top, one.max(two), row.top + row.height]);
        }
        bands
    }
}

/// **The rows of the caret's block, in whichever of the two faces its kind
/// wears** (§7.1.3w).
///
/// One parameter rather than two, for [`CaretSeat`]'s reason: a block is drawn
/// in one face or the other and never both, and a stepper handed two options
/// could be told it is in neither and asked to walk one anyway.
#[derive(Clone, Copy, Debug)]
pub enum CaretRows<'a> {
    /// A fence, a table, a display formula: the monospace grid's folded rows.
    Mono(BlockRows<'a>),
    /// A heading, a paragraph, a list, a quote: the shaper's own rows.
    Prose(&'a ProseRows),
}

/// **One row up or down on a live-preview page**, wherever the caret happens to
/// be standing (T5 ②, §7.1.3t).
///
/// `true` when the motion was a vertical one and the caret has been moved;
/// `false` for every other motion, which the file's own line model answers
/// unchanged ([`preview_edit::move_caret`]).
///
/// **Two coordinate systems, and the seam between them is the whole function.**
/// Inside the source block a row is a *folded* row of that block, because that
/// is what the reader can see: a paragraph written as one long line is drawn as
/// four, and Down that jumped the whole paragraph would skip three of them.
/// Step off the block's top or bottom row and there is no fold to walk any more
/// — the neighbour is drawn as prose and has no rows of its own until the caret
/// arrives in it and it becomes the source block — so the step becomes a step of
/// the *file's* lines, which lands in the block above, the block below, or the
/// blank line of a gap between them, and the next parse makes whichever it is
/// the source block. That is §7.1.3q's own account of Arrow-Up out of a block,
/// and it is why nothing here needs a "leaving a block" event.
///
/// **What a run of these keeps is the coordinate the face it is walking has**
/// (§7.1.3w): a monospace column inside a [`CaretRows::Mono`] block, a pixel
/// inside a [`CaretRows::Prose`] one ([`preview_edit::EditCaret::desired_x`]),
/// and the *file's* own column across the seam between a block and its
/// neighbour, because the block a step lands in has no rows of its own until
/// the next parse makes it the caret's. What neither can promise is the column
/// of a *folded* row against the column of a whole line: leaving a folded block
/// at row three carries the column of that row, not of the line it is part of.
/// That is the same compromise the source face's own
/// [`crate::step_preview_caret_by_row`] makes, said once here rather than
/// discovered twice.
pub fn step_by_row(
    content: &str,
    block: Option<CaretRows<'_>>,
    caret: &mut preview_edit::EditCaret,
    motion: preview_edit::Motion,
    page_rows: usize,
) -> bool {
    let step = match motion {
        preview_edit::Motion::Up => -1isize,
        preview_edit::Motion::Down => 1,
        preview_edit::Motion::PageUp => -(page_rows.max(1) as isize),
        preview_edit::Motion::PageDown => page_rows.max(1) as isize,
        _ => return false,
    };
    caret.heal(content);
    if let Some(CaretRows::Mono(block)) = block
        && block.holds(caret.caret)
        && let Some((row, column)) = block.row_of(caret.caret)
    {
        let wanted = caret.desired_column.unwrap_or(column);
        let target = row as isize + step;
        if target >= 0 && (target as usize) < block.wrap.rows() {
            caret.caret =
                preview_edit::normalize(content, block.offset_at(target as usize, wanted));
            caret.desired_column = Some(wanted);
            return true;
        }
        return step_by_line(content, caret, step, wanted);
    }
    // **A prose block walks the shaper's rows, and keeps a pixel rather than a
    // column** (§7.1.3w). There is no column on a proportional face — the whole
    // of what this ticket learned — so what a run of Up and Down carries is the
    // x the caret set out from, which is a page coordinate and so means the same
    // thing in the block above as in this one.
    if let Some(CaretRows::Prose(prose)) = block
        && let Some((row, x)) = prose.row_of(caret.caret)
    {
        #[allow(clippy::cast_possible_truncation)]
        let wanted = caret.desired_x.unwrap_or_else(|| x.round() as i32);
        let target = row as isize + step;
        if target >= 0 && (target as usize) < prose.len() {
            caret.caret = preview_edit::normalize(
                content,
                #[allow(clippy::cast_precision_loss)]
                prose.offset_at(target as usize, wanted as f32),
            );
            caret.desired_x = Some(wanted);
            return true;
        }
        // Off the top or the bottom: the file's own lines, in the file's own
        // columns — the block it lands in is drawn as prose and has no rows of
        // its own until the caret arrives in it and the next parse makes it the
        // caret's block. The x is kept, so the step *after* that one returns to
        // it.
        let starts = preview_edit::line_starts(content);
        let line = preview_edit::line_index(&starts, caret.caret);
        let (start, _) = preview_edit::line_bounds(content, &starts, line);
        let column = preview_edit::column_of(
            preview_edit::line_text(content, &starts, line),
            caret.caret.saturating_sub(start),
        );
        let moved = step_by_line(content, caret, step, column);
        caret.desired_x = Some(wanted);
        return moved;
    }
    // In a gap, or on a page whose caret has no block: the file's lines, which
    // is the only model there is out here.
    let starts = preview_edit::line_starts(content);
    let line = preview_edit::line_index(&starts, caret.caret);
    let (start, _) = preview_edit::line_bounds(content, &starts, line);
    let column = preview_edit::column_of(
        preview_edit::line_text(content, &starts, line),
        caret.caret.saturating_sub(start),
    );
    let wanted = caret.desired_column.unwrap_or(column);
    step_by_line(content, caret, step, wanted)
}

/// The file's own line model, stepped — the other side of [`step_by_row`]'s
/// seam.
///
/// Off either end of the file is that end, which is what a text field does, and
/// the desired column is kept either way so that coming back returns to it.
fn step_by_line(
    content: &str,
    caret: &mut preview_edit::EditCaret,
    step: isize,
    wanted: usize,
) -> bool {
    let starts = preview_edit::line_starts(content);
    let line = preview_edit::line_index(&starts, caret.caret);
    let target = line as isize + step;
    caret.caret = if target < 0 {
        0
    } else if target as usize >= starts.len() {
        content.len()
    } else {
        preview_edit::offset_at(content, target as usize, wanted)
    };
    caret.caret = preview_edit::normalize(content, caret.caret);
    caret.desired_column = Some(wanted);
    true
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
        let text = "one\n\ntwo\n";
        let spans = ranges(&[(0, 4), (5, 9)]);
        assert_eq!(
            caret_seat(text, &spans, 0),
            CaretSeat::Block(0),
            "at the start"
        );
        assert_eq!(caret_seat(text, &spans, 2), CaretSeat::Block(0), "inside");
        assert_eq!(
            caret_seat(text, &spans, 3),
            CaretSeat::Block(0),
            "the last byte of its own text is still its own",
        );
        assert_eq!(
            caret_seat(text, &spans, 4),
            CaretSeat::Gap { after: Some(0) },
            "one past the range is the blank line, and the blank line is nobody's",
        );
        assert_eq!(
            caret_seat(text, &spans, 5),
            CaretSeat::Block(1),
            "the next block"
        );
        assert_eq!(
            caret_seat(text, &spans, 9),
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
        let text = "# head\ntext\n";
        let spans = ranges(&[(0, 7), (7, 12)]);
        assert_eq!(caret_seat(text, &spans, 6), CaretSeat::Block(0));
        assert_eq!(
            caret_seat(text, &spans, 7),
            CaretSeat::Block(1),
            "no gap to fall into: the next block starts on this very byte",
        );
        assert_eq!(
            caret_seat("", &[], 0),
            CaretSeat::Gap { after: None },
            "an empty document is one gap, and the caret is in front of nothing",
        );
        assert_eq!(
            caret_seat(text, &spans, 0),
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
        let text = "\n\n\n\ntext\n";
        let spans = ranges(&[(4, 8)]);
        assert_eq!(caret_seat(text, &spans, 0), CaretSeat::Gap { after: None });
        assert_eq!(caret_seat(text, &spans, 3), CaretSeat::Gap { after: None });
        assert_eq!(caret_seat(text, &spans, 4), CaretSeat::Block(0));
    }

    /// **The end of a file that does not end in a break belongs to its last
    /// block** (user report, 2026-09-11; audit A6).
    ///
    /// A block's range covers its own line ending, so the byte past the range is
    /// the blank line after it and is nobody's. A file written without a last
    /// break has no such line: the range ends where the text does, and that one
    /// position — where `End` stands, where every character typed at the end of
    /// the document goes — was read as a gap. The paragraph was never drawn as
    /// source and the caret was struck at the left margin of a line below it
    /// that does not exist.
    ///
    /// MUTATION: drop the `!content.ends_with('\n')` clause and the genuine
    /// trailing blank line is swallowed by the last block, so pressing Enter at
    /// the end of a document leaves the old block drawn as source with the caret
    /// nowhere in it.
    #[test]
    fn the_end_of_an_unterminated_last_block_is_that_blocks_own() {
        let content = "abc";
        let spans = ranges(&[(0, 3)]);
        assert_eq!(
            caret_seat(content, &spans, 3),
            CaretSeat::Block(0),
            "End on the only line of the file is inside the block it is the end of",
        );
        assert_eq!(
            place_in_block(content, &spans, 0, 3),
            Some((0, 3)),
            "on its first line, three columns in — after the `c`",
        );
        // The same file with the break it was missing: that byte is the empty
        // last line the editor's own line model insists on, and it is a gap.
        let ended = "abc\n";
        let spans = ranges(&[(0, 4)]);
        assert_eq!(
            caret_seat(ended, &spans, 4),
            CaretSeat::Gap { after: Some(0) },
            "a file that does end in a break still has its empty last line",
        );
        assert_eq!(
            caret_seat(ended, &spans, 3),
            CaretSeat::Block(0),
            "and the end of its text is still the block's",
        );
        assert_eq!(place_in_block(ended, &spans, 0, 3), Some((0, 3)));
        // Two blocks, the last of them unterminated: only the very end of the
        // file is affected, and the gap between them is untouched.
        let two = "one\n\ntwo";
        let spans = ranges(&[(0, 4), (5, 8)]);
        assert_eq!(
            caret_seat(two, &spans, 4),
            CaretSeat::Gap { after: Some(0) },
            "the blank line between two paragraphs is nobody's, as it ever was",
        );
        assert_eq!(caret_seat(two, &spans, 8), CaretSeat::Block(1));
        assert_eq!(place_in_block(two, &spans, 1, 8), Some((0, 3)));
        // CRLF, where the ending is two bytes and a caret may stand at neither
        // of the positions inside it.
        let crlf = "one\r\n\r\ntwo";
        let spans = ranges(&[(0, 5), (7, 10)]);
        assert_eq!(caret_seat(crlf, &spans, 10), CaretSeat::Block(1));
        assert_eq!(place_in_block(crlf, &spans, 1, 10), Some((0, 3)));
        assert_eq!(
            place_in_block(crlf, &spans, 0, 3),
            Some((0, 3)),
            "and the end of a CRLF line is still the end of its text",
        );
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

    /// The three things a walk through the source block needs, built the way
    /// the painter builds them: the block's own bytes and the fold they draw in.
    fn folded(
        content: &str,
        range: &Range<usize>,
        columns: Option<usize>,
    ) -> (String, preview_edit::WrapLayout) {
        let text = block_source(content, range).to_owned();
        let lines = preview_edit::display_lines(&text);
        let wrap = match columns {
            Some(columns) => preview_edit::WrapLayout::wrapped(&lines, columns),
            None => preview_edit::WrapLayout::unwrapped(&lines),
        };
        (text, wrap)
    }

    /// **A click inside the source block names a byte of the file** (T5 ①), and
    /// the arithmetic is the painter's read backwards: the row is a folded row
    /// of *this block*, the column is a column of that row, and the answer is an
    /// offset into the whole file.
    ///
    /// MUTATION: leave `self.start` off [`BlockRows::offset_at`]'s answer and
    /// every click in the second block of a document lands in the first one.
    #[test]
    fn a_row_and_a_column_of_the_source_block_name_a_file_byte() {
        let content = "# head\n\nthe paragraph\n";
        let (text, wrap) = folded(content, &(8..22), None);
        let rows = BlockRows {
            text: &text,
            start: 8,
            wrap: &wrap,
        };
        assert_eq!(rows.offset_at(0, 0), 8, "the block's first byte");
        assert_eq!(rows.offset_at(0, 4), 12, "four columns in");
        assert_eq!(
            rows.offset_at(0, 900),
            21,
            "past the end of the row is the end of its own line",
        );
        assert_eq!(
            rows.offset_at(9, 0),
            21,
            "a row past the end of the block is the end of the block",
        );
        assert_eq!(rows.row_of(12), Some((0, 4)), "and back again");
        assert_eq!(rows.row_of(3), None, "a byte of another block is not ours");
    }

    /// **A folded block answers in its own rows**, which is what the reader can
    /// see: one long paragraph line drawn as three rows is three rows to walk.
    #[test]
    fn a_folded_line_answers_in_the_rows_it_draws_as() {
        // One block, one line, eighteen columns, folded at six.
        let content = "aaa bbb ccc ddd\n";
        let (text, wrap) = folded(content, &(0..16), Some(7));
        let rows = BlockRows {
            text: &text,
            start: 0,
            wrap: &wrap,
        };
        assert!(wrap.rows() > 1, "the fixture has to actually fold");
        let (row, column) = rows.row_of(9).expect("inside the block");
        assert!(row > 0, "the tenth byte is on the second row or later");
        assert_eq!(
            rows.offset_at(row, column),
            9,
            "a row and a column round-trip through the fold",
        );
    }

    /// **Down inside the source block walks its rows; down off the bottom walks
    /// the file's lines** (T5 ②, §7.1.3t) — and the byte it lands on is in the
    /// next block or in the gap, where [`caret_seat`] answers again.
    ///
    /// MUTATION: drop the `target < wrap.rows()` guard and Down at the bottom of
    /// a block clamps to the block's own end for ever, so no arrow key can ever
    /// leave the block the caret is in.
    #[test]
    fn down_out_of_the_bottom_row_walks_into_what_is_under_it() {
        // "one\n\ntwo\n" — two paragraphs with a blank line between them.
        let content = "one\n\ntwo\n";
        let spans = ranges(&[(0, 4), (5, 9)]);
        let (text, wrap) = folded(content, &spans[0], None);
        let rows = BlockRows {
            text: &text,
            start: spans[0].start,
            wrap: &wrap,
        };
        let mut caret = preview_edit::EditCaret {
            anchor: 1,
            caret: 1,
            desired_column: None,
            desired_x: None,
        };
        assert!(step_by_row(
            content,
            Some(CaretRows::Mono(rows)),
            &mut caret,
            preview_edit::Motion::Down,
            10
        ));
        assert_eq!(caret.caret, 4, "the blank line between the two paragraphs");
        assert_eq!(
            caret_seat(content, &spans, caret.caret),
            CaretSeat::Gap { after: Some(0) },
            "which is a gap, and a gap is drawn as one empty source line",
        );
        // And on again into the block under it, the column kept.
        assert!(step_by_row(
            content,
            None,
            &mut caret,
            preview_edit::Motion::Down,
            10
        ));
        assert_eq!(caret.caret, 6, "one column into the second paragraph");
        assert_eq!(
            caret_seat(content, &spans, caret.caret),
            CaretSeat::Block(1)
        );
    }

    /// **Up off the top row lands in the block above**, and the desired column
    /// survives a short line on the way — the behaviour every editor has.
    #[test]
    fn up_out_of_the_top_row_lands_above_and_keeps_the_column() {
        let content = "a long first line\nx\nthe third line\n";
        let spans = ranges(&[(0, 35)]);
        let (text, wrap) = folded(content, &spans[0], None);
        let rows = BlockRows {
            text: &text,
            start: spans[0].start,
            wrap: &wrap,
        };
        let mut caret = preview_edit::EditCaret {
            anchor: 26,
            caret: 26,
            desired_column: None,
            desired_x: None,
        };
        assert_eq!(rows.row_of(26), Some((2, 6)), "six columns into line three");
        assert!(step_by_row(
            content,
            Some(CaretRows::Mono(rows)),
            &mut caret,
            preview_edit::Motion::Up,
            10
        ));
        assert_eq!(caret.caret, 19, "the short line, at its end");
        assert!(step_by_row(
            content,
            Some(CaretRows::Mono(rows)),
            &mut caret,
            preview_edit::Motion::Up,
            10
        ));
        assert_eq!(caret.caret, 6, "and the column comes back on the long one");
    }

    /// **A caret in a gap has no block and walks the file's lines**, which is
    /// how it gets out again.
    #[test]
    fn a_caret_in_a_gap_walks_the_files_lines() {
        let content = "one\n\ntwo\n";
        let mut caret = preview_edit::EditCaret {
            anchor: 4,
            caret: 4,
            desired_column: None,
            desired_x: None,
        };
        assert!(step_by_row(
            content,
            None,
            &mut caret,
            preview_edit::Motion::Up,
            10
        ));
        assert_eq!(caret.caret, 0, "the paragraph above it");
        assert!(!step_by_row(
            content,
            None,
            &mut caret,
            preview_edit::Motion::Left,
            10
        ));
        assert_eq!(caret.caret, 0, "a horizontal motion is not this one's");
    }
    // ── the prose face ──────────────────────────────────────────────────────

    /// The rows a shaper handed back, as a test can write them down: one entry
    /// per row, each a top and the seams on it.
    ///
    /// Twenty pixels a row, which is the number every arithmetic below is done
    /// in, and the x's are the ones a body face would put an ideograph at
    /// sixteen pixels and a star at eight.
    fn prose_rows(rows: &[(f32, &[(usize, f32)])]) -> ProseRows {
        ProseRows {
            index: 1,
            rows: rows
                .iter()
                .map(|(top, seams)| ProseRow {
                    top: *top,
                    height: 20.0,
                    seams: seams
                        .iter()
                        .map(|(offset, x)| ProseSeam {
                            offset: *offset,
                            x: *x,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// `**预览**窗格` at the offsets and x's the body face draws it at — the
    /// mixed line this whole ticket is about, marks and all: two stars, two
    /// ideographs, two stars, two ideographs.
    fn marked_cjk_row() -> ProseRows {
        prose_rows(&[(
            100.0,
            &[
                (0, 0.0),
                (1, 8.0),
                (2, 16.0),
                (5, 32.0),
                (8, 48.0),
                (9, 56.0),
                (10, 64.0),
                (13, 80.0),
                (16, 96.0),
            ],
        )])
    }

    /// **A press in a prose block names the byte under the pointer** (§7.1.3w),
    /// on a line of Chinese and on a line that changes script — the two the
    /// monospace grid got wrong in 2026-09-11's report.
    ///
    /// The nearest *seam* and not the cluster the pointer is inside: a press on
    /// the right half of a character puts the caret after it, which is this
    /// window's rule on every face it has. A cluster three bytes long and
    /// sixteen pixels wide is one place either side and nothing in between.
    ///
    /// MUTATION: round to the cluster the pointer is inside and a press past the
    /// middle of the last character can never reach the end of the line.
    #[test]
    fn a_press_in_a_prose_block_names_the_byte_under_the_pointer() {
        let rows = marked_cjk_row();
        // On the marks themselves, which is the whole point of this face: the
        // stars are characters and a caret may stand between them.
        assert_eq!(rows.press(0.0, 105.0), 0, "in front of the first star");
        assert_eq!(rows.press(9.0, 105.0), 1, "between the two stars");
        assert_eq!(
            rows.press(17.0, 105.0),
            2,
            "in front of the first ideograph"
        );
        // Left half of an ideograph rounds back, right half rounds on.
        assert_eq!(rows.press(20.0, 105.0), 2, "its left half");
        assert_eq!(rows.press(28.0, 105.0), 5, "its right half");
        assert_eq!(rows.press(96.0, 105.0), 16, "the end of the line");
        assert_eq!(rows.press(400.0, 105.0), 16, "and past the end of it");
        // Mixed script, and a y above and below the block: a press has already
        // been judged to be this block's by the time it arrives.
        let mixed = prose_rows(&[(
            100.0,
            &[
                (0, 0.0),
                (1, 9.0),
                (2, 18.0),
                (3, 27.0),
                (4, 36.0),
                (7, 52.0),
            ],
        )]);
        assert_eq!(mixed.press(19.0, 105.0), 2, "abc, the b");
        assert_eq!(mixed.press(-40.0, 40.0), 0, "above the block is its start");
        assert_eq!(mixed.press(400.0, 900.0), 7, "below it is its end");
    }

    /// **The caret, the candidate box and the press read one geometry**
    /// (§7.1.3u, on the face that has no columns).
    ///
    /// The x a press rounds to is the x the caret is struck at, exactly, and it
    /// is the x the IME is handed — because all three are this one value. The
    /// round trip is the assertion: press anywhere, and the caret that press
    /// seats stands on the seam the press was rounded to, to the pixel.
    ///
    /// MUTATION: derive the caret's x from anything but these seams — an advance
    /// times an index, say — and the two stop agreeing on the very first
    /// ideograph, which is the 3.19px-a-character report of 2026-09-11 said in
    /// the proportional face.
    #[test]
    fn the_caret_and_the_ime_box_and_the_press_share_one_geometry_in_a_prose_block() {
        let rows = marked_cjk_row();
        for x in [0.0, 5.0, 9.0, 17.0, 20.0, 28.0, 50.0, 70.0, 95.0, 200.0] {
            let offset = rows.press(x, 105.0);
            let caret = rows.caret(offset).expect("the byte a press named is drawn");
            let seam = rows.rows[0]
                .seams
                .iter()
                .find(|seam| seam.offset == offset)
                .expect("a press rounds to a seam");
            assert!(
                (caret[0] - seam.x).abs() < f32::EPSILON,
                "a press at {x} named byte {offset}, drawn at {}, and the caret stands at {}",
                seam.x,
                caret[0],
            );
            // The caret is a hairline on the row's own line box — which is the
            // rectangle the candidate list is told not to cover.
            assert!((caret[1] - 100.0).abs() < f32::EPSILON);
            assert!((caret[3] - 120.0).abs() < f32::EPSILON);
        }
    }

    /// **Up and Down walk the shaper's rows** (§7.1.3w), and the x survives a
    /// short row on the way — the behaviour every editor has, said in pixels
    /// because a proportional face has no column to say it in.
    ///
    /// MUTATION: keep the desired place in columns and a walk down a page of
    /// Chinese drifts a character to the left per row; drop the `< len()` guard
    /// and no arrow key can ever leave the block.
    #[test]
    fn up_and_down_walk_the_shapers_rows_in_a_prose_block() {
        // One source line folded into three rows: a long first row, a short
        // second one, and a third as long as the first.
        let content = "alpha beta gamma delta\n\nnext\n";
        let rows = prose_rows(&[
            (100.0, &[(0, 0.0), (3, 30.0), (6, 60.0), (9, 90.0)]),
            (120.0, &[(9, 0.0), (12, 30.0)]),
            (140.0, &[(12, 0.0), (15, 30.0), (18, 60.0), (22, 90.0)]),
        ]);
        let mut caret = preview_edit::EditCaret {
            anchor: 6,
            caret: 6,
            desired_column: None,
            desired_x: None,
        };
        assert!(step_by_row(
            content,
            Some(CaretRows::Prose(&rows)),
            &mut caret,
            preview_edit::Motion::Down,
            10
        ));
        assert_eq!(caret.caret, 12, "the short row, at its end");
        assert_eq!(caret.desired_x, Some(60), "and the x it set out from");
        assert!(step_by_row(
            content,
            Some(CaretRows::Prose(&rows)),
            &mut caret,
            preview_edit::Motion::Down,
            10
        ));
        assert_eq!(caret.caret, 18, "the x comes back on the row under it");
        // Off the bottom row is the file's own lines again, which is where the
        // next block begins — and the x is kept for the block it lands in.
        assert!(step_by_row(
            content,
            Some(CaretRows::Prose(&rows)),
            &mut caret,
            preview_edit::Motion::Down,
            10
        ));
        assert_eq!(caret.caret, 23, "the blank line after the paragraph");
        assert_eq!(caret.desired_x, Some(60));
        // And up out of the top row, the same way.
        let mut caret = preview_edit::EditCaret {
            anchor: 3,
            caret: 3,
            desired_column: None,
            desired_x: None,
        };
        assert!(step_by_row(
            content,
            Some(CaretRows::Prose(&rows)),
            &mut caret,
            preview_edit::Motion::Up,
            10
        ));
        assert_eq!(
            caret.caret, 0,
            "off the top of the block is the file's line"
        );
    }

    /// **A selection across a wrapped prose line draws one band per row**
    /// (§7.1.3w) — a band per *line* would start on the first row and run off
    /// the right edge of the pane instead of turning the corner with the text.
    ///
    /// MUTATION: band from the range's ends without cutting it against each
    /// row's own bytes and a two-row selection comes back as one rectangle from
    /// the first seam to the last, covering the margin between them.
    #[test]
    fn a_selection_across_a_wrapped_prose_line_draws_one_band_per_row() {
        let rows = prose_rows(&[
            (100.0, &[(0, 0.0), (3, 30.0), (6, 60.0), (9, 90.0)]),
            (120.0, &[(9, 0.0), (12, 30.0), (15, 60.0)]),
        ]);
        let bands = rows.bands(&(3..12));
        assert_eq!(bands.len(), 2, "one band per row the selection touches");
        assert_eq!(bands[0], [30.0, 100.0, 90.0, 120.0], "the first row's tail");
        assert_eq!(bands[1], [0.0, 120.0, 30.0, 140.0], "the second row's head");
        // A selection inside one row is one band, and an empty one is none.
        assert_eq!(rows.bands(&(3..6)), vec![[30.0, 100.0, 60.0, 120.0]]);
        assert!(rows.bands(&(6..6)).is_empty());
        // A selection that began in the block above draws the part of itself
        // that is here, which is what makes one drag across two faces one band.
        let from_above = rows.bands(&(0..4));
        assert_eq!(from_above.len(), 1);
        assert!((from_above[0][0] - 0.0).abs() < f32::EPSILON);
    }

    /// **A byte that ends one row and begins the next is drawn on the first**
    /// (§7.1.3w, audit A5) — the affinity a face with no affinity bit has to
    /// pick, and the one that stops Down landing at the far left of the row
    /// under the one the reader was aiming at and Up from there answering with
    /// the same byte for ever.
    #[test]
    fn a_soft_wrap_seam_belongs_to_the_row_that_ends_with_it() {
        let rows = prose_rows(&[
            (100.0, &[(0, 0.0), (3, 30.0), (6, 60.0)]),
            (120.0, &[(6, 0.0), (9, 30.0)]),
        ]);
        assert_eq!(rows.row_of(6), Some((0, 60.0)), "the row it ends");
        assert_eq!(rows.caret(6).map(|rect| rect[1]), Some(100.0));
        // The end of the last row is the end of the block, and it has nowhere
        // else to be.
        assert_eq!(rows.row_of(9), Some((1, 30.0)));
        assert_eq!(rows.row_of(10), None, "past the block is not the block's");
    }
}
