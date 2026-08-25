//! A proven GFM pipe table in the transcript, laid out and painted at terminal metrics.
//!
//! # One table layout, drawn twice
//!
//! Everything about *how a table looks* — how wide a column gets, how a cell is inset, where the
//! hairlines fall, that the heading row stands on its own fill — was decided for the preview pane
//! and lives in `main.rs` beside the rest of the markdown layout ([`crate::markdown_table_columns`]
//! and [`crate::push_markdown_table`]). This module supplies the two things that differ when the
//! same table is a block in a terminal pane rather than a block in an open file, and nothing else:
//!
//! 1. **The type size.** The preview sets a document at its own 13px; a terminal pane sets one at
//!    the terminal's font size, so the block reads as the same size as the text it stands over.
//!    Every other number in [`seats::PreviewMarkdownMetrics`] is a ratio of that one, so asking for
//!    the metrics at `terminal_font_px / 13` gives the preview's proportions at the terminal's
//!    scale rather than a second set of constants to keep in step.
//! 2. **The origin.** A preview table sits inside a scrolled document; a block sits at its own
//!    top-left, which the renderer works out from the placement. So everything here is laid out
//!    from `(0, 0)` and [`bt_render::TableBlockPaint`] is handed over in that space.
//!
//! # Why this is not a raster
//!
//! Every other rendered block in this product arrives as RGBA pixels from a worker thread, and a
//! table deliberately does not. A formula is typeset by an engine with its own fonts, so pixels are
//! the only thing it can hand over. A table is *this window's own text* — the same faces, the same
//! CJK fallback chain, the same hinting as the chrome beside it — and rasterizing it would mean
//! shaping it a second time against a second font stack, off the thread that owns the first one.
//! Two shapers is two answers to "how wide is 计划发卡", and the one that got it wrong would be the
//! one that decided the column widths.

use bt_detect::table::{ColumnAlignment, TableSpan};

use crate::{
    MarkdownBlockLayout, MarkdownSink, markdown_table_columns, preview, push_markdown_table, seats,
};

/// A table block's picture and the box it needs.
pub struct TableBlock {
    pub paint: bt_render::TableBlockPaint,
    /// The block's own size in physical pixels — what the decoration record reports as the
    /// artifact's extent, and therefore how many transcript rows the block covers.
    pub width_px: u32,
    pub height_px: u32,
}

/// The metrics a table block is set at: the preview's own proportions, scaled so that body text is
/// the terminal's font size.
///
/// A block that set its cells at 13px beside a terminal running at 16 would read as a quotation
/// from somewhere else. The preview's numbers are all ems of its base, so one division carries all
/// of them across.
#[must_use]
pub fn metrics(terminal_font_px: f32) -> seats::PreviewMarkdownMetrics {
    seats::preview_markdown_metrics(
        (terminal_font_px / preview::PREVIEW_MD_FONT_LOGICAL_PX).max(0.1),
    )
}

/// The rows and alignments a proven span spells, in the shape the shared painter reads.
///
/// Inline runs come from [`preview::parse_inline`], so `**bold**` and `` `code` `` inside a cell
/// mean in a terminal exactly what they mean in an open file.
#[must_use]
pub fn rows(span: &TableSpan) -> (Vec<preview::TableRow>, Vec<ColumnAlignment>) {
    let rows = span
        .rows()
        .map(|row| row.iter().map(|cell| preview::parse_inline(cell)).collect())
        .collect();
    (rows, span.alignments.clone())
}

/// Lay a table out and paint it, from `(0, 0)`.
///
/// `measure` is the shaper: it answers how wide one cell's runs are set, and it is injected for the
/// reason [`crate::markdown_table_columns`] injects it — the arithmetic here is the same whether
/// the answer comes from a live font system or from a test's four-pixels-a-character stand-in.
pub fn build(
    span: &TableSpan,
    metrics: seats::PreviewMarkdownMetrics,
    palette: &bt_render::ChromePalette,
    mut measure: impl FnMut(&[preview::Span], bool) -> f32,
) -> TableBlock {
    let (rows, alignments) = rows(span);
    let columns = markdown_table_columns(&rows, metrics, &mut measure);
    // Uniform, because no cell wraps — the preview settled that on 2026-08-13 and a terminal block
    // has even less reason to disagree: a row whose height depended on the pane would change every
    // time the pane did, and a rendered block's height is how many transcript rows it covers.
    let row_height = metrics.line_height + metrics.table_border + metrics.table_padding_y * 2.0;
    let width = columns.iter().sum::<f32>() + metrics.table_border;
    let height = row_height * rows.len() as f32 + metrics.table_border;
    let placed = MarkdownBlockLayout::table(columns, width, vec![row_height; rows.len()], height);
    let mut quads = Vec::new();
    let mut paragraphs = Vec::new();
    let mut links = Vec::new();
    // **A terminal table has no formulas of its own**, and that is a statement
    // about authority rather than a shortcut. The dollars in a printed table
    // cell are somebody else's output, and what a terminal may do with those is
    // settled by `bt_detect`'s own gates — not by the markdown preview's rule
    // that an author's `$` is markup. An empty document therefore means exactly
    // what it says here: nothing in this table has a picture, so every cell
    // draws the text it printed.
    let math = crate::DocumentMath::default();
    let mut math_sites = Vec::new();
    push_markdown_table(
        MarkdownSink {
            quads: &mut quads,
            paragraphs: &mut paragraphs,
            links: &mut links,
            math_sites: &mut math_sites,
            region: None,
        },
        &rows,
        &alignments,
        &placed,
        [0.0, 0.0],
        crate::MarkdownStyle {
            metrics,
            palette,
            math: &math,
        },
    );
    TableBlock {
        paint: bt_render::TableBlockPaint { quads, paragraphs },
        width_px: width.ceil().max(1.0) as u32,
        height_px: height.ceil().max(1.0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four pixels a character: a stand-in shaper, so the arithmetic can be checked without a
    /// device. The same shape the preview's own layout tests use.
    fn measure(spans: &[preview::Span], heading: bool) -> f32 {
        let characters: usize = spans.iter().map(|span| span.text.chars().count()).sum();
        characters as f32 * if heading { 5.0 } else { 4.0 }
    }

    fn span(source: &str) -> TableSpan {
        let lines: Vec<&str> = source.lines().collect();
        bt_detect::table::table_at(&lines).expect("a table")
    }

    #[test]
    fn a_column_is_its_widest_cell_plus_the_chrome_and_never_below_the_floor() {
        let metrics = metrics(13.0);
        let table = build(
            &span("| a | a much longer heading |\n|---|---|\n| 1 | 2 |"),
            metrics,
            &bt_render::chrome_palette(),
            measure,
        );
        let chrome = metrics.table_border + metrics.table_padding_x * 2.0;
        // The narrow column is floored; the wide one is its heading's own width.
        let expected = chrome
            + metrics.table_min_column
            + chrome
            + measure(&preview::parse_inline("a much longer heading"), true)
            + metrics.table_border;
        assert!(
            (table.width_px as f32 - expected.ceil()).abs() <= 1.0,
            "width {} should be about {expected}",
            table.width_px
        );
    }

    #[test]
    fn the_height_is_one_uniform_row_per_row_of_the_table() {
        let metrics = metrics(13.0);
        let two = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |"),
            metrics,
            &bt_render::chrome_palette(),
            measure,
        );
        let three = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |"),
            metrics,
            &bt_render::chrome_palette(),
            measure,
        );
        let row = metrics.line_height + metrics.table_border + metrics.table_padding_y * 2.0;
        assert!(
            ((three.height_px - two.height_px) as f32 - row).abs() <= 1.0,
            "one more row is one more row's height"
        );
    }

    #[test]
    fn the_heading_row_is_set_in_the_heading_ink_over_its_own_fill() {
        let palette = bt_render::chrome_palette();
        let table = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |"),
            metrics(13.0),
            &palette,
            measure,
        );
        assert!(
            table
                .paint
                .quads
                .iter()
                .any(|quad| quad.color == palette.files_row_hover),
            "the heading band is drawn"
        );
        let heads = &table.paint.paragraphs[..2];
        assert!(
            heads.iter().all(|paragraph| paragraph
                .runs
                .iter()
                .all(|run| run.color == palette.preview_table_head_text)),
            "every heading cell is set in the heading ink"
        );
    }

    #[test]
    fn the_colons_reach_the_cells_that_are_set_by_them() {
        let table = build(
            &span("| l | c | r | n |\n| :-- | :-: | --: | --- |\n| 1 | 2 | 3 | 4 |"),
            metrics(13.0),
            &bt_render::chrome_palette(),
            measure,
        );
        // Four columns, two rows: the body row's cells are paragraphs 4..8.
        let body = &table.paint.paragraphs[4..8];
        assert_eq!(
            body.iter()
                .map(|paragraph| (paragraph.align_center, paragraph.align_right))
                .collect::<Vec<_>>(),
            vec![(false, false), (true, false), (false, true), (false, false)],
            "left and undeclared both read as ordinary text; only centre and right move a cell"
        );
    }

    #[test]
    fn no_cell_wraps_however_narrow_the_column_would_have_to_be() {
        let table = build(
            &span("| a very long single cell indeed |\n|---|"),
            metrics(13.0),
            &bt_render::chrome_palette(),
            measure,
        );
        assert!(
            table
                .paint
                .paragraphs
                .iter()
                .all(|paragraph| !paragraph.wrap),
            "the column was measured to hold the cell whole"
        );
    }

    #[test]
    fn the_picture_starts_at_the_blocks_own_corner() {
        let table = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |"),
            metrics(13.0),
            &bt_render::chrome_palette(),
            measure,
        );
        assert!(
            table
                .paint
                .quads
                .iter()
                .all(|quad| quad.rect[0] >= 0.0 && quad.rect[1] >= 0.0),
            "nothing is drawn above or left of the origin the renderer places"
        );
        assert!(
            table
                .paint
                .quads
                .iter()
                .any(|quad| quad.rect[0] <= 0.001 && quad.rect[1] <= 0.001),
            "and the picture actually reaches it"
        );
    }

    #[test]
    fn a_bigger_terminal_font_sets_a_bigger_table() {
        let small = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |"),
            metrics(12.5),
            &bt_render::chrome_palette(),
            measure,
        );
        let large = build(
            &span("| a | b |\n|---|---|\n| 1 | 2 |"),
            metrics(20.0),
            &bt_render::chrome_palette(),
            measure,
        );
        assert!(large.height_px > small.height_px);
    }
}
