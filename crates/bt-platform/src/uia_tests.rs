//! The arithmetic UI Automation's text ranges are answered by (T-UIA-SKELETON, 88a): pure, so it
//! runs on every gate.

use super::*;

/// Where row `row` of a test pane is drawn: column 0 at x = 4, cells 10 wide, rows 20 tall from
/// y = 2.
fn place(row: u32) -> RowPlace {
    RowPlace {
        left: 4.0,
        cell_width: 10.0,
        top: 2.0 + 20.0 * f64::from(row),
        height: 20.0,
    }
}

/// A test's notion of a wide glyph: the CJK and emoji planes the tests write.
fn wide(character: char) -> bool {
    u32::from(character) >= 0x2E80
}

/// One cell's cluster, whether it is a spacer, and whether a word starts at it — the builder's
/// rule: a word starts at a non-blank cell whose class (word or delimiter) differs from the cell
/// before it.
fn cells_of(text: &str) -> Vec<(String, bool, bool)> {
    let mut cells = Vec::new();
    let mut before: Option<u8> = None;
    for character in text.chars() {
        let class = if character == ' ' {
            0
        } else if "-./".contains(character) {
            1
        } else {
            2
        };
        let word_start = class != 0 && before != Some(class);
        before = Some(class);
        cells.push((character.to_string(), false, word_start));
        if wide(character) {
            cells.push((String::new(), true, false));
        }
    }
    cells
}

fn row(text: &str, index: u32, continues: bool, keep: u32) -> RowText {
    let cells = cells_of(text);
    RowText::from_cells(
        cells.iter().map(|(text, spacer, word_start)| CellIn {
            text,
            spacer: *spacer,
            word_start: *word_start,
        }),
        place(index),
        continues,
        keep,
    )
}

fn pane(rows: Vec<RowText>) -> PaneText {
    PaneText::new(
        PaneId { tab: 1, seat: 7 },
        "pwsh".to_owned(),
        ClientRect {
            left: 0.0,
            top: 0.0,
            width: 800.0,
            height: 600.0,
        },
        true,
        rows,
        None,
        None,
    )
}

/// RED (88a) — **a wide glyph is one unit over two columns**: its text is one character, its
/// rectangle is two cells wide, and a caret or a point on its spacer column lands on it.
///
/// MUTATION: give a spacer a unit of its own in `RowText::from_cells`, or drop the `columns += 1`.
#[test]
fn a_wide_glyph_is_one_unit_over_two_columns() {
    let text = pane(vec![row("a中b", 0, false, 0)]);
    assert_eq!(text.len(), 3, "a, 中, b");
    assert_eq!(text.text(0, 3, -1), "a中b");
    assert_eq!(
        text.rectangles(1, 2),
        vec![ClientRect {
            left: 14.0,
            top: 2.0,
            width: 20.0,
            height: 20.0
        }],
        "the glyph covers columns 1 and 2"
    );
    assert_eq!(
        text.offset_of(CellPoint { row: 0, column: 2 }),
        1,
        "the spacer is the glyph"
    );
    assert_eq!(text.offset_of(CellPoint { row: 0, column: 3 }), 2, "b");
    assert_eq!(
        text.offset_at(29.0, 10.0),
        1,
        "a point on the spacer is the glyph"
    );
    assert_eq!(text.offset_at(39.0, 10.0), 2);
    assert_eq!(
        text.rectangles(2, 3)[0].left,
        34.0,
        "b stands after both columns"
    );
}

/// RED (88a) — **rows are lines joined by `\r\n`, and a row's trailing blanks are not text**.
#[test]
fn rows_join_with_a_line_break_and_trailing_blanks_are_dropped() {
    let text = pane(vec![row("ab  ", 0, false, 0), row("cd", 1, false, 0)]);
    assert_eq!(text.len(), 5, "ab, the break, cd");
    assert_eq!(text.text(0, 5, -1), "ab\r\ncd");
    assert!(text.is_boundary(3, Boundary::Line));
    assert!(
        !text.is_boundary(2, Boundary::Line),
        "the break belongs to its row"
    );
    assert_eq!(text.enclosing(4, Boundary::Line), (3, 5));
    assert_eq!(
        text.enclosing(1, Boundary::Line),
        (0, 3),
        "a line holds its break"
    );
}

/// RED (88a) — **a soft-wrapped row continues into the next with nothing between them**: one
/// paragraph over two lines, and the wrapped row's blanks are real columns of it.
#[test]
fn a_wrapped_row_is_one_paragraph_over_two_lines() {
    let text = pane(vec![row("ab  ", 0, true, 0), row("cd", 1, false, 0)]);
    assert_eq!(text.text(0, text.len(), -1), "ab  cd");
    assert!(text.is_boundary(4, Boundary::Line));
    assert!(!text.is_boundary(4, Boundary::Paragraph));
    assert_eq!(text.enclosing(5, Boundary::Paragraph), (0, 6));
    assert_eq!(text.enclosing(5, Boundary::Line), (4, 6));
}

/// RED (88a) — **a caret after a typed space keeps the space**: the blanks up to the caret's
/// column are text, so the caret's offset is not past the end.
#[test]
fn the_caret_keeps_the_blanks_before_it() {
    let rows = vec![row("ab ", 0, false, 3)];
    let text = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        screen(),
        true,
        rows,
        Some(CellPoint { row: 0, column: 3 }),
        None,
    );
    assert_eq!(text.len(), 3);
    assert_eq!(text.caret(), Some(3));
    assert_eq!(
        text.rectangles(3, 3)[0].left,
        34.0,
        "the caret stands after the space"
    );
    assert_eq!(text.rectangles(3, 3)[0].width, 1.0);
}

/// RED (88a) — **words**: the builder's starts are the boundaries, a line break is one, and a
/// count past either end stops there and says how far it went.
#[test]
fn words_move_by_the_builders_starts() {
    let text = pane(vec![
        row("ls -la foo", 0, false, 0),
        row("bar", 1, false, 0),
    ]);
    assert_eq!(text.moved(0, Boundary::Word, 1), (3, 1), "to -");
    assert_eq!(text.moved(0, Boundary::Word, 2), (4, 2), "to la");
    assert_eq!(text.moved(0, Boundary::Word, 3), (7, 3), "to foo");
    assert_eq!(text.moved(7, Boundary::Word, 1), (10, 1), "to the break");
    assert_eq!(text.moved(10, Boundary::Word, 1), (11, 1), "to bar");
    assert_eq!(
        text.moved(11, Boundary::Word, 5),
        (14, 1),
        "stops at the end"
    );
    assert_eq!(
        text.moved(4, Boundary::Word, -9),
        (0, -2),
        "stops at the start"
    );
    assert_eq!(text.enclosing(8, Boundary::Word), (7, 10), "foo");
    assert_eq!(text.text(7, 10, -1), "foo");
}

/// RED (88a) — **`Move` on a range**: a non-degenerate range becomes one unit and stays one.
#[test]
fn a_range_moves_as_one_unit() {
    let text = pane(vec![row("ab", 0, false, 0), row("cd", 1, false, 0)]);
    assert_eq!(text.moved_range(0, 1, Boundary::Line, 1), (3, 5, 1));
    assert_eq!(
        text.moved_range(3, 5, Boundary::Line, 1),
        (3, 5, 0),
        "no line after the last"
    );
    assert_eq!(text.moved_range(4, 5, Boundary::Line, -1), (0, 3, -1));
    assert_eq!(text.moved_range(4, 4, Boundary::Character, -2), (2, 2, -2));
}

/// RED (88a) — **`GetText`'s limit counts UTF-16 units** and never splits a character.
#[test]
fn the_text_limit_counts_utf16_units() {
    let text = pane(vec![row("a😀b", 0, false, 0)]);
    assert_eq!(text.len(), 3);
    assert_eq!(
        text.text(0, 3, 2),
        "a",
        "the emoji is two units and does not fit"
    );
    assert_eq!(text.text(0, 3, 3), "a😀");
    assert_eq!(text.text(0, 3, 0), "");
}

/// RED (88a) — **`FindText`** over wide glyphs, either way and ignoring case.
#[test]
fn find_answers_offsets_over_wide_glyphs() {
    let text = pane(vec![row("a中B a中b", 0, false, 0)]);
    assert_eq!(text.find(0, text.len(), "中b", false, true), Some((1, 3)));
    assert_eq!(text.find(0, text.len(), "中b", true, true), Some((5, 7)));
    assert_eq!(text.find(0, text.len(), "中b", false, false), Some((5, 7)));
    assert_eq!(text.find(0, 3, "中b", false, false), None);
    assert_eq!(text.find(0, text.len(), "", false, false), None);
}

/// RED (88a) — **a range over two rows is two rectangles**, each its row's cells.
#[test]
fn a_range_over_two_rows_is_two_rectangles() {
    let text = pane(vec![row("abc", 0, false, 0), row("de", 1, false, 0)]);
    let rectangles = text.rectangles(1, 6);
    assert_eq!(rectangles.len(), 2);
    assert_eq!((rectangles[0].left, rectangles[0].width), (14.0, 20.0));
    assert_eq!(
        (rectangles[1].left, rectangles[1].top, rectangles[1].width),
        (4.0, 22.0, 20.0)
    );
}

/// A body far larger than any test pane's rows.
fn screen() -> ClientRect {
    ClientRect {
        left: 0.0,
        top: 0.0,
        width: 800.0,
        height: 600.0,
    }
}

/// RED (the review's F7) — **a range's rectangles stay inside the pane's body**: a row cut by the
/// body's right edge is cut there, a row below the body (the overscan) has no rectangle, and a
/// caret past the right edge stands on the body's last pixel column.
///
/// MUTATION: push the drawn rectangle unclipped in `PaneText::rectangles`, or drop the clamp of a
/// degenerate range's left.
#[test]
fn rectangles_are_clipped_to_the_pane_body() {
    let body = ClientRect {
        left: 0.0,
        top: 0.0,
        width: 25.0,
        height: 30.0,
    };
    let text = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        body,
        true,
        vec![row("abc", 0, false, 0), row("de", 1, false, 0)],
        None,
        None,
    );
    let rectangles = text.rectangles(0, text.len());
    assert_eq!(rectangles.len(), 2, "{rectangles:?}");
    assert_eq!(
        (rectangles[0].left, rectangles[0].width),
        (4.0, 21.0),
        "abc is cut at the body's right edge"
    );
    assert_eq!(
        (rectangles[1].top, rectangles[1].height),
        (22.0, 8.0),
        "de is cut at the body's bottom"
    );
    let caret = text.rectangles(3, 3);
    assert_eq!(caret.len(), 1);
    assert_eq!(
        caret[0].left, 24.0,
        "after abc is past the edge: the last column"
    );
    let below = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        ClientRect {
            height: 20.0,
            ..body
        },
        true,
        vec![row("ab", 0, false, 0), row("x", 1, false, 0)],
        None,
        None,
    );
    assert_eq!(
        below.rectangles(3, 4),
        Vec::new(),
        "a row of the overscan has no rectangle"
    );
}

/// RED (the review's F4) — **a caret on an empty last row is that row**: its offset is the
/// document's end, and every unit there but the document is the empty range at the end, never the
/// row above; a full last row still encloses its last line at the end.
///
/// MUTATION: drop the empty-last-row arm of `PaneText::enclosing`, or place a degenerate range at
/// the end on the row before the end's (`PaneText::rectangles`).
#[test]
fn a_caret_on_an_empty_last_row_reads_as_that_row() {
    let text = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        screen(),
        true,
        vec![row("PS> ls", 0, false, 0), row("", 1, false, 0)],
        Some(CellPoint { row: 1, column: 0 }),
        None,
    );
    let end = text.len();
    assert_eq!(text.caret(), Some(end), "the empty row starts at the end");
    for unit in [
        Boundary::Character,
        Boundary::Word,
        Boundary::Line,
        Boundary::Paragraph,
    ] {
        assert_eq!(text.enclosing(end, unit), (end, end), "{unit:?}");
    }
    assert_eq!(text.enclosing(end, Boundary::Document), (0, end));
    assert_eq!(text.rectangles(end, end)[0].top, 22.0, "drawn on row 1");
    let full = pane(vec![row("ab", 0, false, 0), row("cd", 1, false, 0)]);
    assert_eq!(
        full.enclosing(full.len(), Boundary::Line),
        (3, 5),
        "the last line"
    );
}

/// RED (88a) — **a point below the text is on the last row**, and past a row's end is that row's
/// end.
#[test]
fn a_point_finds_the_nearest_row_and_the_rows_end() {
    let text = pane(vec![row("abc", 0, false, 0), row("de", 1, false, 0)]);
    assert_eq!(text.offset_at(200.0, 30.0), 6, "past de is the end");
    assert_eq!(
        text.offset_at(5.0, 500.0),
        4,
        "below the text is the last row"
    );
    assert_eq!(text.offset_at(200.0, 10.0), 3, "past abc is its break");
}

/// RED (88a) — **a selection is two offsets in order**, its end exclusive.
#[test]
fn a_selection_is_two_offsets_in_order() {
    let text = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        screen(),
        false,
        vec![row("abc", 0, false, 0), row("中e", 1, false, 0)],
        None,
        Some((
            CellPoint { row: 1, column: 2 },
            CellPoint { row: 0, column: 1 },
        )),
    );
    assert_eq!(text.selection(), Some((1, 5)));
    assert_eq!(text.text(1, 5, -1), "bc\r\n中");
}

/// PIN — the text units a pane answers, and the control types the experiment tries.
#[test]
fn units_and_shapes_are_windows_numbers() {
    assert_eq!(Boundary::of_text_unit(0), Boundary::Character);
    assert_eq!(
        Boundary::of_text_unit(1),
        Boundary::Word,
        "Format is a word"
    );
    assert_eq!(Boundary::of_text_unit(2), Boundary::Word);
    assert_eq!(Boundary::of_text_unit(6), Boundary::Document);
    assert_eq!(
        Boundary::of_text_unit(5),
        Boundary::Document,
        "Page is the document"
    );
    assert_eq!(Shape::Edit.control_type_id(), 50_004);
    assert_eq!(Shape::Document.control_type_id(), 50_030);
    assert_eq!(Shape::Text.control_type_id(), 50_020);
    assert!(Shape::Edit.has_value_pattern());
    assert!(!Shape::Text.has_value_pattern());
}

/// PIN — **the root question is read from `lParam`'s low 32 bits**, sign-extended or not.
#[test]
fn the_root_question_reads_the_low_bits() {
    assert!(asks_for_the_root(WM_GETOBJECT, -25));
    assert!(asks_for_the_root(WM_GETOBJECT, 0xFFFF_FFE7));
    assert!(
        !asks_for_the_root(WM_GETOBJECT, -4),
        "OBJID_CLIENT is MSAA's"
    );
    assert!(!asks_for_the_root(0x0010, -25));
}

/// RED (88a) — **a cluster with combining marks is one character**: `e` + U+0301 in one cell is
/// one unit of two code points, and moving by a character steps over both.
///
/// MUTATION: make a unit per `char` in `RowText::from_cells`.
#[test]
fn combining_marks_stay_inside_their_character() {
    let cells = ["e\u{301}", "x"];
    let rows = vec![RowText::from_cells(
        cells.iter().map(|text| CellIn {
            text,
            spacer: false,
            word_start: false,
        }),
        place(0),
        false,
        0,
    )];
    let text = pane(rows);
    assert_eq!(text.len(), 2, "é and x");
    assert_eq!(text.text(0, 1, -1), "e\u{301}", "both code points");
    assert_eq!(text.moved(0, Boundary::Character, 1), (1, 1));
    assert_eq!(text.text(1, 2, -1), "x");
}

/// RED (88a) — **the caret on a wide glyph's spacer is a degenerate range at the glyph's start**:
/// the grid names the second column, the caret's offset is the glyph's, and its rectangle stands
/// at the glyph's left edge.
///
/// MUTATION: map a spacer column to the unit after it in `RowText::index_at_column`.
#[test]
fn the_caret_is_a_degenerate_range_on_the_wide_cell_start() {
    let text = PaneText::new(
        PaneId { tab: 1, seat: 1 },
        String::new(),
        screen(),
        true,
        vec![row("a中b", 0, false, 0)],
        Some(CellPoint { row: 0, column: 2 }),
        None,
    );
    assert_eq!(text.caret(), Some(1), "the glyph, not its spacer");
    let caret = text.rectangles(1, 1);
    assert_eq!(caret.len(), 1);
    assert_eq!(
        (caret[0].left, caret[0].width),
        (14.0, 1.0),
        "at the glyph's left edge"
    );
}

/// RED (88a) — **an unsupported unit expands to the next larger one**: `Format` encloses the word,
/// `Page` the whole document.
///
/// MUTATION: map `Format` or `Page` to `Character` in `Boundary::of_text_unit`.
#[test]
fn expand_to_an_unsupported_unit_takes_the_next_larger() {
    let text = pane(vec![row("ls foo", 0, false, 0), row("bar", 1, false, 0)]);
    assert_eq!(
        text.enclosing(4, Boundary::of_text_unit(1)),
        (3, 6),
        "Format is foo"
    );
    assert_eq!(
        text.enclosing(4, Boundary::of_text_unit(5)),
        (0, text.len()),
        "Page is all"
    );
}

/// RED (88a) — **a range made on an older, longer publication is clamped into the newer**: its
/// offsets past the end become the end, and nothing reads past the rows.
///
/// MUTATION: drop `PaneText::clamp`'s `min`.
#[test]
fn a_range_from_an_older_snapshot_is_clamped() {
    let older = pane((0..10).map(|index| row("row", index, false, 0)).collect());
    let late = older.len() - 2;
    let newer = pane((0..5).map(|index| row("row", index, false, 0)).collect());
    assert!(
        late > newer.len(),
        "the fixture: the range starts past the newer end"
    );
    let (start, end) = (newer.clamp(late), newer.clamp(older.len()));
    assert_eq!((start, end), (newer.len(), newer.len()));
    assert_eq!(newer.text(start, end, -1), "");
    assert_eq!(
        newer.rectangles(start, end).len(),
        1,
        "a degenerate range at the end"
    );
}
