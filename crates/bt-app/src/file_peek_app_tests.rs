//! **`file_peek`, as the application drives it.** Tests whose first assertion is about
//! `file_peek`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// PIN — **the card's bar promises exactly as far as the card's clamp
/// allows** (user ruling, 2026-08-14: the glance scrolls).
///
/// The wheel over the card goes down `scroll_preview_body`, so the offset it
/// writes is clamped by `preview_document_max_scroll` — the one authority
/// every write of a preview offset already goes through. The *bar* is drawn
/// from `preview_document_height` instead, because a box that sizes itself
/// to its content asks the other question. Two numbers, two geometries, and
/// they have to be the same last pixel: a bar that promised further than the
/// clamp allows is a thumb that stops short of its own track, and one that
/// promised less is a document with a tail nothing can reach.
///
/// Asked of the three documents whose two answers are computed apart — the
/// mono body, the patch and the table, each out of its own geometry.
/// Markdown's pair is the same expression written twice in
/// `preview_document_height` and `preview_document_max_scroll`, which is
/// this same guarantee said in a way that cannot drift.
///
/// MUTATIONS that must turn it red:
/// ① drop the outer padding from `PreviewTableGeometry::document_height`
///    (`self.content_height` alone) — the csv's bar is two paddings short of
///    its clamp;
/// ② drop the `padding_y * 2.0` from `preview_mono_geometry`'s
///    `content_height` — text and diff both lose their air;
/// ③ have `file_peek::scroll_bar` measure the overflow against the card's
///    *frame* rather than its body — every case goes red by the head and the
///    foot together.
#[test]
fn the_glance_cards_bar_promises_exactly_as_far_as_its_clamp_allows() {
    let scale = 1.0_f32;
    let advance = 7.0_f32;
    let row = [40.0, 300.0, 240.0, 320.0];
    let window = (1200.0, 800.0);
    let probe = [
        0.0,
        0.0,
        file_peek::body_width(scale),
        file_peek::body_max_height(scale, true),
    ];

    let agree = |what: &str, document: &PreviewDocument, rows_height: f32, columns: usize| {
        let height = preview_document_height(document, probe, scale, advance, rows_height, columns);
        let card = file_peek::PeekContent {
            name: "sample".to_owned(),
            ftype: "text".to_owned(),
            dirty: false,
            meta: Some("3 KB".to_owned()),
            body: file_peek::PeekBody::Document(height),
        };
        let layout = file_peek::layout(
            &card,
            file_peek::PeekAnchor::row(row),
            window,
            60.0,
            24.0,
            scale,
        );
        let clamp = preview_document_max_scroll(
            document,
            layout.body,
            scale,
            advance,
            rows_height,
            columns,
        )[1];
        match preview_body_bar(
            layout.body,
            preview::ScrollAxis::Vertical,
            [0.0, 0.0],
            height,
            scale,
        ) {
            Some(bar) => {
                assert!(
                    clamp > 0.0,
                    "{what}: a bar was drawn over a document with nowhere to go"
                );
                assert!(
                    (bar.overflow - clamp).abs() < 0.5,
                    "{what}: the bar offers {} and the clamp allows {clamp}",
                    bar.overflow
                );
            }
            None => assert_eq!(
                clamp, 0.0,
                "{what}: no bar drawn, so there had better be nowhere to go"
            ),
        }
    };

    // ① Plain text — the mono body, forty lines in a card that holds a dozen.
    let lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    let text_metrics = seats::preview_text_metrics(scale);
    agree(
        "text",
        &PreviewDocument::Text {
            wrap: preview_edit::WrapLayout::unwrapped(&lines),
            lines: lines.clone(),
            highlight: highlight::Highlighting::default(),
        },
        text_metrics.line_height * 40.0,
        0,
    );

    // ② A patch — the same mono geometry with the hunk margins in it.
    let patch = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n context\n-gone\n+here\n".repeat(8);
    let diff_metrics = seats::preview_diff_metrics(scale);
    let margin = (seats::PREVIEW_DIFF_HUNK_MARGIN_LOGICAL_PX * scale).round();
    let mut top = 0.0_f32;
    let diff_rows: Vec<DiffRow> = patch
        .lines()
        .map(|line| {
            let kind = preview::diff_line_kind(line);
            if kind == preview::DiffLineKind::Hunk {
                top += margin;
            }
            let row = DiffRow {
                text: preview::expand_tabs(line),
                kind,
                top,
            };
            top += diff_metrics.line_height;
            row
        })
        .collect();
    let diff_height = diff_rows
        .last()
        .map_or(0.0, |row| row.top + diff_metrics.line_height);
    agree("diff", &PreviewDocument::Diff(diff_rows), diff_height, 40);

    // ③ A csv — the table geometry, whose padding lives somewhere else
    //    entirely and is the one this most easily drifts on.
    let csv = "name,size\n".to_owned() + &"alpha,10\n".repeat(40);
    let rows = preview::csv_rows(&csv);
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let column_cells: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| bt_unicode::text_width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();
    agree(
        "table",
        &PreviewDocument::Table { rows, column_cells },
        0.0,
        0,
    );

    // ④ And a short file, which wears no bar and has nowhere to go — the
    //    other half of the same agreement.
    agree(
        "short text",
        &PreviewDocument::Text {
            wrap: preview_edit::WrapLayout::unwrapped(&lines[..2]),
            lines: lines[..2].to_vec(),
            highlight: highlight::Highlighting::default(),
        },
        text_metrics.line_height * 2.0,
        0,
    );
}
