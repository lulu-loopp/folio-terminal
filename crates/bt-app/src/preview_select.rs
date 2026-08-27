//! **Selecting the words of a rendered document** (user report 2026-08-28:
//! 「渲染后的 md 文字无法选中」).
//!
//! # Why a second selection model
//!
//! This window already has two. The terminal's is a pair of
//! `bt_doc::ContentAnchor`s over a grid of cells, and the preview's *source*
//! face has [`crate::preview_edit::EditCaret`], a pair of byte offsets into one
//! flat string. A rendered markdown page is neither: it is a tree of blocks, some
//! of which hold several runs of text apiece (a list's items, a quote's lines, a
//! table's cells, a fence's lines), set in a proportional face that reflows to
//! the pane. There is no grid to count and there is no one string to index.
//!
//! So the unit here is the **piece** — one run of a document's text that is set
//! as one paragraph — and a place is which piece and how far into it
//! ([`Place`]). Two things follow, and both are the reason for the choice:
//!
//! * **A place survives a scroll.** Only the blocks a pane can currently show
//!   are laid out ([`crate::build_preview_markdown_body`] skips the rest), so the
//!   paragraphs of a `PreviewBody` are renumbered by every wheel notch. A
//!   selection anchored to one of them would slide up the document as the reader
//!   scrolled away from it.
//! * **Document order is the type's**, because [`Place`] is `Ord` on
//!   `(block, piece, offset)` in that order, which is the order the page is read
//!   in. "Which end of this drag is the start" and "is this piece inside the
//!   selection" are then comparisons rather than arithmetic.
//!
//! # What copies
//!
//! What is put on the clipboard is **what was read**, not what was written
//! (user ruling): a heading copies without its hashes, a quote without its `>`,
//! a bold word without its asterisks. The two places the render is *not* what a
//! reader would want back are the two where the document's own mark carries the
//! meaning — a list item's bullet, which is drawn as `•` and copies as `- `, and
//! a formula, which is drawn as a picture and copies as its LaTeX. Both are
//! [`Piece`]'s to state.

use std::ops::Range;

use bt_viewport::horizontal::cluster_word_class;

use crate::preview::{MarkdownBlock, Span};

/// **Where one end of a selection stands in a rendered document.**
///
/// `Ord` is derived and the field order is load-bearing: it is document order,
/// so `min`/`max` over a pair of these is "which of the two comes first on the
/// page" with nothing else written.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Place {
    /// Index into the document's own `[MarkdownBlock]`.
    pub block: usize,
    /// Which piece of that block — see [`pieces`] for the numbering, which is
    /// this module's to define and the layout's to agree with.
    pub piece: usize,
    /// Byte offset into that piece's [`Piece::text`].
    pub offset: usize,
}

impl Place {
    #[must_use]
    pub fn new(block: usize, piece: usize, offset: usize) -> Self {
        Self {
            block,
            piece,
            offset,
        }
    }

    /// The same piece, at its beginning — what a whole-piece comparison asks.
    #[must_use]
    fn piece_start(self) -> Self {
        Self { offset: 0, ..self }
    }
}

/// **How much of the document one gesture takes at a time** — the terminal's
/// `SelectionDragMode`, in this surface's units.
///
/// A property of the drag and not of the selection it leaves behind: once the
/// button is up all that remains is two places, exactly as it is next door.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Grain {
    /// One character at a time — a press and a drag.
    #[default]
    Character,
    /// Whole words — a double click, and the drag that continues from it.
    Word,
    /// Whole pieces — a triple click. A "line" in the terminal's menu and a
    /// *paragraph* here, because a rendered paragraph has no lines of its own:
    /// where it folds is a fact about the pane's width, and a triple click that
    /// took one fold of it would take a different amount of text after a resize.
    Piece,
}

/// **A selection over a rendered document**: where the drag began, where it is
/// now, and how much it takes at a time.
///
/// Unnormalized on purpose, on the terminal's own precedent: `anchor` is where
/// the hand went down and `head` is where it is, so a drag that doubles back
/// through its own origin needs no special case. Every reader asks
/// [`Self::range`] instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub anchor: Place,
    pub head: Place,
    pub grain: Grain,
}

impl Selection {
    /// A selection that has been pressed and not yet dragged.
    #[must_use]
    pub fn collapsed(at: Place, grain: Grain) -> Self {
        Self {
            anchor: at,
            head: at,
            grain,
        }
    }

    /// **The two ends in document order, already grown to the drag's grain.**
    ///
    /// The grain is applied here rather than at the press for one reason: a
    /// double click that then drags backwards must keep the *whole* of the word
    /// it started on, and an anchor grown once at the press would have grown
    /// towards the wrong side. Both ends are grown against the piece they are in
    /// on every read, so the answer is right whichever way the hand went.
    #[must_use]
    pub fn range(&self, pieces: &[Piece]) -> (Place, Place) {
        let (start, end) = self.ordered();
        (
            grow(piece_at(pieces, start), start, self.grain, Side::Start),
            grow(piece_at(pieces, end), end, self.grain, Side::End),
        )
    }

    /// **[`Self::range`] for a caller holding the document rather than a built
    /// list of its pieces** — which is the painter, on every frame.
    ///
    /// The same answer by the same rules, and it exists because [`pieces`] over
    /// a 64KB document allocates every string in it: paying that sixty times a
    /// second to grow two offsets by a word would be asking the whole page a
    /// question about two of its sentences. Only the two blocks the ends stand
    /// in are built.
    #[must_use]
    pub fn range_in(&self, blocks: &[MarkdownBlock]) -> (Place, Place) {
        let (start, end) = self.ordered();
        (
            grow(
                piece_in(blocks, start).as_ref(),
                start,
                self.grain,
                Side::Start,
            ),
            grow(piece_in(blocks, end).as_ref(), end, self.grain, Side::End),
        )
    }

    /// The two ends in document order, before either has grown.
    #[must_use]
    fn ordered(&self) -> (Place, Place) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// Which end of a range a place is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Start,
    End,
}

/// Grow one end of a selection out to its grain's own boundary.
fn grow(piece: Option<&Piece>, place: Place, grain: Grain, side: Side) -> Place {
    let Some(piece) = piece else {
        return place;
    };
    // An atom is all or nothing whatever the grain: a formula picked up by one
    // corner comes whole, because half of `\frac{a}{b}` is not half a formula.
    if piece.atomic {
        return match side {
            Side::Start => place.piece_start(),
            Side::End => Place {
                offset: piece.text.len(),
                ..place
            },
        };
    }
    let offset = match (grain, side) {
        (Grain::Character, _) => place.offset,
        (Grain::Word, Side::Start) => word_start(&piece.text, place.offset),
        (Grain::Word, Side::End) => word_end(&piece.text, place.offset),
        (Grain::Piece, Side::Start) => 0,
        (Grain::Piece, Side::End) => piece.text.len(),
    };
    Place { offset, ..place }
}

/// The piece a place stands in, if the document still has one there.
#[must_use]
pub fn piece_at(pieces: &[Piece], place: Place) -> Option<&Piece> {
    pieces
        .iter()
        .find(|piece| piece.at.block == place.block && piece.at.piece == place.piece)
}

/// **What separates a piece from the one before it**, when both are copied.
///
/// The three answers a rendered document has, and the reason they are a type
/// rather than a `&str` on the piece: the separator is only written *between*
/// two pieces that are both in the selection, so it cannot be part of either
/// one's text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lead {
    /// A new block — a blank line, which is what a paragraph break is in plain
    /// text and what every editor this text is going to will read back as one.
    Block,
    /// Another row of the same block: a list's next item, a quote's next line,
    /// a fence's next line, a table's next row.
    Line,
    /// The next cell of the same table row.
    ///
    /// A tab, because a tab is what one column of anything pastes into: a
    /// spreadsheet, a `.tsv`, and the terminal's own `column -t`.
    Cell,
}

impl Lead {
    #[must_use]
    fn text(self) -> &'static str {
        match self {
            Self::Block => "\n\n",
            Self::Line => "\n",
            Self::Cell => "\t",
        }
    }
}

/// **One run of a document's text that a selection can stand inside.**
///
/// One per paragraph the page sets, and the layout numbers them exactly as
/// [`pieces`] does — see [`Place::piece`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Piece {
    /// This piece's own beginning; [`Place::offset`] is always zero.
    pub at: Place,
    pub lead: Lead,
    /// Written in front of the text when the selection reaches this piece's
    /// **beginning**, and left off when it does not.
    ///
    /// A list item's marker and nothing else. It is not part of [`Self::text`]
    /// because it is not text a pointer can stand in — a browser will not let
    /// you select a bullet either, and for the same reason: the mark is the
    /// list's, not the item's. Conditioned on reaching the beginning because
    /// that is what a reader means: a drag that starts in the middle of the
    /// third item is asking for those words, not for a bullet in front of them.
    pub prefix: String,
    /// What is read here, exactly as it is set on the glass.
    pub text: String,
    /// This piece cannot be split — any touch takes all of it.
    ///
    /// A display formula, whose picture is one thing on the page and whose
    /// source is one thing in the file; there is no offset into it that means
    /// anything to either.
    pub atomic: bool,
}

/// **Every piece of a rendered document, in the order the page is read in.**
///
/// This function is the authority on the numbering, and the layout agrees with
/// it by walking the same lists in the same order. It is not called by the
/// layout — a document of a thousand blocks would then rebuild every string in
/// it on every frame, to hand back indices the layout was already counting — so
/// the agreement is pinned by a test rather than by a shared call.
#[must_use]
pub fn pieces(blocks: &[MarkdownBlock]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        block_pieces(block, index, &mut pieces);
    }
    pieces
}

/// **The piece one place stands in**, built from the document without building
/// the rest of it — see [`Selection::range_in`].
#[must_use]
pub fn piece_in(blocks: &[MarkdownBlock], place: Place) -> Option<Piece> {
    let mut pieces = Vec::new();
    block_pieces(blocks.get(place.block)?, place.block, &mut pieces);
    pieces
        .into_iter()
        .find(|piece| piece.at.piece == place.piece)
}

/// **One block's pieces, numbered** — the authority the layout agrees with.
fn block_pieces(content: &MarkdownBlock, block: usize, pieces: &mut Vec<Piece>) {
    {
        match content {
            MarkdownBlock::Heading { spans, .. } | MarkdownBlock::Paragraph(spans) => {
                pieces.push(Piece {
                    at: Place::new(block, 0, 0),
                    lead: Lead::Block,
                    prefix: String::new(),
                    text: span_text(spans),
                    atomic: false,
                });
            }
            MarkdownBlock::List { ordered, items } => {
                for (index, spans) in items.iter().enumerate() {
                    pieces.push(Piece {
                        at: Place::new(block, index, 0),
                        lead: row_lead(index),
                        prefix: list_prefix(*ordered, index),
                        text: span_text(spans),
                        atomic: false,
                    });
                }
            }
            MarkdownBlock::Quote(lines) => {
                for (index, spans) in lines.iter().enumerate() {
                    pieces.push(Piece {
                        at: Place::new(block, index, 0),
                        lead: row_lead(index),
                        prefix: String::new(),
                        // No `>`: the page does not draw one, and what copies is
                        // what was read. The bar down the left is the quote's
                        // mark, and a bar does not paste.
                        text: span_text(spans),
                        atomic: false,
                    });
                }
            }
            MarkdownBlock::Code { text, .. } => {
                for (index, line) in text.lines().enumerate() {
                    pieces.push(Piece {
                        at: Place::new(block, index, 0),
                        lead: row_lead(index),
                        prefix: String::new(),
                        // The tabs are already spent: a fence is drawn with them
                        // expanded (`tab-size: 4`), and the offsets a pointer
                        // answers in are offsets into what was drawn.
                        text: crate::preview::expand_tabs(line),
                        atomic: false,
                    });
                }
            }
            MarkdownBlock::Table { rows, .. } => {
                let mut index = 0usize;
                for (row, cells) in rows.iter().enumerate() {
                    for (column, cell) in cells.iter().enumerate() {
                        pieces.push(Piece {
                            at: Place::new(block, index, 0),
                            lead: match (row, column) {
                                (0, 0) => Lead::Block,
                                (_, 0) => Lead::Line,
                                _ => Lead::Cell,
                            },
                            prefix: String::new(),
                            text: span_text(cell),
                            atomic: false,
                        });
                        index += 1;
                    }
                }
            }
            // **One piece, indivisible, and its source is what it copies.** The
            // delimiters are put back on here because the block dropped them
            // (see `MarkdownBlock::Math`): what is on the page is a picture, and
            // the only honest plain text for a picture of an equation is the
            // equation, marked as one.
            MarkdownBlock::Math { source } => {
                pieces.push(Piece {
                    at: Place::new(block, 0, 0),
                    lead: Lead::Block,
                    prefix: String::new(),
                    text: format!("$${source}$$"),
                    atomic: true,
                });
            }
            // A rule is a line on the page and no words at all. Nothing to
            // stand in, and nothing to paste — which is what every browser puts
            // on the clipboard for an `<hr>` dragged through.
            MarkdownBlock::Rule => {}
        }
    }
}

/// A row of a multi-row block: the first opens the block, the rest are lines of
/// it.
fn row_lead(index: usize) -> Lead {
    if index == 0 { Lead::Block } else { Lead::Line }
}

/// What a list item copies in front of its words.
///
/// `- ` and `1. ` — the document's own marks rather than the page's `•`, which
/// is the one place the render is deliberately not what pastes: a bullet
/// character in a `.md` file is not a list, and a reader copying a list out of a
/// rendered page is copying markdown.
fn list_prefix(ordered: Option<u64>, index: usize) -> String {
    match ordered {
        Some(first) => format!("{}. ", first.saturating_add(index as u64)),
        None => "- ".to_owned(),
    }
}

/// One paragraph's spans as the string a reader sees.
fn span_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// **The plain text of everything between two places.**
///
/// Walks the document rather than the glass, which is what makes a selection
/// running off the bottom of the pane copy the blocks it reached rather than the
/// blocks that happened to be drawn.
#[must_use]
pub fn copy_text(pieces: &[Piece], start: Place, end: Place) -> String {
    let mut out = String::new();
    let mut written = false;
    for piece in pieces {
        let Some(range) = piece_range(piece, start, end) else {
            continue;
        };
        if range.start >= range.end {
            continue;
        }
        if written {
            out.push_str(piece.lead.text());
        }
        if range.start == 0 {
            out.push_str(&piece.prefix);
        }
        out.push_str(&piece.text[range]);
        written = true;
    }
    out
}

/// **How much of one piece a selection covers**, in that piece's own bytes.
///
/// `None` for a piece outside the selection entirely. An atom is all of itself
/// or none of itself, which is [`Piece::atomic`]'s whole contract.
#[must_use]
pub fn piece_range(piece: &Piece, start: Place, end: Place) -> Option<Range<usize>> {
    let at = piece.at.piece_start();
    if at < start.piece_start() || at > end.piece_start() {
        return None;
    }
    if piece.atomic {
        return Some(0..piece.text.len());
    }
    let from = if at == start.piece_start() {
        start.offset.min(piece.text.len())
    } else {
        0
    };
    let to = if at == end.piece_start() {
        end.offset.min(piece.text.len())
    } else {
        piece.text.len()
    };
    Some(from..to)
}

/// Every piece of the document, from its first byte to its last — `Ctrl+A`.
///
/// `None` for a document with no text in it at all, which is a selection there
/// is nothing to make.
#[must_use]
pub fn select_all(pieces: &[Piece]) -> Option<Selection> {
    let first = pieces.first()?;
    let last = pieces.last()?;
    Some(Selection {
        anchor: first.at,
        head: Place {
            offset: last.text.len(),
            ..last.at
        },
        grain: Grain::Character,
    })
}

/// **Where the word around `offset` begins.**
///
/// The terminal's own three-way classification
/// ([`bt_viewport::horizontal::cluster_word_class`]) walked over grapheme
/// clusters instead of cells, so a double click in a rendered paragraph takes
/// exactly what a double click in the pane beside it takes. One classifier, so
/// the two surfaces cannot come to disagree about what a word is.
#[must_use]
pub fn word_start(text: &str, offset: usize) -> usize {
    let clusters = clusters(text);
    // The cluster the offset stands *on* is the one being classified; standing
    // at the very end of the text there is none, so the one before it is.
    let Some(at) = cluster_index(&clusters, text, offset) else {
        return offset.min(text.len());
    };
    let class = clusters[at].1;
    let mut start = at;
    while start > 0 && clusters[start - 1].1 == class {
        start -= 1;
    }
    clusters[start].0
}

/// **Where the word around `offset` ends** — [`word_start`]'s other side.
#[must_use]
pub fn word_end(text: &str, offset: usize) -> usize {
    let clusters = clusters(text);
    let Some(at) = cluster_index(&clusters, text, offset) else {
        return offset.min(text.len());
    };
    let class = clusters[at].1;
    let mut end = at;
    while end + 1 < clusters.len() && clusters[end + 1].1 == class {
        end += 1;
    }
    clusters
        .get(end + 1)
        .map_or(text.len(), |(next, _)| *next)
        .max(offset.min(text.len()))
}

/// Every grapheme cluster of a piece: where it starts, and what kind of thing
/// it is.
///
/// Built once per question rather than walked backwards a cluster at a time,
/// because the segmenter this window exposes runs forwards only — and a
/// backwards walk over it would re-segment the whole string at every step.
fn clusters(text: &str) -> Vec<(usize, WordClass)> {
    let mut at = 0usize;
    bt_unicode::graphemes(text)
        .map(|cluster| {
            let start = at;
            at += cluster.len();
            (start, cluster_word_class(cluster))
        })
        .collect()
}

/// Which cluster an offset stands on — the one it begins, or the one it ends.
fn cluster_index(clusters: &[(usize, WordClass)], text: &str, offset: usize) -> Option<usize> {
    if clusters.is_empty() {
        return None;
    }
    if offset >= text.len() {
        return Some(clusters.len() - 1);
    }
    // The last cluster that starts at or before the offset, which is the one the
    // offset is inside even when it fell in the middle of a cluster.
    clusters
        .iter()
        .rposition(|(start, _)| *start <= offset)
        .or(Some(0))
}

type WordClass = bt_viewport::horizontal::WordClass;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::Span;

    fn paragraph(text: &str) -> MarkdownBlock {
        MarkdownBlock::Paragraph(vec![Span::plain(text)])
    }

    fn table(rows: &[&[&str]]) -> MarkdownBlock {
        MarkdownBlock::Table {
            rows: rows
                .iter()
                .map(|row| row.iter().map(|cell| vec![Span::plain(cell)]).collect())
                .collect(),
            alignments: Vec::new(),
        }
    }

    /// The whole document, end to end.
    fn whole(pieces: &[Piece]) -> String {
        let Some(selection) = select_all(pieces) else {
            return String::new();
        };
        let (start, end) = selection.range(pieces);
        copy_text(pieces, start, end)
    }

    #[test]
    fn a_place_sorts_in_the_order_the_page_is_read_in() {
        let mut places = vec![
            Place::new(1, 0, 3),
            Place::new(0, 2, 0),
            Place::new(1, 0, 0),
            Place::new(0, 0, 9),
        ];
        places.sort_unstable();
        assert_eq!(
            places,
            vec![
                Place::new(0, 0, 9),
                Place::new(0, 2, 0),
                Place::new(1, 0, 0),
                Place::new(1, 0, 3),
            ]
        );
    }

    #[test]
    fn a_drag_that_ran_backwards_reads_the_same_way_round() {
        let blocks = vec![paragraph("alpha"), paragraph("beta")];
        let pieces = pieces(&blocks);
        let forward = Selection {
            anchor: Place::new(0, 0, 1),
            head: Place::new(1, 0, 2),
            grain: Grain::Character,
        };
        let backward = Selection {
            anchor: Place::new(1, 0, 2),
            head: Place::new(0, 0, 1),
            grain: Grain::Character,
        };
        assert_eq!(forward.range(&pieces), backward.range(&pieces));
        let (start, end) = forward.range(&pieces);
        assert_eq!(copy_text(&pieces, start, end), "lpha\n\nbe");
    }

    #[test]
    fn blocks_are_parted_by_a_blank_line_and_rows_by_one() {
        let blocks = vec![
            MarkdownBlock::Heading {
                level: 2,
                spans: vec![Span::plain("Title")],
            },
            paragraph("Body."),
            MarkdownBlock::Quote(vec![
                vec![Span::plain("first")],
                vec![Span::plain("second")],
            ]),
        ];
        let pieces = pieces(&blocks);
        assert_eq!(whole(&pieces), "Title\n\nBody.\n\nfirst\nsecond");
    }

    #[test]
    fn a_table_copies_its_cells_by_tab_and_its_rows_by_break() {
        let blocks = vec![table(&[&["a", "b"], &["1", "2"]])];
        let pieces = pieces(&blocks);
        assert_eq!(whole(&pieces), "a\tb\n1\t2");
    }

    #[test]
    fn a_drag_across_a_table_and_the_prose_under_it_keeps_document_order() {
        let blocks = vec![
            table(&[&["name", "size"], &["a.txt", "12"]]),
            paragraph("Everything above is a table."),
        ];
        let pieces = pieces(&blocks);
        let selection = Selection {
            anchor: Place::new(0, 1, 0),
            head: Place::new(1, 0, 10),
            grain: Grain::Character,
        };
        let (start, end) = selection.range(&pieces);
        assert_eq!(
            copy_text(&pieces, start, end),
            "size\na.txt\t12\n\nEverything"
        );
    }

    #[test]
    fn a_list_item_carries_its_own_mark_and_only_from_its_first_byte() {
        let blocks = vec![MarkdownBlock::List {
            ordered: None,
            items: vec![vec![Span::plain("one")], vec![Span::plain("two")]],
        }];
        let pieces = pieces(&blocks);
        assert_eq!(whole(&pieces), "- one\n- two");
        let (start, end) = (Place::new(0, 0, 1), Place::new(0, 1, 3));
        assert_eq!(copy_text(&pieces, start, end), "ne\n- two");
    }

    #[test]
    fn a_numbered_list_counts_from_the_number_the_document_wrote() {
        let blocks = vec![MarkdownBlock::List {
            ordered: Some(3),
            items: vec![vec![Span::plain("three")], vec![Span::plain("four")]],
        }];
        assert_eq!(whole(&pieces(&blocks)), "3. three\n4. four");
    }

    #[test]
    fn a_heading_copies_without_its_hashes_and_a_quote_without_its_arrow() {
        let blocks = vec![
            MarkdownBlock::Heading {
                level: 3,
                spans: vec![Span::plain("Design")],
            },
            MarkdownBlock::Quote(vec![vec![Span::plain("quoted")]]),
        ];
        assert_eq!(whole(&pieces(&blocks)), "Design\n\nquoted");
    }

    #[test]
    fn a_formula_is_an_atom_and_copies_as_its_own_source() {
        let blocks = vec![
            paragraph("before"),
            MarkdownBlock::Math {
                source: "a^2 + b^2".to_owned(),
            },
            paragraph("after"),
        ];
        let pieces = pieces(&blocks);
        // One byte of the formula picked up takes the whole of it.
        let selection = Selection {
            anchor: Place::new(1, 0, 4),
            head: Place::new(1, 0, 5),
            grain: Grain::Character,
        };
        let (start, end) = selection.range(&pieces);
        assert_eq!(copy_text(&pieces, start, end), "$$a^2 + b^2$$");
    }

    #[test]
    fn a_rule_is_a_line_and_pastes_as_nothing() {
        let blocks = vec![paragraph("above"), MarkdownBlock::Rule, paragraph("below")];
        assert_eq!(whole(&pieces(&blocks)), "above\n\nbelow");
    }

    #[test]
    fn a_fence_copies_line_by_line_with_its_tabs_already_spent() {
        let blocks = vec![MarkdownBlock::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {\n\tprintln!();\n}".to_owned(),
        }];
        assert_eq!(whole(&pieces(&blocks)), "fn main() {\n    println!();\n}");
    }

    #[test]
    fn a_double_click_takes_the_word_it_landed_in_whichever_way_the_drag_goes() {
        let blocks = vec![paragraph("alpha beta gamma")];
        let pieces = pieces(&blocks);
        let forward = Selection {
            anchor: Place::new(0, 0, 7),
            head: Place::new(0, 0, 8),
            grain: Grain::Word,
        };
        let (start, end) = forward.range(&pieces);
        assert_eq!(copy_text(&pieces, start, end), "beta");
        // Dragged back past its own origin, the word it started on is still
        // whole — the anchor grows towards the end, not towards the head.
        let backward = Selection {
            anchor: Place::new(0, 0, 8),
            head: Place::new(0, 0, 1),
            grain: Grain::Word,
        };
        let (start, end) = backward.range(&pieces);
        assert_eq!(copy_text(&pieces, start, end), "alpha beta");
    }

    #[test]
    fn a_triple_click_takes_the_whole_piece_and_not_one_fold_of_it() {
        let blocks = vec![paragraph("a paragraph long enough to fold somewhere")];
        let pieces = pieces(&blocks);
        let selection = Selection::collapsed(Place::new(0, 0, 12), Grain::Piece);
        let (start, end) = selection.range(&pieces);
        assert_eq!(
            copy_text(&pieces, start, end),
            "a paragraph long enough to fold somewhere"
        );
    }

    #[test]
    fn a_word_stops_at_the_punctuation_the_terminal_stops_at() {
        let text = "call(arg);";
        assert_eq!(&text[word_start(text, 6)..word_end(text, 6)], "arg");
        assert_eq!(&text[word_start(text, 4)..word_end(text, 4)], "(");
    }

    #[test]
    fn a_click_that_never_travelled_selects_nothing() {
        let blocks = vec![paragraph("alpha")];
        let pieces = pieces(&blocks);
        let selection = Selection::collapsed(Place::new(0, 0, 2), Grain::Character);
        let (start, end) = selection.range(&pieces);
        assert_eq!(start, end, "two equal places take nothing");
        assert_eq!(copy_text(&pieces, start, end), "");
    }

    #[test]
    fn select_all_reaches_the_last_byte_of_the_last_block() {
        let blocks = vec![paragraph("first"), table(&[&["x"]]), paragraph("last")];
        let pieces = pieces(&blocks);
        let selection = select_all(&pieces).expect("a document with text");
        assert_ne!(selection.anchor, selection.head);
        assert_eq!(whole(&pieces), "first\n\nx\n\nlast");
    }

    #[test]
    fn a_document_with_nothing_in_it_has_nothing_to_select() {
        assert!(select_all(&pieces(&[MarkdownBlock::Rule])).is_none());
    }
}
