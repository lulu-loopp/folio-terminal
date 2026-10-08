//! **`preview_edit`, as the application drives it.** Tests whose first assertion is about
//! `preview_edit`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::source_block;

/// **The caret the painter strikes, the cell the IME hangs from and the
/// column the editor counts are one number** (user report, 2026-09-11).
///
/// The report was a caret standing a gap to the right of the character it
/// was editing on a line of Chinese, and the gap grew with every ideograph
/// before it: the letters were shaped by a fallback face whose advances are
/// its own, while the caret was counted in cells. The letters are placed on
/// the cells now ([`bt_render::PreviewParagraph::cell_advance`]) and this
/// holds the other three readings to the same arithmetic.
///
/// MUTATION: derive the IME's rectangle from anything but
/// [`markdown_source_cell`] and a candidate list stands beside the caret it
/// claims to follow.
#[test]
fn the_caret_the_ime_and_the_column_are_one_arithmetic_on_a_chinese_line() {
    let line = "网页预览需要 WebView2。";
    let source = source_block(0, 0, line);
    let box_of_block = [100.0, 40.0, 500.0, 60.0];
    let palette = bt_render::chrome_palette();
    let paint = |caret: usize| {
        let column = preview_edit::column_of(line, caret);
        let mut quads = Vec::new();
        let mut paragraphs = Vec::new();
        push_markdown_source_block(
            (&mut quads, &mut paragraphs),
            &source,
            Some(&MarkdownCaretPaint {
                seat: MarkdownCaretSeat::Source {
                    block: source.index,
                    line: 0,
                    column,
                },
                lit: true,
                selection: 0..0,
                band: 0..0,
                caret_width: 2.0,
                preedit: None,
            }),
            &highlight::Highlighting::plain(),
            box_of_block,
            [0.0, 0.0, 1000.0, 1000.0],
            &palette,
        );
        let [quad] = quads.as_slice() else {
            panic!("one caret: {quads:#?}");
        };
        (column, quad.rect[0])
    };
    // Every seam of the line, in the file's own bytes: before each cluster
    // and after the last one.
    let mut byte = 0usize;
    for cluster in bt_unicode::graphemes(line) {
        let (column, drawn) = paint(byte);
        assert!(
            (drawn - (box_of_block[0] + 8.0 * column as f32)).abs() < f32::EPSILON,
            "the caret in front of {cluster:?} is struck at {drawn}, not on its \
                 column {column}",
        );
        assert!(
            (markdown_source_cell(&source, box_of_block, 0, column)[0] - drawn).abs()
                < f32::EPSILON,
            "and the cell the IME hangs its candidates from is the same one",
        );
        // The press that would put the caret there agrees as well, which is
        // the third reading of the one grid.
        assert_eq!(
            markdown_source_offset_at(&source, box_of_block, drawn, 44.0),
            byte,
            "a press on the caret's own x names the byte the caret is at",
        );
        byte += cluster.len();
    }
    let (column, _) = paint(line.len());
    assert_eq!(
        column,
        bt_unicode::text_width(line),
        "the last seam is the whole line's width in cells, ideographs counted \
             as the two they draw as",
    );
}

/// **Enter splits a block and Backspace at a block start merges two**, and
/// neither is a case anybody wrote: the keys move bytes and the parser
/// answers.
///
/// MUTATION: make Backspace refuse to cross a block boundary and the two
/// paragraphs can never be joined again — which is the special case §9.1
/// exists to avoid having.
#[test]
fn enter_and_backspace_split_and_merge_blocks_through_the_reparse() {
    let mut content = String::from("one two\n");
    let mut caret = preview_edit::EditCaret {
        anchor: 3,
        caret: 3,
        desired_column: None,
        desired_x: None,
    };
    let eol = preview_edit::eol_of(&content).to_owned();
    assert!(preview_edit::insert(&mut content, &mut caret, &eol));
    assert!(preview_edit::insert(&mut content, &mut caret, &eol));
    assert_eq!(content, "one\n\n two\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 2, "one paragraph has become two");
    assert_eq!(
        preview_live::caret_seat(&content, &ranges, caret.caret),
        preview_live::CaretSeat::Block(1),
        "and the caret is in the new one, which is therefore the source block",
    );

    let mut content = String::from("one\n\ntwo\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 2);
    let mut caret = preview_edit::EditCaret {
        anchor: 5,
        caret: 5,
        desired_column: None,
        desired_x: None,
    };
    assert!(preview_edit::backspace(&mut content, &mut caret));
    assert_eq!(content, "one\ntwo\n");
    let (_, ranges) = preview::parse_markdown_ranged(&content);
    assert_eq!(ranges.len(), 1, "and two paragraphs have become one");
    assert_eq!(
        preview_live::block_source(&content, &ranges[0]),
        "one\ntwo",
        "the merged block is drawn as both of its lines",
    );
}

/// **A caret's selection is the file's own bytes, marks and all** (T5 ④,
/// research §10 Q3) — the copy semantics, said as an assertion.
///
/// This is where the rendered page's copy parts company with §7.31 ⑥, and
/// the departure is narrow: the *range* is still the run of the document the
/// reader dragged across, and what comes with it is the `#` and the `**`
/// inside that run, because they are the characters the caret was dragged
/// over. It is what a paste of the result puts back.
#[test]
fn a_caret_selection_copies_the_files_own_bytes() {
    let content = "# head\n\nsome **bold** words\n";
    let caret = preview_edit::EditCaret {
        anchor: 0,
        caret: 6,
        desired_column: None,
        desired_x: None,
    };
    assert_eq!(
        caret.selected(content),
        "# head",
        "the hashes are inside the range and come with it",
    );
    let across = preview_edit::EditCaret {
        anchor: 13,
        caret: 21,
        desired_column: None,
        desired_x: None,
    };
    assert_eq!(across.selected(content), "**bold**");
}

/// PIN — the caret, the band and the click all agree about which **row** a
/// column of a folded line is on.
///
/// Three readings of one arithmetic that used to be one reading: the caret's
/// drawn position, the selection band under it and the byte a click names.
/// Left disagreeing they produce the classic reflow bug — a caret drawn on
/// the first row of a paragraph while the text it is editing is on the
/// third.
///
/// MUTATION: build the paint with `WrapLayout::unwrapped` and the caret's row
/// goes to zero while its column runs off the pane — the pre-ruling drawing
/// of a wrapped surface.
#[test]
fn a_caret_on_a_folded_line_is_drawn_on_the_row_it_is_really_on() {
    let lines = vec!["aaaa bbbb cccc dddd".to_owned()];
    let wrap = preview_edit::WrapLayout::wrapped(&lines, 10);
    assert_eq!(wrap.rows(), 2);

    // Column 12 is the third character of "cccc", which is on the second row
    // and two cells into it.
    assert_eq!(preview_caret_row(&wrap, 0, 12), (1, 2));
    // And a caret at the head of the line is on the first row at column
    // zero, which is the case a wrap-blind painter also gets right — so it
    // is asserted next to one it does not.
    assert_eq!(preview_caret_row(&wrap, 0, 0), (0, 0));

    // The band for a selection covering columns 8..14 is cut in two, one
    // piece per row, each measured from its own row's left edge.
    let content = "aaaa bbbb cccc dddd";
    let starts = preview_edit::line_starts(content);
    let selection = 8..14;
    let selected = preview_edit::selected_columns(content, &starts, 0, &selection)
        .expect("the selection covers this line");
    assert_eq!(selected, (8, 14));
    let bands = preview_edit_bands(content, &starts, &selection, &wrap, 0..wrap.rows());
    assert_eq!(
        bands,
        vec![(0, 8, 10), (1, 0, 4)],
        "the band turns the corner with the text it is under"
    );

    // And Down from the first row lands on the same column of the second,
    // rather than leaving the line entirely.
    let mut caret = preview_edit::EditCaret {
        anchor: 2,
        caret: 2,
        desired_column: None,
        desired_x: None,
    };
    step_preview_caret_by_row(content, &mut caret, preview_edit::Motion::Down, &wrap, 10)
        .expect("Down is a vertical motion");
    assert_eq!(
        caret.caret, 12,
        "Down walks one drawn row, not one paragraph"
    );
    assert_eq!(caret.desired_column, Some(2));
    // Up returns to where it started.
    step_preview_caret_by_row(content, &mut caret, preview_edit::Motion::Up, &wrap, 10)
        .expect("Up is one too");
    assert_eq!(caret.caret, 2);
    // Home and End are not this function's business: they belong to the
    // logical line, which is the textarea convention the ruling names.
    assert!(
        step_preview_caret_by_row(
            content,
            &mut caret,
            preview_edit::Motion::LineEnd,
            &wrap,
            10
        )
        .is_none()
    );
}
