//! **The replay probe**, moved with [`bt_render_probe::HeadlessRenderProbe`] out of `bt-render`'s
//! unit tests (thread-door note 2026-09-26, revision (l)4): the test and its claims are the ones
//! `bt-render` held, read through the probe's public road.

#![allow(clippy::disallowed_methods)]

use std::num::NonZeroU32;

use bt_render::CellMetrics;
use bt_render_probe::HeadlessRenderProbe;
use bt_transcript::CapturedCell;
use bt_viewport::horizontal::HorizontalProjection;
use bt_viewport::{FrameViewportOrigin, SUBPIXELS_PER_PX, ViewportFrame};

/// One live cell holding `x`, the cursor on it, one row at `metrics`' height — the frame
/// `bt-render`'s tests call `single_cell_cursor_frame`, built here from the public types.
fn single_cell_cursor_frame(metrics: CellMetrics) -> ViewportFrame {
    let anchor = bt_doc::ContentAnchor::Live {
        screen: bt_doc::ScreenId::Primary,
        point: bt_doc::GridPoint { row: 0, column: 0 },
        bias: bt_doc::Bias::Before,
        generation: bt_doc::GridGeneration(1),
    };
    let height = (metrics.cell_height_px * SUBPIXELS_PER_PX as f32).round() as i64;
    ViewportFrame {
        columns: NonZeroU32::new(1).unwrap(),
        horizontal: HorizontalProjection::unscrolled(1),
        grid_rows: NonZeroU32::new(1).unwrap(),
        rows: NonZeroU32::new(1).unwrap(),
        presentation_offset_subpixels: 0,
        cells: vec![CapturedCell::plain("x")],
        cursor: bt_viewport::GridCursor {
            row: 0,
            column: 0,
            visible: true,
        },
        cursor_in_view: true,
        cell_anchors: vec![bt_viewport::CellAnchor {
            start: anchor.clone(),
            end: anchor,
        }],
        row_map: vec![bt_viewport::FrameVisualRow {
            top_subpixels: 0,
            height_subpixels: height,
            live_grid_row: Some(0),
            continues: false,
            source_ends: None,
        }],
        selection_spans: Vec::new(),
        search_spans: Vec::new(),
        current_search_spans: Vec::new(),
        math_blocks: Vec::new(),
        math_failures: Vec::new(),
        status_text: None,
        viewport_origin: FrameViewportOrigin::Bottom,
        scroll_offset_rows: 0,
        layout_key: bt_doc::LayoutKey {
            width_cells: NonZeroU32::new(1).unwrap(),
            dpi_milli: NonZeroU32::new(1000).unwrap(),
            font_size_subpixels: 16 * 1024,
            font_rev: 1,
            theme_rev: 1,
            lang_rev: 0,
            profile_rev: 0,
            line_wrapping: true,
        },
        view_generation: bt_doc::ViewGeneration(1),
    }
}

/// The replay probe is the two layers and nothing else.
#[test]
fn the_replay_probe_is_a_context_and_an_offscreen_window() {
    let mut probe =
        pollster::block_on(HeadlessRenderProbe::new(400, 300, 1.0)).expect("a headless probe");
    assert!(!probe.adapter_name().is_empty());
    assert!(probe.max_texture_dimension_2d() > 0);
    let frame = single_cell_cursor_frame(probe.base_metrics());
    let sample = probe.prepare_frame(&frame).expect("one replayed frame");
    assert!(sample.narrow_glyphs > 0);
    assert_eq!(sample.row_cache_misses, 1);
    let metrics = probe
        .update_scale_factor(2.0)
        .expect("the probe re-measures like any window");
    assert_eq!(metrics.scale_factor, 2.0);
}
