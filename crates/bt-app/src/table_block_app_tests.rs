//! **`table_block`, as the application drives it.** Tests whose first assertion is about
//! `table_block`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    logical_width, markdown_body, presentation_of, rested_bars,
    restored_two_previews_and_a_terminal,
};

/// **A hand on the divider outranks a preview's floor** (最小值主权,
/// 2026-08-08: "a minimum is law to the program and advice to the user").
///
/// Real machine, slice 7: with the layout above on screen, dragging the root
/// divider to give the terminal half the window did *nothing*. The edit was
/// never the problem — `Edit::DragDivider` writes the ratio the hand asked
/// for and consults no minimum — but the solve that turns that ratio into
/// rectangles ran under `Lawful`, so the concession chain put every seat
/// straight back on its floor and folded the terminal again. The divider
/// went dead under the hand.
///
/// MUTATION: solve the dragged tree under `Lawful` (which is what
/// `drive_divider_drag` did before this slice) and every assertion below the
/// first goes red — the terminal comes back 24 pixels wide.
#[test]
fn a_divider_drag_takes_the_room_a_preview_floor_was_holding() {
    let (mut seats, metrics, viewport) = restored_two_previews_and_a_terminal();
    let terminal = seats.identity();
    let root = seats
        .split_slots(
            &seats
                .solve(viewport, &metrics, SizePolicy::Lawful)
                .expect("the folded layout solves"),
        )
        .first()
        .expect("the tree has a root split")
        .id;

    assert_eq!(
        seats.drag_divider(
            &metrics,
            root,
            bt_layout::Ratio::clamped_from_ppm(500_000),
            bt_layout::LogicalPx::px(950),
        ),
        Ok(true),
        "the edit has always honoured the hand — it writes the ratio asked for"
    );

    // What law does with that same tree, and why the hand's rectangles must
    // not go through this door: the tree still does not fit, so law pays for
    // the window by **rearranging** it — a pane becomes a §2.6.3 bar. That is
    // not an answer to "make this divider move"; it is the program deciding
    // the user asked for one pane fewer.
    //
    // (Before the 2026-08-13 collapse-order ruling the bar was the
    // *terminal's* and this line read `logical_width(terminal) ==
    // COLLAPSED_EXTENT` — the divider looked dead because the seat the hand
    // was enlarging was the very seat law folded. Law now folds a preview
    // instead, which is a better layout and still a layout nobody asked for.)
    let lawful = seats
        .solve(viewport, &metrics, SizePolicy::Lawful)
        .expect("still solvable");
    assert_eq!(
        seats
            .preview_seats()
            .into_iter()
            .filter(|seat| presentation_of(&lawful, *seat).is_collapsed_along(bt_layout::Axis::Row))
            .count(),
        1,
        "law buys the room by turning a pane into a bar"
    );

    // And what it does now: nothing folds at all, and past the floors the
    // floors give way together.
    let sovereign = seats
        .solve(viewport, &metrics, SizePolicy::Sovereign)
        .expect("sovereign cannot refuse");
    assert!(
        seats
            .preview_seats()
            .into_iter()
            .chain([terminal])
            .all(|seat| presentation_of(&sovereign, seat) == bt_layout::Presentation::Full),
        "under the hand every pane stays a pane — there are no bars on this road"
    );
    assert!(
        logical_width(&sovereign, terminal) > 200,
        "and it is wide enough to be one: {}",
        logical_width(&sovereign, terminal)
    );
    let previews: Vec<i64> = seats
        .preview_seats()
        .into_iter()
        .map(|seat| logical_width(&sovereign, seat))
        .collect();
    assert!(
        previews
            .iter()
            .all(|width| *width < bt_layout::MIN_PREVIEW_W.floor_px()),
        "the 360 floor gave way, which is the whole of the ruling: {previews:?}"
    );
    assert!(
        previews
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() <= 1),
        "and it gave way *in proportion*, not by one pane paying for the other: {previews:?}"
    );
}

/// PIN (user ruling, 2026-08-13) — **a wide block is its own scrolling
/// region: it clamps at both ends, it takes the offset, and the prose beside
/// it does not move.**
///
/// The third assertion is the one the ruling is really about. The reported
/// symptom was two defects wearing one shape: a table that could not be
/// scrolled at all (the page's axis had been clamped to zero while the
/// table's width was still in the extent), and — before that — prose sliding
/// out of the pane to reach it.
///
/// MUTATIONS:
/// ① drop the `clamp(0.0, overflow)` in `build_preview_markdown_body` — the
///    first assertion goes red and a table can be pushed past its own end;
/// ② apply the offset to `left` for every block rather than the wide one —
///    the third assertion goes red, which is exactly the page-slide the
///    ruling overturned;
/// ③ stop pushing the block into `blocks` — the second assertion goes red and
///    the table prints over the paragraph beside it, unclipped.
#[test]
fn a_wide_markdown_block_scrolls_inside_itself_and_the_prose_stays_put() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 300.0, 400.0];
    let page = body[2] - body[0] - metrics.padding_x * 2.0;
    let wide = page + 400.0;
    let blocks = vec![
        preview::MarkdownBlock::Paragraph(vec![preview::Span::plain("prose")]),
        preview::MarkdownBlock::Code {
            lang: None,
            text: "a very long line".to_owned(),
        },
    ];
    let layout: preview_viewport::Layout = vec![
        MarkdownBlockLayout::solid(metrics.line_height),
        MarkdownBlockLayout {
            width: wide,
            top: metrics.line_height + metrics.paragraph_gap,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let overflow = wide - page;
    let prose_left = |offsets: &[f32]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        );
        built
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "prose"))
            .expect("the prose is drawn on the page itself")
            .rect[0]
    };
    let fence_left = |offsets: &[f32]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        );
        let block = built
            .blocks
            .first()
            .expect("a block wider than its page scrolls inside itself")
            .clone();
        // The fence's own ground, which is the second quad it draws.
        (block.quads[0].rect[0], block.clip)
    };

    // ① Clamped at both ends, in the block's own units.
    let (at_rest, clip) = fence_left(&[0.0, 0.0]);
    let (at_end, _) = fence_left(&[0.0, overflow]);
    let (past_end, _) = fence_left(&[0.0, overflow * 10.0]);
    let (before_start, _) = fence_left(&[0.0, -50.0]);
    assert_eq!(at_rest - at_end, overflow, "the block travels its overflow");
    assert_eq!(past_end, at_end, "and stops at its own end");
    assert_eq!(before_start, at_rest, "and at its own start");

    // ② The region is cropped to the block's rectangle, so what it scrolls
    //    cannot print over the page.
    assert_eq!(clip[0], body[0] + metrics.padding_x);
    assert_eq!(clip[2], clip[0] + page);

    // ②(b) **The indicator belongs to the block it was pushed with.** A page
    //    scrolled past its first wide block still has one region and one
    //    indicator, and the pair must be the same pair — the version that
    //    re-derived the list and zipped it drew every indicator one block out
    //    as soon as anything above had scrolled off the top.
    let tall: preview_viewport::Layout = vec![
        MarkdownBlockLayout::solid(1000.0),
        MarkdownBlockLayout {
            width: wide,
            top: 1000.0,
            ..MarkdownBlockLayout::solid(metrics.line_height * 3.0)
        },
    ]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 1000.0],
        rested_bars(&[0.0, overflow]),
        (&blocks, &tall),
        &palette,
    );
    let block = built.blocks.first().expect("the wide block is on screen");
    let thumb = block.quads.last().expect("an indicator has a thumb");
    assert!(
        thumb.rect[0] > block.clip[0],
        "the thumb of a block scrolled to its end sits away from the left edge"
    );

    // ③ **The prose does not move**, at any offset the block can hold.
    assert_eq!(prose_left(&[0.0, 0.0]), prose_left(&[0.0, overflow]));
}

/// PIN (user report, 2026-08-13) — **a block is built at its own full width
/// and cropped when it is drawn.**
///
/// The report: drag a code fence's thumb to the right and the fence goes
/// blank but for a sliver of glyphs against its left edge. The cause was
/// that a scrolling block's inner frame was **one page wide and slid left**
/// (`right - offset`), so every rectangle inside it was laid out in a
/// window that walked off its own clip. A fence is one paragraph spanning
/// the whole line, so its single box left the clip bodily and
/// `shape_preview_body`'s `crop_to` — correctly — drew nothing of it. A
/// table survived the same offset only because [`push_markdown_table`] lays
/// its cells out from the block's *origin* and never reads the frame's
/// right edge at all: a reprieve its structure happened to grant, not a
/// rule, which is why both are asserted here.
///
/// The frame is now the content's width placed at the offset, and the
/// cropping is where cropping belongs — at the draw, in `crop_to`, which
/// stays exactly as it is. That gate is for inverted and `NaN` boxes; a
/// block scrolled to its own end is neither, and it was never the thing
/// `crop_to` was put there to stop.
///
/// MUTATION: put the crop back at build time — restore `right - offset` as
/// the frame's right edge — and ① and ② both go red.
#[test]
fn a_scrolled_block_keeps_its_whole_width_and_is_cropped_only_when_drawn() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 300.0, 400.0];
    let page = body[2] - body[0] - metrics.padding_x * 2.0;
    let wide = page + 400.0;
    let overflow = wide - page;
    let line = "a fence line far wider than the page it is standing on";
    let cells = |text: &str| {
        vec![
            vec![preview::Span::plain("head")],
            vec![preview::Span::plain(text)],
        ]
    };
    let blocks = [
        preview::MarkdownBlock::Code {
            lang: None,
            text: line.to_owned(),
        },
        preview::MarkdownBlock::Table {
            rows: vec![cells("first"), cells("last")],
            alignments: vec![bt_detect::table::ColumnAlignment::None; 1],
        },
    ];
    let fence_height =
        metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.line_height;
    let layout: preview_viewport::Layout = [
        MarkdownBlockLayout {
            width: wide,
            ..MarkdownBlockLayout::solid(fence_height)
        },
        MarkdownBlockLayout {
            width: wide,
            top: fence_height + metrics.paragraph_gap,
            columns: vec![80.0, wide - 80.0],
            ..MarkdownBlockLayout::rows(
                vec![metrics.line_height, metrics.line_height],
                metrics.table_border,
            )
        },
    ]
    .into();
    let render = |offsets: &[f32]| {
        markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(offsets),
            (&blocks, &layout),
            &palette,
        )
    };
    let fence_line = |built: &bt_render::PreviewBody| {
        built.blocks[0]
            .paragraphs
            .iter()
            .find(|paragraph| {
                paragraph
                    .runs
                    .iter()
                    .any(|run| run.text.contains("fence line"))
            })
            .expect("a fence draws its line")
            .clone()
    };
    // Both blocks driven to the far end of their own travel — the gesture
    // the report is about — against the same document at rest.
    let at_rest = render(&[0.0, 0.0]);
    let built = render(&[overflow, overflow]);
    let region = |index: usize| {
        built
            .blocks
            .get(index)
            .unwrap_or_else(|| panic!("block {index} is wide and scrolls inside itself"))
    };

    // ① The fence's line is still there, whole, and covering the window.
    let fence = region(0);
    let drawn = fence_line(&built);
    assert_eq!(
        drawn
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        line,
        "and it is the whole line — the text is never cut to what fits"
    );
    let visible = bt_render::crop_to(drawn.rect, fence.clip)
        .expect("its box still meets the block's own rectangle");
    assert_eq!(
        [visible[0], visible[2]],
        [fence.clip[0], fence.clip[2]],
        "covering the window edge to edge, rather than surviving as a sliver"
    );
    // And what stands at the window's left edge is the far end of the line:
    // the box has travelled its whole overflow, and only translated —
    // nothing about it narrowed on the way.
    let resting = fence_line(&at_rest);
    assert_eq!(
        resting.rect[0] - drawn.rect[0],
        overflow,
        "the line has travelled its whole overflow"
    );
    assert_eq!(
        drawn.rect[2] - drawn.rect[0],
        resting.rect[2] - resting.rect[0],
        "and it is the same box that set out — a translation, not a squeeze"
    );
    assert!(
        drawn.rect[0] < fence.clip[0] && drawn.rect[2] >= fence.clip[2],
        "so the window is looking into the middle of it, with the tail \
             still reaching the far edge"
    );

    // ② The table, at the same offset, by the same rule — it looked right
    //    before only by the accident of being made of small boxes.
    let table = region(1);
    let last = table
        .paragraphs
        .iter()
        .find(|paragraph| paragraph.runs.iter().any(|run| run.text == "last"))
        .expect("the last row's wide cell is still drawn");
    assert!(
        bt_render::crop_to(last.rect, table.clip).is_some(),
        "and it stands inside the window a scroll to the end opened on it"
    );

    // ③ At rest, nothing has been glued open: the window starts at the
    //    line's own beginning.
    assert!(
        resting.rect[0] >= at_rest.blocks[0].clip[0],
        "an unscrolled fence begins inside its own window"
    );
}

/// PIN — **the painter puts the document in the measure's column**, centred
/// when the pane can afford it and flush to the pane when it cannot
/// (§7.1.3i; user report 2026-08-16).
///
/// The geometry itself is `preview::markdown_measure_box`'s and is pinned
/// beside it; what is asserted here is that the *painter* asks — a body that
/// went on computing `body[0] + padding_x` for itself would draw the prose
/// across a maximised window while the layout pass wrapped it at the
/// measure, which
/// is a paragraph that reserves four rows and paints two.
///
/// MUTATION: put `let left = body[0] + metrics.padding_x` back at the top of
/// `build_preview_markdown_body` and the wide case goes red.
#[test]
fn a_pane_wider_than_the_measure_is_painted_into_a_centred_column() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = preview::parse_markdown("Prose enough to have an edge.\n");
    let layout: preview_viewport::Layout =
        vec![MarkdownBlockLayout::solid(metrics.line_height)].into();
    let column = |body: [f32; 4]| {
        let built = markdown_body(
            body,
            metrics,
            [0.0, 0.0],
            rested_bars(&[]),
            (&blocks, &layout),
            &palette,
        );
        let rect = built.paragraphs[0].rect;
        (rect[0], rect[2])
    };

    let narrow = [0.0, 0.0, 500.0, 600.0];
    assert_eq!(
        column(narrow),
        (metrics.padding_x, 500.0 - metrics.padding_x),
        "a pane under the measure keeps the pane, exactly as before"
    );

    let wide = [0.0, 0.0, 1601.0, 600.0];
    let (left, right) = column(wide);
    assert_eq!(right - left, metrics.measure, "the column stops growing");
    assert_eq!(
        left - wide[0],
        wide[2] - right,
        "and it is centred in the pane"
    );
}

/// PIN — **a fence is set at 85% of the body on its own 1.45 leading**
/// (`pre { font-size: 85%; line-height: 1.45; padding: 16px }`, §7.1.3i).
///
/// The height the layout pass reserved is `code_line_height` a row, so a
/// painter still stepping by the body's `line_height` would draw the last
/// line of a long fence outside its own ground.
///
/// MUTATION: put `metrics.line_height` back on either the paragraph or the
/// step and the rows stop landing on the reservation.
#[test]
fn a_fence_is_set_at_its_own_size_and_stepped_at_its_own_leading() {
    let palette = bt_render::chrome_palette();
    let metrics = seats::preview_markdown_metrics(1.0);
    let body = [0.0, 0.0, 600.0, 600.0];
    let blocks = preview::parse_markdown("```\nfn a() {}\nfn b() {}\n```\n");
    let rows = 2.0;
    let layout: preview_viewport::Layout = vec![MarkdownBlockLayout::solid(
        metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.code_line_height * rows,
    )]
    .into();
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (&blocks, &layout),
        &palette,
    );
    assert_eq!(built.paragraphs.len(), 2, "one paragraph per fenced line");
    assert!(metrics.code_font < metrics.font_size, "85%, not 100%");
    for paragraph in &built.paragraphs {
        assert_eq!(paragraph.font_size_px, metrics.code_font);
        assert_eq!(paragraph.line_height_px, metrics.code_line_height);
        assert!(!paragraph.wrap, "and it still refuses to reflow");
    }
    assert_eq!(
        built.paragraphs[1].rect[1] - built.paragraphs[0].rect[1],
        metrics.code_line_height,
        "the rows are stepped at the fence's own leading"
    );
    let ground = built.paragraphs[0].rect[1];
    assert_eq!(
        ground,
        body[1] + metrics.padding_y + metrics.code_border + metrics.code_padding_y,
        "and the first one starts inside a 1em pad"
    );
}

/// PIN — a code fence is a box with a ground, and its language rides the
/// top-right corner of that box (mock-up 1202-1211).
///
/// Mutation: drop the `align_right` on the language tag, which parks it over
/// the first line of the code.
#[test]
fn a_code_fence_is_a_box_and_its_language_sits_in_the_corner() {
    let palette = bt_render::chrome_palette();
    let body = [0.0, 0.0, 600.0, 400.0];
    let metrics = seats::preview_markdown_metrics(1.0);
    let blocks = preview::parse_markdown("```rust\nlet x = 1;\n```\n");
    let height = metrics.code_border * 2.0 + metrics.code_padding_y * 2.0 + metrics.line_height;
    let built = markdown_body(
        body,
        metrics,
        [0.0, 0.0],
        rested_bars(&[]),
        (
            &blocks,
            &preview_viewport::Layout::from([MarkdownBlockLayout {
                top: 0.0,
                height,
                ..MarkdownBlockLayout::default()
            }]),
        ),
        &palette,
    );
    let border = built
        .quads
        .iter()
        .find(|quad| quad.color == palette.preview_code_border)
        .expect("the fence has a border");
    let ground = built
        .quads
        .iter()
        .find(|quad| quad.color == palette.preview_code_ground)
        .expect("and a ground inside it");
    assert!(
        ground.rect[0] > border.rect[0] && ground.rect[2] < border.rect[2],
        "the ground is inset by the border"
    );
    let lang = built
        .paragraphs
        .iter()
        .find(|p| p.runs[0].color == palette.preview_code_lang)
        .expect("the language tag is drawn");
    assert_eq!(lang.runs[0].text, "RUST", "and it is upper-cased");
    assert!(lang.align_right, "in the corner, not over the code");
    assert_eq!(lang.letter_spacing_em, seats::PREVIEW_MD_LANG_TRACKING_EM);
    assert_eq!(lang.rect[2], border.rect[2] - metrics.lang_inset_right);
    assert!(
        built.paragraphs.iter().any(|p| {
            p.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                == "let x = 1;"
                && p.runs.iter().all(|run| run.mono)
        }),
        "the code itself is monospace"
    );
}
