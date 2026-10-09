//! **`preview_select`, as the application drives it.** Tests whose first assertion is about
//! `preview_select`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::row_box;

/// **A drag through a table and the paragraph under it bands every piece
/// between them, and only the parts of them it reached.**
///
/// The bands are the shaper's, so what is under test here is the arithmetic
/// that decides *which* bytes of each piece to ask about — the first piece
/// from the offset the drag began at, the last up to where it is now, and
/// every piece in between whole.
///
/// MUTATION: clamp a middle piece to the head's offset instead of its own
/// length and the table's second row loses its tail.
#[test]
fn a_drag_bands_the_first_piece_from_its_offset_and_the_ones_between_whole() {
    let boxes = vec![
        text_box(preview_select::Place::new(0, 0, 0), 5, "alpha"),
        text_box(preview_select::Place::new(0, 1, 0), 4, "beta"),
        text_box(preview_select::Place::new(1, 0, 0), 5, "gamma"),
    ];
    let mut asked: Vec<(String, Range<usize>)> = Vec::new();
    let bands = preview_selection_bands(
        &boxes,
        preview_select::Place::new(0, 0, 2),
        preview_select::Place::new(1, 0, 3),
        &mut |paragraph, range| {
            asked.push((bt_render::preview_paragraph_text(paragraph), range.clone()));
            vec![[range.start as f32, 0.0, range.end as f32, 1.0]]
        },
    );
    assert_eq!(
        asked,
        vec![
            ("alpha".to_owned(), 2..5),
            ("beta".to_owned(), 0..4),
            ("gamma".to_owned(), 0..3),
        ],
        "the head's piece from where the hand went down, the tail's up to \
             where it is, and everything between whole",
    );
    assert_eq!(bands.len(), 3);
}

/// **A formula is one band whatever of it was touched** (the atom rule).
///
/// MUTATION: drop the `atomic` arm of `PreviewTextPiece::shaped_range` and a
/// drag that clips a formula's corner bands two characters of a picture.
#[test]
fn a_formula_is_banded_whole_however_little_of_it_was_touched() {
    let mut formula = text_box(preview_select::Place::new(1, 0, 0), 13, "$$a+b$$");
    formula.piece.atomic = true;
    let boxes = vec![
        text_box(preview_select::Place::new(0, 0, 0), 5, "alpha"),
        formula,
    ];
    let mut asked: Vec<Range<usize>> = Vec::new();
    preview_selection_bands(
        &boxes,
        preview_select::Place::new(0, 0, 5),
        preview_select::Place::new(1, 0, 1),
        &mut |_, range| {
            asked.push(range);
            Vec::new()
        },
    );
    assert_eq!(
        asked,
        vec![0..7],
        "one byte of the formula asked for takes the whole of what is drawn \
             for it, and the piece before it contributed nothing",
    );
}

/// **A picture standing for a formula bands as its own rectangle**, because
/// there is no paragraph under it to ask the shaper about.
#[test]
fn a_rendered_formulas_picture_bands_as_the_box_it_was_drawn_in() {
    let picture = PreviewTextBox {
        piece: PreviewTextPiece {
            at: preview_select::Place::new(0, 0, 0),
            len: 9,
            lead: 0,
            atoms: Vec::new(),
            atomic: true,
        },
        rect: [10.0, 20.0, 130.0, 60.0],
        clip: [0.0, 0.0, 400.0, 400.0],
        paragraph: None,
    };
    let bands = preview_selection_bands(
        &[picture],
        preview_select::Place::new(0, 0, 0),
        preview_select::Place::new(0, 0, 9),
        &mut |_, _| panic!("a picture has no paragraph to shape"),
    );
    assert_eq!(bands, vec![[10.0, 20.0, 130.0, 60.0]]);
}

/// **A band is cropped to the window its piece is seen through**, so a table
/// scrolled sideways inside itself does not paint a highlight over the prose
/// beside it.
#[test]
fn a_band_is_cropped_to_the_window_its_own_block_is_seen_through() {
    let mut cell = text_box(preview_select::Place::new(0, 0, 0), 5, "alpha");
    cell.clip = [100.0, 0.0, 200.0, 50.0];
    let bands = preview_selection_bands(
        &[cell],
        preview_select::Place::new(0, 0, 0),
        preview_select::Place::new(0, 0, 5),
        &mut |_, _| vec![[50.0, 10.0, 150.0, 30.0]],
    );
    assert_eq!(
        bands,
        vec![[100.0, 10.0, 150.0, 30.0]],
        "the half of the band outside the block's window is not drawn",
    );
}

/// **A document that has been re-read leaves no selection standing** (the
/// file changed on disk; the offsets are about text that is gone) — **and a
/// document re-parsed because the reader typed into it leaves both marks
/// exactly where they were** (§7.1.3q; research open question 15, "the
/// single most dangerous line in T4").
///
/// The two halves are one test because the danger is in the *difference*:
/// the clearing rule was written when the only thing that could replace a
/// parse was the disk, and a live preview re-parses on every keystroke. A
/// rule that could not tell the two apart would either highlight somebody
/// else's sentence or drop the reader's own selection every time they typed
/// beside it.
///
/// MUTATION ①: assign `pane.doc` directly at the refresh and a selection made
/// before a save goes on being drawn over whatever replaced it.
/// MUTATION ②: clear on both arms and a selection cannot survive being typed
/// next to; keep on both and an external save keeps a highlight over text
/// that is gone.
#[test]
fn a_freshly_parsed_document_leaves_no_selection_standing() {
    let marked = || PreviewPane {
        md_select: Some(preview_select::Selection::collapsed(
            preview_select::Place::new(3, 1, 4),
            preview_select::Grain::Character,
        )),
        md_text: vec![text_box(
            preview_select::Place::new(3, 1, 0),
            9,
            "somewhere",
        )],
        caret: preview_edit::EditCaret {
            anchor: 12,
            caret: 20,
            desired_column: None,
            desired_x: None,
        },
        ..PreviewPane::default()
    };
    // "new\n\nlines\n" — two paragraphs and the blank line between them,
    // which belongs to neither of them (§7.1.3o).
    let parsed = || PreviewDocument::Markdown {
        blocks: vec![
            preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("new")]),
            preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("lines")]),
        ],
        ranges: vec![0..4, 5..11],
        maps: Vec::new(),
        source: SourceBlocks::default(),
        intrinsic: Vec::new(),
        layout: preview_viewport::Layout::default(),
        math: DocumentMath::default(),
        pictures: DocumentPictures::default(),
        wrap: Arc::default(),
    };

    let mut disk = marked();
    disk.show_document(parsed(), Reparse::Elsewhere);
    assert_eq!(disk.md_select, None, "the selection went with the document");
    assert!(
        disk.md_text.is_empty(),
        "and so did the boxes it was drawn in"
    );
    assert_eq!(
        (disk.caret.anchor, disk.caret.caret),
        (20, 20),
        "and what the caret had dragged over went with it — but not the \
             caret, which is a byte offset the buffer's own doors heal and a \
             session restores before the file it belongs to has even landed",
    );

    let mut ours = marked();
    ours.show_document(parsed(), Reparse::Ours);
    assert!(
        ours.md_select.is_some(),
        "a keystroke is not a stranger's save: what was marked is still \
             about the words it was about",
    );
    assert_eq!(
        (ours.caret.anchor, ours.caret.caret),
        (12, 20),
        "and the caret keeps both its ends",
    );
    assert!(
        ours.md_text.is_empty(),
        "the boxes go whatever happened: they are where the *last* document \
             was drawn and nothing has drawn this one",
    );
}

/// **A double click takes the word and a triple click the block**, in the
/// caret's own coordinate (T5 ①, §7.31 ⑦).
///
/// The word is the classifier the terminal beside the pane uses, walked over
/// the file's bytes; the block is the run of the file it was parsed from,
/// its trailing break off, because the blank line after a paragraph belongs
/// to no block.
#[test]
fn a_repeated_press_takes_a_word_and_then_the_block() {
    let content = "# head\n\nsome bold words\n";
    let (_, ranges) = preview::parse_markdown_ranged(content);
    assert_eq!(
        (
            preview_select::word_start(content, 14),
            preview_select::word_end(content, 14)
        ),
        (13, 17),
        "the word the pointer is inside, and not the spaces round it",
    );
    let block = ranges[1].clone();
    assert_eq!(block, 8..24);
    assert_eq!(
        block.start + preview_live::block_source(content, &block).len(),
        23,
        "a triple click takes the paragraph and stops before its own break",
    );
}

/// One piece of a rendered document, boxed on a page 400px wide.
fn text_box(at: preview_select::Place, len: usize, text: &str) -> PreviewTextBox {
    row_box(at, len, text, 0.0, 20.0)
}
