//! **A byte on the page, and the byte of the file it was copied from** (ticket
//! T6 of the Markdown editing block; research
//! `docs/plans/markdown-edit/research-2026-09-10.md` §1.3, §2, §9.6).
//!
//! T1 gave every block the range of source it was parsed from. That is enough
//! to splice a block back into a file and not enough to put a caret anywhere: a
//! block's payload is a *normalised* rendering, so the third word of a paragraph
//! is at no offset anybody can name in the file. This module is the other half —
//! per **byte**, in both directions, over the whole document.
//!
//! # Two spaces and one answer
//!
//! [`crate::preview_select`] reads the page in **pieces**: one run of text that
//! is set as one paragraph, addressed by [`Place`]. A place is what a click
//! yields ([`crate::preview_place_at`]) and what a selection is made of. The
//! file is bytes. [`file_offset_of`] takes the first to the second and
//! [`place_of`] takes the second to the first, and both are total over a
//! document: any place on drawn text has a file byte, and any file byte has a
//! place.
//!
//! Between them stands [`TextOrigin`] — one drawn string, and for each of its
//! bytes either the source byte it is a copy of ([`Origin::File`]) or the
//! statement that the page draws it and the source does not spell it
//! ([`Origin::Drawn`]). The parser builds one per span against its own block's
//! text ([`crate::preview::parse_markdown_mapped`]), the block joins build one
//! per unit against the file, and [`TextOrigin::through`] composes the two into
//! the one this module answers from: **piece byte → file byte**.
//!
//! # The marks the page does not draw
//!
//! Every markup byte is a source byte with no place on the page, and every
//! synthesised byte is a page byte with no source. Both have to be **skippable
//! in both directions**, and this is the table of who is which. "Undrawn" means
//! the file spells it and the page does not; "synthesised" means the page draws
//! it and the file does not.
//!
//! | The mark | Where | Which | The rule |
//! |---|---|---|---|
//! | `#`…`###### ` | opening a heading | undrawn | the piece begins at the first byte after the space. The *closing* hashes of `## Title ##` are **drawn**: this parser keeps them (`parse_heading`), so they are text like any other |
//! | `> ` | opening a quoted line | undrawn | one per line of the quote |
//! | `- `, `* `, `1. ` | a list item's marker | undrawn | it is not in `Piece::text` at all — copy writes it back out of `Piece::prefix`, which is the list's mark and not the item's |
//! | `*`, `_` | an emphasis pair | undrawn **when spent** | a pair spends the delimiters *nearest the text*, so an opener's leftovers are its head bytes and a closer's are its tail; whatever is left is what the author typed and is drawn as text |
//! | `` ` `` | around a code span | undrawn | the span's text is the bytes between them |
//! | `[`, `](`, `)`, the destination, a title | a link | undrawn | the label is drawn and the destination rides in `Span::target`, which is beside the text and not in it |
//! | `![`, `](`, the destination, `)` | a picture **cut out of prose** | drawn | the piece is `![alt](src)` (`preview_select::image_piece`) and every byte of it is read back out of the file — the title, if there was one, is the part that is undrawn |
//! | `!`, `[`, `]`, `(`, `)` | a picture **inside** a heading, a cell, an item or a quote | undrawn | there the run is its own alt text and nothing else |
//! | `|`, and the whole separator row | a table | undrawn | a cell's piece is the cell trimmed |
//! | ```` ``` ````, the info string | a fence | undrawn | a fence's pieces are its body lines |
//! | `$$`, `\[`, `\]`, `\begin{…}` on their own lines | display mathematics | undrawn | the block dropped them (`MarkdownBlock::Math`) and the piece re-spells its own — see the synthesised list below |
//! | `$…$`, `\(…\)` | inline mathematics | **drawn** | `SpanStyle::Math` keeps its delimiters in `Span::text` deliberately, so they are copies like the formula between them |
//! | the indent, the break | between two joined source lines | undrawn | the single space that stands in their place is synthesised |
//!
//! **The bytes the page draws that no file spells**, which is the whole list a
//! test may hold this module to:
//!
//! 1. the single space `join_source_lines` puts between two source lines of a
//!    paragraph or of a quoted paragraph;
//! 2. the single space a list item's lazy continuation is joined on;
//! 3. the spaces a tab expands to in a fence line (`expand_tabs`);
//! 4. the `\n` between two lines of a display formula's body;
//! 5. the `$$` `preview_select` re-spells around a display formula, the file's
//!    own delimiters standing on lines of their own or being `\[`…`\]` or an
//!    environment's `\begin`…`\end`;
//! 6. the whole `![alt](src)` a picture written as `<img>` or `<picture>` copies
//!    as — that spelling is markdown the file never contained.
//!
//! # Which way a caret rounds
//!
//! * **A place to a file byte.** A place standing at a drawn byte answers with
//!   the byte it was copied from. One standing at a synthesised byte, or at the
//!   very end of a piece, answers **one past the last copy before it** — so a
//!   caret "just after the bold word" lands after the closing `**` rather than
//!   inside it, and a caret at the end of a heading lands at the end of its
//!   text rather than at the start of the next block.
//! * **A file byte to a place.** A drawn byte answers with the place it is drawn
//!   at. An undrawn one answers with the **nearest drawn position**: the
//!   position one past the last drawn byte before it *inside the same block*,
//!   or, when the block draws nothing before it, the first drawn byte after it.
//!   A byte in the tissue between two blocks belongs to neither, and answers
//!   with the start of the block that follows it.
//!
//! The two round-trip exactly on every drawn byte, which is what
//! `the_two_directions_are_inverse_on_every_drawn_byte` holds. Two places do not
//! come back as themselves, and neither is a defect. A place at a *synthesised*
//! byte names a byte the file does not have — a caret inside the `$$` of a
//! formula's piece — so it rounds to the copy before it and comes back rounded.
//! And the **end of a piece is a position rather than a byte**: the prose before
//! a picture ends at the very byte the picture's piece begins with, and the end
//! of a picture inside a link stands in front of the link's own `](…)`, which
//! nobody draws. A byte belongs to the piece that draws it, so that is the name
//! `place_of` gives it. The invariant that does hold everywhere is that the two
//! names have nothing *drawn* between them.
//!
//! **The two directions have no product caller yet, and say so.** The caret is
//! T5 and the second direction is what tells it where the caret went when its
//! block was re-parsed; the reverse-video toolbar that wants
//! [`TextOrigin::text_of`] is v2's. They carry `#[allow(dead_code)]` naming the
//! slice that will call them, which is the discipline `git.rs` states: an
//! `allow` that outlives its excuse is then visible rather than merely
//! tolerated. What is not deferred is the *mapping* — it is built on every
//! parse, and it is held by test on every fixture, because a map discovered at
//! its first call site is a map the parser has already got wrong.

use std::ops::Range;

use crate::preview::MarkdownBlock;
use crate::preview_select::Place;

/// **Where one byte of drawn text came from.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    /// A byte-for-byte copy of this byte of the source.
    File(usize),
    /// The page draws it and the source does not spell it — a join's space, a
    /// tab's expansion, a delimiter a piece puts back on. See the module's list.
    Drawn,
}

/// One run of drawn bytes with one answer between them.
///
/// A run and not a byte, because everything the parser copies it copies whole:
/// a span's text is a slice of its block, a cell is a slice of its row. The
/// runs of a [`TextOrigin`] are contiguous, in text order, and cover the whole
/// of the text they describe.
#[derive(Clone, Debug, Eq, PartialEq)]
struct OriginRun {
    /// Where this run stands in the drawn text.
    text: Range<usize>,
    /// The source byte `text.start` was copied from, or `None` for a run the
    /// page draws and the source does not spell.
    source: Option<usize>,
}

/// **Where each byte of one drawn string came from.**
///
/// Built forwards, by the walk that builds the string itself: [`Self::copied`]
/// for bytes taken out of the source and [`Self::drawn`] for bytes the renderer
/// spelled. The two spaces it can be written in are the reason for
/// [`Self::through`]: the parser knows where a span's text stands in its
/// *block's* text, the block's join knows where its text stands in the *file*,
/// and neither of them alone can answer a click.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextOrigin {
    runs: Vec<OriginRun>,
    len: usize,
}

impl TextOrigin {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One string that is a copy of `len` bytes of the source from `source` on.
    #[must_use]
    pub fn slice(source: usize, len: usize) -> Self {
        let mut origin = Self::new();
        origin.copied(source, len);
        origin
    }

    /// `len` more bytes, copied from `source` onwards.
    ///
    /// Joined to the run before it when the two are contiguous in both spaces,
    /// so that a walk depositing a character at a time still leaves one run.
    pub fn copied(&mut self, source: usize, len: usize) {
        if len == 0 {
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && let Some(start) = last.source
            && start + last.text.len() == source
        {
            last.text.end += len;
            self.len += len;
            return;
        }
        self.runs.push(OriginRun {
            text: self.len..self.len + len,
            source: Some(source),
        });
        self.len += len;
    }

    /// `len` more bytes the page draws that the source does not spell.
    pub fn drawn(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && last.source.is_none()
        {
            last.text.end += len;
            self.len += len;
            return;
        }
        self.runs.push(OriginRun {
            text: self.len..self.len + len,
            source: None,
        });
        self.len += len;
    }

    /// Everything `other` describes, drawn after everything this describes.
    pub fn append(&mut self, other: &Self) {
        for run in &other.runs {
            match run.source {
                Some(source) => self.copied(source, run.text.len()),
                None => self.drawn(run.text.len()),
            }
        }
    }

    /// How many bytes of drawn text this describes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// `#[allow(dead_code)]`: the pair of [`Self::len`], which clippy asks for
    /// wherever there is a length and no product caller has yet needed.
    #[allow(dead_code)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Where the byte at `offset` came from, or `None` past the end.
    ///
    /// `#[allow(dead_code)]`: the per-byte question the tests ask of every
    /// fixture; the product asks [`file_offset_of`], which rounds.
    #[allow(dead_code)]
    #[must_use]
    pub fn origin_of(&self, offset: usize) -> Option<Origin> {
        let run = self.run_at(offset)?;
        Some(match run.source {
            Some(source) => Origin::File(source + offset - run.text.start),
            None => Origin::Drawn,
        })
    }

    /// Where the source byte `source` is drawn, or `None` if it is not.
    ///
    /// `#[allow(dead_code)]`: v2's — a toolbar putting `**` round a word has to
    /// find the word again after the file moved under it.
    #[allow(dead_code)]
    #[must_use]
    pub fn text_of(&self, source: usize) -> Option<usize> {
        self.runs.iter().find_map(|run| {
            let start = run.source?;
            (source >= start && source < start + run.text.len())
                .then(|| run.text.start + source - start)
        })
    }

    /// **This text's bytes in the space the string it was cut from is written
    /// in.** `self` maps a drawn string to the text of `outer`; `outer` maps
    /// that text to the file; the answer maps the drawn string to the file.
    ///
    /// A copied run is split wherever `outer` changes its answer, which is what
    /// makes a bold phrase running across a paragraph's line join come out as
    /// two copies with a synthesised space between them rather than as one run
    /// naming bytes that are not its own.
    #[must_use]
    pub fn through(&self, outer: &Self) -> Self {
        let mut out = Self::new();
        for run in &self.runs {
            let Some(start) = run.source else {
                out.drawn(run.text.len());
                continue;
            };
            let mut taken = 0usize;
            while taken < run.text.len() {
                let at = start + taken;
                let Some(outer_run) = outer.run_at(at) else {
                    // Past the end of what the outer string describes: nothing
                    // in the file answers for it, which is what `Drawn` says.
                    out.drawn(run.text.len() - taken);
                    break;
                };
                let take = (outer_run.text.end - at).min(run.text.len() - taken);
                match outer_run.source {
                    Some(source) => out.copied(source + at - outer_run.text.start, take),
                    None => out.drawn(take),
                }
                taken += take;
            }
        }
        out
    }

    fn run_at(&self, offset: usize) -> Option<&OriginRun> {
        self.runs.iter().find(|run| run.text.contains(&offset))
    }

    /// The file byte this **place** in the text resolves to — the rounding rule
    /// the module doc states, over one piece.
    ///
    /// `offset` may be the text's length, which is where a caret at the end of a
    /// piece stands.
    fn file_offset_at(&self, offset: usize) -> Option<usize> {
        if offset > self.len {
            return None;
        }
        if let Some(Origin::File(source)) = self.origin_of(offset) {
            return Some(source);
        }
        // One past the last byte copied before here — the answer that puts a
        // caret at the end of the bold word *after* its closing delimiters.
        let before = self
            .runs
            .iter()
            .filter(|run| run.text.start < offset)
            .filter_map(|run| {
                let source = run.source?;
                Some(source + run.text.end.min(offset) - run.text.start)
            })
            .next_back();
        before.or_else(|| {
            // Nothing before it was copied: the first copy after it, and failing
            // that this piece draws nothing the file spells at all.
            self.runs
                .iter()
                .find_map(|run| run.source.filter(|_| run.text.start >= offset))
        })
    }
}

/// **Every piece of one block**, in [`crate::preview_select::pieces`]'s own
/// numbering.
///
/// The numbering is that module's to define and everyone else's to agree with;
/// the parser walks the same lists in the same order and a test holds the two
/// together, exactly as the layout is held to it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockOrigins {
    pub pieces: Vec<TextOrigin>,
}

impl BlockOrigins {
    /// A block whose pieces are these, in order.
    #[must_use]
    pub fn new(pieces: Vec<TextOrigin>) -> Self {
        Self { pieces }
    }

    /// A block of one piece.
    #[must_use]
    pub fn one(piece: TextOrigin) -> Self {
        Self::new(vec![piece])
    }

    /// A block with no text in it at all — a rule.
    #[must_use]
    pub fn none() -> Self {
        Self::new(Vec::new())
    }
}

/// **The file byte a place on the page stands at.**
///
/// `None` when the place names a block or a piece the document does not have;
/// every place inside the document answers, because every piece of it has a
/// block range to fall back on even when the piece draws nothing the file
/// spells (a picture written as HTML).
///
/// `#[allow(dead_code)]`: T5's, which is where a click becomes a caret.
#[allow(dead_code)]
#[must_use]
pub fn file_offset_of(
    place: &Place,
    blocks: &[MarkdownBlock],
    ranges: &[Range<usize>],
    maps: &[BlockOrigins],
) -> Option<usize> {
    blocks.get(place.block)?;
    let range = ranges.get(place.block)?;
    let piece = maps.get(place.block)?.pieces.get(place.piece)?;
    // The block's first byte is the answer for a piece with no copy in it at
    // all, which is a picture the file wrote as HTML and nothing else. It is not
    // a clamp on the others: a piece that named a byte outside its own block
    // would be a defect, and `a_file_byte_lands_on_the_nearest_drawn_place`
    // asks every fixture for one rather than papering over it here.
    piece.file_offset_at(place.offset).or(Some(range.start))
}

/// **The place on the page a file byte stands at**, rounded as the module doc
/// says when the byte is one the page does not draw.
///
/// `None` only for a document with no drawn text in it at all.
///
/// `#[allow(dead_code)]`: T5's, which asks it once a keystroke — "the caret's
/// block was re-parsed, where is the caret on the page now" — and v2's, which
/// asks it of a selection the file changed under.
#[allow(dead_code)]
#[must_use]
pub fn place_of(
    file_offset: usize,
    blocks: &[MarkdownBlock],
    ranges: &[Range<usize>],
    maps: &[BlockOrigins],
) -> Option<Place> {
    let block = ranges
        .iter()
        .position(|range| range.contains(&file_offset))
        .filter(|block| *block < blocks.len() && *block < maps.len());
    if let Some(block) = block {
        if let Some(place) = place_in_block(file_offset, block, &maps[block]) {
            return Some(place);
        }
        if !maps[block].pieces.is_empty() {
            // A block that draws nothing the file spells — a picture written as
            // HTML — still has a piece to stand in.
            return Some(Place::new(block, 0, 0));
        }
    }
    // Either the byte is the tissue between two blocks, which belongs to
    // neither, or it is inside a block with no text in it at all — a rule. Both
    // answer with the start of the next block that has words, and with the end
    // of the last one that had them when nothing follows.
    let from = block.map_or_else(
        || {
            ranges
                .iter()
                .position(|range| range.start > file_offset)
                .unwrap_or(maps.len())
        },
        |block| block + 1,
    );
    (from..maps.len())
        .find(|block| !maps[*block].pieces.is_empty())
        .map(|block| Place::new(block, 0, 0))
        .or_else(|| {
            (0..from.min(maps.len())).rev().find_map(|block| {
                let piece = maps[block].pieces.len().checked_sub(1)?;
                Some(Place::new(block, piece, maps[block].pieces[piece].len()))
            })
        })
}

/// The nearest drawn position to `file_offset` inside one block, or `None` when
/// the block draws nothing the file spells.
fn place_in_block(file_offset: usize, block: usize, map: &BlockOrigins) -> Option<Place> {
    // The nearest copy that ends at or before this byte, and the nearest that
    // begins after it — by *source* position and not by the order the runs were
    // deposited in, because a block's pieces need not draw its bytes in the
    // order the file spells them.
    let mut before: Option<(usize, Place)> = None;
    let mut after: Option<(usize, Place)> = None;
    for (index, piece) in map.pieces.iter().enumerate() {
        for run in &piece.runs {
            let Some(source) = run.source else {
                continue;
            };
            let end = source + run.text.len();
            if file_offset >= source && file_offset < end {
                return Some(Place::new(
                    block,
                    index,
                    run.text.start + file_offset - source,
                ));
            }
            if end <= file_offset {
                // One past the last byte drawn before it.
                if before.is_none_or(|(at, _)| at < end) {
                    before = Some((end, Place::new(block, index, run.text.end)));
                }
            } else if after.is_none_or(|(at, _)| at > source) {
                after = Some((source, Place::new(block, index, run.text.start)));
            }
        }
    }
    before.or(after).map(|(_, place)| place)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::{MarkdownBlock, parse_markdown_mapped};
    use crate::preview_select::{Piece, copy_text, pieces};

    /// The same bytes as a file written on this platform.
    fn crlf(src: &str) -> String {
        src.replace('\n', "\r\n")
    }

    /// **Every document the provenance is asked of** — T1's own set, and the
    /// eight shapes this ticket names on top of it.
    fn fixtures() -> Vec<(String, String)> {
        let mut fixtures = Vec::new();
        for (name, src) in [
            ("the page", ranged_page()),
            (
                "nested emphasis",
                "a ***deep* and **wide*** word\n".to_owned(),
            ),
            (
                "a link with a title",
                "see [the page](where.md \"why\") now\n".to_owned(),
            ),
            (
                "inline code with backticks",
                "call `a ` b` and `c` here\n".to_owned(),
            ),
            (
                "inline mathematics",
                "the mass $E = mc^2$ and \\(x\\) stand here\n".to_owned(),
            ),
            (
                "a heading with trailing hashes",
                "## Title ##\n\nprose under it\n".to_owned(),
            ),
            (
                "a list with a lazy continuation",
                "- first item\n  wrapped under it\n- second\n".to_owned(),
            ),
            (
                "a table with aligned cells",
                "| left | right |\n|:---|---:|\n| `a` | **b** |\n".to_owned(),
            ),
            (
                "a quote over two lines",
                "> a quote\n> that wraps\n".to_owned(),
            ),
            (
                "a picture in a sentence",
                "before ![alt](one.png) after\n".to_owned(),
            ),
            (
                "a picture in a link",
                "[![alt](one.png)](where)\n".to_owned(),
            ),
            ("a tab in a fence", "```\n\tone\tstep\n```\n".to_owned()),
            ("nothing at all", String::new()),
            ("one word", "word".to_owned()),
            (
                "this repository's front page",
                include_str!("../../../README.md").to_owned(),
            ),
            // **The two this set was missing** (2026-09-10). Every document
            // above is written in English, which is the one script whose bytes
            // and whose characters are the same count and whose words are spaced
            // — so a map that had confused the two would have come back right
            // from all of them. See [`crate::preview::CHINESE_PAGE`].
            (
                "this repository's front page in Chinese",
                crate::preview::CHINESE_PAGE.to_owned(),
            ),
            (
                "Chinese and English in one page",
                crate::preview::MIXED_SCRIPT_PAGE.to_owned(),
            ),
            (
                "this file's own source",
                include_str!("preview.rs").to_owned(),
            ),
        ] {
            fixtures.push((format!("{name}, LF"), src.clone()));
            fixtures.push((format!("{name}, CRLF"), crlf(&src)));
        }
        fixtures
    }

    /// T1's own page, which is one of every block this parser has.
    fn ranged_page() -> String {
        [
            "# Title",
            "",
            "A paragraph that wraps",
            "over two source lines with **bold across",
            "the fold** in it.",
            "",
            "```rust",
            "let x = 1;",
            "```",
            "",
            "$$",
            "E = mc^2",
            "$$",
            "",
            "| a | b |",
            "|---|---|",
            "| 1 | 2 |",
            "",
            "> a quote",
            "> that wraps",
            "",
            "---",
            "",
            "1. first",
            "2. second",
            "",
            "- bullet",
            "  its lazy continuation",
            "- another",
        ]
        .join("\n")
            + "\n"
    }

    /// **Which bytes of the file the page draws somewhere** — every byte not
    /// marked here is markup, tissue, or a title nobody shows.
    fn drawn_bytes(src: &str, blocks: &[MarkdownBlock], maps: &[BlockOrigins]) -> Vec<bool> {
        let mut drawn = vec![false; src.len()];
        for piece in pieces(blocks) {
            let map = &maps[piece.at.block].pieces[piece.at.piece];
            for offset in 0..map.len() {
                if let Some(Origin::File(source)) = map.origin_of(offset) {
                    drawn[source] = true;
                }
            }
        }
        drawn
    }

    /// The bytes of a piece the file does not spell, named one by one — the
    /// module doc's list, as a predicate a test can apply.
    ///
    /// It is written over the *drawn* text rather than over the parser's
    /// internals on purpose: what it holds is that no byte is synthesised except
    /// for a reason on the list.
    fn is_declared_synthesis(
        block: &MarkdownBlock,
        piece: &Piece,
        map: &TextOrigin,
        offset: usize,
    ) -> bool {
        let byte = piece.text.as_bytes()[offset];
        match block {
            // ③ the spaces a tab expands to.
            MarkdownBlock::Code { .. } => byte == b' ',
            // ④ the break between two body lines, ⑤ the `$$` the piece re-spells
            // around them.
            MarkdownBlock::Math { .. } => byte == b'\n' || byte == b'$',
            // ⑥ a picture the file wrote as HTML, which is the whole piece or
            // none of it: a markdown picture is copied out of the file down to
            // its brackets — save for ① again, an alt text whose own words were
            // wrapped across two source lines.
            MarkdownBlock::Image(_) => {
                byte == b' '
                    || (0..map.len())
                        .all(|offset| matches!(map.origin_of(offset), Some(Origin::Drawn)))
            }
            // ① and ② the space two joined source lines are joined on.
            _ => byte == b' ',
        }
    }

    /// **RED GATE** — every byte of every piece is a copy of a source byte
    /// holding the same character, or is one of the six the module declares.
    ///
    /// MUTATION: hand a paragraph's joined text straight to the file offsets of
    /// its first line and every fixture that wraps comes back naming the wrong
    /// characters.
    #[test]
    fn every_drawn_byte_is_a_copy_of_the_byte_it_says_it_is() {
        for (name, src) in fixtures() {
            let (blocks, _, maps) = parse_markdown_mapped(&src);
            let all = pieces(&blocks);
            for piece in &all {
                let map = &maps[piece.at.block].pieces[piece.at.piece];
                assert_eq!(
                    map.len(),
                    piece.text.len(),
                    "{name}: the map for {:?} is not the length of what it draws",
                    piece.at
                );
                for offset in 0..piece.text.len() {
                    match map.origin_of(offset) {
                        Some(Origin::File(source)) => assert_eq!(
                            src.as_bytes()[source],
                            piece.text.as_bytes()[offset],
                            "{name}: {:?}+{offset} says it copied byte {source}, which is another character",
                            piece.at
                        ),
                        Some(Origin::Drawn) => assert!(
                            is_declared_synthesis(&blocks[piece.at.block], piece, map, offset),
                            "{name}: {:?}+{offset} draws {:?}, which is on nobody's list — the piece is {:?} of {:?}",
                            piece.at,
                            piece.text.as_bytes()[offset] as char,
                            &piece.text[..piece.text.len().min(120)],
                            blocks[piece.at.block]
                        ),
                        None => panic!("{name}: {:?}+{offset} has no answer at all", piece.at),
                    }
                }
            }
        }
    }

    /// The other direction: a source byte that was drawn is drawn once, and the
    /// piece byte it reached holds it.
    #[test]
    fn every_drawn_source_byte_comes_back_to_the_byte_that_drew_it() {
        for (name, src) in fixtures() {
            let (blocks, ranges, maps) = parse_markdown_mapped(&src);
            let all = pieces(&blocks);
            let mut drawn_at: Vec<Option<Place>> = vec![None; src.len()];
            for piece in &all {
                let map = &maps[piece.at.block].pieces[piece.at.piece];
                for offset in 0..piece.text.len() {
                    let Some(Origin::File(source)) = map.origin_of(offset) else {
                        continue;
                    };
                    assert!(
                        drawn_at[source].is_none(),
                        "{name}: byte {source} is drawn twice, at {:?} and at {:?}",
                        drawn_at[source],
                        piece.at
                    );
                    drawn_at[source] = Some(Place::new(piece.at.block, piece.at.piece, offset));
                    assert_eq!(
                        map.text_of(source),
                        Some(offset),
                        "{name}: byte {source} does not come back to the byte that drew it"
                    );
                    assert_eq!(
                        place_of(source, &blocks, &ranges, &maps),
                        Some(Place::new(piece.at.block, piece.at.piece, offset)),
                        "{name}: byte {source} does not come back to the place that drew it"
                    );
                }
            }
        }
    }

    /// **RED GATE** — the two directions are inverse on every drawn byte, and at
    /// the end of a piece they differ only by bytes the page does not draw.
    ///
    /// MUTATION: round a caret at the end of a run *backwards* — to the last
    /// byte copied rather than one past it — and every `**bold**` in every
    /// fixture comes back inside its own closing delimiters.
    #[test]
    fn the_two_directions_are_inverse_on_every_drawn_byte() {
        for (name, src) in fixtures() {
            let (blocks, ranges, maps) = parse_markdown_mapped(&src);
            let drawn = drawn_bytes(&src, &blocks, &maps);
            for piece in pieces(&blocks) {
                let map = &maps[piece.at.block].pieces[piece.at.piece];
                // A place at a byte the page draws and the file does not spell
                // names no file byte to come back from — it rounds, which is
                // what the next test holds it to. The end of a piece is a place
                // like any other and rounds the same way, so it is asked here
                // only when the byte in front of it was a copy.
                let copied = |offset: usize| matches!(map.origin_of(offset), Some(Origin::File(_)));
                for offset in 0..=piece.text.len() {
                    let asked = match offset {
                        // A piece with nothing in it — an empty table cell —
                        // has no byte to be asked about at all.
                        0 => copied(0),
                        _ if offset == piece.text.len() => copied(offset - 1),
                        _ => copied(offset),
                    };
                    if !asked {
                        continue;
                    }
                    let place = Place::new(piece.at.block, piece.at.piece, offset);
                    let file = file_offset_of(&place, &blocks, &ranges, &maps)
                        .unwrap_or_else(|| panic!("{name}: {place:?} answers no file byte"));
                    let back = place_of(file, &blocks, &ranges, &maps)
                        .unwrap_or_else(|| panic!("{name}: {file} lands nowhere"));
                    if back == place {
                        continue;
                    }
                    // **The end of a piece is a position rather than a byte, and
                    // a position at a seam has two names.** The prose before a
                    // picture ends where the picture's own piece begins; the end
                    // of a picture inside a link stands in front of the link's
                    // own `](…)`, which nobody draws. A byte belongs to the
                    // piece that draws it, so that is the name `place_of` gives
                    // it — and what has to hold is that the two names have
                    // nothing *drawn* between them.
                    assert_eq!(
                        offset,
                        piece.text.len(),
                        "{name}: {place:?} → {file} came back as {back:?} from inside a piece"
                    );
                    let again = file_offset_of(&back, &blocks, &ranges, &maps)
                        .unwrap_or_else(|| panic!("{name}: {back:?} answers no file byte"));
                    assert!(
                        drawn[file.min(again)..file.max(again)]
                            .iter()
                            .all(|drawn| !*drawn),
                        "{name}: {place:?} → {file} came back as {back:?} at {again}, and \
                         {:?} between them is drawn",
                        &src[file.min(again)..file.max(again)]
                    );
                }
            }
        }
    }

    /// And the other way round: a file byte lands somewhere, and what that
    /// somewhere answers is the byte itself or the nearest drawn one.
    #[test]
    fn a_file_byte_lands_on_the_nearest_drawn_place() {
        for (name, src) in fixtures() {
            let (blocks, ranges, maps) = parse_markdown_mapped(&src);
            if blocks.is_empty() {
                continue;
            }
            for offset in 0..src.len() {
                let Some(place) = place_of(offset, &blocks, &ranges, &maps) else {
                    panic!("{name}: byte {offset} lands nowhere");
                };
                let back = file_offset_of(&place, &blocks, &ranges, &maps)
                    .unwrap_or_else(|| panic!("{name}: {place:?} answers no file byte"));
                let block = &ranges[place.block];
                assert!(
                    block.start <= back && back <= block.end,
                    "{name}: byte {offset} came back as {back}, outside {block:?}"
                );
            }
        }
    }

    /// The undrawn-mark rule, spelled out on the case the ticket names: a caret
    /// at the edge of an emphasis, a code span or a link resolves outside the
    /// mark, never inside it.
    #[test]
    fn a_caret_at_the_edge_of_a_mark_resolves_outside_it() {
        for (src, text, edges) in [
            // `a **bold** b`: after `bold` is after the closing `**`.
            (
                "a **bold** b\n",
                "a bold b",
                vec![(2usize, 4usize), (6, 10)],
            ),
            // A code span: after `code` is after the closing backtick.
            ("a `code` b\n", "a code b", vec![(2, 3), (6, 8)]),
            // A link: the label's ends are the label's, and the space after it
            // is the space the file has *after the target*, not inside it.
            ("a [word](u) b\n", "a word b", vec![(2, 3), (6, 11)]),
        ] {
            let (blocks, ranges, maps) = parse_markdown_mapped(src);
            let all = pieces(&blocks);
            assert_eq!(all[0].text, text, "{src:?}: the page draws this");
            for (offset, expected) in edges {
                let place = Place::new(0, 0, offset);
                assert_eq!(
                    file_offset_of(&place, &blocks, &ranges, &maps),
                    Some(expected),
                    "{src:?}: the caret at {offset} does not stand at {expected}"
                );
            }
        }
    }

    /// And a source byte *inside* a mark rounds to the nearest drawn position
    /// rather than answering nothing.
    #[test]
    fn a_source_byte_inside_a_mark_rounds_to_a_drawn_place() {
        let src = "a **bold** b\n";
        let (blocks, ranges, maps) = parse_markdown_mapped(src);
        for (offset, expected) in [
            // Inside the opening `**`: the start of the word.
            (2usize, Place::new(0, 0, 2usize)),
            (3, Place::new(0, 0, 2)),
            // Inside the closing `**`: the end of the word.
            (8, Place::new(0, 0, 6)),
            (9, Place::new(0, 0, 6)),
        ] {
            assert_eq!(
                place_of(offset, &blocks, &ranges, &maps),
                Some(expected),
                "byte {offset} of {src:?} does not round to {expected:?}"
            );
        }
    }

    /// The numbering is `preview_select`'s and the parser agrees with it — one
    /// map per piece, in the same order, for every block of every fixture.
    #[test]
    fn there_is_one_map_for_every_piece_the_page_reads() {
        for (name, src) in fixtures() {
            let (blocks, ranges, maps) = parse_markdown_mapped(&src);
            assert_eq!(blocks.len(), maps.len(), "{name}: one map per block");
            assert_eq!(blocks.len(), ranges.len(), "{name}: one range per block");
            let mut counted = vec![0usize; blocks.len()];
            for piece in pieces(&blocks) {
                counted[piece.at.block] += 1;
            }
            for (block, count) in counted.into_iter().enumerate() {
                assert_eq!(
                    maps[block].pieces.len(),
                    count,
                    "{name}: block {block} has {count} pieces and {} maps",
                    maps[block].pieces.len()
                );
            }
        }
    }

    /// **Copy is untouched** — the clipboard reads the same bytes it read before
    /// this ticket, because the reverse mapping rides beside the pieces and not
    /// in them (§7.31 ⑥, copy what you read).
    #[test]
    fn what_copies_is_what_copied_before() {
        for (name, src) in fixtures() {
            let blocks = crate::preview::parse_markdown(&src);
            let all = pieces(&blocks);
            let (mapped, _, _) = parse_markdown_mapped(&src);
            assert_eq!(blocks, mapped, "{name}: the mapped walk found other blocks");
            let Some(first) = all.first() else {
                continue;
            };
            let last = all.last().expect("there is a first, so there is a last");
            let end = Place::new(last.at.block, last.at.piece, last.text.len());
            assert_eq!(
                copy_text(&all, first.at, end),
                copy_text(&pieces(&mapped), first.at, end),
                "{name}: the clipboard changed"
            );
        }
    }
}
