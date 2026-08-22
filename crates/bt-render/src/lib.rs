//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

mod contrast;
mod ground;
mod procedural;
mod rounded_rect;
mod scheme;
mod theme;

use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    mem::size_of,
    num::{NonZeroI64, NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_doc::{ContentAnchor, MathMode, ScreenId};
use bt_transcript::{CapturedCell, CellFlags, CellStyle, CellText, TerminalColor};
use bt_unicode::{cluster_width, graphemes};
#[cfg(test)]
use bt_viewport::FrameViewportOrigin;
use bt_viewport::MATH_TEXTURE_CACHE_BUDGET_BYTES;
use bt_viewport::{
    FrameShapeError, MathBlockAnchor, MathBlockDisplay, MathBlockPlacement, SUBPIXELS_PER_PX,
    SelectionSpan, ViewportFrame,
};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, PrepareError, Resolution, Shaping,
    Stretch, Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
    Wrap,
    cosmic_text::{Fallback, FeatureTag, FontFeatures},
};
use thiserror::Error;
use unicode_properties::emoji::{EmojiStatus, UnicodeEmoji};
use wgpu::util::DeviceExt;

pub use contrast::{
    ALWAYS_REACHABLE_RATIO_LIMIT, MinimumContrast, contrast_ratio, current_minimum_contrast,
    relative_luminance, set_minimum_contrast,
};
pub use ground::{
    BackgroundFit, BackgroundImage, MINIMUM_GROUND_ALPHA, WindowGround, background_uv_rect,
    premultiplied_clear, set_window_ground, window_ground,
};
use rounded_rect::{
    rounded_rect_coverage, rounded_rect_halo_coverage, rounded_rect_shadow_coverage,
};
pub use scheme::{ColourScheme, FOLIO_DARK, FOLIO_LIGHT, selection_from_accent};
pub use theme::{
    CURSOR_BAR_WIDTH_LOGICAL_PX, ChromePalette, CursorStyle, DARK_CHROME, DEFAULT_BACKGROUND_RGB,
    DEFAULT_FOCUS_MINI_HEIGHT_LOGICAL_PX, DOCK_DASH_RATIO, DOCK_PREVIEW_BORDER_LOGICAL_PX,
    DOCK_PREVIEW_FILL_ALPHA, DOCK_PREVIEW_FONT_LOGICAL_PX, DOCK_PREVIEW_LETTER_SPACING_EM,
    DOCK_PREVIEW_RADIUS_LOGICAL_PX, DOCK_SHIFT_BORDER_ALPHA, DOCK_SHIFT_FILL_ALPHA,
    DOCK_SHIFT_INSET_LOGICAL_PX, DOCK_SHIFT_RADIUS_LOGICAL_PX, DRAG_GHOST_BORDER_LOGICAL_PX,
    DRAG_GHOST_FONT_LOGICAL_PX, DRAG_GHOST_GAP_LOGICAL_PX, DRAG_GHOST_PADDING_X_LOGICAL_PX,
    DRAG_GHOST_PADDING_Y_LOGICAL_PX, DRAG_GHOST_POINTER_OFFSET_LOGICAL_PX,
    DRAG_GHOST_RADIUS_LOGICAL_PX, FLOAT_WINDOW_ANIMATION_MS, FLOAT_WINDOW_BORDER_LOGICAL_PX,
    FLOAT_WINDOW_DRAG_MARGIN_LOGICAL_PX, FLOAT_WINDOW_FOOT_LOGICAL_PX,
    FLOAT_WINDOW_GRIP_LOGICAL_PX, FLOAT_WINDOW_HEAD_LOGICAL_PX, FLOAT_WINDOW_MAX_HEIGHT_LOGICAL_PX,
    FLOAT_WINDOW_MAX_HEIGHT_VIEWPORT_FRACTION, FLOAT_WINDOW_MIN_HEIGHT_LOGICAL_PX,
    FLOAT_WINDOW_MIN_STRIP_LOGICAL_PX, FLOAT_WINDOW_MIN_WIDTH_LOGICAL_PX,
    FLOAT_WINDOW_RADIUS_LOGICAL_PX, FLOAT_WINDOW_RISE_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX,
    FLOAT_WINDOW_TRIGGER_GAP_LOGICAL_PX, FLOAT_WINDOW_VIEWPORT_MARGIN_LOGICAL_PX,
    FLOAT_WINDOW_WIDTH_LOGICAL_PX, FOCUS_CARD_BORDER_LOGICAL_PX, FOCUS_CARD_CLOSE_BOX_LOGICAL_PX,
    FOCUS_CARD_CLOSE_GLYPH_LOGICAL_PX, FOCUS_CARD_CLOSE_RADIUS_LOGICAL_PX,
    FOCUS_CARD_FONT_LOGICAL_PX, FOCUS_CARD_GAP_LOGICAL_PX, FOCUS_CARD_HEAD_GAP_LOGICAL_PX,
    FOCUS_CARD_HEAD_PADDING_X_LOGICAL_PX, FOCUS_CARD_HEAD_PADDING_Y_LOGICAL_PX,
    FOCUS_CARD_HEIGHT_OPTIONS_LOGICAL_PX, FOCUS_CARD_PIN_BOX_LOGICAL_PX,
    FOCUS_CARD_RADIUS_LOGICAL_PX, FOCUS_CARD_WAIT_HALO_LOGICAL_PX, FOCUS_CARD_WAIT_HALO_OPACITY,
    FOCUS_COLUMN_WIDTH_LOGICAL_PX, FOCUS_MINI_BORDER_LOGICAL_PX, FOCUS_MINI_FILES_FONT_LOGICAL_PX,
    FOCUS_MINI_FILES_ICON_LOGICAL_PX, FOCUS_MINI_FILES_INDENT_LOGICAL_PX,
    FOCUS_MINI_FILES_LINE_HEIGHT, FOCUS_MINI_FILES_ROW_GAP_LOGICAL_PX, FOCUS_MINI_GAP_LOGICAL_PX,
    FOCUS_MINI_PADDING_LOGICAL_PX, FOCUS_MINI_RADIUS_LOGICAL_PX,
    FOCUS_MINI_ROW_PADDING_BOTTOM_LOGICAL_PX, FOCUS_MINI_ROW_PADDING_TOP_LOGICAL_PX,
    FOCUS_MINI_ROW_PADDING_X_LOGICAL_PX, FOCUS_MINI_SEAM_ALPHA, FOCUS_MINI_SEAM_LOGICAL_PX,
    FOCUS_MINI_TERM_FONT_LOGICAL_PX, FOCUS_MINI_TERM_LINE_HEIGHT, GRAPH_LANE_COUNT,
    LIGHT_BACKGROUND_RGB, LIGHT_CHROME, PANE_HEAD_FILE_MARK_LOGICAL_PX,
    PANE_HEAD_FOLDER_MARK_LOGICAL_PX, PANE_HEAD_PROFILE_MARK_LOGICAL_PX,
    PREVIEW_BODY_INSET_LOGICAL_PX, RAIL_BORDER_LOGICAL_PX, RAIL_GAP_LOGICAL_PX,
    RAIL_LABEL_FONT_LOGICAL_PX, RAIL_LABEL_LINE_LOGICAL_PX, RAIL_LABEL_PADDING_BOTTOM_LOGICAL_PX,
    RAIL_LABEL_PADDING_TOP_LOGICAL_PX, RAIL_LABEL_PADDING_X_LOGICAL_PX, RAIL_LABEL_TRACKING_EM,
    RAIL_NEW_CHEVRON_BOX_LOGICAL_PX, RAIL_NEW_GAP_LOGICAL_PX, RAIL_NEW_MAIN_PADDING_X_LOGICAL_PX,
    RAIL_NEW_MARGIN_TOP_LOGICAL_PX, RAIL_NEW_STICKY_PADDING_BOTTOM_LOGICAL_PX,
    RAIL_PADDING_BOTTOM_LOGICAL_PX, RAIL_PADDING_TOP_LOGICAL_PX, RAIL_PADDING_X_LOGICAL_PX,
    RAIL_PARK_LOGICAL_PX, RAIL_SEAM_INSET_X_LOGICAL_PX, RAIL_SEAM_MARGIN_Y_LOGICAL_PX,
    RAIL_SEAM_THICKNESS_LOGICAL_PX, RAIL_SHADE_WIDTH_LOGICAL_PX, RAIL_TAB_FONT_LOGICAL_PX,
    RAIL_TAB_GAP_LOGICAL_PX, RAIL_TAB_HEIGHT_LOGICAL_PX, RAIL_TAB_PADDING_LEFT_LOGICAL_PX,
    RAIL_TAB_PADDING_RIGHT_LOGICAL_PX, RAIL_TAB_PARKED_PADDING_X_LOGICAL_PX,
    RAIL_TAB_RADIUS_LOGICAL_PX, RAIL_TEXT_FADE_MS, RAIL_TEXT_FADE_OPEN_DELAY_MS,
    RAIL_TRANSITION_MS, RAIL_WIDTH_LOGICAL_PX, SEAT_DIVIDER_GRIP_LENGTH_LOGICAL_PX,
    SEAT_DIVIDER_GRIP_RADIUS_LOGICAL_PX, SEAT_DIVIDER_GRIP_THICKNESS_LOGICAL_PX,
    SEAT_DIVIDER_HIT_LOGICAL_PX, SEAT_DIVIDER_VISUAL_LOGICAL_PX, SEAT_PANE_CLOSE_BOX_LOGICAL_PX,
    SEAT_PANE_CLOSE_GLYPH_LOGICAL_PX, SEAT_PANE_CLOSE_RADIUS_LOGICAL_PX,
    SEAT_RESIZING_CARD_MARGIN_LOGICAL_PX, SEAT_RESIZING_CARD_RADIUS_LOGICAL_PX,
    SEAT_TITLE_BAR_LOGICAL_PX, SEAT_TITLE_EDGE_LOGICAL_PX, SEAT_TITLE_FONT_LOGICAL_PX,
    SEAT_TITLE_GAP_LOGICAL_PX, SEAT_TITLE_PADDING_LOGICAL_PX,
    SEAT_TITLE_TRAILING_PADDING_LOGICAL_PX, TERMINAL_SCROLL_LANE_LOGICAL_PX, Theme, ThemeChange,
    WINDOW_CAPTION_BUTTON_LOGICAL_PX, WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX,
    WINDOW_CAPTION_GLYPH_LOGICAL_PX, WINDOW_NEW_TAB_BOX_LOGICAL_PX,
    WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX, WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX,
    WINDOW_NEW_TAB_GLYPH_LOGICAL_PX, WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX,
    WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX, WINDOW_NEW_TAB_RADIUS_LOGICAL_PX,
    WINDOW_TAB_BADGE_FONT_LOGICAL_PX, WINDOW_TAB_BADGE_HEIGHT_LOGICAL_PX,
    WINDOW_TAB_BADGE_MIN_WIDTH_LOGICAL_PX, WINDOW_TAB_BADGE_PADDING_X_LOGICAL_PX,
    WINDOW_TAB_BADGE_RADIUS_LOGICAL_PX, WINDOW_TAB_BREATHE_MIN_OPACITY,
    WINDOW_TAB_BREATHE_PERIOD_MS, WINDOW_TAB_BREATHE_REDUCED_OPACITY,
    WINDOW_TAB_CLOSE_BOX_LOGICAL_PX, WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX,
    WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX, WINDOW_TAB_DEAD_MARK_OPACITY, WINDOW_TAB_FONT_LOGICAL_PX,
    WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX, WINDOW_TAB_GAP_LOGICAL_PX, WINDOW_TAB_HEIGHT_LOGICAL_PX,
    WINDOW_TAB_MARK_LOGICAL_PX, WINDOW_TAB_MAX_WIDTH_LOGICAL_PX, WINDOW_TAB_MIN_WIDTH_LOGICAL_PX,
    WINDOW_TAB_PADDING_LEFT_LOGICAL_PX, WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX,
    WINDOW_TAB_PIN_FADE_MS, WINDOW_TAB_PIN_REVEAL_MS, WINDOW_TAB_RADIUS_LOGICAL_PX,
    WINDOW_TAB_RING_INDETERMINATE_TURNS, WINDOW_TAB_RING_RADIUS_LOGICAL_PX,
    WINDOW_TAB_RING_SPIN_PERIOD_MS, WINDOW_TAB_RING_STROKE_LOGICAL_PX,
    WINDOW_TAB_RING_SWEEP_TRANSITION_MS, WINDOW_TAB_SQUEEZED_LOGICAL_PX,
    WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX, WINDOW_TAB_STATUS_DOT_LOGICAL_PX,
    WINDOW_TAB_STATUS_DOT_RIGHT_LOGICAL_PX, WINDOW_TAB_STATUS_DOT_TOP_LOGICAL_PX,
    WINDOW_TAB_TIGHT_LOGICAL_PX, WINDOW_TITLE_BAR_LOGICAL_PX, background_rgb, chrome_palette,
    current_cursor_style, current_theme, foreground_rgb, ink_over, set_cursor_style, set_theme,
    theme_revision,
};
use theme::{
    CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX, DEFAULT_DIM_FOREGROUND_RGB, ansi_16_rgb, cursor_rgb,
    unfocused_cursor_rgb,
};
use theme::{
    DEFAULT_STATUS_BACKGROUND_RGB, search_current_ink_rgb, search_current_rgb, search_match_rgb,
    selection_background_rgb,
};
pub use theme::{background_is_light, ink_over_bp, scheme_in_force, schemes_in_force, set_schemes};

/// `mark.srch { border-radius: 3px }` (mock-up 1530) — the corner a found word
/// wears, and the one thing that tells it apart from a dragged selection at a
/// glance.
pub const SEARCH_MATCH_RADIUS_LOGICAL_PX: f32 = 3.0;

/// The grid's font size, in logical pixels, when nothing has chosen one.
///
/// It was a `const` and is now a *default*, because the Appearance block's Font
/// size row makes it a runtime value ([`GpuContext::set_terminal_font`]). The
/// number is unchanged: 16 is what every frame this product has drawn was drawn
/// at, and `bt_persist::DEFAULT_TERMINAL_FONT_SIZE` is the same 16 written down
/// on the persistence side.
pub const DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX: f32 = 16.0;
/// How tall a row is as a multiple of the font size — 22 over 16, the pair this
/// renderer has always used.
///
/// A **ratio** and not a second size, and that is the whole of what makes the
/// Font size row work: a fixed 22-pixel line under a 24-pixel face would clip
/// every descender, and a fixed 22 under a 10-pixel face would draw a grid of
/// mostly air. At 16 it reproduces the old 22.0 exactly, so no existing frame
/// moves by a pixel.
pub const LINE_HEIGHT_TO_FONT_SIZE_RATIO: f32 = 22.0 / 16.0;
const PADDING_LOGICAL_PX: f32 = 8.0;
const NARROW_SHAPING_CACHE_BUDGET_BYTES: usize = 8 * 1024 * 1024;
const WIDE_SHAPING_CACHE_BUDGET_BYTES: usize = 16 * 1024 * 1024;
const COMPOSED_ROW_CACHE_BUDGET_BYTES: usize = 32 * 1024 * 1024;
const MATH_TOOL_BUTTON_LOGICAL_PX: f32 = 22.0;
const MATH_TOOL_GAP_LOGICAL_PX: f32 = 2.0;
/// A rendered formula's raster is cropped tight to its ink, so its glyphs would touch the pane
/// edge while a text row's characters sit inside their cell with natural left bearing. This small
/// indent gives the ink the same visual left edge as the text above it (user report 2026-07-20).
/// Applied to rendered blocks only - a source block already carries its own column offset.
const MATH_LEFT_INDENT_LOGICAL_PX: f32 = 8.0;

fn math_toolbar_vertical_bounds(visible_top: f32, visible_bottom: f32, scale: f32) -> (f32, f32) {
    let band_height = (visible_bottom - visible_top).max(0.0);
    let button = (MATH_TOOL_BUTTON_LOGICAL_PX * scale).min(band_height);
    let top = visible_top + (band_height - button) / 2.0;
    (top, top + button)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathHitTarget {
    Block,
    ToggleSource,
    CopyLatex,
    Failure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MathHit {
    pub anchor: MathBlockAnchor,
    pub target: MathHitTarget,
}

/// Which inline run of this placement the pointer is on.
///
/// An inline placement is one composite covering a whole line, so the block rectangle alone cannot
/// say which of two formulas was clicked. The runs' own offsets can, scaled the same way the block
/// is. A point that lands in the prose *between* two runs still belongs to one of them — it is
/// inside the block, and the nearest run is the only non-arbitrary answer — so containment is
/// tried first and proximity settles the rest. Display math has no runs and yields `None`.
fn inline_run_at(placement: &MathBlockPlacement, block: [f32; 4], x: f32) -> Option<u32> {
    let scale = placement.artifact.render_scale_milli as f32 / 1000.0;
    let extent = |run: &bt_viewport::InlineRunPlacement| {
        let left = block[0] + run.x_px as f32 * scale;
        (left, left + (run.width_px as f32 * scale).max(1.0))
    };
    placement
        .artifact
        .inline_runs
        .iter()
        .find(|run| {
            let (left, right) = extent(run);
            x >= left && x < right
        })
        .or_else(|| {
            placement.artifact.inline_runs.iter().min_by(|left, right| {
                let distance = |run: &bt_viewport::InlineRunPlacement| {
                    let (start, end) = extent(run);
                    (start - x).max(x - end).max(0.0)
                };
                distance(left).total_cmp(&distance(right))
            })
        })
        .map(|run| run.run)
}

#[derive(Clone, Copy, Debug)]
struct MathBlockGeometry {
    block: [f32; 4],
    clip: [f32; 4],
    eye: Option<[f32; 4]>,
    copy: Option<[f32; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StatusOverlayGeometry {
    rect: [f32; 4],
    first_column: usize,
}
/// Width of a math block's overflow fade, in cells — wide enough to read as a soft edge rather
/// than a drawn border, narrow enough to hide only a character or so of the formula.
const MATH_OVERFLOW_FADE_CELLS: f32 = 1.5;
/// The fade never eats more than this share of a narrow band from either side.
const MATH_OVERFLOW_FADE_MAX_BAND_FRACTION: f32 = 0.25;
/// Slabs per fade. Enough that the ramp reads as continuous at any cell width.
const MATH_OVERFLOW_FADE_STEPS: usize = 16;

/// Geometry of a math block's overflow fades: one `(rect, coverage)` slab per ramp step, for
/// whichever sides still hold content the band is cutting off. Split out from the renderer so the
/// decision is testable without a GPU, as with the other overlay geometry in this file.
fn math_overflow_fade_slabs(
    metrics: CellMetrics,
    placement: &MathBlockPlacement,
    geometry: &MathBlockGeometry,
) -> Vec<([f32; 4], f32)> {
    if placement.display != MathBlockDisplay::Rendered {
        return Vec::new();
    }
    let [visible_left, top, visible_right, bottom] = geometry.clip;
    if bottom <= top || visible_right <= visible_left {
        return Vec::new();
    }
    let scaled_width =
        placement.artifact.width_px as f32 * placement.artifact.render_scale_milli as f32 / 1000.0;
    let content_left = math_block_left_px(metrics, placement.left_subpixels, true)
        - placement.horizontal_scroll_px as f32;
    let content_right = content_left + scaled_width;

    // A pixel of slack: a formula sitting flush against its band is not overflowing it.
    let hidden_left = content_left < visible_left - 1.0;
    let hidden_right = content_right > visible_right + 1.0;
    if !hidden_left && !hidden_right {
        return Vec::new();
    }
    // Never let the two fades meet in the middle of a narrow band and swallow the formula.
    let width = (metrics.cell_width_px * MATH_OVERFLOW_FADE_CELLS)
        .min((visible_right - visible_left) * MATH_OVERFLOW_FADE_MAX_BAND_FRACTION);
    if width <= 0.0 {
        return Vec::new();
    }

    let steps = MATH_OVERFLOW_FADE_STEPS;
    let mut slabs = Vec::with_capacity(steps * 2);
    for step in 0..steps {
        let near = step as f32 / steps as f32;
        let far = (step + 1) as f32 / steps as f32;
        // Opaque against the cut edge and gone by the inner end, squared so the formula's own
        // ink stays readable well before the edge instead of greying out evenly.
        let strength = 1.0 - (near + far) / 2.0;
        let coverage = strength * strength;
        if hidden_left {
            slabs.push((
                [
                    visible_left + width * near,
                    top,
                    visible_left + width * far,
                    bottom,
                ],
                coverage,
            ));
        }
        if hidden_right {
            slabs.push((
                [
                    visible_right - width * far,
                    top,
                    visible_right - width * near,
                    bottom,
                ],
                coverage,
            ));
        }
    }
    slabs
}

/// The family the grid is drawn in when nothing has chosen one, and the family
/// this renderer's fixed startup file list actually loads.
///
/// A *default* rather than *the* primary family since the Terminal font row
/// landed: what "primary" means at any moment is whatever
/// `fontdb`'s `Family::Monospace` currently resolves to, which
/// [`GpuContext::set_terminal_font`] moves. Every place that used to compare
/// against this constant now asks the database instead — see
/// [`primary_font_family`] — because a comparison against a constant would
/// answer "no" for the face actually on screen.
const DEFAULT_PRIMARY_FONT_FAMILY: &str = "Consolas";

/// The family `Family::Monospace` resolves to right now.
///
/// One function rather than a field, because `fontdb` already stores the answer
/// and a second copy is a second thing that can go stale. It is the grid's face
/// and never the chrome's: the window's own labels ask for `Family::SansSerif`,
/// which nothing in this slice moves.
fn primary_font_family(font_system: &FontSystem) -> &str {
    font_system.db().family_name(&Family::Monospace)
}
const COLOR_EMOJI_FONT_FAMILY: &str = "Noto Color Emoji";
const SEGOE_COLOR_EMOJI_FONT_FAMILY: &str = "Segoe UI Emoji";
const TEXT_SYMBOL_FONT_FAMILY: &str = "Segoe UI Symbol";
const NARROW_FALLBACK_SIDE_BEARING_EM: f32 = 0.05;
const DOTTED_UNDERLINE_SEGMENT_LOGICAL_PX: f32 = 2.0;
const NOTO_COLOR_EMOJI_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/NotoColorEmoji_WindowsCompatible.ttf"
));

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub cell_width_px: f32,
    pub cell_height_px: f32,
    pub font_size_px: f32,
    pub padding_px: f32,
    pub scale_factor: f64,
    ascii_baseline_px: f32,
    primary_advance_px: f32,
    primary_cap_height_px: f32,
    primary_cap_center_y_px: f32,
}

impl CellMetrics {
    /// Measure the grid at the renderer's default face size.
    ///
    /// Kept as its own entry point because the default size is what every test
    /// and every headless probe means by "the grid", and threading a size
    /// through thirty call sites to say 16 each time would bury the one place
    /// where the number is not 16.
    fn measure(font_system: &mut FontSystem, scale_factor: f64) -> Result<Self, RenderError> {
        Self::measure_at(
            font_system,
            scale_factor,
            DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX,
        )
    }

    /// Measure the grid at a chosen face size, in logical pixels.
    ///
    /// The row height comes out of [`LINE_HEIGHT_TO_FONT_SIZE_RATIO`] rather
    /// than from a constant, so a larger face gets a taller row and the
    /// descenders that a fixed row height would clip stay inside the cell.
    fn measure_at(
        font_system: &mut FontSystem,
        scale_factor: f64,
        font_size_logical_px: f32,
    ) -> Result<Self, RenderError> {
        let scale = scale_factor as f32;
        let font_size_px = font_size_logical_px * scale;
        let cell_height_px = font_size_logical_px * LINE_HEIGHT_TO_FONT_SIZE_RATIO * scale;
        let mut buffer = Buffer::new(font_system, Metrics::new(font_size_px, cell_height_px));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        buffer.set_text(
            // `H` supplies both the monospace advance and the current-size cap-height ink box.
            // Measuring the actual Consolas raster keeps symbol sizing coordinated at every DPI.
            "H",
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        let line = buffer
            .line_layout(font_system, 0)
            .and_then(|lines| lines.first().cloned())
            .ok_or(RenderError::MissingMonospaceMetrics)?;
        buffer.shape_until_scroll(font_system, false);
        let ascii_baseline_px = buffer
            .layout_runs()
            .next()
            .map(|run| run.line_y)
            .ok_or(RenderError::MissingMonospaceMetrics)?;
        let primary_advance_px = line.w.max(1.0);
        let [_, cap_top, _, cap_bottom] =
            glyph_ink_bounds(&buffer, font_system, &mut SwashCache::new())
                .ok_or(RenderError::MissingMonospaceMetrics)?;
        Ok(Self {
            cell_width_px: primary_advance_px.ceil(),
            cell_height_px: cell_height_px.ceil(),
            font_size_px,
            padding_px: (PADDING_LOGICAL_PX * scale).ceil(),
            scale_factor,
            ascii_baseline_px,
            primary_advance_px,
            primary_cap_height_px: (cap_bottom - cap_top).max(1.0),
            primary_cap_center_y_px: (cap_top + cap_bottom) / 2.0,
        })
    }

    pub fn grid_for_pixels(&self, width: u32, height: u32) -> GridSize {
        let usable_width = (width as f32 - 2.0 * self.padding_px).max(self.cell_width_px);
        let usable_height = (height as f32 - 2.0 * self.padding_px).max(self.cell_height_px);
        let columns = (usable_width / self.cell_width_px)
            .floor()
            .clamp(1.0, u16::MAX as f32);
        let rows = (usable_height / self.cell_height_px)
            .floor()
            .clamp(1.0, u16::MAX as f32);
        GridSize {
            columns: NonZeroU16::new(columns as u16).expect("grid columns are clamped above zero"),
            rows: NonZeroU16::new(rows as u16).expect("grid rows are clamped above zero"),
        }
    }

    pub fn cell_height_subpixels(&self) -> NonZeroI64 {
        let value = (self.cell_height_px * SUBPIXELS_PER_PX as f32).round() as i64;
        NonZeroI64::new(value.max(1)).expect("cell height is clamped above zero")
    }

    pub fn cell_width_subpixels(&self) -> NonZeroI64 {
        let value = (self.cell_width_px * SUBPIXELS_PER_PX as f32).round() as i64;
        NonZeroI64::new(value.max(1)).expect("cell width is clamped above zero")
    }

    pub fn ascii_baseline_subpixels(&self) -> NonZeroI64 {
        let value = (self.ascii_baseline_px * SUBPIXELS_PER_PX as f32).round() as i64;
        NonZeroI64::new(value.max(1)).expect("measured ASCII baseline is above zero")
    }

    pub fn dpi_milli(&self) -> NonZeroU32 {
        let value = (self.scale_factor * 1000.0)
            .round()
            .clamp(1.0, u32::MAX as f64);
        NonZeroU32::new(value as u32).expect("DPI scale is clamped above zero")
    }

    /// Hit test against the exact geometry published with a frame. This is the sole row oracle
    /// for selection and protocol forwarding once live rows can have non-uniform pixel heights.
    pub fn hit_test_frame(&self, frame: &ViewportFrame, x: f64, y: f64) -> Option<GridHit> {
        let x = x as f32 - self.padding_px;
        let y = y as f32 - self.padding_px;
        if x < 0.0 {
            return None;
        }
        let column = (x / self.cell_width_px).floor() as u32;
        if column >= frame.columns.get() {
            return None;
        }
        let y_subpixels = (f64::from(y) * SUBPIXELS_PER_PX as f64).floor() as i64;
        frame
            .visual_row_at(y_subpixels)
            .map(|row| GridHit { row, column })
    }

    /// The cell a *gesture* at this point means — the nearest one, never `None`
    /// for want of a cell exactly underneath.
    ///
    /// [`Self::hit_test_frame`]'s `None` is the honest answer to "what is under
    /// the pointer" for a point on the padding, past the last column, or below
    /// the last row. A selection drag asks what the gesture means instead, and
    /// past the end of a row that is the end of the row: a drag pulled to a
    /// pane's edge must select up to it rather than stop a column short.
    ///
    /// The clamping lives here because the answer needs the padding, the cell
    /// width and the frame's own row map — the caller holding a second copy of
    /// that geometry to clamp with is exactly the class of bug this avoids.
    /// `None` only when the frame draws no rows at all.
    pub fn clamped_hit_test_frame(&self, frame: &ViewportFrame, x: f64, y: f64) -> Option<GridHit> {
        let column = ((x as f32 - self.padding_px) / self.cell_width_px).floor();
        let column = if column.is_finite() { column } else { 0.0 };
        let column = (column.max(0.0) as u32).min(frame.columns.get().saturating_sub(1));
        let y_subpixels =
            (f64::from(y as f32 - self.padding_px) * SUBPIXELS_PER_PX as f64).floor() as i64;
        frame
            .clamped_visual_row_at(y_subpixels)
            .map(|row| GridHit { row, column })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridHit {
    pub row: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridSize {
    pub columns: NonZeroU16,
    pub rows: NonZeroU16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Preedit {
    pub text: String,
    /// UTF-8 byte offset of the collapsed IME caret. M0 intentionally ignores target clauses.
    pub cursor_byte: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImeCursorArea {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposedFrame {
    pub frame: ViewportFrame,
    pub ime_caret: bt_viewport::GridCursor,
}

/// Overlay IME preedit on a frame without mutating terminal state.
///
/// The terminal grid remains the sole authority for committed cell width. Preedit is transient UI,
/// but it consumes the same grapheme width oracle so its caret does not jump when text commits.
pub fn compose_preedit(
    frame: &ViewportFrame,
    preedit: Option<&Preedit>,
) -> Result<ComposedFrame, FrameShapeError> {
    frame.validate_shape()?;
    let Some(preedit) = preedit.filter(|preedit| !preedit.text.is_empty() && frame.cursor.visible)
    else {
        return Ok(ComposedFrame {
            frame: frame.clone(),
            ime_caret: frame.cursor,
        });
    };

    let mut composed = frame.clone();
    let cursor_byte = valid_cursor_byte(
        &preedit.text,
        preedit.cursor_byte.unwrap_or(preedit.text.len()),
    );
    let ime_caret = advance_grid_position(
        frame.cursor,
        &preedit.text[..cursor_byte],
        frame.columns.get(),
        frame.grid_rows.get(),
    );
    overlay_preedit_cells(&mut composed, preedit);
    composed.cursor = ime_caret;
    Ok(ComposedFrame {
        frame: composed,
        ime_caret,
    })
}

fn valid_cursor_byte(text: &str, requested: usize) -> usize {
    let mut cursor = requested.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn advance_grid_position(
    start: bt_viewport::GridCursor,
    text: &str,
    columns: u32,
    rows: u32,
) -> bt_viewport::GridCursor {
    let mut row = start.row;
    let mut column = start.column;
    for cluster in graphemes(text) {
        let width = cluster_width(cluster) as u32;
        if width == 0 {
            continue;
        }
        if width == 2 && column + width > columns {
            row = row.saturating_add(1);
            column = 0;
        }
        column += width;
        if column >= columns {
            row = row.saturating_add(column / columns);
            column %= columns;
        }
        if row >= rows {
            row = rows.saturating_sub(1);
            column = columns.saturating_sub(1);
            break;
        }
    }
    bt_viewport::GridCursor {
        row,
        column,
        visible: true,
    }
}

fn overlay_preedit_cells(frame: &mut ViewportFrame, preedit: &Preedit) {
    let columns = frame.columns.get() as usize;
    // IME remains bounded to the PTY grid in phase A. Moving it into a partially visible
    // presentation row belongs to the cursor/IME debt carried into the pixel-offset phase.
    let rows = frame.grid_rows.get() as usize;
    let mut row = frame.cursor.row as usize;
    let mut column = frame.cursor.column as usize;
    let mut previous_lead: Option<usize> = None;

    for cluster in graphemes(&preedit.text) {
        let width = cluster_width(cluster);
        if width == 0 {
            if let Some(index) = previous_lead {
                frame.cells[index].text.push_str(cluster);
            }
            continue;
        }
        if width == 2 && column + width > columns {
            row += 1;
            column = 0;
        }
        if row >= rows || column >= columns {
            break;
        }

        let index = row * columns + column;
        let mut cell = CapturedCell::plain(cluster.to_owned());
        cell.style.flags.insert(CellFlags::UNDERLINE);
        if width == 2 {
            cell.style.flags.insert(CellFlags::WIDE_CHAR);
        }
        frame.cells[index] = cell;
        previous_lead = Some(index);

        if width == 2 && column + 1 < columns {
            let mut spacer = CapturedCell::plain("");
            spacer.wide_spacer = true;
            spacer.style.flags.insert(CellFlags::UNDERLINE);
            frame.cells[index + 1] = spacer;
        }
        column += width;
        if column >= columns {
            row += column / columns;
            column %= columns;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSource {
    Keyboard,
    PtyOutput,
    Resize,
    Expose,
}

const FNV_1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameContentDigest {
    pub nonblank_cells: usize,
    pub first_text_row: i64,
    pub last_text_row: i64,
    pub content_fnv: u64,
}

/// Produce a stable content-only fingerprint for trace correlation.
///
/// Each cell contributes its row and column as little-endian `u32`s, its UTF-8 byte length as a
/// little-endian `u64`, the UTF-8 bytes, then tagged foreground and background colors. Render-only
/// flags, hyperlinks, cursor state, and selection state are intentionally outside this digest.
pub fn frame_content_digest(frame: &ViewportFrame) -> FrameContentDigest {
    let columns = frame.columns.get() as usize;
    let mut content_fnv = FNV_1A_64_OFFSET_BASIS;
    let mut nonblank_cells = 0_usize;
    let mut first_text_row = None;
    let mut last_text_row = None;

    for (index, cell) in frame
        .cells
        .iter()
        .take(frame.drawable_rows().saturating_mul(columns))
        .enumerate()
    {
        let row = index / columns;
        let column = index % columns;
        fnv_write(&mut content_fnv, &(row as u32).to_le_bytes());
        fnv_write(&mut content_fnv, &(column as u32).to_le_bytes());
        fnv_write(&mut content_fnv, &(cell.text.len() as u64).to_le_bytes());
        fnv_write(&mut content_fnv, cell.text.as_bytes());
        fnv_write_color(&mut content_fnv, cell.style.foreground);
        fnv_write_color(&mut content_fnv, cell.style.background);

        if !cell.text.is_empty() && cell.text.as_bytes().iter().any(|byte| *byte != b' ') {
            nonblank_cells = nonblank_cells.saturating_add(1);
            first_text_row.get_or_insert(row as i64);
            last_text_row = Some(row as i64);
        }
    }

    FrameContentDigest {
        nonblank_cells,
        first_text_row: first_text_row.unwrap_or(-1),
        last_text_row: last_text_row.unwrap_or(-1),
        content_fnv,
    }
}

pub fn frame_is_alternate_screen(frame: &ViewportFrame) -> bool {
    frame.cell_anchors.first().is_some_and(|anchor| {
        matches!(
            anchor.start,
            ContentAnchor::Live {
                screen: ScreenId::Alternate,
                ..
            }
        )
    })
}

fn fnv_write(state: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *state ^= u64::from(*byte);
        *state = state.wrapping_mul(FNV_1A_64_PRIME);
    }
}

fn fnv_write_color(state: &mut u64, color: TerminalColor) {
    match color {
        TerminalColor::Named(value) => fnv_write(state, &[0, value]),
        TerminalColor::Indexed(value) => fnv_write(state, &[1, value]),
        TerminalColor::Rgb(red, green, blue) => fnv_write(state, &[2, red, green, blue]),
    }
}

/// Replaceable event boundary carried into the renderer without a winit type.
#[derive(Clone, Copy, Debug)]
pub struct FrameTrigger {
    pub occurred_at: Instant,
    pub source: FrameSource,
}

#[derive(Clone, Copy, Debug)]
pub struct PresentReceipt {
    pub trigger: FrameTrigger,
    pub submitted_at: Instant,
    pub present_called_at: Instant,
}

impl PresentReceipt {
    pub fn latency(self) -> Result<FrameLatency, TimingError> {
        if self.submitted_at < self.trigger.occurred_at
            || self.present_called_at < self.submitted_at
        {
            return Err(TimingError::InvertedTimestamp);
        }
        Ok(FrameLatency {
            event_to_submit: self.submitted_at - self.trigger.occurred_at,
            event_to_present_call: self.present_called_at - self.trigger.occurred_at,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameLatency {
    pub event_to_submit: Duration,
    pub event_to_present_call: Duration,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TimingError {
    #[error("frame timing timestamps are inverted")]
    InvertedTimestamp,
}

#[derive(Default)]
pub struct LatestFrameSlot {
    pending: Option<(ViewportFrame, FrameTrigger)>,
    overwrites: u64,
}

impl LatestFrameSlot {
    pub fn publish(
        &mut self,
        frame: ViewportFrame,
        trigger: FrameTrigger,
    ) -> Result<(), FrameShapeError> {
        frame.validate_shape()?;
        self.overwrites += u64::from(self.pending.replace((frame, trigger)).is_some());
        Ok(())
    }

    pub fn take(&mut self) -> Option<(ViewportFrame, FrameTrigger)> {
        self.pending.take()
    }

    pub fn pending_frame(&self) -> Option<&ViewportFrame> {
        self.pending.as_ref().map(|(frame, _)| frame)
    }

    pub fn overwrites(&self) -> u64 {
        self.overwrites
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("non-rectangular frame: {0}")]
    FrameShape(#[from] FrameShapeError),
    #[error("wgpu error: {0}")]
    Wgpu(String),
    #[error("glyph rendering failed: {0}")]
    GlyphRender(String),
    #[error("no usable monospace font metrics were produced")]
    MissingMonospaceMetrics,
    #[error("no usable chrome sans-serif font metrics were produced")]
    MissingChromeSansMetrics,
    #[error("surface validation failed")]
    SurfaceValidation,
    /// A window asked to draw through a [`GpuContext`] whose atlas and pipelines
    /// were baked for a different swapchain format.
    ///
    /// An error and never a degradation. The format is baked into
    /// [`TextAtlas::new`] and into both pipelines, so "share anyway" would mean
    /// a second atlas, a second rect pipeline and a second math pipeline behind
    /// a name that says one — every glyph in the process uploaded twice, and
    /// nothing in glyphon 0.12 reporting that it happened (spike Q2).
    #[error(
        "surface format {surface:?} does not match the device context's {context:?}; \
         a second format needs a second renderer, not a second swapchain"
    )]
    FormatMismatch {
        context: wgpu::TextureFormat,
        surface: wgpu::TextureFormat,
    },
    /// The adapter did not offer the one composite alpha mode this target has to
    /// be configured with.
    ///
    /// An error and never a substitution, for the same reason as
    /// [`RenderError::FormatMismatch`]: a composition-visual surface quietly
    /// configured `Opaque` would present today's picture perfectly and would
    /// have destroyed the property the whole slice exists to establish, with no
    /// symptom until a web preview is asked to show through it.
    #[error(
        "a {target:?} surface must be configured {required:?}, and this adapter offered {offered:?}"
    )]
    AlphaModeUnavailable {
        target: WindowTargetKind,
        required: wgpu::CompositeAlphaMode,
        offered: Vec<wgpu::CompositeAlphaMode>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum PresentOutcome {
    Presented(PresentReceipt),
    Skipped,
    Reconfigure,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
struct RectInstance {
    rect: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MathVertex {
    position: [f32; 2],
    uv: [f32; 2],
    /// A uniform multiplier on the sampled alpha, `0.0 ..= 1.0`.
    ///
    /// Constant across a quad — it is one element's opacity, not a gradient —
    /// but it lives on the vertex rather than in a uniform because these quads
    /// are drawn from one buffer in one pass, and a uniform would mean either a
    /// bind group per mark or a draw call per mark.
    opacity: f32,
}

/// One corner of the window's ground quad — `docs/DESIGN.md` §7.1.6c-4b.
///
/// The ground colour and the two percentages ride on the vertex for
/// [`MathVertex::opacity`]'s reason, sharpened: there are exactly six of these
/// per frame, so a uniform buffer would be a bind group, a layout entry and a
/// write per frame to carry twenty bytes that the vertex buffer is already
/// carrying anyway.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackgroundVertex {
    position: [f32; 2],
    /// Texture coordinates, which may exceed `0..1` — that is what Tile is, and
    /// why this pipeline's sampler is the crate's only `Repeat` one.
    uv: [f32; 2],
    /// Linear RGB of the scheme's background, with the ground's alpha in `[3]`.
    ground: [f32; 4],
    image_opacity: f32,
}

struct MathTextureTile {
    bind_group: wgpu::BindGroup,
    x_px: u32,
    y_px: u32,
    width_px: u32,
    height_px: u32,
}

struct CachedMathTexture {
    tiles: Vec<MathTextureTile>,
}

struct MathDraw {
    key: String,
    tile_index: usize,
    first_vertex: u32,
}

/// One overlay layer's GPU resources for this frame: its rectangles, its marks
/// and whether its text made it into the atlas. Held apart per layer so the pass
/// can draw all three channels of one layer before starting the next.
struct PreparedOverlayLayer {
    /// This layer's grounds, and the constant the pass fades them in with — see
    /// [`OverlayGround`] and [`create_ground_fade_rect_pipeline`]. The opacity
    /// travels with the buffer because it is a piece of pass state the draw
    /// cannot be issued without.
    ground_buffer: Option<wgpu::Buffer>,
    ground_count: u32,
    ground_opacity: f32,
    rect_buffer: Option<wgpu::Buffer>,
    rect_count: u32,
    icon_buffer: Option<wgpu::Buffer>,
    icon_draws: Vec<MathDraw>,
    text_prepared: bool,
}

/// One terminal seat's GPU resources for this frame, uploaded and ready to draw.
///
/// Held apart per seat for the same reason [`PreparedOverlayLayer`] is held
/// apart per layer: the pass has to finish one seat's channels — cells, bands,
/// text, status bar, toolbars — inside that seat's viewport before it moves the
/// viewport to the next one.
struct PreparedSeat {
    seat: SeatViewport,
    /// The scissor for this seat — see [`SeatFrame::clip`].
    clip: SeatViewport,
    /// Which entry of `seat_slots` holds this seat's prepared glyph batches.
    slot: usize,
    /// The cell backgrounds a program set, premultiplied by the window's alpha —
    /// see [`WindowRenderer::rectangles`]. Its own buffer for the reason the
    /// chrome's grounds have one: they are the same surface as the clear and are
    /// drawn with the clear's arithmetic, under everything else in the seat.
    ground_rect_buffer: wgpu::Buffer,
    ground_rect_count: usize,
    rect_buffer: wgpu::Buffer,
    rect_count: usize,
    math_vertex_buffer: Option<wgpu::Buffer>,
    math_draws: Vec<MathDraw>,
    text_prepared: bool,
    status_rect_buffer: wgpu::Buffer,
    status_rect_count: usize,
    math_overlay_buffer: wgpu::Buffer,
    math_overlay_count: usize,
}

/// One seat's flat fills for a frame, in the two classes [`WindowRenderer::rectangles`]
/// tells apart: the window's own surface, and the marks struck on it.
///
/// Two lists rather than one list with a flag, because the two are drawn by two
/// pipelines and the split is what the caller needs. Order between them is not a
/// judgement call: a ground is the bottom of its own cell by construction, so
/// "grounds, then ink" is the order the single list already had.
struct SeatRects {
    /// Premultiplied by the window's ground alpha, for the `Replace` pipeline.
    grounds: Vec<RectInstance>,
    /// Straight, for the alpha-blending pipeline — exactly as before.
    ink: Vec<RectInstance>,
}

/// One frame's math block draws plus the indices of the `frame.math_blocks` entries that actually
/// put pixels on screen. Overlays that decorate a block (the hover dim) read `drawn` so they can
/// never outlive the raster they decorate.
struct MathDrawBatch {
    draws: Vec<MathDraw>,
    vertices: Vec<MathVertex>,
    drawn: HashSet<usize>,
}

/// Hover-peek thumbnail flyout: transient presentation-layer state set by the app shell
/// (docs/M2-preview-matrix-and-verbs.md §4 peek verb). It deliberately never travels through
/// `ViewportFrame`, so replay/pin contracts and frame equality are untouched by a visible peek.
#[derive(Clone)]
pub struct PeekImageOverlay {
    /// Texture identity in the shared GPU LRU: the decode's content key at this display size
    /// (`bt_term::display_texture_key`). The size is part of the identity, so a differently sized
    /// flyout asks the LRU a different question instead of stretching a stale raster.
    pub key: String,
    /// Display-resolution pixels, already resampled to `peek_thumbnail_extent`. Never the native
    /// decode: a flyout is at most 40% of a pane, and uploading a wallpaper for it would spend
    /// tens of MiB of the shared texture budget on a thumbnail and evict the bands on screen.
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    /// The pane that owns the hovered content, as the rectangle that pane's body draws into.
    ///
    /// The same double-rect precedent [`PreviewImage`] sets, for the same reason: a transient
    /// laid out against "the seat" has to say *which* seat, and with a fleet on screen the honest
    /// answer is the pane the pointer is standing in — never the pane holding the keyboard. It
    /// decides the flyout's *size* only (§ [`peek_thumbnail_extent`]'s 40%-of-a-pane cap, which is
    /// also the bound on the resident texture); where the box lands is decided against the window.
    pub seat: SeatViewport,
    /// Physical-pixel pointer position captured when the hover settled, in **whole-window**
    /// coordinates; the flyout anchors to this point and stays put rather than chasing further
    /// pointer motion.
    pub pointer_x: f32,
    pub pointer_y: f32,
}

/// Persistent image content for the preview seat. Pixels are already resampled by the shared
/// decoration worker to the exact fit returned by [`preview_image_extent`]. The seat is expressed
/// in whole-surface physical pixels; drawing switches the pass to that viewport rather than
/// teaching terminal-frame geometry about neighbouring seats.
#[derive(Clone)]
pub struct PreviewImage {
    pub seat: SeatViewport,
    /// The box the picture may appear in — the scissor to [`Self::seat`]'s
    /// viewport, and equal to it at rest.
    ///
    /// The same pair, with the same meaning and for the same reason, as
    /// [`SeatFrame::clip`] beside a terminal's: `seat` is where the picture was
    /// laid out and where its top-left lands, `clip` is the box it is allowed to
    /// be seen in. They part company for exactly one reason, and it is U8's pane
    /// FLIP — a preview pane mid-flight is drawn at its final *size* from the
    /// corner it left, and cropped by the animating box on its way to the corner
    /// the solver gave it.
    ///
    /// Without it a preview seat's chrome glides while its picture sits at the
    /// destination, which is the pane visibly coming apart: the head arrives a
    /// fifth of a second after the image it belongs to. Landing a preview *is* a
    /// structural edit, so that is on screen the first time anyone opens one.
    pub clip: SeatViewport,
    pub key: String,
    pub rgba: Arc<[u8]>,
    /// Texture dimensions. During a live resize these remain the last clear raster.
    pub width_px: u32,
    pub height_px: u32,
    /// Draw dimensions inside the latest seat. They may briefly differ from the texture dimensions
    /// while a resize is in flight; the sampler provides the intentionally soft transition. Once a
    /// surface has been zoomed they differ *permanently* and on purpose: above 100% the CPU
    /// resample stops at the decode's native pixels and this pair carries the magnification, which
    /// is the whole reason a picture eight times its own size costs one texture rather than sixty
    /// megabytes of one.
    pub display_width_px: u32,
    pub display_height_px: u32,
    /// How far the drawn picture is carried from the **centre** of [`Self::seat`], in physical
    /// pixels (ticket #60).
    ///
    /// A displacement rather than an origin, and that is deliberate: at rest it is `[0.0, 0.0]` and
    /// the picture is centred exactly as it always was, so the resting geometry has one author and
    /// not two. It also survives a pane FLIP for free — [`WindowRenderer::place_preview_image`] moves the
    /// seat under a picture that has already been laid out, and an offset from that seat's centre
    /// travels with it where an absolute corner would have stayed behind.
    pub pan_px: [f32; 2],
}

/// One styled run inside a preview paragraph.
///
/// The unit exists because a markdown line is not one typeface: `a **bold**
/// word` and an inline code span are runs of the same line set in different
/// faces and weights, and a paragraph split into three labels would be three
/// boxes that have to be measured and butted together by hand — which is the
/// wrapping the shaper already does, done worse.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewRun {
    pub text: String,
    pub color: [u8; 3],
    /// Set in the monospace face — a file's own bytes, or an inline code span.
    pub mono: bool,
    pub bold: bool,
    /// How large this run is set **relative to its paragraph's own size** — CSS
    /// `font-size: 85%` on an inline element, and nothing more general than that.
    ///
    /// `1.0` for every run that agrees with the paragraph around it, which is
    /// almost all of them. The caller that does not is markdown's inline code
    /// span (github.css `code { font-size: 85% }`): a monospace face at the same
    /// nominal size as the sans beside it reads a size *larger*, because its
    /// x-height and its stems are cut for a grid.
    ///
    /// **The line height does not follow it.** A paragraph's leading is the
    /// paragraph's; a code span that dragged its own line box down would make
    /// the row it landed on taller than its neighbours, and prose whose leading
    /// changes line by line is exactly the density this metric exists to undo.
    pub font_scale: f32,
}

/// One paragraph of a preview body: styled text with a box to sit in.
///
/// A "paragraph" and not a "line" because [`Self::wrap`] decides which it is.
/// The two bodies this surface draws want opposite answers and both are right:
/// a source file is `white-space: pre` (mock-up 603) so a line that does not fit
/// runs off the edge and is cropped, while a markdown paragraph reflows to the
/// pane like any prose.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewParagraph {
    pub runs: Vec<PreviewRun>,
    /// `[left, top, right, bottom]`, physical pixels, whole-surface coordinates.
    ///
    /// When wrapping, the width of this box is the width text reflows inside.
    /// When not, it is deliberately generous: the crop is
    /// [`PreviewBody::clip`]'s job, and a bound here would be the reflow
    /// `white-space: pre` forbids.
    pub rect: [f32; 4],
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub wrap: bool,
    /// CSS `letter-spacing` in em — the code fence's language tag is the one
    /// caller that asks for it (`.md-code .lang`, mock-up 1208-1211).
    pub letter_spacing_em: f32,
    /// Right-align inside `rect` rather than left-align.
    pub align_right: bool,
    /// Centre horizontally inside `rect`, overriding [`Self::align_right`].
    ///
    /// `.pv-image` is a centred column (mock-up 605) and the sentence under the
    /// picture is the second item in it, which is the one caller today.
    pub align_center: bool,
}

/// Where one of a paragraph's runs actually came to rest, once shaped.
///
/// One entry per run **per visual line**: a link that wraps is two boxes,
/// because it is two boxes — a single box spanning both would claim the margin
/// between them, and a hit test or an underline built from it would answer for
/// text that is not there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewRunBox {
    /// Which entry of [`PreviewParagraph::runs`] this belongs to.
    pub run: usize,
    pub rect: [f32; 4],
}

/// One flat fill under a preview body's text.
///
/// The diff's line tints, the code fence's ground and border, and the table's
/// grid are all this: rectangles that must land under the text of the very same
/// body, which is why they ride here rather than going out as seat chrome. A
/// chrome quad is drawn a whole pass earlier and would sit under the *pane*
/// rather than under the scrolled document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewQuad {
    pub rect: [f32; 4],
    pub color: [u8; 3],
}

/// The body of the preview seat: fills, then text, inside one clip.
///
/// The sibling of [`PreviewImage`] and prepared in the same slot of the frame,
/// because it is the same thing: **content filling a preview seat's body, below
/// the head that names it.** What differs is only that one arrives as pixels
/// from a decoder and the other as a document.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewBody {
    /// The box the **scrolled document** may be seen in — the seat's body, and
    /// already intersected with whatever a pane FLIP is cropping it to. Every
    /// fill and every paragraph is clipped to this.
    ///
    /// It holds no furniture and gives up no height to any. A body once carried
    /// a `foot` slot for the read-only bar, and that bar retired on 2026-08-15:
    /// two 28px strips stacked at the bottom of one pane were one too many, and
    /// the fact the upper one stated now hangs on the right of the path strip
    /// below it (see [`crate::PreviewParagraph`]'s callers in `bt-app`).
    pub clip: [f32; 4],
    pub quads: Vec<PreviewQuad>,
    pub paragraphs: Vec<PreviewParagraph>,
    /// Runs of the body that **scroll inside themselves** (user ruling,
    /// 2026-08-13).
    ///
    /// A rendered markdown page has no horizontal axis of its own: prose folds to
    /// the pane, so moving the whole page sideways pushes the folded text off the
    /// screen to reach a table nobody was reading. What is wider than the pane is
    /// always one *block* — a table, a fence — and a block is what carries the
    /// offset, exactly as GitHub and Typora do it.
    ///
    /// A slot of its own rather than a `clip` on every quad and paragraph,
    /// because the thing being expressed is not "this rectangle is cropped": it
    /// is "these rectangles are one scrolling region", which is what makes the
    /// crop, the offset and the indicator one fact instead of three.
    pub blocks: Vec<PreviewBlock>,
}

impl PreviewBody {
    /// Every paragraph this body holds, its scrolling blocks' included.
    #[must_use]
    pub fn paragraph_count(&self) -> usize {
        self.paragraphs.len()
            + self
                .blocks
                .iter()
                .map(|block| block.paragraphs.len())
                .sum::<usize>()
    }

    /// Every fill this body holds, on the same terms.
    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.quads.len()
            + self
                .blocks
                .iter()
                .map(|block| block.quads.len())
                .sum::<usize>()
    }
}

/// **What one frame did with the preview documents it was handed** — the
/// forensic record behind `BT_PREVIEW_TRACE` (user report 2026-08-21: a markdown
/// preview drawing its heading rules and none of its words).
///
/// Four numbers and a flag, because the picture the report describes has exactly
/// four places it can come from and they are otherwise indistinguishable from
/// outside:
///
/// * `bodies == 0` — nothing was built for this seat at all; the two lines on
///   screen are the pane's own head and foot hairlines.
/// * `paragraphs == 0` while `quads > 0` — the document was built and its blocks
///   carried no text, which is a layout that ran on something empty.
/// * `drawn == 0` while `paragraphs > 0` — every paragraph was refused by
///   `shape_preview_body`'s own filters: an empty run, a box outside its clip, or
///   a box that did not survive `crop_to` (zero height, inverted, `NaN`).
/// * `prepared == false` while `drawn > 0` — the batch was shaped and the atlas
///   refused it, and the frame presented every fill with none of its glyphs.
///
/// **It changes no behaviour.** Nothing in the renderer reads it back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreviewTextFrame {
    pub bodies: usize,
    pub paragraphs: usize,
    pub quads: usize,
    /// Paragraphs that survived every filter and were handed to the atlas.
    pub drawn: usize,
    /// Whether the atlas took them.
    pub prepared: bool,
}

/// One rendered table block's picture, in the block's own coordinates.
///
/// **Coordinates, not pixels, and that is the whole of what makes a table different from a
/// formula here.** A formula arrives as an RGBA raster because a formula is typeset by an engine
/// that knows nothing about this window; a table is set in *this* window's own text, in the
/// terminal's own font at the terminal's own size, so rasterizing it would mean shaping it a
/// second time against a second font stack — and the first thing that would cost is the CJK
/// fallback chain the chrome shaper already carries. So the block hands over its layout instead of
/// its pixels, in a space whose origin is the block's top-left, and this renderer puts that origin
/// where the placement says the block starts.
///
/// It is [`PreviewQuad`] and [`PreviewParagraph`] because a table in a terminal pane and a table
/// in a rendered markdown file are the same picture: same column arithmetic, same hairlines, same
/// heading band, same cell insets. One layout, drawn twice at two metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct TableBlockPaint {
    pub quads: Vec<PreviewQuad>,
    pub paragraphs: Vec<PreviewParagraph>,
}

/// One block of a preview body that owns a horizontal offset.
///
/// Its contents are already placed at that offset; what this carries is the
/// window they may be seen through, which is the block's own rectangle rather
/// than the body's. Everything is cropped to *both* — a block scrolled sideways
/// still cannot be seen through the pane head.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewBlock {
    pub clip: [f32; 4],
    pub quads: Vec<PreviewQuad>,
    pub paragraphs: Vec<PreviewParagraph>,
}

/// Fit an image inside a preview body while preserving aspect ratio and never enlarging it beyond
/// its native dimensions.
#[must_use]
pub fn preview_image_extent(
    body_width_px: u32,
    body_height_px: u32,
    image_width_px: u32,
    image_height_px: u32,
) -> Option<(u32, u32)> {
    if body_width_px == 0 || body_height_px == 0 || image_width_px == 0 || image_height_px == 0 {
        return None;
    }
    let width_scale = body_width_px as f64 / image_width_px as f64;
    let height_scale = body_height_px as f64 / image_height_px as f64;
    let scale = width_scale.min(height_scale).min(1.0);
    Some((
        (image_width_px as f64 * scale).floor().max(1.0) as u32,
        (image_height_px as f64 * scale).floor().max(1.0) as u32,
    ))
}

struct PeekBoxLayout {
    /// Outer border rect; the interior and image rects nest inside it.
    frame: [f32; 4],
    interior: [f32; 4],
    image: [f32; 4],
}

/// Border thickness and interior inset of the flyout box in physical pixels. The thumbnail extent
/// and the box that frames it are derived from these same two numbers, so the size the app
/// resamples to and the size the box reserves can never disagree.
fn peek_border_px(scale_factor: f32) -> f32 {
    (FLOAT_WINDOW_BORDER_LOGICAL_PX * scale_factor)
        .round()
        .max(1.0)
}

fn peek_inset_px(scale_factor: f32) -> f32 {
    6.0 * scale_factor
}

/// The exact pixel box the flyout shows an image in: capped to 40% of the padded pane per axis,
/// never upscaled, `None` when the padded pane cannot host the box that frames it.
///
/// This is the size the app must resample to *before* the pixels reach the renderer. Applying it
/// twice is the identity — an image already at its display extent is within the cap, so `fit`
/// saturates at 1.0 and the extent is returned unchanged — which is what lets `peek_box_layout`
/// agree with the display-sized dimensions the app hands it.
pub fn peek_thumbnail_extent(
    viewport_width: f32,
    viewport_height: f32,
    padding_px: f32,
    scale_factor: f32,
    image_width_px: u32,
    image_height_px: u32,
) -> Option<(u32, u32)> {
    if image_width_px == 0 || image_height_px == 0 {
        return None;
    }
    let avail_width = viewport_width - 2.0 * padding_px;
    let avail_height = viewport_height - 2.0 * padding_px;
    if avail_width <= 0.0 || avail_height <= 0.0 {
        return None;
    }
    let native_width = image_width_px as f32;
    let native_height = image_height_px as f32;
    let fit = (avail_width * 0.4 / native_width)
        .min(avail_height * 0.4 / native_height)
        .min(1.0);
    // Truncate, never round: a rounded extent could exceed the 40% cap it was derived from, and
    // that cap is what bounds the resident texture.
    let thumb_width = ((native_width * fit) as u32).max(1);
    let thumb_height = ((native_height * fit) as u32).max(1);
    let chrome = 2.0 * (peek_border_px(scale_factor) + peek_inset_px(scale_factor));
    if thumb_width as f32 + chrome > avail_width || thumb_height as f32 + chrome > avail_height {
        return None;
    }
    Some((thumb_width, thumb_height))
}

/// Pure flyout placement around the extent above: anchored below-right of the pointer, flipped
/// above when the bottom lacks room, and clamped into the window. Returns `None` when the pane
/// is too small to host the box.
///
/// Two rectangles, two jobs, and they are deliberately not the same one.
///
/// * The **pane** — the one owning the hovered content — sets the size, because "a flyout is at
///   most 40% of a pane" is what bounds the texture it uploads.
/// * The **window** sets where the box may land. A floating window is not a child of the pane it
///   was raised from: the mock-up's `--menu`/`--border`/`--floatr`/`--shadow` popups are laid over
///   the whole surface, and a peek raised near a pane edge that clamped itself into that pane
///   would jump away from the word it belongs to for no reason the user can see. Overhanging the
///   neighbour is correct and is what the draw order makes visible (the flyout is issued after
///   every seat and after seat chrome).
///
/// Everything the box needs is expressed in whole-window physical pixels, `pointer_*` included,
/// so what comes back is directly the rectangle on the surface.
///
/// `N = 1` is not a special case: a lone terminal leaf's pane *is* the window, both pairs of
/// extents are the same numbers, and every expression below is the one that was here when a single
/// viewport drove both.
#[allow(clippy::too_many_arguments)]
fn peek_box_layout(
    pane_width: f32,
    pane_height: f32,
    window_width: f32,
    window_height: f32,
    padding_px: f32,
    scale_factor: f32,
    image_width_px: u32,
    image_height_px: u32,
    pointer_x: f32,
    pointer_y: f32,
) -> Option<PeekBoxLayout> {
    let (thumb_width_px, thumb_height_px) = peek_thumbnail_extent(
        pane_width,
        pane_height,
        padding_px,
        scale_factor,
        image_width_px,
        image_height_px,
    )?;
    let avail_left = padding_px;
    let avail_top = padding_px;
    let avail_right = window_width - padding_px;
    let avail_bottom = window_height - padding_px;
    let thumb_width = thumb_width_px as f32;
    let thumb_height = thumb_height_px as f32;
    let border = peek_border_px(scale_factor);
    let inset = peek_inset_px(scale_factor);
    let box_width = thumb_width + 2.0 * (border + inset);
    let box_height = thumb_height + 2.0 * (border + inset);
    let offset_x = 12.0 * scale_factor;
    let offset_y = 18.0 * scale_factor;
    let mut top = pointer_y + offset_y;
    if top + box_height > avail_bottom {
        top = pointer_y - offset_y - box_height;
    }
    let top = top.clamp(avail_top, avail_bottom - box_height);
    let left = (pointer_x + offset_x).clamp(avail_left, avail_right - box_width);
    let frame = [left, top, left + box_width, top + box_height];
    let interior = [
        frame[0] + border,
        frame[1] + border,
        frame[2] - border,
        frame[3] - border,
    ];
    let image = [
        interior[0] + inset,
        interior[1] + inset,
        interior[0] + inset + thumb_width,
        interior[1] + inset + thumb_height,
    ];
    Some(PeekBoxLayout {
        frame,
        interior,
        image,
    })
}

/// One flat fill of a floating window's chrome: a physical-pixel rectangle, the
/// colour it is drawn in, and the alpha it is blended at — the shape's own
/// antialiasing already folded into that alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PeekBoxFill {
    layer: PeekBoxLayer,
    rect: [f32; 4],
    color: [u8; 3],
    alpha: f32,
}

/// The three planes a floating window is made of. They are named because two of
/// them can share a colour — a shadow and a hairline are both black on the light
/// palette — and "which plane put this pixel here" is then not a question the
/// pixel can answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PeekBoxLayer {
    Lift,
    Hairline,
    Face,
}

/// The flyout's chrome, back to front: its lift, its hairline, its face.
///
/// A floating window in the mock-up is `--menu` behind a `--border` hairline,
/// rounded by 10px, over `--shadow`. Three of those are colours and arrive as
/// palette tokens; the fourth is a shape, built the way the tab's is in
/// `marks.rs` — analytic coverage, never nested quads.
///
/// The hairline is not drawn as a ring. The whole box is filled in `--border` at
/// the mock-up's own alpha and the face is laid over it one border-pixel in, so
/// what survives is exactly the border-box a browser would leave: one blended
/// pixel of `rgba(255,255,255,.094)` over whatever the terminal is showing behind
/// it, antialiased on both of its edges rather than only the outer one.
/// Compositing that hairline into an opaque colour the way the rest of the
/// palette does is not available here — a flyout has no known surface under it.
fn peek_box_fills(layout: &PeekBoxLayout, palette: ChromePalette, scale: f32) -> Vec<PeekBoxFill> {
    let radius = FLOAT_WINDOW_RADIUS_LOGICAL_PX * scale;
    let border = peek_border_px(scale);
    let alpha = |value: u8| f32::from(value) / 255.0;
    let paint = |layer: PeekBoxLayer,
                 coverage: Vec<rounded_rect::CoverageRect>,
                 color: [u8; 3],
                 alpha: f32| {
        coverage.into_iter().map(move |entry| PeekBoxFill {
            layer,
            rect: entry.rect,
            color,
            alpha: entry.coverage * alpha,
        })
    };
    // The lift: one soft falloff around the box — never under it, because an
    // outer shadow is clipped out of the border box it lifts. It was two rings
    // until 2026-08-13; see `rounded_rect_shadow_coverage` for why two steps
    // 14 pixels wide read as two rings rather than as a shadow.
    let spread = FLOAT_WINDOW_SHADOW_LOGICAL_PX * scale;
    let mut fills: Vec<PeekBoxFill> = paint(
        PeekBoxLayer::Lift,
        rounded_rect_shadow_coverage(layout.frame, radius, spread),
        palette.menu_shadow,
        overlay_shadow_alpha(
            alpha(palette.menu_shadow_inner_alpha),
            alpha(palette.menu_shadow_outer_alpha),
        ),
    )
    .collect();
    fills.extend(paint(
        PeekBoxLayer::Hairline,
        rounded_rect_coverage(layout.frame, radius),
        palette.menu_border,
        alpha(palette.menu_border_alpha),
    ));
    // The face's round is concentric with the box's: one border in on every side,
    // so one border smaller in radius. Anything else and the hairline would
    // thicken through the corner.
    fills.extend(paint(
        PeekBoxLayer::Face,
        rounded_rect_coverage(layout.interior, radius - border),
        palette.menu_surface,
        1.0,
    ));
    fills
}

struct ComposedRow {
    narrow_glyphs: Vec<NarrowGlyph>,
    wide_glyphs: Vec<WideGlyph>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct RowMetricsKey {
    cell_width_bits: u32,
    cell_height_bits: u32,
    font_size_bits: u32,
    padding_bits: u32,
    scale_factor_bits: u64,
    ascii_baseline_bits: u32,
    primary_advance_bits: u32,
    primary_cap_height_bits: u32,
    primary_cap_center_y_bits: u32,
}

impl From<CellMetrics> for RowMetricsKey {
    fn from(metrics: CellMetrics) -> Self {
        Self {
            cell_width_bits: metrics.cell_width_px.to_bits(),
            cell_height_bits: metrics.cell_height_px.to_bits(),
            font_size_bits: metrics.font_size_px.to_bits(),
            padding_bits: metrics.padding_px.to_bits(),
            scale_factor_bits: metrics.scale_factor.to_bits(),
            ascii_baseline_bits: metrics.ascii_baseline_px.to_bits(),
            primary_advance_bits: metrics.primary_advance_px.to_bits(),
            primary_cap_height_bits: metrics.primary_cap_height_px.to_bits(),
            primary_cap_center_y_bits: metrics.primary_cap_center_y_px.to_bits(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ComposedRowKey {
    cells: Vec<CapturedCell>,
    metrics: RowMetricsKey,
    font_revision: u64,
    theme_revision: u64,
    status_overlay: Option<String>,
}

impl Hash for ComposedRowKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.metrics.hash(state);
        self.font_revision.hash(state);
        self.theme_revision.hash(state);
        self.status_overlay.hash(state);
        self.cells.len().hash(state);
        for cell in &self.cells {
            cell.text.hash(state);
            cell.style.flags.bits().hash(state);
            hash_terminal_color(cell.style.foreground, state);
            hash_terminal_color(cell.style.background, state);
            cell.hyperlink.hash(state);
            cell.wide_spacer.hash(state);
        }
    }
}

fn hash_terminal_color<H: Hasher>(color: TerminalColor, state: &mut H) {
    match color {
        TerminalColor::Named(value) => {
            0_u8.hash(state);
            value.hash(state);
        }
        TerminalColor::Indexed(value) => {
            1_u8.hash(state);
            value.hash(state);
        }
        TerminalColor::Rgb(red, green, blue) => {
            2_u8.hash(state);
            red.hash(state);
            green.hash(state);
            blue.hash(state);
        }
    }
}

struct LruNode<K, V> {
    key: Arc<K>,
    value: V,
    resident_bytes: usize,
    previous: Option<usize>,
    next: Option<usize>,
}

/// Hash lookup plus an index-based intrusive list. Lookup, promotion, and one eviction are O(1)
/// without unsafe pointers. The byte budget accounts for the node's owned key/value estimate;
/// hash-table bucket slack and allocator headers are intentionally outside the approximation.
struct ByteLru<K, V> {
    indices: HashMap<Arc<K>, usize>,
    nodes: Vec<Option<LruNode<K, V>>>,
    free: Vec<usize>,
    most_recent: Option<usize>,
    least_recent: Option<usize>,
    resident_bytes: usize,
    budget_bytes: usize,
}

impl<K: Eq + Hash, V> ByteLru<K, V> {
    fn new(budget_bytes: usize) -> Self {
        Self {
            indices: HashMap::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            most_recent: None,
            least_recent: None,
            resident_bytes: 0,
            budget_bytes,
        }
    }

    fn clear(&mut self) {
        self.indices.clear();
        self.nodes.clear();
        self.free.clear();
        self.most_recent = None;
        self.least_recent = None;
        self.resident_bytes = 0;
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        let index = *self.indices.get(key)?;
        self.promote(index);
        Some(&self.nodes[index].as_ref().expect("LRU index is live").value)
    }

    fn insert(&mut self, key: K, value: V, resident_bytes: usize) -> (bool, u64) {
        debug_assert!(!self.indices.contains_key(&key));
        if resident_bytes > self.budget_bytes {
            return (false, 0);
        }

        let mut evictions = 0_u64;
        while self.resident_bytes.saturating_add(resident_bytes) > self.budget_bytes {
            if !self.remove_least_recent() {
                break;
            }
            evictions = evictions.saturating_add(1);
        }

        let key = Arc::new(key);
        let index = self.free.pop().unwrap_or_else(|| {
            self.nodes.push(None);
            self.nodes.len() - 1
        });
        let old_head = self.most_recent;
        self.nodes[index] = Some(LruNode {
            key: Arc::clone(&key),
            value,
            resident_bytes,
            previous: None,
            next: old_head,
        });
        if let Some(old_head) = old_head {
            self.nodes[old_head]
                .as_mut()
                .expect("LRU head is live")
                .previous = Some(index);
        } else {
            self.least_recent = Some(index);
        }
        self.most_recent = Some(index);
        self.resident_bytes = self.resident_bytes.saturating_add(resident_bytes);
        self.indices.insert(key, index);
        (true, evictions)
    }

    fn promote(&mut self, index: usize) {
        if self.most_recent == Some(index) {
            return;
        }
        let (previous, next) = {
            let node = self.nodes[index].as_ref().expect("LRU index is live");
            (node.previous, node.next)
        };
        if let Some(previous) = previous {
            self.nodes[previous]
                .as_mut()
                .expect("LRU previous index is live")
                .next = next;
        }
        if let Some(next) = next {
            self.nodes[next]
                .as_mut()
                .expect("LRU next index is live")
                .previous = previous;
        } else {
            self.least_recent = previous;
        }
        let old_head = self.most_recent;
        let node = self.nodes[index].as_mut().expect("LRU index is live");
        node.previous = None;
        node.next = old_head;
        if let Some(old_head) = old_head {
            self.nodes[old_head]
                .as_mut()
                .expect("LRU head is live")
                .previous = Some(index);
        }
        self.most_recent = Some(index);
    }

    fn remove_least_recent(&mut self) -> bool {
        let Some(index) = self.least_recent else {
            return false;
        };
        let node = self.nodes[index].take().expect("LRU tail is live");
        self.least_recent = node.previous;
        if let Some(previous) = node.previous {
            self.nodes[previous]
                .as_mut()
                .expect("LRU previous index is live")
                .next = None;
        } else {
            self.most_recent = None;
        }
        self.indices.remove(node.key.as_ref());
        self.resident_bytes = self.resident_bytes.saturating_sub(node.resident_bytes);
        self.free.push(index);
        true
    }

    fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.indices.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RowCacheCounters {
    hits: u64,
    misses: u64,
    evictions: u64,
    resident_bytes: usize,
}

struct ComposedRowCache {
    entries: ByteLru<ComposedRowKey, Arc<ComposedRow>>,
    counters: RowCacheCounters,
}

impl ComposedRowCache {
    fn new() -> Self {
        Self {
            entries: ByteLru::new(COMPOSED_ROW_CACHE_BUDGET_BYTES),
            counters: RowCacheCounters::default(),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.counters = RowCacheCounters::default();
    }

    fn get(&mut self, key: &ComposedRowKey) -> Option<Arc<ComposedRow>> {
        let row = self.entries.get(key).map(Arc::clone);
        if row.is_some() {
            self.counters.hits = self.counters.hits.saturating_add(1);
        } else {
            self.counters.misses = self.counters.misses.saturating_add(1);
        }
        row
    }

    fn insert(&mut self, key: ComposedRowKey, row: Arc<ComposedRow>) {
        let resident_bytes = composed_row_resident_bytes(&key, &row);
        let (_, evictions) = self.entries.insert(key, row, resident_bytes);
        self.counters.evictions = self.counters.evictions.saturating_add(evictions);
        self.counters.resident_bytes = self.entries.resident_bytes();
    }
}

/// Resident-byte approximation used for every cache budget and reported verbatim in perf traces.
///
/// cosmic-text does not expose the capacities of its internal shaping/layout caches. We charge the
/// public `Buffer`/`BufferLine` values, UTF-8 text bytes, and one `LayoutGlyph` per visible layout
/// glyph. Row entries additionally charge cell/string capacities and glyph-vector capacities.
/// Each row glyph conservatively charges its referenced buffer in full, so shared `Arc<Buffer>`
/// allocations are double-counted rather than silently omitted after the shape-cache entry is
/// evicted. Hash bucket slack and allocator headers remain unmeasurable without a heap profiler.
fn buffer_resident_bytes(buffer: &Buffer) -> usize {
    let line_bytes = buffer
        .lines
        .iter()
        .map(|line| size_of::<glyphon::cosmic_text::BufferLine>() + line.text().len())
        .sum::<usize>();
    let glyph_count = buffer
        .layout_runs()
        .map(|run| run.glyphs.len())
        .sum::<usize>();
    size_of::<Buffer>()
        .saturating_add(line_bytes)
        .saturating_add(glyph_count.saturating_mul(size_of::<glyphon::cosmic_text::LayoutGlyph>()))
}

fn shape_entry_resident_bytes(key: &ShapeKey, buffer: &Buffer, value_bytes: usize) -> usize {
    size_of::<Arc<ShapeKey>>()
        .saturating_add(3 * size_of::<usize>())
        .saturating_add(size_of::<ShapeKey>())
        .saturating_add(key.text.heap_bytes())
        .saturating_add(value_bytes)
        .saturating_add(buffer_resident_bytes(buffer))
}

fn captured_cell_resident_bytes(cell: &CapturedCell) -> usize {
    size_of::<CapturedCell>()
        .saturating_add(cell.text.heap_bytes())
        .saturating_add(cell.hyperlink.as_ref().map_or(0, |hyperlink| {
            hyperlink
                .uri
                .capacity()
                .saturating_add(hyperlink.id.as_ref().map_or(0, String::capacity))
        }))
}

fn composed_row_resident_bytes(key: &ComposedRowKey, row: &ComposedRow) -> usize {
    let key_bytes = size_of::<ComposedRowKey>()
        .saturating_add(
            key.cells
                .iter()
                .map(captured_cell_resident_bytes)
                .sum::<usize>(),
        )
        .saturating_add(
            key.status_overlay
                .as_ref()
                .map_or(0, |status| status.capacity()),
        );
    let narrow_buffer_bytes = row
        .narrow_glyphs
        .iter()
        .map(|glyph| buffer_resident_bytes(&glyph.buffer))
        .sum::<usize>();
    let wide_buffer_bytes = row
        .wide_glyphs
        .iter()
        .map(|glyph| buffer_resident_bytes(&glyph.buffer))
        .sum::<usize>();
    size_of::<LruNode<ComposedRowKey, Arc<ComposedRow>>>()
        .saturating_add(key_bytes)
        .saturating_add(size_of::<ComposedRow>())
        .saturating_add(
            row.narrow_glyphs
                .capacity()
                .saturating_mul(size_of::<NarrowGlyph>()),
        )
        .saturating_add(
            row.wide_glyphs
                .capacity()
                .saturating_mul(size_of::<WideGlyph>()),
        )
        .saturating_add(narrow_buffer_bytes)
        .saturating_add(wide_buffer_bytes)
}

#[derive(Clone, Copy, Debug, Default)]
struct TextPreparationStats {
    elapsed: Duration,
    rows_reshaped: u64,
    row_cache: RowCacheCounters,
    narrow: ShapeCacheCounters,
    wide: ShapeCacheCounters,
}

struct WideGlyph {
    column: usize,
    buffer: Arc<Buffer>,
    left_offset_px: f32,
    top_offset_px: f32,
    color: Color,
}

struct NarrowGlyph {
    column: usize,
    buffer: Arc<Buffer>,
    left_offset_px: f32,
    top_offset_px: f32,
    color: Color,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ShapeKey {
    /// The cluster itself, inline. A key is built for *every* non-blank cell on
    /// *every* row compose — only the misses reach the shaper, but every one of
    /// them used to reach the allocator first.
    text: CellText,
    bold: bool,
    italic: bool,
}

struct CachedNarrowShape {
    buffer: Arc<Buffer>,
    left_offset_px: f32,
    top_offset_px: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ShapeCacheCounters {
    hits: u64,
    misses: u64,
    evictions: u64,
    miss_time: Duration,
    resident_bytes: usize,
}

impl ShapeCacheCounters {
    fn delta_since(self, earlier: Self) -> Self {
        Self {
            hits: self.hits.saturating_sub(earlier.hits),
            misses: self.misses.saturating_sub(earlier.misses),
            evictions: self.evictions.saturating_sub(earlier.evictions),
            miss_time: self.miss_time.saturating_sub(earlier.miss_time),
            resident_bytes: self.resident_bytes,
        }
    }
}

struct NarrowShapingCache {
    entries: ByteLru<ShapeKey, CachedNarrowShape>,
    track_perf: bool,
    counters: ShapeCacheCounters,
    #[cfg(test)]
    color_emoji_trial_shapes: u64,
}

impl NarrowShapingCache {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_perf_tracking(false)
    }

    fn with_perf_tracking(track_perf: bool) -> Self {
        Self {
            entries: ByteLru::new(NARROW_SHAPING_CACHE_BUDGET_BYTES),
            track_perf,
            counters: ShapeCacheCounters::default(),
            #[cfg(test)]
            color_emoji_trial_shapes: 0,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.counters = ShapeCacheCounters::default();
        #[cfg(test)]
        {
            self.color_emoji_trial_shapes = 0;
        }
    }

    fn get_or_shape(
        &mut self,
        key: ShapeKey,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: CellMetrics,
    ) -> (Arc<Buffer>, f32, f32) {
        if let Some(cached) = self.entries.get(&key) {
            if self.track_perf {
                self.counters.hits = self.counters.hits.saturating_add(1);
            }
            return (
                Arc::clone(&cached.buffer),
                cached.left_offset_px,
                cached.top_offset_px,
            );
        }

        let miss_started = self.track_perf.then(Instant::now);
        let (mut buffer, family, size_policy) = shape_narrow_buffer_for_key(
            &key,
            font_system,
            swash_cache,
            metrics,
            #[cfg(test)]
            &mut self.color_emoji_trial_shapes,
        );
        let (left_offset_px, top_offset_px) = match size_policy {
            NarrowSizePolicy::StrictCell => {
                let em_scale = narrow_fallback_em_scale(
                    &buffer,
                    font_system,
                    swash_cache,
                    metrics.cell_width_px,
                );
                if em_scale < 1.0 {
                    buffer = shape_narrow_buffer(&key, font_system, metrics, em_scale, family);
                }
                let glyph_baseline_px = buffer
                    .layout_runs()
                    .next()
                    .map_or(metrics.ascii_baseline_px, |run| run.line_y);
                (
                    0.0,
                    baseline_offset_px(metrics.ascii_baseline_px, glyph_baseline_px),
                )
            }
            NarrowSizePolicy::TextCoordinated => {
                let em_scale = text_coordinated_symbol_em_scale(
                    &buffer,
                    font_system,
                    swash_cache,
                    metrics.cell_width_px,
                    metrics.primary_cap_height_px,
                );
                if (em_scale - 1.0).abs() > f32::EPSILON {
                    buffer = shape_narrow_buffer(&key, font_system, metrics, em_scale, family);
                }
                align_ink_offsets(
                    &buffer,
                    font_system,
                    swash_cache,
                    metrics.cell_width_px,
                    metrics.primary_cap_center_y_px,
                )
            }
            NarrowSizePolicy::ColorEmoji => center_ink_offsets(
                &buffer,
                font_system,
                swash_cache,
                metrics.cell_width_px,
                metrics.cell_height_px,
            ),
        };
        let buffer = Arc::new(buffer);
        let resident_bytes =
            shape_entry_resident_bytes(&key, &buffer, size_of::<CachedNarrowShape>());
        let (_, evictions) = self.entries.insert(
            key,
            CachedNarrowShape {
                buffer: Arc::clone(&buffer),
                left_offset_px,
                top_offset_px,
            },
            resident_bytes,
        );
        self.counters.evictions = self.counters.evictions.saturating_add(evictions);
        self.counters.resident_bytes = self.entries.resident_bytes();
        if let Some(miss_started) = miss_started {
            self.counters.misses = self.counters.misses.saturating_add(1);
            self.counters.miss_time = self
                .counters
                .miss_time
                .saturating_add(miss_started.elapsed());
        }
        (buffer, left_offset_px, top_offset_px)
    }
}

struct CachedWideShape {
    buffer: Arc<Buffer>,
    left_offset_px: f32,
    top_offset_px: f32,
}

struct WideShapingCache {
    entries: ByteLru<ShapeKey, CachedWideShape>,
    track_perf: bool,
    counters: ShapeCacheCounters,
    #[cfg(test)]
    color_emoji_trial_shapes: u64,
}

impl WideShapingCache {
    #[cfg(test)]
    fn new() -> Self {
        Self::with_perf_tracking(false)
    }

    fn with_perf_tracking(track_perf: bool) -> Self {
        Self {
            entries: ByteLru::new(WIDE_SHAPING_CACHE_BUDGET_BYTES),
            track_perf,
            counters: ShapeCacheCounters::default(),
            #[cfg(test)]
            color_emoji_trial_shapes: 0,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.counters = ShapeCacheCounters::default();
        #[cfg(test)]
        {
            self.color_emoji_trial_shapes = 0;
        }
    }

    fn get_or_shape(
        &mut self,
        key: ShapeKey,
        font_system: &mut FontSystem,
        swash_cache: &mut SwashCache,
        metrics: CellMetrics,
    ) -> (Arc<Buffer>, f32, f32) {
        if let Some(cached) = self.entries.get(&key) {
            if self.track_perf {
                self.counters.hits = self.counters.hits.saturating_add(1);
            }
            return (
                Arc::clone(&cached.buffer),
                cached.left_offset_px,
                cached.top_offset_px,
            );
        }

        let miss_started = self.track_perf.then(Instant::now);
        let (buffer, size_policy) = shape_wide_buffer_for_key(
            &key,
            font_system,
            swash_cache,
            metrics,
            #[cfg(test)]
            &mut self.color_emoji_trial_shapes,
        );
        let (left_offset_px, top_offset_px) = match size_policy {
            WideSizePolicy::MonospaceSlot => {
                let glyph_baseline_px = buffer
                    .layout_runs()
                    .next()
                    .map_or(metrics.ascii_baseline_px, |run| run.line_y);
                (
                    0.0,
                    baseline_offset_px(metrics.ascii_baseline_px, glyph_baseline_px),
                )
            }
            WideSizePolicy::ColorEmojiBox { .. } => center_ink_offsets(
                &buffer,
                font_system,
                swash_cache,
                2.0 * metrics.cell_width_px,
                metrics.cell_height_px,
            ),
        };
        let buffer = Arc::new(buffer);
        let resident_bytes =
            shape_entry_resident_bytes(&key, &buffer, size_of::<CachedWideShape>());
        let (_, evictions) = self.entries.insert(
            key,
            CachedWideShape {
                buffer: Arc::clone(&buffer),
                left_offset_px,
                top_offset_px,
            },
            resident_bytes,
        );
        self.counters.evictions = self.counters.evictions.saturating_add(evictions);
        self.counters.resident_bytes = self.entries.resident_bytes();
        if let Some(miss_started) = miss_started {
            self.counters.misses = self.counters.misses.saturating_add(1);
            self.counters.miss_time = self
                .counters
                .miss_time
                .saturating_add(miss_started.elapsed());
        }
        (buffer, left_offset_px, top_offset_px)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NarrowCellSlot {
    column: usize,
    /// The cell's own storage, not a copy of it on the heap. One `String` per
    /// non-blank cell, rebuilt on every row compose, was the second half of the
    /// allocation storm [`CellText`] exists to end.
    text: CellText,
    style: CellStyle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WideCellSlot {
    column: usize,
    text: CellText,
    style: CellStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrepareFailurePolicy {
    PresentWithoutText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceFailure {
    Unavailable,
    Outdated,
    Lost,
    Validation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceFailurePolicy {
    Skip,
    Reconfigure,
    FatalValidation,
}

fn prepare_failure_policy(error: PrepareError) -> PrepareFailurePolicy {
    match error {
        PrepareError::AtlasFull => PrepareFailurePolicy::PresentWithoutText,
    }
}

fn surface_failure_policy(failure: SurfaceFailure) -> SurfaceFailurePolicy {
    match failure {
        SurfaceFailure::Unavailable => SurfaceFailurePolicy::Skip,
        SurfaceFailure::Outdated | SurfaceFailure::Lost => SurfaceFailurePolicy::Reconfigure,
        SurfaceFailure::Validation => SurfaceFailurePolicy::FatalValidation,
    }
}

/// The glyphon state one Terminal seat needs to put text on the glass.
///
/// Grid text and the status overlay travel together because they are laid out
/// in the same seat-local coordinate space and therefore share the seat's
/// `Viewport`; they need two renderers rather than one because the status bar
/// draws *over* a rectangle that is itself drawn over the grid, and a single
/// prepared batch cannot be interleaved with a pipeline change.
struct SeatTextSlot {
    viewport: Viewport,
    text_renderer: TextRenderer,
    status_text_renderer: TextRenderer,
}

/// Everything one process's GPU costs once — the device, the shared glyph
/// atlas, the two pipelines, the font system — and nothing that belongs to a
/// single window.
///
/// # Why this is a layer and not a field bag
///
/// A second OS window is a second `wgpu::Surface`, not a second renderer. The
/// multiwindow spike (2026-08-12, Q2) measured which side of that line every
/// resource falls on and this struct is that column of the table: an
/// `Instance`, an `Adapter`, a `Device` and a `Queue` can serve any number of
/// surfaces, one `Cache`/`TextAtlas` pair holds every window's glyphs, and one
/// [`FontSystem`] spares each new window the thirteen-file fallback chain
/// [`terminal_font_system`] loads from disk. What may *not* be shared lives on
/// [`WindowRenderer`].
///
/// Three constraints travel with it, and all three are enforced here rather
/// than trusted:
///
/// 1. **The format is a hinge.** [`TextAtlas::new`] and both pipelines bake the
///    surface's [`wgpu::TextureFormat`] in. A window whose swapchain resolves to
///    a different format cannot share this context — that is a second renderer,
///    not a second swapchain — so [`GpuContext::accept_format`] returns
///    [`RenderError::FormatMismatch`] instead of quietly minting a second atlas.
/// 2. **prepare→render is a pair.** See [`WindowRenderer::present_frame`].
/// 3. **Caches keyed by pixel font size follow the surface, not the device.**
///    They are on [`WindowRenderer`]; two windows on monitors of different
///    scale factors are two sets of pixel font sizes, and one cache holding both
///    would evict or pollute per frame.
pub struct GpuContext {
    /// Kept — not dropped after `new` as it once was — because a second
    /// `Surface` must be created by the same `Instance` that created the
    /// `Device`, and `get_default_config`/`get_capabilities` must be asked of
    /// the same `Adapter`.
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The format the atlas and both pipelines were built for. The whole of the
    /// sharing contract: a surface that cannot resolve to this is not this
    /// context's window.
    format: wgpu::TextureFormat,
    max_texture_dimension_2d: u32,
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Kept so a window — or a seat that appears mid-session (a split) — can
    /// mint its own viewport without rebuilding anything.
    glyphon_cache: Cache,
    atlas: TextAtlas,
    rect_pipeline: wgpu::RenderPipeline,
    /// The same pipeline blending `Replace`, for the chrome quads that *are*
    /// the window's ground — see [`ChromeSurface::Ground`].
    ground_rect_pipeline: wgpu::RenderPipeline,
    /// The same again, cross-faded by the pass's blend constant, for a ground on
    /// a floating layer that carries its own opacity — see [`OverlayGround`].
    ground_fade_rect_pipeline: wgpu::RenderPipeline,
    math_pipeline: wgpu::RenderPipeline,
    math_bind_group_layout: wgpu::BindGroupLayout,
    math_sampler: wgpu::Sampler,
    background_pipeline: wgpu::RenderPipeline,
    background_bind_group_layout: wgpu::BindGroupLayout,
    background_sampler: wgpu::Sampler,
    /// The window's ground picture, uploaded once and keyed by its content.
    ///
    /// **One slot and not an entry in `math_textures`**, for two reasons that
    /// both come from it being one picture for the whole process. It never
    /// competes: a wallpaper is tens of megabytes against a 64 MiB budget shared
    /// with every formula, mark and preview on screen, and dropping it in there
    /// would evict most of them once per frame and then be evicted itself.
    /// And it never needs finding: there is at most one, so a hash lookup per
    /// frame would be answering a question with one possible answer.
    background_texture: Option<(String, wgpu::BindGroup)>,
    /// Content-keyed textures for math rasters, peek thumbnails, chrome marks
    /// and preview pictures. Shared with the device because the bytes are the
    /// same bytes whichever window asks for them.
    math_textures: ByteLru<String, CachedMathTexture>,
    math_texture_evictions: u64,
    /// The chrome sans face's cap height per em, resolved once: it is a property
    /// of the face, so neither a DPI change nor a new title nor a second window
    /// can move it.
    ///
    /// **Nothing in the Terminal font row touches this.** The grid's face is a
    /// setting; the chrome's is not, and this field is one of the two reasons
    /// why (`docs/DESIGN.md` §7.1.6c-3b names the other). A settings row that
    /// changed the sans face would leave this ratio describing the face it
    /// replaced, and every chrome label would sit off its own centre line.
    chrome_cap_height_ratio: f32,
    /// How large the **grid's** face is drawn, in logical pixels, for every
    /// window on this device.
    ///
    /// On the device and not on the window because the `FontSystem` it is a
    /// property of is on the device: one font database serves every window, so
    /// two windows cannot be at two sizes any more than they can be in two
    /// families. A per-window size wants a per-window database, which is the
    /// cost this renderer's shared-database design exists to avoid.
    terminal_font_size_logical_px: f32,
    /// The device-layer half of [`RendererInitTimings`]. `surface_configure`
    /// is a window's cost and is stored on [`WindowRenderer`]; the field is left
    /// zero here and filled in by [`WindowRenderer::init_timings`].
    init_timings: RendererInitTimings,
}

/// What a window's swapchain is built upon.
///
/// # Two doors, and the alpha they are offered is the difference
///
/// wgpu's dx12 backend answers a window-handle target with exactly one
/// composite alpha mode — `vec![Opaque]`,
/// `wgpu-hal-30.0.0/src/dx12/adapter.rs:1364` — and a
/// **composition-visual** target with the whole set, `PreMultiplied` among them.
/// That single line is why this enum exists: a surface that will one day have a
/// hole cut in it for a web preview to show through has to be `PreMultiplied`,
/// and no amount of configuring an HWND swapchain will make it so.
///
/// Both arms produce the same picture today. Nothing above this layer knows
/// which door it came through except [`SurfaceAlphaReport`], which records the
/// answer for the startup trace.
pub enum WindowTarget {
    /// The window itself — on Windows, its `HWND`. wgpu builds the swapchain
    /// with `CreateSwapChainForHwnd` and the desktop compositor owns the
    /// presentation entirely.
    Hwnd(wgpu::SurfaceTarget<'static>),
    /// An `IDCompositionVisual` the caller owns, as a raw COM pointer.
    ///
    /// wgpu takes a reference on it and holds that reference for the surface's
    /// whole life, so the pointer only has to be a live visual **at the moment
    /// the surface is created**. What the caller keeps owing afterwards is the
    /// commit: see `bt_platform::Compositor::commit`.
    CompositionVisual(*mut std::ffi::c_void),
}

/// Which of [`WindowTarget`]'s two doors a window came through, kept after the
/// target itself has been consumed into a surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowTargetKind {
    Hwnd,
    CompositionVisual,
}

impl WindowTarget {
    #[must_use]
    fn kind(&self) -> WindowTargetKind {
        match self {
            Self::Hwnd(_) => WindowTargetKind::Hwnd,
            Self::CompositionVisual(_) => WindowTargetKind::CompositionVisual,
        }
    }
}

/// What the adapter offered this surface and what it was configured with.
///
/// Kept so the startup trace can print both, exactly as the WebView2 spike
/// printed them — the offered list is the evidence for the chosen mode, and
/// reading the chosen mode alone would leave "why not the other one" unanswered
/// on a machine where it goes wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceAlphaReport {
    pub target: WindowTargetKind,
    pub offered: Vec<wgpu::CompositeAlphaMode>,
    pub chosen: wgpu::CompositeAlphaMode,
}

impl SurfaceAlphaReport {
    /// Whether this surface can carry a translucent ground (§7.1.6c-4b).
    ///
    /// A method rather than a comparison at the call site, so that `bt-app` never
    /// has to name a `wgpu` enum to answer a question about its own settings
    /// page — the same boundary `Theme` keeps against `ThemeModeV1`. It is also
    /// the one place the equivalence "premultiplied ⇒ the ground may be
    /// translucent" is written down.
    #[must_use]
    pub fn is_premultiplied(&self) -> bool {
        self.chosen == wgpu::CompositeAlphaMode::PreMultiplied
    }
}

/// The composite alpha mode a target **must** be configured with.
///
/// Not a preference and not a search through a ranked list: each door has one
/// right answer, and the surface that cannot give it is a surface this program
/// will not present through. An HWND target is `Opaque` because that is the
/// only thing dx12 offers it; a visual target is `PreMultiplied` because that
/// is the whole reason for going through a visual at all, and configuring one
/// `Opaque` would build the ground for the web slice and then pave over it.
#[must_use]
fn required_alpha_mode(target: WindowTargetKind) -> wgpu::CompositeAlphaMode {
    match target {
        WindowTargetKind::Hwnd => wgpu::CompositeAlphaMode::Opaque,
        WindowTargetKind::CompositionVisual => wgpu::CompositeAlphaMode::PreMultiplied,
    }
}

/// Assert the adapter offered what this target requires, and say what it did
/// offer when it did not.
///
/// Pure, and separated from the surface for the reason every gate in this file
/// is: a headless probe cannot make a composition visual, so the only way this
/// decision can be held by a test is to hand it the list rather than a surface.
fn choose_alpha_mode(
    target: WindowTargetKind,
    offered: &[wgpu::CompositeAlphaMode],
) -> Result<wgpu::CompositeAlphaMode, RenderError> {
    let required = required_alpha_mode(target);
    if offered.contains(&required) {
        Ok(required)
    } else {
        Err(RenderError::AlphaModeUnavailable {
            target,
            required,
            offered: offered.to_vec(),
        })
    }
}

/// Build the surface one of [`WindowTarget`]'s two doors names.
///
/// # The one `unsafe` in this crate, and why it is here rather than in `bt-platform`
///
/// `bt-platform` is this workspace's deliberately narrow `unsafe` boundary and
/// everything else inherits `unsafe_code = "deny"`. The exception is exactly one
/// call, `Instance::create_surface_unsafe`, and it cannot be moved: what it
/// produces is a `wgpu::Surface`, and `bt-platform` neither depends on wgpu nor
/// should start to — it would drag the whole graphics stack into the crate whose
/// entire point is being small enough to audit. The COM half of the arrangement
/// *is* in `bt-platform`: `Compositor` makes the visual, owns it and commits it.
/// What crosses the boundary is a raw pointer, and this is the one line that
/// dereferences it.
///
/// # SAFETY
///
/// wgpu asks for a live `IDCompositionVisual` at the moment of the call and
/// nothing more: it takes its own reference on the visual and holds it for the
/// surface's whole life
/// (`wgpu-hal-30.0.0/src/dx12/mod.rs:551`, `from_raw_borrowed(..).to_owned()`).
/// The pointer arrives from `bt_platform::Compositor::gpu_visual_ptr`, which
/// returns the raw pointer of a visual that `Compositor` owns; the caller holds
/// that `Compositor` across this call. A null or dangling pointer is therefore
/// not reachable from any caller in this program, and no caller outside it can
/// construct a `WindowTarget::CompositionVisual` without reading the safety
/// note on the variant.
#[allow(unsafe_code)]
fn create_surface(
    instance: &wgpu::Instance,
    target: WindowTarget,
) -> Result<wgpu::Surface<'static>, RenderError> {
    match target {
        WindowTarget::Hwnd(target) => instance.create_surface(target),
        WindowTarget::CompositionVisual(visual) => unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(visual))
        },
    }
    .map_err(|error| RenderError::Wgpu(error.to_string()))
}

/// Where one [`WindowRenderer`] puts its frame.
///
/// A swapchain image and an offscreen attachment differ in exactly two places —
/// how the frame is acquired and whether anything is handed back to the
/// compositor — and in nothing else: same format, same size discipline, same
/// pass. Keeping them one type is what lets the headless probe be a real window
/// renderer instead of a parallel copy of one.
enum FrameTarget {
    Surface(Box<wgpu::Surface<'static>>),
    Offscreen(wgpu::Texture),
}

/// What a window owes the compositor once its pass closes: a swapchain image is
/// handed back through `Queue::present`, and an offscreen attachment is not
/// handed back at all.
enum AcquiredFrame {
    Swapchain(wgpu::SurfaceTexture),
    Offscreen,
}

/// What acquiring this frame's attachment said, decided before anything is done
/// about it so the borrow of [`FrameTarget`] ends first — the reconfigure a
/// suboptimal swapchain asks for needs `&mut self`.
enum SurfaceAcquisition {
    Frame(wgpu::SurfaceTexture),
    Suboptimal(wgpu::SurfaceTexture),
    Failed(SurfaceFailure),
    Offscreen(wgpu::TextureView),
}

/// One window's half of the renderer: its surface, its resolution, its metrics,
/// and every cache that is keyed by a pixel font size.
///
/// Constructible against a [`GpuContext`] any number of times — that is the
/// point of the split — and each instance keeps its own [`CellMetrics`],
/// composed-row cache and shaping caches, because those are keyed by pixel font
/// size and two windows at 1.5x and 2.0x are two sets of pixel font sizes
/// (spike Q3).
pub struct WindowRenderer {
    target: FrameTarget,
    config: wgpu::SurfaceConfiguration,
    /// What the adapter offered this surface and what it was configured with,
    /// or `None` for a window drawing into a texture — an offscreen attachment
    /// is never composited and has no alpha mode to report.
    alpha_report: Option<SurfaceAlphaReport>,
    configured_size: (u32, u32),
    /// The terminal seat's rectangle inside the swapchain (§4.1). Equal to the
    /// whole surface whenever the tree is a lone terminal leaf.
    seat: SeatViewport,
    chrome_quads: Vec<ChromeQuad>,
    chrome_labels: Vec<ChromeLabel>,
    chrome_icons: Vec<ChromeIcon>,
    /// The modal overlay's own stack. Kept apart from the chrome's lists rather
    /// than appended to them because the two are drawn in different places in the
    /// frame: seat chrome owns the space between seats, and a modal owns the
    /// window — including the seats' own content, which is drawn *after* chrome.
    ///
    /// A stack rather than one triple because a popup inside a dialog has to
    /// cover the dialog in every channel, not just in the one it happens to draw
    /// its own surface with — see [`OverlayLayer`].
    overlay_layers: Vec<OverlayLayer>,
    /// One glyphon viewport and text-renderer pair per Terminal seat, grown on
    /// demand — the same shape, and for the same reason, as
    /// [`WindowRenderer::overlay_text_renderers`].
    ///
    /// Two constraints force it. A glyphon `Viewport` is a GPU uniform holding
    /// *one* resolution, and grid text is laid out in seat-local pixels, so two
    /// seats of different sizes cannot share one. And a `TextRenderer` holds
    /// exactly one prepared batch, so preparing a second seat into it would
    /// destroy the first seat's glyphs before the pass ever ran.
    ///
    /// Slot 0 is built in `new` and is the slot a lone terminal leaf uses on
    /// every frame it ever draws, which is the whole of the N = 1 identity
    /// argument: same object, same resolution, same batch, same draw call.
    seat_slots: Vec<SeatTextSlot>,
    /// A second glyphon viewport whose resolution names the whole surface rather
    /// than the seat, so chrome text can be positioned in window coordinates
    /// while grid text stays in seat-local ones.
    chrome_viewport: Viewport,
    chrome_text_renderer: TextRenderer,
    /// One text renderer per overlay layer, grown on demand: a glyphon renderer
    /// holds one prepared batch, so two layers of text are two renderers.
    overlay_text_renderers: Vec<TextRenderer>,
    /// Artifacts the byte budget refused outright, and visible blocks left without a texture. Both
    /// used to be silent `continue`s; a band that draws its placement but not its pixels is a bare
    /// rectangle on screen, so it is counted where the frame trace can see it.
    ///
    /// Counted per window rather than beside the shared LRU: they are read by
    /// `BT_PERF_TRACE`, which reports one window's frame.
    math_texture_refusals: u64,
    textureless_math_blocks: u64,
    /// The device limit this window's swapchain is clamped by, copied from the
    /// [`GpuContext`] that built it (multiwindow slice C).
    ///
    /// A window is built against exactly one device and cannot migrate to
    /// another, so this is a property of the window as much as of the device —
    /// and reading it here rather than through a borrow is what lets
    /// [`Self::presentation_geometry`] be a question about this window alone.
    /// Two windows on one device hold the same number by construction; there is
    /// no moment at which they could disagree, because the only writer is the
    /// constructor.
    max_texture_dimension_2d: u32,
    metrics: CellMetrics,
    /// This window's share of [`RendererInitTimings`] — configuring its own
    /// swapchain, and measuring the cell against its own scale factor. The rest
    /// of that report is the device layer's and is charged once per process.
    surface_configure_time: Duration,
    font_metrics_time: Duration,
    text_rows: Vec<Arc<ComposedRow>>,
    status_overlay: Option<Arc<ComposedRow>>,
    composed_row_cache: ComposedRowCache,
    font_revision: u64,
    narrow_shaping_cache: NarrowShapingCache,
    wide_shaping_cache: WideShapingCache,
    glyph_degraded_frames: u64,
    window_focused: bool,
    cursor_blink_visible: bool,
    peek_overlay: Option<PeekImageOverlay>,
    preview_image: Option<PreviewImage>,
    preview_bodies: Vec<PreviewBody>,
    /// **What the last frame did with the preview documents it was handed** —
    /// see [`PreviewTextFrame`]. Written on every present, read by whoever is
    /// tracing; nothing in this module branches on it.
    preview_text_frame: PreviewTextFrame,
    /// This frame's table pictures, keyed by the source text of the block each belongs to.
    table_blocks: HashMap<String, TableBlockPaint>,
    preview_text_renderer: TextRenderer,
    trace_perf: bool,
    perf_frame: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationGeometry {
    /// Effective physical pixel size used by the current surface configuration.
    pub swapchain_size: (u32, u32),
    /// Per-axis device limit applied to the swapchain size.
    pub max_texture_dimension_2d: u32,
}

/// The rectangle of the swapchain the terminal draws itself into, in physical
/// pixels.
///
/// This is the seam `docs/M2-layout-solver-spec.md` §4.1 names: the solver hands
/// out seat rectangles, and the terminal seat's rectangle arrives here. The
/// terminal's own frame machinery never learns that it moved — every pixel it
/// computes is still relative to its own top-left, and the whole translation is
/// one `set_viewport`/`set_scissor_rect` pair plus a glyphon `Resolution` that
/// names the seat instead of the window.
///
/// A lone terminal leaf solves to the whole viewport, so the seat is
/// `(0, 0, config.width, config.height)` and every expression below is
/// numerically the one that was there before this type existed. That is the
/// byte-identity argument, and it is an argument about *values*, not about a
/// branch that skips the new code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeatViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SeatViewport {
    /// The whole surface: what a lone leaf solves to.
    #[must_use]
    pub const fn whole(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// Clamped so the rectangle is inside `width` x `height` and never empty.
    #[must_use]
    pub(crate) fn clamped_to(self, width: u32, height: u32) -> Self {
        let x = self.x.min(width.saturating_sub(1));
        let y = self.y.min(height.saturating_sub(1));
        Self {
            x,
            y,
            width: self.width.clamp(1, width.saturating_sub(x).max(1)),
            height: self.height.clamp(1, height.saturating_sub(y).max(1)),
        }
    }
}

/// One terminal leaf's content, plus the rectangle it draws into.
///
/// A tab holds one of these per Terminal seat. The pair is the whole of what
/// multi-session presentation adds: [`ViewportFrame`] deliberately carries no
/// seat and no session identity — its `layout_key` is a *shape* (width, DPI,
/// font and theme revisions), so two shells the same width at the same DPI
/// produce equal keys — and that is why the rectangle has to arrive beside the
/// frame rather than be read out of it.
///
/// `N = 1` is not a special case anywhere below it. A lone terminal leaf solves
/// to the whole seat, so a one-element slice reproduces the single-frame command
/// stream value for value; the loop that draws it is the loop that draws four.
#[derive(Clone, Copy, Debug)]
pub struct SeatFrame<'a> {
    /// Where this leaf's body lands in the swapchain, from `solve` (red line
    /// L10: the renderer never invents it).
    ///
    /// This is where the contents were *laid out* and where their top-left
    /// lands: the pass viewport, and the glyphon `Resolution` the grid's pixels
    /// are normalized by. Its extent is therefore the size the grid was reflowed
    /// to, and moving it never reflows anything.
    pub seat: SeatViewport,
    /// The box those contents are allowed to *appear* in — the pass scissor.
    ///
    /// At rest it is [`Self::seat`], and then every value below is the one that
    /// was there when one rectangle drove both calls: the same argument
    /// [`SeatViewport`]'s own doc makes for `N = 1`, about values rather than
    /// about a branch that skips the new code.
    ///
    /// They part company for exactly one reason, and it is U8's pane FLIP. The
    /// mock-up animates a pane with `transform: scale(s)` on the box and
    /// `scale(1/s)` on an inner wrapper (6584-6586), because in CSS a transform
    /// is the only way to move a box that is already laid out — the
    /// counter-scale is there to *undo* the stretch the outer scale would
    /// otherwise put on the text. Composed, the pair says: contents laid out at
    /// the final size, placed at the animating box's top-left, clipped by the
    /// pane's `overflow: hidden`. Native has that composition directly, which is
    /// this pair of rectangles, and so nothing here is ever scaled — not a
    /// glyph, not a quad, not a viewport. Transcribing the CSS literally by
    /// scaling this crate's viewport would reflow the grid every frame of the
    /// animation, which is the exact stretch the counter-scale was invented to
    /// cancel.
    ///
    /// Must be clamped inside the surface and never empty: the scissor is
    /// validated against the attachment, while the viewport above legitimately
    /// hangs off its edge — a pane growing leftward is laid out at its final
    /// width from its old corner, so its right edge is past the window's until
    /// the flight lands.
    pub clip: SeatViewport,
    pub frame: &'a ViewportFrame,
    /// Whether this is the seat holding keyboard focus.
    ///
    /// It is what the caret is drawn from. A window nobody is looking at fades
    /// every caret in it; inside a window that *is* being looked at, exactly one
    /// pane is being typed into, and the panes beside it are in the same position
    /// as a whole window that lost focus — so they wear the same faded shape and
    /// stand outside the blink. Composed, the rule is one conjunction:
    /// `window_focused && seat.focused`.
    pub focused: bool,
}

/// One flat rectangle of seat chrome, in physical pixels of the whole surface.
///
/// Chrome is everything the solver produced that is not a seat's interior:
/// dividers, a preview's title bar and body, a collapsed seat's bar. It is drawn
/// after the terminal seat with the pass restored to the whole surface, so it is
/// the one class of draw that legitimately knows where the window's edges are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeQuad {
    /// `[left, top, right, bottom]`.
    pub rect: [f32; 4],
    pub color: [u8; 3],
    /// Whether this rectangle **is** the window at that place, or something
    /// struck on it — see [`ChromeSurface`].
    pub surface: ChromeSurface,
}

/// Which of the two things a flat chrome rectangle is (user ruling 2026-08-18,
/// "one translucency"; `docs/DESIGN.md` §7.1.6c-4b).
///
/// §7.1.6c-4b's Background opacity row says "panes **and the window ground**;
/// text and menus stay opaque", and until this ruling only one of those two
/// nouns was true: the pane bodies are the clear and were translucent, while
/// every band the chrome painted — the tab strip, the rail, each pane's head,
/// the floor under a files column — was an opaque lid laid on top of the very
/// window it is part of. Measured at 30% over a solid `#00C800` desktop: pane
/// body `(30, 170, 30)`, tab strip `(37, 37, 37)`, pane head `(27, 27, 27)` —
/// the ground let the desktop through and the chrome did not.
///
/// The distinction is **not** "chrome versus terminal". It is the one the ruling
/// draws: a *ground* is the window's own surface wearing another name, and takes
/// the window's alpha; *ink* is everything struck on a ground — a hairline, a
/// hover fill, a pill, a divider, a caption button's plate — and stays opaque,
/// because it is a mark on the glass rather than the glass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChromeSurface {
    /// Struck on a ground, and therefore opaque: the palette pre-composited it
    /// against the ground it lands on, which is only true of a colour that is
    /// actually laid over that ground.
    #[default]
    Ink,
    /// The window's ground at this rectangle. Drawn with the clear's own
    /// premultiplied arithmetic at [`WindowGround::alpha`], so a window at 30%
    /// is 30% here too — one sheet of glass, not a translucent hole in an
    /// opaque one.
    ///
    /// **A ground that floats travels [`OverlayGround`] instead.** The vertical
    /// rail is drawn on a level of the overlay stack rather than in the chrome
    /// pass, and for nine days the class was simply dropped at that lift —
    /// §7.1.6c-4f's second amendment, and the reason the rail was the one band
    /// in the window this enum did not reach.
    Ground,
}

impl ChromeQuad {
    /// A mark on the glass: the overwhelmingly common quad, and what every quad
    /// was before grounds were told apart from them.
    #[must_use]
    pub fn ink(rect: [f32; 4], color: [u8; 3]) -> Self {
        Self {
            rect,
            color,
            surface: ChromeSurface::Ink,
        }
    }

    /// The glass itself at this rectangle.
    #[must_use]
    pub fn ground(rect: [f32; 4], color: [u8; 3]) -> Self {
        Self {
            rect,
            color,
            surface: ChromeSurface::Ground,
        }
    }
}

/// One line of seat chrome text.
#[derive(Clone, Debug, PartialEq)]
pub struct ChromeLabel {
    pub text: String,
    /// The box the text is laid out in, `[left, top, right, bottom]`.
    ///
    /// Laying out is the whole of its job now. It used to be clipped to as well
    /// — one rectangle doing two things, which cost nothing while the two were
    /// always the same box. U8's pane FLIP is the first caller for which they
    /// differ: a pane mid-flight is drawn at its final *size* through a clip box
    /// that is still the size it left, so the caption is laid out in the box the
    /// solver gave it and shown through a narrower one. Intersecting this rect
    /// instead of adding [`Self::clip`] beside it would re-do the layout inside
    /// the smaller box — a centred body notice would re-centre itself on the
    /// visible sliver and slide as the pane grew, and a right-aligned control
    /// would walk inward — which is the stretching the counter-scale exists to
    /// prevent, moved from the glyphs into their placement.
    pub rect: [f32; 4],
    /// The box the text is *clipped* to — glyphon's `TextArea.bounds`.
    ///
    /// `None` means "clipped to [`Self::rect`]", which is what every label that
    /// is not inside a moving pane wants and is the value that was there before
    /// this field existed. Set to `Some` only where the two genuinely part
    /// company, and always as an intersection *with* `rect`: a clip wider than
    /// the layout box would let a long title escape the head it belongs to.
    pub clip: Option<[f32; 4]>,
    pub font_size_px: f32,
    pub color: [u8; 3],
    /// Right-align inside `rect` rather than left-align. The `x` affordance of a
    /// title bar is the only user of this today.
    pub align_right: bool,
    /// Centre horizontally inside `rect`, overriding `align_right`. Seat body
    /// states (an empty preview's hint, "Loading …", a failure notice) use this;
    /// vertical centring is what every label already gets.
    pub align_center: bool,
    /// Extra advance between glyphs — CSS `letter-spacing`, **in em**, which is
    /// the unit cosmic-text's own `Attrs::letter_spacing` takes: it is added to
    /// a glyph's advance while that advance is still normalized by the face's
    /// units-per-em, and the sum is scaled by the font size afterwards. So this
    /// is a ratio and never needs the DPI scale applied to it.
    ///
    /// Zero for every label that does not ask for it. The settings dialog's
    /// group headings (`.group-label { letter-spacing: .05em }`) are the only
    /// user today: at 11px, uppercase and tracked is the whole difference
    /// between a heading and a small sentence.
    pub letter_spacing_em: f32,
    /// How heavy the face is drawn — CSS `font-weight`.
    ///
    /// Chrome text is `--ink*` at one weight almost everywhere, which is why
    /// this field did not exist until the pane-count badge needed it: the badge
    /// is `font-weight: 600` (mock-up line 296) and drawing it at the regular
    /// weight left a number too thin to read as a count against the tab title
    /// beside it.
    pub weight: ChromeLabelWeight,
    /// `font-variant-numeric: tabular-nums` — draw figures on one fixed
    /// advance instead of the face's proportional ones.
    ///
    /// Only text that *changes under a fixed layout* needs this. The pane-count
    /// badge (mock-up line 302) is the one such label today: it is a number,
    /// centred in a box, that counts up while the box stays put.
    pub tabular_numerals: bool,
    /// Set this line in the **terminal's** face rather than the chrome's.
    ///
    /// The distinction [`preview_run_attrs`] already draws, arriving in the
    /// chrome for the first time with §7.1.6b′'s F2 thumbnails: chrome is the
    /// window talking about itself and is set in the sans face, while a line
    /// lifted out of a running shell is a *document*, and that document's bytes
    /// were written in a grid. `Family::Monospace` resolves to the same family
    /// the terminal grid is set in (see [`monospace_family_name`]), so six rows
    /// of a session's tail shrunk onto a card keep the column alignment that is
    /// the whole reason a table, a progress bar or a `ls` listing is legible at
    /// all — set proportionally they are the same characters and a different
    /// picture.
    ///
    /// `false` everywhere else, and deliberately a field rather than a second
    /// label type: everything else about a mini transcript row — its box, its
    /// clip, its size, its ink — is what every other chrome label has, and a
    /// parallel struct would be a second shaping path to keep in step with this
    /// one.
    pub mono: bool,
}

/// `font-variant-numeric: tabular-nums` — the `tnum` `OpenType` feature.
///
/// The chrome's face draws proportional figures by default, and by a wide
/// margin: measured on Segoe UI Variable at the badge's own 10px, `1` advances
/// 3.79 against `0`'s 5.39. A count that changes width as it counts is exactly
/// the wobble the mock-up's declaration exists to stop.
const TABULAR_FIGURES: FeatureTag = FeatureTag::new(b"tnum");

/// The weights the chrome draws text at.
///
/// Three, because the mock-up asks for three and no more: everything is the
/// face's regular weight except `.panecount` at `600` and the focused pane
/// head at `500`. A general "any u16" field would be a wider promise than the
/// design makes and than the loaded face can keep.
///
/// The chrome's face is Windows 11's Segoe UI Variable, a variable font whose
/// default instance is `wght 400`; cosmic-text steers that axis from the shaping
/// attributes, so `SemiBold` is the real 600 instance of the same face rather
/// than a second file or a synthetic emboldening — and `Medium` is the real 500
/// instance, one interpolation step of the same axis.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChromeLabelWeight {
    /// The face's own regular weight — `wght 400`, and what every label that
    /// does not ask for otherwise gets.
    #[default]
    Regular,
    /// `font-weight: 500`, worn by `.pane.focused .panehead` (mock-up line
    /// 1644).
    ///
    /// The mock-up's own note there is that the focused pane is told apart by
    /// *hierarchy* rather than by a fill — "a title at `--ink` beside titles at
    /// `--ink3` is already a hierarchy" — and the weight is the second half of
    /// that hierarchy, not decoration on top of the colour.
    Medium,
    /// `font-weight: 600`, worn by `.panecount`.
    SemiBold,
}

impl ChromeLabelWeight {
    /// The shaping weight this maps to.
    fn shaping_weight(self) -> Weight {
        match self {
            Self::Regular => Weight::NORMAL,
            Self::Medium => Weight::MEDIUM,
            Self::SemiBold => Weight::SEMIBOLD,
        }
    }
}

/// One chrome mark, already rasterized to the exact physical box it occupies.
///
/// The mock-up draws every mark in the chrome — the profile square on a tab, the
/// file and folder marks on a pane head, the four caption glyphs, and the active
/// tab's own rounded silhouette — as SVG. Nothing here re-draws them from
/// primitives: the app rasterizes the mock-up's own `<symbol>` bodies at the
/// physical size the box wants and hands over the pixels, so what lands on screen
/// and what the design says are the same document, and the curves carry resvg's
/// analytic antialiasing rather than a staircase of nested rectangles.
///
/// `key` is the content identity — mark, physical size, and resolved colour —
/// and is what the shared GPU texture LRU is asked. Two icons with the same key
/// are the same pixels, which is what makes equality cheap enough to run on
/// every chrome rebuild.
#[derive(Clone, Debug)]
pub struct ChromeIcon {
    pub key: String,
    /// `[left, top, right, bottom]`, in physical pixels of the whole surface.
    pub rect: [f32; 4],
    pub rgba: Arc<[u8]>,
    pub width_px: u32,
    pub height_px: u32,
    /// A uniform multiplier on the raster's own alpha, `0.0 ..= 1.0`.
    ///
    /// Deliberately *not* part of [`Self::key`]: opacity is the one property of
    /// a mark that changes continuously, and folding it into the content
    /// identity would mint a new texture on every frame of a breath. The key
    /// stays "which mark, how big, what colour"; how faded it is this frame
    /// rides on the quad.
    ///
    /// The mock-up needs exactly this twice — `.ticon.working`'s breath and
    /// `.ticon-wrap.dead`'s .35 — and in both the artwork is unchanged and only
    /// its presence varies.
    pub opacity: f32,
    /// The box this raster may be **seen** in, in physical pixels of the whole
    /// surface — CSS `overflow: hidden` on the element it is drawn inside.
    ///
    /// `None` for every mark, and that is the honest default: a chrome mark is
    /// laid out to fit the control it belongs to, so it has nothing to be cropped
    /// by. What needs it is a *picture* — [`OverlayLayer::images`]' tenant — once
    /// the picture can be larger than the box it is looked at through. A preview
    /// float's zoomed image is exactly that: at 400% the drawn rectangle runs
    /// past the window's body on every side, and without a crop it would paint
    /// over that window's own head, its foot and the desk beside it.
    ///
    /// Applied by cropping the quad and its texture coordinates together rather
    /// than by a scissor, because a layer's marks are one draw list under one
    /// scissor and a per-icon scissor would break it into one draw per mark. The
    /// two are the same picture: a crop of the geometry with the matching crop of
    /// the sample rectangle is what a scissor does, computed once on the CPU.
    pub clip: Option<[f32; 4]>,
}

/// One flat fill of the modal overlay, in physical pixels of the whole surface.
///
/// The difference from [`ChromeQuad`] is `alpha`, and the difference is the whole
/// reason the type exists. Seat chrome is opaque by construction — every hairline
/// it draws sits on a surface the palette knows, so the palette pre-composites it
/// and the pipeline never has to blend. A modal overlay has no such surface under
/// it: a scrim is *defined* as "the window, dimmed", a dialog's own hairline lies
/// over the scrim over whatever the terminal is showing, and a rounded corner's
/// antialiasing is a coverage fraction and nothing else. All three are honest
/// only if the blend happens at draw time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayQuad {
    /// `[left, top, right, bottom]`.
    pub rect: [f32; 4],
    pub color: [u8; 3],
    /// `0.0 ..= 1.0`. Carries both the design's own alpha and, for a rounded
    /// shape, that pixel's coverage — already multiplied together.
    pub alpha: f32,
}

/// One rectangle of an overlay layer that **is** the window at that place —
/// [`ChromeSurface::Ground`] arriving on a floating level of the stack
/// (`docs/DESIGN.md` §7.1.6c-4f, the rail's amendment of 2026-08-18).
///
/// Its own type rather than an [`OverlayQuad`] with a flag, because the flag
/// would have to say what `alpha` then meant. An overlay quad's alpha is a
/// coverage — how much of a colour lands on an unknown surface — and a ground
/// has no such number to give: it does not land *on* the window, it **is** the
/// window there, at the window's own alpha and nobody else's. A type that cannot
/// express a coverage is the only way to say that once instead of in every
/// caller's comment.
///
/// The one fade a ground answers to is its **layer's** — CSS `opacity` on the
/// element the layer is — and that fade is a cross-fade towards what stands
/// under the layer rather than a coverage on the source. See
/// [`create_ground_fade_rect_pipeline`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayGround {
    /// `[left, top, right, bottom]`.
    pub rect: [f32; 4],
    pub color: [u8; 3],
}

/// One stacking layer of the modal overlay: its fills, its marks and its text.
///
/// The overlay draws in three channels — instanced rectangles, rasterized marks,
/// then shaped glyphs — and the channels have a fixed order inside the pass, so
/// "pushed later" only wins *within* a channel. A popup whose surface is a fill
/// and whose neighbours' captions are text is therefore not covered by being
/// pushed last: the text channel runs after every fill in the same layer, and the
/// captions come back out through the popup's face.
///
/// A layer is the fix, and it is the mock-up's own `z-index` in this pipeline's
/// terms: every channel of layer *n* is drawn before any channel of layer *n+1*,
/// so a popup on a layer of its own covers whatever the layers under it drew,
/// whichever channel that content went down and however many rows are added to
/// them later.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayLayer {
    /// This layer's **grounds**, drawn before its fills (§7.1.6c-4f).
    ///
    /// A channel of its own and not a class inside `quads`, for the reason the
    /// channels are channels at all: the order between them is fixed rather than
    /// argued about per caller, and a ground is the bottom of its own region by
    /// construction — it is the surface everything else in the layer is struck
    /// on. Seat chrome makes the same split with a field on the quad because its
    /// quads are one flat list with no layers to hang a channel from; here there
    /// are layers, so the channel is the cheaper true statement.
    ///
    /// Empty for every layer but the rail's. A menu, a dialog, a tip and a
    /// scrim are all things laid *over* the window — "text and menus stay
    /// opaque" — and the rail is the one floating level that is a panel of the
    /// window itself.
    pub grounds: Vec<OverlayGround>,
    pub quads: Vec<OverlayQuad>,
    pub labels: Vec<ChromeLabel>,
    pub icons: Vec<ChromeIcon>,
    /// The layer's own `opacity`, `0.0 ..= 1.0` — CSS `opacity` on the element
    /// this layer *is*, which is why it lives here and not on each fill.
    ///
    /// A layer is already the mock-up's `z-index` in this pipeline's terms (see
    /// above); `.tip { opacity: 0; transition: opacity .09s }` asks it to be the
    /// mock-up's `opacity` too, and for the same reason. A fading popup is one
    /// thing fading, not a fill and a hairline and a caption that each happen to
    /// be fading at the same rate: the moment they are separate the caller has to
    /// remember to fade all three, and the one it forgets is the one nobody looks
    /// at until it is wrong.
    ///
    /// CSS composites the element into a group and fades the group once, which
    /// this pipeline has no offscreen buffer to do; the fold is per primitive
    /// instead. The two answers differ only where a layer overlaps itself — a
    /// hairline showing faintly through the face laid over it — and only while
    /// `opacity` is strictly between 0 and 1. At the `--border` alphas the
    /// palette actually uses (.088/.094) the widest gap that opens is under 2.5%
    /// of an already-invisible ink, and it closes as the fade lands.
    pub opacity: f32,
    /// A scrolled **document** inside this layer (P43's second tenant).
    ///
    /// A preview float is a window with a file in it, and a file is not quads
    /// and captions: it is [`PreviewBody`] — runs in two faces, wrapped or not,
    /// under a clip, with blocks that scroll inside themselves. Handing it to
    /// `set_preview_bodies` instead would draw it in the pass *before* the
    /// overlays, which is the pass this layer's own window fill is drawn after —
    /// so the document would be behind the window that contains it.
    ///
    /// Riding here instead makes the z-order true by construction: a layer's
    /// three channels are finished before the next layer's are opened, so the
    /// document sits above its own window and below whatever stands over it.
    pub body: Option<PreviewBody>,
}

impl Default for OverlayLayer {
    /// An empty layer at full strength.
    ///
    /// Written out rather than derived for the reason `TabMarkState`'s is
    /// (`seats.rs`): a derived default would give `opacity` 0.0, and every layer
    /// built without naming it — which is every layer that existed before this
    /// field did — would draw nothing at all. CSS's own initial value is `1`,
    /// and "I did not say" has to keep meaning "fully there".
    fn default() -> Self {
        Self {
            grounds: Vec::new(),
            quads: Vec::new(),
            labels: Vec::new(),
            icons: Vec::new(),
            opacity: 1.0,
            body: None,
        }
    }
}

impl OverlayLayer {
    /// Whether the layer draws nothing at all — including a layer faded out of
    /// existence, which costs a text renderer and a pass through three channels
    /// to draw exactly nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        (self.grounds.is_empty()
            && self.quads.is_empty()
            && self.labels.is_empty()
            && self.icons.is_empty())
            || self.opacity <= 0.0
    }

    /// This layer's fills with its own [`opacity`](Self::opacity) folded into
    /// their alpha.
    #[must_use]
    pub fn faded_quads(&self) -> Vec<OverlayQuad> {
        self.quads
            .iter()
            .map(|quad| OverlayQuad {
                alpha: quad.alpha * self.opacity,
                ..*quad
            })
            .collect()
    }

    /// This layer's marks with its own [`opacity`](Self::opacity) folded into
    /// theirs. Both are uniform multipliers on a raster's alpha, so the fold is
    /// the product and the mark's texture identity is untouched.
    #[must_use]
    pub fn faded_icons(&self) -> Vec<ChromeIcon> {
        self.icons
            .iter()
            .map(|icon| ChromeIcon {
                opacity: icon.opacity * self.opacity,
                ..icon.clone()
            })
            .collect()
    }
}

/// The fills a rounded rectangle is made of: whole runs where it covers a pixel
/// completely, single pixels where it covers part of one, each carrying `alpha`
/// scaled by that pixel's exact coverage.
///
/// This is the floating-window craft the hover-peek flyout is built from, exposed
/// so a caller composing an overlay can build the same corners rather than a
/// second, staircased set. A bordered box is two of these — the whole box in the
/// border's colour, the face laid one border in with one border less radius — and
/// that is exactly how a browser leaves a `border: 1px solid` border-box.
#[must_use]
pub fn rounded_overlay_fill(
    rect: [f32; 4],
    radius_px: f32,
    color: [u8; 3],
    alpha: f32,
) -> Vec<OverlayQuad> {
    rounded_rect_coverage(rect, radius_px)
        .into_iter()
        .map(|entry| OverlayQuad {
            rect: entry.rect,
            color,
            alpha: entry.coverage * alpha,
        })
        .collect()
}

/// The ring between a rounded rectangle and the same rectangle grown by
/// `extent_px` on every side — a floating surface's lift, with the hole a
/// browser's own `box-shadow` leaves under the box it lifts.
#[must_use]
pub fn rounded_overlay_halo(
    rect: [f32; 4],
    radius_px: f32,
    extent_px: f32,
    color: [u8; 3],
    alpha: f32,
) -> Vec<OverlayQuad> {
    rounded_rect_halo_coverage(rect, radius_px, extent_px)
        .into_iter()
        .map(|entry| OverlayQuad {
            rect: entry.rect,
            color,
            alpha: entry.coverage * alpha,
        })
        .collect()
}

/// How many bands [`rounded_overlay_shadow`] cuts its falloff into, at most —
/// see [`rounded_rect::SHADOW_RINGS`].
pub const OVERLAY_SHADOW_RINGS: usize = rounded_rect::SHADOW_RINGS;

/// **A floating surface's lift**: the same ring as [`rounded_overlay_halo`], cut
/// into concentric bands a pixel or two wide whose strength falls off with
/// distance — one soft shadow rather than a set of rings around a box.
///
/// `alpha` is the strength immediately against the box; the curve takes it to
/// zero at `extent_px`. See [`rounded_rect::rounded_rect_shadow_coverage`] for
/// the falloff and for the report that asked for it.
///
/// [`rounded_overlay_halo`] is deliberately left as the exact, uniform ring it
/// is: its other caller strokes an outline with it, and an outline that faded
/// towards its outer edge would be a bug rather than a softness.
#[must_use]
pub fn rounded_overlay_shadow(
    rect: [f32; 4],
    radius_px: f32,
    extent_px: f32,
    color: [u8; 3],
    alpha: f32,
) -> Vec<OverlayQuad> {
    rounded_rect_shadow_coverage(rect, radius_px, extent_px)
        .into_iter()
        .map(|entry| OverlayQuad {
            rect: entry.rect,
            color,
            alpha: entry.coverage * alpha,
        })
        .collect()
}

/// What two rings at the palette's inner and outer alphas composited to
/// immediately against the box — the one number the soft falloff is anchored on.
///
/// The palette carries a *pair* per floating surface because that is the shape
/// the lift used to be drawn in. The pair is kept, and this is the sentence that
/// reads it: whatever the old two rings put right up against the box, the new
/// curve starts at, so no surface's shadow got lighter where it is darkest. Every
/// other distance is the curve's own answer.
#[must_use]
pub fn overlay_shadow_alpha(inner: f32, outer: f32) -> f32 {
    inner + outer - inner * outer
}

/// Identity is `key` plus placement. The bytes are a function of the key (that is
/// what the key *is*), so comparing them would be paying megabytes per frame to
/// re-learn something the string already said.
impl PartialEq for ChromeIcon {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.rect == other.rect
            && self.width_px == other.width_px
            && self.height_px == other.height_px
            // The crop is placement, exactly as `rect` is: the same raster seen
            // through a box that moved is a different set of pixels on screen.
            && self.clip == other.clip
            // Opacity is *not* covered by the key — deliberately, so that a
            // breathing mark reuses one raster — which means it is the one
            // property of an icon that this comparison has to read for itself.
            // Left out, every frame of a breath compares equal to the last and
            // the chrome is never rebuilt.
            && self.opacity == other.opacity
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RendererInitTimings {
    pub adapter: Duration,
    pub device: Duration,
    pub surface_configure: Duration,
    pub font_system: Duration,
    pub font_metrics: Duration,
    pub render_resources: Duration,
}

/// CPU-side phase timings from the headless replay probe. The probe executes the production text
/// shaping, glyph rasterization/atlas upload, and command encoding paths against a real wgpu
/// device, but has no window-system swapchain to present.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderProbeSample {
    pub total: Duration,
    pub row_compose: Duration,
    pub shape_cache_miss: Duration,
    pub atlas_prepare_upload: Duration,
    pub encode_submit: Duration,
    pub math_prepare_upload: Duration,
    pub rows_reshaped: u64,
    pub row_cache_hits: u64,
    pub row_cache_misses: u64,
    pub row_cache_evictions: u64,
    pub row_cache_resident_bytes: usize,
    pub narrow_hits: u64,
    pub narrow_misses: u64,
    pub narrow_evictions: u64,
    pub wide_hits: u64,
    pub wide_misses: u64,
    pub wide_evictions: u64,
    pub narrow_resident_bytes: usize,
    pub wide_resident_bytes: usize,
    pub math_texture_uploads: u64,
    pub math_texture_upload_bytes: usize,
    pub math_texture_evictions: u64,
    pub math_texture_resident_bytes: usize,
    /// glyphon 0.12 exposes no atlas occupancy or mutation counters. These remain `None` rather
    /// than converting requested glyphs or elapsed time into invented hit/upload estimates.
    pub atlas_hits: Option<u64>,
    pub atlas_misses: Option<u64>,
    pub atlas_grows: Option<u64>,
    pub atlas_evictions: Option<u64>,
    pub atlas_upload_bytes: Option<u64>,
    pub narrow_glyphs: u64,
    pub wide_glyphs: u64,
}

/// Headless render-path instrumentation used by `bt-replay`.
///
/// Two layers and nothing else: the process's [`GpuContext`], and one
/// [`WindowRenderer`] whose target is a texture instead of a swapchain. The
/// multiwindow spike called this shape the template for the split — "the device
/// layer without a surface" — and folding it back onto the real types is what
/// keeps the claim honest: the probe measures the production shaping, atlas and
/// encoding paths because it *is* a window renderer, not because a second copy
/// of one was kept in step by hand.
#[doc(hidden)]
pub struct HeadlessRenderProbe {
    gpu: GpuContext,
    window: WindowRenderer,
    adapter_name: String,
}

impl HeadlessRenderProbe {
    pub async fn new(width: u32, height: u32, scale_factor: f64) -> Result<Self, RenderError> {
        // Named outright rather than asked of a swapchain, which is the whole of
        // what "headless" means here. It is the format the window path picks on
        // every machine this ships to, so the atlas and pipelines the probe
        // measures are the ones a window would have built.
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let mut gpu = GpuContext::headless(format).await?;
        let adapter_name = gpu.adapter_name();
        let window = WindowRenderer::offscreen(&mut gpu, width, height, scale_factor, format)?;
        Ok(Self {
            gpu,
            window,
            adapter_name,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.gpu.max_texture_dimension_2d()
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.window.update_scale_factor(&mut self.gpu, scale_factor)
    }

    pub fn prepare_frame(
        &mut self,
        frame: &ViewportFrame,
    ) -> Result<RenderProbeSample, RenderError> {
        self.window.probe_frame(&mut self.gpu, frame)
    }
}

impl GpuContext {
    /// Open the process's device layer and its first window in one call.
    ///
    /// The order is the one the hardware forces and not a preference: an
    /// `Instance` makes the `Surface`, the `Surface` chooses the `Adapter`, the
    /// `Adapter` names the format, and only then can an atlas and two pipelines
    /// be baked for it. **Every later window skips the first three steps
    /// entirely** — [`WindowRenderer::new`] against the context this returned —
    /// which is the whole of what makes a second window a second surface rather
    /// than a second renderer.
    ///
    /// The pair comes back unmarried: the device layer belongs to the process
    /// and the window layer to one window, and a type that held both would be
    /// the first window quietly owning every later one's device.
    pub async fn open(
        target: WindowTarget,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<(Self, WindowRenderer), RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let kind = target.kind();
        let surface = create_surface(&instance, target)?;
        let mut gpu = Self::bootstrap_for_surface(instance, &surface).await?;
        let window =
            WindowRenderer::from_surface(&mut gpu, surface, kind, width, height, scale_factor)?;
        Ok((gpu, window))
    }

    /// Build the device layer against the surface that bootstraps it.
    ///
    /// The surface comes in by reference and stays the caller's, because two
    /// separate things are being read off it and neither of them makes this
    /// context the surface's owner. An adapter is chosen *for* a surface —
    /// `compatible_surface` is not decoration on a hybrid-GPU laptop — and the
    /// swapchain format the atlas and both pipelines are baked for is that
    /// surface's. Every later window is measured against the answer this one
    /// gave: see [`Self::accept_format`].
    pub async fn bootstrap_for_surface(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'static>,
    ) -> Result<Self, RenderError> {
        let phase_started = Instant::now();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let adapter_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Folio device"),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let device_time = phase_started.elapsed();
        let format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or_else(|| RenderError::Wgpu("surface has no sRGB format".to_owned()))?;
        Self::assemble(
            instance,
            adapter,
            device,
            queue,
            format,
            adapter_time,
            device_time,
        )
    }

    /// The device layer with no window at all: `compatible_surface: None` and
    /// the format named outright rather than asked of a swapchain.
    ///
    /// The replay probe's shape, and the shape every headless test uses. It is
    /// also the honest statement of what this layer is — nothing here needs a
    /// surface except the choice of adapter, and a caller who has no surface has
    /// no choice to make.
    pub async fn headless(format: wgpu::TextureFormat) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let phase_started = Instant::now();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: None,
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let adapter_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Folio replay probe device"),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let device_time = phase_started.elapsed();
        Self::assemble(
            instance,
            adapter,
            device,
            queue,
            format,
            adapter_time,
            device_time,
        )
    }

    fn assemble(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        adapter_time: Duration,
        device_time: Duration,
    ) -> Result<Self, RenderError> {
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;
        let phase_started = Instant::now();
        let mut font_system = terminal_font_system();
        let font_system_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let mut swash_cache = SwashCache::new();
        let chrome_cap_height_ratio = chrome_cap_height_ratio(&mut font_system, &mut swash_cache)
            .ok_or(RenderError::MissingChromeSansMetrics)?;
        let font_metrics_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let glyphon_cache = Cache::new(&device);
        let atlas = TextAtlas::new(&device, &queue, &glyphon_cache, format);
        let rect_pipeline = create_rect_pipeline(&device, format);
        let ground_rect_pipeline = create_ground_rect_pipeline(&device, format);
        let ground_fade_rect_pipeline = create_ground_fade_rect_pipeline(&device, format);
        let (math_pipeline, math_bind_group_layout, math_sampler) =
            create_math_pipeline(&device, format);
        let (background_pipeline, background_bind_group_layout, background_sampler) =
            create_background_pipeline(&device, format);
        let render_resources_time = phase_started.elapsed();
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            format,
            max_texture_dimension_2d,
            font_system,
            swash_cache,
            glyphon_cache,
            atlas,
            rect_pipeline,
            ground_rect_pipeline,
            ground_fade_rect_pipeline,
            math_pipeline,
            math_bind_group_layout,
            math_sampler,
            background_pipeline,
            background_bind_group_layout,
            background_sampler,
            background_texture: None,
            math_textures: ByteLru::new(MATH_TEXTURE_CACHE_BUDGET_BYTES),
            math_texture_evictions: 0,
            chrome_cap_height_ratio,
            terminal_font_size_logical_px: DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX,
            init_timings: RendererInitTimings {
                adapter: adapter_time,
                device: device_time,
                // A window's cost, filled in by `WindowRenderer::init_timings`.
                surface_configure: Duration::ZERO,
                font_system: font_system_time,
                font_metrics: font_metrics_time,
                render_resources: render_resources_time,
            },
        })
    }

    /// Point the **grid's** face at a family and a size — never the chrome's.
    ///
    /// The two halves of the Appearance block's font rows arrive together
    /// because they cost the same thing: every measurement, every shaped run
    /// and every composed row is derived from the pair, so changing one is
    /// exactly as invalidating as changing both, and a caller that could change
    /// them separately would pay twice for one visible change.
    ///
    /// **The files come in rather than being looked up here.** This renderer
    /// builds its font database from a fixed file list on purpose (see
    /// [`terminal_font_system`]) — enumerating `Fonts/` is the startup cost that
    /// design exists to avoid, and this crate has no business opening a system
    /// font collection. `bt-platform` enumerates once, when the user opens the
    /// picker, and hands back name and paths together; an empty `files` is the
    /// ordinary case for a family the startup list already loaded.
    ///
    /// Loading is idempotent and cheap on repeat: `fontdb` refuses a face it
    /// already holds, so re-choosing a family the user has picked before does
    /// not grow the database.
    ///
    /// The caller must follow this with [`WindowRenderer::apply_font_change`]
    /// for every window on the device. This function moves the database and the
    /// size; it cannot reach the per-window caches that are now stale, and a
    /// window that is never told will go on drawing rows composed in the old
    /// face until something else invalidates them.
    pub fn set_terminal_font(
        &mut self,
        family: &str,
        files: &[std::path::PathBuf],
        size_logical_px: f32,
    ) {
        for file in files {
            let _ = self.font_system.db_mut().load_font_file(file);
        }
        // Asked of the database rather than assumed from `family`: a family the
        // machine no longer has (uninstalled since it was written to
        // `settings.json`) must leave the grid on the face it can actually
        // draw, not on a name that resolves to nothing.
        let family = if family.is_empty() || !font_family_available(&self.font_system, family) {
            DEFAULT_PRIMARY_FONT_FAMILY
        } else {
            family
        };
        self.font_system.db_mut().set_monospace_family(family);
        self.terminal_font_size_logical_px = size_logical_px;
    }

    /// The grid's face size in logical pixels, as last set.
    #[must_use]
    pub fn terminal_font_size_logical_px(&self) -> f32 {
        self.terminal_font_size_logical_px
    }

    /// The family the grid's face currently resolves to.
    #[must_use]
    pub fn terminal_font_family(&self) -> &str {
        primary_font_family(&self.font_system)
    }

    /// The swapchain format this context's atlas and pipelines were baked for.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    #[must_use]
    pub fn adapter_name(&self) -> String {
        self.adapter.get_info().name
    }

    #[must_use]
    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.max_texture_dimension_2d
    }

    /// Make the device hold exactly the ground's picture, and answer whether a
    /// quad can be drawn from it.
    ///
    /// **`None` releases the slot** (§7.1.6c-4d, clearing arm). A ground with no
    /// picture is a ground with no texture: the reader chose `None`, and a
    /// wallpaper kept on the device after that is a whole screen of texels — up
    /// to `MAX_BACKGROUND_IMAGE_RGBA_BYTES` worth — held for a window that has
    /// no way left to reach it. The slot mirrors the ground in force, so this is
    /// the one call the frame path makes whether or not there is a picture,
    /// rather than a call it skips when there is not.
    ///
    /// Content-keyed and idempotent: the overwhelmingly common frame asks for
    /// the key already in the slot and does nothing at all. A different key
    /// replaces the slot outright — there is one ground, so keeping the old
    /// texture around would be keeping a wallpaper nobody can reach.
    ///
    /// `false` when the picture will not fit this device: a single texture and
    /// not the tiled path `upload_rgba_tiles` uses for formulas, because a
    /// tiled ground would need per-tile UV rectangles under three fits and a
    /// `Repeat` sampler that repeats the *tile* rather than the picture. The
    /// caller's answer to `false` is to draw the plain clear, which is the same
    /// answer it gives to "no picture chosen".
    fn hold_background_texture(&mut self, image: Option<&ground::BackgroundImage>) -> bool {
        let Some(image) = image else {
            self.background_texture = None;
            return false;
        };
        if self
            .background_texture
            .as_ref()
            .is_some_and(|(key, _)| key == &image.key)
        {
            return true;
        }
        let expected = image.width_px as usize * image.height_px as usize * 4;
        if image.width_px == 0
            || image.height_px == 0
            || image.width_px > self.max_texture_dimension_2d
            || image.height_px > self.max_texture_dimension_2d
            || image.rgba.len() != expected
        {
            self.background_texture = None;
            return false;
        }
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("window ground texture"),
                size: wgpu::Extent3d {
                    width: image.width_px,
                    height: image.height_px,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &image.rgba,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("window ground bind group"),
            layout: &self.background_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.background_sampler),
                },
            ],
        });
        self.background_texture = Some((image.key.clone(), bind_group));
        true
    }

    /// The sharing gate. `Ok` only when `format` is the one the atlas and both
    /// pipelines were built for.
    ///
    /// Every window constructor goes through here, and there is no path around
    /// it: an unequal format is [`RenderError::FormatMismatch`] and never a
    /// silently-minted second atlas.
    fn accept_format(&self, format: wgpu::TextureFormat) -> Result<(), RenderError> {
        if format == self.format {
            Ok(())
        } else {
            Err(RenderError::FormatMismatch {
                context: self.format,
                surface: format,
            })
        }
    }

    fn upload_math_texture(
        &self,
        artifact: &bt_viewport::ProjectedMathArtifact,
    ) -> Option<CachedMathTexture> {
        self.upload_rgba_tiles(&artifact.rgba, artifact.width_px, artifact.height_px)
    }

    /// Cut an RGBA image into device-sized tiles and upload each as its own
    /// bound texture. On the device layer because the bytes, the sampler and the
    /// bind-group layout are all the device's; nothing here knows which window
    /// asked.
    fn upload_rgba_tiles(
        &self,
        rgba: &[u8],
        width_px: u32,
        height_px: u32,
    ) -> Option<CachedMathTexture> {
        let expected = width_px as usize * height_px as usize * 4;
        if width_px == 0 || height_px == 0 || rgba.len() != expected {
            return None;
        }
        let tile_limit = self.max_texture_dimension_2d.max(1);
        let mut tiles = Vec::new();
        for y in (0..height_px).step_by(tile_limit as usize) {
            let height = (height_px - y).min(tile_limit);
            for x in (0..width_px).step_by(tile_limit as usize) {
                let width = (width_px - x).min(tile_limit);
                let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
                for row in y..y + height {
                    let start = (row as usize * width_px as usize + x as usize) * 4;
                    let end = start + width as usize * 4;
                    bytes.extend_from_slice(&rgba[start..end]);
                }
                let texture = self.device.create_texture_with_data(
                    &self.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("math block texture tile"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &bytes,
                );
                let view = texture.create_view(&Default::default());
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("math block texture bind group"),
                    layout: &self.math_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.math_sampler),
                        },
                    ],
                });
                tiles.push(MathTextureTile {
                    bind_group,
                    x_px: x,
                    y_px: y,
                    width_px: width,
                    height_px: height,
                });
            }
        }
        Some(CachedMathTexture { tiles })
    }
}

impl WindowRenderer {
    /// A window over an existing device layer: the second, third and every
    /// later `Folio` window.
    ///
    /// The surface is created from `gpu`'s own [`wgpu::Instance`], which is not
    /// a convenience — a surface created by any other instance cannot be
    /// presented on this device.
    pub fn new(
        gpu: &mut GpuContext,
        target: WindowTarget,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self, RenderError> {
        let kind = target.kind();
        let surface = create_surface(&gpu.instance, target)?;
        Self::from_surface(gpu, surface, kind, width, height, scale_factor)
    }

    /// The same window, when the caller already holds the surface — which the
    /// first window does, because its surface is what chose the adapter.
    fn from_surface(
        gpu: &mut GpuContext,
        surface: wgpu::Surface<'static>,
        target: WindowTargetKind,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self, RenderError> {
        let phase_started = Instant::now();
        let swapchain_size = surface_config_size(width, height, gpu.max_texture_dimension_2d);
        let mut config = surface
            .get_default_config(&gpu.adapter, swapchain_size.0, swapchain_size.1)
            .ok_or_else(|| RenderError::Wgpu("surface has no default configuration".to_owned()))?;
        let capabilities = surface.get_capabilities(&gpu.adapter);
        config.format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or_else(|| RenderError::Wgpu("surface has no sRGB format".to_owned()))?;
        gpu.accept_format(config.format)?;
        // The gate, before the surface is configured rather than after: a
        // visual target that cannot be `PreMultiplied` is not this program's
        // window, and configuring it anyway would leave a swapchain on screen
        // that looks right and is not.
        config.alpha_mode = choose_alpha_mode(target, &capabilities.alpha_modes)?;
        let alpha_report = SurfaceAlphaReport {
            target,
            offered: capabilities.alpha_modes.clone(),
            chosen: config.alpha_mode,
        };
        config.desired_maximum_frame_latency = 1;
        surface.configure(&gpu.device, &config);
        let surface_configure_time = phase_started.elapsed();
        let mut window = Self::assemble(
            gpu,
            FrameTarget::Surface(Box::new(surface)),
            config,
            swapchain_size,
            scale_factor,
            surface_configure_time,
        )?;
        window.alpha_report = Some(alpha_report);
        Ok(window)
    }

    /// A window renderer with a texture where its swapchain would be.
    ///
    /// Everything else about it is a window: its own metrics for its own scale
    /// factor, its own composed-row and shaping caches, its own seat slots and
    /// chrome viewport, and the shared atlas underneath. That is what makes the
    /// replay probe a real instance of this type rather than a second
    /// implementation of it, and what lets a test hold two of them over one
    /// [`GpuContext`] without a window manager.
    pub fn offscreen(
        gpu: &mut GpuContext,
        width: u32,
        height: u32,
        scale_factor: f64,
        format: wgpu::TextureFormat,
    ) -> Result<Self, RenderError> {
        gpu.accept_format(format)?;
        let size = surface_config_size(width.max(1), height.max(1), gpu.max_texture_dimension_2d);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Folio offscreen window target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0,
            height: size.1,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: Vec::new(),
        };
        Self::assemble(
            gpu,
            FrameTarget::Offscreen(texture),
            config,
            size,
            scale_factor,
            Duration::ZERO,
        )
    }

    /// The per-window state both constructors end in: the glyphon objects that
    /// hold one prepared batch each, the metrics this window's scale factor
    /// produces, and the caches keyed by the pixel font size those metrics name.
    fn assemble(
        gpu: &mut GpuContext,
        target: FrameTarget,
        config: wgpu::SurfaceConfiguration,
        configured_size: (u32, u32),
        scale_factor: f64,
        surface_configure_time: Duration,
    ) -> Result<Self, RenderError> {
        let phase_started = Instant::now();
        let metrics = CellMetrics::measure(&mut gpu.font_system, scale_factor)?;
        let font_metrics_time = phase_started.elapsed();
        let device = &gpu.device;
        let cache = &gpu.glyphon_cache;
        let chrome_viewport = Viewport::new(device, cache);
        // Slot 0: the seat a lone terminal leaf draws into on every frame it
        // ever draws. Built here rather than on demand so that shape never pays
        // an allocation mid-frame.
        let seat_slots = vec![SeatTextSlot {
            viewport: Viewport::new(device, cache),
            text_renderer: TextRenderer::new(
                &mut gpu.atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            ),
            status_text_renderer: TextRenderer::new(
                &mut gpu.atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            ),
        }];
        let chrome_text_renderer = TextRenderer::new(
            &mut gpu.atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
        // Its own renderer rather than a share of the chrome's, for the reason
        // every seat has its own: one `TextRenderer` holds one prepared batch,
        // and a preview body is prepared from a different shaping run (a
        // monospace face, one buffer per line) than the chrome around it.
        let preview_text_renderer = TextRenderer::new(
            &mut gpu.atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
        let trace_perf = std::env::var_os("BT_PERF_TRACE").is_some();
        // A window that draws into a texture is an instrument by construction —
        // `HeadlessRenderProbe` is the only thing that builds one — so its
        // shaping caches count hits, misses and evictions whether or not the
        // trace is on, which is what `bt-replay` reports. A window on a
        // swapchain pays for that counting only when someone asked for it.
        let measure_shaping = trace_perf || matches!(target, FrameTarget::Offscreen(_));
        Ok(Self {
            target,
            config,
            // Filled in by `from_surface`, which is the only constructor with a
            // compositor to answer to.
            alpha_report: None,
            configured_size,
            seat: SeatViewport::whole(configured_size.0, configured_size.1),
            chrome_quads: Vec::new(),
            chrome_labels: Vec::new(),
            chrome_icons: Vec::new(),
            overlay_layers: Vec::new(),
            seat_slots,
            chrome_viewport,
            chrome_text_renderer,
            overlay_text_renderers: Vec::new(),
            math_texture_refusals: 0,
            textureless_math_blocks: 0,
            max_texture_dimension_2d: gpu.max_texture_dimension_2d,
            metrics,
            surface_configure_time,
            font_metrics_time,
            text_rows: Vec::new(),
            status_overlay: None,
            composed_row_cache: ComposedRowCache::new(),
            font_revision: 1,
            narrow_shaping_cache: NarrowShapingCache::with_perf_tracking(measure_shaping),
            wide_shaping_cache: WideShapingCache::with_perf_tracking(measure_shaping),
            glyph_degraded_frames: 0,
            window_focused: true,
            cursor_blink_visible: true,
            peek_overlay: None,
            preview_image: None,
            preview_bodies: Vec::new(),
            preview_text_frame: PreviewTextFrame::default(),
            table_blocks: HashMap::new(),
            preview_text_renderer,
            trace_perf,
            perf_frame: 0,
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn ime_cursor_area(&self, frame: &ViewportFrame) -> ImeCursorArea {
        ime_cursor_area_for_metrics(self.metrics, frame)
    }

    pub fn math_hit_test(&self, frame: &ViewportFrame, x: f64, y: f64) -> Option<MathHit> {
        let point = [x as f32, y as f32];
        if let Some(failure) = frame.math_failures.iter().rev().find(|failure| {
            self.math_failure_geometry(frame, failure)
                .is_some_and(|(_, hit)| point_in_rect(point, hit))
        }) {
            return Some(MathHit {
                anchor: failure.anchor.clone(),
                target: MathHitTarget::Failure,
            });
        }
        frame.math_blocks.iter().rev().find_map(|placement| {
            if placement.artifact.kind != bt_viewport::RgbaArtifactKind::Math {
                return None;
            }
            let geometry = self.math_block_geometry(frame, placement)?;
            let target = if geometry.eye.is_some_and(|rect| point_in_rect(point, rect)) {
                MathHitTarget::ToggleSource
            } else if geometry.copy.is_some_and(|rect| point_in_rect(point, rect)) {
                MathHitTarget::CopyLatex
            } else if point_in_rect(point, geometry.block) {
                MathHitTarget::Block
            } else {
                return None;
            };
            Some(MathHit {
                anchor: placement.anchor.with_run(inline_run_at(
                    placement,
                    geometry.block,
                    point[0],
                )),
                target,
            })
        })
    }

    /// What the adapter offered this window's surface and what it was
    /// configured with. `None` for an offscreen target.
    #[must_use]
    pub fn alpha_report(&self) -> Option<&SurfaceAlphaReport> {
        self.alpha_report.as_ref()
    }

    pub fn presentation_geometry(&self) -> PresentationGeometry {
        PresentationGeometry {
            swapchain_size: (self.config.width, self.config.height),
            max_texture_dimension_2d: self.max_texture_dimension_2d,
        }
    }

    /// Startup phase timings, reassembled from the two layers.
    ///
    /// `adapter`, `device`, `font_system` and `render_resources` are charged
    /// once per process; `surface_configure` is this window's; `font_metrics`
    /// is both — the face's cap height is measured once and the cell is
    /// measured per window, because the cell is a function of that window's
    /// scale factor.
    #[must_use]
    pub fn init_timings(&self, gpu: &GpuContext) -> RendererInitTimings {
        RendererInitTimings {
            surface_configure: self.surface_configure_time,
            font_metrics: gpu.init_timings.font_metrics + self.font_metrics_time,
            ..gpu.init_timings
        }
    }

    /// Select the cursor presentation without changing terminal DEC cursor visibility.
    pub fn set_window_focused(&mut self, focused: bool) -> bool {
        let changed = self.window_focused != focused;
        self.window_focused = focused;
        changed
    }

    /// Select the focused cursor's blink phase without affecting the unfocused outline.
    pub fn set_cursor_blink_visible(&mut self, visible: bool) -> bool {
        let changed = self.cursor_blink_visible != visible;
        self.cursor_blink_visible = visible;
        changed
    }

    /// The display box `seat` would show a native decode of `image_width_px` x `image_height_px`
    /// in, or `None` when that pane cannot host the flyout at all. The app asks this before a peek
    /// so it resamples once, off-thread, to exactly the pixels the flyout draws.
    ///
    /// `seat` is named by the caller rather than read off `self.seat` for the reason
    /// [`PeekImageOverlay::seat`] exists: the pane that owns the hovered content is the pane under
    /// the pointer, and asking the focused pane would size the thumbnail against a rectangle the
    /// hover has nothing to do with — which, with panes of different sizes, is a different picture.
    pub fn peek_thumbnail_extent(
        &self,
        seat: SeatViewport,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Option<(u32, u32)> {
        peek_thumbnail_extent(
            seat.width as f32,
            seat.height as f32,
            self.metrics.padding_px,
            self.metrics.scale_factor as f32,
            image_width_px,
            image_height_px,
        )
    }

    /// Replace the hover-peek flyout. Returns whether the visible overlay changed so the caller
    /// can skip redundant redraw requests. Peek state is renderer-side only; frames stay pure.
    pub fn set_peek_overlay(&mut self, overlay: Option<PeekImageOverlay>) -> bool {
        let changed = match (&self.peek_overlay, &overlay) {
            (None, None) => false,
            (Some(current), Some(next)) => {
                current.key != next.key
                    || current.pointer_x != next.pointer_x
                    || current.pointer_y != next.pointer_y
            }
            _ => true,
        };
        self.peek_overlay = overlay;
        changed
    }

    /// Replace the persistent preview-seat raster. Like peek, this is presentation state beside a
    /// terminal frame; unlike peek it owns a solver-provided neighbouring seat viewport.
    pub fn set_preview_image(&mut self, image: Option<PreviewImage>) -> bool {
        let changed = match (&self.preview_image, &image) {
            (None, None) => false,
            (Some(current), Some(next)) => {
                current.seat != next.seat
                    || current.clip != next.clip
                    || current.key != next.key
                    || current.width_px != next.width_px
                    || current.height_px != next.height_px
                    || current.display_width_px != next.display_width_px
                    || current.display_height_px != next.display_height_px
            }
            _ => true,
        };
        self.preview_image = image;
        changed
    }

    /// Replace **every** preview body this frame. Returns whether anything
    /// changed.
    ///
    /// The whole body, every frame it differs, rather than a placement door
    /// beside it: a document's rectangles are recomputed from the seat and the
    /// scroll offset by the caller anyway, so there is no cheaper half to move.
    ///
    /// # A list, since slice 5
    ///
    /// This was one `Option`, because the window had one preview seat. A tab can
    /// now hold a pinned preview pane, the un-pinned one beside it and a torn-off
    /// float at the same time, and each of those is a document scrolled to its own
    /// place. Every [`PreviewBody`] already carries its own `clip` in
    /// whole-surface coordinates, so nothing here has to know which surface a body
    /// came from — the list is drawn in the order it is given, which is the order
    /// the caller paints its surfaces in.
    /// What the last presented frame did with the preview documents — the one
    /// reader is `BT_PREVIEW_TRACE` in `bt-app`.
    #[must_use]
    pub fn preview_text_frame(&self) -> PreviewTextFrame {
        self.preview_text_frame
    }

    pub fn set_preview_bodies(&mut self, bodies: Vec<PreviewBody>) -> bool {
        let changed = self.preview_bodies != bodies;
        self.preview_bodies = bodies;
        changed
    }

    /// Hand over this frame's table pictures, keyed by the block's own source text.
    ///
    /// A map rather than a slot on the placement because the placement travels through
    /// `bt-viewport`, which does not know this crate's drawing types and must not learn them: a
    /// projection says *where* a block is and how much of it may be seen, and a picture is not
    /// part of that answer.
    ///
    /// **The source and not the artifact key**, because the source is the identity both sides of
    /// this hand-off can see: the key is minted inside `bt-term` from versions the caller of this
    /// method never holds, while the source is on the placement the renderer is already reading.
    /// Two panes showing the same table are then showing one picture, which is also correct — and
    /// the caller owns staleness, since it hands the whole map over every time anything moves.
    pub fn set_table_blocks(&mut self, blocks: HashMap<String, TableBlockPaint>) -> bool {
        let changed = self.table_blocks != blocks;
        self.table_blocks = blocks;
        changed
    }

    /// How tall a paragraph will be when wrapped into `width_px`.
    ///
    /// The caller lays a document out top to bottom and cannot know where the
    /// second block starts without knowing how many lines the first took — and
    /// how many lines a paragraph takes is a question only the shaper can
    /// answer. Its twin is [`Self::measure_chrome_text`], and it exists for the
    /// same reason that one does.
    pub fn measure_preview_paragraph(
        &mut self,
        gpu: &mut GpuContext,
        runs: &[PreviewRun],
        width_px: f32,
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        if runs.iter().all(|run| run.text.is_empty()) {
            return line_height_px;
        }
        let mut buffer = Buffer::new(
            &mut gpu.font_system,
            Metrics::new(font_size_px, line_height_px),
        );
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_size(Some(width_px.max(1.0)), None);
        set_preview_runs(
            &mut buffer,
            runs,
            0.0,
            Metrics::new(font_size_px, line_height_px),
        );
        buffer.shape_until_scroll(&mut gpu.font_system, false);
        buffer.layout_runs().count().max(1) as f32 * line_height_px
    }

    /// How wide a paragraph is when it is **not** wrapped.
    ///
    /// [`Self::measure_preview_paragraph`]'s other axis, and it exists for the
    /// blocks that refuse to reflow: a markdown table's columns are as wide as
    /// their own widest cell and a code fence is as wide as its longest line
    /// (user rulings, 2026-08-13), so the horizontal scroll extent of a rendered
    /// document is a question only the shaper can answer. Approximating it from
    /// a character count is what a monospace grid may do and a proportional face
    /// may not — the error compounds across a column of prose and ends as a
    /// scroll that stops before the last word.
    pub fn measure_preview_paragraph_width(
        &mut self,
        gpu: &mut GpuContext,
        runs: &[PreviewRun],
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        if runs.iter().all(|run| run.text.is_empty()) {
            return 0.0;
        }
        let mut buffer = Buffer::new(
            &mut gpu.font_system,
            Metrics::new(font_size_px, line_height_px),
        );
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, Some(line_height_px));
        set_preview_runs(
            &mut buffer,
            runs,
            0.0,
            Metrics::new(font_size_px, line_height_px),
        );
        buffer.shape_until_scroll(&mut gpu.font_system, false);
        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max)
    }

    /// Where each of a paragraph's runs landed once the shaper had it.
    ///
    /// The third of [`Self::measure_preview_paragraph`]'s family, and the one
    /// that answers a *pointer* rather than a layout: a markdown link is a run
    /// inside a paragraph of proportional text that has already wrapped, so
    /// where it is on screen is not something the caller can add up from
    /// character counts — only the shaper knows, and it is the same shaper that
    /// drew it. Asked lazily, one paragraph at a time, so a document of a
    /// thousand paragraphs costs nothing until a pointer is over one of them.
    ///
    /// The buffer is built exactly as [`shape_preview_body`] builds it and the
    /// pen origin comes from the same [`preview_paragraph_left`], because a box
    /// measured any other way is a box in a different place than the glyphs.
    pub fn measure_preview_run_boxes(
        &mut self,
        gpu: &mut GpuContext,
        paragraph: &PreviewParagraph,
    ) -> Vec<PreviewRunBox> {
        let mut buffer = Buffer::new(
            &mut gpu.font_system,
            Metrics::new(paragraph.font_size_px, paragraph.line_height_px),
        );
        if paragraph.wrap {
            buffer.set_wrap(Wrap::WordOrGlyph);
            buffer.set_size(Some((paragraph.rect[2] - paragraph.rect[0]).max(1.0)), None);
        } else {
            buffer.set_wrap(Wrap::None);
            buffer.set_size(None, Some(paragraph.line_height_px));
        }
        set_preview_runs(
            &mut buffer,
            &paragraph.runs,
            paragraph.letter_spacing_em,
            Metrics::new(paragraph.font_size_px, paragraph.line_height_px),
        );
        buffer.shape_until_scroll(&mut gpu.font_system, false);
        let left = preview_paragraph_left(paragraph, &buffer);
        let top = paragraph.rect[1];
        // The runs are concatenated into one line before shaping, so a glyph
        // says which run it came from by where its cluster starts.
        let mut ends = Vec::with_capacity(paragraph.runs.len());
        let mut total = 0usize;
        for run in &paragraph.runs {
            total += run.text.len();
            ends.push(total);
        }
        let mut boxes: Vec<PreviewRunBox> = Vec::new();
        for line in buffer.layout_runs() {
            for glyph in line.glyphs {
                let Some(run) = ends.iter().position(|end| glyph.start < *end) else {
                    continue;
                };
                let rect = [
                    left + glyph.x,
                    top + line.line_top,
                    left + glyph.x + glyph.w,
                    top + line.line_top + paragraph.line_height_px,
                ];
                // Glyphs arrive in visual order, so one run broken across a
                // line — or across a bidi boundary — comes back as the several
                // boxes it is drawn as.
                match boxes.last_mut() {
                    Some(last) if last.run == run && (last.rect[1] - rect[1]).abs() < 0.5 => {
                        last.rect[0] = last.rect[0].min(rect[0]);
                        last.rect[2] = last.rect[2].max(rect[2]);
                    }
                    _ => boxes.push(PreviewRunBox { run, rect }),
                }
            }
        }
        boxes
    }

    /// How wide one cell of the monospace face is at `font_size_px`.
    ///
    /// Measured over a run of cells and divided rather than asked of one: a
    /// single advance carries the face's own rounding, and a document three
    /// hundred columns wide would end up three hundred roundings away from where
    /// its own scroller thinks it ends.
    pub fn preview_mono_advance(&mut self, gpu: &mut GpuContext, font_size_px: f32) -> f32 {
        const CELLS: usize = 32;
        let sample = "M".repeat(CELLS);
        let line_height = font_size_px * 1.4;
        let mut buffer = Buffer::new(
            &mut gpu.font_system,
            Metrics::new(font_size_px, line_height),
        );
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, Some(line_height));
        buffer.set_text(
            &sample,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut gpu.font_system, false);
        let width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);
        width / CELLS as f32
    }

    /// **U8 — move the preview seat's picture to where its pane is drawn this
    /// frame.** Returns whether the pair changed.
    ///
    /// A second door beside [`Self::set_preview_image`] because it answers a
    /// different question at a different rate. The *content* — the raster, its
    /// key, the extent it was fitted to — changes when the layout commits or the
    /// scale worker delivers, which is a handful of times per preview; the pair
    /// of rectangles changes on every frame of a 200ms flight. Routing the flight
    /// through `set_preview_image` would have the caller rebuild a whole
    /// `PreviewImage` sixty times a second, `Arc` clone and key string included,
    /// in order to move two integers.
    ///
    /// Nothing at all when there is no picture: a placement is a fact about an
    /// image, and there is no empty image to hold one.
    pub fn place_preview_image(&mut self, seat: SeatViewport, clip: SeatViewport) -> bool {
        let Some(image) = self.preview_image.as_mut() else {
            return false;
        };
        let changed = image.seat != seat || image.clip != clip;
        image.seat = seat;
        image.clip = clip;
        changed
    }

    pub fn resize(&mut self, gpu: &GpuContext, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let swapchain_size = surface_config_size(width, height, gpu.max_texture_dimension_2d);
        self.config.width = swapchain_size.0;
        self.config.height = swapchain_size.1;
        // A new surface makes the seat rectangle that was solved against the old
        // one provisional, and the caller re-solves before the next frame. Until
        // it does, the rectangle is *clamped* rather than replaced: the only
        // legitimate author of a seat rectangle is `solve` (red line L10), and
        // substituting the whole surface here would have this crate invent one —
        // a lone leaf's answer, handed to a tree that is not a lone leaf. That
        // hands every content extent below (`pane_right`, `pane_bottom`, the
        // peek box, the math band and its toolbar) the window instead of the
        // seat, and removes the pass scissor that keeps the terminal off its
        // neighbours. Clamping keeps the one safety property the substitution
        // was buying — a shrunk surface can never be drawn outside — without
        // discarding a split.
        self.seat = self.seat.clamped_to(swapchain_size.0, swapchain_size.1);
        Ok(())
    }

    /// The rectangle of the swapchain the terminal seat occupies.
    #[must_use]
    pub fn seat_viewport(&self) -> SeatViewport {
        self.seat
    }

    /// Place the terminal seat. Returns whether the rectangle changed.
    ///
    /// Red line L10 runs the other way and is worth restating here: nothing
    /// inside the terminal may call this. The rectangle arrives from `solve`,
    /// never from the content that will be drawn in it.
    pub fn set_seat_viewport(&mut self, seat: SeatViewport) -> bool {
        let seat = seat.clamped_to(self.config.width, self.config.height);
        let changed = self.seat != seat;
        self.seat = seat;
        changed
    }

    /// Replace the seat chrome drawn around the terminal. Returns whether the
    /// visible chrome changed, so the caller can skip a redundant redraw.
    pub fn set_chrome(
        &mut self,
        quads: Vec<ChromeQuad>,
        labels: Vec<ChromeLabel>,
        icons: Vec<ChromeIcon>,
    ) -> bool {
        let changed = self.chrome_quads != quads
            || self.chrome_labels != labels
            || self.chrome_icons != icons;
        self.chrome_quads = quads;
        self.chrome_labels = labels;
        self.chrome_icons = icons;
        changed
    }

    /// Replace the modal overlay: the scrim and whatever dialog stands on it,
    /// bottom layer first. Returns whether the visible overlay changed, so the
    /// caller can skip a redundant redraw. An empty stack means "no modal", which
    /// is the state every frame that has never opened one is already in.
    ///
    /// Each [`OverlayLayer`] is drawn whole — fills, then marks, then text —
    /// before the next one starts, so a layer covers every channel of the layers
    /// below it.
    ///
    /// This is presentation state beside the frame, exactly as the peek flyout is
    /// (DESIGN §7.1.5: a modal is a window-level stance, not a property of the
    /// terminal's content), so `ViewportFrame` equality and the replay contracts
    /// stay untouched by a visible dialog.
    pub fn set_modal_overlay(&mut self, layers: Vec<OverlayLayer>) -> bool {
        let changed = self.overlay_layers != layers;
        self.overlay_layers = layers;
        changed
    }

    /// How wide `text` will be when drawn as a plain [`ChromeLabel`] at
    /// `font_size_px`, in physical pixels.
    ///
    /// The one caller that cannot do without it is the tab's pane-count badge:
    /// the mock-up sizes that pill as `max(min-width, text + padding)`, so its
    /// box is a function of the number inside it, and the font is the only thing
    /// that knows how wide a number is.
    ///
    /// "Plain" is the whole of the difference from [`Self::measure_chrome_label`]
    /// — the face's regular weight, no tracking and proportional figures, which
    /// is what the great majority of this chrome's labels are drawn as. A caller
    /// whose label carries any of the three must measure with it; see the note
    /// on [`chrome_label_attrs`].
    pub fn measure_chrome_text(
        &mut self,
        gpu: &mut GpuContext,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        self.measure_chrome_label(
            gpu,
            text,
            font_size_px,
            ChromeLabelWeight::Regular,
            0.0,
            false,
        )
    }

    /// How wide `text` will be when drawn as a [`ChromeLabel`] with
    /// [`ChromeLabel::mono`] set, in physical pixels.
    ///
    /// **What it is for is a column count, not a box** (§7.1.6b′ F2). A mini
    /// transcript row is clipped by the card, so nothing about the card's
    /// geometry needs this; what needs it is the *cut* — how many characters of
    /// a running session's line are worth carrying into the frame at all. Asking
    /// the face for one character's advance answers that exactly rather than by
    /// a guessed ratio, and it is exact precisely because the face is
    /// monospaced: every character in it has that same advance.
    pub fn measure_chrome_mono_text(
        &mut self,
        gpu: &mut GpuContext,
        text: &str,
        font_size_px: f32,
    ) -> f32 {
        measure_chrome_label(
            &mut gpu.font_system,
            text,
            font_size_px,
            ChromeLabelWeight::Regular,
            0.0,
            false,
            true,
        )
    }

    /// How wide `text` will be when drawn as a [`ChromeLabel`] carrying `weight`,
    /// `letter_spacing_em` and `tabular_numerals`.
    ///
    /// The measurement a *button* needs: `.float-win .fly-head button` is
    /// `font-weight: 600; letter-spacing: .04em; text-transform: uppercase`, and
    /// a box sized from the untracked regular-weight width of the same string is
    /// a box the caption overflows by a letter. The measurement a *meta column*
    /// needs is the third of the three: `font-variant-numeric: tabular-nums`
    /// widens `1` to the widest digit's advance, and a box cut from the
    /// proportional width is a box the age runs out of.
    pub fn measure_chrome_label(
        &mut self,
        gpu: &mut GpuContext,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        letter_spacing_em: f32,
        tabular_numerals: bool,
    ) -> f32 {
        measure_chrome_label(
            &mut gpu.font_system,
            text,
            font_size_px,
            weight,
            letter_spacing_em,
            tabular_numerals,
            false,
        )
    }

    /// Re-measure the cell for a new scale factor and drop everything keyed by
    /// the pixel font size the old one named.
    ///
    /// Every cache cleared here except one is this window's, which is the whole
    /// reason they sit on this side of the split: a window dragged to a 1.5x
    /// monitor invalidates its own composed rows and its own shaping, and says
    /// nothing about a window still on the 2.0x one.
    ///
    /// The exception is `math_textures`, which is the device's shared LRU. A
    /// scale change re-rasterizes every band, so the entries this window put
    /// there are dead; clearing the whole cache also drops a second window's
    /// live entries, which costs that window one re-upload per band and cannot
    /// draw anything wrong. Narrowing that to this window's own keys wants a
    /// window tag on the key and belongs with the slice that opens the second
    /// window, not with the one that splits the type.
    pub fn update_scale_factor(
        &mut self,
        gpu: &mut GpuContext,
        scale_factor: f64,
    ) -> Result<CellMetrics, RenderError> {
        let metrics = CellMetrics::measure_at(
            &mut gpu.font_system,
            scale_factor,
            gpu.terminal_font_size_logical_px,
        )?;
        self.adopt_metrics(gpu, metrics);
        Ok(self.metrics)
    }

    /// Re-measure and re-shape after [`GpuContext::set_terminal_font`] moved the
    /// grid's face or its size.
    ///
    /// **The same invalidation as a DPI change, and deliberately the same code
    /// path.** A new face and a new monitor are one event to everything
    /// downstream of the measurement: both change the advance, the row height
    /// and the baseline, so both make every measurement, every shaped run,
    /// every composed row and every rasterized band describe a grid that is no
    /// longer on screen. Two lists that had to be kept in step by hand would
    /// drift the first time one of them gained an entry — see
    /// [`Self::adopt_metrics`], which is that one list.
    ///
    /// The scale factor is the window's own and is not passed in: a font change
    /// does not move the window, so the monitor it is on is the monitor it was
    /// already on.
    pub fn apply_font_change(&mut self, gpu: &mut GpuContext) -> Result<CellMetrics, RenderError> {
        let metrics = CellMetrics::measure_at(
            &mut gpu.font_system,
            self.metrics.scale_factor,
            gpu.terminal_font_size_logical_px,
        )?;
        self.adopt_metrics(gpu, metrics);
        Ok(self.metrics)
    }

    /// Take a freshly measured grid and throw away everything that described
    /// the old one.
    ///
    /// The whole list, in one place, because the failure mode of a font or DPI
    /// change is never a crash — it is a window that keeps drawing correct
    /// glyphs at the wrong size, from a cache whose key did not happen to
    /// include the thing that moved.
    fn adopt_metrics(&mut self, gpu: &mut GpuContext, metrics: CellMetrics) {
        self.metrics = metrics;
        self.text_rows.clear();
        self.status_overlay = None;
        self.composed_row_cache.clear();
        self.font_revision = self.font_revision.saturating_add(1);
        self.narrow_shaping_cache.clear();
        self.wide_shaping_cache.clear();
        gpu.math_textures.clear();
    }

    /// How many times this window's grid has been re-measured.
    ///
    /// Every composed row is keyed by it, so a caller that keys anything else on
    /// the shape of the grid — the math layout, which caches typeset bands
    /// against the metrics they were laid out for — has to read the same number
    /// rather than assume one.
    #[must_use]
    pub fn font_revision(&self) -> u64 {
        self.font_revision
    }

    /// Grow the per-seat glyphon slots so each of `count` seats owns one.
    ///
    /// Grown and never shrunk, exactly as `overlay_text_renderers` is: closing a
    /// pane is a common thing to do and re-opening one should not have to build
    /// GPU objects again.
    fn ensure_seat_slots(&mut self, gpu: &mut GpuContext, count: usize) {
        while self.seat_slots.len() < count {
            self.seat_slots.push(SeatTextSlot {
                viewport: Viewport::new(&gpu.device, &gpu.glyphon_cache),
                text_renderer: TextRenderer::new(
                    &mut gpu.atlas,
                    &gpu.device,
                    wgpu::MultisampleState::default(),
                    None,
                ),
                status_text_renderer: TextRenderer::new(
                    &mut gpu.atlas,
                    &gpu.device,
                    wgpu::MultisampleState::default(),
                    None,
                ),
            });
        }
    }

    /// Present one terminal frame into the seat the caller last placed.
    ///
    /// The N = 1 door into [`Self::present_frame`]. Kept as its own entry point
    /// because a lone terminal leaf is the shape this product opens in, and
    /// because every replay and probe path has exactly one shell by
    /// construction.
    pub fn present(
        &mut self,
        gpu: &mut GpuContext,
        frame: &ViewportFrame,
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        self.present_frame(
            gpu,
            &[SeatFrame {
                seat: self.seat,
                // Nothing animates on this path — it is the single-seat wrapper
                // — so the box the contents appear in is the box they were laid
                // out in, and the two calls below take the values they always did.
                clip: self.seat,
                frame,
                focused: true,
            }],
            trigger,
        )
    }

    /// Present every terminal leaf of a tab, each into its own seat rectangle.
    ///
    /// One pass, N seats. The per-seat work — glyphon resolution, grid text,
    /// the status overlay, cell rectangles, math bands and their toolbars — runs
    /// once per entry against that entry's rectangle; the window-level work —
    /// seat chrome, the peek flyout, a preview's picture, the modal overlay —
    /// runs once, after, exactly where it ran before.
    ///
    /// An empty slice is a legal frame, not an error: a tab whose panes are all
    /// files columns has no terminal to draw and still owes the window its
    /// chrome.
    ///
    /// # One call, and why there is no `prepare` beside it
    ///
    /// Preparing and rendering are one entry point on purpose, and this is the
    /// invariant the multiwindow spike (Q2) asked slice A be built around:
    /// **the prepares and the render of one window's one frame must complete as
    /// a pair, with no other window's prepare between them.**
    ///
    /// The atlas is shared. A `prepare` can grow it, and growing it moves every
    /// glyph already placed in it — including the ones another window's
    /// `TextRenderer` has already been handed coordinates for. Batch two
    /// windows' prepares and render them afterwards and the first window draws
    /// its text from the wrong place in the atlas. glyphon 0.12 exports no
    /// occupancy or mutation counter (every atlas field of [`RenderProbeSample`]
    /// is permanently `None`), so nothing will report the violation — it will
    /// only be visible as the other window's characters turning to confetti.
    ///
    /// Making the pair a single call is how that is enforced rather than
    /// documented: there is no public `prepare` for a caller to hold open.
    pub fn present_frame(
        &mut self,
        gpu: &mut GpuContext,
        seats: &[SeatFrame<'_>],
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        let frame_started = Instant::now();
        for entry in seats {
            entry.frame.validate_shape()?;
        }
        let validated_at = Instant::now();
        // Chrome text is laid out in window pixels and gets its own resolution.
        // Grid text is laid out in seat-local pixels, so each seat's resolution
        // is that seat's, updated inside the loop below; the pass viewport lands
        // those pixels at the seat's corner.
        self.chrome_viewport.update(
            &gpu.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        let viewport_updated_at = Instant::now();
        self.ensure_seat_slots(gpu, seats.len());
        // Outside this function `self.seat` names the focused seat, and the tail
        // of the loop puts it back. Inside, it is the seat *currently being
        // composed*, which is what every extent helper below already means by
        // it: a band's right edge, the status bar's width and the cell
        // rectangles' normalisation are all seat-relative by design.
        let focused_seat = seats
            .iter()
            .find(|entry| entry.focused)
            .map_or(self.seat, |entry| entry.seat);
        let empty_rect = [RectInstance::zeroed()];
        // How much of the window reaches the eye, read **once** for the whole
        // frame (§7.1.6c-4b/4f): the clear, a program's cell backgrounds, every
        // chrome band and the rail's own panel are one sheet of glass, and two
        // reads of a value a settings write can move between them is how a sheet
        // becomes two.
        let ground_alpha = ground::window_ground().alpha;
        let mut prepared: Vec<PreparedSeat> = Vec::with_capacity(seats.len());
        // Every rendered table on screen, from every seat, gathered as this frame's extra preview
        // bodies. They join the list the caller set rather than forming a pass of their own: a
        // body is already "fills then text inside one clip, in window coordinates", which is
        // exactly what a table block is, and a second pass would be a second answer to the same
        // question about ordering.
        let mut table_block_bodies: Vec<PreviewBody> = Vec::new();
        let mut focused_text_stats = None;
        let mut rows_prepared_at = validated_at;
        let mut atlas_prepared_at = validated_at;
        let mut math_prepared_at = validated_at;

        for (index, entry) in seats.iter().enumerate() {
            let frame = entry.frame;
            self.seat = entry.seat;
            self.seat_slots[index].viewport.update(
                &gpu.queue,
                Resolution {
                    width: entry.seat.width,
                    height: entry.seat.height,
                },
            );
            let text_stats = self.prepare_text_rows(gpu, frame)?;
            rows_prepared_at = Instant::now();
            // `text_rows` and `status_overlay` stay single slots on purpose:
            // they are staging for the prepare that immediately follows, and
            // glyphon copies what it needs into this seat's own renderer. What
            // may not be shared is the renderer, and it is not.
            let text_prepare_result = {
                let slot = &mut self.seat_slots[index];
                match prepare_text_atlas(
                    &mut slot.text_renderer,
                    &gpu.device,
                    &gpu.queue,
                    &mut gpu.font_system,
                    &mut gpu.atlas,
                    &slot.viewport,
                    &mut gpu.swash_cache,
                    &self.text_rows,
                    self.metrics,
                    frame,
                ) {
                    Ok(()) => prepare_status_text_atlas(
                        &mut slot.status_text_renderer,
                        &gpu.device,
                        &gpu.queue,
                        &mut gpu.font_system,
                        &mut gpu.atlas,
                        &slot.viewport,
                        &mut gpu.swash_cache,
                        self.status_overlay.as_deref(),
                        self.metrics,
                        frame,
                        entry.seat.width as f32,
                    ),
                    Err(error) => Err(error),
                }
            };
            let text_prepared = match text_prepare_result {
                Ok(()) => true,
                Err(error) => {
                    // glyphon grows each atlas geometrically before returning AtlasFull. If the
                    // device limit is genuinely exhausted, keep the terminal alive and present the
                    // theme/background rectangles; trimming allows the next frame to retry.
                    match prepare_failure_policy(error) {
                        PrepareFailurePolicy::PresentWithoutText => {
                            if self.glyph_degraded_frames == 0 {
                                eprintln!(
                                    "Folio glyph atlas reached the device limit; presenting without text and retrying"
                                );
                            }
                            self.glyph_degraded_frames += 1;
                            gpu.atlas.trim();
                            false
                        }
                    }
                }
            };
            atlas_prepared_at = Instant::now();

            // Math draws first: the hover dim rect decorates a block's raster, so it must know which
            // rasters this frame actually put on screen before it decides to darken anything.
            let math_batch = self.prepare_math_draws(gpu, frame);
            table_block_bodies.extend(self.table_block_bodies(frame));
            math_prepared_at = Instant::now();
            let math_vertex_buffer = (!math_batch.vertices.is_empty()).then(|| {
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("visible math block vertices"),
                        contents: bytemuck::cast_slice(&math_batch.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            let SeatRects {
                grounds: ground_rects,
                ink: rects,
            } = self.rectangles(
                frame,
                &math_batch.drawn,
                self.window_focused && entry.focused,
                ground_alpha,
            );
            let ground_rect_data = if ground_rects.is_empty() {
                empty_rect.as_slice()
            } else {
                ground_rects.as_slice()
            };
            let ground_rect_buffer =
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("terminal cell grounds"),
                        contents: bytemuck::cast_slice(ground_rect_data),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let rect_data = if rects.is_empty() {
                empty_rect.as_slice()
            } else {
                rects.as_slice()
            };
            let rect_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terminal cell rectangles"),
                    contents: bytemuck::cast_slice(rect_data),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let status_rects = frame
                .status_text
                .as_deref()
                .and_then(|status| {
                    status_overlay_geometry(self.metrics, frame, status, entry.seat.width as f32)
                })
                .map(|geometry| {
                    self.pixel_rect(
                        geometry.rect[0],
                        geometry.rect[1],
                        geometry.rect[2],
                        geometry.rect[3],
                        DEFAULT_STATUS_BACKGROUND_RGB,
                    )
                })
                .into_iter()
                .collect::<Vec<_>>();
            let status_rect_data = if status_rects.is_empty() {
                empty_rect.as_slice()
            } else {
                status_rects.as_slice()
            };
            let status_rect_buffer =
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("status overlay rectangle"),
                        contents: bytemuck::cast_slice(status_rect_data),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            let math_overlays = self.math_overlay_rectangles(frame);
            let overlay_data = if math_overlays.is_empty() {
                empty_rect.as_slice()
            } else {
                math_overlays.as_slice()
            };
            let math_overlay_buffer =
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("math toolbar overlay rectangles"),
                        contents: bytemuck::cast_slice(overlay_data),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
            if entry.focused {
                focused_text_stats = Some(text_stats);
            }
            prepared.push(PreparedSeat {
                seat: entry.seat,
                // Clamped here rather than trusted, and only this one of the
                // pair: `seat` is the solver's own answer travelling under a
                // translation that keeps both of its ends on the surface, while
                // `clip` is what the scissor is validated against and a scissor
                // outside the attachment is a device error rather than a
                // clipped-away pixel. `clamped_to` never returns an empty
                // rectangle, which is the other half of that validation.
                clip: entry.clip.clamped_to(self.config.width, self.config.height),
                slot: index,
                ground_rect_buffer,
                ground_rect_count: ground_rects.len(),
                rect_buffer,
                rect_count: rects.len(),
                math_vertex_buffer,
                math_draws: math_batch.draws,
                text_prepared,
                status_rect_buffer,
                status_rect_count: status_rects.len(),
                math_overlay_buffer,
                math_overlay_count: math_overlays.len(),
            });
        }
        // Back to the focused seat before anything window-level is prepared:
        // outside this function `self.seat` names the focused seat, and every
        // seat-relative helper reached from elsewhere — the IME cursor area, a
        // band's right edge — means that one by it.
        self.seat = focused_seat;

        let (peek_rects, peek_draws, peek_vertices) = self.prepare_peek_draws(gpu);
        let peek_rect_buffer = (!peek_rects.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("peek flyout rectangles"),
                    contents: bytemuck::cast_slice(&peek_rects),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let peek_vertex_buffer = (!peek_vertices.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("peek flyout image vertices"),
                    contents: bytemuck::cast_slice(&peek_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let (preview_seat, preview_draws, preview_vertices) = self.prepare_preview_draws(gpu);
        let preview_vertex_buffer = (!preview_vertices.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview seat image vertices"),
                    contents: bytemuck::cast_slice(&preview_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        // Seat chrome. Empty whenever the tree is a lone terminal leaf, and every
        // branch below is guarded on emptiness, so a lone leaf issues exactly the
        // command stream it issued before seats existed.
        //
        // **Two buffers and not one** (§7.1.6c-4b, "one translucency"): the
        // grounds carry the window's own alpha and are drawn with the clear's
        // arithmetic, the ink is opaque and blends over them. Splitting them
        // costs nothing in order — a ground is the bottom of its own region's
        // stack by construction, since it is the band the rest is struck on.
        let chrome_ground_rects: Vec<RectInstance> = self
            .chrome_quads
            .iter()
            .filter(|quad| quad.surface == ChromeSurface::Ground)
            .map(|quad| {
                premultiplied_surface_pixel_rect(
                    quad.rect,
                    quad.color,
                    ground_alpha,
                    self.config.width,
                    self.config.height,
                )
            })
            .collect();
        let chrome_ground_rect_buffer = (!chrome_ground_rects.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("seat chrome grounds"),
                    contents: bytemuck::cast_slice(chrome_ground_rects.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let chrome_rects: Vec<RectInstance> = self
            .chrome_quads
            .iter()
            .filter(|quad| quad.surface == ChromeSurface::Ink)
            .map(|quad| {
                surface_pixel_rect(quad.rect, quad.color, self.config.width, self.config.height)
            })
            .collect();
        let chrome_rect_buffer = (!chrome_rects.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("seat chrome rectangles"),
                    contents: bytemuck::cast_slice(chrome_rects.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let chrome_icons = std::mem::take(&mut self.chrome_icons);
        let (chrome_icon_draws, chrome_icon_vertices) =
            self.prepare_chrome_icon_draws(gpu, &chrome_icons);
        self.chrome_icons = chrome_icons;
        let chrome_icon_buffer = (!chrome_icon_vertices.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("chrome mark vertices"),
                    contents: bytemuck::cast_slice(chrome_icon_vertices.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let chrome_layouts = shape_chrome_labels(
            &mut gpu.font_system,
            &self.chrome_labels,
            gpu.chrome_cap_height_ratio,
            1.0,
        );
        let chrome_prepared = !chrome_layouts.is_empty()
            && prepare_chrome_text_atlas(
                &mut self.chrome_text_renderer,
                &gpu.device,
                &gpu.queue,
                &mut gpu.font_system,
                &mut gpu.atlas,
                &self.chrome_viewport,
                &mut gpu.swash_cache,
                &chrome_layouts,
            )
            .is_ok();
        // One flat list over every body, because they are one draw: each body
        // already carries its own `clip` in whole-surface coordinates, so two
        // documents on screen are two sets of cropped rectangles and not two
        // passes.
        let preview_body_rects: Vec<RectInstance> = self
            .preview_bodies
            .iter()
            .chain(table_block_bodies.iter())
            .flat_map(|body| {
                preview_body_rect_instances(body, self.config.width, self.config.height)
            })
            .collect();
        let preview_body_rect_buffer = (!preview_body_rects.is_empty()).then(|| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview body fills"),
                    contents: bytemuck::cast_slice(preview_body_rects.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let mut preview_text_layouts: Vec<ChromeTextLayout> = Vec::new();
        for body in self.preview_bodies.iter().chain(table_block_bodies.iter()) {
            preview_text_layouts.extend(shape_preview_body(&mut gpu.font_system, body));
        }
        let preview_text_prepared = !preview_text_layouts.is_empty()
            && prepare_chrome_text_atlas(
                &mut self.preview_text_renderer,
                &gpu.device,
                &gpu.queue,
                &mut gpu.font_system,
                &mut gpu.atlas,
                &self.chrome_viewport,
                &mut gpu.swash_cache,
                &preview_text_layouts,
            )
            .is_ok();
        // **The forensic line for "the body drew its rules and none of its
        // words"** (user report 2026-08-21). The three numbers are the three
        // places that picture can come from and they separate them completely:
        // a body that was never built (`bodies`), a body built whose paragraphs
        // did not survive their own clip (`paragraphs` standing while `drawn`
        // falls to zero — `shape_preview_body`'s filters), and a batch that was
        // shaped and then refused by the atlas (`drawn` standing while
        // `prepared` is false). Recorded here rather than reasoned about later
        // because the frame is gone by the time anybody asks.
        self.preview_text_frame = PreviewTextFrame {
            bodies: self.preview_bodies.len(),
            paragraphs: self
                .preview_bodies
                .iter()
                .map(PreviewBody::paragraph_count)
                .sum(),
            quads: self
                .preview_bodies
                .iter()
                .map(PreviewBody::quad_count)
                .sum(),
            drawn: preview_text_layouts.len(),
            prepared: preview_text_prepared,
        };
        // The modal overlay, one layer at a time. Empty on every frame no dialog
        // is up, and every branch below is guarded on emptiness, so a window
        // without one issues exactly the command stream it issued before modals
        // existed.
        //
        // Each layer keeps its own rectangle buffer, its own mark buffer and its
        // own text renderer, because that is what lets the pass finish a layer's
        // three channels before opening the next one's — a popup's face cannot
        // cover a caption it shares a text batch with.
        let overlay_layers = std::mem::take(&mut self.overlay_layers);
        let mut overlay_draws: Vec<PreparedOverlayLayer> = Vec::with_capacity(overlay_layers.len());
        for (index, layer) in overlay_layers.iter().enumerate() {
            // The layer's grounds, on the clear's own arithmetic and in their own
            // buffer: the pass draws them under everything else this layer has,
            // through a pipeline the layer's opacity is a blend constant of.
            let ground_rects: Vec<RectInstance> = layer
                .grounds
                .iter()
                .map(|ground| {
                    premultiplied_surface_pixel_rect(
                        ground.rect,
                        ground.color,
                        ground_alpha,
                        self.config.width,
                        self.config.height,
                    )
                })
                .collect();
            let ground_buffer = (!ground_rects.is_empty()).then(|| {
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("overlay layer grounds"),
                        contents: bytemuck::cast_slice(ground_rects.as_slice()),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            let mut rects: Vec<RectInstance> = layer
                .faded_quads()
                .iter()
                .map(|quad| {
                    surface_pixel_rect_with_alpha(
                        quad.rect,
                        quad.color,
                        quad.alpha,
                        self.config.width,
                        self.config.height,
                    )
                })
                .collect();
            // The document's fills, over this layer's own face and under its
            // captions — the same order, and the same croppings, the seat's lane
            // uses, because it is the same document drawn one layer up.
            if let Some(body) = layer.body.as_ref() {
                rects.extend(preview_body_rect_instances(
                    body,
                    self.config.width,
                    self.config.height,
                ));
            }
            let rect_buffer = (!rects.is_empty()).then(|| {
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("modal overlay rectangles"),
                        contents: bytemuck::cast_slice(rects.as_slice()),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            let (icon_draws, icon_vertices) =
                self.prepare_chrome_icon_draws(gpu, &layer.faded_icons());
            let icon_buffer = (!icon_vertices.is_empty()).then(|| {
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("modal overlay mark vertices"),
                        contents: bytemuck::cast_slice(icon_vertices.as_slice()),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
            let mut layouts = shape_chrome_labels(
                &mut gpu.font_system,
                &layer.labels,
                gpu.chrome_cap_height_ratio,
                layer.opacity,
            );
            if let Some(body) = layer.body.as_ref() {
                layouts.extend(shape_preview_body(&mut gpu.font_system, body));
            }
            while self.overlay_text_renderers.len() <= index {
                self.overlay_text_renderers.push(TextRenderer::new(
                    &mut gpu.atlas,
                    &gpu.device,
                    wgpu::MultisampleState::default(),
                    None,
                ));
            }
            let text_prepared = !layouts.is_empty()
                && prepare_chrome_text_atlas(
                    &mut self.overlay_text_renderers[index],
                    &gpu.device,
                    &gpu.queue,
                    &mut gpu.font_system,
                    &mut gpu.atlas,
                    &self.chrome_viewport,
                    &mut gpu.swash_cache,
                    &layouts,
                )
                .is_ok();
            overlay_draws.push(PreparedOverlayLayer {
                ground_buffer,
                ground_count: ground_rects.len() as u32,
                ground_opacity: layer.opacity.clamp(0.0, 1.0),
                rect_buffer,
                rect_count: rects.len() as u32,
                icon_buffer,
                icon_draws,
                text_prepared,
            });
        }
        self.overlay_layers = overlay_layers;
        let overlay_has_work = overlay_draws.iter().any(|layer| {
            layer.ground_buffer.is_some()
                || layer.rect_buffer.is_some()
                || layer.icon_buffer.is_some()
                || layer.text_prepared
        });
        let rectangles_prepared_at = Instant::now();
        // Keep the old DXGI back buffers alive while CPU shaping and GPU resource preparation run.
        // ResizeBuffers discards them; configuring only immediately before acquire/submit bounds
        // both the default-black interval and DXGI's stretch of the old frame.
        self.configure_surface_if_needed(gpu)?;
        let acquisition = self.acquire();
        let (acquired, view) = match acquisition {
            SurfaceAcquisition::Frame(texture) => {
                let view = texture.texture.create_view(&Default::default());
                (AcquiredFrame::Swapchain(texture), view)
            }
            SurfaceAcquisition::Suboptimal(texture) => {
                self.configure_surface(gpu)?;
                let view = texture.texture.create_view(&Default::default());
                (AcquiredFrame::Swapchain(texture), view)
            }
            SurfaceAcquisition::Failed(failure) => {
                return self.handle_surface_failure(gpu, failure);
            }
            SurfaceAcquisition::Offscreen(view) => (AcquiredFrame::Offscreen, view),
        };
        let surface_acquired_at = Instant::now();
        // The window's ground picture — uploaded (at most once per file) and its
        // quad measured here rather than inside the pass, because the pass holds
        // a borrow of the device for its whole life. `None` for the ordinary
        // window: no picture chosen, and then the clear is the entire ground.
        let ground = ground::window_ground();
        // Asked on every frame and not only when there is a picture: the slot
        // *is* the ground's picture, so the frame that first has none is the
        // frame that has to let the last one go.
        let ground_quad = gpu
            .hold_background_texture(ground.image.as_deref())
            .then(|| {
                let image = ground.image.as_ref().expect("a held texture has a picture");
                let uv = ground::background_uv_rect(
                    ground.fit,
                    self.config.width,
                    self.config.height,
                    image.width_px,
                    image.height_px,
                );
                let [r, g, b] = srgb_rgb_to_linear(default_background());
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("window ground quad"),
                        contents: bytemuck::cast_slice(&background_quad_vertices(
                            uv,
                            [r as f32, g as f32, b as f32],
                            ground.alpha,
                            ground.image_opacity,
                        )),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Folio frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Folio terminal pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Theme colors are authored in sRGB. The sRGB surface encodes the linear
                        // clear value exactly once, matching the rectangle upload path below.
                        load: wgpu::LoadOp::Clear(theme_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            // The ground picture, before the first seat and after nothing: one
            // quad over the whole surface, at the pass's default viewport. Its
            // blend is `Replace`, so it supersedes the clear rather than sitting
            // on top of it — the two write the same value for the same ground,
            // and this one additionally carries the picture.
            //
            // Per window and not per pane: a split is two views of one place,
            // and a picture cut at every divider would move every time one was
            // dragged.
            if let Some(buffer) = ground_quad.as_ref()
                && let Some((_, bind_group)) = gpu.background_texture.as_ref()
            {
                pass.set_pipeline(&gpu.background_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..1);
            }
            // Everything a terminal draws is in seat-local pixels; the
            // viewport/scissor pair opening each iteration is the entire
            // translation, and for a lone leaf the seat *is* the surface, so for
            // N = 1 these are the two calls that were always here, with the same
            // values, around the same draws.
            for seat in &prepared {
                pass.set_viewport(
                    seat.seat.x as f32,
                    seat.seat.y as f32,
                    seat.seat.width as f32,
                    seat.seat.height as f32,
                    0.0,
                    1.0,
                );
                // Viewport from `seat`, scissor from `clip`. The two are equal
                // at rest — for `N = 1` they are both the whole surface — and
                // differ only while a pane is mid-FLIP, where the difference is
                // precisely the mock-up's counter-scale (see [`SeatFrame::clip`]).
                pass.set_scissor_rect(seat.clip.x, seat.clip.y, seat.clip.width, seat.clip.height);
                let slot = &self.seat_slots[seat.slot];
                // The paper a program declared, before the ink printed on it: the
                // same order the one list had, drawn with the clear's own
                // arithmetic so a banner is the window wearing another colour
                // rather than a slab laid on it (§7.1.6c-4f).
                if seat.ground_rect_count > 0 {
                    pass.set_pipeline(&gpu.ground_rect_pipeline);
                    pass.set_vertex_buffer(0, seat.ground_rect_buffer.slice(..));
                    pass.draw(0..6, 0..seat.ground_rect_count as u32);
                }
                if seat.rect_count > 0 {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, seat.rect_buffer.slice(..));
                    pass.draw(0..6, 0..seat.rect_count as u32);
                }
                if let Some(vertex_buffer) = seat.math_vertex_buffer.as_ref() {
                    pass.set_pipeline(&gpu.math_pipeline);
                    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    for draw in &seat.math_draws {
                        if let Some(texture) = gpu.math_textures.get(&draw.key)
                            && let Some(tile) = texture.tiles.get(draw.tile_index)
                        {
                            pass.set_bind_group(0, &tile.bind_group, &[]);
                            pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                        }
                    }
                }
                if seat.text_prepared {
                    slot.text_renderer
                        .render(&gpu.atlas, &slot.viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
                if seat.text_prepared && seat.status_rect_count > 0 {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, seat.status_rect_buffer.slice(..));
                    pass.draw(0..6, 0..seat.status_rect_count as u32);
                    slot.status_text_renderer
                        .render(&gpu.atlas, &slot.viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
                if seat.math_overlay_count > 0 {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, seat.math_overlay_buffer.slice(..));
                    pass.draw(0..6, 0..seat.math_overlay_count as u32);
                }
            }
            // Seat chrome last, with the pass restored to the whole window: it is
            // the one class of draw that legitimately owns the space between
            // seats. Skipped entirely when there is no chrome.
            if chrome_ground_rect_buffer.is_some()
                || chrome_rect_buffer.is_some()
                || chrome_icon_buffer.is_some()
                || chrome_prepared
            {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                // The grounds first, and with the clear's own blend: each one
                // *is* the window at its rectangle, so it supersedes whatever is
                // there rather than sitting on it. Everything below is struck on
                // what these lay down.
                if let Some(buffer) = chrome_ground_rect_buffer.as_ref() {
                    pass.set_pipeline(&gpu.ground_rect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..chrome_ground_rects.len() as u32);
                }
                if let Some(buffer) = chrome_rect_buffer.as_ref() {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..chrome_rects.len() as u32);
                }
                // Marks sit between the flat fills and the text: the active tab's
                // own silhouette is a mark, and it has to land over the title
                // bar's fill and under the tab's title.
                if let Some(buffer) = chrome_icon_buffer.as_ref() {
                    pass.set_pipeline(&gpu.math_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for draw in &chrome_icon_draws {
                        if let Some(texture) = gpu.math_textures.get(&draw.key)
                            && let Some(tile) = texture.tiles.get(draw.tile_index)
                        {
                            pass.set_bind_group(0, &tile.bind_group, &[]);
                            pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                        }
                    }
                }
                if chrome_prepared {
                    self.chrome_text_renderer
                        .render(&gpu.atlas, &self.chrome_viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
            }
            // Preview content is above that seat's body chrome, but its viewport excludes the title
            // bar, so the filename and existing close affordance remain visible.
            if let (Some((seat, clip)), Some(vertex_buffer)) =
                (preview_seat, preview_vertex_buffer.as_ref())
            {
                // U8 — the viewport is where the picture was laid out and the
                // scissor is the box it may appear in, exactly as a terminal
                // seat's pair is. The two are the same rectangle at rest, so a
                // preview that is not in flight issues the calls it always did.
                pass.set_viewport(
                    seat.x as f32,
                    seat.y as f32,
                    seat.width as f32,
                    seat.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(clip.x, clip.y, clip.width, clip.height);
                pass.set_pipeline(&gpu.math_pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for draw in &preview_draws {
                    if let Some(texture) = gpu.math_textures.get(&draw.key)
                        && let Some(tile) = texture.tiles.get(draw.tile_index)
                    {
                        pass.set_bind_group(0, &tile.bind_group, &[]);
                        pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                    }
                }
            }
            // The preview seat's text body, in the same slot as its picture and
            // for the same reason — above that seat's body chrome, below the
            // floating windows. Whole-surface viewport rather than the picture's
            // seat-local one: these coordinates are already the window's, and
            // every line carries its own clip box, so a pane FLIP crops them
            // without the pass having to.
            if preview_text_prepared || preview_body_rect_buffer.is_some() {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                // Fills first: a diff's line tint, a fence's ground, a table's
                // grid — every one of them is *under* the text of the very same
                // body, and both are drawn in the document's own scrolled
                // coordinates.
                if let Some(buffer) = preview_body_rect_buffer.as_ref() {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..preview_body_rects.len() as u32);
                }
                if preview_text_prepared {
                    self.preview_text_renderer
                        .render(&gpu.atlas, &self.chrome_viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
            }
            // The hover-peek flyout, over every seat, over seat chrome and over a
            // preview's picture — a floating window is not a child of the pane it
            // was raised from. Its rectangles are already the window's own, so the
            // pass is restored to the surface and nothing is translated.
            //
            // Drawn here rather than inside the seat loop, which is what made it
            // the focused pane's tenant: laid out in that pane's coordinates,
            // scissored to that pane's box, and therefore unable either to answer
            // a hover in another pane or to overhang the pane it belongs to.
            if peek_rect_buffer.is_some() || peek_vertex_buffer.is_some() {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                if let Some(rect_buffer) = peek_rect_buffer.as_ref() {
                    pass.set_pipeline(&gpu.rect_pipeline);
                    pass.set_vertex_buffer(0, rect_buffer.slice(..));
                    pass.draw(0..6, 0..peek_rects.len() as u32);
                }
                if let Some(vertex_buffer) = peek_vertex_buffer.as_ref() {
                    pass.set_pipeline(&gpu.math_pipeline);
                    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    for draw in &peek_draws {
                        if let Some(texture) = gpu.math_textures.get(&draw.key)
                            && let Some(tile) = texture.tiles.get(draw.tile_index)
                        {
                            pass.set_bind_group(0, &tile.bind_group, &[]);
                            pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                        }
                    }
                }
            }
            // The modal overlay, last of everything and over the whole window.
            // DESIGN §7.1.5: "模态遮罩 z-order 高于一切弹出层与浮窗" — and in this
            // pass "everything" has to include the two layers that outrank seat
            // chrome, the peek flyout and a preview seat's own picture. A scrim
            // that anything at all can be seen through unblurred is a scrim in
            // name only.
            if overlay_has_work {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                // Bottom layer first, and each one's three channels closed before
                // the next one's open: this loop *is* the overlay's z-order, and
                // it is the reason a picker's popup covers the row under it
                // whether that row drew itself as a fill, a mark or a caption.
                for (index, layer) in overlay_draws.iter().enumerate() {
                    // The grounds first, as they are in the chrome's own pass and
                    // for the same reason: a ground is the surface the rest of
                    // this layer is struck on. The blend constant is this layer's
                    // opacity, which is the whole of how a floating ground fades
                    // — see [`create_ground_fade_rect_pipeline`].
                    if let Some(buffer) = layer.ground_buffer.as_ref() {
                        let fade = f64::from(layer.ground_opacity);
                        pass.set_pipeline(&gpu.ground_fade_rect_pipeline);
                        pass.set_blend_constant(wgpu::Color {
                            r: fade,
                            g: fade,
                            b: fade,
                            a: fade,
                        });
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..6, 0..layer.ground_count);
                    }
                    if let Some(buffer) = layer.rect_buffer.as_ref() {
                        pass.set_pipeline(&gpu.rect_pipeline);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..6, 0..layer.rect_count);
                    }
                    if let Some(buffer) = layer.icon_buffer.as_ref() {
                        pass.set_pipeline(&gpu.math_pipeline);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        for draw in &layer.icon_draws {
                            if let Some(texture) = gpu.math_textures.get(&draw.key)
                                && let Some(tile) = texture.tiles.get(draw.tile_index)
                            {
                                pass.set_bind_group(0, &tile.bind_group, &[]);
                                pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                            }
                        }
                    }
                    if layer.text_prepared {
                        self.overlay_text_renderers[index]
                            .render(&gpu.atlas, &self.chrome_viewport, &mut pass)
                            .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                    }
                }
            }
        }
        let encoded_at = Instant::now();
        gpu.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        match acquired {
            AcquiredFrame::Swapchain(texture) => gpu.queue.present(texture),
            // Nothing to hand back: an offscreen frame is finished the moment it
            // is submitted.
            AcquiredFrame::Offscreen => {}
        }
        let present_called_at = Instant::now();
        let receipt = PresentReceipt {
            trigger,
            submitted_at,
            present_called_at,
        };
        gpu.atlas.trim();
        // The trace reports the focused seat: it is the one whose latency a
        // person is waiting on, and summing counters across seats would invent a
        // number no cache ever held (`resident_bytes` is a gauge, not a tally).
        // `seats=` names how many were drawn so the reading is never mistaken
        // for the whole window's cost.
        if self.trace_perf
            && let Some((frame, text_stats)) = seats
                .iter()
                .find(|entry| entry.focused)
                .map(|entry| entry.frame)
                .zip(focused_text_stats)
        {
            let total_elapsed = frame_started.elapsed();
            let digest_started = Instant::now();
            let digest = frame_content_digest(frame);
            let alternate_screen = frame_is_alternate_screen(frame);
            let digest_elapsed = digest_started.elapsed();
            self.perf_frame = self.perf_frame.saturating_add(1);
            eprintln!(
                "BT_PERF_TRACE frame={} seats={} source={:?} cells={} nonblank_cells={} first_text_row={} last_text_row={} content_fnv={:016x} alt={} digest_us={} validate_us={} viewport_us={} row_compose_us={} rows_reshaped={} row_cache_hits={} row_cache_misses={} row_cache_evictions={} row_cache_resident_bytes={} shape_miss_us={} narrow_hits={} narrow_misses={} narrow_evictions={} narrow_resident_bytes={} wide_hits={} wide_misses={} wide_evictions={} wide_resident_bytes={} atlas_prepare_upload_us={} atlas_hits=unmeasurable_glyphon_0_12 atlas_misses=unmeasurable_glyphon_0_12 atlas_grows=unmeasurable_glyphon_0_12 atlas_evictions=unmeasurable_glyphon_0_12 atlas_upload_bytes=unmeasurable_glyphon_0_12 rectangles_us={} math_prepare_upload_us={} math_blocks={} math_texture_evictions={} math_texture_refusals={} textureless_math_blocks={} math_texture_resident_bytes={} acquire_us={} encode_us={} submit_present_us={} total_us={}",
                self.perf_frame,
                seats.len(),
                trigger.source,
                frame.cells.len(),
                digest.nonblank_cells,
                digest.first_text_row,
                digest.last_text_row,
                digest.content_fnv,
                u8::from(alternate_screen),
                digest_elapsed.as_micros(),
                (validated_at - frame_started).as_micros(),
                (viewport_updated_at - validated_at).as_micros(),
                text_stats.elapsed.as_micros(),
                text_stats.rows_reshaped,
                text_stats.row_cache.hits,
                text_stats.row_cache.misses,
                text_stats.row_cache.evictions,
                text_stats.row_cache.resident_bytes,
                (text_stats.narrow.miss_time + text_stats.wide.miss_time).as_micros(),
                text_stats.narrow.hits,
                text_stats.narrow.misses,
                text_stats.narrow.evictions,
                text_stats.narrow.resident_bytes,
                text_stats.wide.hits,
                text_stats.wide.misses,
                text_stats.wide.evictions,
                text_stats.wide.resident_bytes,
                (atlas_prepared_at - rows_prepared_at).as_micros(),
                (rectangles_prepared_at - math_prepared_at).as_micros(),
                (math_prepared_at - atlas_prepared_at).as_micros(),
                frame.math_blocks.len(),
                gpu.math_texture_evictions,
                self.math_texture_refusals,
                self.textureless_math_blocks,
                gpu.math_textures.resident_bytes(),
                (surface_acquired_at - rectangles_prepared_at).as_micros(),
                (encoded_at - surface_acquired_at).as_micros(),
                (present_called_at - encoded_at).as_micros(),
                total_elapsed.as_micros(),
            );
        }
        Ok(PresentOutcome::Presented(receipt))
    }

    fn prepare_text_rows(
        &mut self,
        gpu: &mut GpuContext,
        frame: &ViewportFrame,
    ) -> Result<TextPreparationStats, RenderError> {
        prepare_text_rows(
            frame,
            self.metrics,
            &mut self.text_rows,
            &mut self.status_overlay,
            &mut self.composed_row_cache,
            self.font_revision,
            theme_revision(),
            &mut gpu.font_system,
            &mut gpu.swash_cache,
            &mut self.narrow_shaping_cache,
            &mut self.wide_shaping_cache,
        )
    }

    fn prepare_math_draws(&mut self, gpu: &mut GpuContext, frame: &ViewportFrame) -> MathDrawBatch {
        // UI-UX §7.5c, M1.9a ruling: do not invent automatic math line breaking. With terminal
        // wrapping on (the current native default), the pane clips a left-aligned, max-content
        // raster and therefore acts as the block's horizontal viewport. Scrolling controls are
        // part of the M1.9b interaction slice; default blockMax is unlimited, so no vertical clamp
        // is applied here.
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        let mut drawn = HashSet::new();
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.seat.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.seat.height as f32;

        for (index, placement) in frame.math_blocks.iter().enumerate() {
            if !math_block_admits_texture(frame, placement) {
                continue;
            }
            let key = &placement.artifact.key;
            if gpu.math_textures.get(key).is_none()
                && let Some(texture) = gpu.upload_math_texture(&placement.artifact)
            {
                let (admitted, evictions) =
                    gpu.math_textures
                        .insert(key.clone(), texture, placement.artifact.rgba.len());
                gpu.math_texture_evictions = gpu.math_texture_evictions.saturating_add(evictions);
                if !admitted {
                    self.note_math_texture_refusal(key, placement.artifact.rgba.len());
                }
            }
            let Some(tile_geometry) = gpu.math_textures.get(key).map(|texture| {
                texture
                    .tiles
                    .iter()
                    .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                    .collect::<Vec<_>>()
            }) else {
                // A placed band with no texture. Silence here is what painted a bare grey
                // rectangle: the band's own pixels never drew while everything around them did.
                self.note_textureless_block(gpu, key, placement.artifact.rgba.len());
                continue;
            };
            let Some(geometry) = self.math_block_geometry(frame, placement) else {
                continue;
            };
            drawn.insert(index);
            let scale = placement.artifact.render_scale_milli as f32 / 1000.0;
            let block_top = if placement.artifact.mode == MathMode::Inline {
                pane_top
                    + placement.top_subpixels as f32 / SUBPIXELS_PER_PX as f32
                    + self.metrics.ascii_baseline_px
                    - placement.artifact.baseline_subpixels as f32 / SUBPIXELS_PER_PX as f32
            } else {
                pane_top
                    + placement
                        .top_subpixels
                        .saturating_add(placement.content_offset_subpixels)
                        as f32
                        / SUBPIXELS_PER_PX as f32
            };
            for (tile_index, (tile_x, tile_y, tile_width, tile_height)) in
                tile_geometry.into_iter().enumerate()
            {
                let left = math_block_left_px(
                    self.metrics,
                    placement.left_subpixels,
                    placement.display == MathBlockDisplay::Rendered,
                ) + tile_x as f32 * scale
                    - placement.horizontal_scroll_px as f32;
                let top = block_top + tile_y as f32 * scale - placement.vertical_scroll_px as f32;
                let right = left + tile_width as f32 * scale;
                let bottom = top + tile_height as f32 * scale;
                let visible_left = left.max(geometry.clip[0]).max(pane_left);
                let visible_top = top.max(geometry.clip[1]).max(pane_top);
                let visible_right = right.min(geometry.clip[2]).min(pane_right);
                let visible_bottom = bottom.min(geometry.clip[3]).min(pane_bottom);
                if visible_right <= visible_left || visible_bottom <= visible_top {
                    continue;
                }
                let uv_left = (visible_left - left) / (tile_width as f32 * scale);
                let uv_top = (visible_top - top) / (tile_height as f32 * scale);
                let uv_right = (visible_right - left) / (tile_width as f32 * scale);
                let uv_bottom = (visible_bottom - top) / (tile_height as f32 * scale);
                let first_vertex = vertices.len() as u32;
                vertices.extend(math_quad_vertices(
                    visible_left,
                    visible_top,
                    visible_right,
                    visible_bottom,
                    uv_left,
                    uv_top,
                    uv_right,
                    uv_bottom,
                    self.seat.width,
                    self.seat.height,
                    1.0,
                ));
                draws.push(MathDraw {
                    key: key.clone(),
                    tile_index,
                    first_vertex,
                });
            }
        }
        MathDrawBatch {
            draws,
            vertices,
            drawn,
        }
    }

    /// The LRU refused the texture outright: one artifact larger than the whole budget. Nothing
    /// evicts it into fitting, so the band will stay textureless until its size changes.
    fn note_math_texture_refusal(&mut self, key: &str, resident_bytes: usize) {
        self.math_texture_refusals = self.math_texture_refusals.saturating_add(1);
        if self.trace_perf {
            eprintln!(
                "BT_PERF_TRACE math_texture_refused key={key} bytes={resident_bytes} budget={MATH_TEXTURE_CACHE_BUDGET_BYTES}"
            );
        }
    }

    /// A visible block whose texture is not resident after this frame's upload attempt. Counted
    /// always so `BT_PERF_TRACE` can carry it; printed per occurrence only under the trace.
    fn note_textureless_block(&mut self, gpu: &GpuContext, key: &str, resident_bytes: usize) {
        self.textureless_math_blocks = self.textureless_math_blocks.saturating_add(1);
        if self.trace_perf {
            eprintln!(
                "BT_PERF_TRACE math_block_without_texture key={key} bytes={resident_bytes} resident={}",
                gpu.math_textures.resident_bytes(),
            );
        }
    }

    /// Build the hover-peek flyout draws: border and background rects for the flat pipeline plus
    /// textured quads through the shared content-keyed texture LRU. Empty when no peek is up,
    /// when the owning pane cannot host the box, or when the texture upload fails.
    ///
    /// Everything here is in whole-window physical pixels — the pane the hover belongs to is
    /// consulted for the box's *size* and for nothing else — so the pass draws it with the
    /// viewport restored to the surface, above every seat and above seat chrome.
    fn prepare_peek_draws(
        &mut self,
        gpu: &mut GpuContext,
    ) -> (Vec<RectInstance>, Vec<MathDraw>, Vec<MathVertex>) {
        let Some(overlay) = self.peek_overlay.clone() else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some(layout) = peek_box_layout(
            overlay.seat.width as f32,
            overlay.seat.height as f32,
            self.config.width as f32,
            self.config.height as f32,
            self.metrics.padding_px,
            self.metrics.scale_factor as f32,
            overlay.width_px,
            overlay.height_px,
            overlay.pointer_x,
            overlay.pointer_y,
        ) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        if gpu.math_textures.get(&overlay.key).is_none()
            && let Some(texture) =
                gpu.upload_rgba_tiles(&overlay.rgba, overlay.width_px, overlay.height_px)
        {
            let (admitted, evictions) =
                gpu.math_textures
                    .insert(overlay.key.clone(), texture, overlay.rgba.len());
            gpu.math_texture_evictions = gpu.math_texture_evictions.saturating_add(evictions);
            if !admitted {
                self.note_math_texture_refusal(&overlay.key, overlay.rgba.len());
            }
        }
        let Some(tile_geometry) = gpu.math_textures.get(&overlay.key).map(|texture| {
            texture
                .tiles
                .iter()
                .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                .collect::<Vec<_>>()
        }) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let rects = self.peek_box_rects(&layout);
        let fit = (layout.image[2] - layout.image[0]) / overlay.width_px as f32;
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        for (tile_index, (tile_x, tile_y, tile_width, tile_height)) in
            tile_geometry.into_iter().enumerate()
        {
            let left = layout.image[0] + tile_x as f32 * fit;
            let top = layout.image[1] + tile_y as f32 * fit;
            let right = left + tile_width as f32 * fit;
            let bottom = top + tile_height as f32 * fit;
            let first_vertex = vertices.len() as u32;
            vertices.extend(math_quad_vertices(
                left,
                top,
                right,
                bottom,
                0.0,
                0.0,
                1.0,
                1.0,
                self.config.width,
                self.config.height,
                1.0,
            ));
            draws.push(MathDraw {
                key: overlay.key.clone(),
                tile_index,
                first_vertex,
            });
        }
        (rects, draws, vertices)
    }

    /// The flyout's own chrome, bottom to top: its lift, its hairline, its face.
    ///
    /// A floating window in the mock-up is `--menu` behind a `--border` hairline,
    /// rounded by `--floatr`, over `--shadow`. Three of those four are colours and
    /// arrive as tokens; the fourth is a shape, and it is built the same way here
    /// as the tab's is in `marks.rs` — analytic coverage rather than nested quads.
    ///
    /// The hairline is not drawn as a ring. The whole box is filled in `--border`
    /// at its own alpha and the face is laid over it one pixel in, so what stays
    /// visible is exactly the border-box the browser would leave: one blended
    /// pixel of the mock-up's own `rgba(255,255,255,.094)` over whatever the
    /// terminal is showing, with the round's antialiasing on both of its edges
    /// instead of only the outer one.
    fn peek_box_rects(&self, layout: &PeekBoxLayout) -> Vec<RectInstance> {
        peek_box_fills(layout, chrome_palette(), self.metrics.scale_factor as f32)
            .into_iter()
            .map(|fill| {
                // Normalized by the surface, not by `self.seat`: the flyout is a floating window
                // over the whole window, and its rectangles arrive here already in the window's
                // own pixels.
                surface_pixel_rect_with_alpha(
                    fill.rect,
                    fill.color,
                    fill.alpha,
                    self.config.width,
                    self.config.height,
                )
            })
            .collect()
    }

    /// Upload and place every chrome mark, in whole-surface pixels.
    ///
    /// The marks ride the same textured-quad path the math rasters and the
    /// preview image already use — one texture pipeline, one LRU, one sampler.
    /// They are keyed by their content identity, so a mark that survives a frame
    /// costs a hash lookup, and one the budget evicted simply re-uploads on the
    /// next frame from the bytes the app is still holding.
    ///
    /// Takes the list rather than reading `self.chrome_icons`, because the modal
    /// overlay's marks are the same kind of thing drawn in a different plane, and
    /// two copies of this loop would be two places for the LRU bookkeeping to
    /// drift apart.
    fn prepare_chrome_icon_draws(
        &mut self,
        gpu: &mut GpuContext,
        icons: &[ChromeIcon],
    ) -> (Vec<MathDraw>, Vec<MathVertex>) {
        let icons = icons.to_vec();
        let (surface_width, surface_height) = (self.config.width, self.config.height);
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        for icon in &icons {
            if gpu.math_textures.get(&icon.key).is_none()
                && let Some(texture) =
                    gpu.upload_rgba_tiles(&icon.rgba, icon.width_px, icon.height_px)
            {
                let (admitted, evictions) =
                    gpu.math_textures
                        .insert(icon.key.clone(), texture, icon.rgba.len());
                gpu.math_texture_evictions = gpu.math_texture_evictions.saturating_add(evictions);
                if !admitted {
                    self.note_math_texture_refusal(&icon.key, icon.rgba.len());
                }
            }
            let Some(tile_geometry) = gpu.math_textures.get(&icon.key).map(|texture| {
                texture
                    .tiles
                    .iter()
                    .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                    .collect::<Vec<_>>()
            }) else {
                continue;
            };
            let scale_x = (icon.rect[2] - icon.rect[0]) / icon.width_px.max(1) as f32;
            let scale_y = (icon.rect[3] - icon.rect[1]) / icon.height_px.max(1) as f32;
            for (tile_index, (tile_x, tile_y, tile_width, tile_height)) in
                tile_geometry.into_iter().enumerate()
            {
                let left = icon.rect[0] + tile_x as f32 * scale_x;
                let top = icon.rect[1] + tile_y as f32 * scale_y;
                let tile = [
                    left,
                    top,
                    left + tile_width as f32 * scale_x,
                    top + tile_height as f32 * scale_y,
                ];
                let Some((quad, uv)) = cropped_icon_quad(tile, icon.clip) else {
                    continue;
                };
                let first_vertex = vertices.len() as u32;
                vertices.extend(math_quad_vertices(
                    quad[0],
                    quad[1],
                    quad[2],
                    quad[3],
                    uv[0],
                    uv[1],
                    uv[2],
                    uv[3],
                    surface_width,
                    surface_height,
                    icon.opacity,
                ));
                draws.push(MathDraw {
                    key: icon.key.clone(),
                    tile_index,
                    first_vertex,
                });
            }
        }
        (draws, vertices)
    }

    /// The preview seat's picture: its viewport, the box it may appear in, and
    /// the tiles.
    ///
    /// The pair travels together for the reason [`SeatFrame`] carries both — they
    /// are computed from one sample of one clock by the caller, and a renderer
    /// holding only the viewport would have to invent the crop.
    fn prepare_preview_draws(
        &mut self,
        gpu: &mut GpuContext,
    ) -> (
        Option<(SeatViewport, SeatViewport)>,
        Vec<MathDraw>,
        Vec<MathVertex>,
    ) {
        let Some(image) = self.preview_image.clone() else {
            return (None, Vec::new(), Vec::new());
        };
        if gpu.math_textures.get(&image.key).is_none()
            && let Some(texture) =
                gpu.upload_rgba_tiles(&image.rgba, image.width_px, image.height_px)
        {
            let (admitted, evictions) =
                gpu.math_textures
                    .insert(image.key.clone(), texture, image.rgba.len());
            gpu.math_texture_evictions = gpu.math_texture_evictions.saturating_add(evictions);
            if !admitted {
                self.note_math_texture_refusal(&image.key, image.rgba.len());
            }
        }
        let Some(tile_geometry) = gpu.math_textures.get(&image.key).map(|texture| {
            texture
                .tiles
                .iter()
                .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                .collect::<Vec<_>>()
        }) else {
            return (Some((image.seat, image.clip)), Vec::new(), Vec::new());
        };
        // Signed, because a zoomed picture is wider than the box it is seen through and its left
        // edge is then off the left of the seat; the saturating unsigned subtraction this replaced
        // would have pinned it to zero and drawn the *left* of the picture whatever the pan said.
        // Floored so that the resting, unzoomed case is the integer division it has always been.
        let left_inset = ((image.seat.width as f32 - image.display_width_px as f32) / 2.0).floor()
            + image.pan_px[0];
        let top_inset = ((image.seat.height as f32 - image.display_height_px as f32) / 2.0).floor()
            + image.pan_px[1];
        let scale_x = image.display_width_px as f32 / image.width_px as f32;
        let scale_y = image.display_height_px as f32 / image.height_px as f32;
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        for (tile_index, (tile_x, tile_y, tile_width, tile_height)) in
            tile_geometry.into_iter().enumerate()
        {
            let left = left_inset + tile_x as f32 * scale_x;
            let top = top_inset + tile_y as f32 * scale_y;
            let first_vertex = vertices.len() as u32;
            vertices.extend(math_quad_vertices(
                left,
                top,
                left + tile_width as f32 * scale_x,
                top + tile_height as f32 * scale_y,
                0.0,
                0.0,
                1.0,
                1.0,
                image.seat.width,
                image.seat.height,
                1.0,
            ));
            draws.push(MathDraw {
                key: image.key.clone(),
                tile_index,
                first_vertex,
            });
        }
        (Some((image.seat, image.clip)), draws, vertices)
    }

    fn math_block_geometry(
        &self,
        frame: &ViewportFrame,
        placement: &MathBlockPlacement,
    ) -> Option<MathBlockGeometry> {
        if !frame
            .drawable_interval_overlaps(placement.top_subpixels, placement.clip_height_subpixels)
        {
            return None;
        }
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.seat.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.seat.height as f32;
        let band_top = pane_top + placement.top_subpixels as f32 / SUBPIXELS_PER_PX as f32;
        let top = if placement.artifact.mode == MathMode::Inline {
            band_top + self.metrics.ascii_baseline_px
                - placement.artifact.baseline_subpixels as f32 / SUBPIXELS_PER_PX as f32
        } else {
            band_top + placement.content_offset_subpixels as f32 / SUBPIXELS_PER_PX as f32
        };
        let clip_height = placement.clip_height_subpixels.max(1) as f32 / SUBPIXELS_PER_PX as f32;
        let scaled_width = if placement.display == MathBlockDisplay::Source {
            placement
                .source
                .lines()
                .map(|line| line.chars().count() + 4)
                .max()
                .unwrap_or(4) as f32
                * self.metrics.cell_width_px
        } else {
            placement.artifact.width_px as f32 * placement.artifact.render_scale_milli as f32
                / 1000.0
        };
        let scaled_height = if placement.display == MathBlockDisplay::Source {
            clip_height
        } else {
            placement.artifact.height_px as f32 * placement.artifact.render_scale_milli as f32
                / 1000.0
        };
        let ([visible_top, visible_bottom], [clip_top, clip_bottom]) = math_vertical_bounds(
            placement.artifact.mode,
            pane_top,
            pane_bottom,
            band_top,
            top,
            scaled_height,
            clip_height,
        );
        let (visible_left, visible_right) = math_horizontal_bounds(
            self.metrics,
            self.seat.width,
            frame.columns,
            placement.left_subpixels,
            scaled_width,
            placement.display == MathBlockDisplay::Rendered,
        )?;
        if visible_right <= visible_left || visible_bottom <= visible_top {
            return None;
        }
        let block = [visible_left, visible_top, visible_right, visible_bottom];
        // Display math owns a complete presentation box: alpha-tight ink is offset by symmetric
        // padding inside the band, while the clip is the band itself. Inline math retains its
        // baseline-relative clip. In both cases the visible raster is intersected with this clip
        // above, so the frame-level rule remains explicit: clip contains every block pixel.
        let clip = [visible_left, clip_top, pane_right, clip_bottom];
        // The scissor must never crop the visible raster: its top may not sit below the block's
        // top, nor its bottom above the block's bottom. This is the invariant the centred multi-
        // line clip violated (see above); asserting it here fails the moment any future change
        // decouples the clip from `top` again.
        debug_assert!(
            clip[1] <= block[1] + 0.5 && clip[3] >= block[3] - 0.5,
            "math scissor crops the raster: clip={clip:?} block={block:?}"
        );
        let (eye, copy) = if placement.toolbar_visible {
            let scale = self.metrics.scale_factor as f32;
            let (toolbar_top, toolbar_bottom) =
                math_toolbar_vertical_bounds(visible_top, visible_bottom, scale);
            let button = toolbar_bottom - toolbar_top;
            let gap = MATH_TOOL_GAP_LOGICAL_PX * scale;
            let total = button * 2.0 + gap;
            let left = visible_right.min(pane_right - total).max(pane_left);
            (
                Some([left, toolbar_top, left + button, toolbar_bottom]),
                Some([
                    left + button + gap,
                    toolbar_top,
                    left + total,
                    toolbar_bottom,
                ]),
            )
        } else {
            (None, None)
        };
        Some(MathBlockGeometry {
            block,
            clip,
            eye,
            copy,
        })
    }

    /// This seat's rendered tables, turned into bodies in whole-window coordinates.
    ///
    /// The geometry is the block's own, read from exactly the arithmetic `prepare_math_draws` uses
    /// to place a raster tile — same origin, same interior scroll subtraction, same clip — so a
    /// table and a formula standing in the same place stand in the *same* place. What the two do
    /// with that box then differs: a raster is a quad with a texture on it, and a table is the
    /// fills and the text its layout already decided, translated to the box's corner and cropped
    /// by the clip the body carries.
    fn table_block_bodies(&self, frame: &ViewportFrame) -> Vec<PreviewBody> {
        if self.table_blocks.is_empty() {
            return Vec::new();
        }
        let origin_x = self.seat.x as f32;
        let origin_y = self.seat.y as f32;
        let pane_top = self.metrics.padding_px;
        frame
            .math_blocks
            .iter()
            .filter(|placement| {
                placement.artifact.kind == bt_viewport::RgbaArtifactKind::Table
                    && placement.display == MathBlockDisplay::Rendered
            })
            .filter_map(|placement| {
                let paint = self.table_blocks.get(&placement.artifact.source)?;
                let geometry = self.math_block_geometry(frame, placement)?;
                let left = math_block_left_px(self.metrics, placement.left_subpixels, true)
                    - placement.horizontal_scroll_px as f32;
                let top = pane_top
                    + placement
                        .top_subpixels
                        .saturating_add(placement.content_offset_subpixels)
                        as f32
                        / SUBPIXELS_PER_PX as f32
                    - placement.vertical_scroll_px as f32;
                let shift = |rect: [f32; 4]| {
                    [
                        origin_x + left + rect[0],
                        origin_y + top + rect[1],
                        origin_x + left + rect[2],
                        origin_y + top + rect[3],
                    ]
                };
                Some(PreviewBody {
                    clip: [
                        origin_x + geometry.clip[0],
                        origin_y + geometry.clip[1],
                        origin_x + geometry.clip[2],
                        origin_y + geometry.clip[3],
                    ],
                    quads: paint
                        .quads
                        .iter()
                        .map(|quad| PreviewQuad {
                            rect: shift(quad.rect),
                            color: quad.color,
                        })
                        .collect(),
                    paragraphs: paint
                        .paragraphs
                        .iter()
                        .map(|paragraph| PreviewParagraph {
                            rect: shift(paragraph.rect),
                            ..paragraph.clone()
                        })
                        .collect(),
                    blocks: Vec::new(),
                })
            })
            .collect()
    }

    fn math_failure_geometry(
        &self,
        frame: &ViewportFrame,
        placement: &bt_viewport::MathFailurePlacement,
    ) -> Option<([f32; 4], [f32; 4])> {
        if !frame.drawable_interval_overlaps(placement.top_subpixels, placement.height_subpixels) {
            return None;
        }
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.seat.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.seat.height as f32;
        let raw_top = pane_top + placement.top_subpixels as f32 / SUBPIXELS_PER_PX as f32;
        let raw_bottom = raw_top + placement.height_subpixels as f32 / SUBPIXELS_PER_PX as f32;
        let top = raw_top.max(pane_top);
        let bottom = raw_bottom.min(pane_bottom);
        if bottom <= top || pane_right <= pane_left {
            return None;
        }
        let scale = self.metrics.scale_factor as f32;
        let marker_right = pane_right - scale;
        let marker_left = (marker_right - 2.0 * scale).max(pane_left);
        let inset = (4.0 * scale).min((bottom - top) / 3.0);
        let marker = [marker_left, top + inset, marker_right, bottom - inset];
        let hit = [
            (pane_right - 14.0 * scale).max(pane_left),
            top,
            pane_right,
            bottom,
        ];
        Some((marker, hit))
    }

    fn handle_surface_failure(
        &mut self,
        gpu: &GpuContext,
        failure: SurfaceFailure,
    ) -> Result<PresentOutcome, RenderError> {
        match surface_failure_policy(failure) {
            SurfaceFailurePolicy::Skip => Ok(PresentOutcome::Skipped),
            SurfaceFailurePolicy::Reconfigure => {
                self.configure_surface(gpu)?;
                Ok(PresentOutcome::Reconfigure)
            }
            SurfaceFailurePolicy::FatalValidation => Err(RenderError::SurfaceValidation),
        }
    }

    fn configure_surface_if_needed(&mut self, gpu: &GpuContext) -> Result<(), RenderError> {
        let requested_size = (self.config.width, self.config.height);
        if self.configured_size != requested_size {
            self.configure_surface(gpu)?;
        }
        Ok(())
    }

    /// Bring the target up to the size `config` names.
    ///
    /// A swapchain is reconfigured; an offscreen texture is rebuilt, because a
    /// texture has no `configure` and its extent is fixed at creation. Both
    /// leave `configured_size` equal to `config`, which is the only thing the
    /// rest of the frame reads.
    fn configure_surface(&mut self, gpu: &GpuContext) -> Result<(), RenderError> {
        match &mut self.target {
            FrameTarget::Surface(surface) => surface.configure(&gpu.device, &self.config),
            FrameTarget::Offscreen(texture) => {
                *texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Folio offscreen window target"),
                    size: wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.config.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
            }
        }
        self.configured_size = (self.config.width, self.config.height);
        Ok(())
    }

    /// Ask the target for this frame's attachment, and decide nothing about it.
    ///
    /// Split from acting on the answer so the borrow of [`FrameTarget`] ends
    /// before `&mut self` is needed again for a reconfigure.
    fn acquire(&self) -> SurfaceAcquisition {
        match &self.target {
            FrameTarget::Surface(surface) => match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture) => SurfaceAcquisition::Frame(texture),
                wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                    SurfaceAcquisition::Suboptimal(texture)
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    SurfaceAcquisition::Failed(SurfaceFailure::Unavailable)
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    SurfaceAcquisition::Failed(SurfaceFailure::Outdated)
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    SurfaceAcquisition::Failed(SurfaceFailure::Lost)
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    SurfaceAcquisition::Failed(SurfaceFailure::Validation)
                }
            },
            // An offscreen attachment is always available and never stale: it is
            // the same texture until a resize replaces it. Its view is taken
            // here, where the target is already in hand, so that the frame never
            // has to ask a second time which kind of target it is drawing into.
            FrameTarget::Offscreen(texture) => {
                SurfaceAcquisition::Offscreen(texture.create_view(&Default::default()))
            }
        }
    }

    /// One seat's flat fills. `seat_focused` is that seat's own caret standing:
    /// the window holds focus *and* this is the pane being typed into. Every other
    /// pane on screen is, as far as its caret is concerned, in exactly the
    /// position a window that lost focus is in.
    /// Every flat fill one seat draws, split into the two classes the
    /// one-translucency ruling names (§7.1.6c-4f, ruling of 2026-08-18).
    ///
    /// **A background is a background wherever it was declared.** The clear
    /// under a pane carries the window's alpha; a cell whose background a
    /// *program* set is the same surface at that rectangle, wearing another
    /// colour, so it carries the same alpha through the same premultiplied
    /// arithmetic. Until this ruling it did not, and the difference was visible
    /// in one screenshot: an agent's grey message bars and its "jump to bottom"
    /// chip stood as opaque slabs on a window you could otherwise see the desk
    /// through — a banner that was *more* solid than the terminal it was printed
    /// into.
    ///
    /// The line is **not** "cell fills versus everything else". It is content
    /// against state, and the fills are enumerated here rather than sampled
    /// because the failure this guards is one of them landing on the wrong side:
    ///
    /// *Content backgrounds — the ground, and they take the window's alpha:*
    /// - **A resolved cell background** that differs from the theme's default:
    ///   SGR 40-49/100-107, `48;5;N`, `48;2;r;g;b`, and a theme index the palette
    ///   answered. The program said "the paper here is this colour", and paper is
    ///   what the window's alpha is *about*.
    /// - **A reverse-video cell**, which reaches this loop already swapped by
    ///   [`resolve_colors`] and is therefore not a special case: `SGR 7` is how a
    ///   program declares a background with the two colours it already has. A
    ///   selected menu row in `fzf` and a highlighted bar in `top` are both this,
    ///   and neither is Folio's state.
    ///
    /// *State — Folio's own marks, and they stay opaque:*
    /// - **The selection.** It answers a drag the reader is making right now; it
    ///   must read as one continuous sweep over whatever it crosses, and a
    ///   translucent sweep over a program's own banner would be two colours
    ///   mixing into a third that means nothing.
    /// - **The search grounds**, both of them — the matches and the current one.
    ///   Same argument, plus a second: the pair is only legible as a *pair* if
    ///   the difference between them survives what they are drawn over.
    /// - **The caret.** A cursor is the one thing on screen that must never be
    ///   ambiguous, and it is a block of ink by design.
    /// - **The math block's hover dim, its failure marker and its overflow
    ///   fades**, which decorate a raster the block itself put down rather than
    ///   the window — a fill on a picture, not a picture's paper.
    /// - **Box-drawing geometry and underlines**, which are foreground: they are
    ///   drawn in the cell's *ink* colour and are glyphs by another means.
    ///
    /// The seat's status band and the math toolbar are outside this function and
    /// stay where they are, for the same reason as the last two: chrome the app
    /// draws inside a pane is a mark on it.
    ///
    /// Red gate: hand the cell backgrounds back to the ink list and a 30% window
    /// shows an opaque banner again; hand the selection to the grounds and a
    /// selection over glass goes half-transparent.
    fn rectangles(
        &self,
        frame: &ViewportFrame,
        drawn_math_blocks: &HashSet<usize>,
        seat_focused: bool,
        ground_alpha: f32,
    ) -> SeatRects {
        let columns = frame.columns.get() as usize;
        let drawable_rows = frame.drawable_rows();
        let mut grounds = Vec::new();
        let mut rects = Vec::new();
        for (index, cell) in frame
            .cells
            .iter()
            .take(drawable_rows.saturating_mul(columns))
            .enumerate()
        {
            let (_, background) = resolve_colors(&cell.style);
            if background != default_background() {
                grounds.push(premultiplied_by_ground(
                    self.cell_rect(frame, index / columns, index % columns, background),
                    ground_alpha,
                ));
            }
        }
        // Read once per frame from the same atomic word the background comes
        // from, so a theme switch cannot leave a live selection wearing the
        // previous canvas's fill.
        let selection_background = selection_background_rgb();
        for span in &frame.selection_spans {
            let start = span.start_column.min(frame.columns.get()) as usize;
            let end = span.end_column.min(frame.columns.get()) as usize;
            if end > start && (span.row as usize) < drawable_rows {
                rects.push(self.cell_rect_span(
                    frame,
                    span,
                    start,
                    end - start,
                    selection_background,
                ));
            }
        }
        // The search's two grounds, over the selection and under everything else
        // — `mark.srch` is an inline element inside the text, so a hit inside a
        // selection reads as a hit rather than disappearing into it.
        //
        // **Rounded, unlike the selection** (D-16, mock 1530-1532's `3px`): a
        // selection is a continuous sweep the reader dragged and a hit is a word
        // the machine found, and the corner is the whole of what says so. One
        // rounded box per *span*, not per cell, which is why the run coalescing
        // happens in the projection — `rounded_rect_coverage` on a five-cell run
        // rounds the run's own two ends, and rounding every cell would draw four
        // beads with pinched joins between them.
        let match_radius =
            (SEARCH_MATCH_RADIUS_LOGICAL_PX * self.metrics.scale_factor as f32).max(0.0);
        for (spans, ground) in [
            (&frame.search_spans, search_match_rgb()),
            (&frame.current_search_spans, search_current_rgb()),
        ] {
            for span in spans {
                let start = span.start_column.min(frame.columns.get()) as usize;
                let end = span.end_column.min(frame.columns.get()) as usize;
                if end <= start || (span.row as usize) >= drawable_rows {
                    continue;
                }
                let bounds =
                    selection_span_bounds_px(self.metrics, frame, span, start, end - start);
                rects.extend(rounded_rect_coverage(bounds, match_radius).into_iter().map(
                    |entry| {
                        self.pixel_rect_with_coverage(
                            entry.rect[0],
                            entry.rect[1],
                            entry.rect[2],
                            entry.rect[3],
                            ground,
                            entry.coverage,
                        )
                    },
                ));
            }
        }
        for (index, placement) in frame.math_blocks.iter().enumerate() {
            if math_block_dim_is_drawn(placement, drawn_math_blocks.contains(&index))
                && let Some(geometry) = self.math_block_geometry(frame, placement)
            {
                rects.push(self.pixel_rect_with_coverage(
                    geometry.block[0],
                    geometry.block[1],
                    geometry.block[2],
                    geometry.block[3],
                    DEFAULT_STATUS_BACKGROUND_RGB,
                    0.45,
                ));
            }
        }
        for failure in &frame.math_failures {
            if let Some((marker, _)) = self.math_failure_geometry(frame, failure) {
                rects.push(self.pixel_rect_with_coverage(
                    marker[0],
                    marker[1],
                    marker[2],
                    marker[3],
                    DEFAULT_DIM_FOREGROUND_RGB,
                    0.65,
                ));
            }
        }
        if let Some(caret) = seat_caret(
            self.metrics,
            frame,
            seat_focused,
            self.cursor_blink_visible,
            current_cursor_style(),
        ) {
            rects.extend(caret.bounds.into_iter().map(|[left, top, right, bottom]| {
                self.pixel_rect(left, top, right, bottom, caret.ink)
            }));
        }
        for (index, cell) in frame
            .cells
            .iter()
            .take(drawable_rows.saturating_mul(columns))
            .enumerate()
        {
            if cell.style.flags.contains(CellFlags::HIDDEN) {
                continue;
            }
            let mut characters = cell.text.chars();
            let Some(character) = characters.next() else {
                continue;
            };
            if characters.next().is_some() {
                continue;
            }
            let row = index / columns;
            let column = index % columns;
            let [left, top, right, bottom] = frame_cell_bounds_px(self.metrics, frame, row, column);
            let Some(geometry) = procedural::geometry(
                character,
                left,
                top,
                right - left,
                bottom - top,
                // Box-drawing and block geometry is authored against the
                // renderer's default face size, so the scale it wants is "how
                // much bigger than that is this cell's face" — which is the
                // physical size over the default *logical* one, exactly as it
                // was before the size became a setting. A DPI change and a Font
                // size change both move `font_size_px`, and both should thicken
                // these rules by the same factor.
                self.metrics.font_size_px / DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX,
            ) else {
                continue;
            };
            let (foreground, _) = resolve_colors(&cell.style);
            rects.extend(geometry.into_iter().map(|rect| {
                self.pixel_rect_with_coverage(
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    foreground,
                    rect.coverage,
                )
            }));
        }
        let drawable_cells = drawable_rows.saturating_mul(columns);
        let mut index = 0;
        while index < drawable_cells {
            let cell = &frame.cells[index];
            if cell.style.flags.contains(CellFlags::UNDERLINE) {
                let row = index / columns;
                let column = index % columns;
                let [left, _, right, bottom] =
                    frame_cell_bounds_px(self.metrics, frame, row, column);
                let (foreground, _) = resolve_colors(&cell.style);
                rects.push(self.pixel_rect(
                    left,
                    bottom - self.metrics.scale_factor as f32,
                    right,
                    bottom,
                    foreground,
                ));
                index += 1;
                continue;
            }
            if !cell.style.flags.contains(CellFlags::DOTTED_UNDERLINE) {
                index += 1;
                continue;
            }

            let row = index / columns;
            let start_column = index % columns;
            let (foreground, _) = resolve_colors(&cell.style);
            let end = dotted_underline_run_end(&frame.cells, index, columns, drawable_cells);

            let [left, _, _, bottom] = frame_cell_bounds_px(self.metrics, frame, row, start_column);
            let right = left + (end - index) as f32 * self.metrics.cell_width_px;
            rects.extend(
                dotted_underline_segments(left, right, bottom, self.metrics.scale_factor)
                    .into_iter()
                    .map(|[segment_left, top, segment_right, segment_bottom]| {
                        self.pixel_rect(
                            segment_left,
                            top,
                            segment_right,
                            segment_bottom,
                            foreground,
                        )
                    }),
            );
            index = end;
        }
        SeatRects {
            grounds,
            ink: rects,
        }
    }

    /// A display formula is never wrapped, so one too wide for its band is cut off at the pane
    /// edge with nothing to say it continues. These fades are that signal: the band dissolves
    /// into the background on whichever side still holds content. Because each side is
    /// independent they double as a position readout — both lit is the middle of a long
    /// formula, one lit is an end, neither is a formula that fits.
    ///
    /// Drawn per block rather than as a corner caption because several formulas can share a
    /// screen, and a cue that marks the exact edge where content continues has to sit on that
    /// edge. The pane's one status line is already spoken for by scroll state and render
    /// failures, and could not name which of several blocks it meant.
    fn math_overflow_fade_rectangles(
        &self,
        placement: &MathBlockPlacement,
        geometry: &MathBlockGeometry,
    ) -> Vec<RectInstance> {
        math_overflow_fade_slabs(self.metrics, placement, geometry)
            .into_iter()
            .map(|(rect, coverage)| {
                self.pixel_rect_with_coverage(
                    rect[0],
                    rect[1],
                    rect[2],
                    rect[3],
                    background_rgb(),
                    coverage,
                )
            })
            .collect()
    }

    fn math_overlay_rectangles(&self, frame: &ViewportFrame) -> Vec<RectInstance> {
        let mut rects = Vec::new();
        let ink = foreground_rgb();
        let unit = self.metrics.scale_factor as f32;
        for placement in &frame.math_blocks {
            let Some(geometry) = self.math_block_geometry(frame, placement) else {
                continue;
            };
            // Before the toolbar, so the buttons stay crisp on top of it.
            rects.extend(self.math_overflow_fade_rectangles(placement, &geometry));
            let (Some(eye), Some(copy)) = (geometry.eye, geometry.copy) else {
                continue;
            };
            for button in [eye, copy] {
                rects.push(self.pixel_rect(
                    button[0],
                    button[1],
                    button[2],
                    button[3],
                    DEFAULT_STATUS_BACKGROUND_RGB,
                ));
            }
            let eye_mid_x = (eye[0] + eye[2]) / 2.0;
            let eye_mid_y = (eye[1] + eye[3]) / 2.0;
            let eye_half_w = (eye[2] - eye[0]) * 0.29;
            let eye_half_h = (eye[3] - eye[1]) * 0.18;
            rects.extend([
                self.pixel_rect(
                    eye_mid_x - eye_half_w,
                    eye_mid_y - eye_half_h,
                    eye_mid_x + eye_half_w,
                    eye_mid_y - eye_half_h + unit,
                    ink,
                ),
                self.pixel_rect(
                    eye_mid_x - eye_half_w,
                    eye_mid_y + eye_half_h - unit,
                    eye_mid_x + eye_half_w,
                    eye_mid_y + eye_half_h,
                    ink,
                ),
                self.pixel_rect(
                    eye_mid_x - unit,
                    eye_mid_y - unit,
                    eye_mid_x + unit,
                    eye_mid_y + unit,
                    ink,
                ),
            ]);
            let copy_inset = (copy[2] - copy[0]) * 0.27;
            let first = [
                copy[0] + copy_inset - 2.0 * unit,
                copy[1] + copy_inset - 2.0 * unit,
                copy[2] - copy_inset,
                copy[3] - copy_inset,
            ];
            let second = [
                copy[0] + copy_inset,
                copy[1] + copy_inset,
                copy[2] - copy_inset + 2.0 * unit,
                copy[3] - copy_inset + 2.0 * unit,
            ];
            for outline in [first, second] {
                rects.extend([
                    self.pixel_rect(outline[0], outline[1], outline[2], outline[1] + unit, ink),
                    self.pixel_rect(outline[0], outline[3] - unit, outline[2], outline[3], ink),
                    self.pixel_rect(outline[0], outline[1], outline[0] + unit, outline[3], ink),
                    self.pixel_rect(outline[2] - unit, outline[1], outline[2], outline[3], ink),
                ]);
            }
        }
        rects
    }

    fn cell_rect(
        &self,
        frame: &ViewportFrame,
        row: usize,
        column: usize,
        color: [u8; 3],
    ) -> RectInstance {
        let [left, top, right, bottom] = frame_cell_bounds_px(self.metrics, frame, row, column);
        self.pixel_rect(left, top, right, bottom, color)
    }

    fn cell_rect_span(
        &self,
        frame: &ViewportFrame,
        selection: &SelectionSpan,
        column: usize,
        span: usize,
        color: [u8; 3],
    ) -> RectInstance {
        let [left, top, right, bottom] =
            selection_span_bounds_px(self.metrics, frame, selection, column, span);
        self.pixel_rect(left, top, right, bottom, color)
    }

    fn pixel_rect(
        &self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: [u8; 3],
    ) -> RectInstance {
        self.pixel_rect_with_coverage(left, top, right, bottom, color, 1.0)
    }

    #[allow(clippy::too_many_arguments)]
    fn pixel_rect_with_coverage(
        &self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: [u8; 3],
        coverage: f32,
    ) -> RectInstance {
        let width = self.seat.width.max(1) as f32;
        let height = self.seat.height.max(1) as f32;
        RectInstance {
            rect: [
                left / width * 2.0 - 1.0,
                1.0 - top / height * 2.0,
                right / width * 2.0 - 1.0,
                1.0 - bottom / height * 2.0,
            ],
            color: rect_gpu_color_with_coverage(color, coverage),
        }
    }

    /// Draw one frame's grid text into this window's attachment and report what
    /// the CPU spent doing it — the `bt-replay` instrumentation path.
    ///
    /// Every phase here is the production path: the same [`prepare_text_rows`]
    /// against the same composed-row and shaping caches, the same glyphon
    /// prepare into the same shared atlas, the same math textures through the
    /// same device LRU. What it leaves out is everything a window draws *around*
    /// the grid — seat chrome, the modal overlay, the peek flyout, a preview's
    /// picture — because a replay has none of them and timing an empty list is
    /// timing nothing.
    ///
    /// Written against [`Self::acquire`] rather than against a texture, so a
    /// probe over a swapchain would present a real frame instead of taking a
    /// branch no window can reach.
    ///
    /// The pair invariant of [`Self::present_frame`] holds here for the same
    /// reason and by the same construction: the prepares and the render are
    /// inside one call.
    fn probe_frame(
        &mut self,
        gpu: &mut GpuContext,
        frame: &ViewportFrame,
    ) -> Result<RenderProbeSample, RenderError> {
        let started = Instant::now();
        frame.validate_shape()?;
        let (width, height) = (self.config.width, self.config.height);
        self.seat_slots[0]
            .viewport
            .update(&gpu.queue, Resolution { width, height });
        let text_stats = self.prepare_text_rows(gpu, frame)?;
        let rows_prepared_at = Instant::now();
        {
            let slot = &mut self.seat_slots[0];
            prepare_text_atlas(
                &mut slot.text_renderer,
                &gpu.device,
                &gpu.queue,
                &mut gpu.font_system,
                &mut gpu.atlas,
                &slot.viewport,
                &mut gpu.swash_cache,
                &self.text_rows,
                self.metrics,
                frame,
            )
            .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            prepare_status_text_atlas(
                &mut slot.status_text_renderer,
                &gpu.device,
                &gpu.queue,
                &mut gpu.font_system,
                &mut gpu.atlas,
                &slot.viewport,
                &mut gpu.swash_cache,
                self.status_overlay.as_deref(),
                self.metrics,
                frame,
                // A headless probe has no seat: it renders into the whole target, so the surface is
                // the pane.
                width as f32,
            )
            .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
        }
        let atlas_prepared_at = Instant::now();
        let math_evictions_before = gpu.math_texture_evictions;
        let (math_texture_uploads, math_texture_upload_bytes) =
            self.probe_math_textures(gpu, frame);
        let math_prepared_at = Instant::now();
        let (acquired, view) = match self.acquire() {
            SurfaceAcquisition::Frame(texture) | SurfaceAcquisition::Suboptimal(texture) => {
                let view = texture.texture.create_view(&Default::default());
                (AcquiredFrame::Swapchain(texture), view)
            }
            SurfaceAcquisition::Offscreen(view) => (AcquiredFrame::Offscreen, view),
            SurfaceAcquisition::Failed(failure) => {
                return Err(RenderError::Wgpu(format!(
                    "probe could not acquire a frame: {failure:?}"
                )));
            }
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Folio replay probe frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Folio replay probe pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(theme_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let slot = &self.seat_slots[0];
            slot.text_renderer
                .render(&gpu.atlas, &slot.viewport, &mut pass)
                .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            if self.status_overlay.is_some() {
                slot.status_text_renderer
                    .render(&gpu.atlas, &slot.viewport, &mut pass)
                    .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            }
        }
        gpu.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        match acquired {
            AcquiredFrame::Swapchain(texture) => gpu.queue.present(texture),
            AcquiredFrame::Offscreen => {}
        }
        gpu.atlas.trim();
        Ok(RenderProbeSample {
            total: started.elapsed(),
            row_compose: text_stats.elapsed,
            shape_cache_miss: text_stats.narrow.miss_time + text_stats.wide.miss_time,
            atlas_prepare_upload: atlas_prepared_at - rows_prepared_at,
            encode_submit: submitted_at - math_prepared_at,
            math_prepare_upload: math_prepared_at - atlas_prepared_at,
            rows_reshaped: text_stats.rows_reshaped,
            row_cache_hits: text_stats.row_cache.hits,
            row_cache_misses: text_stats.row_cache.misses,
            row_cache_evictions: text_stats.row_cache.evictions,
            row_cache_resident_bytes: text_stats.row_cache.resident_bytes,
            narrow_hits: text_stats.narrow.hits,
            narrow_misses: text_stats.narrow.misses,
            narrow_evictions: text_stats.narrow.evictions,
            wide_hits: text_stats.wide.hits,
            wide_misses: text_stats.wide.misses,
            wide_evictions: text_stats.wide.evictions,
            narrow_resident_bytes: text_stats.narrow.resident_bytes,
            wide_resident_bytes: text_stats.wide.resident_bytes,
            math_texture_uploads,
            math_texture_upload_bytes,
            math_texture_evictions: gpu
                .math_texture_evictions
                .saturating_sub(math_evictions_before),
            math_texture_resident_bytes: gpu.math_textures.resident_bytes(),
            atlas_hits: None,
            atlas_misses: None,
            atlas_grows: None,
            atlas_evictions: None,
            atlas_upload_bytes: None,
            narrow_glyphs: self
                .text_rows
                .iter()
                .map(|row| row.narrow_glyphs.len() as u64)
                .sum(),
            wide_glyphs: self
                .text_rows
                .iter()
                .map(|row| row.wide_glyphs.len() as u64)
                .sum(),
        })
    }

    /// Upload every math raster this frame would draw, and count what that cost.
    ///
    /// [`Self::prepare_math_draws`]'s upload half without its geometry half: the
    /// probe has no pass to place quads in, and the number it is after is the
    /// device traffic.
    fn probe_math_textures(&mut self, gpu: &mut GpuContext, frame: &ViewportFrame) -> (u64, usize) {
        let mut uploads = 0_u64;
        let mut upload_bytes = 0_usize;
        for placement in &frame.math_blocks {
            if !math_block_admits_texture(frame, placement) {
                continue;
            }
            let key = &placement.artifact.key;
            if gpu.math_textures.get(key).is_some() {
                continue;
            }
            let Some(texture) = gpu.upload_math_texture(&placement.artifact) else {
                continue;
            };
            uploads = uploads.saturating_add(1);
            upload_bytes = upload_bytes.saturating_add(placement.artifact.rgba.len());
            let (_, evictions) =
                gpu.math_textures
                    .insert(key.clone(), texture, placement.artifact.rgba.len());
            gpu.math_texture_evictions = gpu.math_texture_evictions.saturating_add(evictions);
        }
        (uploads, upload_bytes)
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_text_rows(
    frame: &ViewportFrame,
    metrics: CellMetrics,
    text_rows: &mut Vec<Arc<ComposedRow>>,
    status_overlay: &mut Option<Arc<ComposedRow>>,
    composed_row_cache: &mut ComposedRowCache,
    font_revision: u64,
    theme_revision: u64,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    narrow_shaping_cache: &mut NarrowShapingCache,
    wide_shaping_cache: &mut WideShapingCache,
) -> Result<TextPreparationStats, RenderError> {
    let started = Instant::now();
    let narrow_before = narrow_shaping_cache.counters;
    let wide_before = wide_shaping_cache.counters;
    let row_before = composed_row_cache.counters;
    let mut rows_reshaped = 0_u64;
    let source_rows = text_row_cells(frame)?;
    let rows = frame.drawable_rows();
    let mut next_rows = Vec::with_capacity(rows);

    for source_cells in source_rows.take(rows) {
        // Row placement is intentionally absent from this key. Cached rows own shaping only;
        // `prepare_text_atlas` remaps the same Arc through the presented frame's live prefix map.
        let key = ComposedRowKey {
            cells: source_cells.to_vec(),
            metrics: metrics.into(),
            font_revision,
            theme_revision,
            status_overlay: None,
        };
        if let Some(row) = composed_row_cache.get(&key) {
            next_rows.push(row);
            continue;
        }
        rows_reshaped = rows_reshaped.saturating_add(1);

        let narrow_glyphs = shape_narrow_glyphs(
            source_cells,
            font_system,
            swash_cache,
            metrics,
            narrow_shaping_cache,
        );
        let wide_glyphs = shape_wide_glyphs(
            source_cells,
            font_system,
            swash_cache,
            metrics,
            wide_shaping_cache,
        );
        let row = Arc::new(ComposedRow {
            narrow_glyphs,
            wide_glyphs,
        });
        composed_row_cache.insert(key, Arc::clone(&row));
        next_rows.push(row);
    }
    if let Some(status) = frame.status_text.as_deref() {
        let cells = status_overlay_cells(frame.columns.get() as usize, status);
        let key = ComposedRowKey {
            cells: cells.clone(),
            metrics: metrics.into(),
            font_revision,
            theme_revision,
            status_overlay: Some(status.to_owned()),
        };
        if let Some(row) = composed_row_cache.get(&key) {
            *status_overlay = Some(row);
        } else {
            rows_reshaped = rows_reshaped.saturating_add(1);
            let row = Arc::new(ComposedRow {
                narrow_glyphs: shape_narrow_glyphs(
                    &cells,
                    font_system,
                    swash_cache,
                    metrics,
                    narrow_shaping_cache,
                ),
                wide_glyphs: shape_wide_glyphs(
                    &cells,
                    font_system,
                    swash_cache,
                    metrics,
                    wide_shaping_cache,
                ),
            });
            composed_row_cache.insert(key, Arc::clone(&row));
            *status_overlay = Some(row);
        }
    } else {
        *status_overlay = None;
    }
    *text_rows = next_rows;
    Ok(TextPreparationStats {
        elapsed: started.elapsed(),
        rows_reshaped,
        row_cache: RowCacheCounters {
            hits: composed_row_cache
                .counters
                .hits
                .saturating_sub(row_before.hits),
            misses: composed_row_cache
                .counters
                .misses
                .saturating_sub(row_before.misses),
            evictions: composed_row_cache
                .counters
                .evictions
                .saturating_sub(row_before.evictions),
            resident_bytes: composed_row_cache.counters.resident_bytes,
        },
        narrow: narrow_shaping_cache.counters.delta_since(narrow_before),
        wide: wide_shaping_cache.counters.delta_since(wide_before),
    })
}

/// A chrome rectangle in whole-surface pixels.
///
/// Deliberately a free function rather than a `WindowRenderer` method: the seat-local
/// [`WindowRenderer::pixel_rect`] and this one differ in exactly which rectangle they
/// call "the world", and having them side by side as one method with a flag is
/// how the two would eventually be confused for each other.
/// Crop a rectangle to a clip, or `None` when nothing of it survives.
///
/// **The one gate between a laid-out rectangle and the GPU** (user report,
/// 2026-08-13). The pass below runs in whole-surface coordinates with no
/// scissor, so a rectangle that arrives inverted (`right < left`) or carrying a
/// `NaN` is not cropped by anything downstream — it is mapped straight into
/// normalised device coordinates, where an inverted box is a box the rasteriser
/// happily fills across the whole surface and a `NaN` is a box with no edges at
/// all. Either one comes out as bands of colour lying over panes that have
/// nothing to do with the preview, which is exactly what was reported against a
/// document holding a 250-character fence line.
///
/// A layout that produces such a rectangle is wrong and this does not make it
/// right. What it does is keep the consequence inside the pane that owns it: a
/// preview that draws nothing is a bug you can see and reason about, and a
/// preview that draws over the file tree is a bug that looks like the renderer's.
pub fn crop_to(rect: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    if !rect.iter().all(|value| value.is_finite()) {
        return None;
    }
    let cropped = [
        rect[0].max(clip[0]),
        rect[1].max(clip[1]),
        rect[2].min(clip[2]),
        rect[3].min(clip[3]),
    ];
    (cropped[2] > cropped[0] && cropped[3] > cropped[1]).then_some(cropped)
}

fn surface_pixel_rect(rect: [f32; 4], color: [u8; 3], width: u32, height: u32) -> RectInstance {
    surface_pixel_rect_with_alpha(rect, color, 1.0, width, height)
}

/// The same rectangle as the window's **ground** at that place: premultiplied in
/// linear light by the ground's alpha, for a pipeline that blends `Replace`.
///
/// This is [`ground::premultiplied_clear`] with a rectangle around it, and it
/// has to be the same arithmetic for the same reason the clear linearises: the
/// surface is sRGB and premultiplied, so a band that is to sit flush with the
/// clear must be encoded on the same side of that encode. A straight-alpha
/// source here is the bug that reads as a ground band brighter than the ground
/// beside it.
fn premultiplied_surface_pixel_rect(
    rect: [f32; 4],
    color: [u8; 3],
    alpha: f32,
    width: u32,
    height: u32,
) -> RectInstance {
    premultiplied_by_ground(
        surface_pixel_rect_with_alpha(rect, color, 1.0, width, height),
        alpha,
    )
}

/// The same encoding, applied to a rectangle whose geometry is already settled:
/// `(A·colour, A)` in linear light, which is what a `Replace` pipeline onto a
/// premultiplied surface requires and what [`ground::premultiplied_clear`]
/// writes.
///
/// Split out from [`premultiplied_surface_pixel_rect`] because the grid's own
/// grounds are laid out in **seat-local** pixels (`pixel_rect`) rather than
/// whole-surface ones, and the encoding must be the one arithmetic whichever
/// coordinate space the rectangle came out of. A second copy of it in the seat
/// lane is exactly how a program's banner would end up a shade off the pane it
/// is printed into.
fn premultiplied_by_ground(mut instance: RectInstance, alpha: f32) -> RectInstance {
    let alpha = alpha.clamp(0.0, 1.0);
    for channel in &mut instance.color[..3] {
        *channel *= alpha;
    }
    instance.color[3] = alpha;
    instance
}

/// The same rectangle, blended rather than laid down opaque — what the modal
/// overlay draws with, because a scrim, a hairline over an unknown surface and a
/// rounded corner's coverage are all statements about *how much* of a colour
/// lands, and none of them can be pre-composited the way seat chrome's are.
fn surface_pixel_rect_with_alpha(
    rect: [f32; 4],
    color: [u8; 3],
    alpha: f32,
    width: u32,
    height: u32,
) -> RectInstance {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    RectInstance {
        rect: [
            rect[0] / w * 2.0 - 1.0,
            1.0 - rect[1] / h * 2.0,
            rect[2] / w * 2.0 - 1.0,
            1.0 - rect[3] / h * 2.0,
        ],
        color: rect_gpu_color_with_coverage(color, alpha),
    }
}

/// A shaped chrome label and where it goes, in whole-surface pixels.
struct ChromeTextLayout {
    buffer: Buffer,
    left: f32,
    top: f32,
    bounds: TextBounds,
    color: Color,
}

/// The shaping attributes one chrome label is set with.
///
/// **One builder, read by the measurer and by the shaper both.** A width and the
/// ink it is supposed to hold are the same shaping run asked twice, and every
/// attribute that changes an advance — the weight axis, letter spacing, tabular
/// figures — has to be on both runs or the box is measured for a text that is
/// not the one drawn. It was not: `measure_chrome_text` set neither the weight
/// nor the tracking, so the float's `DOCK` was measured as regular-weight
/// untracked text and drawn as semibold at `.04em`, and the last letter was
/// clipped by the box its own caption had been used to size.
fn chrome_label_attrs(
    weight: ChromeLabelWeight,
    letter_spacing_em: f32,
    tabular_numerals: bool,
    mono: bool,
) -> Attrs<'static> {
    let mut attrs = Attrs::new()
        .family(if mono {
            Family::Monospace
        } else {
            Family::SansSerif
        })
        .weight(weight.shaping_weight());
    if tabular_numerals {
        let mut features = FontFeatures::new();
        features.enable(TABULAR_FIGURES);
        attrs = attrs.font_features(features);
    }
    if letter_spacing_em != 0.0 {
        attrs = attrs.letter_spacing(letter_spacing_em);
    }
    attrs
}

/// How wide a chrome label's text will be, in physical pixels.
///
/// Shaped through the same face, the same size and the same shaper
/// [`shape_chrome_labels`] uses, because a width arrived at any other way is a
/// second opinion about the same font — and a box sized from a second opinion is
/// a box the ink does not fit.
///
/// The caller of the hour is the tab's pane-count badge, which the mock-up sizes
/// as `max(min-width, text + padding)` (`.panecount`, lines 292-304): the badge
/// cannot know its own width without knowing the number's.
fn measure_chrome_label(
    font_system: &mut FontSystem,
    text: &str,
    font_size_px: f32,
    weight: ChromeLabelWeight,
    letter_spacing_em: f32,
    tabular_numerals: bool,
    mono: bool,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let line_height = font_size_px * 1.4;
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size_px, line_height));
    buffer.set_wrap(Wrap::None);
    // No width bound at all: this asks what the text *wants*, not what it would
    // be squeezed into.
    buffer.set_size(None, Some(line_height));
    buffer.set_text(
        text,
        // **Tabular figures are a parameter, and they were not** (user report,
        // 2026-08-17). The note that stood here said they could only ever make a
        // string narrower, so measuring without them was safe. It is the wrong
        // way round: `tnum` gives *every* digit the widest digit's advance, so a
        // string carrying a narrow one — `1h`, `Aug 10`, a short hash with a `1`
        // in it — shapes **wider** than the same string measured proportionally.
        // A right-aligned label whose box was cut from the narrow number then
        // has its `left` clamped to that box's left edge by `shape_chrome_labels`
        // and runs off the right of it, where the clip cuts the last glyph in
        // half. That is what the Git page's meta column was doing at every width.
        &chrome_label_attrs(weight, letter_spacing_em, tabular_numerals, mono),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max)
}

/// Shape every chrome label. The buffers are owned by the returned vector so
/// they outlive the `prepare` that borrows them.
/// Shape one batch of chrome text.
///
/// `alpha` is the batch's uniform opacity — the layer's own, for overlay text,
/// and `1.0` for the seat chrome, which never fades. It rides here rather than on
/// [`ChromeLabel`] because it is a property of the *group* a label was drawn in
/// and not of the label: a caption does not decide how faded the popup carrying
/// it is, and a per-label field would ask all thirty-odd construction sites to
/// answer a question only their layer can.
fn shape_chrome_labels(
    font_system: &mut FontSystem,
    labels: &[ChromeLabel],
    cap_height_ratio: f32,
    alpha: f32,
) -> Vec<ChromeTextLayout> {
    labels
        .iter()
        .filter(|label| !label.text.is_empty() && label.rect[2] > label.rect[0])
        .map(|label| {
            let width = label.rect[2] - label.rect[0];
            let line_height = label.font_size_px * 1.4;
            let mut buffer =
                Buffer::new(font_system, Metrics::new(label.font_size_px, line_height));
            buffer.set_wrap(Wrap::None);
            buffer.set_size(Some(width), Some(line_height));
            let attrs = chrome_label_attrs(
                label.weight,
                label.letter_spacing_em,
                label.tabular_numerals,
                label.mono,
            );
            buffer.set_text(&label.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(font_system, false);
            let text_width = buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0_f32, f32::max);
            let left = if label.align_center {
                (label.rect[0] + (width - text_width) / 2.0).max(label.rect[0])
            } else if label.align_right {
                (label.rect[2] - text_width).max(label.rect[0])
            } else {
                label.rect[0]
            };
            // The axis `.tab { align-items: center }` and `.panehead { align-items:
            // center }` put every item of the row on. A mark box is centred on it
            // by `seats.rs`, and what the eye pairs with that mark is the cap band
            // — cap line down to baseline — because that is the part of a title
            // that has ink in it at every letter.
            //
            // Centring the *line box* instead (all cosmic-text's own half-leading
            // can do, since it centres the face's ascent+descent box) hands the
            // band whatever asymmetry the face reserves for accents and tails: the
            // chrome's own face puts the cap band 0.09em above the line box's
            // centre, which is ~1.75 physical pixels at 200% — a visible step
            // between a mark and the word beside it.
            //
            // The band is derived from the face's cap height and the rect, never
            // from the label's own ink, so a title that changes text does not move:
            // "Preview" and a filename with a descender share one baseline.
            let axis = (label.rect[1] + label.rect[3]) / 2.0;
            let baseline = axis + cap_height_ratio * label.font_size_px / 2.0;
            let baseline_in_buffer = buffer
                .layout_runs()
                .next()
                .map(|run| run.line_y)
                .expect("a non-empty chrome label shapes into at least one run");
            let [r, g, b] = label.color;
            let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
            // The layout above read `rect`; only the *bounds* read `clip`, and
            // they fall back to `rect` when nothing asked for a narrower one —
            // so a label with no clip is shaped, placed and cropped with exactly
            // the four numbers it was cropped with before the field existed.
            let clip = label.clip.unwrap_or(label.rect);
            ChromeTextLayout {
                buffer,
                left,
                top: baseline - baseline_in_buffer,
                bounds: TextBounds {
                    left: clip[0].floor() as i32,
                    top: clip[1].floor() as i32,
                    right: clip[2].ceil() as i32,
                    bottom: clip[3].ceil() as i32,
                },
                color: Color::rgba(r, g, b, a),
            }
        })
        .collect()
}

/// The shaping attributes one preview run is set with.
///
/// **The face is the difference from chrome text.** Chrome is the window talking
/// about itself and is set in the sans face; a preview body is a document, and a
/// document's own bytes were written in a grid. `Family::Monospace` resolves to
/// the same family the terminal grid is set in, so the preview of a source file
/// and the terminal beside it agree about how wide a character is — the whole of
/// the mock-up's `font: 12.5px/1.5 Consolas, "Cascadia Mono", monospace`.
fn preview_run_attrs(mono: bool, bold: bool, letter_spacing_em: f32) -> Attrs<'static> {
    let mut attrs = Attrs::new()
        .family(if mono {
            Family::Monospace
        } else {
            Family::SansSerif
        })
        .weight(if bold {
            Weight::SEMIBOLD
        } else {
            Weight::NORMAL
        });
    if letter_spacing_em != 0.0 {
        attrs = attrs.letter_spacing(letter_spacing_em);
    }
    attrs
}

/// Fill a buffer with one paragraph's styled runs.
///
/// One buffer per paragraph and not per run, which is what lets the shaper wrap
/// a mixed line: `a **bold** word` may break between any two of its words, and
/// three buffers butted together could only ever break between the three.
fn set_preview_runs(
    buffer: &mut Buffer,
    runs: &[PreviewRun],
    letter_spacing_em: f32,
    metrics: Metrics,
) {
    let default = preview_run_attrs(false, false, letter_spacing_em);
    buffer.set_rich_text(
        runs.iter().map(|run| {
            let [r, g, b] = run.color;
            let mut attrs = preview_run_attrs(run.mono, run.bold, letter_spacing_em)
                .color(Color::rgba(r, g, b, 255));
            // The size is the run's own; the *leading* stays the paragraph's,
            // so a line carrying a code span is exactly as tall as the line
            // above it. See [`PreviewRun::font_scale`].
            if run.font_scale != 1.0 {
                attrs = attrs.metrics(Metrics::new(
                    (metrics.font_size * run.font_scale).max(1.0),
                    metrics.line_height,
                ));
            }
            (run.text.as_str(), attrs)
        }),
        &default,
        Shaping::Advanced,
        None,
    );
}

/// The x the pen starts at for a shaped paragraph.
///
/// A free function because two callers need the *same* answer and for the same
/// reason the hit test and the paint of every other surface share their
/// geometry: [`shape_preview_body`] draws from it and
/// [`WindowRenderer::measure_preview_run_boxes`] reports boxes against it, and a
/// centred caption whose runs were measured from its left edge would hand back
/// boxes half a paragraph away from its own glyphs.
fn preview_paragraph_left(paragraph: &PreviewParagraph, buffer: &Buffer) -> f32 {
    if !paragraph.align_center && !paragraph.align_right {
        return paragraph.rect[0];
    }
    let width = (paragraph.rect[2] - paragraph.rect[0]).max(1.0);
    let text_width = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max);
    if paragraph.align_center {
        (paragraph.rect[0] + (width - text_width) / 2.0).max(paragraph.rect[0])
    } else {
        (paragraph.rect[2] - text_width).max(paragraph.rect[0])
    }
}

/// Shape a preview body — one buffer per visible paragraph.
/// Every filled rectangle one preview body draws, already cropped.
///
/// A free function because two lanes ask for it now: the seat's, which runs
/// before the overlays, and an overlay layer's own
/// ([`OverlayLayer::body`]) — and a second copy of these croppings is a second
/// chance for a document in a float to be clipped differently from the same
/// document in a pane.
fn preview_body_rect_instances(
    body: &PreviewBody,
    surface_width: u32,
    surface_height: u32,
) -> Vec<RectInstance> {
    let mut rects: Vec<RectInstance> = body
        .quads
        .iter()
        .filter_map(|quad| {
            // Cropped to the body here rather than by a scissor, because the pass
            // is in whole-surface coordinates and a second scissor would have to
            // be set and unset around two draws that are otherwise one.
            let rect = crop_to(quad.rect, body.clip)?;
            Some(surface_pixel_rect(
                rect,
                quad.color,
                surface_width,
                surface_height,
            ))
        })
        .collect();
    // A scrolling block's fills are cropped to the block **and** to the body: the
    // offset moves them sideways out of their own rectangle, and the body's clip
    // still ends where the pane head begins.
    for block in &body.blocks {
        let Some(window) = crop_to(block.clip, body.clip) else {
            continue;
        };
        rects.extend(block.quads.iter().filter_map(|quad| {
            let rect = crop_to(quad.rect, window)?;
            Some(surface_pixel_rect(
                rect,
                quad.color,
                surface_width,
                surface_height,
            ))
        }));
    }
    rects
}

fn shape_preview_body(font_system: &mut FontSystem, body: &PreviewBody) -> Vec<ChromeTextLayout> {
    let content = body
        .paragraphs
        .iter()
        .map(|paragraph| (paragraph, body.clip));
    // A scrolling block's text is cropped to the block, so a table shifted
    // sideways is cut at its own edge rather than printing over the prose beside
    // it. Intersected with the body's clip for the reason the fills are.
    let blocks = body.blocks.iter().flat_map(|block| {
        let window = crop_to(block.clip, body.clip).unwrap_or(block.clip);
        block
            .paragraphs
            .iter()
            .map(move |paragraph| (paragraph, window))
    });
    content
        .chain(blocks)
        .filter(|(paragraph, _)| paragraph.runs.iter().any(|run| !run.text.is_empty()))
        // A paragraph wholly above or below its own clip is not drawn at all,
        // which is what keeps a 64KB file's cost proportional to the pane rather
        // than to the file — the same rule `push_files_tree` applies to its rows.
        .filter(|(paragraph, clip)| paragraph.rect[3] > clip[1] && paragraph.rect[1] < clip[3])
        // A paragraph whose own box does not survive the clip draws nothing, and
        // a paragraph whose box is inverted or carries a `NaN` must draw nothing
        // — see `crop_to`. A `TextBounds` built from such a box is not a crop at
        // all, and the glyphs land wherever the arithmetic put them.
        .filter_map(|(paragraph, clip)| crop_to(paragraph.rect, clip).map(|c| (paragraph, c)))
        .map(|(paragraph, clip)| {
            let width = (paragraph.rect[2] - paragraph.rect[0]).max(1.0);
            let mut buffer = Buffer::new(
                font_system,
                Metrics::new(paragraph.font_size_px, paragraph.line_height_px),
            );
            if paragraph.wrap {
                buffer.set_wrap(Wrap::WordOrGlyph);
                buffer.set_size(Some(width), None);
            } else {
                buffer.set_wrap(Wrap::None);
                buffer.set_size(None, Some(paragraph.line_height_px));
            }
            set_preview_runs(
                &mut buffer,
                &paragraph.runs,
                paragraph.letter_spacing_em,
                Metrics::new(paragraph.font_size_px, paragraph.line_height_px),
            );
            buffer.shape_until_scroll(font_system, false);
            let left = preview_paragraph_left(paragraph, &buffer);
            let [r, g, b] = paragraph.runs.first().map_or([0, 0, 0], |run| run.color);
            ChromeTextLayout {
                buffer,
                left,
                // The line box's top, not a cap-height axis: body text sits on
                // consecutive baselines a line height apart, which is what the
                // metrics already encode.
                top: paragraph.rect[1],
                bounds: TextBounds {
                    left: clip[0].floor() as i32,
                    top: clip[1].floor() as i32,
                    right: clip[2].ceil() as i32,
                    bottom: clip[3].ceil() as i32,
                },
                color: Color::rgba(r, g, b, 255),
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn prepare_chrome_text_atlas(
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    atlas: &mut TextAtlas,
    viewport: &Viewport,
    swash_cache: &mut SwashCache,
    layouts: &[ChromeTextLayout],
) -> Result<(), PrepareError> {
    text_renderer.prepare(
        device,
        queue,
        font_system,
        atlas,
        viewport,
        layouts.iter().map(|layout| TextArea {
            buffer: &layout.buffer,
            left: layout.left,
            top: layout.top,
            scale: 1.0,
            bounds: layout.bounds,
            default_color: layout.color,
            custom_glyphs: &[],
        }),
        swash_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_text_atlas(
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    atlas: &mut TextAtlas,
    viewport: &Viewport,
    swash_cache: &mut SwashCache,
    text_rows: &[Arc<ComposedRow>],
    metrics: CellMetrics,
    frame: &ViewportFrame,
) -> Result<(), PrepareError> {
    let padding = metrics.padding_px;
    let text_right = (padding + frame.columns.get() as f32 * metrics.cell_width_px).ceil() as i32;
    // **The current hit's ink, applied here and not in the shaping** (§7.1.5d).
    //
    // A composed row is cached on its cells, and its cells are what the shell
    // wrote; folding a search into that key would mean reshaping every visible
    // row on every keystroke of a query, for a change that is one colour deep.
    // So the recolour happens at the draw, exactly where the selection's ground
    // is decided, and both marks stay properties of the *frame* rather than of
    // the text. `--termbg` is the ruled ink (mock 1526-1529) and the ground
    // under it is the solid accent, which is the pair that contrasts on either
    // canvas.
    let current_ink = (!frame.current_search_spans.is_empty()).then(|| {
        let ink = search_current_ink_rgb();
        Color::rgb(ink[0], ink[1], ink[2])
    });
    let recoloured = |row: usize, column: usize, resting: Color| {
        current_ink
            .filter(|_| {
                frame.current_search_spans.iter().any(|span| {
                    span.row as usize == row
                        && (span.start_column as usize..span.end_column as usize).contains(&column)
                })
            })
            .unwrap_or(resting)
    };
    let narrow_text_areas = text_rows.iter().enumerate().flat_map(|(row, text_row)| {
        text_row.narrow_glyphs.iter().map(move |glyph| {
            let [left, top, _, bottom] = frame_cell_bounds_px(metrics, frame, row, glyph.column);
            TextArea {
                buffer: &glyph.buffer,
                left: left + glyph.left_offset_px,
                top: top + glyph.top_offset_px,
                scale: 1.0,
                // Clip to the terminal row, not the cell. The grid owns pen origins, while
                // accents and fallback ink remain free to overhang adjacent cells.
                bounds: TextBounds {
                    left: padding.floor() as i32,
                    top: top.floor() as i32,
                    right: text_right,
                    bottom: bottom.ceil() as i32,
                },
                default_color: recoloured(row, glyph.column, glyph.color),
                custom_glyphs: &[],
            }
        })
    });
    let wide_text_areas = text_rows.iter().enumerate().flat_map(|(row, text_row)| {
        text_row.wide_glyphs.iter().map(move |wide| {
            let [left, top, _, bottom] = frame_cell_bounds_px(metrics, frame, row, wide.column);
            TextArea {
                buffer: &wide.buffer,
                left: left + wide.left_offset_px,
                top: top + wide.top_offset_px,
                scale: 1.0,
                bounds: TextBounds {
                    left: left.floor() as i32,
                    top: top.floor() as i32,
                    right: (left + 2.0 * metrics.cell_width_px).ceil() as i32,
                    bottom: bottom.ceil() as i32,
                },
                default_color: recoloured(row, wide.column, wide.color),
                custom_glyphs: &[],
            }
        })
    });
    text_renderer.prepare(
        device,
        queue,
        font_system,
        atlas,
        viewport,
        narrow_text_areas.chain(wide_text_areas),
        swash_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare_status_text_atlas(
    text_renderer: &mut TextRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    font_system: &mut FontSystem,
    atlas: &mut TextAtlas,
    viewport: &Viewport,
    swash_cache: &mut SwashCache,
    status_overlay: Option<&ComposedRow>,
    metrics: CellMetrics,
    frame: &ViewportFrame,
    seat_width_px: f32,
) -> Result<(), PrepareError> {
    let Some(status) = frame.status_text.as_deref() else {
        return Ok(());
    };
    let Some(row) = status_overlay else {
        return Ok(());
    };
    let Some(geometry) = status_overlay_geometry(metrics, frame, status, seat_width_px) else {
        return Ok(());
    };
    let narrow_text_areas = row.narrow_glyphs.iter().map(|glyph| {
        let left = geometry.rect[0]
            + glyph.column.saturating_sub(geometry.first_column) as f32 * metrics.cell_width_px;
        TextArea {
            buffer: &glyph.buffer,
            left: left + glyph.left_offset_px,
            top: geometry.rect[1] + glyph.top_offset_px,
            scale: 1.0,
            bounds: TextBounds {
                left: geometry.rect[0].floor() as i32,
                top: geometry.rect[1].floor() as i32,
                right: geometry.rect[2].ceil() as i32,
                bottom: geometry.rect[3].ceil() as i32,
            },
            default_color: glyph.color,
            custom_glyphs: &[],
        }
    });
    let wide_text_areas = row.wide_glyphs.iter().map(|wide| {
        let left = geometry.rect[0]
            + wide.column.saturating_sub(geometry.first_column) as f32 * metrics.cell_width_px;
        TextArea {
            buffer: &wide.buffer,
            left: left + wide.left_offset_px,
            top: geometry.rect[1] + wide.top_offset_px,
            scale: 1.0,
            bounds: TextBounds {
                left: left.floor() as i32,
                top: geometry.rect[1].floor() as i32,
                right: (left + 2.0 * metrics.cell_width_px).ceil() as i32,
                bottom: geometry.rect[3].ceil() as i32,
            },
            default_color: wide.color,
            custom_glyphs: &[],
        }
    });
    text_renderer.prepare(
        device,
        queue,
        font_system,
        atlas,
        viewport,
        narrow_text_areas.chain(wide_text_areas),
        swash_cache,
    )
}

/// Validate the complete render frame before exposing exact presentation rows to text shaping.
///
/// This is the shared slice boundary used by `WindowRenderer::prepare_text_rows` and deterministic
/// resize replay tests. `chunks_exact` is only constructed after the presentation-rectangle proof;
/// phase-A drawing consumes its frame-owned drawable prefix and leaves the overscan suffix intact.
pub fn text_row_cells(
    frame: &ViewportFrame,
) -> Result<std::slice::ChunksExact<'_, CapturedCell>, FrameShapeError> {
    frame.validate_shape()?;
    Ok(frame.cells.chunks_exact(frame.columns.get() as usize))
}

fn status_overlay_cells(columns: usize, status: &str) -> Vec<CapturedCell> {
    let mut displayed = vec![CapturedCell::default(); columns];
    let characters = status.chars().collect::<Vec<_>>();
    let shown = characters.len().min(displayed.len());
    let start = displayed.len() - shown;
    for (cell, character) in displayed[start..]
        .iter_mut()
        .zip(characters[characters.len() - shown..].iter())
    {
        *cell = CapturedCell::plain(character.to_string());
    }
    displayed
}

/// Where the "N rows above" indicator sits, right-aligned against the *visible* pane.
///
/// The pane and the grid are the same width at rest, and then this is the arithmetic that was
/// always here. They differ while the grid is wider than its seat, which is what the typed-input
/// ConPTY resize deferral (`bt-app`, user ruling 2026-08-04) leaves on screen for the length of a
/// narrowing drag: the seat rectangle moves immediately, the grid waits for the child. Aligning to
/// the grid there would place the entire indicator to the right of the scissor rectangle, so the
/// one affordance that says "you are not at the bottom" would disappear exactly while the drag that
/// scrolled it away is happening. `first_column` stays a *grid* column because it indexes the
/// prepared glyph row, not a pixel.
fn status_overlay_geometry(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    status: &str,
    seat_width_px: f32,
) -> Option<StatusOverlayGeometry> {
    if frame.drawable_rows() < 2 {
        return None;
    }
    let columns = frame.columns.get() as usize;
    // The same count `CellMetrics::grid_for_pixels` would answer for this seat, so a grid that fits
    // its seat is bounded by itself and this whole clamp is inert.
    let visible_columns = ((seat_width_px - 2.0 * metrics.padding_px) / metrics.cell_width_px)
        .floor()
        .clamp(0.0, u16::MAX as f32) as usize;
    let shown = status.chars().count().min(columns.min(visible_columns));
    if shown == 0 {
        return None;
    }
    let first_column = columns - shown;
    let right = (metrics.padding_px + columns as f32 * metrics.cell_width_px)
        .min(seat_width_px - metrics.padding_px);
    let left = right - shown as f32 * metrics.cell_width_px;
    Some(StatusOverlayGeometry {
        rect: [
            left,
            metrics.padding_px,
            right,
            metrics.padding_px + metrics.cell_height_px,
        ],
        first_column,
    })
}

/// The seat width a grid of `frame.columns` exactly fits into, i.e. the resting case where the
/// pane and the grid are the same thing.
#[cfg(test)]
fn fitting_seat_width(metrics: CellMetrics, frame: &ViewportFrame) -> f32 {
    2.0 * metrics.padding_px + frame.columns.get() as f32 * metrics.cell_width_px
}

#[cfg(test)]
fn cell_bounds_px(metrics: CellMetrics, row: usize, column: usize) -> [f32; 4] {
    let left = metrics.padding_px + column as f32 * metrics.cell_width_px;
    let top = metrics.padding_px + row as f32 * metrics.cell_height_px;
    [
        left,
        top,
        left + metrics.cell_width_px,
        top + metrics.cell_height_px,
    ]
}

fn frame_cell_bounds_px(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    row: usize,
    column: usize,
) -> [f32; 4] {
    let left = metrics.padding_px + column as f32 * metrics.cell_width_px;
    let mapped = frame
        .row_map
        .get(row)
        .expect("frame geometry consumers validate row_map before drawing");
    let top = metrics.padding_px + mapped.top_subpixels as f32 / SUBPIXELS_PER_PX as f32;
    [
        left,
        top,
        left + metrics.cell_width_px,
        top + mapped.height_subpixels as f32 / SUBPIXELS_PER_PX as f32,
    ]
}

/// The end of the dotted underline run that begins at `index`: the first cell of the same row that
/// is not dotted in the same colour, or the row's end.
///
/// The run is defined by what a cell *wears*, never by why it wears it. That is what makes an
/// explicit OSC 8 link, an inferred URL and a verified image reference one visual system rather than
/// three: two of them standing side by side merge into a single dash pattern that does not restart
/// at their boundary, because nothing here can tell where one ends and the next begins. A solid
/// underline breaks the run, since the solid mark is drawn instead of the dots for that cell.
fn dotted_underline_run_end(
    cells: &[bt_transcript::CapturedCell],
    index: usize,
    columns: usize,
    drawable_cells: usize,
) -> usize {
    let row = index / columns;
    let (foreground, _) = resolve_colors(&cells[index].style);
    let mut end = index + 1;
    while end < drawable_cells && end / columns == row {
        let next = &cells[end];
        let (next_foreground, _) = resolve_colors(&next.style);
        if next.style.flags.contains(CellFlags::UNDERLINE)
            || !next.style.flags.contains(CellFlags::DOTTED_UNDERLINE)
            || next_foreground != foreground
        {
            break;
        }
        end += 1;
    }
    end
}

/// Split one contiguous dotted underline run into opaque, physical-pixel-aligned rectangles.
///
/// Cell metrics are already expressed in physical pixels. The nominal two-logical-pixel dash and
/// gap therefore become 2/3/4 physical pixels at 1x/1.5x/2x. Horizontal edges and the baseline
/// are rounded to device pixels; thickness stays identical to the existing solid underline.
fn dotted_underline_segments(
    left: f32,
    right: f32,
    bottom: f32,
    scale_factor: f64,
) -> Vec<[f32; 4]> {
    let left = left.round();
    let right = right.round();
    let bottom = bottom.round();
    let thickness = (scale_factor as f32).max(1.0);
    let segment = (DOTTED_UNDERLINE_SEGMENT_LOGICAL_PX * scale_factor as f32)
        .round()
        .max(1.0);
    let period = segment * 2.0;
    let mut rectangles = Vec::new();
    let mut x = left;
    while x < right {
        rectangles.push([x, bottom - thickness, (x + segment).min(right), bottom]);
        x += period;
    }
    rectangles
}

fn selection_span_bounds_px(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    selection: &SelectionSpan,
    column: usize,
    span: usize,
) -> [f32; 4] {
    let vertical = frame
        .selection_span_vertical_interval(selection)
        .expect("renderer validates every selection span against the frame row map");
    let left = metrics.padding_px + column as f32 * metrics.cell_width_px;
    let top = metrics.padding_px + vertical.start as f32 / SUBPIXELS_PER_PX as f32;
    [
        left,
        top,
        left + span as f32 * metrics.cell_width_px,
        metrics.padding_px + vertical.end as f32 / SUBPIXELS_PER_PX as f32,
    ]
}

fn ime_cursor_area_for_metrics(metrics: CellMetrics, frame: &ViewportFrame) -> ImeCursorArea {
    let [left, raw_top, right, raw_bottom] = frame_cell_bounds_px(
        metrics,
        frame,
        frame.cursor.row as usize,
        frame.cursor.column as usize,
    );
    // A partially visible first or overscan row may carry the live cursor. The GPU clips the cursor
    // itself to the pane; native IME geometry must use that same clipped interval or Windows can
    // place the candidate window above/below the terminal even though the visible caret is inside.
    let pane_top = metrics.padding_px;
    let pane_bottom = pane_top + frame.grid_rows.get() as f32 * metrics.cell_height_px;
    let top = raw_top.clamp(pane_top, pane_bottom);
    let bottom = raw_bottom.clamp(top, pane_bottom);
    ImeCursorArea {
        x: left.floor() as i32,
        y: top.floor() as i32,
        width: (right.ceil() - left.floor()).max(1.0) as u32,
        height: (bottom.ceil() - top.floor()).max(1.0) as u32,
    }
}

fn cursor_cell_span(frame: &ViewportFrame) -> (usize, usize) {
    let columns = frame.columns.get() as usize;
    let row = frame.cursor.row as usize;
    let column = frame.cursor.column as usize;
    let index = row * columns + column;
    let Some(cell) = frame.cells.get(index) else {
        return (column, 1);
    };
    if cell.style.flags.contains(CellFlags::WIDE_CHAR) {
        (column, 2.min(columns.saturating_sub(column)))
    } else if cell.wide_spacer && column > 0 {
        (column - 1, 2)
    } else {
        (column, 1)
    }
}

/// Whether this seat draws a caret quad at all.
///
/// `focused` is the caret's own standing — the window holds focus and this is the pane being typed
/// into — so the blink belongs to exactly one caret on screen. A faded caret is steady rather than
/// dark half the time: it is already saying "not here", and a second pane blinking out of phase
/// with the one under the hands reads as two cursors competing for them.
fn cursor_quad_visible(
    terminal_cursor_visible: bool,
    focused: bool,
    blink_phase_visible: bool,
) -> bool {
    terminal_cursor_visible && (!focused || blink_phase_visible)
}

/// One seat's caret: the ink it is drawn in, and every rectangle it puts on screen.
struct SeatCaret {
    ink: [u8; 3],
    bounds: Vec<[f32; 4]>,
}

/// The whole caret decision for one seat, in one place: whether it draws at all, in which ink, and
/// in which shape. `None` when this seat shows no caret this frame.
///
/// `focused` is this seat's *own* standing — the window holds focus and this is the pane being
/// typed into — and it is deliberately one parameter rather than two. A pane beside the focused
/// one is, as far as its caret is concerned, in exactly the position a whole window that lost
/// focus is in: nobody is typing there. So it gets exactly that treatment, and all three of the
/// answers below move together instead of being three independent rules that could drift apart.
fn seat_caret(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    focused: bool,
    blink_phase_visible: bool,
    style: CursorStyle,
) -> Option<SeatCaret> {
    if !cursor_quad_visible(frame.cursor.visible, focused, blink_phase_visible) {
        return None;
    }
    // A caret parked past what this frame actually draws is not on screen, and placing it would be
    // a rectangle measured from a row the seat has no pixels for.
    if (frame.cursor.row as usize) >= frame.drawable_rows()
        || frame.cursor.column >= frame.columns.get()
    {
        return None;
    }
    Some(SeatCaret {
        ink: if focused {
            cursor_rgb()
        } else {
            unfocused_cursor_rgb()
        },
        bounds: cursor_pixel_bounds_for_style(metrics, frame, focused, style),
    })
}

#[cfg(test)]
fn cursor_pixel_bounds(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    focused: bool,
) -> Vec<[f32; 4]> {
    cursor_pixel_bounds_for_style(metrics, frame, focused, current_cursor_style())
}

/// The caret's rectangles for one explicitly named shape.
///
/// The process-wide shape is read once, where the seat's fills are built, so nothing below this
/// line depends on global state.
fn cursor_pixel_bounds_for_style(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    focused: bool,
    style: CursorStyle,
) -> Vec<[f32; 4]> {
    let (column, span) = cursor_cell_span(frame);
    let [left, top, _, bottom] =
        frame_cell_bounds_px(metrics, frame, frame.cursor.row as usize, column);
    let right = left + span as f32 * metrics.cell_width_px;
    let cell = [left, top, right, bottom];
    // Losing focus fades the caret's ink and leaves its shape alone: a bar stays the same bar in
    // the same place, an underline the same underline. A block is the one shape whose faded form
    // is geometric — the classic hollow box, which is also the only way for a full-cell caret to
    // stop hiding the cell's own glyph while the window is away. Every other shape already lets
    // the cell read through it, so it has nothing to hollow out.
    if focused || style != CursorStyle::Block {
        return cursor_shape_pixel_bounds(metrics, cell, style);
    }

    // The outline is one logical pixel at every DPI, never wider than half the cell it rings.
    let stroke = (metrics.scale_factor as f32)
        .max(1.0)
        .round()
        .min((right - left) / 2.0)
        .min((bottom - top) / 2.0);
    vec![
        [left, top, right, top + stroke],
        [left, bottom - stroke, right, bottom],
        [left, top + stroke, left + stroke, bottom - stroke],
        [right - stroke, top + stroke, right, bottom - stroke],
    ]
}

fn cursor_shape_pixel_bounds(
    metrics: CellMetrics,
    [left, top, right, bottom]: [f32; 4],
    style: CursorStyle,
) -> Vec<[f32; 4]> {
    match style {
        CursorStyle::Bar => {
            let width = (CURSOR_BAR_WIDTH_LOGICAL_PX * metrics.scale_factor as f32)
                .round()
                .max(1.0);
            let left = left.round();
            vec![[left, top, left + width, bottom]]
        }
        CursorStyle::Block => vec![[left, top, right, bottom]],
        CursorStyle::Underline => {
            let height = (CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX * metrics.scale_factor as f32)
                .round()
                .max(1.0)
                .min(bottom - top);
            vec![[left, bottom - height, right, bottom]]
        }
    }
}

/// **The CJK half of the chrome's family stack, named and ordered** (i18n slice,
/// 2026-08-17).
///
/// Until the Chinese interface shipped this was left to cosmic-text's own
/// Windows fallback table, which asks for `Microsoft YaHei UI` and then falls
/// through to `Segoe UI`, `Segoe UI Emoji`, `Segoe UI Symbol`. That is fine
/// while no ideograph ever reaches the chrome and unacceptable the day every
/// menu is written in them: the face an ideograph lands on decides its weight,
/// its width and its vertical metrics, and "whatever the fallback table happened
/// to find" is not a design decision, it is the absence of one. Worse, the tail
/// of that list is a *symbol* face — a machine missing YaHei would draw the
/// settings dialog in Segoe UI Symbol's outlines and nothing would say so.
///
/// So the chain is written down. It is read in this order, first face present on
/// the machine wins, and it is the same order for every ideograph the window
/// draws:
///
/// 1. `Microsoft YaHei UI` — Simplified Chinese, the UI cut, and Windows' own
///    interface face since 7. The product's Chinese is 简体, so this is the face
///    the design is drawn in.
/// 2. `Microsoft YaHei` — the text cut of the same family, for a Windows that
///    has one and not the other.
/// 3. `DengXian` — Simplified, and the face Office installs; a machine that has
///    had Chinese support added rather than shipped with it often has this and
///    not YaHei.
/// 4. `Microsoft JhengHei UI` / `Microsoft JhengHei` — Traditional. Not the
///    product's Chinese, but every glyph it *does* have is a correct Han glyph,
///    which is the whole question at this point in the list.
/// 5. `Yu Gothic UI` / `Meiryo UI` — Japanese. Han unification means these carry
///    the ideographs with Japanese regional forms; visibly a little off to a
///    Chinese reader, and legible, which beats a box.
/// 6. `Malgun Gothic` — Korean, same argument one step further out.
/// 7. `SimSun` / `NSimSun` — the compatibility face every Chinese Windows has
///    had since XP. Last because it is a serif screen face designed for 12px
///    bitmaps and looks nothing like the rest of this window; present because it
///    is the one that is always there.
///
/// Deliberately **no symbol or emoji face on this list.** An ideograph that
/// reached one would be a mistake, and the shaper's `.notdef` box is a better
/// report of that mistake than a plausible-looking wrong glyph.
const CJK_FALLBACK_FAMILIES: [&str; 11] = [
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "DengXian",
    "Microsoft JhengHei UI",
    "Microsoft JhengHei",
    "Yu Gothic UI",
    "Meiryo UI",
    "Malgun Gothic",
    "SimSun",
    "NSimSun",
    "MS Gothic",
];

/// The files those families live in, in the order the families are read.
///
/// Files rather than families for [`CHROME_SANS_FONT_FILES`]' reason: a file name
/// is a fact about this operating system and a family name is a claim about what
/// is inside the file, and only one of the two can be checked by trying it.
/// Missing entries are harmless — a machine without Korean support simply has no
/// `malgun.ttf`, and the chain moves on.
///
/// They are memory-mapped rather than read, so a face nothing ever shapes costs
/// address space and no pages. That is what lets this list grow past the three
/// files it used to hold without giving up the "never enumerate Fonts/" rule the
/// loader exists to keep.
const CJK_FALLBACK_FONT_FILES: [&str; 9] = [
    "msyh.ttc",
    "msyhbd.ttc",
    "msyhl.ttc",
    "Deng.ttf",
    "Dengb.ttf",
    "Dengl.ttf",
    "msjh.ttc",
    "YuGothR.ttc",
    "malgun.ttf",
];

/// This product's own fallback table: cosmic-text's Windows one, with the CJK
/// scripts answered by [`CJK_FALLBACK_FAMILIES`] instead of by a one-entry guess
/// keyed on a locale this process never sets.
///
/// The locale is the reason a custom table is needed rather than a longer font
/// list. cosmic-text picks its Han face by *locale* — `zh-TW` gets JhengHei,
/// `ja` gets Yu Gothic, everything else gets one entry, `Microsoft YaHei UI` —
/// and this `FontSystem` is built with `"en-US"` and always will be: the locale
/// steers language-sensitive shaping across the whole terminal grid, and
/// switching it because the *chrome* is in Chinese would be changing how a shell's
/// output is shaped to fix how a menu looks.
#[cfg(target_os = "windows")]
#[derive(Debug)]
struct FolioFallback;

#[cfg(target_os = "windows")]
impl Fallback for FolioFallback {
    fn common_fallback(&self) -> &[&'static str] {
        // Unchanged from the platform's: this is the list reached after the
        // script-specific one, and every CJK script now has a specific one.
        &[
            "Segoe UI",
            "Segoe UI Emoji",
            "Segoe UI Symbol",
            "Segoe UI Historic",
        ]
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        use unicode_script::Script;
        match script {
            // One chain for all four, because they share the ideographs and
            // because a mixed line — a Japanese file name in a Chinese dialog —
            // must not change face halfway through for a character both faces
            // have.
            Script::Han | Script::Hiragana | Script::Katakana | Script::Hangul => {
                &CJK_FALLBACK_FAMILIES
            }
            other => glyphon::cosmic_text::PlatformFallback.script_fallback(other, locale),
        }
    }
}

#[cfg(target_os = "windows")]
fn terminal_font_system() -> FontSystem {
    // Keep startup bounded: load a fixed terminal/CJK/symbol fallback chain, never enumerate
    // Fonts/. Noto Color Emoji is compiled into the executable so tests and a standalone binary
    // do not depend on their working directory or on an installer copying a sidecar font.
    // The CJK half of that chain is [`CJK_FALLBACK_FONT_FILES`], loaded in the order
    // [`CJK_FALLBACK_FAMILIES`] names its families. Missing optional files are harmless.
    let windows = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    let fonts = std::path::PathBuf::from(windows).join("Fonts");
    let mut db = glyphon::fontdb::Database::new();
    db.load_font_source(glyphon::fontdb::Source::Binary(Arc::new(
        NOTO_COLOR_EMOJI_BYTES,
    )));
    for file in [
        "consola.ttf",
        "consolab.ttf",
        "consolai.ttf",
        "consolaz.ttf",
        "simsun.ttc",
        "seguiemj.ttf",
        "seguisym.ttf",
    ] {
        let _ = db.load_font_file(fonts.join(file));
    }
    for file in CJK_FALLBACK_FONT_FILES {
        let _ = db.load_font_file(fonts.join(file));
    }
    db.set_monospace_family(DEFAULT_PRIMARY_FONT_FAMILY);
    load_chrome_sans_family(&mut db, &fonts);
    FontSystem::new_with_locale_and_db_and_fallback("en-US".to_owned(), db, FolioFallback)
}

/// The window chrome's UI face, from the mock-up's own stack
/// (`font-family: "Inter", "Segoe UI Variable Text", "Segoe UI"`).
///
/// Only the files are named here, not the families: a family name is a claim
/// about what a file contains, and the one that matters — what `Family::SansSerif`
/// must resolve to — is read back off the face that actually loaded. The first
/// entry that this Windows ships wins, which is what a CSS stack means.
///
/// * `SegUIVar.ttf` is Windows 11's Segoe UI Variable. Its default instance is
///   `wght 400, opsz 10.5`, and the optical-size axis is what names the family's
///   members, so that default *is* the stack's "Segoe UI Variable Text" — the
///   member the mock-up asks for. cosmic-text renders a variable font's default
///   instance, so we get it without having to steer an axis we cannot address.
/// * `segoeui.ttf` is the static Segoe UI every earlier Windows has.
///
/// Inter heads the mock-up's stack and is not on this list: Windows ships no such
/// face, we bundle no assets for chrome, and finding a user-installed one would
/// mean enumerating `Fonts/` at startup — the cost this whole loader exists to
/// avoid. A machine with Inter installed therefore renders the second entry,
/// which is what a browser would do if Inter were absent there too.
const CHROME_SANS_FONT_FILES: [&str; 2] = ["SegUIVar.ttf", "segoeui.ttf"];

/// Load the chrome's UI face and make it the answer to `Family::SansSerif`.
///
/// Without this the request lands wherever the database's first entry happens to
/// be — for a terminal-only font list, an emoji face, which shapes the chrome's
/// titles in Segoe UI Emoji's fallback outlines.
#[cfg(target_os = "windows")]
fn load_chrome_sans_family(db: &mut glyphon::fontdb::Database, fonts: &std::path::Path) {
    for file in CHROME_SANS_FONT_FILES {
        let first_new_face = db.len();
        if db.load_font_file(fonts.join(file)).is_err() {
            continue;
        }
        let Some(family) = db
            .faces()
            .nth(first_new_face)
            .and_then(|face| face.families.first())
            .map(|(family, _)| family.clone())
        else {
            continue;
        };
        db.set_sans_serif_family(family);
        return;
    }
}

#[cfg(not(target_os = "windows"))]
fn terminal_font_system() -> FontSystem {
    let mut font_system = FontSystem::new();
    font_system
        .db_mut()
        .load_font_source(glyphon::fontdb::Source::Binary(Arc::new(
            NOTO_COLOR_EMOJI_BYTES,
        )));
    font_system
}

fn narrow_cell_slots(cells: &[CapturedCell]) -> Vec<NarrowCellSlot> {
    cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| {
            !cell.wide_spacer
                && !cell
                    .style
                    .flags
                    .intersects(CellFlags::WIDE_CHAR | CellFlags::HIDDEN)
                && !cell.text.is_empty()
                && !cell.text.chars().all(char::is_whitespace)
                && !procedural::supports_text(&cell.text)
        })
        .map(|(column, cell)| NarrowCellSlot {
            column,
            text: cell.text.clone(),
            style: cell.style.clone(),
        })
        .collect()
}

fn shape_narrow_buffer(
    key: &ShapeKey,
    font_system: &mut FontSystem,
    metrics: CellMetrics,
    em_scale: f32,
    family: Family<'static>,
) -> Buffer {
    let mut buffer = Buffer::new(
        font_system,
        Metrics::new(metrics.font_size_px, metrics.cell_height_px),
    );
    buffer.set_wrap(Wrap::None);
    // A finite line width makes RTL scalars align within the cell-sized buffer, shifting the
    // local pen away from zero. The TextArea owns the absolute grid origin and row clipping,
    // so the shaping buffer itself must stay horizontally unbounded.
    buffer.set_size(None, Some(metrics.cell_height_px));
    buffer.set_monospace_width(None);
    let mut attrs = shape_attrs(key, family).metrics(Metrics::new(
        metrics.font_size_px * em_scale,
        metrics.cell_height_px,
    ));
    if matches!(family, Family::Monospace) && key.text.chars().count() == 1 {
        attrs = attrs.letter_spacing(
            (metrics.cell_width_px - metrics.primary_advance_px) / metrics.font_size_px,
        );
    }
    buffer.set_text(&key.text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn shape_narrow_buffer_for_key(
    key: &ShapeKey,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    metrics: CellMetrics,
    #[cfg(test)] color_emoji_trial_shapes: &mut u64,
) -> (Buffer, Family<'static>, NarrowSizePolicy) {
    match font_presentation_route(&key.text, font_system) {
        PresentationRoute::TerminalText => {
            let family = Family::Monospace;
            (
                shape_narrow_buffer(key, font_system, metrics, 1.0, family),
                family,
                NarrowSizePolicy::StrictCell,
            )
        }
        PresentationRoute::TextSymbol => {
            let family = Family::Name(TEXT_SYMBOL_FONT_FAMILY);
            (
                shape_narrow_buffer(key, font_system, metrics, 1.0, family),
                family,
                if key.text.chars().any(is_text_coordinated_symbol) {
                    NarrowSizePolicy::TextCoordinated
                } else {
                    NarrowSizePolicy::StrictCell
                },
            )
        }
        PresentationRoute::ColorEmoji => {
            // Which face draws the cluster is a coverage-and-composition question, asked at a
            // nominal em against that em's own square; how large the answer is drawn is
            // [`color_emoji_box_px`], applied to the measured ink below. Keeping the two apart is
            // what lets the size rule change without moving a single cluster between faces.
            let trial_em_px = metrics.cell_height_px;
            let segoe_family = Family::Name(SEGOE_COLOR_EMOJI_FONT_FAMILY);
            let mut family = Family::Name(COLOR_EMOJI_FONT_FAMILY);
            let mut trial = None;
            if font_family_available(font_system, SEGOE_COLOR_EMOJI_FONT_FAMILY) {
                #[cfg(test)]
                {
                    *color_emoji_trial_shapes = color_emoji_trial_shapes.saturating_add(1);
                }
                let segoe = shape_narrow_buffer(
                    key,
                    font_system,
                    metrics,
                    trial_em_px / metrics.font_size_px,
                    segoe_family,
                );
                if is_color_cluster_from_family_within_slot(
                    &segoe,
                    font_system,
                    swash_cache,
                    SEGOE_COLOR_EMOJI_FONT_FAMILY,
                    trial_em_px,
                    trial_em_px,
                ) {
                    family = segoe_family;
                    trial = Some(segoe);
                }
            }
            let trial = trial.unwrap_or_else(|| {
                shape_narrow_buffer(
                    key,
                    font_system,
                    metrics,
                    trial_em_px / metrics.font_size_px,
                    family,
                )
            });

            let em_px = color_emoji_fitted_em_px(
                &trial,
                font_system,
                swash_cache,
                shaped_em_px(&trial, trial_em_px),
                color_emoji_box_px(metrics, 1),
            );
            (
                shape_narrow_buffer(
                    key,
                    font_system,
                    metrics,
                    em_px / metrics.font_size_px,
                    family,
                ),
                family,
                NarrowSizePolicy::ColorEmoji,
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NarrowSizePolicy {
    StrictCell,
    TextCoordinated,
    ColorEmoji,
}

/// The side of the square a colour emoji is drawn at, for a cluster that owns `cells` cells.
///
/// A colour emoji glyph is a square: one em across and one em tall, ink to the edges. So the only
/// question is how long that em is, and the answer is the largest square the cells the cluster
/// owns can contain — `min(cell_height, cells × cell_width)`. Sized by the row's height instead,
/// as this did until 2026-08-17, a one-cell emoji at 16px Consolas is a 22px square standing in an
/// 8.8px column: it covers the character on either side of it, which is exactly what the report
/// that ended that rule showed. Two cells of the same grid hold 17.6px, still short of the row, so
/// the same formula sizes the common wide emoji too and one rule serves both.
fn color_emoji_box_px(metrics: CellMetrics, cells: usize) -> f32 {
    metrics
        .cell_height_px
        .min(cells as f32 * metrics.cell_width_px)
}

/// The em that makes a colour emoji's raster ink exactly [`color_emoji_box_px`] on its long side.
///
/// Measured rather than assumed, because a colour face's ink is not its em: Segoe UI Emoji draws
/// COLR outlines a little past the em box and the bundled Noto is a bitmap strike whose ink is a
/// quarter again as large as the advance it declares. Sizing by either face's own em would put the
/// two at different visual sizes and let both cross the cell boundary; fitting the ink puts one
/// square on the glass whatever face drew it.
fn color_emoji_fitted_em_px(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    shaped_em_px: f32,
    box_px: f32,
) -> f32 {
    let Some([left, top, right, bottom]) = glyph_ink_bounds(buffer, font_system, swash_cache)
    else {
        return shaped_em_px;
    };
    let longest_side_px = (right - left).max(bottom - top);
    if longest_side_px <= 0.0 {
        return shaped_em_px;
    }
    shaped_em_px * box_px / longest_side_px
}

/// The em a shaped buffer's first glyph actually carries, which is not the buffer's nominal size
/// once cosmic-text has normalized a fallback face against a monospace advance.
fn shaped_em_px(buffer: &Buffer, fallback_em_px: f32) -> f32 {
    buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| glyph.font_size)
        .next()
        .unwrap_or(fallback_em_px)
}

fn narrow_fallback_em_scale(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cell_width_px: f32,
) -> f32 {
    let glyphs = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().cloned())
        .collect::<Vec<_>>();
    if glyphs.is_empty()
        || glyphs
            .iter()
            .any(|glyph| is_primary_font_id(font_system, glyph.font_id))
    {
        return 1.0;
    }

    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for glyph in &glyphs {
        left = left.min(glyph.x);
        right = right.max(glyph.x + glyph.w);
        let physical = glyph.physical((0.0, 0.0), 1.0);
        if let Some(image) = swash_cache.get_image_uncached(font_system, physical.cache_key) {
            let ink_left = physical.x as f32 + image.placement.left as f32;
            left = left.min(ink_left);
            right = right.max(ink_left + image.placement.width as f32);
        }
    }
    let occupied_width = (right - left).max(0.0);
    if occupied_width <= cell_width_px {
        return 1.0;
    }

    let side_bearing_px = (cell_width_px * NARROW_FALLBACK_SIDE_BEARING_EM).max(1.0);
    let target_width = (cell_width_px - 2.0 * side_bearing_px).max(1.0);
    (target_width / occupied_width).min(1.0)
}

fn text_coordinated_symbol_em_scale(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cell_width_px: f32,
    cap_height_px: f32,
) -> f32 {
    let Some([left, top, right, bottom]) = glyph_ink_bounds(buffer, font_system, swash_cache)
    else {
        return 1.0;
    };
    let ink_width = right - left;
    let ink_height = bottom - top;
    if ink_width <= 0.0 || ink_height <= 0.0 {
        return 1.0;
    }
    (cell_width_px / ink_width).min(cap_height_px / ink_height)
}

/// The chrome's sans-serif face's cap height, as a fraction of its em.
///
/// Resolved by shaping, not by asking the database, because `Family::SansSerif`
/// is a *request*: only a shaped run says which face the request landed on. The
/// answer is a property of the face alone, so one measurement serves every chrome
/// label at every DPI and nothing here runs per frame.
///
/// The face's published cap height is preferred over its ink: it is exact, where
/// a raster has already been rounded to whole pixels. A face that publishes none
/// is measured from the capital's ink box, which is the same quantity said the
/// long way round.
fn chrome_cap_height_ratio(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) -> Option<f32> {
    // Any size answers, since the ratio is size-free; a large one keeps the
    // measured branch's pixel quantisation under a thousandth of an em.
    const PROBE_PX: f32 = 512.0;
    let mut buffer = Buffer::new(font_system, Metrics::new(PROBE_PX, PROBE_PX));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(None, None);
    buffer.set_text(
        "H",
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
    let (font_id, font_weight) = buffer
        .layout_runs()
        .next()
        .and_then(|run| run.glyphs.first())
        .map(|glyph| (glyph.font_id, glyph.font_weight))?;
    if let Some(font) = font_system.get_font(font_id, font_weight) {
        let metrics = font.metrics();
        if let Some(cap_height) = metrics.cap_height
            && metrics.units_per_em > 0
        {
            return Some(cap_height / f32::from(metrics.units_per_em));
        }
    }
    let [_, cap_top, _, cap_bottom] = glyph_ink_bounds(&buffer, font_system, swash_cache)?;
    Some((cap_bottom - cap_top) / PROBE_PX)
}

fn glyph_ink_bounds(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) -> Option<[f32; 4]> {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let Some(image) = swash_cache.get_image_uncached(font_system, physical.cache_key)
            else {
                continue;
            };
            let left = physical.x as f32 + image.placement.left as f32;
            let top = run.line_y + physical.y as f32 - image.placement.top as f32;
            bounds[0] = bounds[0].min(left);
            bounds[1] = bounds[1].min(top);
            bounds[2] = bounds[2].max(left + image.placement.width as f32);
            bounds[3] = bounds[3].max(top + image.placement.height as f32);
        }
    }
    bounds[0].is_finite().then_some(bounds)
}

fn center_ink_offsets(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    slot_width_px: f32,
    slot_height_px: f32,
) -> (f32, f32) {
    let Some([left, top, right, bottom]) = glyph_ink_bounds(buffer, font_system, swash_cache)
    else {
        return (0.0, 0.0);
    };
    (
        (slot_width_px - (right - left)) / 2.0 - left,
        (slot_height_px - (bottom - top)) / 2.0 - top,
    )
}

fn align_ink_offsets(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    slot_width_px: f32,
    target_center_y_px: f32,
) -> (f32, f32) {
    let Some([left, top, right, bottom]) = glyph_ink_bounds(buffer, font_system, swash_cache)
    else {
        return (0.0, 0.0);
    };
    (
        (slot_width_px - (right - left)) / 2.0 - left,
        target_center_y_px - (top + bottom) / 2.0,
    )
}

fn is_primary_font_id(font_system: &FontSystem, id: glyphon::fontdb::ID) -> bool {
    let primary = primary_font_family(font_system);
    font_system
        .db()
        .face(id)
        .is_some_and(|face| face.families.iter().any(|(family, _)| family == primary))
}

fn shape_narrow_glyphs(
    cells: &[CapturedCell],
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    metrics: CellMetrics,
    cache: &mut NarrowShapingCache,
) -> Vec<NarrowGlyph> {
    narrow_cell_slots(cells)
        .into_iter()
        .map(|slot| {
            let key = ShapeKey {
                text: slot.text,
                bold: slot.style.flags.contains(CellFlags::BOLD),
                italic: slot.style.flags.contains(CellFlags::ITALIC),
            };
            let (buffer, left_offset_px, top_offset_px) =
                cache.get_or_shape(key, font_system, swash_cache, metrics);
            let (foreground, _) = resolve_colors(&slot.style);
            NarrowGlyph {
                column: slot.column,
                buffer,
                left_offset_px,
                top_offset_px,
                color: Color::rgb(foreground[0], foreground[1], foreground[2]),
            }
        })
        .collect()
}

fn wide_cell_slots(cells: &[CapturedCell]) -> Vec<WideCellSlot> {
    cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| {
            cell.style.flags.contains(CellFlags::WIDE_CHAR)
                && !cell.style.flags.contains(CellFlags::HIDDEN)
                && !cell.text.is_empty()
                && !procedural::supports_text(&cell.text)
        })
        .map(|(column, cell)| WideCellSlot {
            column,
            text: cell.text.clone(),
            style: cell.style.clone(),
        })
        .collect()
}

fn shape_wide_glyphs(
    cells: &[CapturedCell],
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    metrics: CellMetrics,
    cache: &mut WideShapingCache,
) -> Vec<WideGlyph> {
    wide_cell_slots(cells)
        .into_iter()
        .map(|slot| {
            let key = ShapeKey {
                text: slot.text,
                bold: slot.style.flags.contains(CellFlags::BOLD),
                italic: slot.style.flags.contains(CellFlags::ITALIC),
            };
            let (buffer, left_offset_px, top_offset_px) =
                cache.get_or_shape(key, font_system, swash_cache, metrics);
            let (foreground, _) = resolve_colors(&slot.style);
            WideGlyph {
                column: slot.column,
                buffer,
                left_offset_px,
                top_offset_px,
                color: Color::rgb(foreground[0], foreground[1], foreground[2]),
            }
        })
        .collect()
}

/// How a two-cell slot sizes the face it shapes with, and therefore where the result is placed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum WideSizePolicy {
    /// A CJK full-width glyph owns a two-cell slot. Matching one cell would shrink the fallback
    /// face to half width; leaving the em alone puts each fallback face at a different visual
    /// size. cosmic-text normalizes the fallback em to the full slot, and the glyph keeps the
    /// row's baseline.
    MonospaceSlot,
    /// A colour emoji is a square of a measured em, centred on the two cells it owns rather than
    /// sat on the baseline — the same rule a one-cell colour emoji obeys, with two cells' worth of
    /// room to obey it in.
    ColorEmojiBox { em_px: f32 },
}

fn shape_wide_buffer(
    key: &ShapeKey,
    font_system: &mut FontSystem,
    metrics: CellMetrics,
    family: Family<'static>,
    size_policy: WideSizePolicy,
) -> Buffer {
    let mut buffer = Buffer::new(
        font_system,
        Metrics::new(metrics.font_size_px, metrics.cell_height_px),
    );
    buffer.set_wrap(Wrap::None);
    buffer.set_size(
        Some(2.0 * metrics.cell_width_px),
        Some(metrics.cell_height_px),
    );
    let attrs = match size_policy {
        WideSizePolicy::MonospaceSlot => {
            buffer.set_monospace_width(Some(metrics.font_size_px * wide_slot_em_scale(metrics)));
            shape_attrs(key, family)
        }
        WideSizePolicy::ColorEmojiBox { em_px } => {
            buffer.set_monospace_width(None);
            shape_attrs(key, family).metrics(Metrics::new(em_px, metrics.cell_height_px))
        }
    };
    buffer.set_text(&key.text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn shape_wide_buffer_for_key(
    key: &ShapeKey,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    metrics: CellMetrics,
    #[cfg(test)] color_emoji_trial_shapes: &mut u64,
) -> (Buffer, WideSizePolicy) {
    match font_presentation_route(&key.text, font_system) {
        PresentationRoute::TerminalText => (
            shape_wide_buffer(
                key,
                font_system,
                metrics,
                Family::Monospace,
                WideSizePolicy::MonospaceSlot,
            ),
            WideSizePolicy::MonospaceSlot,
        ),
        PresentationRoute::TextSymbol => (
            shape_wide_buffer(
                key,
                font_system,
                metrics,
                Family::Name(TEXT_SYMBOL_FONT_FAMILY),
                WideSizePolicy::MonospaceSlot,
            ),
            WideSizePolicy::MonospaceSlot,
        ),
        PresentationRoute::ColorEmoji => {
            // As in the narrow route: the trial answers which face, at the slot-normalized em it
            // has always been asked at, and the fit below answers how large.
            let mut family = Family::Name(COLOR_EMOJI_FONT_FAMILY);
            let mut trial = None;
            if font_family_available(font_system, SEGOE_COLOR_EMOJI_FONT_FAMILY) {
                #[cfg(test)]
                {
                    *color_emoji_trial_shapes = color_emoji_trial_shapes.saturating_add(1);
                }
                let segoe_family = Family::Name(SEGOE_COLOR_EMOJI_FONT_FAMILY);
                let segoe = shape_wide_buffer(
                    key,
                    font_system,
                    metrics,
                    segoe_family,
                    WideSizePolicy::MonospaceSlot,
                );
                if is_color_cluster_from_family_within_slot(
                    &segoe,
                    font_system,
                    swash_cache,
                    SEGOE_COLOR_EMOJI_FONT_FAMILY,
                    2.0 * metrics.cell_width_px,
                    metrics.cell_height_px,
                ) {
                    family = segoe_family;
                    trial = Some(segoe);
                }
            }
            let trial = trial.unwrap_or_else(|| {
                shape_wide_buffer(
                    key,
                    font_system,
                    metrics,
                    family,
                    WideSizePolicy::MonospaceSlot,
                )
            });

            let em_px = color_emoji_fitted_em_px(
                &trial,
                font_system,
                swash_cache,
                shaped_em_px(&trial, metrics.font_size_px * wide_slot_em_scale(metrics)),
                color_emoji_box_px(metrics, 2),
            );
            let size_policy = WideSizePolicy::ColorEmojiBox { em_px };
            (
                shape_wide_buffer(key, font_system, metrics, family, size_policy),
                size_policy,
            )
        }
    }
}

fn baseline_offset_px(reference_baseline_px: f32, glyph_baseline_px: f32) -> f32 {
    reference_baseline_px - glyph_baseline_px
}

fn wide_slot_em_scale(metrics: CellMetrics) -> f32 {
    2.0 * metrics.cell_width_px / metrics.font_size_px
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PresentationRoute {
    TerminalText,
    TextSymbol,
    ColorEmoji,
}

fn font_presentation_route(text: &str, font_system: &mut FontSystem) -> PresentationRoute {
    if let Some(route) = explicit_presentation_route(text) {
        return route;
    }

    // Keep bare media controls and geometric shapes on a stable monochrome face so their
    // text-coordinated size policy does not depend on primary-font coverage. An explicit VS16
    // above still requests color.
    if text.chars().any(is_text_coordinated_symbol) {
        return PresentationRoute::TextSymbol;
    }

    // Match Windows Terminal's visual default: characters with Emoji=Yes use the color route even
    // when Emoji_Presentation=No and even when Consolas contains a monochrome glyph. Bare ASCII
    // keycap components remain terminal text until VS16 or U+20E3 makes the intent explicit.
    if text.chars().any(has_color_emoji_property) {
        return PresentationRoute::ColorEmoji;
    }

    if primary_font_supports_text(font_system, text) {
        return PresentationRoute::TerminalText;
    }

    if text.chars().any(is_text_symbol) {
        return PresentationRoute::TextSymbol;
    }
    PresentationRoute::TerminalText
}

fn explicit_presentation_route(text: &str) -> Option<PresentationRoute> {
    text.chars()
        .rev()
        .find(|character| matches!(character, '\u{fe0e}' | '\u{fe0f}'))
        .map(|selector| {
            if selector == '\u{fe0e}' {
                PresentationRoute::TextSymbol
            } else {
                PresentationRoute::ColorEmoji
            }
        })
}

fn has_color_emoji_property(character: char) -> bool {
    matches!(
        character.emoji_status(),
        EmojiStatus::EmojiPresentation
            | EmojiStatus::EmojiPresentationAndModifierBase
            | EmojiStatus::EmojiPresentationAndEmojiComponent
            | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
            | EmojiStatus::EmojiOther
            | EmojiStatus::EmojiModifierBase
    )
}

fn is_text_symbol(character: char) -> bool {
    matches!(character, '\u{2190}'..='\u{2bff}')
}

fn is_text_coordinated_symbol(character: char) -> bool {
    matches!(character, '\u{23ef}'..='\u{23fa}' | '\u{25a0}'..='\u{25ff}')
}

fn primary_font_supports_text(font_system: &mut FontSystem, text: &str) -> bool {
    let primary_id = font_system.db().query(&glyphon::fontdb::Query {
        // `Family::Monospace` and not a name: the grid's face is whatever
        // `set_terminal_font` last pointed the database at, and asking by name
        // would keep answering for Consolas after the user chose something else
        // — which decides whether a symbol is drawn from the grid's face or
        // from the fallback chain.
        families: &[Family::Monospace],
        weight: Weight::NORMAL,
        stretch: Stretch::Normal,
        style: Style::Normal,
    });
    let Some(primary_id) = primary_id else {
        return false;
    };
    let Some(primary_font) = font_system.get_font(primary_id, Weight::NORMAL) else {
        return false;
    };
    let charmap = primary_font.as_swash().charmap();
    text.chars().all(|character| charmap.map(character) != 0)
}

fn font_family_available(font_system: &FontSystem, family: &str) -> bool {
    font_system.db().faces().any(|face| {
        face.families
            .iter()
            .any(|(candidate, _)| candidate == family)
    })
}

fn is_color_cluster_from_family_within_slot(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    family: &str,
    slot_width_px: f32,
    slot_height_px: f32,
) -> bool {
    let glyphs = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .collect::<Vec<_>>();
    if glyphs.is_empty()
        || glyphs.iter().any(|glyph| {
            glyph.glyph_id == 0
                || !font_system.db().face(glyph.font_id).is_some_and(|face| {
                    face.families
                        .iter()
                        .any(|(candidate, _)| candidate == family)
                })
        })
    {
        return false;
    }

    for glyph in &glyphs {
        let physical = glyph.physical((0.0, 0.0), 1.0);
        if !swash_cache
            .get_image_uncached(font_system, physical.cache_key)
            .is_some_and(|image| image.content == glyphon::SwashContent::Color)
        {
            return false;
        }
    }

    let Some([left, top, right, bottom]) = glyph_ink_bounds(buffer, font_system, swash_cache)
    else {
        return false;
    };
    const SIZE_TOLERANCE_PX: f32 = 0.5;
    let dimensions_fit = right - left <= slot_width_px + SIZE_TOLERANCE_PX
        && bottom - top <= slot_height_px + SIZE_TOLERANCE_PX;
    if glyphs.len() == 1 {
        return dimensions_fit;
    }

    dimensions_fit
        && left >= -SIZE_TOLERANCE_PX
        && top >= -SIZE_TOLERANCE_PX
        && right <= slot_width_px + SIZE_TOLERANCE_PX
        && bottom <= slot_height_px + SIZE_TOLERANCE_PX
}

fn shape_attrs(key: &ShapeKey, family: Family<'static>) -> Attrs<'static> {
    let mut attrs = Attrs::new().family(family);
    if key.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if key.italic {
        attrs = attrs.style(Style::Italic);
    }
    attrs
}

/// The one whole-surface fill in this renderer, and therefore "the ground".
///
/// `rectangles()` emits a quad per cell only where the resolved background
/// differs from the theme's default, so what a reader sees as the surface every
/// pane sits on is this clear. Its alpha is the window's ground opacity
/// (§7.1.6c-4b), premultiplied in linear light because the surface format is
/// sRGB and wgpu encodes the clear value exactly once — the same reason this
/// function has always linearised.
fn theme_clear_color() -> wgpu::Color {
    ground::premultiplied_clear(
        srgb_rgb_to_linear(default_background()),
        ground::window_ground().alpha,
    )
}

#[cfg(test)]
fn rect_gpu_color(color: [u8; 3]) -> [f32; 4] {
    rect_gpu_color_with_coverage(color, 1.0)
}

fn rect_gpu_color_with_coverage(color: [u8; 3], coverage: f32) -> [f32; 4] {
    let [r, g, b] = srgb_rgb_to_linear(color);
    [r as f32, g as f32, b as f32, coverage.clamp(0.0, 1.0)]
}

fn srgb_rgb_to_linear([r, g, b]: [u8; 3]) -> [f64; 3] {
    [r, g, b].map(srgb_channel_to_linear)
}

fn srgb_channel_to_linear(channel: u8) -> f64 {
    let srgb = f64::from(channel) / 255.0;
    if srgb <= 0.04045 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn create_rect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    create_rect_pipeline_with_blend(
        device,
        format,
        "terminal rectangle",
        wgpu::BlendState::ALPHA_BLENDING,
    )
}

/// The rectangle pipeline again, blending `Replace` — what a **ground** is drawn
/// with (§7.1.6c-4b, the one-translucency ruling of 2026-08-18).
///
/// A ground is not painted *onto* the window; it **is** the window at that
/// rectangle, exactly as the clear is and exactly as the picture quad is. Under
/// `ALPHA_BLENDING` there is no source that leaves the destination at the
/// ground's own alpha — `a + (1 − a)·A = A` has no solution but `a = 0` — so a
/// band drawn that way comes out opaque however its alpha is chosen, which is
/// what made the tab strip and every pane head an opaque lid over a translucent
/// window. `Replace` with a premultiplied source is the same arithmetic the
/// clear already runs, written once more where the clear cannot reach.
fn create_ground_rect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    create_rect_pipeline_with_blend(device, format, "chrome ground", wgpu::BlendState::REPLACE)
}

/// The ground pipeline again, **cross-fading** towards whatever it is being laid
/// over by the pass's blend constant — what an [`OverlayGround`] is drawn with
/// (§7.1.6c-4f, the rail's amendment).
///
/// A ground on a floating layer has one thing the chrome's grounds do not: the
/// layer's own CSS `opacity`, which the rail's fold animates from 0 to 1. That
/// fade cannot be a source alpha. `Replace` would ignore it, and `ALPHA_BLENDING`
/// would land the destination on `o + (1 − o)·A` — a rail mid-fold going *more*
/// opaque than the window it is folding into, which is the same arithmetic the
/// one-translucency ruling threw out. What a fading element actually is, is a
/// lerp between the element and what stands behind it, and `Constant` /
/// `OneMinusConstant` on both components is that lerp exactly:
///
/// ```text
/// out = o·(A·colour, A) + (1 − o)·dst
/// ```
///
/// At `o = 1` — the rail at rest, which is every frame but the ~180 ms of a fold
/// — it reduces to `Replace` on a premultiplied source, byte for byte the
/// arithmetic every other ground is drawn with. At `A = 1` it reduces to the
/// `ALPHA_BLENDING` this channel used before it existed, byte for byte, which is
/// why an opaque window's rail is unchanged at every point of its fold.
fn create_ground_fade_rect_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    create_rect_pipeline_with_blend(device, format, "overlay ground", ground_fade_blend())
}

/// The blend state [`create_ground_fade_rect_pipeline`] is built with, named on
/// its own so the pin can assert the arithmetic rather than a pipeline handle.
fn ground_fade_blend() -> wgpu::BlendState {
    let fade = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Constant,
        dst_factor: wgpu::BlendFactor::OneMinusConstant,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: fade,
        alpha: fade,
    }
}

fn create_rect_pipeline_with_blend(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    label: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("{label} shader")),
        source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{label} pipeline layout")),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("{label} pipeline")),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<RectInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 1,
                    },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // Only rounded-corner edge instances use fractional alpha. Backgrounds, cursors,
                // straight box lines, blocks, and underlines retain alpha=1 and stay pixel-sharp.
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_math_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("math block texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("math block scaled sampler"),
        // Live row-band fitting and same-content DPI relayout both deliberately scale an existing
        // raster. Linear filtering makes that brief/adaptive preview readable until fresh pixels
        // atomically replace it.
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("math block shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("math.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("math block pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("math block pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<MathVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32,
                        offset: 16,
                        shader_location: 2,
                    },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                // resvg/tiny-skia rasterizes into premultiplied bytes. bt-math converts those to
                // straight sRGB RGBA before upload so the sRGB texture decode happens before the
                // GPU applies coverage in linear space.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, bind_group_layout, sampler)
}

/// One tile of a [`ChromeIcon`], cut down to the box it may be seen in.
///
/// Returns the quad and the fraction of the tile's own texture that quad now
/// samples — the two have to move together or a cropped picture is a *scaled*
/// picture, which is the bug this function exists to not have. `None` when the
/// crop leaves nothing: a tile entirely outside its clip is a draw call that
/// covers no pixels, and the caller skips it rather than emitting a degenerate
/// quad.
///
/// A missing clip is not "clip to nothing" but "no crop was asked for", which is
/// every chrome mark in the window.
fn cropped_icon_quad(tile: [f32; 4], clip: Option<[f32; 4]>) -> Option<([f32; 4], [f32; 4])> {
    let Some(clip) = clip else {
        return Some((tile, [0.0, 0.0, 1.0, 1.0]));
    };
    let (width, height) = (tile[2] - tile[0], tile[3] - tile[1]);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let quad = [
        tile[0].max(clip[0]),
        tile[1].max(clip[1]),
        tile[2].min(clip[2]),
        tile[3].min(clip[3]),
    ];
    if quad[2] <= quad[0] || quad[3] <= quad[1] {
        return None;
    }
    let uv = [
        (quad[0] - tile[0]) / width,
        (quad[1] - tile[1]) / height,
        (quad[2] - tile[0]) / width,
        (quad[3] - tile[1]) / height,
    ];
    Some((quad, uv))
}

/// The window's ground picture — one quad, its own pipeline.
///
/// Its own and not the math pipeline's, because it needs two things that
/// pipeline cannot give and must not be taught:
///
/// - **a `Repeat` sampler.** Tile is `uv` running past 1.0, and the math
///   sampler is `ClampToEdge` (the descriptor default) precisely so that a
///   formula's last texel does not bleed round to its first.
/// - **`Replace` blending.** This quad does not composite onto the clear, it
///   supersedes it, writing the finished premultiplied ground including its
///   alpha. `ALPHA_BLENDING` would leave the destination alpha at
///   `src.a + dst.a·(1 − src.a)`, i.e. an opaque window wherever the picture is
///   opaque, which is the whole feature inverted.
fn create_background_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("window ground texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("window ground sampler"),
        // Repeat on both axes: Tile is the only fit that reaches past the
        // texture, and the other two never produce a `uv` outside `0..1`, so one
        // sampler serves all three.
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        // Stretch and Fill scale the picture by whatever the window's size
        // demands, which is almost never 1:1 — nearest sampling would show that
        // as stair-stepping on every edge in the photograph.
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("window ground shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("background.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("window ground pipeline layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("window ground pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<BackgroundVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 16,
                        shader_location: 2,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32,
                        offset: 32,
                        shader_location: 3,
                    },
                ],
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, bind_group_layout, sampler)
}

/// The six vertices of the ground quad, covering the whole surface.
///
/// Positions are the NDC corners outright rather than pixels converted the way
/// [`math_quad_vertices`] does: this quad is always the entire target, so there
/// is no viewport to divide by and no rounding to get wrong.
fn background_quad_vertices(
    uv: [f32; 4],
    ground_linear_rgb: [f32; 3],
    alpha: f32,
    image_opacity: f32,
) -> [BackgroundVertex; 6] {
    let [u0, v0, u1, v1] = uv;
    let ground = [
        ground_linear_rgb[0],
        ground_linear_rgb[1],
        ground_linear_rgb[2],
        alpha,
    ];
    let corner = |x: f32, y: f32, u: f32, v: f32| BackgroundVertex {
        position: [x, y],
        uv: [u, v],
        ground,
        image_opacity,
    };
    [
        corner(-1.0, 1.0, u0, v0),
        corner(-1.0, -1.0, u0, v1),
        corner(1.0, -1.0, u1, v1),
        corner(-1.0, 1.0, u0, v0),
        corner(1.0, -1.0, u1, v1),
        corner(1.0, 1.0, u1, v0),
    ]
}

#[allow(clippy::too_many_arguments)]
fn math_quad_vertices(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    uv_left: f32,
    uv_top: f32,
    uv_right: f32,
    uv_bottom: f32,
    viewport_width: u32,
    viewport_height: u32,
    opacity: f32,
) -> [MathVertex; 6] {
    let width = viewport_width.max(1) as f32;
    let height = viewport_height.max(1) as f32;
    let position = |x: f32, y: f32| [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0];
    let corner = |x: f32, y: f32, u: f32, v: f32| MathVertex {
        position: position(x, y),
        uv: [u, v],
        opacity,
    };
    [
        corner(left, top, uv_left, uv_top),
        corner(left, bottom, uv_left, uv_bottom),
        corner(right, bottom, uv_right, uv_bottom),
        corner(left, top, uv_left, uv_top),
        corner(right, bottom, uv_right, uv_bottom),
        corner(right, top, uv_right, uv_top),
    ]
}

/// The x where a math block's raster (and its clip and hit rect) begins. Every consumer must use
/// this one value: the quad vertices, the scissor and the hit test all key on it, and computing
/// the indent in one place while the quad computes its own left is exactly how the indent clipped
/// the raster's left edge (user report 2026-07-20). `rendered_indent` gives a tight-cropped
/// formula the left bearing that text glyphs have; a source block already carries its column.
fn math_block_left_px(metrics: CellMetrics, left_subpixels: i64, rendered_indent: bool) -> f32 {
    let indent = if rendered_indent {
        (MATH_LEFT_INDENT_LOGICAL_PX * metrics.scale_factor as f32).round()
    } else {
        0.0
    };
    metrics.padding_px + indent + left_subpixels as f32 / SUBPIXELS_PER_PX as f32
}

fn math_horizontal_bounds(
    metrics: CellMetrics,
    surface_width: u32,
    columns: NonZeroU32,
    left_subpixels: i64,
    scaled_width: f32,
    rendered_indent: bool,
) -> Option<(f32, f32)> {
    let pane_left = metrics.padding_px;
    let pane_right =
        (pane_left + columns.get() as f32 * metrics.cell_width_px).min(surface_width as f32);
    let block_left = math_block_left_px(metrics, left_subpixels, rendered_indent);
    let visible_left = block_left.max(pane_left);
    let visible_right = (block_left + scaled_width).min(pane_right);
    (visible_right > visible_left).then_some((visible_left, visible_right))
}

fn math_vertical_bounds(
    mode: MathMode,
    pane_top: f32,
    pane_bottom: f32,
    band_top: f32,
    raster_top: f32,
    raster_height: f32,
    clip_height: f32,
) -> ([f32; 2], [f32; 2]) {
    let clip = if mode == MathMode::Display {
        [
            band_top.max(pane_top),
            (band_top + clip_height).min(pane_bottom),
        ]
    } else {
        [
            raster_top.max(pane_top),
            (raster_top + clip_height).min(pane_bottom),
        ]
    };
    let visible = [
        raster_top.max(clip[0]),
        (raster_top + raster_height).min(clip[1]),
    ];
    (visible, clip)
}

fn point_in_rect([x, y]: [f32; 2], [left, top, right, bottom]: [f32; 4]) -> bool {
    x >= left && x < right && y >= top && y < bottom
}

fn default_foreground() -> [u8; 3] {
    foreground_rgb()
}

fn default_background() -> [u8; 3] {
    background_rgb()
}

/// Whether a block's hover dim scrim is drawn.
///
/// The dim darkens a block so its toolbar reads against it. A Rendered block's substance is its
/// texture: if that texture did not draw, the scrim would be the only thing on screen where the
/// picture belongs — the bare grey rectangle. A Source block draws as terminal text and owns no
/// texture, so its hover dim is unconditional (projection deliberately allows a Source block to
/// carry `toolbar_visible`; see `crates/bt-term/src/session.rs`).
fn math_block_dim_is_drawn(placement: &MathBlockPlacement, textured: bool) -> bool {
    placement.toolbar_visible && (placement.display == MathBlockDisplay::Source || textured)
}

/// Whether a placement may upload its texture into the shared byte budget this frame.
///
/// Visibility is part of the question, not an afterthought. Uploading first and asking later let a
/// band scrolled off screen admit its texture and evict the texture of a band the user is looking
/// at; the visible band then found nothing resident, skipped its quad, and left its placement and
/// hover chrome drawing over bare background. Both preparation paths ask this one question.
fn math_block_admits_texture(frame: &ViewportFrame, placement: &MathBlockPlacement) -> bool {
    // A table has no texture and never will: its picture is the window's own text and fills,
    // handed to this renderer as a preview body at the block's own box. Admitting it here would
    // ask the atlas to upload an empty `rgba` and then report the miss as a textureless block —
    // the exact diagnostic that exists to catch a band whose pixels failed to arrive.
    placement.artifact.kind != bt_viewport::RgbaArtifactKind::Table
        && placement.display != MathBlockDisplay::Source
        && frame
            .drawable_interval_overlaps(placement.top_subpixels, placement.clip_height_subpixels)
}

fn surface_config_size(width: u32, height: u32, max_texture_dimension_2d: u32) -> (u32, u32) {
    let limit = max_texture_dimension_2d.max(1);
    (width.max(1).min(limit), height.max(1).min(limit))
}

/// A cell's two colours, and the only place a `CellStyle` becomes bytes.
///
/// **The minimum-contrast floor is applied here and nowhere else** (DESIGN §2.6). The order of
/// the three steps below is the whole of its specification and none of it is incidental:
///
/// 1. `DIM` first, because `SGR 2` is the program asking for a *different colour*, and the
///    floor's question is about the colour that will actually be drawn. A floor applied before
///    the dimming would be answered and then two-thirds undone.
/// 2. `INVERSE` next, because `SGR 7` is how a program declares a background out of the two
///    colours it already has — `rectangles`' own ruling — so after the swap, `foreground` is
///    the ink and `background` is the paper, which is what the floor is about. Running the
///    floor first would hold the *paper* to a contrast ratio and then paint text with it.
/// 3. The floor last, on the pair as drawn. It moves `foreground` and never `background`: a
///    cell's paper is a fact the program declared and a run of cells that share a ground must
///    go on sharing it.
fn resolve_colors(style: &CellStyle) -> ([u8; 3], [u8; 3]) {
    let mut foreground = terminal_color(style.foreground, true);
    let mut background = terminal_color(style.background, false);
    if style.flags.contains(CellFlags::DIM) {
        foreground = foreground.map(|channel| channel.saturating_mul(2) / 3);
    }
    if style.flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }
    (contrast::raise_to_floor(foreground, background), background)
}

fn terminal_color(color: TerminalColor, foreground: bool) -> [u8; 3] {
    // Named codes 16..=28 are the stable Folio encoding declared by bt-transcript.
    match color {
        TerminalColor::Rgb(r, g, b) => [r, g, b],
        TerminalColor::Indexed(index) => indexed_color(index),
        TerminalColor::Named(16 | 27) if foreground => default_foreground(),
        TerminalColor::Named(17) if !foreground => default_background(),
        TerminalColor::Named(18) => cursor_rgb(),
        TerminalColor::Named(28) => DEFAULT_DIM_FOREGROUND_RGB,
        TerminalColor::Named(code @ 19..=26) => {
            indexed_color(code - 19).map(|channel| channel.saturating_mul(2) / 3)
        }
        TerminalColor::Named(code) => indexed_color(code.min(15)),
    }
}

fn indexed_color(index: u8) -> [u8; 3] {
    // The 240 above the scheme's sixteen are the protocol's, not this window's,
    // and they are spelled once in `bt_transcript` because the terminal answers
    // `OSC 4;N;?` out of the same table this draws from.
    bt_transcript::indexed_cube_color(index).unwrap_or_else(|| ansi_16_rgb()[index as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::CapturedCell;

    /// Every style the sweep below asks `resolve_colors` about: the two defaults, both ends of
    /// the palette, a scheme index that collides with its own ground under Solarized, a direct
    /// RGB, and the Folio-owned named codes — crossed with all four combinations of the two
    /// flags that reach this function.
    fn styles_under_test() -> Vec<CellStyle> {
        let colours = [
            TerminalColor::Named(16),
            TerminalColor::Named(17),
            TerminalColor::Named(18),
            TerminalColor::Named(28),
            TerminalColor::Named(21),
            TerminalColor::Named(0),
            TerminalColor::Named(15),
            TerminalColor::Indexed(0),
            TerminalColor::Indexed(8),
            TerminalColor::Indexed(59),
            TerminalColor::Indexed(255),
            TerminalColor::Rgb(0x00, 0x2b, 0x36),
            TerminalColor::Rgb(0x58, 0x6e, 0x75),
            TerminalColor::Rgb(0xff, 0xff, 0xff),
        ];
        let flags = [
            CellFlags::empty(),
            CellFlags::DIM,
            CellFlags::INVERSE,
            CellFlags::DIM | CellFlags::INVERSE,
        ];
        let mut styles = Vec::new();
        for foreground in colours {
            for background in colours {
                for flags in flags {
                    styles.push(CellStyle {
                        flags,
                        foreground,
                        background,
                    });
                }
            }
        }
        styles
    }

    /// What `resolve_colors` was, to the byte, before the minimum-contrast floor existed.
    ///
    /// Copied out of the commit that introduced the floor rather than factored out of the live
    /// function, and that is the point: a shared helper would move with the code it is meant to
    /// hold still, and the pin would go on passing while both halves drifted together.
    fn resolve_colors_before_the_floor(style: &CellStyle) -> ([u8; 3], [u8; 3]) {
        let mut foreground = terminal_color(style.foreground, true);
        let mut background = terminal_color(style.background, false);
        if style.flags.contains(CellFlags::DIM) {
            foreground = foreground.map(|channel| channel.saturating_mul(2) / 3);
        }
        if style.flags.contains(CellFlags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        (foreground, background)
    }

    /// **STRUCTURAL PIN (DESIGN §2.6)** — with the floor `Off`, this renderer draws the bytes it
    /// drew before the floor existed, for every style that reaches `resolve_colors`.
    ///
    /// The default of a feature that overrides colours a program asked for has to be provably
    /// inert, and "provably" means against the old arithmetic rather than against itself: the
    /// reference above is the previous body verbatim, so a floor that leaked into the `Off` path
    /// — an `Off` rung that quietly meant `1.0`, a memo consulted before the rung was read, a
    /// clamp applied unconditionally — turns this red on the first style it touches.
    ///
    /// The second half proves the pin can fail, which is the only thing that makes the first
    /// half worth running: raise the floor and the sweep must diverge, and it must diverge in
    /// the foreground **only**. A background that moved would be the one thing §2.6 promises
    /// never happens.
    #[test]
    fn the_floor_off_is_byte_for_byte_the_renderer_that_had_no_floor() {
        let guard = contrast::FloorGuard::take();

        guard.set(contrast::MinimumContrast::Off);
        for style in styles_under_test() {
            assert_eq!(
                resolve_colors(&style),
                resolve_colors_before_the_floor(&style),
                "Off moved a byte for {style:?}"
            );
        }

        guard.set(contrast::MinimumContrast::Ratio45);
        let mut inks_moved = 0_usize;
        for style in styles_under_test() {
            let (foreground, background) = resolve_colors(&style);
            let (was_foreground, was_background) = resolve_colors_before_the_floor(&style);
            assert_eq!(
                background, was_background,
                "the floor moved the paper under {style:?}"
            );
            if foreground != was_foreground {
                inks_moved += 1;
            }
        }
        assert!(
            inks_moved > 0,
            "raising the floor to 4.5:1 changed no ink at all — the pin above proves nothing"
        );
    }

    #[test]
    fn peek_box_layout_places_below_right_without_upscaling() {
        let layout = peek_box_layout(
            1000.0, 800.0, 1000.0, 800.0, 8.0, 1.0, 100, 50, 100.0, 100.0,
        )
        .unwrap();
        // 1x thumbnail (no upscale), border 1, inset 6: box is 114x64 at pointer + (12, 18).
        assert_eq!(layout.frame, [112.0, 118.0, 226.0, 182.0]);
        assert_eq!(layout.interior, [113.0, 119.0, 225.0, 181.0]);
        assert_eq!(layout.image, [119.0, 125.0, 219.0, 175.0]);
    }

    #[test]
    fn peek_box_layout_flips_above_a_bottom_pointer() {
        let layout = peek_box_layout(
            1000.0, 800.0, 1000.0, 800.0, 8.0, 1.0, 100, 50, 100.0, 780.0,
        )
        .unwrap();
        assert_eq!(layout.frame[1], 780.0 - 18.0 - 64.0);
        assert!(layout.frame[3] <= 792.0);
    }

    #[test]
    fn peek_box_layout_caps_large_images_preserving_aspect_and_clamps_horizontally() {
        let layout = peek_box_layout(
            1000.0, 800.0, 1000.0, 800.0, 8.0, 1.0, 4000, 2000, 950.0, 100.0,
        )
        .unwrap();
        let width = layout.image[2] - layout.image[0];
        let height = layout.image[3] - layout.image[1];
        assert!(width <= (1000.0 - 16.0) * 0.4 + 1e-3);
        // The extent is whole pixels, so the aspect is preserved to within that quantization.
        assert!((height - width * 0.5).abs() <= 1.0);
        // Pointer near the right edge: the box clamps inside the padded pane.
        assert!(layout.frame[2] <= 992.0 + 1e-3);
        assert!(layout.frame[0] >= 8.0);
    }

    /// PIN — the flyout belongs to the pane that owns the hovered content, and to the window it
    /// is drawn over; it belongs to the focused pane in neither sense.
    ///
    /// A 1600x900 window carrying three panes side by side — `[0,600)` focused, `[600,1000)`
    /// hovered, `[1000,1600)` a bystander — with the pointer near the right edge of the middle
    /// one. Two facts, and swapping the hovered pane's extents back to the focused pane's breaks
    /// the first while re-clamping the box into its own pane breaks the second:
    ///
    /// * the thumbnail is capped at 40% of the *hovered* pane, which at these sizes is visibly
    ///   smaller than 40% of the focused one;
    /// * the box lands at the pointer in window coordinates and is allowed to overhang the pane it
    ///   was raised from, stopping only at the window's own padded edge.
    #[test]
    fn a_peek_is_sized_by_the_pane_that_owns_it_and_placed_against_the_window() {
        const WINDOW: (f32, f32) = (1600.0, 900.0);
        const HOVERED_PANE: (f32, f32) = (400.0, 900.0);
        const FOCUSED_PANE: (f32, f32) = (600.0, 900.0);
        const PADDING: f32 = 8.0;
        // Near the middle pane's right edge (it ends at x = 1000), so a box clamped into that pane
        // and a box clamped into the window land in provably different places.
        const POINTER: (f32, f32) = (980.0, 300.0);

        let layout = peek_box_layout(
            HOVERED_PANE.0,
            HOVERED_PANE.1,
            WINDOW.0,
            WINDOW.1,
            PADDING,
            1.0,
            400,
            200,
            POINTER.0,
            POINTER.1,
        )
        .expect("a 400px-wide pane can host a flyout in a 1600px window");

        let hovered_extent =
            peek_thumbnail_extent(HOVERED_PANE.0, HOVERED_PANE.1, PADDING, 1.0, 400, 200)
                .expect("the hovered pane sizes the thumbnail");
        let focused_extent =
            peek_thumbnail_extent(FOCUSED_PANE.0, FOCUSED_PANE.1, PADDING, 1.0, 400, 200)
                .expect("the focused pane would size it differently");
        assert!(
            hovered_extent.0 < focused_extent.0,
            "these two panes must disagree about the cap, or the pin proves nothing: \
             hovered {hovered_extent:?} focused {focused_extent:?}",
        );
        assert_eq!(
            (
                layout.image[2] - layout.image[0],
                layout.image[3] - layout.image[1],
            ),
            (hovered_extent.0 as f32, hovered_extent.1 as f32),
            "the picture is 40% of the pane that owns it, never of the pane holding the keyboard",
        );

        assert_eq!(
            layout.frame[0],
            POINTER.0 + 12.0,
            "the box is anchored at the pointer in the window's own coordinates",
        );
        assert!(
            layout.frame[2] > 1000.0,
            "a floating window may overhang the pane it was raised from: {:?}",
            layout.frame,
        );
        assert!(
            layout.frame[2] <= WINDOW.0 - PADDING + 1e-3 && layout.frame[0] >= PADDING,
            "and stops at the window's padded edge: {:?}",
            layout.frame,
        );
    }

    /// PIN — inside a focused window, the caret under the hands is the only one that is solid and
    /// the only one that blinks; every other pane wears the faded form and stands still.
    ///
    /// The mutation this exists to catch is the single-tenant one: reading the *window's* focus
    /// where the *seat's* was meant. Substitute it and an unfocused pane's block refills, takes the
    /// focused ink, and vanishes on the dark half of the blink.
    #[test]
    fn only_the_pane_under_the_hands_blinks_and_the_rest_wear_the_faded_caret() {
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 0.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        let frame = single_cell_cursor_frame(metrics);

        for style in [CursorStyle::Bar, CursorStyle::Block, CursorStyle::Underline] {
            let lit = seat_caret(metrics, &frame, true, true, style)
                .expect("the focused pane draws its caret on the lit phase");
            assert_eq!(lit.ink, cursor_rgb());
            assert!(
                seat_caret(metrics, &frame, true, false, style).is_none(),
                "the focused caret's dark phase draws nothing ({style:?})",
            );

            let unfocused_lit = seat_caret(metrics, &frame, false, true, style)
                .expect("an unfocused pane still shows where its caret is");
            let unfocused_dark = seat_caret(metrics, &frame, false, false, style)
                .expect("and shows it just as steadily on the other phase");
            assert_eq!(
                unfocused_lit.ink,
                unfocused_cursor_rgb(),
                "an unfocused pane's caret takes the faded ink ({style:?})",
            );
            assert_eq!(
                unfocused_lit.bounds, unfocused_dark.bounds,
                "an unfocused pane's caret does not participate in the blink ({style:?})",
            );
            assert_ne!(
                unfocused_lit.ink, lit.ink,
                "the two panes' carets must be told apart by their ink ({style:?})",
            );
        }

        // The block is the one shape whose faded form is geometric, and the hollow it leaves is how
        // an unfocused pane stops covering its own glyph.
        let focused_block = seat_caret(metrics, &frame, true, true, CursorStyle::Block).unwrap();
        let unfocused_block = seat_caret(metrics, &frame, false, true, CursorStyle::Block).unwrap();
        assert_eq!(focused_block.bounds.len(), 1);
        assert_eq!(
            unfocused_block.bounds.len(),
            4,
            "an unfocused block hollows out into four strokes",
        );
    }

    /// The alpha one plane of the flyout's chrome puts on one pixel. Draws within
    /// a plane add, which is what makes "the lift is this strong here" a claim
    /// about the screen rather than about one draw call.
    fn peek_fill_alpha(fills: &[PeekBoxFill], layer: PeekBoxLayer, x: f32, y: f32) -> f32 {
        fills
            .iter()
            .filter(|fill| fill.layer == layer)
            .filter(|fill| {
                fill.rect[0] <= x && x < fill.rect[2] && fill.rect[1] <= y && y < fill.rect[3]
            })
            .map(|fill| fill.alpha)
            .sum()
    }

    /// PIN (float-window pass): the hover peek wears the mock-up's floating
    /// window — a `--menu` face behind a `--border` hairline at the mock-up's own
    /// alpha, a 10px round, and a lift under it — on both palettes and at every
    /// DPI. Nothing of the flat Campbell-grey box it used to be survives.
    #[test]
    fn the_peek_flyout_wears_the_mock_ups_floating_window() {
        for (theme, palette) in [("dark", DARK_CHROME), ("light", LIGHT_CHROME)] {
            for dpi_milli in [1000_u32, 1250, 1500, 2000] {
                let scale = dpi_milli as f32 / 1000.0;
                let layout = peek_box_layout(
                    1400.0 * scale,
                    900.0 * scale,
                    1400.0 * scale,
                    900.0 * scale,
                    8.0 * scale,
                    scale,
                    (200.0 * scale) as u32,
                    (140.0 * scale) as u32,
                    300.0 * scale,
                    200.0 * scale,
                )
                .expect("the peek box fits this window");
                let fills = peek_box_fills(&layout, palette, scale);
                let border_alpha = f32::from(palette.menu_border_alpha) / 255.0;
                let [left, top, right, bottom] = layout.frame.map(f32::round);
                let border = peek_border_px(scale);
                let radius = FLOAT_WINDOW_RADIUS_LOGICAL_PX * scale;
                let where_is = |x: f32, y: f32| format!("{theme} @{dpi_milli} at ({x}, {y})");

                // Each plane is drawn in the token it belongs to.
                for (layer, color) in [
                    (PeekBoxLayer::Face, palette.menu_surface),
                    (PeekBoxLayer::Hairline, palette.menu_border),
                    (PeekBoxLayer::Lift, palette.menu_shadow),
                ] {
                    assert!(
                        fills
                            .iter()
                            .filter(|fill| fill.layer == layer)
                            .all(|fill| fill.color == color),
                        "{layer:?} must be drawn in its own token ({theme} @{dpi_milli})"
                    );
                }
                // The face fills the box, and does not reach out into the
                // hairline's own pixel.
                let centre = ((left + right) / 2.0, (top + bottom) / 2.0);
                assert_eq!(
                    peek_fill_alpha(&fills, PeekBoxLayer::Face, centre.0, centre.1),
                    1.0,
                    "the flyout's face must be opaque --menu: {}",
                    where_is(centre.0, centre.1)
                );
                assert_eq!(
                    peek_fill_alpha(&fills, PeekBoxLayer::Face, centre.0, top),
                    0.0,
                    "the face must start one border in: {}",
                    where_is(centre.0, top)
                );
                assert_eq!(
                    peek_fill_alpha(&fills, PeekBoxLayer::Face, centre.0, top + border),
                    1.0,
                    "and it must start *exactly* one border in: {}",
                    where_is(centre.0, top + border)
                );
                // The hairline is one pixel of --border at its own alpha.
                for (x, y) in [
                    (centre.0, top),
                    (centre.0, bottom - 1.0),
                    (left, centre.1),
                    (right - 1.0, centre.1),
                ] {
                    let seen = peek_fill_alpha(&fills, PeekBoxLayer::Hairline, x, y);
                    assert!(
                        (seen - border_alpha).abs() < 1e-6,
                        "the hairline must be --border at its own alpha, saw {seen} \
                         instead of {border_alpha}: {}",
                        where_is(x, y)
                    );
                }
                // Round, not square: the box's own corner pixel is empty, and the
                // corner is antialiased rather than stepped — the same claim
                // `marks.rs` pins for the tab.
                for (x, y) in [
                    (left, top),
                    (right - 1.0, top),
                    (left, bottom - 1.0),
                    (right - 1.0, bottom - 1.0),
                ] {
                    assert_eq!(
                        peek_fill_alpha(&fills, PeekBoxLayer::Hairline, x, y),
                        0.0,
                        "a floating window's corner is round: {}",
                        where_is(x, y)
                    );
                }
                let feathered = fills
                    .iter()
                    .filter(|fill| fill.layer == PeekBoxLayer::Hairline)
                    .filter(|fill| fill.alpha > 0.0 && fill.alpha < border_alpha - 1e-6)
                    .count();
                assert!(
                    feathered >= radius as usize,
                    "an antialiased round spends at least one partial pixel per row, \
                     saw {feathered} ({theme} @{dpi_milli})"
                );
                // The lift surrounds the box, reaches exactly its spread, and is
                // never painted under the box it lifts — a shadow that shows
                // through a .09-alpha hairline is a doubled hairline.
                let spread = (FLOAT_WINDOW_SHADOW_LOGICAL_PX * scale).round();
                assert!(
                    peek_fill_alpha(&fills, PeekBoxLayer::Lift, centre.0, top - 1.0) > 0.0,
                    "the flyout must sit above the terminal, not on it: {}",
                    where_is(centre.0, top - 1.0)
                );
                assert_eq!(
                    peek_fill_alpha(&fills, PeekBoxLayer::Lift, centre.0, top - spread - 1.0),
                    0.0,
                    "the lift must stop at its spread: {}",
                    where_is(centre.0, top - spread - 1.0)
                );
                for (x, y) in [centre, (centre.0, top), (left, centre.1)] {
                    assert_eq!(
                        peek_fill_alpha(&fills, PeekBoxLayer::Lift, x, y),
                        0.0,
                        "the lift is clipped out of the box it lifts: {}",
                        where_is(x, y)
                    );
                }
                // It also falls off outwards instead of standing as one band.
                let near = peek_fill_alpha(&fills, PeekBoxLayer::Lift, centre.0, top - 1.0);
                let far = peek_fill_alpha(&fills, PeekBoxLayer::Lift, centre.0, top - spread);
                assert!(
                    near > far && far > 0.0,
                    "the lift must fade outwards, saw {near} then {far} ({theme} @{dpi_milli})"
                );
                // The decomposition costs what an outline costs, not what a fill
                // costs: whole runs merge, and only the rounds are spent per
                // pixel. A flyout that started emitting one quad per pixel of its
                // own area would be ~30 times this at 100% and would grow with the
                // square of the DPI.
                let perimeter = 2.0 * ((right - left) + (bottom - top));
                assert!(
                    (fills.len() as f32) < 2.0 * perimeter,
                    "{} quads for a {}×{} flyout ({theme} @{dpi_milli}) — the runs \
                     have stopped merging",
                    fills.len(),
                    right - left,
                    bottom - top
                );
                // Red gate: the box this pass replaced was Campbell bright-black
                // around the status bar's grey, with square corners.
                assert!(
                    !fills.iter().any(|fill| fill.color == [0x76, 0x76, 0x76]
                        || fill.color == DEFAULT_STATUS_BACKGROUND_RGB),
                    "the flyout must be built from --menu/--border, not the old flat box"
                );
            }
        }
    }

    #[test]
    fn peek_box_layout_refuses_a_window_too_small_for_the_box() {
        assert!(peek_box_layout(30.0, 30.0, 30.0, 30.0, 8.0, 1.0, 10, 10, 10.0, 10.0).is_none());
        assert!(
            peek_box_layout(1000.0, 800.0, 1000.0, 800.0, 8.0, 1.0, 0, 10, 10.0, 10.0).is_none()
        );
    }

    /// The flyout must be able to consume the extent it published: resampling to
    /// `peek_thumbnail_extent` and laying that out again must reproduce the same thumbnail, or the
    /// renderer would silently rescale the pixels the app resampled for it.
    #[test]
    fn peek_thumbnail_extent_is_idempotent_so_the_layout_agrees_with_display_sized_pixels() {
        for (native_width, native_height) in
            [(4000_u32, 2000_u32), (3840, 2400), (100, 50), (17, 4099)]
        {
            let (display_width, display_height) =
                peek_thumbnail_extent(1000.0, 800.0, 8.0, 1.5, native_width, native_height)
                    .unwrap();
            assert!(display_width <= native_width && display_height <= native_height);
            assert_eq!(
                peek_thumbnail_extent(1000.0, 800.0, 8.0, 1.5, display_width, display_height),
                Some((display_width, display_height)),
                "a display-sized image is already within the cap, so the extent is the identity",
            );
            let layout = peek_box_layout(
                1000.0,
                800.0,
                1000.0,
                800.0,
                8.0,
                1.5,
                display_width,
                display_height,
                40.0,
                40.0,
            )
            .unwrap();
            assert_eq!(layout.image[2] - layout.image[0], display_width as f32);
            assert_eq!(layout.image[3] - layout.image[1], display_height as f32);
        }
    }

    /// Pin (b) of the peek raster defect, on the real `ByteLru` at the real budget: hovering a
    /// wallpaper path while bands that fit the budget are on screen must evict none of them. The
    /// native-resolution upload the defect performed does evict one — the grey band symptom.
    #[test]
    fn a_display_sized_peek_thumbnail_does_not_evict_an_onscreen_bands_texture() {
        // A 4K pane showing three image bands, each already display-sized (5421eab): full padded
        // pane width by the one-third-viewport height cap. That is the resident set a hover meets.
        let (viewport_width, viewport_height) = (3840.0_f32, 2160.0_f32);
        let padding = 8.0_f32;
        let band_width = (viewport_width - 2.0 * padding) as usize;
        let band_height = ((viewport_height - 2.0 * padding) / 3.0) as usize;
        let band_bytes = band_width * band_height * 4;
        let band_keys: Vec<String> = (0..3)
            .map(|index| {
                bt_term::display_texture_key(
                    &format!("image:band{index}"),
                    band_width as u32,
                    band_height as u32,
                )
            })
            .collect();

        // The hovered path is a 3840x2400 wallpaper: 35 MiB decoded, against a 64 MiB budget the
        // bands already hold half of.
        let (native_width, native_height) = (3840_u32, 2400_u32);
        let native_bytes = native_width as usize * native_height as usize * 4;
        assert!(
            3 * band_bytes + native_bytes > MATH_TEXTURE_CACHE_BUDGET_BYTES,
            "the premise: the native decode does not fit beside the bands on screen",
        );
        let (thumb_width, thumb_height) = peek_thumbnail_extent(
            viewport_width,
            viewport_height,
            padding,
            1.0,
            native_width,
            native_height,
        )
        .unwrap();
        let thumb_bytes = thumb_width as usize * thumb_height as usize * 4;

        let mut cache = ByteLru::<String, ()>::new(MATH_TEXTURE_CACHE_BUDGET_BYTES);
        for key in &band_keys {
            let (admitted, evictions) = cache.insert(key.clone(), (), band_bytes);
            assert!(admitted && evictions == 0);
        }
        let peek_key = bt_term::display_texture_key("image:wallpaper", thumb_width, thumb_height);
        let (admitted, evictions) = cache.insert(peek_key, (), thumb_bytes);
        assert!(admitted, "the flyout's own texture is admissible");
        assert_eq!(
            evictions, 0,
            "hovering a path evicted a band that fits: {thumb_bytes} peek bytes beside \
             {band_bytes} per band",
        );
        for key in &band_keys {
            assert!(
                cache.get(key).is_some(),
                "the bands on screen kept their textures across the hover",
            );
        }

        // Same LRU, same bands, with the peek uploading the native decode the way the defect did:
        // a band's texture is gone and its quad has nothing to sample.
        let mut native = ByteLru::<String, ()>::new(MATH_TEXTURE_CACHE_BUDGET_BYTES);
        for key in &band_keys {
            assert!(native.insert(key.clone(), (), band_bytes).0);
        }
        let (_, native_evictions) = native.insert("image:wallpaper".to_owned(), (), native_bytes);
        assert_eq!(native_evictions, 1);
        assert!(
            native.get(&band_keys[0]).is_none(),
            "this is the defect: a native-resolution peek evicted the oldest band on screen",
        );
    }

    /// A rendered live math placement carrying `resident_bytes` of RGBA, for texture-budget tests.
    fn test_math_placement(
        key: &str,
        top_subpixels: i64,
        clip_height_subpixels: i64,
        resident_bytes: usize,
    ) -> MathBlockPlacement {
        let width_px = 1_u32;
        let height_px = (resident_bytes / 4) as u32;
        MathBlockPlacement {
            start: bt_transcript::TranscriptId(1),
            // History-anchored: a scrolled-away block is exactly a history block off the top of
            // the pane, and it keeps the fixture free of the live band-top frame invariant.
            anchor: bt_viewport::MathBlockAnchor::History {
                run: None,
                start: bt_transcript::TranscriptId(1),
                end: bt_transcript::TranscriptId(1),
            },
            source: key.to_owned(),
            artifact: bt_viewport::ProjectedMathArtifact {
                inline_runs: Vec::new(),
                key: key.to_owned(),
                end: bt_transcript::TranscriptId(1),
                rgba: Arc::from(vec![0_u8; resident_bytes]),
                width_px,
                height_px,
                height_subpixels: clip_height_subpixels,
                baseline_subpixels: 0,
                mode: MathMode::Display,
                kind: bt_viewport::RgbaArtifactKind::InlineImage { animated: false },
                vertical_padding_subpixels: 0,
                render_scale_milli: 1000,
                source: key.to_owned(),
            },
            top_subpixels,
            left_subpixels: 0,
            content_offset_subpixels: 0,
            clip_height_subpixels,
            display: MathBlockDisplay::Rendered,
            horizontal_overflow: bt_viewport::HorizontalOverflowOwner::Block,
            horizontal_scroll_px: 0,
            vertical_scroll_px: 0,
            toolbar_visible: false,
            occluded_source_rows: 0,
            occluded_visible_rows: Vec::new(),
            live_occurrence_id: None,
            frozen_prefix_rows: 0,
            clipped_top_rows: 0,
            clipped_bottom_rows: 0,
        }
    }

    /// An inline composite carrying the given runs, in raster-pixel offsets within the image.
    fn test_inline_placement(
        runs: &[(u32, u32, u32)],
        render_scale_milli: u32,
    ) -> MathBlockPlacement {
        let mut placement = test_math_placement("inline", 0, 24 * SUBPIXELS_PER_PX, 4);
        placement.artifact.mode = MathMode::Inline;
        placement.artifact.kind = bt_viewport::RgbaArtifactKind::Math;
        placement.artifact.render_scale_milli = render_scale_milli;
        placement.artifact.inline_runs = runs
            .iter()
            .map(|(run, x_px, width_px)| bt_viewport::InlineRunPlacement {
                run: *run,
                x_px: *x_px,
                width_px: *width_px,
            })
            .collect();
        placement
    }

    /// PIN (slice 4): the pointer resolves to one *run*, not to the line it sits on.
    ///
    /// An inline placement is a single composite covering a whole logical line, so the block
    /// rectangle a hit test matches cannot by itself say which of two formulas was under the
    /// cursor — and every interaction that follows is answered per run: which LaTeX the copy button
    /// puts on the clipboard, which formula the toolbar is about. Copying already had its own pin;
    /// the step *before* it, turning a pixel into a run index, had none, and a regression there
    /// would silently hand every interaction to run 0.
    #[test]
    fn a_pointer_over_an_inline_composite_resolves_to_the_run_beneath_it() {
        // Two runs with a gap of prose between them: cells 0..40 and 100..140 inside the image,
        // drawn in a block whose left edge is at x = 10.
        let placement = test_inline_placement(&[(0, 0, 40), (1, 100, 40)], 1000);
        let block = [10.0, 0.0, 160.0, 24.0];
        let run_at = |x: f32| inline_run_at(&placement, block, x);

        assert_eq!(run_at(15.0), Some(0), "inside the first run");
        assert_eq!(run_at(120.0), Some(1), "inside the second run");

        // A point in the prose between two formulas is inside the block and belongs to neither.
        // Nearest is the only non-arbitrary answer, and it must actually be nearest.
        assert_eq!(run_at(70.0), Some(0), "the gap resolves to the nearer run");
        assert_eq!(run_at(100.0), Some(1), "and to the other one on its side");
        assert_eq!(run_at(0.0), Some(0), "left of everything is the first run");
        assert_eq!(
            run_at(500.0),
            Some(1),
            "right of everything is the last run"
        );

        // Run offsets live in raster-pixel space, so they must be scaled exactly as the block is —
        // a stale raster presented at half scale puts its second run at half the offset. Without
        // this the pointer would resolve against a geometry the user is not looking at.
        let halved = test_inline_placement(&[(0, 0, 40), (1, 100, 40)], 500);
        assert_eq!(
            inline_run_at(&halved, block, 70.0),
            Some(1),
            "at half scale the second run covers 60..80, so 70 is inside it"
        );

        // Display math has no runs, and must not invent one.
        let display = test_inline_placement(&[], 1000);
        assert_eq!(inline_run_at(&display, block, 20.0), None);
    }

    fn test_cell_anchors(count: usize) -> Vec<bt_viewport::CellAnchor> {
        (0..count)
            .map(|column| {
                let anchor = bt_doc::ContentAnchor::Live {
                    screen: bt_doc::ScreenId::Primary,
                    point: bt_doc::GridPoint {
                        row: 0,
                        column: column as u32,
                    },
                    bias: bt_doc::Bias::Before,
                    generation: bt_doc::GridGeneration(1),
                };
                bt_viewport::CellAnchor {
                    start: anchor.clone(),
                    end: anchor,
                }
            })
            .collect()
    }

    fn test_row_map(rows: u32) -> Vec<bt_viewport::FrameVisualRow> {
        (0..rows)
            .map(|row| bt_viewport::FrameVisualRow {
                top_subpixels: i64::from(row) * 22 * SUBPIXELS_PER_PX,
                height_subpixels: 22 * SUBPIXELS_PER_PX,
                live_grid_row: Some(row),
                continues: false,
            })
            .collect()
    }

    fn test_row_map_for_metrics(
        rows: u32,
        metrics: CellMetrics,
    ) -> Vec<bt_viewport::FrameVisualRow> {
        let height = (metrics.cell_height_px * SUBPIXELS_PER_PX as f32).round() as i64;
        (0..rows)
            .map(|row| bt_viewport::FrameVisualRow {
                top_subpixels: i64::from(row) * height,
                height_subpixels: height,
                live_grid_row: Some(row),
                continues: false,
            })
            .collect()
    }

    #[test]
    fn display_math_inset_aligns_visual_left_and_moves_hit_geometry_with_it() {
        let metrics = CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 8.0,
            scale_factor: 1.0,
            ascii_baseline_px: 15.0,
            primary_advance_px: 10.0,
            primary_cap_height_px: 12.0,
            primary_cap_center_y_px: 10.0,
        };
        let inset_subpixels = 5 * SUBPIXELS_PER_PX;
        // A source-column offset (left_subpixels) positions the block without the rendered indent.
        let (visible_left, visible_right) = math_horizontal_bounds(
            metrics,
            200,
            NonZeroU32::new(10).unwrap(),
            inset_subpixels,
            40.0,
            false,
        )
        .unwrap();
        let expected_left = metrics.padding_px + 5.0;
        assert!((visible_left - expected_left).abs() <= 1.0);

        // A rendered block gets a small left indent so its tight-cropped ink lines up with text.
        let (rendered_left, _) =
            math_horizontal_bounds(metrics, 200, NonZeroU32::new(10).unwrap(), 0, 40.0, true)
                .unwrap();
        assert!(rendered_left > metrics.padding_px + 1.0);

        let hit = [visible_left, 10.0, visible_right, 30.0];
        assert!(!point_in_rect([metrics.padding_px + 1.0, 20.0], hit));
        assert!(point_in_rect([visible_left + 1.0, 20.0], hit));
    }

    /// Content extents read the *seat*, and the seat is what stops them.
    ///
    /// A band wider than the pane, and the toolbar pinned to its right edge,
    /// must both end inside the seat. The only number that can end them is the
    /// one passed here, so this is the pin for "no draw site substitutes the
    /// window extent": hand it the window (1920) instead of the seat (975) and
    /// the assertions fail, which is exactly the picture the user saw — the
    /// band and its toolbar past the divider, in the next seat.
    #[test]
    fn a_band_and_its_toolbar_end_inside_the_seat_not_inside_the_window() {
        let metrics = CellMetrics {
            cell_width_px: 18.0,
            cell_height_px: 44.0,
            font_size_px: 32.0,
            padding_px: 16.0,
            scale_factor: 2.0,
            ascii_baseline_px: 34.0,
            primary_advance_px: 18.0,
            primary_cap_height_px: 24.0,
            primary_cap_center_y_px: 20.0,
        };
        const SEAT_WIDTH: u32 = 975;
        const WINDOW_WIDTH: u32 = 1920;
        // 60 columns is wider than this seat holds: padding + 60 * 18 = 1096.
        let columns = NonZeroU32::new(60).unwrap();
        let (left, right) = math_horizontal_bounds(
            metrics, SEAT_WIDTH, columns, 0, 4000.0, // an image far wider than either extent
            true,
        )
        .expect("a visible band");
        assert!(left >= metrics.padding_px);
        assert!(
            right <= SEAT_WIDTH as f32,
            "the band ran to {right}, past the {SEAT_WIDTH}px seat"
        );
        // The toolbar is placed at the band's right edge, clamped to the same
        // pane edge; it must not reach past the seat either.
        let (toolbar_top, toolbar_bottom) = math_toolbar_vertical_bounds(40.0, 120.0, 2.0);
        let button = toolbar_bottom - toolbar_top;
        let total = button * 2.0 + MATH_TOOL_GAP_LOGICAL_PX * 2.0;
        let pane_right = (metrics.padding_px + columns.get() as f32 * metrics.cell_width_px)
            .min(SEAT_WIDTH as f32);
        let toolbar_left = right.min(pane_right - total).max(metrics.padding_px);
        assert!(
            toolbar_left + total <= SEAT_WIDTH as f32,
            "the toolbar ran to {}, past the {SEAT_WIDTH}px seat",
            toolbar_left + total
        );
        // Red gate: the same call against the window extent does escape.
        let (_, window_right) =
            math_horizontal_bounds(metrics, WINDOW_WIDTH, columns, 0, 4000.0, true).unwrap();
        assert!(
            window_right > SEAT_WIDTH as f32,
            "the pin would pass even if a draw site read the window"
        );
    }

    /// PIN: the "N rows above" indicator ends inside the seat, not inside the grid.
    ///
    /// The two are the same rectangle at rest. They differ while the grid is wider than its seat,
    /// which the typed-input ConPTY resize deferral (`bt-app`, user ruling 2026-08-04) leaves on
    /// screen for the length of a narrowing drag. Right-aligning to the grid there puts the whole
    /// indicator past the scissor rectangle — the affordance that says "you are not at the bottom"
    /// would vanish exactly while the drag that scrolled it away is under way. The red gate is the
    /// second half: hand the same call a seat that fits the grid and the overlay does sit at the
    /// grid's right edge, so this pin is not asserting an unconditional clamp.
    #[test]
    fn the_rows_above_indicator_ends_inside_the_seat_not_inside_an_over_wide_grid() {
        let metrics = CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 8.0,
            scale_factor: 1.0,
            ascii_baseline_px: 15.0,
            primary_advance_px: 10.0,
            primary_cap_height_px: 11.0,
            primary_cap_center_y_px: 9.0,
        };
        let columns = 80_usize;
        let rows = 4_usize;
        let frame = ViewportFrame {
            columns: NonZeroU32::new(columns as u32).unwrap(),
            grid_rows: NonZeroU32::new(rows as u32).unwrap(),
            rows: NonZeroU32::new(rows as u32).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::default(); columns * rows],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
            cell_anchors: test_cell_anchors(columns * rows),
            row_map: test_row_map(rows as u32),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: Some("7 rows above · Shift+wheel".to_owned()),
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(columns as u32),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let status = frame.status_text.clone().unwrap();

        // The resting case: a seat the grid exactly fits. The overlay ends at the grid's right edge,
        // which is also the seat's, and every earlier frame in the product is this case.
        let fitting = fitting_seat_width(metrics, &frame);
        let resting = status_overlay_geometry(metrics, &frame, &status, fitting).unwrap();
        assert!((resting.rect[2] - (fitting - metrics.padding_px)).abs() <= 0.01);

        // Mid-deferral: an 80-column grid inside a seat that only holds 40 columns.
        let narrow_seat = 2.0 * metrics.padding_px + 40.0 * metrics.cell_width_px;
        let clamped = status_overlay_geometry(metrics, &frame, &status, narrow_seat).unwrap();
        assert!(
            clamped.rect[2] <= narrow_seat - metrics.padding_px + 0.01,
            "the indicator ran to {}, past the {narrow_seat}px seat",
            clamped.rect[2]
        );
        assert!(
            clamped.rect[0] >= metrics.padding_px - 0.01,
            "the indicator started at {}, before the pane's own left edge",
            clamped.rect[0]
        );
        assert!(
            resting.rect[2] > narrow_seat,
            "the pin would pass even if the geometry still read the grid"
        );
        // `first_column` indexes the prepared glyph row, which is a grid row, so it must keep
        // naming grid columns however far left the pixels moved.
        assert_eq!(clamped.first_column + status.chars().count(), columns);
    }

    #[test]
    fn math_toolbar_shrinks_to_the_visible_source_row_band() {
        for (scale, top, bottom) in [(1.0, 8.0, 26.0), (1.25, 10.0, 32.5)] {
            let (button_top, button_bottom) = math_toolbar_vertical_bounds(top, bottom, scale);
            assert!(button_top >= top);
            assert!(button_bottom <= bottom);
            assert_eq!(button_bottom - button_top, bottom - top);
        }
        assert_eq!(
            math_toolbar_vertical_bounds(5.0, 35.0, 1.0),
            (9.0, 31.0),
            "a taller block keeps the intended 22px control"
        );
    }

    #[test]
    fn display_math_clip_is_the_padded_box_and_contains_the_tight_raster() {
        let (visible, clip) =
            math_vertical_bounds(MathMode::Display, 0.0, 100.0, 10.0, 15.0, 20.0, 30.0);
        assert_eq!(clip, [10.0, 40.0]);
        assert_eq!(visible, [15.0, 35.0]);
        assert!(clip[0] <= visible[0] && visible[1] <= clip[1]);
        assert_eq!(visible[0] - clip[0], clip[1] - visible[1]);

        let (inline_visible, inline_clip) =
            math_vertical_bounds(MathMode::Inline, 0.0, 100.0, 10.0, 15.0, 20.0, 30.0);
        assert_eq!(inline_clip, [15.0, 45.0]);
        assert_eq!(inline_visible, [15.0, 35.0]);
        // Mutation: keying display clip_top to raster_top makes display clip equal inline_clip.
        assert_ne!(clip, inline_clip);
    }

    fn shape_narrow_for_test(
        cells: &[CapturedCell],
        font_system: &mut FontSystem,
        metrics: CellMetrics,
    ) -> Vec<NarrowGlyph> {
        shape_narrow_glyphs(
            cells,
            font_system,
            &mut SwashCache::new(),
            metrics,
            &mut NarrowShapingCache::new(),
        )
    }

    fn shape_wide_for_test(
        cells: &[CapturedCell],
        font_system: &mut FontSystem,
        metrics: CellMetrics,
    ) -> Vec<WideGlyph> {
        shape_wide_glyphs(
            cells,
            font_system,
            &mut SwashCache::new(),
            metrics,
            &mut WideShapingCache::new(),
        )
    }

    fn assert_narrow_glyph_origins(glyphs: &[NarrowGlyph], metrics: CellMetrics) {
        const X_TOLERANCE: f32 = 0.0001;

        for slot in glyphs {
            let layout_glyphs = slot
                .buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter())
                .collect::<Vec<_>>();
            assert!(
                !layout_glyphs.is_empty(),
                "column {} has no glyph",
                slot.column
            );
            for glyph in layout_glyphs {
                let actual_x = slot.column as f32 * metrics.cell_width_px + glyph.x;
                let expected_x = slot.column as f32 * metrics.cell_width_px;
                assert!(
                    (actual_x - expected_x).abs() <= X_TOLERANCE,
                    "column {}: glyph x={actual_x} but cell-grid x={expected_x}",
                    slot.column
                );
            }
        }
    }

    fn first_layout_glyph(buffer: &Buffer) -> glyphon::cosmic_text::LayoutGlyph {
        buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .next()
            .cloned()
            .expect("shaped buffer has a glyph")
    }

    fn glyph_family(font_system: &FontSystem, glyph: &glyphon::cosmic_text::LayoutGlyph) -> String {
        font_system
            .db()
            .face(glyph.font_id)
            .and_then(|face| face.families.first())
            .map(|(family, _)| family.clone())
            .expect("glyph font has a family")
    }

    fn raster_content(font_system: &mut FontSystem, buffer: &Buffer) -> glyphon::SwashContent {
        let glyph = first_layout_glyph(buffer);
        SwashCache::new()
            .get_image_uncached(font_system, glyph.physical((0.0, 0.0), 1.0).cache_key)
            .expect("glyph rasterizes")
            .content
    }

    fn occupied_width_px(font_system: &mut FontSystem, buffer: &Buffer) -> f32 {
        let glyphs = buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().cloned())
            .collect::<Vec<_>>();
        let mut cache = SwashCache::new();
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        for glyph in glyphs {
            left = left.min(glyph.x);
            right = right.max(glyph.x + glyph.w);
            let physical = glyph.physical((0.0, 0.0), 1.0);
            if let Some(image) = cache.get_image_uncached(font_system, physical.cache_key) {
                let ink_left = physical.x as f32 + image.placement.left as f32;
                left = left.min(ink_left);
                right = right.max(ink_left + image.placement.width as f32);
            }
        }
        right - left
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn presentation_selectors_and_ambiguous_symbols_route_explicitly() {
        let mut font_system = terminal_font_system();
        for text in ["👨‍👩‍👧‍👦", "👍🏽", "🇺🇸", "☂️", "☂", "⚠", "©"]
        {
            assert_eq!(
                font_presentation_route(text, &mut font_system),
                PresentationRoute::ColorEmoji
            );
        }
        for text in ["☂︎", "⚠︎", "☆", "⏵", "▶", "▲", "■"] {
            assert_eq!(
                font_presentation_route(text, &mut font_system),
                PresentationRoute::TextSymbol
            );
        }
        for text in ["#", "*", "1", "A"] {
            assert_eq!(
                font_presentation_route(text, &mut font_system),
                PresentationRoute::TerminalText
            );
        }
        for text in ["│", "─", "█", "▓", "▒"] {
            assert!(
                procedural::supports_text(text),
                "{text} must bypass font routing"
            );
        }
        assert_eq!(cluster_width("⚠"), 1);
        assert_eq!(cluster_width("☆"), 1);
        assert_eq!(cluster_width("│"), 1);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn fixed_font_database_contains_embedded_noto_and_tolerates_optional_segoe_emoji() {
        let font_system = terminal_font_system();
        let families = font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(family, _)| family.as_str()))
            .collect::<Vec<_>>();
        assert!(families.contains(&COLOR_EMOJI_FONT_FAMILY));
        assert!(families.contains(&TEXT_SYMBOL_FONT_FAMILY));
        let noto = font_system
            .db()
            .faces()
            .find(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| family == COLOR_EMOJI_FONT_FAMILY)
            })
            .unwrap();
        assert!(matches!(noto.source, glyphon::fontdb::Source::Binary(_)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn color_emoji_uses_segoe_for_supported_clusters_and_noto_for_missing_clusters() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let segoe_available = font_family_available(&font_system, SEGOE_COLOR_EMOJI_FONT_FAMILY);
        for (text, uses_segoe_when_available) in
            [("👍🏽", true), ("☂️", true), ("🇺🇸", false), ("👨‍👩‍👧‍👦", true)]
        {
            let mut cell = CapturedCell::plain(text);
            cell.style.flags.insert(CellFlags::WIDE_CHAR);
            let shaped = shape_wide_for_test(&[cell], &mut font_system, metrics);
            let wide = &shaped[0];
            let glyphs = wide
                .buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter())
                .collect::<Vec<_>>();
            let expected_family = if segoe_available && uses_segoe_when_available {
                SEGOE_COLOR_EMOJI_FONT_FAMILY
            } else {
                COLOR_EMOJI_FONT_FAMILY
            };
            let expected_glyph_count = if text == "👨‍👩‍👧‍👦"
                && expected_family == SEGOE_COLOR_EMOJI_FONT_FAMILY
            {
                4
            } else {
                1
            };
            assert_eq!(
                glyphs.len(),
                expected_glyph_count,
                "{text} must keep the accepted cluster composition"
            );
            assert!(
                glyphs.iter().all(|glyph| {
                    glyph.glyph_id != 0 && glyph_family(&font_system, glyph) == expected_family
                }),
                "{text} must use non-.notdef glyphs from the selected family"
            );
            assert_eq!(
                raster_content(&mut font_system, &wide.buffer),
                glyphon::SwashContent::Color,
                "{text} must remain on glyphon's color atlas"
            );
            // A colour emoji's em is fitted to its measured ink, not negotiated through a
            // monospace advance, so the square on the glass is the slot's and not the face's.
            // Where that square sits is `no_color_emoji_glyph_box_leaves_the_cells_it_owns`.
            assert_eq!(wide.buffer.monospace_width(), None);
            let mut swash_cache = SwashCache::new();
            let [left, top, right, bottom] =
                glyph_ink_bounds(&wide.buffer, &mut font_system, &mut swash_cache).unwrap();
            assert!(
                ((right - left).max(bottom - top) - color_emoji_box_px(metrics, 2)).abs() <= 0.5,
                "{text} must be drawn at the square its two cells hold"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn default_text_emoji_fits_the_square_its_one_cell_can_hold() {
        let mut font_system = terminal_font_system();
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            let shaped =
                shape_narrow_for_test(&[CapturedCell::plain("⚠")], &mut font_system, metrics);
            assert_eq!(shaped.len(), 1);
            let glyphs = shaped[0]
                .buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter())
                .collect::<Vec<_>>();
            assert_eq!(glyphs.len(), 1, "⚠ must shape as one glyph");
            assert_ne!(glyphs[0].glyph_id, 0, "⚠ must not be .notdef");
            assert!(
                [SEGOE_COLOR_EMOJI_FONT_FAMILY, COLOR_EMOJI_FONT_FAMILY]
                    .contains(&glyph_family(&font_system, glyphs[0]).as_str()),
                "⚠ must bypass monochrome primary-font coverage"
            );
            assert_eq!(
                raster_content(&mut font_system, &shaped[0].buffer),
                glyphon::SwashContent::Color,
                "⚠ must reach glyphon's color atlas"
            );
            let mut swash_cache = SwashCache::new();
            let [left, top, right, bottom] =
                glyph_ink_bounds(&shaped[0].buffer, &mut font_system, &mut swash_cache).unwrap();
            assert!(
                (right - left).max(bottom - top) - color_emoji_box_px(metrics, 1) <= 0.5,
                "scale {scale_factor}: ⚠ owns one cell and must stay inside it"
            );
            let centered_left = left + shaped[0].left_offset_px;
            let centered_right = right + shaped[0].left_offset_px;
            let centered_top = top + shaped[0].top_offset_px;
            let centered_bottom = bottom + shaped[0].top_offset_px;
            assert!(
                ((centered_left + centered_right) / 2.0 - metrics.cell_width_px / 2.0).abs() <= 0.5
            );
            assert!(
                ((centered_top + centered_bottom) / 2.0 - metrics.cell_height_px / 2.0).abs()
                    <= 0.5
            );
            assert!(centered_top >= -0.5 && centered_bottom <= metrics.cell_height_px + 0.5);
        }
    }

    /// One rule for one- and two-cell colour emoji: the square is `min(cell height, cells × cell
    /// width)` and it is centred on the cells the cluster owns, so no colour emoji ink ever
    /// crosses into a neighbour's column. `⚠` and `☂` are one-cell text-default emoji, the rest
    /// own two.
    #[cfg(target_os = "windows")]
    #[test]
    fn no_color_emoji_glyph_box_leaves_the_cells_it_owns() {
        let mut font_system = terminal_font_system();
        let mut swash_cache = SwashCache::new();
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            for (text, cells) in [
                ("⚠", 1),
                ("☂", 1),
                ("↔\u{fe0f}", 2),
                ("⏱\u{fe0f}", 2),
                ("❤\u{fe0f}", 2),
                ("😀", 2),
                ("👍🏽", 2),
                ("👨‍👩‍👧‍👦", 2),
            ] {
                let mut cell = CapturedCell::plain(text);
                let (buffer, left_offset_px, top_offset_px) = if cells == 2 {
                    cell.style.flags.insert(CellFlags::WIDE_CHAR);
                    let shaped = shape_wide_for_test(&[cell], &mut font_system, metrics);
                    let wide = &shaped[0];
                    (
                        Arc::clone(&wide.buffer),
                        wide.left_offset_px,
                        wide.top_offset_px,
                    )
                } else {
                    let shaped = shape_narrow_for_test(&[cell], &mut font_system, metrics);
                    let narrow = &shaped[0];
                    (
                        Arc::clone(&narrow.buffer),
                        narrow.left_offset_px,
                        narrow.top_offset_px,
                    )
                };
                assert_eq!(
                    raster_content(&mut font_system, &buffer),
                    glyphon::SwashContent::Color,
                    "scale {scale_factor}: {text} must be a colour emoji for this rule to bind"
                );

                let box_px = color_emoji_box_px(metrics, cells);
                assert_eq!(
                    box_px,
                    metrics
                        .cell_height_px
                        .min(cells as f32 * metrics.cell_width_px)
                );
                let slot_width_px = cells as f32 * metrics.cell_width_px;
                let [left, top, right, bottom] =
                    glyph_ink_bounds(&buffer, &mut font_system, &mut swash_cache).unwrap();
                let placed = [
                    left + left_offset_px,
                    top + top_offset_px,
                    right + left_offset_px,
                    bottom + top_offset_px,
                ];
                assert!(
                    placed[2] - placed[0] <= box_px + 0.5 && placed[3] - placed[1] <= box_px + 0.5,
                    "scale {scale_factor}: {text} must fit a {box_px}px square, got {placed:?}"
                );
                assert!(
                    placed[0] >= -0.5
                        && placed[2] <= slot_width_px + 0.5
                        && placed[1] >= -0.5
                        && placed[3] <= metrics.cell_height_px + 0.5,
                    "scale {scale_factor}: {text} must stay inside its {cells} cell(s), got {placed:?}"
                );
                assert!(
                    ((placed[0] + placed[2]) / 2.0 - slot_width_px / 2.0).abs() <= 0.5
                        && ((placed[1] + placed[3]) / 2.0 - metrics.cell_height_px / 2.0).abs()
                            <= 0.5,
                    "scale {scale_factor}: {text} must be centred on its cells, got {placed:?}"
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn missing_segoe_emoji_degrades_every_color_cluster_to_embedded_noto() {
        let mut font_system = terminal_font_system();
        let segoe_faces = font_system
            .db()
            .faces()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(family, _)| family == SEGOE_COLOR_EMOJI_FONT_FAMILY)
            })
            .map(|face| face.id)
            .collect::<Vec<_>>();
        for face in segoe_faces {
            font_system.db_mut().remove_face(face);
        }
        assert!(!font_family_available(
            &font_system,
            SEGOE_COLOR_EMOJI_FONT_FAMILY
        ));

        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        for text in ["👍🏽", "☂️", "🇺🇸", "👨‍👩‍👧‍👦"] {
            let mut cell = CapturedCell::plain(text);
            cell.style.flags.insert(CellFlags::WIDE_CHAR);
            let shaped = shape_wide_for_test(&[cell], &mut font_system, metrics);
            let glyph = first_layout_glyph(&shaped[0].buffer);
            assert_ne!(glyph.glyph_id, 0);
            assert_eq!(glyph_family(&font_system, &glyph), COLOR_EMOJI_FONT_FAMILY);
            assert_eq!(
                raster_content(&mut font_system, &shaped[0].buffer),
                glyphon::SwashContent::Color
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wide_shaping_cache_hits_do_not_repeat_color_emoji_trial_shapes() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut cell = CapturedCell::plain("👍🏽");
        cell.style.flags.insert(CellFlags::WIDE_CHAR);
        let cells = [cell];
        let mut swash_cache = SwashCache::new();
        let mut cache = WideShapingCache::new();

        let first = shape_wide_glyphs(
            &cells,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 1);
        let trials_after_miss = cache.color_emoji_trial_shapes;
        assert_eq!(
            trials_after_miss,
            u64::from(font_family_available(
                &font_system,
                SEGOE_COLOR_EMOJI_FONT_FAMILY
            ))
        );

        let mut recolored = cells.clone();
        recolored[0].style.foreground = TerminalColor::Rgb(255, 0, 0);
        let second = shape_wide_glyphs(
            &recolored,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.color_emoji_trial_shapes, trials_after_miss);
        assert!(Arc::ptr_eq(&first[0].buffer, &second[0].buffer));
        assert_ne!(first[0].color, second[0].color);

        let mut bold = cells.clone();
        bold[0].style.flags.insert(CellFlags::BOLD);
        let bold = shape_wide_glyphs(
            &bold,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.color_emoji_trial_shapes, 2 * trials_after_miss);
        assert!(!Arc::ptr_eq(&first[0].buffer, &bold[0].buffer));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn vs15_and_non_emoji_symbols_stay_monochrome_and_inside_narrow_cells() {
        let mut font_system = terminal_font_system();
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            let glyphs = shape_narrow_for_test(
                &[
                    CapturedCell::plain("☂︎"),
                    CapturedCell::plain("⚠︎"),
                    CapturedCell::plain("☆"),
                ],
                &mut font_system,
                metrics,
            );
            assert_eq!(glyphs.len(), 3);
            for glyph in &glyphs {
                let layout = first_layout_glyph(&glyph.buffer);
                assert_ne!(layout.glyph_id, 0);
                assert_eq!(glyph_family(&font_system, &layout), TEXT_SYMBOL_FONT_FAMILY);
                assert_eq!(
                    raster_content(&mut font_system, &glyph.buffer),
                    glyphon::SwashContent::Mask
                );
                assert!(
                    occupied_width_px(&mut font_system, &glyph.buffer) <= metrics.cell_width_px,
                    "scale {scale_factor}, column {} fallback ink/advance must fit one cell",
                    glyph.column
                );
            }

            let star = first_layout_glyph(&glyphs[2].buffer);
            assert!(
                star.font_size < metrics.font_size_px,
                "scale {scale_factor}: fallback star must be em-normalized"
            );
            for character in ['☆', '·', '•'] {
                assert!(!is_text_coordinated_symbol(character));
            }

            assert!(procedural::supports_text("│"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn pinned_media_and_geometric_symbols_match_primary_cap_height_and_center() {
        let mut font_system = terminal_font_system();
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            for text in ["⏵", "▶"] {
                let shaped =
                    shape_narrow_for_test(&[CapturedCell::plain(text)], &mut font_system, metrics);
                let glyph = &shaped[0];
                let layout = first_layout_glyph(&glyph.buffer);
                assert_ne!(layout.glyph_id, 0, "{text} must not be .notdef");
                assert_eq!(
                    glyph_family(&font_system, &layout),
                    TEXT_SYMBOL_FONT_FAMILY,
                    "{text} must use the monochrome symbol face"
                );
                let mut swash_cache = SwashCache::new();
                let [left, top, right, bottom] =
                    glyph_ink_bounds(&glyph.buffer, &mut font_system, &mut swash_cache).unwrap();
                let ink_width = right - left;
                let ink_height = bottom - top;
                assert!(
                    (ink_height - metrics.primary_cap_height_px).abs() <= 1.0,
                    "scale {scale_factor}: {text} ink height {ink_height} must match primary cap height {}",
                    metrics.primary_cap_height_px
                );
                assert!(
                    ink_width <= metrics.cell_width_px + 1.0,
                    "scale {scale_factor}: {text} ink width {ink_width} must remain inside one cell"
                );
                let centered_x = (left + right) / 2.0 + glyph.left_offset_px;
                let centered_y = (top + bottom) / 2.0 + glyph.top_offset_px;
                assert!((centered_x - metrics.cell_width_px / 2.0).abs() <= 0.5);
                assert!((centered_y - metrics.primary_cap_center_y_px).abs() <= 0.5);
            }
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn box_drawing_and_block_elements_bypass_shaping_and_the_glyph_cache() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let mut swash_cache = SwashCache::new();
        let mut cache = NarrowShapingCache::new();

        for text in ["─", "│", "┌", "╬", "╭", "█", "▀", "▒"] {
            let cells = (0..8)
                .map(|_| CapturedCell::plain(text))
                .collect::<Vec<_>>();
            let glyphs = shape_narrow_glyphs(
                &cells,
                &mut font_system,
                &mut swash_cache,
                metrics,
                &mut cache,
            );
            assert!(glyphs.is_empty(), "{text} must not enter shaping");
            assert!(
                cache.entries.is_empty(),
                "{text} must not enter the atlas cache"
            );

            let mut malformed_wide = CapturedCell::plain(text);
            malformed_wide.style.flags.insert(CellFlags::WIDE_CHAR);
            assert!(
                shape_wide_for_test(&[malformed_wide], &mut font_system, metrics).is_empty(),
                "{text} must have programmatic priority even if the grid marks it wide"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn primary_italic_glyph_is_not_em_normalized() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut cell = CapturedCell::plain("f");
        cell.style.flags.insert(CellFlags::ITALIC);
        let glyphs = shape_narrow_for_test(&[cell], &mut font_system, metrics);
        let glyph = first_layout_glyph(&glyphs[0].buffer);
        assert_eq!(
            glyph_family(&font_system, &glyph),
            DEFAULT_PRIMARY_FONT_FAMILY
        );
        assert_eq!(glyph.font_size, metrics.font_size_px);
    }

    #[test]
    fn grid_dimensions_are_nonzero_and_derived_from_metrics() {
        let metrics = CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 5.0,
            scale_factor: 1.0,
            ascii_baseline_px: 16.0,
            primary_advance_px: 10.0,
            primary_cap_height_px: 12.0,
            primary_cap_center_y_px: 10.0,
        };
        assert_eq!(
            metrics.grid_for_pixels(810, 490),
            GridSize {
                columns: NonZeroU16::new(80).unwrap(),
                rows: NonZeroU16::new(24).unwrap(),
            }
        );
        assert_eq!(metrics.grid_for_pixels(0, 0).columns.get(), 1);
    }

    #[test]
    fn status_overlay_uses_real_frame_state_and_never_occupies_the_bottom_grid_row() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut swash_cache = SwashCache::new();
        let mut narrow_cache = NarrowShapingCache::new();
        let mut wide_cache = WideShapingCache::new();
        let mut row_cache = ComposedRowCache::new();
        let mut text_rows = Vec::new();
        let mut status_overlay = None;
        let columns = 16_usize;
        let rows = 3_usize;
        let mut cells = vec![CapturedCell::default(); columns * rows];
        for (cell, character) in cells[(rows - 1) * columns..]
            .iter_mut()
            .zip("bottom-line".chars())
        {
            *cell = CapturedCell::plain(character.to_string());
        }
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(columns as u32).unwrap(),
            grid_rows: NonZeroU32::new(rows as u32).unwrap(),
            rows: NonZeroU32::new(rows as u32).unwrap(),
            presentation_offset_subpixels: 0,
            cells,
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
            cell_anchors: test_cell_anchors(columns * rows),
            row_map: test_row_map(rows as u32),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(columns as u32),
            view_generation: bt_doc::ViewGeneration(1),
        };
        prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(text_rows.len(), rows);
        assert!(status_overlay.is_none(), "no overflow means no overlay row");
        let bottom_row = Arc::clone(&text_rows[rows - 1]);

        frame.status_text = Some("2 rows above".to_owned());
        prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        let overlay = status_overlay.as_ref().expect("overflow shapes an overlay");
        assert_eq!(text_rows.len(), rows, "overlay is not a synthetic grid row");
        assert!(Arc::ptr_eq(&text_rows[rows - 1], &bottom_row));
        assert!(!overlay.narrow_glyphs.is_empty());
        assert_eq!(
            frame.cells[(rows - 1) * columns..]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>(),
            "bottom-line"
        );
        let geometry = status_overlay_geometry(
            metrics,
            &frame,
            "2 rows above",
            fitting_seat_width(metrics, &frame),
        )
        .unwrap();
        let bottom_row_top = frame_cell_bounds_px(metrics, &frame, rows - 1, 0)[1];
        assert!(
            geometry.rect[3] <= bottom_row_top,
            "independent overlay {:?} must not cover the bottom row starting at {bottom_row_top}",
            geometry.rect
        );

        frame.status_text = None;
        prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert!(
            status_overlay.is_none(),
            "clearing overflow removes the overlay"
        );
    }

    #[test]
    fn metrics_and_ime_client_rect_apply_the_reported_dpi_scale() {
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            let mut font_system = terminal_font_system();
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            assert_eq!(metrics.scale_factor, scale_factor);
            assert_eq!(
                metrics.font_size_px,
                DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX * scale_factor as f32
            );
            assert_eq!(
                metrics.cell_height_px,
                (DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX
                    * LINE_HEIGHT_TO_FONT_SIZE_RATIO
                    * scale_factor as f32)
                    .ceil(),
                "the row height is a ratio of the face size now, and at the                  default size it must still be the 22 logical pixels it always was"
            );
            assert_eq!(
                DEFAULT_TERMINAL_FONT_SIZE_LOGICAL_PX * LINE_HEIGHT_TO_FONT_SIZE_RATIO,
                22.0
            );

            let cursor = bt_viewport::GridCursor {
                row: 2,
                column: 3,
                visible: true,
            };
            let frame = ViewportFrame {
                columns: NonZeroU32::new(4).unwrap(),
                grid_rows: NonZeroU32::new(3).unwrap(),
                rows: NonZeroU32::new(3).unwrap(),
                presentation_offset_subpixels: 0,
                cells: vec![CapturedCell::default(); 12],
                cursor,
                cell_anchors: test_cell_anchors(12),
                row_map: test_row_map_for_metrics(3, metrics),
                selection_spans: Vec::new(),
                search_spans: Vec::new(),
                current_search_spans: Vec::new(),
                math_blocks: Vec::new(),
                math_failures: Vec::new(),
                status_text: None,
                viewport_origin: FrameViewportOrigin::Bottom,
                scroll_offset_rows: 0,
                layout_key: bt_doc_layout_key(4),
                view_generation: bt_doc::ViewGeneration(1),
            };
            let area = ime_cursor_area_for_metrics(metrics, &frame);
            let bounds = cell_bounds_px(metrics, 2, 3);
            assert_eq!(area.x, bounds[0].floor() as i32);
            assert_eq!(area.y, bounds[1].floor() as i32);
            assert_eq!(area.width, (bounds[2].ceil() - bounds[0].floor()) as u32);
            assert_eq!(area.height, (bounds[3].ceil() - bounds[1].floor()) as u32);
        }
    }

    #[test]
    fn expanded_live_prefix_places_cursor_ime_and_selection_on_one_vertical_axis() {
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 18.0,
            font_size_px: 14.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 12.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        let unit = SUBPIXELS_PER_PX;
        let frame = ViewportFrame {
            columns: NonZeroU32::new(2).unwrap(),
            grid_rows: NonZeroU32::new(4).unwrap(),
            rows: NonZeroU32::new(4).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain("x"); 8],
            cursor: bt_viewport::GridCursor {
                row: 2,
                column: 1,
                visible: true,
            },
            cell_anchors: test_cell_anchors(8),
            row_map: vec![
                bt_viewport::FrameVisualRow {
                    top_subpixels: -22 * unit,
                    height_subpixels: 40 * unit,
                    live_grid_row: Some(0),
                    continues: false,
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 18 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(1),
                    continues: false,
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 36 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(2),
                    continues: false,
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 54 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(3),
                    continues: false,
                },
            ],
            selection_spans: vec![bt_viewport::SelectionSpan {
                row: 2,
                start_column: 0,
                end_column: 2,
            }],
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(2),
            view_generation: bt_doc::ViewGeneration(2),
        };

        let row_bounds = frame_cell_bounds_px(metrics, &frame, 2, 0);
        assert_eq!(row_bounds, [4.0, 40.0, 12.0, 58.0]);
        assert_eq!(
            metrics.hit_test_frame(&frame, 13.0, 41.0),
            Some(GridHit { row: 2, column: 1 })
        );
        let ime = ime_cursor_area_for_metrics(metrics, &frame);
        assert_eq!((ime.x, ime.y, ime.width, ime.height), (12, 40, 8, 18));
        assert_eq!(
            cursor_pixel_bounds(metrics, &frame, true),
            vec![[12.0, 40.0, 13.0, 58.0]],
            "the caret is the thin bar and starts at the cell it is in"
        );
        assert_eq!(
            frame_cell_bounds_px(
                metrics,
                &frame,
                frame.selection_spans[0].row as usize,
                frame.selection_spans[0].start_column as usize,
            )[1],
            40.0,
            "selection rectangles consume the same frame row prefix"
        );
    }

    /// A drag pulled past a pane's edges names the cell at that edge, where the
    /// plain hit test correctly names nothing at all.
    ///
    /// The two answers differ over three kinds of point, and every one of them is
    /// somewhere a selection drag routinely goes: the grid's own margin, the
    /// slack between the last column and the pane's right edge, and everything
    /// below the last row. A drag clamped only to the pane's *box* lands on all
    /// three, is told there is no cell there, and stops following the hand — a
    /// column short of the end of the line, or a row short of the end of the
    /// pane.
    #[test]
    fn a_gesture_past_the_grids_edges_names_the_edge_cell_where_a_hover_names_nothing() {
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 18.0,
            font_size_px: 14.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 12.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        let frame = ViewportFrame {
            columns: NonZeroU32::new(3).unwrap(),
            grid_rows: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(2).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain("x"); 6],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            cell_anchors: test_cell_anchors(6),
            row_map: test_row_map_for_metrics(2, metrics),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(3),
            view_generation: bt_doc::ViewGeneration(1),
        };
        // The grid occupies x 4..28 and y 4..40; a pane holding it is wider and
        // taller than that, and the rest is margin and slack.
        let last = GridHit { row: 1, column: 2 };

        // Inside the grid the two agree, which is what keeps this a clamp and not
        // a second hit test.
        for (x, y) in [(5.0, 5.0), (13.0, 25.0), (27.0, 39.0)] {
            assert_eq!(
                metrics.clamped_hit_test_frame(&frame, x, y),
                metrics.hit_test_frame(&frame, x, y),
                "({x}, {y}) is over a cell, so both answer it"
            );
        }

        for (what, x, y, expected) in [
            ("the slack past the last column", 31.0, 39.0, last),
            ("below the last row", 27.0, 400.0, last),
            ("past the bottom-right corner", 999.0, 999.0, last),
            ("the left margin", 0.0, 5.0, GridHit { row: 0, column: 0 }),
            (
                "above the first row",
                5.0,
                0.0,
                GridHit { row: 0, column: 0 },
            ),
            (
                "past the top-left corner",
                -999.0,
                -999.0,
                GridHit { row: 0, column: 0 },
            ),
        ] {
            assert_eq!(
                metrics.hit_test_frame(&frame, x, y),
                None,
                "{what} is not over a cell — if it were, this test would be \
                 measuring nothing"
            );
            assert_eq!(
                metrics.clamped_hit_test_frame(&frame, x, y),
                Some(expected),
                "a drag reaching {what} means the cell at that edge"
            );
        }
    }

    #[test]
    fn ime_candidate_anchor_clips_with_partial_first_and_overscan_cursors() {
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 18.0,
            font_size_px: 14.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 12.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        let unit = SUBPIXELS_PER_PX;
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(2).unwrap(),
            grid_rows: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(3).unwrap(),
            presentation_offset_subpixels: 7 * unit,
            cells: vec![CapturedCell::plain("x"); 6],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 1,
                visible: true,
            },
            cell_anchors: test_cell_anchors(6),
            row_map: vec![
                bt_viewport::FrameVisualRow {
                    top_subpixels: -7 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(0),
                    continues: false,
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 11 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(1),
                    continues: false,
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 29 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: None,
                    continues: false,
                },
            ],
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(2),
            view_generation: bt_doc::ViewGeneration(3),
        };
        frame.validate_shape().unwrap();

        let first = ime_cursor_area_for_metrics(metrics, &frame);
        assert_eq!(
            (first.x, first.y, first.width, first.height),
            (12, 4, 8, 11)
        );

        frame.cursor.row = 2;
        let overscan = ime_cursor_area_for_metrics(metrics, &frame);
        assert_eq!(
            (overscan.x, overscan.y, overscan.width, overscan.height),
            (12, 33, 8, 7)
        );
    }

    #[test]
    fn real_math_decoration_keeps_inter_block_selection_in_its_own_row_map_band() {
        let started = std::time::Instant::now();
        let mut session = bt_term::DualPlaneSession::new(
            NonZeroU32::new(40).unwrap(),
            NonZeroU32::new(24).unwrap(),
        );
        session
            .feed_at(
                // The application writes its own last row, as a full-screen TUI does: with no blank
                // live tail the bottom relief is zero, so the four inflated bands keep the classic
                // cut-at-top frame this fixture needs (a blank tail would instead yield pane at the
                // bottom and reveal them completely — see bt-viewport `continuous_frame`).
                b"\x1b[?1049h$$x0$$\r\nafter-0\r\n$$x1$$\r\nafter-1\r\n$$x2$$\r\nafter-2\r\n$$x3$$\r\nafter-3\x1b[24;1Hstatus-row",
                started,
            )
            .unwrap();
        assert_eq!(
            session.advance_live_stability(started + bt_term::LIVE_MATH_STABLE_INTERVAL),
            4
        );
        let raster = bt_math::MathRaster {
            rgba: vec![255; 40 * 40 * 4],
            width_px: 40,
            height_px: 40,
            content_height_px: 24,
            ascent_px: 20.0,
            descent_px: 4.0,
            baseline_px: 20.0,
            render_time: std::time::Duration::from_millis(1),
            inline_runs: Vec::new(),
        };
        let mut completed = 0;
        while let Some(mut task) = session.take_live_worker_task() {
            assert!(bt_detect::resolve_live_detection_task(&mut task));
            assert!(session.complete_live_worker_result(task, Ok(raster.clone())));
            completed += 1;
        }
        assert_eq!(completed, 4);

        let mut projection = session.new_projection(session.layout_key());
        let unselected = session.viewport_frame(&mut projection).unwrap();
        let rendered_bands = unselected
            .math_blocks
            .iter()
            .map(|block| match block.anchor {
                MathBlockAnchor::Live {
                    band_start_row,
                    band_end_row,
                    ..
                } => (band_start_row, band_end_row),
                MathBlockAnchor::History { .. } => panic!("fixture must stay on the live plane"),
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered_bands, [(0, 0), (2, 2), (4, 4), (6, 6)]);
        assert!(
            unselected.row_map[0].top_subpixels < 0,
            "four free-height blocks must exercise the negative-top frame"
        );
        let start = unselected
            .anchor_at(0, 0, bt_doc::Bias::Before)
            .unwrap()
            .unwrap();
        let end = unselected
            .anchor_at(7, 39, bt_doc::Bias::After)
            .unwrap()
            .unwrap();
        session.set_view_selection(Some(bt_viewport::ViewSelection { start, end }));
        session.refresh_projection(&mut projection);

        let selected = session.viewport_frame(&mut projection).unwrap();
        assert_eq!(
            selected
                .selection_spans
                .iter()
                .map(|span| span.row)
                .collect::<Vec<_>>(),
            [1, 3, 5, 7],
            "only the four rendered band rows may be suppressed"
        );
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 18.0,
            font_size_px: 14.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 12.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        let status = selected
            .status_text
            .as_deref()
            .expect("negative-top live frame must publish rows-above status");
        assert!(status.ends_with(" rows above · Shift+wheel"));
        let terminal_last_row = session.terminal().visible_row(23).unwrap().cells;
        assert_eq!(
            &selected.cells[23 * 40..24 * 40],
            terminal_last_row.as_slice(),
            "the independent overflow overlay must not rewrite final-row cells"
        );
        let status_geometry = status_overlay_geometry(
            metrics,
            &selected,
            status,
            fitting_seat_width(metrics, &selected),
        )
        .unwrap();
        let last_row_top = frame_cell_bounds_px(metrics, &selected, 23, 0)[1];
        assert!(
            status_geometry.rect[3] <= last_row_top,
            "overflow overlay {:?} intersects final row starting at {last_row_top}",
            status_geometry.rect
        );
        for span in &selected.selection_spans {
            let rect = selection_span_bounds_px(
                metrics,
                &selected,
                span,
                span.start_column as usize,
                span.end_column.saturating_sub(span.start_column) as usize,
            );
            let mapped = selected.row_map[span.row as usize];
            let row_top =
                metrics.padding_px + mapped.top_subpixels as f32 / SUBPIXELS_PER_PX as f32;
            let row_bottom = metrics.padding_px
                + mapped.top_subpixels.saturating_add(mapped.height_subpixels) as f32
                    / SUBPIXELS_PER_PX as f32;
            assert!(
                row_top <= rect[1] && rect[3] <= row_bottom,
                "selection row {} rect {rect:?} escaped row_map [{row_top}, {row_bottom})",
                span.row
            );
        }
    }

    #[test]
    fn latest_frame_slot_overwrites_instead_of_queueing() {
        let frame = ViewportFrame {
            columns: NonZeroU32::new(1).unwrap(),
            grid_rows: NonZeroU32::new(1).unwrap(),
            rows: NonZeroU32::new(1).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain("a")],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            cell_anchors: test_cell_anchors(1),
            row_map: test_row_map(1),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(1),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let mut slot = LatestFrameSlot::default();
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        slot.publish(frame.clone(), trigger).unwrap();
        slot.publish(frame, trigger).unwrap();
        assert_eq!(slot.overwrites(), 1);
        assert_eq!(slot.pending_frame().unwrap().cells[0].text, "a");
        assert!(slot.take().is_some());
        assert!(slot.pending_frame().is_none());
        assert!(slot.take().is_none());
    }

    #[test]
    fn frame_content_digest_is_stable_for_known_and_blank_frames() {
        let make_frame = |columns: u32, rows: u32, cells: Vec<CapturedCell>| ViewportFrame {
            columns: NonZeroU32::new(columns).unwrap(),
            grid_rows: NonZeroU32::new(rows).unwrap(),
            rows: NonZeroU32::new(rows).unwrap(),
            presentation_offset_subpixels: 0,
            cell_anchors: test_cell_anchors(cells.len()),
            row_map: test_row_map(rows),
            cells,
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(columns),
            view_generation: bt_doc::ViewGeneration(1),
        };

        let mut colored = CapturedCell::plain("A");
        colored.style.foreground = TerminalColor::Rgb(1, 2, 3);
        colored.style.background = TerminalColor::Indexed(4);
        let known = make_frame(
            3,
            3,
            vec![
                CapturedCell::plain(""),
                CapturedCell::plain(" "),
                colored,
                CapturedCell::plain("  "),
                CapturedCell::plain(""),
                CapturedCell::plain("你"),
                CapturedCell::plain(""),
                CapturedCell::plain(""),
                CapturedCell::plain(""),
            ],
        );
        assert_eq!(
            frame_content_digest(&known),
            FrameContentDigest {
                nonblank_cells: 2,
                first_text_row: 0,
                last_text_row: 1,
                content_fnv: 0x154a_541c_8466_f6df,
            }
        );

        let blank = make_frame(2, 2, vec![CapturedCell::plain(""); 4]);
        let blank_digest = frame_content_digest(&blank);
        assert_eq!(blank_digest.nonblank_cells, 0);
        assert_eq!(blank_digest.first_text_row, -1);
        assert_eq!(blank_digest.last_text_row, -1);
    }

    /// PIN (grey-band root fix, 2026-08-02): the hover dim never outlives the raster it dims.
    ///
    /// A rendered block whose texture did not draw must not draw its scrim either: the scrim alone
    /// over background IS the bare grey rectangle the user reported. Source blocks are unaffected
    /// — they draw as terminal text, own no texture, and projection deliberately lets them carry
    /// `toolbar_visible`.
    #[test]
    fn the_hover_dim_is_drawn_only_over_a_block_that_put_pixels_on_screen() {
        let mut rendered = test_math_placement("k", 0, 20 * SUBPIXELS_PER_PX, 16);
        let mut source = rendered.clone();
        source.display = MathBlockDisplay::Source;

        assert!(
            !math_block_dim_is_drawn(&rendered, true),
            "no hover, no dim"
        );
        assert!(
            !math_block_dim_is_drawn(&source, true),
            "no hover, no dim for a source block either",
        );

        rendered.toolbar_visible = true;
        source.toolbar_visible = true;
        assert!(
            math_block_dim_is_drawn(&rendered, true),
            "a hovered block that drew its raster dims it",
        );
        assert!(
            !math_block_dim_is_drawn(&rendered, false),
            "a hovered block whose texture never drew must not paint a bare scrim",
        );
        assert!(
            math_block_dim_is_drawn(&source, false),
            "a source block owns no texture; its hover dim is over its own text",
        );
    }

    /// PIN (grey-band root fix, 2026-08-02): an off-screen band never evicts an on-screen band's
    /// texture.
    ///
    /// `prepare_math_draws` used to upload and admit a block's texture BEFORE asking whether the
    /// block was on screen. With the shared byte budget near full, the scrolled-away band's upload
    /// evicted the visible band's texture; the visible band then found nothing resident, skipped
    /// its quad, and drew only its placement and hover dim — a bare grey rectangle. Visibility is
    /// now the first question both preparation paths ask, so this reproduces the eviction against
    /// the real LRU and proves the decision that prevents it.
    #[test]
    fn an_offscreen_band_does_not_evict_an_onscreen_bands_texture() {
        let rows = 4_u32;
        let cell = 20 * SUBPIXELS_PER_PX;
        let block_bytes = 6 * 1024 * 1024;
        let frame = ViewportFrame {
            columns: NonZeroU32::new(4).unwrap(),
            grid_rows: NonZeroU32::new(rows).unwrap(),
            rows: NonZeroU32::new(rows).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain("x"); 4 * rows as usize],
            cell_anchors: test_cell_anchors(4 * rows as usize),
            row_map: (0..rows)
                .map(|row| bt_viewport::FrameVisualRow {
                    top_subpixels: i64::from(row) * cell,
                    height_subpixels: cell,
                    live_grid_row: Some(row),
                    continues: false,
                })
                .collect(),
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: vec![
                test_math_placement("onscreen", 0, cell, block_bytes),
                // Two full grid heights above the pane: scrolled away, nothing of it is drawable.
                test_math_placement("offscreen", -2 * i64::from(rows) * cell, cell, block_bytes),
            ],
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(4),
            view_generation: bt_doc::ViewGeneration(1),
        };
        frame.validate_shape().unwrap();

        assert!(
            math_block_admits_texture(&frame, &frame.math_blocks[0]),
            "the on-screen band is the one that needs a texture",
        );
        assert!(
            !math_block_admits_texture(&frame, &frame.math_blocks[1]),
            "a band with no drawable pixels must not compete for the texture budget",
        );

        // The real LRU, sized so the pair cannot coexist: admitting the off-screen block is exactly
        // one eviction of the on-screen block, and its quad then has nothing to sample.
        let mut cache = ByteLru::<String, ()>::new(block_bytes + block_bytes / 2);
        let mut evictions = 0_u64;
        for placement in &frame.math_blocks {
            if !math_block_admits_texture(&frame, placement) {
                continue;
            }
            let (admitted, evicted) = cache.insert(
                placement.artifact.key.clone(),
                (),
                placement.artifact.rgba.len(),
            );
            assert!(admitted);
            evictions += evicted;
        }
        assert_eq!(evictions, 0, "no visible band may be evicted by this frame");
        assert!(
            cache.get(&"onscreen".to_owned()).is_some(),
            "the on-screen band's texture stayed resident",
        );

        // Same LRU, same frame, with visibility ignored the way the defect did: the on-screen
        // band's texture is gone before it is ever drawn.
        let mut unordered = ByteLru::<String, ()>::new(block_bytes + block_bytes / 2);
        let mut unordered_evictions = 0_u64;
        for placement in &frame.math_blocks {
            let (_, evicted) = unordered.insert(
                placement.artifact.key.clone(),
                (),
                placement.artifact.rgba.len(),
            );
            unordered_evictions += evicted;
        }
        assert_eq!(unordered_evictions, 1);
        assert!(
            unordered.get(&"onscreen".to_owned()).is_none(),
            "this is the defect: the off-screen band evicted the band on screen",
        );
    }

    #[test]
    fn zero_offset_overscan_has_the_same_pixel_draw_inputs_as_the_legacy_rectangle() {
        let columns = 2_usize;
        let grid_rows = 2_usize;
        let legacy = ViewportFrame {
            columns: NonZeroU32::new(columns as u32).unwrap(),
            grid_rows: NonZeroU32::new(grid_rows as u32).unwrap(),
            rows: NonZeroU32::new(grid_rows as u32).unwrap(),
            presentation_offset_subpixels: 0,
            cells: ["a", "b", "c", "d"]
                .into_iter()
                .map(CapturedCell::plain)
                .collect(),
            cursor: bt_viewport::GridCursor {
                row: 1,
                column: 1,
                visible: true,
            },
            cell_anchors: test_cell_anchors(columns * grid_rows),
            row_map: test_row_map(grid_rows as u32),
            selection_spans: vec![SelectionSpan {
                row: 1,
                start_column: 0,
                end_column: 2,
            }],
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(columns as u32),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let mut overscan = legacy.clone();
        overscan.rows = NonZeroU32::new((grid_rows + 1) as u32).unwrap();
        let mut dangerous = CapturedCell::plain("OVERSCAN");
        dangerous.style.background = TerminalColor::Rgb(255, 0, 0);
        overscan.cells.extend([dangerous.clone(), dangerous]);
        overscan.cell_anchors.extend(test_cell_anchors(columns));
        overscan.row_map.push(bt_viewport::FrameVisualRow {
            top_subpixels: grid_rows as i64 * 22 * SUBPIXELS_PER_PX,
            height_subpixels: 22 * SUBPIXELS_PER_PX,
            live_grid_row: None,
            continues: false,
        });
        overscan.cursor.row = grid_rows as u32;
        overscan.selection_spans.push(SelectionSpan {
            row: grid_rows as u32,
            start_column: 0,
            end_column: columns as u32,
        });

        legacy.validate_shape().unwrap();
        overscan.validate_shape().unwrap();
        assert_eq!(legacy.drawable_rows(), overscan.drawable_rows());
        assert_eq!(
            frame_content_digest(&legacy),
            frame_content_digest(&overscan)
        );
        assert!(!overscan.drawable_interval_overlaps(
            overscan.row_map[grid_rows].top_subpixels,
            overscan.row_map[grid_rows].height_subpixels,
        ));

        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut swash_cache = SwashCache::new();
        let mut narrow_cache = NarrowShapingCache::new();
        let mut wide_cache = WideShapingCache::new();
        let mut row_cache = ComposedRowCache::new();
        let mut legacy_rows = Vec::new();
        let mut legacy_status = None;
        prepare_text_rows(
            &legacy,
            metrics,
            &mut legacy_rows,
            &mut legacy_status,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        let mut overscan_rows = Vec::new();
        let mut overscan_status = None;
        prepare_text_rows(
            &overscan,
            metrics,
            &mut overscan_rows,
            &mut overscan_status,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();

        assert_eq!(legacy_rows.len(), grid_rows);
        assert_eq!(overscan_rows.len(), grid_rows);
        assert!(
            legacy_rows
                .iter()
                .zip(&overscan_rows)
                .all(|(legacy, overscan)| Arc::ptr_eq(legacy, overscan)),
            "the renderer must reuse the exact shaped rows; hidden overscan contributes no pixels"
        );
    }

    #[test]
    fn publish_composition_and_text_row_boundary_reject_non_rectangular_frames() {
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(2).unwrap(),
            grid_rows: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(2).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain(""); 4],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            cell_anchors: test_cell_anchors(4),
            row_map: test_row_map(2),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(2),
            view_generation: bt_doc::ViewGeneration(1),
        };
        frame.cells.pop();
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Resize,
        };

        assert!(
            LatestFrameSlot::default()
                .publish(frame.clone(), trigger)
                .is_err()
        );
        assert!(compose_preedit(&frame, None).is_err());
        assert!(matches!(
            text_row_cells(&frame),
            Err(FrameShapeError::CellCount {
                expected: 4,
                actual: 3,
            })
        ));

        frame.cells.push(CapturedCell::plain(""));
        frame.layout_key = bt_doc_layout_key(1);
        assert!(matches!(
            text_row_cells(&frame),
            Err(FrameShapeError::LayoutWidth {
                frame: 2,
                layout: 1,
            })
        ));
    }

    fn bt_doc_layout_key(width_cells: u32) -> bt_doc::LayoutKey {
        bt_doc::LayoutKey {
            width_cells: NonZeroU32::new(width_cells).unwrap(),
            dpi_milli: NonZeroU32::new(1000).unwrap(),
            font_rev: 1,
            theme_rev: 1,
            lang_rev: 0,
            profile_rev: 0,
        }
    }

    #[test]
    fn timing_gate_rejects_inverted_boundaries() {
        let now = Instant::now();
        let receipt = PresentReceipt {
            trigger: FrameTrigger {
                occurred_at: now,
                source: FrameSource::Keyboard,
            },
            submitted_at: now + Duration::from_millis(2),
            present_called_at: now + Duration::from_millis(1),
        };
        assert_eq!(receipt.latency(), Err(TimingError::InvertedTimestamp));
    }

    #[test]
    fn sgr_palette_and_inverse_are_resolved_before_rendering() {
        let style = CellStyle {
            flags: CellFlags::INVERSE,
            foreground: TerminalColor::Named(1),
            background: TerminalColor::Named(4),
        };
        assert_eq!(resolve_colors(&style), (indexed_color(4), indexed_color(1)));
        assert_ne!(indexed_color(196), indexed_color(21));
    }

    #[test]
    fn campbell_defaults_and_explicit_ansi_palette_keep_distinct_color_paths() {
        assert_eq!(
            theme::parse_background_rgb("#123aBC"),
            Some([0x12, 0x3a, 0xbc])
        );
        for invalid in ["123abc", "#123ab", "#123abcd", "#12xz89", "＃123abc"] {
            assert_eq!(theme::parse_background_rgb(invalid), None);
        }
        assert_eq!(DEFAULT_BACKGROUND_RGB, [0x1b, 0x1b, 0x1b]);
        let expected_background = std::env::var("BT_BG")
            .ok()
            .and_then(|value| theme::parse_background_rgb(&value))
            .unwrap_or(DEFAULT_BACKGROUND_RGB);
        assert_eq!(default_background(), expected_background);
        assert_eq!(default_foreground(), foreground_rgb());
        assert_eq!(
            theme::cursor_for_background(DEFAULT_BACKGROUND_RGB),
            [0xd4, 0xd4, 0xd4]
        );
        assert_eq!(
            terminal_color(TerminalColor::Named(18), true),
            cursor_rgb(),
            "the cursor quad and cursor named color share the mock-up cursor"
        );
        for (index, expected) in ansi_16_rgb().iter().copied().enumerate() {
            assert_eq!(indexed_color(index as u8), expected);
        }

        assert_eq!(
            terminal_color(TerminalColor::Named(16), true),
            default_foreground(),
            "SGR 39/default foreground must resolve through the theme default"
        );
        assert_eq!(
            terminal_color(TerminalColor::Named(17), false),
            default_background(),
            "SGR 49/default background must resolve through the theme default"
        );
        assert_eq!(
            terminal_color(TerminalColor::Named(0), true),
            ansi_16_rgb()[0],
            "explicit ANSI black must resolve through palette slot 0"
        );
        assert_eq!(
            terminal_color(TerminalColor::Indexed(15), true),
            ansi_16_rgb()[15],
            "indexed ANSI bright white must resolve through palette slot 15"
        );
    }

    #[test]
    fn dotted_underline_geometry_scales_and_aligns_dash_gaps_to_physical_pixels() {
        for (scale_factor, expected) in [
            (
                1.0,
                vec![
                    [8.0, 21.0, 10.0, 22.0],
                    [12.0, 21.0, 14.0, 22.0],
                    [16.0, 21.0, 18.0, 22.0],
                ],
            ),
            (1.5, vec![[8.0, 20.5, 11.0, 22.0], [14.0, 20.5, 17.0, 22.0]]),
            (2.0, vec![[8.0, 20.0, 12.0, 22.0], [16.0, 20.0, 18.0, 22.0]]),
        ] {
            assert_eq!(
                dotted_underline_segments(8.0, 18.0, 22.0, scale_factor),
                expected
            );
        }
    }

    /// PIN (verification ruling 2026-08-04, part 4): a verified image reference standing next to a
    /// link is drawn as *one* dash pattern, because the run is defined by the mark and not by its
    /// source.
    ///
    /// This is what "the same vocabulary" has to mean at the pixel level. Had the reference been
    /// given a parallel flag or a parallel drawing path, the two spans would each start their own
    /// dash phase and a reader would see the seam — the exact tell that two systems are pretending
    /// to be one. The solid case is pinned beside it because that is the hover upgrade: a cell that
    /// has gone solid closes the dotted run rather than being absorbed by it.
    #[test]
    fn a_reference_and_a_link_side_by_side_are_one_dotted_run() {
        let dotted = |foreground: Option<bt_transcript::TerminalColor>| {
            let mut cell = bt_transcript::CapturedCell::plain("x");
            cell.style.flags.insert(CellFlags::DOTTED_UNDERLINE);
            if let Some(foreground) = foreground {
                cell.style.foreground = foreground;
            }
            cell
        };
        let mut solid = bt_transcript::CapturedCell::plain("x");
        solid.style.flags.insert(CellFlags::UNDERLINE);

        // Columns 0..3 an OSC 8 link, columns 3..6 a verified image reference: one run, 0..6.
        let mut cells = vec![dotted(None); 6];
        cells.push(bt_transcript::CapturedCell::plain("x"));
        assert_eq!(dotted_underline_run_end(&cells, 0, 8, cells.len()), 6);

        // The hover upgrade closes the run where it starts.
        let mut hovered = vec![dotted(None); 3];
        hovered.extend(vec![solid; 3]);
        assert_eq!(dotted_underline_run_end(&hovered, 0, 8, hovered.len()), 3);

        // A colour change is still a new run: the dots take the text's own colour.
        let mut recoloured = vec![dotted(None); 3];
        recoloured.extend(vec![
            dotted(Some(bt_transcript::TerminalColor::Indexed(4)));
            3
        ]);
        assert_eq!(
            dotted_underline_run_end(&recoloured, 0, 8, recoloured.len()),
            3
        );

        // A row boundary always ends the run, whatever the next row wears.
        let across = vec![dotted(None); 6];
        assert_eq!(dotted_underline_run_end(&across, 1, 3, across.len()), 3);
    }

    #[test]
    fn dotted_underline_geometry_clips_the_final_dash_without_fractional_x_edges() {
        let segments = dotted_underline_segments(8.2, 17.6, 21.8, 1.5);
        assert_eq!(
            segments,
            vec![[8.0, 20.5, 11.0, 22.0], [14.0, 20.5, 17.0, 22.0]]
        );
        assert!(
            segments
                .iter()
                .flatten()
                .all(|coordinate| coordinate.is_finite())
        );
        assert!(segments.iter().all(|segment| segment[0].fract() == 0.0
            && segment[2].fract() == 0.0
            && segment[3].fract() == 0.0));
    }

    #[test]
    fn srgb_theme_colors_are_linearized_at_clear_and_rect_upload_boundaries() {
        // The clear now carries the window ground's alpha, which is process
        // state — so this test takes the same lock every test that moves
        // process colour takes, or it reads a ground another test is halfway
        // through setting.
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = RestoreGround(window_ground());
        let _ = set_window_ground(WindowGround::opaque());
        let clear = theme_clear_color();
        // The transfer function itself, pinned at one known point so the
        // background can move without the linearization silently changing.
        assert!(
            (srgb_channel_to_linear(12) - 0.003_676_507_324_047_436).abs() < f64::EPSILON,
            "the sRGB transfer function moved"
        );
        let expected = srgb_channel_to_linear(DEFAULT_BACKGROUND_RGB[0]);
        assert_eq!([clear.r, clear.g, clear.b], [expected; 3]);
        assert_eq!(clear.a, 1.0);

        let rect = rect_gpu_color(default_background());
        assert_eq!(
            rect,
            [expected as f32, expected as f32, expected as f32, 1.0]
        );
        assert_ne!(
            rect[0],
            f32::from(DEFAULT_BACKGROUND_RGB[0]) / 255.0,
            "sRGB bytes must never be uploaded to an sRGB surface as linear channels"
        );

        let antialiased = rect_gpu_color_with_coverage([0x80, 0x40, 0x20], 0.375);
        assert_eq!(antialiased[3], 0.375);
        assert_eq!(
            &antialiased[..3],
            &srgb_rgb_to_linear([0x80, 0x40, 0x20]).map(|channel| channel as f32),
            "coverage belongs in straight alpha and must not premultiply linear RGB"
        );
        assert_eq!(rect_gpu_color_with_coverage([0, 0, 0], -1.0)[3], 0.0);
        assert_eq!(rect_gpu_color_with_coverage([0, 0, 0], 2.0)[3], 1.0);
    }

    /// The seat rectangle a surface change leaves behind, pinned as the exact
    /// expression `WindowRenderer::resize` applies to `self.seat`.
    ///
    /// A solved split must survive: substituting the whole surface here is a
    /// lone leaf's answer given to a tree that is not one, and it silently
    /// widens every content extent (`pane_right`, `pane_bottom`, the peek box,
    /// the math band and its toolbar) and the pass scissor to the window — which
    /// is how terminal content came to be drawn over a neighbouring seat. What
    /// must still hold is that the rectangle never leaves the new surface.
    ///
    /// Red gate: replace the clamp with `SeatViewport::whole(width, height)` and
    /// the first case fails.
    #[test]
    fn a_surface_change_clamps_the_solved_seat_and_never_reinvents_it() {
        let split = SeatViewport {
            x: 0,
            y: 0,
            width: 975,
            height: 1200,
        };
        assert_eq!(
            split.clamped_to(1920, 1200),
            split,
            "a no-op surface change must leave a solved split exactly as solved"
        );
        let right_seat = SeatViewport {
            x: 976,
            y: 0,
            width: 944,
            height: 1200,
        };
        assert_eq!(
            right_seat.clamped_to(1920, 1200),
            right_seat,
            "a seat with a non-zero origin survives untouched too"
        );
        for (width, height) in [(1280u32, 800u32), (1, 1), (600, 1200)] {
            let clamped = right_seat.clamped_to(width, height);
            assert!(
                clamped.x + clamped.width <= width.max(1)
                    && clamped.y + clamped.height <= height.max(1),
                "{clamped:?} escapes a {width}x{height} surface"
            );
            assert!(clamped.width >= 1 && clamped.height >= 1);
        }
    }

    #[test]
    fn surface_config_size_clamps_each_axis_to_the_device_limit() {
        const LIMIT: u32 = 8192;
        for (requested, expected) in [(0, 1), (1, 1), (8192, 8192), (8193, 8192), (65_464, 8192)] {
            assert_eq!(
                surface_config_size(requested, requested, LIMIT),
                (expected, expected)
            );
        }
        assert_eq!(surface_config_size(534, 65_464, LIMIT), (534, 8192));
    }

    #[test]
    fn content_addressed_rows_remap_arcs_after_screen_position_changes() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut swash_cache = SwashCache::new();
        let mut narrow_cache = NarrowShapingCache::new();
        let mut wide_cache = WideShapingCache::new();
        let mut row_cache = ComposedRowCache::new();
        let mut text_rows = Vec::new();
        let mut status_overlay = None;
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(2).unwrap(),
            grid_rows: NonZeroU32::new(3).unwrap(),
            rows: NonZeroU32::new(3).unwrap(),
            presentation_offset_subpixels: 0,
            cells: ["a", "b", "c"]
                .into_iter()
                .flat_map(|text| vec![CapturedCell::plain(text); 2])
                .collect(),
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
            cell_anchors: test_cell_anchors(6),
            row_map: test_row_map(3),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(2),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let cold = prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(cold.rows_reshaped, 3);
        assert_eq!(cold.row_cache.misses, 3);
        let original = text_rows.clone();

        frame.cells.rotate_left(2);
        let shifted = prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(shifted.rows_reshaped, 0);
        assert_eq!(shifted.row_cache.hits, 3);
        assert!(Arc::ptr_eq(&text_rows[0], &original[1]));
        assert!(Arc::ptr_eq(&text_rows[1], &original[2]));
        assert!(Arc::ptr_eq(&text_rows[2], &original[0]));
        assert!(shifted.row_cache.resident_bytes > 0);

        let shifted_rows = text_rows.clone();
        frame.presentation_offset_subpixels = 4 * SUBPIXELS_PER_PX;
        frame.row_map[0].top_subpixels = -4 * SUBPIXELS_PER_PX;
        frame.row_map[0].height_subpixels += 4 * SUBPIXELS_PER_PX;
        frame.row_map[1].top_subpixels += 4 * SUBPIXELS_PER_PX;
        frame.row_map[2].top_subpixels += 4 * SUBPIXELS_PER_PX;
        let remapped_geometry = prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            1,
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(remapped_geometry.rows_reshaped, 0);
        assert_eq!(remapped_geometry.row_cache.hits, 3);
        assert!(
            text_rows
                .iter()
                .zip(&shifted_rows)
                .all(|(next, previous)| Arc::ptr_eq(next, previous)),
            "row height is placement geometry, not a shaping-cache identity"
        );
    }

    #[test]
    fn row_cache_key_separates_style_metrics_revision_and_status_overlay() {
        let mut base = CapturedCell::plain("x");
        let metrics = RowMetricsKey::from(CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 16.0,
            font_size_px: 14.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 12.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 8.0,
        });
        let key = ComposedRowKey {
            cells: vec![base.clone()],
            metrics,
            font_revision: 7,
            theme_revision: 11,
            status_overlay: None,
        };
        let changed_cell_key = |cell| ComposedRowKey {
            cells: vec![cell],
            ..key.clone()
        };
        for flag in [
            CellFlags::INVERSE,
            CellFlags::BOLD,
            CellFlags::ITALIC,
            CellFlags::UNDERLINE,
            CellFlags::DIM,
            CellFlags::HIDDEN,
            CellFlags::STRIKEOUT,
            CellFlags::DOUBLE_UNDERLINE,
            CellFlags::UNDERCURL,
            CellFlags::DOTTED_UNDERLINE,
            CellFlags::DASHED_UNDERLINE,
            CellFlags::WIDE_CHAR,
        ] {
            let mut styled = base.clone();
            styled.style.flags.insert(flag);
            assert_ne!(key, changed_cell_key(styled), "flag {flag:?} must key rows");
        }
        let mut changed_text = base.clone();
        changed_text.text = "y".into();
        assert_ne!(key, changed_cell_key(changed_text));
        let mut spacer = base.clone();
        spacer.wide_spacer = true;
        assert_ne!(key, changed_cell_key(spacer));
        let mut foreground = base.clone();
        foreground.style.foreground = TerminalColor::Rgb(1, 2, 3);
        assert_ne!(key, changed_cell_key(foreground));
        let mut background = base.clone();
        background.style.background = TerminalColor::Indexed(42);
        assert_ne!(key, changed_cell_key(background));
        base.hyperlink = Some(bt_transcript::CellHyperlink::implicit(
            "https://example.invalid",
        ));
        assert_ne!(key, changed_cell_key(base));

        let scaled = ComposedRowKey {
            metrics: RowMetricsKey {
                scale_factor_bits: 2.0_f64.to_bits(),
                ..metrics
            },
            ..key.clone()
        };
        let revised = ComposedRowKey {
            font_revision: 8,
            ..key.clone()
        };
        let rethemed = ComposedRowKey {
            theme_revision: 12,
            ..key.clone()
        };
        let status = ComposedRowKey {
            status_overlay: Some("status".to_owned()),
            ..key.clone()
        };
        assert_ne!(key, scaled);
        assert_ne!(key, revised);
        assert_ne!(key, rethemed);
        assert_ne!(key, status);
    }

    /// The process theme is process-wide, so every test that moves it has to
    /// take the same lock — one per test would let two of them interleave a
    /// switch with another's reads.
    static THEME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct RestoreTheme(Theme);
    impl Drop for RestoreTheme {
        fn drop(&mut self) {
            let _ = set_theme(self.0);
        }
    }

    /// The window ground is process-wide for the same reason the theme is, so it
    /// is put back the same way.
    struct RestoreGround(WindowGround);
    impl Drop for RestoreGround {
        fn drop(&mut self) {
            let _ = set_window_ground(self.0.clone());
        }
    }

    /// PIN (§7.1.6c-4b) — the ground's two percentages are clamped where the
    /// ground is *set*, so no surface anywhere can be handed a value outside
    /// its range, and a ground that did not move costs no revision.
    ///
    /// Clamped at the setter and not at the draw because there are two draws
    /// (the clear and the quad) and one setter: a floor enforced at the draw is
    /// a floor two places have to agree on, and the day they disagree is the day
    /// the clear and the picture describe different windows.
    #[test]
    fn setting_the_window_ground_clamps_its_percentages_and_advances_the_revision() {
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = RestoreGround(window_ground());
        let _ = set_window_ground(WindowGround::opaque());

        let before = theme_revision();
        assert_eq!(
            set_window_ground(WindowGround {
                image: None,
                fit: BackgroundFit::Tile,
                image_opacity: 4.0,
                alpha: 0.01,
            }),
            ThemeChange::Changed
        );
        let read = window_ground();
        assert_eq!(read.image_opacity, 1.0);
        assert_eq!(
            read.alpha, MINIMUM_GROUND_ALPHA,
            "there is no setting from which this window can be made unreadable"
        );
        assert_eq!(read.fit, BackgroundFit::Tile);
        assert!(
            theme_revision() > before,
            "the ground rides the one revision channel that invalidates every \
             theme-authored artefact — a second list would be a second thing to \
             forget"
        );

        let settled = theme_revision();
        assert_eq!(
            set_window_ground(WindowGround {
                image: None,
                fit: BackgroundFit::Tile,
                image_opacity: 1.0,
                alpha: MINIMUM_GROUND_ALPHA,
            }),
            ThemeChange::Unchanged
        );
        assert_eq!(
            theme_revision(),
            settled,
            "a settings write that did not touch the ground must cost nothing"
        );
    }

    /// PIN (§7.1.6c-4b) — the ground's alpha reaches the clear, premultiplied,
    /// and an opaque ground leaves the clear bit-identical to what §2.3 A2's
    /// acceptance shots were taken against.
    #[test]
    fn the_grounds_alpha_reaches_the_clear_premultiplied() {
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = RestoreGround(window_ground());

        let _ = set_window_ground(WindowGround::opaque());
        let opaque = theme_clear_color();
        assert_eq!(opaque.a, 1.0);
        let linear = srgb_channel_to_linear(default_background()[0]);
        assert!((opaque.r - linear).abs() < 1e-12);

        let _ = set_window_ground(WindowGround {
            alpha: 0.6,
            ..WindowGround::opaque()
        });
        let translucent = theme_clear_color();
        assert!((translucent.a - 0.6).abs() < 1e-6);
        assert!(
            (translucent.r - linear * 0.6).abs() < 1e-6,
            "a `PreMultiplied` swapchain given straight alpha draws a window \
             that is too bright, and on a dark desktop nobody can see it"
        );
        assert!(
            translucent.r < opaque.r,
            "premultiplication darkens toward the desktop, never toward white"
        );

        // **And in the other state** (user report 2026-08-18): a ground that
        // names a picture answers with the same clear as one that does not.
        // That is the whole of what the failed-decode path relies on — a file
        // that will not open leaves `image: None` on a ground whose alpha the
        // reader set, and the window must be exactly as see-through as it
        // would have been with the picture.
        //
        // Red gate: make `theme_clear_color` read anything off `image` and one
        // of these two goes red.
        let with_picture = WindowGround {
            alpha: 0.6,
            image: Some(std::sync::Arc::new(BackgroundImage {
                key: "image:probe".to_owned(),
                rgba: std::sync::Arc::from(vec![255_u8, 0, 0, 255]),
                width_px: 1,
                height_px: 1,
            })),
            ..WindowGround::opaque()
        };
        let _ = set_window_ground(with_picture.clone());
        assert_eq!(
            theme_clear_color(),
            translucent,
            "a ground with a picture clears to the same value as one without"
        );
        let _ = set_window_ground(WindowGround {
            image: None,
            ..with_picture
        });
        assert_eq!(
            theme_clear_color(),
            translucent,
            "and so does the ground a refused picture leaves behind"
        );
    }

    /// PIN — the ground quad is the whole surface, six vertices, one opacity and
    /// one ground colour on every one of them.
    ///
    /// The corners are NDC literals rather than pixels divided by a viewport,
    /// because this quad is always the entire target: a rounding rule here would
    /// be a rounding rule with nothing to round.
    #[test]
    fn the_ground_quad_covers_the_whole_surface_with_one_ground_on_every_corner() {
        let uv = background_uv_rect(BackgroundFit::Tile, 1000, 700, 300, 300);
        let quad = background_quad_vertices(uv, [0.1, 0.2, 0.3], 0.6, 0.45);
        assert_eq!(quad.len(), 6, "two triangles");
        for vertex in &quad {
            assert_eq!(vertex.ground, [0.1, 0.2, 0.3, 0.6]);
            assert_eq!(vertex.image_opacity, 0.45);
            assert!(vertex.position.iter().all(|axis| axis.abs() == 1.0));
        }
        let xs: Vec<f32> = quad.iter().map(|vertex| vertex.position[0]).collect();
        let ys: Vec<f32> = quad.iter().map(|vertex| vertex.position[1]).collect();
        assert!(xs.contains(&-1.0) && xs.contains(&1.0));
        assert!(ys.contains(&-1.0) && ys.contains(&1.0));
        // The top-left corner samples the UV rectangle's origin, which is what
        // makes Tile start at the window's top-left rather than anywhere else.
        let top_left = quad
            .iter()
            .find(|vertex| vertex.position == [-1.0, 1.0])
            .expect("the quad has a top-left corner");
        assert_eq!(top_left.uv, [uv[0], uv[1]]);
    }

    /// PIN — the floor this crate clamps to is the floor the file format
    /// declares, held in two crates by one number.
    ///
    /// `bt-render` does not depend on `bt-persist`, so the constant is written
    /// twice on purpose; this is the nail that stops the two copies drifting
    /// into a settings page whose lowest position the renderer refuses to draw.
    #[test]
    fn the_floor_here_is_the_floor_in_the_file_format() {
        assert!((MINIMUM_GROUND_ALPHA - 0.3).abs() < f32::EPSILON);
    }

    /// PIN: a live selection changes colour the moment the theme does. The fill
    /// is read from the same atomic word as the background, so there is no
    /// frame on which a selection is still wearing the other canvas's colour.
    #[test]
    fn theme_switch_repaints_the_selection_fill() {
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_theme = current_theme();
        let _restore = RestoreTheme(original_theme);
        assert_ne!(
            set_theme(Theme::Dark),
            ThemeChange::LockedByEnvironment,
            "the runtime theme must be switchable for this renderer pin"
        );

        assert_eq!(theme::selection_background_rgb(), [0x26, 0x4f, 0x78]);
        let dark_revision = theme_revision();

        assert_eq!(set_theme(Theme::Light), ThemeChange::Changed);
        assert_eq!(
            theme::selection_background_rgb(),
            [0xc1, 0xcd, 0xf3],
            "the switch must reach the selection fill, not only the ink"
        );
        assert!(
            theme_revision() > dark_revision,
            "the selection rides the revision channel that invalidates theme-authored artifacts"
        );
    }

    #[test]
    fn theme_switch_recomposes_an_ansi_colored_rendered_row() {
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_theme = current_theme();
        let _restore = RestoreTheme(original_theme);
        assert_ne!(
            set_theme(Theme::Dark),
            ThemeChange::LockedByEnvironment,
            "the runtime theme must be switchable for this renderer pin"
        );

        let mut yellow = CapturedCell::plain("Y");
        yellow.style.foreground = TerminalColor::Named(3);
        let frame = ViewportFrame {
            columns: NonZeroU32::new(1).unwrap(),
            grid_rows: NonZeroU32::new(1).unwrap(),
            rows: NonZeroU32::new(1).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![yellow],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: false,
            },
            cell_anchors: test_cell_anchors(1),
            row_map: test_row_map(1),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(1),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut swash_cache = SwashCache::new();
        let mut narrow_cache = NarrowShapingCache::new();
        let mut wide_cache = WideShapingCache::new();
        let mut row_cache = ComposedRowCache::new();
        let mut text_rows = Vec::new();
        let mut status_overlay = None;

        let dark = prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            theme_revision(),
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(dark.rows_reshaped, 1);
        assert_eq!(dark.row_cache.misses, 1);
        assert_eq!(
            text_rows[0].narrow_glyphs[0].color,
            Color::rgb(0xc1, 0x9c, 0x00)
        );
        let dark_row = Arc::clone(&text_rows[0]);

        assert_eq!(set_theme(Theme::Light), ThemeChange::Changed);
        let light = prepare_text_rows(
            &frame,
            metrics,
            &mut text_rows,
            &mut status_overlay,
            &mut row_cache,
            1,
            theme_revision(),
            &mut font_system,
            &mut swash_cache,
            &mut narrow_cache,
            &mut wide_cache,
        )
        .unwrap();
        assert_eq!(light.rows_reshaped, 1);
        assert_eq!(light.row_cache.misses, 1);
        assert!(!Arc::ptr_eq(&text_rows[0], &dark_row));
        assert_eq!(
            text_rows[0].narrow_glyphs[0].color,
            Color::rgb(0x99, 0x99, 0x00)
        );
    }

    #[test]
    fn byte_lru_promotes_and_evicts_the_tail_in_constant_time_links() {
        let mut cache = ByteLru::new(2);
        assert_eq!(cache.insert("a", 1, 1), (true, 0));
        assert_eq!(cache.insert("b", 2, 1), (true, 0));
        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.insert("c", 3, 1), (true, 1));
        assert!(cache.get(&"b").is_none());
        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"c"), Some(&3));
        assert_eq!(cache.resident_bytes(), 2);
    }

    #[test]
    fn narrow_slots_preserve_blank_columns_and_style_boundaries() {
        let mut red = CapturedCell::plain("A");
        red.style.foreground = TerminalColor::Rgb(255, 0, 0);
        let cells = [
            red,
            CapturedCell::plain(""),
            CapturedCell::plain("B"),
            CapturedCell::plain(" "),
        ];
        let slots = narrow_cell_slots(&cells);
        assert_eq!(slots.len(), 2);
        assert_eq!((slots[0].column, slots[0].text.as_str()), (0, "A"));
        assert_eq!((slots[1].column, slots[1].text.as_str()), (2, "B"));
        assert_ne!(slots[0].style, slots[1].style);
    }

    #[test]
    fn preedit_is_transient_underlined_grid_content_with_a_collapsed_caret() {
        let frame = ViewportFrame {
            columns: NonZeroU32::new(8).unwrap(),
            grid_rows: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(2).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain(""); 16],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 2,
                visible: true,
            },
            cell_anchors: test_cell_anchors(16),
            row_map: test_row_map(2),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(8),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let composed = compose_preedit(
            &frame,
            Some(&Preedit {
                text: "nihao".to_owned(),
                cursor_byte: Some(2),
            }),
        )
        .unwrap();

        assert_eq!(composed.ime_caret.column, 4);
        assert_eq!(composed.frame.cursor, composed.ime_caret);
        assert_eq!(composed.frame.cells[2].text, "n");
        assert!(
            composed.frame.cells[2]
                .style
                .flags
                .contains(CellFlags::UNDERLINE)
        );
        assert_eq!(
            frame.cells[2].text, "",
            "source terminal frame is untouched"
        );
    }

    #[test]
    fn preedit_uses_the_same_cluster_oracle_as_committed_cells() {
        let frame = ViewportFrame {
            columns: NonZeroU32::new(8).unwrap(),
            grid_rows: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(2).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain(""); 16],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 1,
                visible: true,
            },
            cell_anchors: test_cell_anchors(16),
            row_map: test_row_map(2),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(8),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let text = "👨‍👩‍👧‍👦☆中";
        let composed = compose_preedit(
            &frame,
            Some(&Preedit {
                text: text.to_owned(),
                cursor_byte: Some(text.len()),
            }),
        )
        .unwrap();

        assert_eq!(composed.ime_caret.column, 6);
        assert_eq!(composed.frame.cells[1].text, "👨‍👩‍👧‍👦");
        assert!(
            composed.frame.cells[1]
                .style
                .flags
                .contains(CellFlags::WIDE_CHAR)
        );
        assert!(composed.frame.cells[2].wide_spacer);
        assert_eq!(composed.frame.cells[3].text, "☆");
        assert!(
            !composed.frame.cells[3]
                .style
                .flags
                .contains(CellFlags::WIDE_CHAR)
        );
        assert_eq!(composed.frame.cells[4].text, "中");
        assert!(composed.frame.cells[5].wide_spacer);
    }

    #[test]
    fn mixed_cjk_ascii_wide_slots_use_exact_terminal_cell_origins() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut ni = CapturedCell::plain("你");
        ni.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut hao = CapturedCell::plain("好");
        hao.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut spacer = CapturedCell::plain("");
        spacer.wide_spacer = true;
        let cells = [
            CapturedCell::plain("A"),
            ni,
            spacer.clone(),
            CapturedCell::plain("B"),
            hao,
            spacer,
        ];

        let slots = wide_cell_slots(&cells);
        assert_eq!(
            slots.iter().map(|slot| slot.column).collect::<Vec<_>>(),
            [1, 4]
        );
        assert_eq!(
            cell_bounds_px(metrics, 0, slots[0].column)[0],
            metrics.padding_px + metrics.cell_width_px
        );
        assert_eq!(
            cell_bounds_px(metrics, 0, slots[1].column)[0],
            metrics.padding_px + 4.0 * metrics.cell_width_px
        );

        let narrow = shape_narrow_for_test(&cells, &mut font_system, metrics);
        assert_eq!(
            narrow.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
            [0, 3]
        );
        assert_narrow_glyph_origins(&narrow, metrics);
    }

    #[test]
    fn baseline_offset_aligns_an_independent_fallback_buffer() {
        assert_eq!(baseline_offset_px(17.5, 15.0), 2.5);
        assert_eq!(baseline_offset_px(15.0, 17.5), -2.5);
    }

    #[test]
    fn cjk_size_compensation_targets_the_two_cell_em() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        assert_eq!(
            metrics.font_size_px * wide_slot_em_scale(metrics),
            2.0 * metrics.cell_width_px
        );
        assert!(wide_slot_em_scale(metrics) > 1.0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cjk_wide_buffer_matches_two_cells_and_the_ascii_baseline() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let mut cell = CapturedCell::plain("你");
        cell.style.flags.insert(CellFlags::WIDE_CHAR);
        let glyphs = shape_wide_for_test(&[cell], &mut font_system, metrics);
        let wide = &glyphs[0];
        let run = wide.buffer.layout_runs().next().unwrap();
        let glyph = &run.glyphs[0];

        assert_eq!(
            wide.buffer.monospace_width(),
            Some(2.0 * metrics.cell_width_px)
        );
        assert!(glyph.font_size >= metrics.font_size_px);
        assert!(glyph.w > metrics.cell_width_px);
        assert!(glyph.w <= 2.0 * metrics.cell_width_px);
        assert_eq!(run.line_y + wide.top_offset_px, metrics.ascii_baseline_px);
    }

    /// PIN: Bar, Block and Underline retain their ruled geometry at every supported DPI.
    #[cfg(target_os = "windows")]
    #[test]
    fn every_cursor_style_has_its_ruled_geometry_at_every_dpi() {
        let mut font_system = terminal_font_system();
        for dpi_milli in [1000_u32, 1250, 1500, 2000] {
            let scale = f64::from(dpi_milli) / 1000.0;
            let metrics = CellMetrics::measure(&mut font_system, scale).unwrap();
            let frame = ViewportFrame {
                columns: NonZeroU32::new(1).unwrap(),
                grid_rows: NonZeroU32::new(1).unwrap(),
                rows: NonZeroU32::new(1).unwrap(),
                presentation_offset_subpixels: 0,
                cells: vec![CapturedCell::plain("x")],
                cursor: bt_viewport::GridCursor {
                    row: 0,
                    column: 0,
                    visible: true,
                },
                cell_anchors: test_cell_anchors(1),
                row_map: test_row_map_for_metrics(1, metrics),
                selection_spans: Vec::new(),
                search_spans: Vec::new(),
                current_search_spans: Vec::new(),
                math_blocks: Vec::new(),
                math_failures: Vec::new(),
                status_text: None,
                viewport_origin: FrameViewportOrigin::Bottom,
                scroll_offset_rows: 0,
                layout_key: bt_doc_layout_key(1),
                view_generation: bt_doc::ViewGeneration(1),
            };
            let cell = frame_cell_bounds_px(metrics, &frame, 0, 0);
            let bar = cursor_shape_pixel_bounds(metrics, cell, CursorStyle::Bar);
            assert_eq!(bar.len(), 1);
            let [left, top, right, bottom] = bar[0];
            assert_eq!(
                right - left,
                (CURSOR_BAR_WIDTH_LOGICAL_PX * metrics.scale_factor as f32)
                    .round()
                    .max(1.0),
                "at {dpi_milli} milli-DPI the caret is not the thin bar",
            );
            assert!(
                right - left >= 1.0 && right - left < metrics.cell_width_px / 2.0,
                "thin, never half the cell and never nothing, at {dpi_milli} milli-DPI"
            );
            assert_eq!(left, cell[0].round(), "the caret starts at its own cell");
            assert_eq!([top, bottom], [cell[1], cell[3]], "the row's full height");

            assert_eq!(
                cursor_shape_pixel_bounds(metrics, cell, CursorStyle::Block),
                vec![cell],
                "at {dpi_milli} milli-DPI block is the whole cell"
            );
            let underline = cursor_shape_pixel_bounds(metrics, cell, CursorStyle::Underline);
            assert_eq!(underline.len(), 1);
            assert_eq!(underline[0][0], cell[0]);
            assert_eq!(underline[0][2], cell[2]);
            assert_eq!(underline[0][3], cell[3]);
            assert_eq!(
                underline[0][3] - underline[0][1],
                (CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX * metrics.scale_factor as f32)
                    .round()
                    .max(1.0),
                "at {dpi_milli} milli-DPI underline is two logical pixels high"
            );
        }
    }

    #[test]
    fn cursor_on_either_half_of_a_wide_cell_anchors_to_the_lead_column() {
        let mut lead = CapturedCell::plain("中");
        lead.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut spacer = CapturedCell::plain("");
        spacer.wide_spacer = true;
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(3).unwrap(),
            grid_rows: NonZeroU32::new(1).unwrap(),
            rows: NonZeroU32::new(1).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![lead, spacer, CapturedCell::plain("x")],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            cell_anchors: test_cell_anchors(3),
            row_map: test_row_map(1),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(3),
            view_generation: bt_doc::ViewGeneration(1),
        };
        assert_eq!(cursor_cell_span(&frame), (0, 2));
        frame.cursor.column = 1;
        assert_eq!(cursor_cell_span(&frame), (0, 2));

        // The bar is not stretched over a wide character — it marks where the
        // next glyph starts, and that is one column edge whichever half of the
        // character the grid reports the cursor in.
        let metrics = CellMetrics {
            cell_width_px: 8.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 4.0,
            scale_factor: 1.0,
            ascii_baseline_px: 0.0,
            primary_advance_px: 8.0,
            primary_cap_height_px: 10.0,
            primary_cap_center_y_px: 5.0,
        };
        frame.row_map = test_row_map_for_metrics(1, metrics);
        assert_eq!(
            cursor_pixel_bounds(metrics, &frame, true),
            vec![[4.0, 4.0, 5.0, 24.0]]
        );
        frame.cursor.column = 0;
        assert_eq!(
            cursor_pixel_bounds(metrics, &frame, true),
            vec![[4.0, 4.0, 5.0, 24.0]]
        );
    }

    /// One cell, one caret in it, at whatever DPI the metrics describe.
    fn single_cell_cursor_frame(metrics: CellMetrics) -> ViewportFrame {
        ViewportFrame {
            columns: NonZeroU32::new(1).unwrap(),
            grid_rows: NonZeroU32::new(1).unwrap(),
            rows: NonZeroU32::new(1).unwrap(),
            presentation_offset_subpixels: 0,
            cells: vec![CapturedCell::plain("x")],
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            cell_anchors: test_cell_anchors(1),
            row_map: test_row_map_for_metrics(1, metrics),
            selection_spans: Vec::new(),
            search_spans: Vec::new(),
            current_search_spans: Vec::new(),
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(1),
            view_generation: bt_doc::ViewGeneration(1),
        }
    }

    /// Square cells at a chosen scale, so a caret's geometry can be read at any
    /// DPI without a font.
    fn cursor_test_metrics(scale: f32) -> CellMetrics {
        CellMetrics {
            cell_width_px: 8.0 * scale,
            cell_height_px: 20.0 * scale,
            font_size_px: 16.0 * scale,
            padding_px: 4.0 * scale,
            scale_factor: f64::from(scale),
            ascii_baseline_px: 0.0,
            primary_advance_px: 8.0 * scale,
            primary_cap_height_px: 10.0 * scale,
            primary_cap_center_y_px: 5.0 * scale,
        }
    }

    /// PIN — losing focus fades the caret's ink and leaves its shape alone. A
    /// bar stays the same bar in the same place; an underline the same
    /// underline. Only the block, whose faded form is the classic hollow box,
    /// changes geometry at all.
    #[test]
    fn an_unfocused_caret_keeps_the_shape_the_user_chose() {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let metrics = cursor_test_metrics(scale);
            let frame = single_cell_cursor_frame(metrics);
            for style in [CursorStyle::Bar, CursorStyle::Underline] {
                assert_eq!(
                    cursor_pixel_bounds_for_style(metrics, &frame, false, style),
                    cursor_pixel_bounds_for_style(metrics, &frame, true, style),
                    "at {scale}× a {style:?} caret must not change shape when the window \
                     loses focus — only its ink fades"
                );
            }
            let focused_block =
                cursor_pixel_bounds_for_style(metrics, &frame, true, CursorStyle::Block);
            let unfocused_block =
                cursor_pixel_bounds_for_style(metrics, &frame, false, CursorStyle::Block);
            assert_eq!(focused_block.len(), 1, "a focused block is one filled cell");
            assert_eq!(
                unfocused_block.len(),
                4,
                "at {scale}× a block's faded form is its hollow outline"
            );
        }
    }

    #[test]
    fn an_unfocused_block_hollows_out_and_focus_refills_it() {
        let metrics = cursor_test_metrics(1.0);
        let frame = single_cell_cursor_frame(metrics);

        assert_eq!(
            cursor_pixel_bounds_for_style(metrics, &frame, true, CursorStyle::Block),
            vec![[4.0, 4.0, 12.0, 24.0]],
            "a focused block is the whole cell"
        );
        let outline = cursor_pixel_bounds_for_style(metrics, &frame, false, CursorStyle::Block);
        assert_eq!(outline.len(), 4);
        assert_eq!(outline[0], [4.0, 4.0, 12.0, 5.0]);
        assert_eq!(outline[1], [4.0, 23.0, 12.0, 24.0]);
        assert_eq!(outline[2], [4.0, 5.0, 5.0, 23.0]);
        assert_eq!(outline[3], [11.0, 5.0, 12.0, 23.0]);

        assert_eq!(
            cursor_pixel_bounds_for_style(metrics, &frame, true, CursorStyle::Bar),
            vec![[4.0, 4.0, 5.0, 24.0]],
            "a focused caret in the default shape is the one-logical-pixel bar"
        );
        assert_eq!(
            cursor_pixel_bounds_for_style(metrics, &frame, false, CursorStyle::Bar),
            vec![[4.0, 4.0, 5.0, 24.0]],
            "and that bar is exactly what an unfocused window keeps"
        );
    }

    /// PIN — the hollow outline's stroke is one *logical* pixel: it scales with
    /// the DPI like every other measure, instead of thinning to half a logical
    /// pixel at 200%.
    #[test]
    fn the_hollow_outline_stroke_scales_with_the_dpi() {
        let metrics = cursor_test_metrics(2.0);
        let frame = single_cell_cursor_frame(metrics);

        let outline = cursor_pixel_bounds_for_style(metrics, &frame, false, CursorStyle::Block);
        assert_eq!(outline.len(), 4);
        assert_eq!(
            outline[0][3] - outline[0][1],
            2.0,
            "at 200% the stroke is two device pixels, one logical pixel"
        );
        assert_eq!(
            outline[2][2] - outline[2][0],
            2.0,
            "the vertical strokes wear the same width"
        );
    }

    /// PIN — an unfocused caret is drawn in a *different, quieter* ink than a
    /// focused one, and a live theme switch reaches both of them.
    #[test]
    fn theme_switch_repaints_both_caret_inks() {
        let _lock = THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let original_theme = current_theme();
        let _restore = RestoreTheme(original_theme);
        assert_ne!(
            set_theme(Theme::Dark),
            ThemeChange::LockedByEnvironment,
            "the runtime theme must be switchable for this renderer pin"
        );

        assert_eq!(cursor_rgb(), [0xd4, 0xd4, 0xd4]);
        assert_eq!(unfocused_cursor_rgb(), [0x6e, 0x6e, 0x6e]);
        assert_ne!(
            unfocused_cursor_rgb(),
            cursor_rgb(),
            "an unfocused caret must be visibly quieter than a focused one"
        );

        assert_eq!(set_theme(Theme::Light), ThemeChange::Changed);
        assert_eq!(cursor_rgb(), [0x37, 0x35, 0x2f]);
        assert_eq!(
            unfocused_cursor_rgb(),
            [0xa5, 0xa4, 0xa1],
            "the switch must reach the faded caret, not only the focused one"
        );
        assert_ne!(unfocused_cursor_rgb(), cursor_rgb());
    }

    #[test]
    fn hidden_blink_phase_omits_only_the_focused_cursor_quad() {
        assert!(cursor_quad_visible(true, true, true));
        assert!(
            !cursor_quad_visible(true, true, false),
            "a focused hidden phase must submit no cursor quad"
        );
        assert!(
            cursor_quad_visible(true, false, false),
            "the unfocused hollow cursor does not blink"
        );
        assert!(!cursor_quad_visible(false, false, true));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn bounded_cjk_fallback_shapes_chinese_without_notdef() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut buffer = Buffer::new(
            &mut font_system,
            Metrics::new(metrics.font_size_px, metrics.cell_height_px),
        );
        buffer.set_text("你好世界", &Attrs::new(), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut font_system, false);
        let glyphs = buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .collect::<Vec<_>>();
        assert_eq!(glyphs.len(), 4);
        assert!(
            glyphs.iter().all(|glyph| glyph.glyph_id != 0),
            "every CJK scalar must resolve to a real fallback glyph"
        );
    }

    #[test]
    fn shaped_ascii_glyphs_stay_on_integer_cell_columns() {
        const COLUMNS: usize = 80;

        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let cells = vec![CapturedCell::plain("M"); COLUMNS];
        let glyphs = shape_narrow_for_test(&cells, &mut font_system, metrics);

        assert_eq!(glyphs.len(), COLUMNS);
        assert_narrow_glyph_origins(&glyphs, metrics);
    }

    #[test]
    fn narrow_shaping_cache_reuses_content_across_columns_rows_and_colors() {
        const COLUMNS: usize = 80;

        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let mut cells = vec![CapturedCell::plain("M"); COLUMNS];
        cells[1].style.foreground = TerminalColor::Rgb(255, 0, 0);
        let mut cache = NarrowShapingCache::new();

        let mut swash_cache = SwashCache::new();
        let first = shape_narrow_glyphs(
            &cells,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 1);
        assert!(
            first
                .iter()
                .all(|glyph| Arc::ptr_eq(&first[0].buffer, &glyph.buffer))
        );
        assert_ne!(first[0].color, first[1].color);

        let second = shape_narrow_glyphs(
            &cells,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 1);
        assert!(Arc::ptr_eq(&first[0].buffer, &second[0].buffer));

        cells[0].style.flags.insert(CellFlags::BOLD);
        let bold = shape_narrow_glyphs(
            &cells,
            &mut font_system,
            &mut swash_cache,
            metrics,
            &mut cache,
        );
        assert_eq!(cache.entries.len(), 2);
        assert!(!Arc::ptr_eq(&first[0].buffer, &bold[0].buffer));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn regional_indicator_flag_cells_pin_every_glyph_to_its_grid_column() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let cells = [
            CapturedCell::plain("|"),
            CapturedCell::plain("🇺"),
            CapturedCell::plain("🇸"),
            CapturedCell::plain("|"),
        ];

        let glyphs = shape_narrow_for_test(&cells, &mut font_system, metrics);
        assert_eq!(
            glyphs.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_narrow_glyph_origins(&glyphs, metrics);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn arbitrary_multicodepoint_narrow_cluster_cannot_cross_cell_origins() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        // Lam + alef is a shaping cluster when presented as one run. A legacy terminal grid may
        // still assign the two code points to separate narrow cells, so each grid slot must own an
        // independent absolute origin just like the RI pair above.
        let cells = [
            CapturedCell::plain("x"),
            CapturedCell::plain("ل"),
            CapturedCell::plain("ا"),
            CapturedCell::plain("y"),
        ];

        let glyphs = shape_narrow_for_test(&cells, &mut font_system, metrics);
        assert_eq!(
            glyphs.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_narrow_glyph_origins(&glyphs, metrics);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn mixed_fallback_and_wide_glyphs_keep_every_pen_on_its_grid_column() {
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let mut cjk = CapturedCell::plain("中");
        cjk.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut fullwidth_b = CapturedCell::plain("Ｂ");
        fullwidth_b.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut spacer = CapturedCell::plain("");
        spacer.wide_spacer = true;
        let cells = [
            CapturedCell::plain("|"),
            CapturedCell::plain("A"),
            CapturedCell::plain("☆"),
            cjk,
            spacer.clone(),
            CapturedCell::plain("│"),
            fullwidth_b,
            spacer,
            CapturedCell::plain("|"),
        ];
        let narrow = shape_narrow_for_test(&cells, &mut font_system, metrics);
        assert_eq!(
            narrow.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
            [0, 1, 2, 8]
        );
        assert_narrow_glyph_origins(&narrow, metrics);

        let wide = shape_wide_for_test(&cells, &mut font_system, metrics);
        assert_eq!(
            wide.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
            [3, 6]
        );
        for glyph in wide {
            let local_x = glyph.buffer.layout_runs().next().unwrap().glyphs[0].x;
            assert_eq!(local_x, 0.0);
            assert_eq!(
                cell_bounds_px(metrics, 0, glyph.column)[0],
                metrics.padding_px + glyph.column as f32 * metrics.cell_width_px
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn fallback_in_left_half_cannot_shift_cup_positioned_right_half() {
        const COLUMNS: usize = 64;
        const RIGHT_HALF_COLUMN: usize = 42;

        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.5).unwrap();
        let mut fallback_row = vec![CapturedCell::plain(""); COLUMNS];
        fallback_row[0] = CapturedCell::plain("|");
        fallback_row[1] = CapturedCell::plain("A");
        fallback_row[2] = CapturedCell::plain("🇺");
        fallback_row[3] = CapturedCell::plain("🇸");
        fallback_row[4] = CapturedCell::plain("|");
        fallback_row[RIGHT_HALF_COLUMN] = CapturedCell::plain("|");
        fallback_row[RIGHT_HALF_COLUMN + 1] = CapturedCell::plain("R");

        let mut control_row = vec![CapturedCell::plain(""); COLUMNS];
        control_row[RIGHT_HALF_COLUMN] = CapturedCell::plain("|");
        control_row[RIGHT_HALF_COLUMN + 1] = CapturedCell::plain("R");

        for cells in [&fallback_row, &control_row] {
            let glyphs = shape_narrow_for_test(cells, &mut font_system, metrics);
            assert_narrow_glyph_origins(&glyphs, metrics);
            assert!(glyphs.iter().any(|glyph| glyph.column == RIGHT_HALF_COLUMN));
            assert!(
                glyphs
                    .iter()
                    .any(|glyph| glyph.column == RIGHT_HALF_COLUMN + 1)
            );
        }
    }

    #[test]
    fn mixed_prompt_glyphs_and_cursor_share_the_same_cell_axis() {
        for scale_factor in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let mut font_system = terminal_font_system();
            let metrics = CellMetrics::measure(&mut font_system, scale_factor).unwrap();
            let mut cells = "PS D:\\Developer\\BetterTerminal> carg"
                .chars()
                .map(|character| CapturedCell::plain(character.to_string()))
                .collect::<Vec<_>>();
            for cell in &mut cells[..3] {
                cell.style.foreground = TerminalColor::Rgb(120, 130, 140);
            }
            let glyphs = shape_narrow_for_test(&cells, &mut font_system, metrics);
            let expected_columns = cells
                .iter()
                .enumerate()
                .filter(|(_, cell)| !cell.text.chars().all(char::is_whitespace))
                .map(|(column, _)| column)
                .collect::<Vec<_>>();
            assert_eq!(
                glyphs.iter().map(|glyph| glyph.column).collect::<Vec<_>>(),
                expected_columns
            );
            assert_narrow_glyph_origins(&glyphs, metrics);

            let last_text_cell = cell_bounds_px(metrics, 0, cells.len() - 1);
            let cursor_cell = cell_bounds_px(metrics, 0, cells.len());
            assert_eq!(last_text_cell[2], cursor_cell[0]);
        }
    }

    #[test]
    fn top_left_cell_origins_do_not_depend_on_surface_width() {
        let metrics = CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 8.0,
            scale_factor: 1.0,
            ascii_baseline_px: 16.0,
            primary_advance_px: 10.0,
            primary_cap_height_px: 12.0,
            primary_cap_center_y_px: 10.0,
        };
        assert_eq!(cell_bounds_px(metrics, 0, 0), [8.0, 8.0, 18.0, 28.0]);
        assert_eq!(cell_bounds_px(metrics, 3, 7), [78.0, 68.0, 88.0, 88.0]);
    }

    #[test]
    fn preview_fit_preserves_aspect_ratio_without_upscaling() {
        assert_eq!(preview_image_extent(800, 600, 1600, 1200), Some((800, 600)));
        assert_eq!(preview_image_extent(2000, 2000, 320, 240), Some((320, 240)));
        assert_eq!(preview_image_extent(0, 600, 320, 240), None);
    }

    #[test]
    fn preview_fit_handles_extreme_aspect_ratios() {
        assert_eq!(preview_image_extent(300, 300, 10_000, 1), Some((300, 1)));
        assert_eq!(preview_image_extent(300, 300, 1, 10_000), Some((1, 300)));
        assert_eq!(preview_image_extent(1, 1, 10_000, 10_000), Some((1, 1)));
    }

    #[test]
    fn atlas_exhaustion_degrades_the_frame_instead_of_exiting() {
        assert_eq!(
            prepare_failure_policy(PrepareError::AtlasFull),
            PrepareFailurePolicy::PresentWithoutText
        );
    }

    /// The vertical centre of a chrome label's cap band — cap line to baseline,
    /// the part of a title an icon beside it is read against — once the label has
    /// been shaped for real.
    #[cfg(target_os = "windows")]
    fn cap_band_centre(
        font_system: &mut FontSystem,
        cap_height_ratio: f32,
        label: &ChromeLabel,
    ) -> f32 {
        let layouts = shape_chrome_labels(
            font_system,
            std::slice::from_ref(label),
            cap_height_ratio,
            1.0,
        );
        let layout = layouts.first().expect("the label shapes");
        let baseline = layout.top
            + layout
                .buffer
                .layout_runs()
                .next()
                .expect("the label has a run")
                .line_y;
        baseline - cap_height_ratio * label.font_size_px / 2.0
    }

    /// PIN — a label's *measured* width is the width its own glyphs take on the
    /// drawing path, tracking and weight included.
    ///
    /// Every box in this chrome that hugs its caption is sized from a measure
    /// and filled by the shaper, so the two runs have to be one run asked twice.
    /// They were not: the measurer set neither the weight axis nor letter
    /// spacing, so any label carrying either was measured short — by 2.4 physical
    /// pixels on the float's `DOCK` at 100%, which is most of a letter, and the
    /// `K` was clipped against the very bounds its short measure had drawn.
    ///
    /// Asserted against the sum of the run's own advances rather than against a
    /// number, because the number is the face's business and the *agreement* is
    /// ours. `.float-win .fly-head button` (mock-up 720-725) is the case that
    /// found it, and the loop keeps the plain and tabular cases honest beside it.
    ///
    /// **The tabular rows are the second half of the same lesson** (user report,
    /// 2026-08-17): the doc above already claimed the loop kept them honest and
    /// the loop had no such column, because the measurer refused `tnum` as a
    /// parameter on the belief that tabular figures can only narrow a string.
    /// They widen every narrow digit to the widest one's advance, so `1h` and
    /// `Aug 10` and `8c56194` are each a couple of pixels longer than they
    /// measured — which is exactly how much of the last glyph the Git page's
    /// meta column was losing off its right edge.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_chrome_labels_measured_width_is_the_width_its_glyphs_take() {
        let mut font_system = terminal_font_system();
        let mut swash_cache = SwashCache::new();
        let cap_height_ratio = chrome_cap_height_ratio(&mut font_system, &mut swash_cache)
            .expect("the chrome sans face publishes or renders a cap height");
        for (what, text, size, weight, tracking, tabular) in [
            (
                "`.fly-head button` — the float's DOCK",
                "DOCK",
                10.0_f32,
                ChromeLabelWeight::SemiBold,
                0.04_f32,
                false,
            ),
            // The same button at 200%, where a per-glyph tracking error is twice
            // as large and a rounding-sized tolerance would hide it.
            (
                "the same button at 200%",
                "DOCK",
                20.0,
                ChromeLabelWeight::SemiBold,
                0.04,
                false,
            ),
            (
                "a plain pane title",
                "Terminal",
                11.5,
                ChromeLabelWeight::Regular,
                0.0,
                false,
            ),
            (
                "a focused pane title, which is Medium",
                "Terminal",
                11.5,
                ChromeLabelWeight::Medium,
                0.0,
                false,
            ),
            // `.gbr .gtime` — a branch row's age, the string in the user's own
            // screenshot with half its `h` cut off.
            (
                "a Git row's age, in tabular figures",
                "1h",
                10.0,
                ChromeLabelWeight::Regular,
                0.0,
                true,
            ),
            (
                "an older age, which falls back to a date",
                "Aug 10",
                10.0,
                ChromeLabelWeight::Regular,
                0.0,
                true,
            ),
            // `.gcommit code` — the short hash at the far right of a commit row.
            (
                "a short hash, in tabular figures",
                "8c56194",
                10.5,
                ChromeLabelWeight::Regular,
                0.0,
                true,
            ),
            (
                "the same hash at 200%",
                "8c56194",
                21.0,
                ChromeLabelWeight::Regular,
                0.0,
                true,
            ),
        ] {
            let measured = measure_chrome_label(
                &mut font_system,
                text,
                size,
                weight,
                tracking,
                tabular,
                false,
            );
            let label = ChromeLabel {
                mono: false,
                text: text.to_owned(),
                // Laid out in a box far wider than the text wants, so the run
                // that comes back is the text's own width and not the box's.
                rect: [0.0, 0.0, 1_000.0, size * 1.4],
                clip: None,
                font_size_px: size,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: tracking,
                weight,
                tabular_numerals: tabular,
            };
            let laid_out = shape_chrome_labels(
                &mut font_system,
                std::slice::from_ref(&label),
                cap_height_ratio,
                1.0,
            )
            .first()
            .expect("the label shapes")
            .buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0_f32, f32::max);
            assert!(
                (measured - laid_out).abs() < 0.01,
                "{what}: measured {measured} against {laid_out} laid out"
            );
        }
    }

    /// PIN (T2 breathing): a mark that has only changed opacity is a *changed*
    /// mark, and `set_chrome` must say so.
    ///
    /// Red gate, and a silent one. `ChromeIcon`'s equality deliberately ignores
    /// the pixel bytes — two icons with the same key are the same raster, which
    /// is what makes the comparison cheap enough to run every frame. Opacity
    /// arrived on the same struct and would have inherited that exemption,
    /// which is precisely wrong: the whole point of carrying the breath beside
    /// the pixels instead of inside them is that the *pixels do not change*. An
    /// equality that reads only the pixels' identity therefore reports every
    /// frame of a breath as "nothing happened", and the icon never breathes.
    #[test]
    fn a_mark_that_only_changed_opacity_is_a_changed_mark() {
        let icon = |opacity: f32| ChromeIcon {
            key: "chrome-mark:i-folder:26x26:7a99ff".to_owned(),
            rect: [0.0, 0.0, 26.0, 26.0],
            rgba: Arc::from(vec![0_u8; 26 * 26 * 4]),
            width_px: 26,
            height_px: 26,
            opacity,
            clip: None,
        };
        assert_ne!(
            icon(1.0),
            icon(0.28),
            "two opacities of one mark are two different pictures"
        );
        assert_eq!(icon(0.28), icon(0.28), "and the same one is still the same");
        // The exemption that made this a trap is still in force: the bytes
        // themselves are not compared, because the key already identifies them.
        let mut recoloured = icon(1.0);
        recoloured.rgba = Arc::from(vec![255_u8; 26 * 26 * 4]);
        assert_eq!(
            recoloured,
            icon(1.0),
            "one key is one raster — comparing the bytes would cost a frame"
        );
    }

    /// PIN (T2 breathing / dead marks): a textured chrome quad carries a
    /// uniform opacity, and it carries it on every vertex of both triangles.
    ///
    /// The mock-up dims a mark in two places — `.ticon.working` breathes
    /// between 1 and .28 (line 246) and `.ticon-wrap.dead .ticon` holds .35
    /// (line 285) — and both are the *same artwork at a different opacity*.
    /// Baking that into the raster instead would re-rasterize the mark on
    /// every frame of every breath and defeat the mark cache, which is keyed
    /// on content; carrying it on the quad keeps one raster per mark forever.
    ///
    /// The per-vertex assertion is the one that matters: a quad is two
    /// triangles sharing an edge, and an opacity written to only the first
    /// three vertices interpolates across the seam and tears the mark in half
    /// diagonally.
    #[test]
    fn a_textured_quad_carries_one_opacity_on_every_vertex() {
        let vertices = math_quad_vertices(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 1.0, 1.0, 100, 100, 0.4);
        assert_eq!(vertices.len(), 6, "two triangles");
        for (index, vertex) in vertices.iter().enumerate() {
            assert!(
                (vertex.opacity - 0.4).abs() < f32::EPSILON,
                "vertex {index} carries {} rather than the quad's own .4",
                vertex.opacity
            );
        }
        // Fully opaque is the ordinary case and must survive the trip
        // untouched — a mark that is not dimmed is not "dimmed by 1.0", it is
        // the raster's own alpha and nothing else.
        let opaque = math_quad_vertices(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 1.0, 1.0, 100, 100, 1.0);
        assert!(opaque.iter().all(|vertex| vertex.opacity == 1.0));
    }

    /// The advance of each glyph of `text`, shaped the way the chrome shapes it.
    #[cfg(target_os = "windows")]
    fn glyph_advances(
        font_system: &mut FontSystem,
        text: &str,
        font_size_px: f32,
        weight: ChromeLabelWeight,
        tabular_numerals: bool,
    ) -> Vec<f32> {
        let label = ChromeLabel {
            mono: false,
            text: text.to_owned(),
            rect: [0.0, 0.0, 400.0, 15.0],
            font_size_px,
            color: [255, 255, 255],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight,
            tabular_numerals,
            clip: None,
        };
        let layouts = shape_chrome_labels(font_system, std::slice::from_ref(&label), 0.7, 1.0);
        layouts[0]
            .buffer
            .layout_runs()
            .next()
            .expect("the label shapes")
            .glyphs
            .iter()
            .map(|glyph| glyph.w)
            .collect()
    }

    /// The widest and narrowest figure advance differ by this much.
    #[cfg(target_os = "windows")]
    fn figure_advance_spread(
        font_system: &mut FontSystem,
        weight: ChromeLabelWeight,
        tabular_numerals: bool,
    ) -> f32 {
        let advances = glyph_advances(
            font_system,
            "0123456789",
            WINDOW_TAB_BADGE_FONT_LOGICAL_PX,
            weight,
            tabular_numerals,
        );
        assert_eq!(advances.len(), 10, "every digit shapes");
        let widest = advances.iter().copied().fold(f32::MIN, f32::max);
        let narrowest = advances.iter().copied().fold(f32::MAX, f32::min);
        widest - narrowest
    }

    /// PIN (T2 badge weight): `.panecount { font-weight: 600 }` (mock-up line
    /// 296) reaches the face and actually moves it.
    ///
    /// Red gate: the badge's digits were drawn at the face's regular weight,
    /// visibly lighter than the mock-up's, because `ChromeLabel` had no way to
    /// ask for anything else. The assertion that matters is not that the field
    /// exists but that it *changes the shaped result* — the chrome's face is a
    /// variable font whose default instance is `wght 400`, and a weight request
    /// that the axis quietly ignored would leave the badge exactly as thin as
    /// the bug it is fixing while every structural test still passed.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_badge_weight_reaches_the_variable_face_and_moves_its_axis() {
        let mut font_system = terminal_font_system();
        let badge = WINDOW_TAB_BADGE_FONT_LOGICAL_PX;
        let regular = glyph_advances(
            &mut font_system,
            "88",
            badge,
            ChromeLabelWeight::Regular,
            true,
        );
        let semibold = glyph_advances(
            &mut font_system,
            "88",
            badge,
            ChromeLabelWeight::SemiBold,
            true,
        );
        assert_eq!(regular.len(), 2, "both digits shape");
        assert_eq!(semibold.len(), 2);
        // Heavier strokes need more room: if the `wght` axis moved, the advance
        // moved with it. Equality here would mean the request was dropped.
        assert!(
            semibold.iter().sum::<f32>() > regular.iter().sum::<f32>(),
            "semibold {semibold:?} must outrun regular {regular:?} —              an unmoved axis is a badge still drawn at 400"
        );
        // `Regular` is what a label that says nothing gets, so every call site
        // that predates this field is untouched by it.
        assert_eq!(ChromeLabelWeight::default(), ChromeLabelWeight::Regular);
    }

    /// PIN (D4): `.pane.focused .panehead { font-weight: 500 }` (mock-up line
    /// 1644) reaches the face and actually moves it, at the size the pane head
    /// is drawn at.
    ///
    /// Red gate: the focused pane head took the mock-up's `--ink` and left its
    /// `font-weight` on the floor, because `ChromeLabelWeight` had a 400 and a
    /// 600 and nothing between them. Adding a variant is the easy half; the
    /// half that can silently fail is the axis, because 500 is an *interpolated*
    /// instance of Segoe UI Variable rather than a shipped file, and a request
    /// the shaper quietly dropped would leave a focused head exactly as light
    /// as the bug while the enum looked right.
    ///
    /// So the three weights are measured as an ordered run: 400 < 500 < 600.
    /// A pair alone would pass if `Medium` were wired to `SEMIBOLD` by a
    /// copy-paste, which is precisely how a three-armed `match` goes wrong.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_focused_pane_head_weight_reaches_the_variable_face_and_moves_its_axis() {
        let mut font_system = terminal_font_system();
        // A pane head's own caption at a pane head's own size: the axis has to
        // move where the design asks for it, not merely somewhere.
        let run = |font_system: &mut FontSystem, weight| -> f32 {
            glyph_advances(
                font_system,
                "Terminal",
                SEAT_TITLE_FONT_LOGICAL_PX,
                weight,
                false,
            )
            .iter()
            .sum()
        };
        let regular = run(&mut font_system, ChromeLabelWeight::Regular);
        let medium = run(&mut font_system, ChromeLabelWeight::Medium);
        let semibold = run(&mut font_system, ChromeLabelWeight::SemiBold);
        assert!(
            regular < medium,
            "500 must outrun 400 — an unmoved axis is a head still drawn at \
             regular: saw {regular} against {medium}"
        );
        assert!(
            medium < semibold,
            "and 500 must stay under 600, or `Medium` is `SemiBold` wearing a \
             different name: saw {medium} against {semibold}"
        );
    }

    /// PIN (T2 badge): `.panecount { font-variant-numeric: tabular-nums }`
    /// (mock-up line 302) — the pane count must not wobble as it counts.
    ///
    /// The badge is a fixed box with a number centred in it, so a face whose
    /// figures have their own widths shifts the glyphs every time the count
    /// changes. This face's figures very much do: measured at the badge's own
    /// 10px, `1` advances 3.79 against `0`'s 5.39, so the declaration is not
    /// decoration and asking for `tnum` is not optional.
    ///
    /// Both halves are asserted, because either one alone can pass while the
    /// badge still wobbles: that the feature is *needed* (off, the figures are
    /// plainly proportional) and that it *works* (on, they are one advance).
    #[cfg(target_os = "windows")]
    #[test]
    fn tabular_figures_are_needed_by_this_face_and_delivered_by_the_feature() {
        let mut font_system = terminal_font_system();
        for weight in [ChromeLabelWeight::Regular, ChromeLabelWeight::SemiBold] {
            let proportional = figure_advance_spread(&mut font_system, weight, false);
            assert!(
                proportional > 1.0,
                "{weight:?}: figures spread {proportional}px without `tnum` —                  if this face ever ships tabular defaults, this pin is the                  place that says the feature request became redundant"
            );
            let tabular = figure_advance_spread(&mut font_system, weight, true);
            // A quarter pixel at the badge's 10px. Segoe UI Variable's tabular
            // figures are exact at `wght 400` and carry one interpolation
            // artefact at 600 — `7` comes back 0.19px narrow — which is a
            // twentieth of the wobble the feature just removed and lands well
            // inside a pixel the badge is centred in.
            assert!(
                tabular < 0.25,
                "{weight:?}: figures still spread {tabular}px with `tnum` on"
            );
            assert!(
                tabular < proportional / 5.0,
                "{weight:?}: `tnum` must actually reach this face"
            );
        }
    }

    /// PIN (chrome font pass): a chrome label is shaped in a real UI sans — the
    /// mock-up's own stack — and never in the emoji face the request used to land
    /// on, while a title that carries an emoji still reaches that face for the
    /// emoji alone.
    ///
    /// Red gate: before this pass every glyph of "PowerShell" came back
    /// `Segoe UI Emoji`, because `Family::SansSerif` had nothing else to resolve
    /// to in a terminal-only font database. The first assertion is exactly that
    /// bug, and the last one is the reason the fix cannot be "drop the emoji
    /// faces": a session title may legitimately contain one.
    #[cfg(target_os = "windows")]
    #[test]
    fn chrome_labels_shape_in_a_ui_sans_and_still_reach_the_emoji_face() {
        let mut font_system = terminal_font_system();
        let label = ChromeLabel {
            mono: false,
            text: "✳ PowerShell".to_owned(),
            rect: [0.0, 0.0, 400.0, 34.0],
            font_size_px: WINDOW_TAB_FONT_LOGICAL_PX,
            color: [255, 255, 255],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        };
        let layouts = shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7, 1.0);
        let run = layouts[0]
            .buffer
            .layout_runs()
            .next()
            .expect("the label shapes");
        let families: Vec<(char, String)> = run
            .glyphs
            .iter()
            .map(|glyph| {
                (
                    label.text[glyph.start..glyph.end]
                        .chars()
                        .next()
                        .expect("a glyph covers at least one character"),
                    glyph_family(&font_system, glyph),
                )
            })
            .collect();
        let letters: Vec<&(char, String)> = families
            .iter()
            .filter(|(character, _)| character.is_ascii_alphabetic())
            .collect();
        assert_eq!(letters.len(), "PowerShell".len(), "every letter shapes");
        for (character, family) in &letters {
            assert!(
                !family.contains("Emoji"),
                "'{character}' shaped in {family} — the chrome's sans is not an emoji face"
            );
            assert!(
                family == "Segoe UI Variable" || family == "Segoe UI",
                "'{character}' shaped in {family}, which is neither face \
                 {CHROME_SANS_FONT_FILES:?} carries"
            );
        }
        let (_, emoji_family) = families
            .iter()
            .find(|(character, _)| *character == '✳')
            .expect("the emoji in a title still shapes");
        assert!(
            emoji_family.contains("Emoji") || emoji_family.contains("Symbol"),
            "an emoji in a title must fall back to a face that has it, saw {emoji_family}"
        );
    }

    /// PIN (i18n slice, 2026-08-17) — **a Chinese chrome label shapes in a CJK
    /// face this file names, and never in a symbol or emoji one.**
    ///
    /// **This one was green before the chain existed, and that is the point.**
    /// cosmic-text's own Windows table looks an ideograph up by *locale*, this
    /// `FontSystem`'s locale is `"en-US"` and always will be, and the single
    /// entry that answers for it is `Microsoft YaHei UI` — which the machine
    /// this was written on happens to have. Behind that one entry came
    /// `Segoe UI`, `Segoe UI Emoji`, `Segoe UI Symbol`: a Windows without YaHei
    /// would have drawn the whole settings dialog in a symbol face, and no test
    /// anywhere would have said so. What is being pinned is therefore not a bug
    /// that was fixed but a fact that was previously an accident.
    ///
    /// Both halves are asserted, and the second is the one that has teeth: it is
    /// not enough that the glyphs resolve, they have to resolve *inside the
    /// declared chain*. A face that is not on the list is the fallback table
    /// having found something we never chose.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_chinese_chrome_label_shapes_in_a_named_cjk_face_and_never_in_a_symbol_one() {
        // A row title, a picker item, an option and a category heading: the four
        // shapes of Chinese this dialog actually draws.
        const CHINESE: &str = "设置语言外观常规重启以切换";
        let mut font_system = terminal_font_system();
        let label = ChromeLabel {
            mono: false,
            text: CHINESE.to_owned(),
            rect: [0.0, 0.0, 600.0, 34.0],
            font_size_px: WINDOW_TAB_FONT_LOGICAL_PX,
            color: [255, 255, 255],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        };
        let layouts = shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7, 1.0);
        let run = layouts[0]
            .buffer
            .layout_runs()
            .next()
            .expect("the label shapes");
        let glyphs: Vec<_> = run.glyphs.iter().collect();
        assert_eq!(
            glyphs.len(),
            CHINESE.chars().count(),
            "every ideograph gets a glyph of its own"
        );
        for glyph in glyphs {
            let character = CHINESE[glyph.start..glyph.end]
                .chars()
                .next()
                .expect("a glyph covers at least one character");
            assert_ne!(
                glyph.glyph_id, 0,
                "'{character}' came back as .notdef — no face on the chain has it"
            );
            let family = glyph_family(&font_system, glyph);
            assert!(
                !family.contains("Emoji") && !family.contains("Symbol"),
                "'{character}' shaped in {family} — an ideograph must never reach a \
                 symbol or emoji face"
            );
            assert!(
                CJK_FALLBACK_FAMILIES.contains(&family.as_str()),
                "'{character}' shaped in {family}, which is not on the declared \
                 chain {CJK_FALLBACK_FAMILIES:?} — the fallback table found a face \
                 nobody chose"
            );
        }
    }

    /// PIN — the chain is ordered, and the order is the design.
    ///
    /// Simplified Chinese before Traditional before Japanese before Korean before
    /// the compatibility face, and no symbol or emoji face anywhere on it. This
    /// reads the constant rather than shaping anything, because the order is a
    /// ruling and a machine that happens to lack the first three faces would
    /// still have to keep it.
    #[test]
    fn the_cjk_chain_puts_simplified_first_and_the_bitmap_serif_last() {
        let index = |family: &str| {
            CJK_FALLBACK_FAMILIES
                .iter()
                .position(|it| *it == family)
                .unwrap_or_else(|| panic!("{family} must be on the chain"))
        };
        assert_eq!(
            index("Microsoft YaHei UI"),
            0,
            "the product's Chinese is Simplified, so its UI face leads"
        );
        assert!(index("Microsoft YaHei") < index("DengXian"));
        assert!(index("DengXian") < index("Microsoft JhengHei UI"));
        assert!(index("Microsoft JhengHei UI") < index("Yu Gothic UI"));
        assert!(index("Yu Gothic UI") < index("Malgun Gothic"));
        assert!(
            index("Malgun Gothic") < index("SimSun"),
            "SimSun is the face that is always there, which is why it is the one \
             reached only when nothing else was"
        );
        for family in CJK_FALLBACK_FAMILIES {
            assert!(
                !family.contains("Emoji") && !family.contains("Symbol"),
                "{family} is on the ideograph chain and is not an ideograph face"
            );
        }
    }

    /// PIN — the active tab's title sits on the same axis its mark does.
    ///
    /// `seats.rs` centres the mark box on the tab's own centre, so the title has
    /// to put its cap band there too: half-leading alone centres the face's
    /// ascent+descent box, and the chrome face's is asymmetric enough to leave a
    /// visible step between the mark and the word (measured on screen at 200%:
    /// 3.5 physical pixels).
    #[cfg(target_os = "windows")]
    #[test]
    fn tab_title_cap_band_centres_on_the_tab() {
        let mut font_system = terminal_font_system();
        let mut swash_cache = SwashCache::new();
        let cap_height_ratio = chrome_cap_height_ratio(&mut font_system, &mut swash_cache)
            .expect("the chrome sans face publishes or renders a cap height");
        for dpi_milli in [1000_u32, 1250, 1500, 2000] {
            let scale = dpi_milli as f32 / 1000.0;
            let title = (WINDOW_TITLE_BAR_LOGICAL_PX * scale).round();
            let tab_height = (WINDOW_TAB_HEIGHT_LOGICAL_PX * scale).round();
            let tab_top = title - tab_height;
            let label = ChromeLabel {
                mono: false,
                text: "PowerShell".to_owned(),
                rect: [64.0, tab_top, 400.0, title],
                font_size_px: WINDOW_TAB_FONT_LOGICAL_PX * scale,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            };
            let rect_centre = (label.rect[1] + label.rect[3]) / 2.0;
            let cap_centre = cap_band_centre(&mut font_system, cap_height_ratio, &label);
            assert!(
                (cap_centre - rect_centre).abs() <= 0.5,
                "tab title cap band off the tab's axis at {dpi_milli} milli-DPI:                  cap centre {cap_centre}, tab centre {rect_centre}"
            );
        }
    }

    /// PIN — a pane head's title sits on the same axis its mark does.
    #[cfg(target_os = "windows")]
    #[test]
    fn pane_head_title_cap_band_centres_on_the_head() {
        let mut font_system = terminal_font_system();
        let mut swash_cache = SwashCache::new();
        let cap_height_ratio = chrome_cap_height_ratio(&mut font_system, &mut swash_cache)
            .expect("the chrome sans face publishes or renders a cap height");
        for dpi_milli in [1000_u32, 1250, 1500, 2000] {
            let scale = dpi_milli as f32 / 1000.0;
            // The head's *content* box, which is the border box less its
            // hairline: `* { box-sizing: border-box }` makes `.panehead`'s 30px
            // twenty-nine rows of flex row plus one of border, and the caption
            // is centred in the twenty-nine.
            let bar = (SEAT_TITLE_BAR_LOGICAL_PX * scale).round()
                - (SEAT_TITLE_EDGE_LOGICAL_PX * scale).max(1.0);
            let label = ChromeLabel {
                mono: false,
                text: "Terminal".to_owned(),
                rect: [48.0, 0.0, 400.0, bar],
                font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            };
            let rect_centre = (label.rect[1] + label.rect[3]) / 2.0;
            let cap_centre = cap_band_centre(&mut font_system, cap_height_ratio, &label);
            assert!(
                (cap_centre - rect_centre).abs() <= 0.5,
                "pane head title cap band off the head's axis at {dpi_milli} milli-DPI:                  cap centre {cap_centre}, head centre {rect_centre}"
            );
        }
    }

    /// PIN (settings dialog): the two marks the mock-up sets as *text* rather
    /// than as symbols — the combo's `▼` and a picker item's `✓` — reach a face
    /// that has them.
    ///
    /// Red gate: glyph id 0 is `.notdef`, the empty box a face returns for a
    /// character it does not carry. Before the fallback list included a symbol
    /// face this is exactly what a tracked-down tofu would look like, and the
    /// assertion is the one thing that tells a rendered chevron apart from a
    /// rendered rectangle.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_chrome_face_can_set_the_combos_own_marks() {
        let mut font_system = terminal_font_system();
        for (what, text, size) in [
            ("the chevron", "\u{25bc}", 8.5_f32),
            ("the tick", "\u{2713}", 11.0),
        ] {
            let label = ChromeLabel {
                mono: false,
                text: text.to_owned(),
                rect: [0.0, 0.0, 40.0, 28.0],
                font_size_px: size,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            };
            let layouts =
                shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7, 1.0);
            let run = layouts
                .first()
                .and_then(|layout| layout.buffer.layout_runs().next())
                .unwrap_or_else(|| panic!("{what} must shape"));
            let glyphs: Vec<_> = run.glyphs.iter().collect();
            assert_eq!(glyphs.len(), 1, "{what} is one character");
            assert_ne!(
                glyphs[0].glyph_id, 0,
                "{what} came back .notdef — the chrome's face stack has no glyph for it, \
                 which on screen is a tofu box where the mark should be"
            );
            assert!(run.line_w > 0.0, "{what} takes width on the line");
        }
    }

    /// PIN (`.group-label { letter-spacing: .05em }`): tracking is real advance,
    /// and it is *em*, so `.05` adds exactly `.05 × font-size` per glyph.
    ///
    /// Red gate: two of them. Tracking that never reached the shaper would leave
    /// both runs the same width. Tracking taken for pixels — which is what this
    /// pass shipped for one screenshot — would widen a 22px heading by 1.1 em a
    /// glyph, over twenty times the asked-for amount, and the exact-width
    /// assertion is what tells those two apart.
    #[cfg(target_os = "windows")]
    #[test]
    fn letter_spacing_is_em_and_widens_the_run_by_exactly_that_much() {
        let mut font_system = terminal_font_system();
        let text = "APPEARANCE";
        let width_of = |spacing: f32, size: f32, font_system: &mut FontSystem| {
            let label = ChromeLabel {
                mono: false,
                text: text.to_owned(),
                rect: [0.0, 0.0, 4000.0, 20.0],
                font_size_px: size,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: spacing,
                weight: ChromeLabelWeight::Regular,
                tabular_numerals: false,
                clip: None,
            };
            shape_chrome_labels(font_system, std::slice::from_ref(&label), 0.7, 1.0)[0]
                .buffer
                .layout_runs()
                .map(|run| run.line_w)
                .fold(0.0_f32, f32::max)
        };
        for size in [11.0_f32, 22.0] {
            let plain = width_of(0.0, size, &mut font_system);
            let tracked = width_of(0.05, size, &mut font_system);
            let expected = plain + 0.05 * size * text.chars().count() as f32;
            assert!(
                (tracked - expected).abs() <= 0.5,
                "at {size}px, .05em must add .05 x size per glyph: {tracked} vs {expected} \
                 (untracked {plain})"
            );
        }
    }

    /// PIN (modal overlay): an overlay quad reaches the pipeline carrying its
    /// own alpha, and the scrim's `rgba(15,15,15,.35)` survives the trip.
    ///
    /// Red gate: the chrome path this one sits beside is opaque by construction —
    /// route an overlay quad through `surface_pixel_rect` and the alpha comes
    /// back 1.0, which on screen is a black window instead of a dimmed one.
    #[test]
    fn an_overlay_quad_keeps_its_alpha_where_a_chrome_quad_has_none() {
        let palette = chrome_palette();
        let scrim = surface_pixel_rect_with_alpha(
            [0.0, 0.0, 1280.0, 800.0],
            palette.modal_scrim,
            f32::from(palette.modal_scrim_alpha) / 255.0,
            1280,
            800,
        );
        assert!(
            (scrim.color[3] - 0.35).abs() < 0.005,
            "the scrim's alpha must reach the vertex, saw {}",
            scrim.color[3]
        );
        let opaque = surface_pixel_rect([0.0, 0.0, 1280.0, 800.0], palette.modal_scrim, 1280, 800);
        assert_eq!(opaque.color[3], 1.0, "a chrome quad has no alpha to carry");
        assert_eq!(
            [scrim.color[0], scrim.color[1], scrim.color[2]],
            [opaque.color[0], opaque.color[1], opaque.color[2]],
            "only the alpha differs; the colour is the same linear triple"
        );
        assert_eq!(scrim.rect, opaque.rect, "and so is the placement");
    }

    /// PIN (float-window craft, public form): the rounded primitives a composed
    /// overlay is built from are the same analytic coverage the peek flyout's
    /// own corners are, and they carry the caller's alpha through.
    #[test]
    fn the_public_rounded_overlay_primitives_are_the_float_windows_own_coverage() {
        let frame = [40.0, 20.0, 280.0, 180.0];
        let radius = 10.0_f32;
        let fills = rounded_overlay_fill(frame, radius, [0x20, 0x20, 0x20], 0.5);
        assert!(!fills.is_empty());
        let partial = fills
            .iter()
            .filter(|quad| quad.alpha > 0.0 && quad.alpha < 0.5)
            .count();
        assert!(
            partial >= radius as usize,
            "a rounded corner spends at least one partial pixel per row, saw {partial}"
        );
        let full = fills.iter().filter(|quad| quad.alpha == 0.5).count();
        assert!(
            full > 0,
            "the straight middle carries the caller's own alpha"
        );
        assert!(
            fills.iter().all(|quad| quad.alpha <= 0.5),
            "coverage may only take alpha away, never add it"
        );
        // The lift is a ring with the hole a browser's own `box-shadow` leaves:
        // it may reach into a corner the round cut away, but never under a pixel
        // the box covers whole — otherwise a translucent hairline is drawn over
        // its own shadow and reads twice as dark.
        let halo = rounded_overlay_halo(frame, radius, 3.0, [0, 0, 0], 0.18);
        assert!(!halo.is_empty());
        let solid = [
            frame[0] + radius,
            frame[1] + radius,
            frame[2] - radius,
            frame[3] - radius,
        ];
        for quad in &halo {
            let overlaps = quad.rect[0] < solid[2]
                && quad.rect[2] > solid[0]
                && quad.rect[1] < solid[3]
                && quad.rect[3] > solid[1];
            assert!(
                !overlaps,
                "the lift must not be drawn under the box it lifts: {quad:?}"
            );
        }
        // And it does reach around every side, not only two of them.
        assert!(halo.iter().any(|quad| quad.rect[3] <= frame[1]), "above");
        assert!(halo.iter().any(|quad| quad.rect[1] >= frame[3]), "below");
        assert!(halo.iter().any(|quad| quad.rect[2] <= frame[0]), "left");
        assert!(halo.iter().any(|quad| quad.rect[0] >= frame[2]), "right");
    }

    /// PIN — **a floating surface's lift is one soft shadow, not a set of rings**
    /// (user report + screenshot, 2026-08-13: three concentric squares around the
    /// glance card).
    ///
    /// The report is about a *shape*, and this is that shape stated as numbers.
    /// Walk one pixel at a time away from the box and read the alpha:
    ///
    /// * it never goes back up — a shadow that brightened outward would be a ring;
    /// * no two neighbouring pixels differ by more than [`MAX_STEP`], so nothing
    ///   in the falloff is a cliff;
    /// * **no run of one alpha is wider than one band**, which is the assertion
    ///   the report actually asked for: a visible ring *is* a plateau, and at 28px
    ///   of reach the old two-ring lift had two of them fourteen pixels wide;
    /// * it starts at the caller's own alpha and has run out by the reach.
    ///
    /// Mutation: put the two rings back —
    /// `halo(rect, radius, extent, .., a) ++ halo(rect, radius, extent / 2, .., a)`
    /// — and the plateau assertion fails at the first sample (a run 14 pixels
    /// wide against a band of 2), with the step assertion failing beside it where
    /// the outer ring stops.
    #[test]
    fn a_floating_surfaces_lift_falls_off_instead_of_standing_in_rings() {
        /// The largest alpha two neighbouring pixels of the falloff may differ by.
        /// A shadow is steepest right against the box — that is what a blurred
        /// step edge does — so this is not tight; it is the bound that separates
        /// "steep" from "a ring ending".
        const MAX_STEP: f32 = 0.08;
        let frame = [200.0, 100.0, 500.0, 364.0];
        let (radius, extent, alpha) = (8.0_f32, 28.0_f32, 0.5_f32);
        let shadow = rounded_overlay_shadow(frame, radius, extent, [0, 0, 0], alpha);
        assert!(!shadow.is_empty());
        for quad in &shadow {
            let overlaps = quad.rect[0] < frame[2] - radius
                && quad.rect[2] > frame[0] + radius
                && quad.rect[1] < frame[3] - radius
                && quad.rect[3] > frame[1] + radius;
            assert!(
                !overlaps,
                "the lift is still not drawn under its box: {quad:?}"
            );
        }

        // A scanline through the box's own middle, walking left out of it: the
        // straight flank of every band, one sample per physical pixel.
        let middle = (frame[1] + frame[3]) / 2.0;
        let at = |x: f32| -> f32 {
            shadow
                .iter()
                .filter(|quad| {
                    quad.rect[0] <= x
                        && x < quad.rect[2]
                        && quad.rect[1] <= middle
                        && middle < quad.rect[3]
                })
                .map(|quad| quad.alpha)
                .sum()
        };
        let samples: Vec<f32> = (0..extent as i32)
            .map(|step| at(frame[0] - 1.0 - step as f32))
            .collect();

        assert!(
            (samples[0] - alpha).abs() <= f32::EPSILON,
            "the shadow starts at exactly the strength the caller asked for: {}",
            samples[0]
        );
        let band = (extent / OVERLAY_SHADOW_RINGS as f32).ceil() as usize;
        let mut run = 1_usize;
        for pair in samples.windows(2) {
            let (near, far) = (pair[0], pair[1]);
            assert!(
                far <= near + f32::EPSILON,
                "the falloff never brightens outward: {samples:?}"
            );
            assert!(
                near - far <= MAX_STEP,
                "no cliff in the falloff: {near} → {far} in {samples:?}"
            );
            if (near - far).abs() <= f32::EPSILON {
                run += 1;
                assert!(
                    run <= band,
                    "no plateau wider than one band ({band}px): {samples:?}"
                );
            } else {
                run = 1;
            }
        }
        assert!(
            *samples.last().expect("the reach is sampled") <= MAX_STEP,
            "and it has run out by the reach: {samples:?}"
        );
    }

    /// A preview float's picture can be zoomed past its own window — at 400% the
    /// drawn rectangle runs off every side of the body — and the window has no
    /// scissor of its own to catch it: a layer's marks are one draw list under
    /// the whole-surface scissor. So the crop is geometry, and the pin is that
    /// the texture coordinates are cropped *with* it. Crop the quad alone and the
    /// picture is not clipped but squeezed, which is the same picture at the
    /// wrong scale and reads as a rendering bug rather than as a window edge.
    #[test]
    fn a_cropped_icon_samples_only_the_part_of_itself_that_is_still_visible() {
        // A 100×100 tile at the origin, seen through a box that cuts its left
        // quarter and its bottom half away.
        let (quad, uv) =
            cropped_icon_quad([0.0, 0.0, 100.0, 100.0], Some([25.0, 0.0, 100.0, 50.0]))
                .expect("the visible part of a partly clipped tile");
        assert_eq!(
            quad,
            [25.0, 0.0, 100.0, 50.0],
            "the quad is the intersection"
        );
        assert_eq!(
            uv,
            [0.25, 0.0, 1.0, 0.5],
            "and it samples exactly the fraction of the raster that box covers"
        );

        let (whole, uv) = cropped_icon_quad([10.0, 10.0, 20.0, 20.0], Some([0.0, 0.0, 40.0, 40.0]))
            .expect("a tile wholly inside its clip is untouched");
        assert_eq!(whole, [10.0, 10.0, 20.0, 20.0]);
        assert_eq!(uv, [0.0, 0.0, 1.0, 1.0]);

        assert!(
            cropped_icon_quad([0.0, 0.0, 10.0, 10.0], Some([50.0, 50.0, 90.0, 90.0])).is_none(),
            "a tile entirely outside its clip is not a draw call"
        );

        let (bare, uv) = cropped_icon_quad([3.0, 4.0, 5.0, 6.0], None)
            .expect("no clip is not a clip to nothing");
        assert_eq!(bare, [3.0, 4.0, 5.0, 6.0]);
        assert_eq!(uv, [0.0, 0.0, 1.0, 1.0]);
    }

    /// M136: `.tip { opacity: 0; transition: opacity .09s ease }` fades a *popup*,
    /// not a fill — so the layer's opacity has to reach every channel the layer
    /// draws in. A fold that reached the fills and forgot the marks would show as
    /// a tooltip whose icon arrives before its box.
    #[test]
    fn a_layers_opacity_reaches_its_fills_and_its_marks_alike() {
        let layer = OverlayLayer {
            quads: vec![
                OverlayQuad {
                    rect: [0.0, 0.0, 10.0, 10.0],
                    color: [1, 2, 3],
                    alpha: 1.0,
                },
                // A corner's coverage is already an alpha. The fade multiplies it
                // rather than replacing it, or every rounded corner would square
                // off for the duration of the fade.
                OverlayQuad {
                    rect: [0.0, 0.0, 1.0, 1.0],
                    color: [1, 2, 3],
                    alpha: 0.5,
                },
            ],
            labels: Vec::new(),
            icons: vec![ChromeIcon {
                key: "mark".to_owned(),
                rect: [0.0, 0.0, 8.0, 8.0],
                rgba: Arc::from(vec![0_u8; 4].into_boxed_slice()),
                width_px: 1,
                height_px: 1,
                opacity: 0.5,
                clip: None,
            }],
            opacity: 0.25,
            body: None,
            grounds: Vec::new(),
        };

        let quads = layer.faded_quads();
        assert!((quads[0].alpha - 0.25).abs() < 1e-6, "{:?}", quads[0].alpha);
        assert!(
            (quads[1].alpha - 0.125).abs() < 1e-6,
            "{:?}",
            quads[1].alpha
        );
        // Colour is untouched: fading is an alpha, and folding it into the ink
        // would darken the tip toward black instead of dissolving it into
        // whatever stands behind it.
        assert_eq!(quads[0].color, [1, 2, 3]);

        let icons = layer.faded_icons();
        assert!(
            (icons[0].opacity - 0.125).abs() < 1e-6,
            "{:?}",
            icons[0].opacity
        );
        // The raster's identity must not move with the fade, or a 90ms fade mints
        // a fresh texture on every frame of itself.
        assert_eq!(icons[0].key, "mark");
    }

    /// The same fold, for the one channel that cannot carry it on a struct field:
    /// a glyph's colour is built in the shaper and nowhere else.
    #[test]
    #[cfg(target_os = "windows")]
    fn a_layers_opacity_reaches_its_letters() {
        let mut font_system = terminal_font_system();
        let label = ChromeLabel {
            mono: false,
            text: "Settings".to_owned(),
            rect: [0.0, 0.0, 400.0, 20.0],
            font_size_px: 11.0,
            color: [10, 20, 30],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            clip: None,
        };
        let opaque =
            shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7, 1.0)[0].color;
        let half =
            shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7, 0.5)[0].color;
        assert_eq!(opaque.as_rgba(), [10, 20, 30, 255]);
        assert_eq!(half.as_rgba(), [10, 20, 30, 128]);
    }

    /// A layer nobody can see is not a layer: it would still cost a text renderer
    /// and a pass through three channels to draw exactly nothing, every frame the
    /// tooltip spends waiting for its delay to elapse.
    #[test]
    fn a_layer_faded_to_nothing_counts_as_empty() {
        let solid = OverlayLayer {
            quads: vec![OverlayQuad {
                rect: [0.0, 0.0, 10.0, 10.0],
                color: [1, 2, 3],
                alpha: 1.0,
            }],
            ..Default::default()
        };
        assert!(!solid.is_empty());
        assert!(
            OverlayLayer {
                opacity: 0.0,
                ..solid.clone()
            }
            .is_empty()
        );
        // And a layer that never mentioned opacity is fully there — CSS's own
        // initial value, not a derived zero.
        assert_eq!(OverlayLayer::default().opacity, 1.0);
    }

    #[test]
    fn lost_surface_reconfigures_instead_of_becoming_fatal() {
        assert_eq!(
            surface_failure_policy(SurfaceFailure::Lost),
            SurfaceFailurePolicy::Reconfigure
        );
        assert_eq!(
            surface_failure_policy(SurfaceFailure::Validation),
            SurfaceFailurePolicy::FatalValidation
        );
    }

    fn fade_metrics() -> CellMetrics {
        CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 8.0,
            scale_factor: 1.0,
            ascii_baseline_px: 15.0,
            primary_advance_px: 10.0,
            primary_cap_height_px: 11.0,
            primary_cap_center_y_px: 9.0,
        }
    }

    /// Band 400px wide starting at the block's own left edge, holding a formula `width_px` wide
    /// scrolled `scroll_px` from its start.
    fn fade_case(width_px: u32, scroll_px: u32) -> (MathBlockPlacement, MathBlockGeometry, f32) {
        let metrics = fade_metrics();
        let left = math_block_left_px(metrics, 0, true);
        let right = left + 400.0;
        let mut placement = test_math_placement("f", 0, 20 * SUBPIXELS_PER_PX, 4);
        placement.left_subpixels = 0;
        placement.display = MathBlockDisplay::Rendered;
        placement.horizontal_scroll_px = scroll_px;
        placement.artifact.width_px = width_px;
        placement.artifact.render_scale_milli = 1000;
        let geometry = MathBlockGeometry {
            block: [left, 0.0, right, 20.0],
            clip: [left, 0.0, right, 20.0],
            eye: None,
            copy: None,
        };
        (placement, geometry, right)
    }

    /// A formula that fits its band is cut off on neither side, so it gets no fade at all — the
    /// cue has to mean something, which means it cannot be always on.
    #[test]
    fn a_formula_that_fits_its_band_gets_no_fade() {
        let (placement, geometry, _) = fade_case(390, 0);
        assert!(math_overflow_fade_slabs(fade_metrics(), &placement, &geometry).is_empty());
    }

    /// Unscrolled and over-wide: content continues to the right only, so only the right edge
    /// fades. A fade on the left here would claim there is something behind the start.
    #[test]
    fn an_unscrolled_over_wide_formula_fades_only_on_the_right() {
        let metrics = fade_metrics();
        let (placement, geometry, right) = fade_case(1200, 0);
        let slabs = math_overflow_fade_slabs(metrics, &placement, &geometry);
        assert_eq!(slabs.len(), MATH_OVERFLOW_FADE_STEPS);
        let fade_width = metrics.cell_width_px * MATH_OVERFLOW_FADE_CELLS;
        for (rect, _) in &slabs {
            assert!(
                rect[0] >= right - fade_width - 0.01 && rect[2] <= right + 0.01,
                "right fade slab {rect:?} escaped the band's right edge"
            );
        }
    }

    /// Scrolled into the middle of a long formula, both ends are cut off and both fade — which
    /// is what makes the pair readable as a position rather than a decoration.
    #[test]
    fn a_formula_scrolled_to_its_middle_fades_on_both_sides() {
        let metrics = fade_metrics();
        let (placement, geometry, _) = fade_case(1200, 400);
        let slabs = math_overflow_fade_slabs(metrics, &placement, &geometry);
        assert_eq!(slabs.len(), MATH_OVERFLOW_FADE_STEPS * 2);
    }

    /// Panned all the way to the end, nothing remains to the right, so that fade goes out and
    /// only the left one is left burning.
    #[test]
    fn a_formula_scrolled_to_its_end_fades_only_on_the_left() {
        let metrics = fade_metrics();
        let (placement, geometry, _) = fade_case(1200, 800);
        let left_edge = math_block_left_px(metrics, 0, true);
        let slabs = math_overflow_fade_slabs(metrics, &placement, &geometry);
        assert_eq!(slabs.len(), MATH_OVERFLOW_FADE_STEPS);
        let fade_width = metrics.cell_width_px * MATH_OVERFLOW_FADE_CELLS;
        for (rect, _) in &slabs {
            assert!(
                rect[0] >= left_edge - 0.01 && rect[2] <= left_edge + fade_width + 0.01,
                "left fade slab {rect:?} escaped the band's left edge"
            );
        }
    }

    /// The ramp runs the right way round: densest against the cut edge, gone by the inner end.
    /// Reversed, the fade would read as a bar drawn across the formula.
    #[test]
    fn the_fade_is_densest_against_the_cut_edge() {
        let metrics = fade_metrics();
        let (placement, geometry, right) = fade_case(1200, 0);
        let slabs = math_overflow_fade_slabs(metrics, &placement, &geometry);
        let outermost = slabs
            .iter()
            .max_by(|a, b| a.0[2].total_cmp(&b.0[2]))
            .expect("an over-wide formula must fade");
        let innermost = slabs
            .iter()
            .min_by(|a, b| a.0[0].total_cmp(&b.0[0]))
            .expect("an over-wide formula must fade");
        assert!(
            outermost.1 > innermost.1,
            "coverage {} at the edge must exceed {} inside it",
            outermost.1,
            innermost.1
        );
        assert!(outermost.1 <= 1.0 && innermost.1 >= 0.0);
        assert!((outermost.0[2] - right).abs() <= 0.01);
    }

    /// Source view is plain text the pane already wraps and scrolls by its own rules; the band
    /// fade belongs to the rendered raster only.
    #[test]
    fn source_view_never_fades() {
        let (mut placement, geometry, _) = fade_case(1200, 0);
        placement.display = MathBlockDisplay::Source;
        assert!(math_overflow_fade_slabs(fade_metrics(), &placement, &geometry).is_empty());
    }

    /// PIN — **a run's `font_scale` shrinks its glyphs and leaves its paragraph's
    /// leading alone** (markdown's `code { font-size: 85% }`, `DESIGN.md`
    /// §7.1.3i).
    ///
    /// Both halves matter and only one of them is obvious. A code span that came
    /// out the same size as the prose around it is the reported complaint; a code
    /// span that dragged the line box down with it would make one line of a
    /// paragraph taller than its neighbours, which is a worse density problem
    /// than the one being fixed.
    ///
    /// MUTATION: pass `Metrics::new(size * scale, line_height * scale)` instead
    /// of keeping the paragraph's leading and the second assertion goes red.
    #[test]
    fn a_runs_font_scale_shrinks_its_glyphs_and_not_its_line() {
        let mut font_system = terminal_font_system();
        let metrics = Metrics::new(13.0, 21.0);
        let mut width = |scale: f32| {
            let mut buffer = Buffer::new(&mut font_system, metrics);
            buffer.set_wrap(Wrap::None);
            buffer.set_size(None, Some(metrics.line_height));
            set_preview_runs(
                &mut buffer,
                &[PreviewRun {
                    text: "monospaced_identifier".to_owned(),
                    color: [0, 0, 0],
                    mono: true,
                    bold: false,
                    font_scale: scale,
                }],
                0.0,
                metrics,
            );
            buffer.shape_until_scroll(&mut font_system, false);
            let runs: Vec<_> = buffer.layout_runs().collect();
            assert_eq!(runs.len(), 1, "one unwrapped line");
            (runs[0].line_w, runs[0].line_height)
        };
        let (full, full_line) = width(1.0);
        let (small, small_line) = width(0.85);
        assert!(
            small < full,
            "85% of the paragraph's size is narrower than all of it: {small} against {full}"
        );
        assert!(
            (small / full - 0.85).abs() < 0.05,
            "and narrower by about the ratio asked for: {}",
            small / full
        );
        assert_eq!(
            small_line, full_line,
            "the line box is the paragraph's, whatever the run is set at"
        );
    }

    /// The multiwindow block's slice A2 (= the web preview block's slice 1).
    ///
    /// None of it needs an adapter, and that is the point: a headless probe
    /// cannot make a DirectComposition visual, so the decision that matters —
    /// which alpha mode a target is configured with, and what happens when the
    /// adapter will not give it — is a pure function taking the offered list,
    /// and these tests hand it the two lists dx12 actually produces.
    mod composition_ground {
        use super::*;

        /// The two lists, verbatim from `wgpu-hal-30.0.0/src/dx12/adapter.rs:1364`.
        fn dx12_offers(target: WindowTargetKind) -> Vec<wgpu::CompositeAlphaMode> {
            match target {
                WindowTargetKind::Hwnd => vec![wgpu::CompositeAlphaMode::Opaque],
                WindowTargetKind::CompositionVisual => vec![
                    wgpu::CompositeAlphaMode::Auto,
                    wgpu::CompositeAlphaMode::Inherit,
                    wgpu::CompositeAlphaMode::Opaque,
                    wgpu::CompositeAlphaMode::PostMultiplied,
                    wgpu::CompositeAlphaMode::PreMultiplied,
                ],
            }
        }

        /// PIN (WebView2 spike, Q1) — **a visual target is `PreMultiplied` and
        /// an HWND target is `Opaque`**, and the choice is made by which door
        /// the window came through rather than by taking whatever is first in
        /// the offered list.
        ///
        /// The visual target offers five modes and `Auto` is the one
        /// `get_default_config` would leave in place. `Auto` presents today's
        /// opaque picture perfectly well, which is exactly why this is pinned:
        /// nothing on screen would ever say it had been picked, and the hole the
        /// web slice cuts would simply come out black.
        ///
        /// MUTATIONS:
        /// ① return the first offered mode instead of the required one and the
        ///    visual case reads `Auto`;
        /// ② give both doors the same answer and one of the two assertions goes
        ///    red whichever answer is chosen.
        #[test]
        fn a_visual_is_configured_premultiplied_and_a_window_handle_opaque() {
            assert_eq!(
                choose_alpha_mode(
                    WindowTargetKind::CompositionVisual,
                    &dx12_offers(WindowTargetKind::CompositionVisual)
                )
                .expect("dx12 offers PreMultiplied to a visual target"),
                wgpu::CompositeAlphaMode::PreMultiplied
            );
            assert_eq!(
                choose_alpha_mode(WindowTargetKind::Hwnd, &dx12_offers(WindowTargetKind::Hwnd))
                    .expect("dx12 offers Opaque to a window-handle target"),
                wgpu::CompositeAlphaMode::Opaque
            );
        }

        /// And an adapter that will not offer it is a refusal carrying what it
        /// did offer — never a substitution.
        ///
        /// The shape of the mistake this forbids is the one the whole slice is
        /// built to avoid: a composition-visual surface silently configured
        /// `Opaque` looks correct in every screenshot and has quietly undone the
        /// only property the slice was for.
        #[test]
        fn an_adapter_that_will_not_give_the_required_mode_is_an_error_and_not_a_substitution() {
            let offered = [
                wgpu::CompositeAlphaMode::Auto,
                wgpu::CompositeAlphaMode::Opaque,
            ];
            match choose_alpha_mode(WindowTargetKind::CompositionVisual, &offered) {
                Err(RenderError::AlphaModeUnavailable {
                    target,
                    required,
                    offered,
                }) => {
                    assert_eq!(target, WindowTargetKind::CompositionVisual);
                    assert_eq!(required, wgpu::CompositeAlphaMode::PreMultiplied);
                    assert_eq!(
                        offered,
                        vec![
                            wgpu::CompositeAlphaMode::Auto,
                            wgpu::CompositeAlphaMode::Opaque
                        ],
                        "the refusal carries the evidence, because on the machine where \
                         this fires nobody else can see the list"
                    );
                }
                other => panic!("expected AlphaModeUnavailable, got {other:?}"),
            }
        }

        /// Both doors exist and both are reachable from outside this crate.
        ///
        /// A compile-shaped test rather than a behavioural one: what could break
        /// here is the plumbing — a variant that stops being public, or a
        /// `SurfaceTarget` conversion that no longer accepts what the app holds
        /// — and none of that can be caught by a running window on the machine
        /// that already works.
        #[test]
        fn both_window_targets_can_be_named_and_told_apart() {
            // Never handed to wgpu: what is under test is the discriminant, and
            // `kind` reads it without touching what it points at.
            let visual = WindowTarget::CompositionVisual(std::ptr::null_mut());
            assert_eq!(visual.kind(), WindowTargetKind::CompositionVisual);
            assert_eq!(
                required_alpha_mode(visual.kind()),
                wgpu::CompositeAlphaMode::PreMultiplied
            );
            // The `Hwnd` arm is exercised by its kind alone: constructing a
            // `SurfaceTarget` needs a real window handle, and what is being
            // pinned is that the two kinds are distinct and answer differently.
            assert_ne!(WindowTargetKind::Hwnd, WindowTargetKind::CompositionVisual);
            assert_eq!(
                required_alpha_mode(WindowTargetKind::Hwnd),
                wgpu::CompositeAlphaMode::Opaque
            );
        }

        /// An offscreen window has no alpha mode to report, and says so rather
        /// than inventing one — `bt-replay` draws into a texture that is never
        /// composited with anything.
        #[test]
        fn a_window_drawing_into_a_texture_reports_no_alpha_mode() {
            let mut gpu =
                pollster::block_on(GpuContext::headless(wgpu::TextureFormat::Bgra8UnormSrgb))
                    .expect("a headless device context on this machine's adapter");
            let window = WindowRenderer::offscreen(
                &mut gpu,
                64,
                64,
                1.0,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            )
            .expect("an offscreen window");
            assert!(window.alpha_report().is_none());
        }
    }

    /// §7.1.6c-4f's second half: **a background is a background wherever it was
    /// declared**, and Folio's own state fills are not backgrounds.
    ///
    /// Every test in here needs a real adapter, for `two_layers`' reason: what is
    /// under test is what the render path actually hands the two pipelines, and
    /// the only honest way to ask that is to ask a window renderer.
    mod one_translucency {
        use super::*;

        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

        fn context() -> GpuContext {
            pollster::block_on(GpuContext::headless(FORMAT))
                .expect("a headless device context on this machine's adapter")
        }

        /// A four-column, two-row grid: one cell whose background a program set
        /// outright, one that set it by reversing what it already had, the rest
        /// left at the theme's default — and a selection over the second row.
        fn grid() -> ViewportFrame {
            let columns = 4_usize;
            let rows = 2_usize;
            let mut cells = vec![CapturedCell::plain(" "); columns * rows];
            cells[0].style.background = TerminalColor::Rgb(38, 38, 38);
            cells[1].style.flags = CellFlags::INVERSE;
            ViewportFrame {
                columns: NonZeroU32::new(columns as u32).unwrap(),
                grid_rows: NonZeroU32::new(rows as u32).unwrap(),
                rows: NonZeroU32::new(rows as u32).unwrap(),
                presentation_offset_subpixels: 0,
                cells,
                cursor: bt_viewport::GridCursor {
                    row: 0,
                    column: 0,
                    visible: false,
                },
                cell_anchors: test_cell_anchors(columns * rows),
                row_map: test_row_map(rows as u32),
                selection_spans: vec![bt_viewport::SelectionSpan {
                    row: 1,
                    start_column: 0,
                    end_column: 4,
                }],
                search_spans: Vec::new(),
                current_search_spans: Vec::new(),
                math_blocks: Vec::new(),
                math_failures: Vec::new(),
                status_text: None,
                viewport_origin: FrameViewportOrigin::Bottom,
                scroll_offset_rows: 0,
                layout_key: bt_doc_layout_key(columns as u32),
                view_generation: bt_doc::ViewGeneration(1),
            }
        }

        /// PIN (user ruling 2026-08-18; `docs/DESIGN.md` §7.1.6c-4f) — **a cell
        /// background a program set carries the window's alpha, and Folio's own
        /// state fills stay opaque.**
        ///
        /// The screenshot this comes from is an agent's transcript in a 30%
        /// window: its grey message bars and its "jump to bottom" chip stood as
        /// solid slabs on a window the desk showed through, because the grid
        /// painted every resolved background at alpha 1 while the clear beside it
        /// was glass. A banner more solid than the paper it is printed on is the
        /// wrong way round.
        ///
        /// Both classes are asserted from one call, because the failure that
        /// matters is a fill on the wrong side of the line and either direction
        /// is a bug: a selection at the ground's alpha is a selection that cannot
        /// be read over a program's own colour.
        ///
        /// **The reversed cell is in the list on purpose.** `SGR 7` is how a
        /// program declares a background out of the two colours it already has,
        /// and it reaches this loop already swapped — so it is not a special case
        /// here, and a reading that treated "declared background" as "SGR 4x
        /// only" would leave every `fzf` row and every `top` header opaque.
        ///
        /// Mutation: push the cell backgrounds onto `ink` and the first
        /// assertion finds no grounds; push the selection onto `grounds` and the
        /// count goes to three.
        #[test]
        fn a_programs_cell_background_is_glass_and_a_selection_is_not() {
            const ALPHA: f32 = 0.3;
            let mut gpu = context();
            let window = WindowRenderer::offscreen(&mut gpu, 400, 200, 1.0, FORMAT)
                .expect("an offscreen window");
            let frame = grid();
            let rects = window.rectangles(&frame, &HashSet::new(), true, ALPHA);

            assert_eq!(
                rects.grounds.len(),
                2,
                "the two declared backgrounds — the explicit one and the reversed one — \
                 and nothing else: a default cell is the clear itself"
            );
            for ground in &rects.grounds {
                assert!(
                    (ground.color[3] - ALPHA).abs() < 1e-6,
                    "a background is the window at that cell: {}",
                    ground.color[3]
                );
            }
            // The explicit one, against the arithmetic the clear runs. Same
            // colour, same encoding, same sheet of glass.
            let expected = ground::premultiplied_clear(srgb_rgb_to_linear([38, 38, 38]), ALPHA);
            assert!((rects.grounds[0].color[0] - expected.r as f32).abs() < 1e-5);
            assert!((rects.grounds[0].color[1] - expected.g as f32).abs() < 1e-5);
            assert!((rects.grounds[0].color[2] - expected.b as f32).abs() < 1e-5);
            assert!((rects.grounds[0].color[3] - expected.a as f32).abs() < 1e-5);

            let selection = rect_gpu_color(selection_background_rgb());
            assert!(
                rects.ink.iter().any(|rect| rect.color == selection),
                "a selection is Folio saying where the reader is dragging, and it \
                 stays legible over whatever it crosses"
            );
        }

        /// PIN — **at 100% opacity the split changes nothing.**
        ///
        /// The grounds are premultiplied by 1 and drawn with `Replace`, which
        /// writes what `ALPHA_BLENDING` at alpha 1 writes; they are also the
        /// leading block of the list they were split out of, so "grounds then
        /// ink" is the order the one buffer already had. Between them those two
        /// facts are the whole claim that §2.3's zero-diff shots still stand, and
        /// they are asserted rather than argued.
        ///
        /// Mutation: premultiply by anything but the alpha handed in and the
        /// second assertion finds a channel that moved.
        #[test]
        fn an_opaque_window_draws_the_same_bytes_it_drew_before_the_split() {
            let mut gpu = context();
            let window = WindowRenderer::offscreen(&mut gpu, 400, 200, 1.0, FORMAT)
                .expect("an offscreen window");
            let frame = grid();
            let opaque = window.rectangles(&frame, &HashSet::new(), true, 1.0);
            for ground in &opaque.grounds {
                assert!(
                    (ground.color[3] - 1.0).abs() < 1e-6,
                    "an opaque window's ground is opaque"
                );
            }
            assert_eq!(
                opaque.grounds[0].color,
                rect_gpu_color([38, 38, 38]),
                "premultiplying by one is the identity, so the byte that reaches \
                 the pipeline is the byte that reached it before"
            );
            // And the ink half is untouched at every alpha: it is the same list,
            // built by the same arithmetic, whatever the window's opacity is.
            let glass = window.rectangles(&frame, &HashSet::new(), true, 0.3);
            assert_eq!(opaque.ink, glass.ink);
        }

        /// PIN (§7.1.6c-4f, the rail's amendment) — **a floating ground fades by
        /// cross-fading towards what is under it, and never by going opaque.**
        ///
        /// The rail is the one overlay level that is a panel of the window rather
        /// than a surface laid over it, and it carries a fold: `.rail { opacity }`
        /// from 0 to 1 over 180 ms. `ALPHA_BLENDING` cannot express that on a
        /// translucent window — it would land the destination on
        /// `o + (1 − o)·A`, a rail *more* opaque mid-fold than the glass it is
        /// folding into, which is the very arithmetic the one-translucency ruling
        /// threw out. The blend constant is the lerp that can.
        ///
        /// The model below is the blend state itself, applied by hand, so the
        /// three claims are made against the factors the pipeline is built with:
        /// at rest it is `Replace`, on an opaque window it is what the old
        /// alpha-blended path wrote, and mid-fold on a glass window it never
        /// leaves the destination more opaque than either surface.
        ///
        /// Mutation: swap the factors back to `SrcAlpha`/`OneMinusSrcAlpha` and
        /// the third claim fails at every fraction of the fold.
        #[test]
        fn a_folding_ground_lerps_towards_what_it_covers() {
            let blend = ground_fade_blend();
            assert_eq!(blend.color, blend.alpha, "one lerp, both components");
            assert_eq!(blend.color.src_factor, wgpu::BlendFactor::Constant);
            assert_eq!(blend.color.dst_factor, wgpu::BlendFactor::OneMinusConstant);
            assert_eq!(blend.color.operation, wgpu::BlendOperation::Add);

            // `out = o·src + (1 − o)·dst`, which is what those three lines say.
            let composite = |src: [f32; 4], dst: [f32; 4], o: f32| {
                let mut out = [0.0f32; 4];
                for channel in 0..4 {
                    out[channel] = o * src[channel] + (1.0 - o) * dst[channel];
                }
                out
            };
            let panel = [24u8, 24, 24];
            for alpha in [1.0f32, 0.3] {
                let src = premultiplied_surface_pixel_rect(
                    [0.0, 0.0, 220.0, 900.0],
                    panel,
                    alpha,
                    800,
                    600,
                )
                .color;
                // Whatever stands under the rail: the pane's own glass.
                let dst = premultiplied_surface_pixel_rect(
                    [0.0, 0.0, 220.0, 900.0],
                    [17, 17, 17],
                    alpha,
                    800,
                    600,
                )
                .color;
                assert_eq!(
                    composite(src, dst, 1.0),
                    src,
                    "a rail at rest is `Replace`: the ground and nothing of what it covers"
                );
                for step in 0..=10 {
                    let o = step as f32 / 10.0;
                    let out = composite(src, dst, o);
                    assert!(
                        out[3] <= alpha + 1e-6,
                        "alpha {alpha}, fold {o}: a folding panel must not be more \
                         opaque than the window it folds into — {}",
                        out[3]
                    );
                }
            }
        }
    }

    /// The multiwindow block's slice A1. Every test here needs a real adapter:
    /// what is under test is which side of the device/window line a resource
    /// lives on, and that is not a question a mock can answer.
    mod two_layers {
        use super::*;

        const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

        fn context() -> GpuContext {
            pollster::block_on(GpuContext::headless(FORMAT))
                .expect("a headless device context on this machine's adapter")
        }

        /// The device layer, standing up with no window anywhere near it.
        ///
        /// Not a smoke test — it is the whole claim of the split. If a
        /// `GpuContext` could only be reached through a surface, a second window
        /// would have to bring its own device, and the atlas and the thirteen
        /// font files behind `terminal_font_system` would be per window again.
        #[test]
        fn a_gpu_context_is_constructible_with_no_surface_at_all() {
            let gpu = context();
            assert_eq!(
                gpu.format(),
                FORMAT,
                "the format is the context's, named outright rather than asked of a swapchain"
            );
            assert!(
                gpu.max_texture_dimension_2d() > 0,
                "the device limit the tile cutter reads has to arrive with the device"
            );
            assert!(
                !gpu.adapter_name().is_empty(),
                "the adapter is kept, not dropped after `new` as it once was"
            );
        }

        /// PIN (§7.1.6c-4d, clearing arm) — a ground with no picture holds no
        /// texture.
        ///
        /// The frame path used to ask for the texture only when there *was* a
        /// picture, so `None` never reached the slot and a wallpaper the reader
        /// had cleared stayed on the device for the life of the process — a
        /// whole screen of texels, up to the resample ceiling, held for a window
        /// with no way left to reach it. "The picture on screen stays up while
        /// the next one decodes" is a rule about the *next* picture; it was
        /// silently also keeping the last one after there was no next.
        ///
        /// Red gate: skip the call on the `None` arm — which is what the frame
        /// path did — and the last assertion finds the old bind group still
        /// there.
        #[test]
        fn clearing_the_ground_picture_releases_the_texture_it_held() {
            let mut gpu = context();
            let image = ground::BackgroundImage {
                key: "bg:ridge".to_owned(),
                rgba: std::sync::Arc::from(vec![255u8, 0, 0, 255]),
                width_px: 1,
                height_px: 1,
            };
            assert!(gpu.hold_background_texture(Some(&image)));
            assert_eq!(
                gpu.background_texture.as_ref().map(|(key, _)| key.as_str()),
                Some("bg:ridge")
            );
            // Idempotent: the same picture asks the device nothing.
            assert!(gpu.hold_background_texture(Some(&image)));

            assert!(
                !gpu.hold_background_texture(None),
                "with no picture there is no quad to draw"
            );
            assert!(
                gpu.background_texture.is_none(),
                "a cleared wallpaper must be let go of, not kept where nothing can reach it"
            );
        }

        /// PIN (user ruling 2026-08-18, "one translucency") — a ground band is
        /// premultiplied by the window's alpha and an ink mark is not.
        ///
        /// The two encodings are what the two pipelines require: a ground is
        /// laid down with `Replace` onto a premultiplied surface, so it must
        /// arrive as `(A·colour, A)` — the same value
        /// [`ground::premultiplied_clear`] writes — while ink blends over it
        /// straight at full alpha. Handing the ground pipeline a straight-alpha
        /// source is the bug that reads as a band brighter than the ground
        /// beside it.
        ///
        /// Red gate: drop the premultiply and the three colour channels come
        /// back at full strength.
        #[test]
        fn a_ground_band_carries_the_windows_alpha_and_ink_stays_opaque() {
            const ALPHA: f32 = 0.3;
            let rect = [0.0, 0.0, 100.0, 20.0];
            let colour = [200u8, 100, 50];
            let ink = surface_pixel_rect(rect, colour, 800, 600);
            let band = premultiplied_surface_pixel_rect(rect, colour, ALPHA, 800, 600);

            assert_eq!(ink.rect, band.rect, "the geometry is the same rectangle");
            assert!(
                (ink.color[3] - 1.0).abs() < 1e-6,
                "a mark on the glass is opaque: {}",
                ink.color[3]
            );
            assert!(
                (band.color[3] - ALPHA).abs() < 1e-6,
                "a ground is the window, and the window is {ALPHA}: {}",
                band.color[3]
            );
            for channel in 0..3 {
                assert!(
                    (band.color[channel] - ink.color[channel] * ALPHA).abs() < 1e-6,
                    "channel {channel} must be premultiplied: {} vs {}",
                    band.color[channel],
                    ink.color[channel] * ALPHA
                );
            }
            // The band and the clear beside it are the same arithmetic, so a
            // window with no picture is one flat sheet and not two.
            let clear = ground::premultiplied_clear(srgb_rgb_to_linear(colour), ALPHA);
            assert!((band.color[0] - clear.r as f32).abs() < 1e-5);
            assert!((band.color[1] - clear.g as f32).abs() < 1e-5);
            assert!((band.color[2] - clear.b as f32).abs() < 1e-5);
            assert!((band.color[3] - clear.a as f32).abs() < 1e-5);
        }

        /// Unequal formats are an error and never a degradation.
        ///
        /// The atlas and both pipelines bake the format in, so "share anyway"
        /// would mean a second atlas behind a name that says one. The refusal is
        /// on the constructor because that is the only door a window comes
        /// through.
        #[test]
        fn a_window_refuses_a_context_baked_for_another_format() {
            let mut gpu = context();
            let refused = WindowRenderer::offscreen(
                &mut gpu,
                200,
                120,
                1.0,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            );
            match refused {
                Err(RenderError::FormatMismatch { context, surface }) => {
                    assert_eq!(context, FORMAT);
                    assert_eq!(surface, wgpu::TextureFormat::Rgba8UnormSrgb);
                }
                Err(other) => panic!("expected a format mismatch, got {other}"),
                Ok(_) => panic!("a second format was silently accepted"),
            }
            WindowRenderer::offscreen(&mut gpu, 200, 120, 1.0, FORMAT)
                .expect("the context's own format is accepted");
        }

        /// Spike Q3: `CellMetrics` follows the surface, not the device.
        ///
        /// Two monitors at 1.5x and 2.0x are two pixel font sizes at the same
        /// instant, so a window that re-measures says nothing about its
        /// neighbour — including through `font_revision`, which is what every
        /// cache key in the composed-row cache is stamped with.
        #[test]
        fn two_windows_over_one_context_measure_their_own_cells() {
            let mut gpu = context();
            let fine =
                WindowRenderer::offscreen(&mut gpu, 400, 300, 2.0, FORMAT).expect("a 2.0x window");
            let mut coarse =
                WindowRenderer::offscreen(&mut gpu, 800, 600, 1.5, FORMAT).expect("a 1.5x window");
            assert_eq!(fine.metrics().scale_factor, 2.0);
            assert_eq!(coarse.metrics().scale_factor, 1.5);
            assert!(
                fine.metrics().font_size_px > coarse.metrics().font_size_px,
                "the same logical size on a denser window is more pixels: {} vs {}",
                fine.metrics().font_size_px,
                coarse.metrics().font_size_px
            );

            let fine_metrics = fine.metrics();
            let fine_revision = fine.font_revision;
            coarse
                .update_scale_factor(&mut gpu, 3.0)
                .expect("the coarse window follows its own monitor");
            assert_eq!(coarse.metrics().scale_factor, 3.0);
            assert_eq!(
                fine.metrics(),
                fine_metrics,
                "one window's DPI change may not re-measure another window's cell"
            );
            assert_eq!(
                fine.font_revision, fine_revision,
                "nor may it invalidate another window's row cache"
            );
        }

        /// The other half of Q3: the caches keyed by pixel font size are the
        /// window's, and the atlas underneath them is the device's.
        ///
        /// Both windows are at the same scale here on purpose. Equal metrics
        /// means equal cache keys, so a second window missing on a row the first
        /// one has already composed can only mean the two caches are two
        /// objects — which is exactly what a shared cache would get wrong.
        #[test]
        fn each_window_composes_rows_into_its_own_cache_over_the_shared_atlas() {
            let mut gpu = context();
            let mut first =
                WindowRenderer::offscreen(&mut gpu, 400, 300, 1.0, FORMAT).expect("first window");
            let mut second =
                WindowRenderer::offscreen(&mut gpu, 400, 300, 1.0, FORMAT).expect("second window");
            let frame = single_cell_cursor_frame(first.metrics());

            let cold = first
                .probe_frame(&mut gpu, &frame)
                .expect("the first window's first frame");
            assert_eq!(cold.row_cache_misses, 1);
            assert!(
                cold.narrow_glyphs > 0,
                "a frame with a character in it puts a glyph in the shared atlas"
            );

            let warm = first
                .probe_frame(&mut gpu, &frame)
                .expect("the first window's second frame");
            assert_eq!(warm.row_cache_hits, 1);
            assert_eq!(warm.row_cache_misses, 0);

            let other = second
                .probe_frame(&mut gpu, &frame)
                .expect("the second window's first frame");
            assert_eq!(
                other.row_cache_misses, 1,
                "the second window owns its own composed rows, warm or not"
            );
            assert!(
                other.narrow_glyphs > 0,
                "and draws them through the one atlas the first window already grew"
            );
        }

        /// PIN (the Font size row, 2026-08-17) — **a font change invalidates
        /// exactly what a DPI change invalidates.**
        ///
        /// The failure this exists to catch never crashes and never looks wrong
        /// in a unit test: a window that keeps drawing correct glyphs from rows
        /// that were composed for the old grid, because the cache key did not
        /// happen to include the thing that moved. So the assertions are the
        /// four observable consequences of the invalidation list, not a reading
        /// of the list itself —
        ///
        /// 1. the grid is re-measured (a bigger face is a wider, taller cell),
        /// 2. `font_revision` moves, which is what re-keys every composed row,
        /// 3. a frame that was warm a moment ago misses the row cache, and
        /// 4. re-choosing the same font is still a full invalidation, because
        ///    the caller cannot know that nothing moved and a no-op that
        ///    *sometimes* skipped the clear would be the worst of both.
        ///
        /// MUTATIONS: ① drop `composed_row_cache.clear()` from `adopt_metrics`
        /// and (3) goes green-to-red — the second frame hits the cache and the
        /// old rows are drawn at the new cell size; ② drop the `font_revision`
        /// bump and (2) and (3) both fail; ③ measure at the default size instead
        /// of `gpu.terminal_font_size_logical_px` and (1) fails.
        #[test]
        fn changing_the_grids_face_re_measures_it_and_leaves_no_row_cached() {
            let mut gpu = context();
            let mut window =
                WindowRenderer::offscreen(&mut gpu, 400, 300, 1.0, FORMAT).expect("a window");

            let before = window.metrics();
            let revision_before = window.font_revision();
            let frame = single_cell_cursor_frame(before);
            let warm = window.probe_frame(&mut gpu, &frame).expect("one frame");
            assert_eq!(warm.row_cache_misses, 1, "the first frame composes its row");
            let warm_again = window
                .probe_frame(&mut gpu, &frame)
                .expect("the same frame");
            assert_eq!(
                warm_again.row_cache_misses, 0,
                "and the second finds it — which is the state the change has to undo"
            );

            gpu.set_terminal_font(DEFAULT_PRIMARY_FONT_FAMILY, &[], 24.0);
            let after = window.apply_font_change(&mut gpu).expect("a re-measure");

            assert!(
                after.cell_height_px > before.cell_height_px,
                "24 logical pixels of face wants a taller row than 16 did                  ({} vs {})",
                after.cell_height_px,
                before.cell_height_px
            );
            assert!(
                after.cell_width_px > before.cell_width_px,
                "and a wider cell ({} vs {})",
                after.cell_width_px,
                before.cell_width_px
            );
            assert_eq!(
                after.font_size_px, 24.0,
                "at scale 1.0 the physical size is the logical one"
            );
            assert_eq!(
                after.scale_factor, before.scale_factor,
                "a font change does not move the window to another monitor"
            );
            assert!(
                window.font_revision() > revision_before,
                "every composed row is keyed by this number"
            );

            let cold = window
                .probe_frame(&mut gpu, &single_cell_cursor_frame(after))
                .expect("one frame at the new size");
            assert_eq!(
                cold.row_cache_misses, 1,
                "nothing composed for the old grid may survive into the new one"
            );

            // (4) — the same font again is still a full invalidation.
            let revision = window.font_revision();
            gpu.set_terminal_font(DEFAULT_PRIMARY_FONT_FAMILY, &[], 24.0);
            window.apply_font_change(&mut gpu).expect("a re-measure");
            assert!(window.font_revision() > revision);
        }

        /// PIN — a family this machine does not have leaves the grid on the face
        /// it can actually draw.
        ///
        /// `settings.json` holds a family *name*, and a name outlives the font:
        /// uninstall it, or open the file on another machine, and the row that
        /// was chosen names nothing. Pointing `fontdb` at a family it has never
        /// heard of makes `Family::Monospace` resolve to whatever the database's
        /// first face happens to be — for this database, an emoji font — so the
        /// whole grid would come up in Noto Color Emoji's fallback outlines.
        #[test]
        fn a_family_this_machine_does_not_have_falls_back_to_the_face_it_draws() {
            let mut gpu = context();
            gpu.set_terminal_font("No Such Family Is Installed", &[], 16.0);
            assert_eq!(gpu.terminal_font_family(), DEFAULT_PRIMARY_FONT_FAMILY);
            gpu.set_terminal_font("", &[], 16.0);
            assert_eq!(
                gpu.terminal_font_family(),
                DEFAULT_PRIMARY_FONT_FAMILY,
                "an unnamed family is the settings file's way of saying `the default`"
            );
        }

        /// The replay probe is the two layers and nothing else.
        #[test]
        fn the_replay_probe_is_a_context_and_an_offscreen_window() {
            let mut probe = pollster::block_on(HeadlessRenderProbe::new(400, 300, 1.0))
                .expect("a headless probe");
            assert!(!probe.adapter_name().is_empty());
            assert!(probe.max_texture_dimension_2d() > 0);
            let frame = single_cell_cursor_frame(probe.window.metrics());
            let sample = probe.prepare_frame(&frame).expect("one replayed frame");
            assert!(sample.narrow_glyphs > 0);
            assert_eq!(sample.row_cache_misses, 1);
            let metrics = probe
                .update_scale_factor(2.0)
                .expect("the probe re-measures like any window");
            assert_eq!(metrics.scale_factor, 2.0);
        }
    }
}
