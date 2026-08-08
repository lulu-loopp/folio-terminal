//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

mod procedural;
mod rounded_rect;
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
use bt_transcript::{CapturedCell, CellFlags, CellStyle, TerminalColor};
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
};
use thiserror::Error;
use unicode_properties::emoji::{EmojiStatus, UnicodeEmoji};
use wgpu::util::DeviceExt;

use rounded_rect::{rounded_rect_coverage, rounded_rect_halo_coverage};
use theme::{
    CURSOR_BAR_WIDTH_LOGICAL_PX, CURSOR_UNDERLINE_HEIGHT_LOGICAL_PX, DEFAULT_CURSOR_RGB,
    DEFAULT_DIM_FOREGROUND_RGB, ansi_16_rgb,
};
pub use theme::{
    ChromePalette, CursorStyle, DARK_CHROME, DEFAULT_BACKGROUND_RGB,
    FLOAT_WINDOW_BORDER_LOGICAL_PX, FLOAT_WINDOW_RADIUS_LOGICAL_PX, FLOAT_WINDOW_SHADOW_LOGICAL_PX,
    LIGHT_BACKGROUND_RGB, LIGHT_CHROME, PANE_HEAD_FILE_MARK_LOGICAL_PX,
    PANE_HEAD_FOLDER_MARK_LOGICAL_PX, PANE_HEAD_PROFILE_MARK_LOGICAL_PX,
    PREVIEW_BODY_INSET_LOGICAL_PX, SEAT_DIVIDER_HIT_LOGICAL_PX, SEAT_DIVIDER_VISUAL_LOGICAL_PX,
    SEAT_TITLE_BAR_LOGICAL_PX, SEAT_TITLE_EDGE_LOGICAL_PX, SEAT_TITLE_FONT_LOGICAL_PX,
    SEAT_TITLE_GAP_LOGICAL_PX, SEAT_TITLE_PADDING_LOGICAL_PX, Theme, ThemeChange,
    WINDOW_CAPTION_BUTTON_LOGICAL_PX, WINDOW_CAPTION_GEAR_GLYPH_LOGICAL_PX,
    WINDOW_CAPTION_GLYPH_LOGICAL_PX, WINDOW_NEW_TAB_BOX_LOGICAL_PX,
    WINDOW_NEW_TAB_CHEVRON_HEIGHT_LOGICAL_PX, WINDOW_NEW_TAB_CHEVRON_WIDTH_LOGICAL_PX,
    WINDOW_NEW_TAB_GLYPH_LOGICAL_PX, WINDOW_NEW_TAB_MARGIN_BOTTOM_LOGICAL_PX,
    WINDOW_NEW_TAB_MARGIN_LEFT_LOGICAL_PX, WINDOW_NEW_TAB_RADIUS_LOGICAL_PX,
    WINDOW_TAB_CLOSE_BOX_LOGICAL_PX, WINDOW_TAB_CLOSE_GLYPH_LOGICAL_PX,
    WINDOW_TAB_CLOSE_RADIUS_LOGICAL_PX, WINDOW_TAB_FONT_LOGICAL_PX,
    WINDOW_TAB_GAP_BETWEEN_LOGICAL_PX, WINDOW_TAB_GAP_LOGICAL_PX, WINDOW_TAB_HEIGHT_LOGICAL_PX,
    WINDOW_TAB_MARK_LOGICAL_PX, WINDOW_TAB_MAX_WIDTH_LOGICAL_PX,
    WINDOW_TAB_PADDING_LEFT_LOGICAL_PX, WINDOW_TAB_PADDING_RIGHT_LOGICAL_PX,
    WINDOW_TAB_RADIUS_LOGICAL_PX, WINDOW_TAB_SQUEEZED_LOGICAL_PX,
    WINDOW_TAB_SQUEEZED_PADDING_LOGICAL_PX, WINDOW_TAB_TIGHT_LOGICAL_PX,
    WINDOW_TITLE_BAR_LOGICAL_PX, background_rgb, chrome_palette, current_cursor_style,
    current_theme, foreground_rgb, set_cursor_style, set_theme, theme_revision,
};
use theme::{DEFAULT_SELECTION_BACKGROUND_RGB, DEFAULT_STATUS_BACKGROUND_RGB};

const BASE_FONT_SIZE_LOGICAL_PX: f32 = 16.0;
const BASE_LINE_HEIGHT_LOGICAL_PX: f32 = 22.0;
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
const PRIMARY_FONT_FAMILY: &str = "Consolas";
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
    fn measure(font_system: &mut FontSystem, scale_factor: f64) -> Result<Self, RenderError> {
        let scale = scale_factor as f32;
        let font_size_px = BASE_FONT_SIZE_LOGICAL_PX * scale;
        let cell_height_px = BASE_LINE_HEIGHT_LOGICAL_PX * scale;
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
}

#[derive(Clone, Copy, Debug)]
pub enum PresentOutcome {
    Presented(PresentReceipt),
    Skipped,
    Reconfigure,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RectInstance {
    rect: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MathVertex {
    position: [f32; 2],
    uv: [f32; 2],
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
    /// Physical-pixel pointer position captured when the hover settled; the flyout anchors to
    /// this point and stays put rather than chasing further pointer motion.
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
    pub key: String,
    pub rgba: Arc<[u8]>,
    /// Texture dimensions. During a live resize these remain the last clear raster.
    pub width_px: u32,
    pub height_px: u32,
    /// Draw dimensions inside the latest seat. They may briefly differ from the texture dimensions
    /// while a resize is in flight; the sampler provides the intentionally soft transition.
    pub display_width_px: u32,
    pub display_height_px: u32,
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
/// above when the bottom lacks room, and clamped horizontally into the pane. Returns `None` when
/// the window is too small to host the box.
#[allow(clippy::too_many_arguments)]
fn peek_box_layout(
    viewport_width: f32,
    viewport_height: f32,
    padding_px: f32,
    scale_factor: f32,
    image_width_px: u32,
    image_height_px: u32,
    pointer_x: f32,
    pointer_y: f32,
) -> Option<PeekBoxLayout> {
    let (thumb_width_px, thumb_height_px) = peek_thumbnail_extent(
        viewport_width,
        viewport_height,
        padding_px,
        scale_factor,
        image_width_px,
        image_height_px,
    )?;
    let avail_left = padding_px;
    let avail_top = padding_px;
    let avail_right = viewport_width - padding_px;
    let avail_bottom = viewport_height - padding_px;
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
    // The lift: two concentric rings around the box — never under it, because an
    // outer shadow is clipped out of the border box it lifts — the wider and
    // fainter one first, so the two compose into a falloff rather than a band.
    let spread = FLOAT_WINDOW_SHADOW_LOGICAL_PX * scale;
    let mut fills: Vec<PeekBoxFill> = [
        (spread, palette.menu_shadow_outer_alpha),
        (spread / 2.0, palette.menu_shadow_inner_alpha),
    ]
    .into_iter()
    .flat_map(|(extent, shadow_alpha)| {
        paint(
            PeekBoxLayer::Lift,
            rounded_rect_halo_coverage(layout.frame, radius, extent),
            palette.menu_shadow,
            alpha(shadow_alpha),
        )
    })
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
        .saturating_add(key.text.capacity())
        .saturating_add(value_bytes)
        .saturating_add(buffer_resident_bytes(buffer))
}

fn captured_cell_resident_bytes(cell: &CapturedCell) -> usize {
    size_of::<CapturedCell>()
        .saturating_add(cell.text.capacity())
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
    text: String,
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
            NarrowSizePolicy::CellHeightEmoji => center_ink_offsets(
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
    ) -> (Arc<Buffer>, f32) {
        if let Some(cached) = self.entries.get(&key) {
            if self.track_perf {
                self.counters.hits = self.counters.hits.saturating_add(1);
            }
            return (Arc::clone(&cached.buffer), cached.top_offset_px);
        }

        let miss_started = self.track_perf.then(Instant::now);
        let buffer = shape_wide_buffer_for_key(
            &key,
            font_system,
            swash_cache,
            metrics,
            #[cfg(test)]
            &mut self.color_emoji_trial_shapes,
        );
        let glyph_baseline_px = buffer
            .layout_runs()
            .next()
            .map_or(metrics.ascii_baseline_px, |run| run.line_y);
        let top_offset_px = baseline_offset_px(metrics.ascii_baseline_px, glyph_baseline_px);
        let buffer = Arc::new(buffer);
        let resident_bytes =
            shape_entry_resident_bytes(&key, &buffer, size_of::<CachedWideShape>());
        let (_, evictions) = self.entries.insert(
            key,
            CachedWideShape {
                buffer: Arc::clone(&buffer),
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
        (buffer, top_offset_px)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NarrowCellSlot {
    column: usize,
    text: String,
    style: CellStyle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WideCellSlot {
    column: usize,
    text: String,
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

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    max_texture_dimension_2d: u32,
    configured_size: (u32, u32),
    /// The terminal seat's rectangle inside the swapchain (§4.1). Equal to the
    /// whole surface whenever the tree is a lone terminal leaf.
    seat: SeatViewport,
    chrome_quads: Vec<ChromeQuad>,
    chrome_labels: Vec<ChromeLabel>,
    chrome_icons: Vec<ChromeIcon>,
    /// The modal overlay's own three lists. Kept apart from the chrome's rather
    /// than appended to them because the two are drawn in different places in the
    /// frame: seat chrome owns the space between seats, and a modal owns the
    /// window — including the seats' own content, which is drawn *after* chrome.
    overlay_quads: Vec<OverlayQuad>,
    overlay_labels: Vec<ChromeLabel>,
    overlay_icons: Vec<ChromeIcon>,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    /// A second glyphon viewport whose resolution names the whole surface rather
    /// than the seat, so chrome text can be positioned in window coordinates
    /// while grid text stays in seat-local ones.
    chrome_viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    status_text_renderer: TextRenderer,
    chrome_text_renderer: TextRenderer,
    overlay_text_renderer: TextRenderer,
    rect_pipeline: wgpu::RenderPipeline,
    math_pipeline: wgpu::RenderPipeline,
    math_bind_group_layout: wgpu::BindGroupLayout,
    math_sampler: wgpu::Sampler,
    math_textures: ByteLru<String, CachedMathTexture>,
    math_texture_evictions: u64,
    /// Artifacts the byte budget refused outright, and visible blocks left without a texture. Both
    /// used to be silent `continue`s; a band that draws its placement but not its pixels is a bare
    /// rectangle on screen, so it is counted where the frame trace can see it.
    math_texture_refusals: u64,
    textureless_math_blocks: u64,
    metrics: CellMetrics,
    /// The chrome sans face's cap height per em, resolved once: it is a property
    /// of the face, so neither a DPI change nor a new title can move it.
    chrome_cap_height_ratio: f32,
    init_timings: RendererInitTimings,
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
}

/// One line of seat chrome text.
#[derive(Clone, Debug, PartialEq)]
pub struct ChromeLabel {
    pub text: String,
    /// The box the text is laid out in and clipped to, `[left, top, right, bottom]`.
    pub rect: [f32; 4],
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

/// Identity is `key` plus placement. The bytes are a function of the key (that is
/// what the key *is*), so comparing them would be paying megabytes per frame to
/// re-learn something the string already said.
impl PartialEq for ChromeIcon {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.rect == other.rect
            && self.width_px == other.width_px
            && self.height_px == other.height_px
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
#[doc(hidden)]
pub struct HeadlessRenderProbe {
    device: wgpu::Device,
    queue: wgpu::Queue,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    status_text_renderer: TextRenderer,
    math_bind_group_layout: wgpu::BindGroupLayout,
    math_sampler: wgpu::Sampler,
    math_textures: ByteLru<String, CachedMathTexture>,
    math_texture_evictions: u64,
    metrics: CellMetrics,
    text_rows: Vec<Arc<ComposedRow>>,
    status_overlay: Option<Arc<ComposedRow>>,
    composed_row_cache: ComposedRowCache,
    font_revision: u64,
    narrow_shaping_cache: NarrowShapingCache,
    wide_shaping_cache: WideShapingCache,
    target: wgpu::Texture,
    width: u32,
    height: u32,
    adapter_name: String,
    max_texture_dimension_2d: u32,
}

impl HeadlessRenderProbe {
    pub async fn new(width: u32, height: u32, scale_factor: f64) -> Result<Self, RenderError> {
        let width = width.max(1);
        let height = height.max(1);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: None,
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("BetterTerminal replay probe device"),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;
        let format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, scale_factor)?;
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let status_text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let (_, math_bind_group_layout, math_sampler) = create_math_pipeline(&device, format);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("BetterTerminal replay probe target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Ok(Self {
            device,
            queue,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            status_text_renderer,
            math_bind_group_layout,
            math_sampler,
            math_textures: ByteLru::new(MATH_TEXTURE_CACHE_BUDGET_BYTES),
            math_texture_evictions: 0,
            metrics,
            text_rows: Vec::new(),
            status_overlay: None,
            composed_row_cache: ComposedRowCache::new(),
            font_revision: 1,
            narrow_shaping_cache: NarrowShapingCache::with_perf_tracking(true),
            wide_shaping_cache: WideShapingCache::with_perf_tracking(true),
            target,
            width,
            height,
            adapter_name,
            max_texture_dimension_2d,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn max_texture_dimension_2d(&self) -> u32 {
        self.max_texture_dimension_2d
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.metrics = CellMetrics::measure(&mut self.font_system, scale_factor)?;
        self.text_rows.clear();
        self.status_overlay = None;
        self.composed_row_cache.clear();
        self.font_revision = self.font_revision.saturating_add(1);
        self.narrow_shaping_cache.clear();
        self.wide_shaping_cache.clear();
        self.math_textures.clear();
        Ok(self.metrics)
    }

    pub fn prepare_frame(
        &mut self,
        frame: &ViewportFrame,
    ) -> Result<RenderProbeSample, RenderError> {
        let started = Instant::now();
        frame.validate_shape()?;
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.width,
                height: self.height,
            },
        );
        let text_stats = prepare_text_rows(
            frame,
            self.metrics,
            &mut self.text_rows,
            &mut self.status_overlay,
            &mut self.composed_row_cache,
            self.font_revision,
            theme_revision(),
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.narrow_shaping_cache,
            &mut self.wide_shaping_cache,
        )?;
        let rows_prepared_at = Instant::now();
        prepare_text_atlas(
            &mut self.text_renderer,
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            &mut self.swash_cache,
            &self.text_rows,
            self.metrics,
            frame,
        )
        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
        prepare_status_text_atlas(
            &mut self.status_text_renderer,
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            &mut self.swash_cache,
            self.status_overlay.as_deref(),
            self.metrics,
            frame,
            // A headless probe has no seat: it renders into the whole target, so the surface is
            // the pane.
            self.width as f32,
        )
        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
        let atlas_prepared_at = Instant::now();
        let math_evictions_before = self.math_texture_evictions;
        let (math_texture_uploads, math_texture_upload_bytes) = self.prepare_math_textures(frame);
        let math_prepared_at = Instant::now();
        let view = self.target.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BetterTerminal replay probe frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BetterTerminal replay probe pass"),
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
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            if self.status_overlay.is_some() {
                self.status_text_renderer
                    .render(&self.atlas, &self.viewport, &mut pass)
                    .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            }
        }
        self.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        self.atlas.trim();
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
            math_texture_evictions: self
                .math_texture_evictions
                .saturating_sub(math_evictions_before),
            math_texture_resident_bytes: self.math_textures.resident_bytes(),
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

    fn prepare_math_textures(&mut self, frame: &ViewportFrame) -> (u64, usize) {
        let mut uploads = 0_u64;
        let mut upload_bytes = 0_usize;
        for placement in &frame.math_blocks {
            if !math_block_admits_texture(frame, placement) {
                continue;
            }
            let key = &placement.artifact.key;
            if self.math_textures.get(key).is_some() {
                continue;
            }
            let Some(texture) = self.upload_math_texture(&placement.artifact) else {
                continue;
            };
            uploads = uploads.saturating_add(1);
            upload_bytes = upload_bytes.saturating_add(placement.artifact.rgba.len());
            let (_, evictions) =
                self.math_textures
                    .insert(key.clone(), texture, placement.artifact.rgba.len());
            self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
        }
        (uploads, upload_bytes)
    }

    fn upload_math_texture(
        &self,
        artifact: &bt_viewport::ProjectedMathArtifact,
    ) -> Option<CachedMathTexture> {
        let expected = artifact.width_px as usize * artifact.height_px as usize * 4;
        if artifact.width_px == 0 || artifact.height_px == 0 || artifact.rgba.len() != expected {
            return None;
        }
        let tile_limit = self.max_texture_dimension_2d.max(1);
        let mut tiles = Vec::new();
        for y in (0..artifact.height_px).step_by(tile_limit as usize) {
            let height = (artifact.height_px - y).min(tile_limit);
            for x in (0..artifact.width_px).step_by(tile_limit as usize) {
                let width = (artifact.width_px - x).min(tile_limit);
                let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
                for row in y..y + height {
                    let start = (row as usize * artifact.width_px as usize + x as usize) * 4;
                    let end = start + width as usize * 4;
                    bytes.extend_from_slice(&artifact.rgba[start..end]);
                }
                let texture = self.device.create_texture_with_data(
                    &self.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("headless math block texture tile"),
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
                    label: Some("headless math block texture bind group"),
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

impl Renderer {
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(target)
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let phase_started = Instant::now();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
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
                label: Some("BetterTerminal device"),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let device_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let max_texture_dimension_2d = device.limits().max_texture_dimension_2d;
        let swapchain_size = surface_config_size(width, height, max_texture_dimension_2d);
        let mut config = surface
            .get_default_config(&adapter, swapchain_size.0, swapchain_size.1)
            .ok_or_else(|| RenderError::Wgpu("surface has no default configuration".to_owned()))?;
        config.format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or_else(|| RenderError::Wgpu("surface has no sRGB format".to_owned()))?;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);
        let surface_configure_time = phase_started.elapsed();

        let phase_started = Instant::now();
        let mut font_system = terminal_font_system();
        let font_system_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let metrics = CellMetrics::measure(&mut font_system, scale_factor)?;
        let mut swash_cache = SwashCache::new();
        let chrome_cap_height_ratio = chrome_cap_height_ratio(&mut font_system, &mut swash_cache)
            .ok_or(RenderError::MissingChromeSansMetrics)?;
        let font_metrics_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let chrome_viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let status_text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let chrome_text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let overlay_text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let rect_pipeline = create_rect_pipeline(&device, config.format);
        let (math_pipeline, math_bind_group_layout, math_sampler) =
            create_math_pipeline(&device, config.format);
        let render_resources_time = phase_started.elapsed();
        let trace_perf = std::env::var_os("BT_PERF_TRACE").is_some();
        Ok(Self {
            surface,
            device,
            queue,
            config,
            max_texture_dimension_2d,
            configured_size: swapchain_size,
            seat: SeatViewport::whole(swapchain_size.0, swapchain_size.1),
            chrome_quads: Vec::new(),
            chrome_labels: Vec::new(),
            chrome_icons: Vec::new(),
            overlay_quads: Vec::new(),
            overlay_labels: Vec::new(),
            overlay_icons: Vec::new(),
            font_system,
            swash_cache,
            viewport,
            chrome_viewport,
            atlas,
            text_renderer,
            status_text_renderer,
            chrome_text_renderer,
            overlay_text_renderer,
            rect_pipeline,
            math_pipeline,
            math_bind_group_layout,
            math_sampler,
            math_textures: ByteLru::new(MATH_TEXTURE_CACHE_BUDGET_BYTES),
            math_texture_evictions: 0,
            math_texture_refusals: 0,
            textureless_math_blocks: 0,
            metrics,
            chrome_cap_height_ratio,
            init_timings: RendererInitTimings {
                adapter: adapter_time,
                device: device_time,
                surface_configure: surface_configure_time,
                font_system: font_system_time,
                font_metrics: font_metrics_time,
                render_resources: render_resources_time,
            },
            text_rows: Vec::new(),
            status_overlay: None,
            composed_row_cache: ComposedRowCache::new(),
            font_revision: 1,
            narrow_shaping_cache: NarrowShapingCache::with_perf_tracking(trace_perf),
            wide_shaping_cache: WideShapingCache::with_perf_tracking(trace_perf),
            glyph_degraded_frames: 0,
            window_focused: true,
            cursor_blink_visible: true,
            peek_overlay: None,
            preview_image: None,
            trace_perf,
            perf_frame: 0,
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn init_timings(&self) -> RendererInitTimings {
        self.init_timings
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
                anchor: placement.anchor.clone(),
                target,
            })
        })
    }

    pub fn presentation_geometry(&self) -> PresentationGeometry {
        PresentationGeometry {
            swapchain_size: (self.config.width, self.config.height),
            max_texture_dimension_2d: self.max_texture_dimension_2d,
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

    /// The display box this viewport would show a native decode of `image_width_px` x
    /// `image_height_px` in, or `None` when the pane cannot host the flyout at all. The app asks
    /// this before a peek so it resamples once, off-thread, to exactly the pixels the flyout draws.
    pub fn peek_thumbnail_extent(
        &self,
        image_width_px: u32,
        image_height_px: u32,
    ) -> Option<(u32, u32)> {
        peek_thumbnail_extent(
            self.seat.width as f32,
            self.seat.height as f32,
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

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let swapchain_size = surface_config_size(width, height, self.max_texture_dimension_2d);
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

    /// Replace the modal overlay: the scrim and whatever dialog stands on it.
    /// Returns whether the visible overlay changed, so the caller can skip a
    /// redundant redraw. Empty vectors mean "no modal", which is the state every
    /// frame that has never opened one is already in.
    ///
    /// This is presentation state beside the frame, exactly as the peek flyout is
    /// (DESIGN §7.1.5: a modal is a window-level stance, not a property of the
    /// terminal's content), so `ViewportFrame` equality and the replay contracts
    /// stay untouched by a visible dialog.
    pub fn set_modal_overlay(
        &mut self,
        quads: Vec<OverlayQuad>,
        labels: Vec<ChromeLabel>,
        icons: Vec<ChromeIcon>,
    ) -> bool {
        let changed = self.overlay_quads != quads
            || self.overlay_labels != labels
            || self.overlay_icons != icons;
        self.overlay_quads = quads;
        self.overlay_labels = labels;
        self.overlay_icons = icons;
        changed
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.metrics = CellMetrics::measure(&mut self.font_system, scale_factor)?;
        self.text_rows.clear();
        self.status_overlay = None;
        self.composed_row_cache.clear();
        self.font_revision = self.font_revision.saturating_add(1);
        self.narrow_shaping_cache.clear();
        self.wide_shaping_cache.clear();
        self.math_textures.clear();
        Ok(self.metrics)
    }

    pub fn present(
        &mut self,
        frame: &ViewportFrame,
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        let frame_started = Instant::now();
        frame.validate_shape()?;
        let validated_at = Instant::now();
        // Grid text is laid out in seat-local pixels, so glyphon's resolution is
        // the seat's; the pass viewport below lands those pixels at the seat's
        // corner. Chrome text is laid out in window pixels and gets its own.
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.seat.width,
                height: self.seat.height,
            },
        );
        self.chrome_viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        let viewport_updated_at = Instant::now();
        let text_stats = self.prepare_text_rows(frame)?;
        let rows_prepared_at = Instant::now();
        let text_prepare_result = prepare_text_atlas(
            &mut self.text_renderer,
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            &mut self.swash_cache,
            &self.text_rows,
            self.metrics,
            frame,
        );
        let text_prepare_result = match text_prepare_result {
            Ok(()) => prepare_status_text_atlas(
                &mut self.status_text_renderer,
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                &mut self.swash_cache,
                self.status_overlay.as_deref(),
                self.metrics,
                frame,
                self.seat.width as f32,
            ),
            Err(error) => Err(error),
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
                                "BetterTerminal glyph atlas reached the device limit; presenting without text and retrying"
                            );
                        }
                        self.glyph_degraded_frames += 1;
                        self.atlas.trim();
                        false
                    }
                }
            }
        };
        let atlas_prepared_at = Instant::now();

        // Math draws first: the hover dim rect decorates a block's raster, so it must know which
        // rasters this frame actually put on screen before it decides to darken anything.
        let math_batch = self.prepare_math_draws(frame);
        let math_prepared_at = Instant::now();
        let math_vertex_buffer = (!math_batch.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("visible math block vertices"),
                    contents: bytemuck::cast_slice(&math_batch.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let (peek_rects, peek_draws, peek_vertices) = self.prepare_peek_draws();
        let peek_rect_buffer = (!peek_rects.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("peek flyout rectangles"),
                    contents: bytemuck::cast_slice(&peek_rects),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let peek_vertex_buffer = (!peek_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("peek flyout image vertices"),
                    contents: bytemuck::cast_slice(&peek_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let (preview_seat, preview_draws, preview_vertices) = self.prepare_preview_draws();
        let preview_vertex_buffer = (!preview_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("preview seat image vertices"),
                    contents: bytemuck::cast_slice(&preview_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let math_draws = math_batch.draws;

        let rects = self.rectangles(frame, &math_batch.drawn);
        let empty_rect = [RectInstance::zeroed()];
        let rect_data = if rects.is_empty() {
            empty_rect.as_slice()
        } else {
            rects.as_slice()
        };
        let rect_buffer = self
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
                status_overlay_geometry(self.metrics, frame, status, self.seat.width as f32)
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
            self.device
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
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("math toolbar overlay rectangles"),
                    contents: bytemuck::cast_slice(overlay_data),
                    usage: wgpu::BufferUsages::VERTEX,
                });
        // Seat chrome. Empty whenever the tree is a lone terminal leaf, and every
        // branch below is guarded on emptiness, so a lone leaf issues exactly the
        // command stream it issued before seats existed.
        let chrome_rects: Vec<RectInstance> = self
            .chrome_quads
            .iter()
            .map(|quad| {
                surface_pixel_rect(quad.rect, quad.color, self.config.width, self.config.height)
            })
            .collect();
        let chrome_rect_buffer = (!chrome_rects.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("seat chrome rectangles"),
                    contents: bytemuck::cast_slice(chrome_rects.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let chrome_icons = std::mem::take(&mut self.chrome_icons);
        let (chrome_icon_draws, chrome_icon_vertices) =
            self.prepare_chrome_icon_draws(&chrome_icons);
        self.chrome_icons = chrome_icons;
        let chrome_icon_buffer = (!chrome_icon_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("chrome mark vertices"),
                    contents: bytemuck::cast_slice(chrome_icon_vertices.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let chrome_layouts = shape_chrome_labels(
            &mut self.font_system,
            &self.chrome_labels,
            self.chrome_cap_height_ratio,
        );
        let chrome_prepared = !chrome_layouts.is_empty()
            && prepare_chrome_text_atlas(
                &mut self.chrome_text_renderer,
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.chrome_viewport,
                &mut self.swash_cache,
                &chrome_layouts,
            )
            .is_ok();
        // The modal overlay. Empty on every frame no dialog is up, and every
        // branch below is guarded on emptiness, so a window without one issues
        // exactly the command stream it issued before modals existed.
        let overlay_rects: Vec<RectInstance> = self
            .overlay_quads
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
        let overlay_rect_buffer = (!overlay_rects.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("modal overlay rectangles"),
                    contents: bytemuck::cast_slice(overlay_rects.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let overlay_icons = std::mem::take(&mut self.overlay_icons);
        let (overlay_icon_draws, overlay_icon_vertices) =
            self.prepare_chrome_icon_draws(&overlay_icons);
        self.overlay_icons = overlay_icons;
        let overlay_icon_buffer = (!overlay_icon_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("modal overlay mark vertices"),
                    contents: bytemuck::cast_slice(overlay_icon_vertices.as_slice()),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let overlay_layouts = shape_chrome_labels(
            &mut self.font_system,
            &self.overlay_labels,
            self.chrome_cap_height_ratio,
        );
        let overlay_prepared = !overlay_layouts.is_empty()
            && prepare_chrome_text_atlas(
                &mut self.overlay_text_renderer,
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.chrome_viewport,
                &mut self.swash_cache,
                &overlay_layouts,
            )
            .is_ok();
        let rectangles_prepared_at = Instant::now();
        // Keep the old DXGI back buffers alive while CPU shaping and GPU resource preparation run.
        // ResizeBuffers discards them; configuring only immediately before acquire/submit bounds
        // both the default-black interval and DXGI's stretch of the old frame.
        self.configure_surface_if_needed()?;
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                self.configure_surface()?;
                texture
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return self.handle_surface_failure(SurfaceFailure::Unavailable);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                return self.handle_surface_failure(SurfaceFailure::Outdated);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return self.handle_surface_failure(SurfaceFailure::Lost);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return self.handle_surface_failure(SurfaceFailure::Validation);
            }
        };
        let surface_acquired_at = Instant::now();
        let view = texture.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("BetterTerminal frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("BetterTerminal terminal pass"),
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
            // Everything the terminal draws is in seat-local pixels; this pair is
            // the entire translation, and for a lone leaf the seat *is* the
            // surface, so these are the two calls that were always here.
            pass.set_viewport(
                self.seat.x as f32,
                self.seat.y as f32,
                self.seat.width as f32,
                self.seat.height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(self.seat.x, self.seat.y, self.seat.width, self.seat.height);
            if !rects.is_empty() {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, rect_buffer.slice(..));
                pass.draw(0..6, 0..rects.len() as u32);
            }
            if let Some(vertex_buffer) = math_vertex_buffer.as_ref() {
                pass.set_pipeline(&self.math_pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for draw in &math_draws {
                    if let Some(texture) = self.math_textures.get(&draw.key)
                        && let Some(tile) = texture.tiles.get(draw.tile_index)
                    {
                        pass.set_bind_group(0, &tile.bind_group, &[]);
                        pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                    }
                }
            }
            if text_prepared {
                self.text_renderer
                    .render(&self.atlas, &self.viewport, &mut pass)
                    .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            }
            if text_prepared && !status_rects.is_empty() {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, status_rect_buffer.slice(..));
                pass.draw(0..6, 0..status_rects.len() as u32);
                self.status_text_renderer
                    .render(&self.atlas, &self.viewport, &mut pass)
                    .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            }
            if !math_overlays.is_empty() {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, math_overlay_buffer.slice(..));
                pass.draw(0..6, 0..math_overlays.len() as u32);
            }
            // The peek flyout is the topmost transient surface: above grid text, bands, the
            // status bar, and math toolbars.
            if let Some(rect_buffer) = peek_rect_buffer.as_ref() {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, rect_buffer.slice(..));
                pass.draw(0..6, 0..peek_rects.len() as u32);
            }
            if let Some(vertex_buffer) = peek_vertex_buffer.as_ref() {
                pass.set_pipeline(&self.math_pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for draw in &peek_draws {
                    if let Some(texture) = self.math_textures.get(&draw.key)
                        && let Some(tile) = texture.tiles.get(draw.tile_index)
                    {
                        pass.set_bind_group(0, &tile.bind_group, &[]);
                        pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                    }
                }
            }
            // Seat chrome last, with the pass restored to the whole window: it is
            // the one class of draw that legitimately owns the space between
            // seats. Skipped entirely when there is no chrome.
            if chrome_rect_buffer.is_some() || chrome_icon_buffer.is_some() || chrome_prepared {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                if let Some(buffer) = chrome_rect_buffer.as_ref() {
                    pass.set_pipeline(&self.rect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..chrome_rects.len() as u32);
                }
                // Marks sit between the flat fills and the text: the active tab's
                // own silhouette is a mark, and it has to land over the title
                // bar's fill and under the tab's title.
                if let Some(buffer) = chrome_icon_buffer.as_ref() {
                    pass.set_pipeline(&self.math_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for draw in &chrome_icon_draws {
                        if let Some(texture) = self.math_textures.get(&draw.key)
                            && let Some(tile) = texture.tiles.get(draw.tile_index)
                        {
                            pass.set_bind_group(0, &tile.bind_group, &[]);
                            pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                        }
                    }
                }
                if chrome_prepared {
                    self.chrome_text_renderer
                        .render(&self.atlas, &self.chrome_viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
            }
            // Preview content is above that seat's body chrome, but its viewport excludes the title
            // bar, so the filename and existing close affordance remain visible.
            if let (Some(seat), Some(vertex_buffer)) =
                (preview_seat, preview_vertex_buffer.as_ref())
            {
                pass.set_viewport(
                    seat.x as f32,
                    seat.y as f32,
                    seat.width as f32,
                    seat.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(seat.x, seat.y, seat.width, seat.height);
                pass.set_pipeline(&self.math_pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                for draw in &preview_draws {
                    if let Some(texture) = self.math_textures.get(&draw.key)
                        && let Some(tile) = texture.tiles.get(draw.tile_index)
                    {
                        pass.set_bind_group(0, &tile.bind_group, &[]);
                        pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                    }
                }
            }
            // The modal overlay, last of everything and over the whole window.
            // DESIGN §7.1.5: "模态遮罩 z-order 高于一切弹出层与浮窗" — and in this
            // pass "everything" has to include the two layers that outrank seat
            // chrome, the peek flyout and a preview seat's own picture. A scrim
            // that anything at all can be seen through unblurred is a scrim in
            // name only.
            if overlay_rect_buffer.is_some() || overlay_icon_buffer.is_some() || overlay_prepared {
                pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
                if let Some(buffer) = overlay_rect_buffer.as_ref() {
                    pass.set_pipeline(&self.rect_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..overlay_rects.len() as u32);
                }
                if let Some(buffer) = overlay_icon_buffer.as_ref() {
                    pass.set_pipeline(&self.math_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for draw in &overlay_icon_draws {
                        if let Some(texture) = self.math_textures.get(&draw.key)
                            && let Some(tile) = texture.tiles.get(draw.tile_index)
                        {
                            pass.set_bind_group(0, &tile.bind_group, &[]);
                            pass.draw(draw.first_vertex..draw.first_vertex + 6, 0..1);
                        }
                    }
                }
                if overlay_prepared {
                    self.overlay_text_renderer
                        .render(&self.atlas, &self.chrome_viewport, &mut pass)
                        .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
                }
            }
        }
        let encoded_at = Instant::now();
        self.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        self.queue.present(texture);
        let present_called_at = Instant::now();
        let receipt = PresentReceipt {
            trigger,
            submitted_at,
            present_called_at,
        };
        self.atlas.trim();
        if self.trace_perf {
            let total_elapsed = frame_started.elapsed();
            let digest_started = Instant::now();
            let digest = frame_content_digest(frame);
            let alternate_screen = frame_is_alternate_screen(frame);
            let digest_elapsed = digest_started.elapsed();
            self.perf_frame = self.perf_frame.saturating_add(1);
            eprintln!(
                "BT_PERF_TRACE frame={} source={:?} cells={} nonblank_cells={} first_text_row={} last_text_row={} content_fnv={:016x} alt={} digest_us={} validate_us={} viewport_us={} row_compose_us={} rows_reshaped={} row_cache_hits={} row_cache_misses={} row_cache_evictions={} row_cache_resident_bytes={} shape_miss_us={} narrow_hits={} narrow_misses={} narrow_evictions={} narrow_resident_bytes={} wide_hits={} wide_misses={} wide_evictions={} wide_resident_bytes={} atlas_prepare_upload_us={} atlas_hits=unmeasurable_glyphon_0_12 atlas_misses=unmeasurable_glyphon_0_12 atlas_grows=unmeasurable_glyphon_0_12 atlas_evictions=unmeasurable_glyphon_0_12 atlas_upload_bytes=unmeasurable_glyphon_0_12 rectangles_us={} math_prepare_upload_us={} math_blocks={} math_texture_evictions={} math_texture_refusals={} textureless_math_blocks={} math_texture_resident_bytes={} acquire_us={} encode_us={} submit_present_us={} total_us={}",
                self.perf_frame,
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
                self.math_texture_evictions,
                self.math_texture_refusals,
                self.textureless_math_blocks,
                self.math_textures.resident_bytes(),
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
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.narrow_shaping_cache,
            &mut self.wide_shaping_cache,
        )
    }

    fn prepare_math_draws(&mut self, frame: &ViewportFrame) -> MathDrawBatch {
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
            if self.math_textures.get(key).is_none()
                && let Some(texture) = self.upload_math_texture(&placement.artifact)
            {
                let (admitted, evictions) =
                    self.math_textures
                        .insert(key.clone(), texture, placement.artifact.rgba.len());
                self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
                if !admitted {
                    self.note_math_texture_refusal(key, placement.artifact.rgba.len());
                }
            }
            let Some(tile_geometry) = self.math_textures.get(key).map(|texture| {
                texture
                    .tiles
                    .iter()
                    .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                    .collect::<Vec<_>>()
            }) else {
                // A placed band with no texture. Silence here is what painted a bare grey
                // rectangle: the band's own pixels never drew while everything around them did.
                self.note_textureless_block(key, placement.artifact.rgba.len());
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
    fn note_textureless_block(&mut self, key: &str, resident_bytes: usize) {
        self.textureless_math_blocks = self.textureless_math_blocks.saturating_add(1);
        if self.trace_perf {
            eprintln!(
                "BT_PERF_TRACE math_block_without_texture key={key} bytes={resident_bytes} resident={}",
                self.math_textures.resident_bytes(),
            );
        }
    }

    /// Build the hover-peek flyout draws: border and background rects for the flat pipeline plus
    /// textured quads through the shared content-keyed texture LRU. Empty when no peek is up,
    /// when the window cannot host the box, or when the texture upload fails.
    fn prepare_peek_draws(&mut self) -> (Vec<RectInstance>, Vec<MathDraw>, Vec<MathVertex>) {
        let Some(overlay) = self.peek_overlay.clone() else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        let Some(layout) = peek_box_layout(
            self.seat.width as f32,
            self.seat.height as f32,
            self.metrics.padding_px,
            self.metrics.scale_factor as f32,
            overlay.width_px,
            overlay.height_px,
            overlay.pointer_x,
            overlay.pointer_y,
        ) else {
            return (Vec::new(), Vec::new(), Vec::new());
        };
        if self.math_textures.get(&overlay.key).is_none()
            && let Some(texture) =
                self.upload_rgba_tiles(&overlay.rgba, overlay.width_px, overlay.height_px)
        {
            let (admitted, evictions) =
                self.math_textures
                    .insert(overlay.key.clone(), texture, overlay.rgba.len());
            self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
            if !admitted {
                self.note_math_texture_refusal(&overlay.key, overlay.rgba.len());
            }
        }
        let Some(tile_geometry) = self.math_textures.get(&overlay.key).map(|texture| {
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
                self.seat.width,
                self.seat.height,
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
                self.pixel_rect_with_coverage(
                    fill.rect[0],
                    fill.rect[1],
                    fill.rect[2],
                    fill.rect[3],
                    fill.color,
                    fill.alpha,
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
        icons: &[ChromeIcon],
    ) -> (Vec<MathDraw>, Vec<MathVertex>) {
        let icons = icons.to_vec();
        let (surface_width, surface_height) = (self.config.width, self.config.height);
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        for icon in &icons {
            if self.math_textures.get(&icon.key).is_none()
                && let Some(texture) =
                    self.upload_rgba_tiles(&icon.rgba, icon.width_px, icon.height_px)
            {
                let (admitted, evictions) =
                    self.math_textures
                        .insert(icon.key.clone(), texture, icon.rgba.len());
                self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
                if !admitted {
                    self.note_math_texture_refusal(&icon.key, icon.rgba.len());
                }
            }
            let Some(tile_geometry) = self.math_textures.get(&icon.key).map(|texture| {
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
                    surface_width,
                    surface_height,
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

    fn prepare_preview_draws(&mut self) -> (Option<SeatViewport>, Vec<MathDraw>, Vec<MathVertex>) {
        let Some(image) = self.preview_image.clone() else {
            return (None, Vec::new(), Vec::new());
        };
        if self.math_textures.get(&image.key).is_none()
            && let Some(texture) =
                self.upload_rgba_tiles(&image.rgba, image.width_px, image.height_px)
        {
            let (admitted, evictions) =
                self.math_textures
                    .insert(image.key.clone(), texture, image.rgba.len());
            self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
            if !admitted {
                self.note_math_texture_refusal(&image.key, image.rgba.len());
            }
        }
        let Some(tile_geometry) = self.math_textures.get(&image.key).map(|texture| {
            texture
                .tiles
                .iter()
                .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                .collect::<Vec<_>>()
        }) else {
            return (Some(image.seat), Vec::new(), Vec::new());
        };
        let left_inset = (image.seat.width.saturating_sub(image.display_width_px) / 2) as f32;
        let top_inset = (image.seat.height.saturating_sub(image.display_height_px) / 2) as f32;
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
            ));
            draws.push(MathDraw {
                key: image.key.clone(),
                tile_index,
                first_vertex,
            });
        }
        (Some(image.seat), draws, vertices)
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

    fn upload_math_texture(
        &self,
        artifact: &bt_viewport::ProjectedMathArtifact,
    ) -> Option<CachedMathTexture> {
        self.upload_rgba_tiles(&artifact.rgba, artifact.width_px, artifact.height_px)
    }

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

    fn handle_surface_failure(
        &mut self,
        failure: SurfaceFailure,
    ) -> Result<PresentOutcome, RenderError> {
        match surface_failure_policy(failure) {
            SurfaceFailurePolicy::Skip => Ok(PresentOutcome::Skipped),
            SurfaceFailurePolicy::Reconfigure => {
                self.configure_surface()?;
                Ok(PresentOutcome::Reconfigure)
            }
            SurfaceFailurePolicy::FatalValidation => Err(RenderError::SurfaceValidation),
        }
    }

    fn configure_surface_if_needed(&mut self) -> Result<(), RenderError> {
        let requested_size = (self.config.width, self.config.height);
        if self.configured_size != requested_size {
            self.configure_surface()?;
        }
        Ok(())
    }

    fn configure_surface(&mut self) -> Result<(), RenderError> {
        self.surface.configure(&self.device, &self.config);
        self.configured_size = (self.config.width, self.config.height);
        Ok(())
    }

    fn rectangles(
        &self,
        frame: &ViewportFrame,
        drawn_math_blocks: &HashSet<usize>,
    ) -> Vec<RectInstance> {
        let columns = frame.columns.get() as usize;
        let drawable_rows = frame.drawable_rows();
        let mut rects = Vec::new();
        for (index, cell) in frame
            .cells
            .iter()
            .take(drawable_rows.saturating_mul(columns))
            .enumerate()
        {
            let (_, background) = resolve_colors(&cell.style);
            if background != default_background() {
                rects.push(self.cell_rect(frame, index / columns, index % columns, background));
            }
        }
        for span in &frame.selection_spans {
            let start = span.start_column.min(frame.columns.get()) as usize;
            let end = span.end_column.min(frame.columns.get()) as usize;
            if end > start && (span.row as usize) < drawable_rows {
                rects.push(self.cell_rect_span(
                    frame,
                    span,
                    start,
                    end - start,
                    DEFAULT_SELECTION_BACKGROUND_RGB,
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
        if cursor_quad_visible(
            frame.cursor.visible,
            self.window_focused,
            self.cursor_blink_visible,
        ) && (frame.cursor.row as usize) < drawable_rows
            && frame.cursor.column < frame.columns.get()
        {
            rects.extend(
                cursor_pixel_bounds(self.metrics, frame, self.window_focused)
                    .into_iter()
                    .map(|[left, top, right, bottom]| {
                        self.pixel_rect(left, top, right, bottom, DEFAULT_CURSOR_RGB)
                    }),
            );
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
                self.metrics.font_size_px / BASE_FONT_SIZE_LOGICAL_PX,
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
        rects
    }

    fn math_overlay_rectangles(&self, frame: &ViewportFrame) -> Vec<RectInstance> {
        let mut rects = Vec::new();
        let ink = foreground_rgb();
        let unit = self.metrics.scale_factor as f32;
        for placement in &frame.math_blocks {
            let Some(geometry) = self.math_block_geometry(frame, placement) else {
                continue;
            };
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
/// Deliberately a free function rather than a `Renderer` method: the seat-local
/// [`Renderer::pixel_rect`] and this one differ in exactly which rectangle they
/// call "the world", and having them side by side as one method with a flag is
/// how the two would eventually be confused for each other.
fn surface_pixel_rect(rect: [f32; 4], color: [u8; 3], width: u32, height: u32) -> RectInstance {
    surface_pixel_rect_with_alpha(rect, color, 1.0, width, height)
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

/// Shape every chrome label. The buffers are owned by the returned vector so
/// they outlive the `prepare` that borrows them.
fn shape_chrome_labels(
    font_system: &mut FontSystem,
    labels: &[ChromeLabel],
    cap_height_ratio: f32,
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
            let mut attrs = Attrs::new().family(Family::SansSerif);
            if label.letter_spacing_em != 0.0 {
                attrs = attrs.letter_spacing(label.letter_spacing_em);
            }
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
            ChromeTextLayout {
                buffer,
                left,
                top: baseline - baseline_in_buffer,
                bounds: TextBounds {
                    left: label.rect[0].floor() as i32,
                    top: label.rect[1].floor() as i32,
                    right: label.rect[2].ceil() as i32,
                    bottom: label.rect[3].ceil() as i32,
                },
                color: Color::rgb(r, g, b),
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
                default_color: glyph.color,
                custom_glyphs: &[],
            }
        })
    });
    let wide_text_areas = text_rows.iter().enumerate().flat_map(|(row, text_row)| {
        text_row.wide_glyphs.iter().map(move |wide| {
            let [left, top, _, bottom] = frame_cell_bounds_px(metrics, frame, row, wide.column);
            TextArea {
                buffer: &wide.buffer,
                left,
                top: top + wide.top_offset_px,
                scale: 1.0,
                bounds: TextBounds {
                    left: left.floor() as i32,
                    top: top.floor() as i32,
                    right: (left + 2.0 * metrics.cell_width_px).ceil() as i32,
                    bottom: bottom.ceil() as i32,
                },
                default_color: wide.color,
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
            left,
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
/// This is the shared slice boundary used by `Renderer::prepare_text_rows` and deterministic
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

fn cursor_quad_visible(
    terminal_cursor_visible: bool,
    window_focused: bool,
    blink_phase_visible: bool,
) -> bool {
    terminal_cursor_visible && (!window_focused || blink_phase_visible)
}

fn cursor_pixel_bounds(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    focused: bool,
) -> Vec<[f32; 4]> {
    let (column, span) = cursor_cell_span(frame);
    let [left, top, _, bottom] =
        frame_cell_bounds_px(metrics, frame, frame.cursor.row as usize, column);
    let right = left + span as f32 * metrics.cell_width_px;
    if focused {
        return focused_cursor_pixel_bounds(
            metrics,
            [left, top, right, bottom],
            current_cursor_style(),
        );
    }

    // Match Windows Terminal's focus cue: retain a visible one-device-pixel hollow caret while
    // allowing the cell contents to remain readable through its center.
    // This stays the whole cell's outline for every selected focused shape.
    let stroke = 1.0_f32.min((right - left) / 2.0).min((bottom - top) / 2.0);
    vec![
        [left, top, right, top + stroke],
        [left, bottom - stroke, right, bottom],
        [left, top + stroke, left + stroke, bottom - stroke],
        [right - stroke, top + stroke, right, bottom - stroke],
    ]
}

fn focused_cursor_pixel_bounds(
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

#[cfg(target_os = "windows")]
fn terminal_font_system() -> FontSystem {
    // Keep startup bounded: load a fixed terminal/CJK/symbol fallback chain, never enumerate
    // Fonts/. Noto Color Emoji is compiled into the executable so tests and a standalone binary
    // do not depend on their working directory or on an installer copying a sidecar font.
    // Microsoft YaHei UI and DengXian cover Simplified Chinese on supported Windows versions;
    // SimSun is the final compatibility face. Missing optional files are harmless.
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
        "msyh.ttc",
        "msyhbd.ttc",
        "msyhl.ttc",
        "Deng.ttf",
        "Dengb.ttf",
        "Dengl.ttf",
        "simsun.ttc",
        "seguiemj.ttf",
        "seguisym.ttf",
    ] {
        let _ = db.load_font_file(fonts.join(file));
    }
    db.set_monospace_family(PRIMARY_FONT_FAMILY);
    load_chrome_sans_family(&mut db, &fonts);
    FontSystem::new_with_locale_and_db("en-US".to_owned(), db)
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
            let segoe_family = Family::Name(SEGOE_COLOR_EMOJI_FONT_FAMILY);
            if font_family_available(font_system, SEGOE_COLOR_EMOJI_FONT_FAMILY) {
                #[cfg(test)]
                {
                    *color_emoji_trial_shapes = color_emoji_trial_shapes.saturating_add(1);
                }
                let segoe = shape_narrow_buffer(
                    key,
                    font_system,
                    metrics,
                    narrow_emoji_em_scale(metrics),
                    segoe_family,
                );
                if is_color_cluster_from_family_within_slot(
                    &segoe,
                    font_system,
                    swash_cache,
                    SEGOE_COLOR_EMOJI_FONT_FAMILY,
                    metrics.cell_height_px,
                    metrics.cell_height_px,
                ) {
                    return (segoe, segoe_family, NarrowSizePolicy::CellHeightEmoji);
                }
            }

            let noto_family = Family::Name(COLOR_EMOJI_FONT_FAMILY);
            (
                shape_narrow_buffer(
                    key,
                    font_system,
                    metrics,
                    narrow_emoji_em_scale(metrics),
                    noto_family,
                ),
                noto_family,
                NarrowSizePolicy::CellHeightEmoji,
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NarrowSizePolicy {
    StrictCell,
    TextCoordinated,
    CellHeightEmoji,
}

fn narrow_emoji_em_scale(metrics: CellMetrics) -> f32 {
    metrics.cell_height_px / metrics.font_size_px
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
    font_system.db().face(id).is_some_and(|face| {
        face.families
            .iter()
            .any(|(family, _)| family == PRIMARY_FONT_FAMILY)
    })
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
            let (buffer, top_offset_px) =
                cache.get_or_shape(key, font_system, swash_cache, metrics);
            let (foreground, _) = resolve_colors(&slot.style);
            WideGlyph {
                column: slot.column,
                buffer,
                top_offset_px,
                color: Color::rgb(foreground[0], foreground[1], foreground[2]),
            }
        })
        .collect()
}

fn shape_wide_buffer(
    key: &ShapeKey,
    font_system: &mut FontSystem,
    metrics: CellMetrics,
    family: Family<'static>,
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
    // A CJK full-width glyph owns a two-cell slot. Matching one cell would shrink the fallback
    // face to half width; omitting this entirely leaves each fallback face at a different visual
    // size. Let cosmic-text normalize the fallback em to the full slot.
    buffer.set_monospace_width(Some(metrics.font_size_px * wide_slot_em_scale(metrics)));
    let attrs = shape_attrs(key, family);
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
) -> Buffer {
    match font_presentation_route(&key.text, font_system) {
        PresentationRoute::TerminalText => {
            shape_wide_buffer(key, font_system, metrics, Family::Monospace)
        }
        PresentationRoute::TextSymbol => shape_wide_buffer(
            key,
            font_system,
            metrics,
            Family::Name(TEXT_SYMBOL_FONT_FAMILY),
        ),
        PresentationRoute::ColorEmoji => {
            if font_family_available(font_system, SEGOE_COLOR_EMOJI_FONT_FAMILY) {
                #[cfg(test)]
                {
                    *color_emoji_trial_shapes = color_emoji_trial_shapes.saturating_add(1);
                }
                let segoe = shape_wide_buffer(
                    key,
                    font_system,
                    metrics,
                    Family::Name(SEGOE_COLOR_EMOJI_FONT_FAMILY),
                );
                if is_color_cluster_from_family_within_slot(
                    &segoe,
                    font_system,
                    swash_cache,
                    SEGOE_COLOR_EMOJI_FONT_FAMILY,
                    2.0 * metrics.cell_width_px,
                    metrics.cell_height_px,
                ) {
                    return segoe;
                }
            }

            shape_wide_buffer(
                key,
                font_system,
                metrics,
                Family::Name(COLOR_EMOJI_FONT_FAMILY),
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
        families: &[Family::Name(PRIMARY_FONT_FAMILY)],
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

fn theme_clear_color() -> wgpu::Color {
    let [r, g, b] = srgb_rgb_to_linear(default_background());
    wgpu::Color { r, g, b, a: 1.0 }
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
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("terminal rectangle shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("terminal rectangle pipeline layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("terminal rectangle pipeline"),
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
) -> [MathVertex; 6] {
    let width = viewport_width.max(1) as f32;
    let height = viewport_height.max(1) as f32;
    let position = |x: f32, y: f32| [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0];
    [
        MathVertex {
            position: position(left, top),
            uv: [uv_left, uv_top],
        },
        MathVertex {
            position: position(left, bottom),
            uv: [uv_left, uv_bottom],
        },
        MathVertex {
            position: position(right, bottom),
            uv: [uv_right, uv_bottom],
        },
        MathVertex {
            position: position(left, top),
            uv: [uv_left, uv_top],
        },
        MathVertex {
            position: position(right, bottom),
            uv: [uv_right, uv_bottom],
        },
        MathVertex {
            position: position(right, top),
            uv: [uv_right, uv_top],
        },
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
    placement.display != MathBlockDisplay::Source
        && frame
            .drawable_interval_overlaps(placement.top_subpixels, placement.clip_height_subpixels)
}

fn surface_config_size(width: u32, height: u32, max_texture_dimension_2d: u32) -> (u32, u32) {
    let limit = max_texture_dimension_2d.max(1);
    (width.max(1).min(limit), height.max(1).min(limit))
}

fn resolve_colors(style: &CellStyle) -> ([u8; 3], [u8; 3]) {
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

fn terminal_color(color: TerminalColor, foreground: bool) -> [u8; 3] {
    // Named codes 16..=28 are the stable BetterTerminal encoding declared by bt-transcript.
    match color {
        TerminalColor::Rgb(r, g, b) => [r, g, b],
        TerminalColor::Indexed(index) => indexed_color(index),
        TerminalColor::Named(16 | 27) if foreground => default_foreground(),
        TerminalColor::Named(17) if !foreground => default_background(),
        TerminalColor::Named(18) => DEFAULT_CURSOR_RGB,
        TerminalColor::Named(28) => DEFAULT_DIM_FOREGROUND_RGB,
        TerminalColor::Named(code @ 19..=26) => {
            indexed_color(code - 19).map(|channel| channel.saturating_mul(2) / 3)
        }
        TerminalColor::Named(code) => indexed_color(code.min(15)),
    }
}

fn indexed_color(index: u8) -> [u8; 3] {
    if index < 16 {
        return ansi_16_rgb()[index as usize];
    }
    if index < 232 {
        let cube = index - 16;
        let component = |value: u8| if value == 0 { 0 } else { 55 + 40 * value };
        return [
            component(cube / 36),
            component((cube % 36) / 6),
            component(cube % 6),
        ];
    }
    let gray = 8 + 10 * (index - 232);
    [gray, gray, gray]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_transcript::CapturedCell;

    #[test]
    fn peek_box_layout_places_below_right_without_upscaling() {
        let layout = peek_box_layout(1000.0, 800.0, 8.0, 1.0, 100, 50, 100.0, 100.0).unwrap();
        // 1x thumbnail (no upscale), border 1, inset 6: box is 114x64 at pointer + (12, 18).
        assert_eq!(layout.frame, [112.0, 118.0, 226.0, 182.0]);
        assert_eq!(layout.interior, [113.0, 119.0, 225.0, 181.0]);
        assert_eq!(layout.image, [119.0, 125.0, 219.0, 175.0]);
    }

    #[test]
    fn peek_box_layout_flips_above_a_bottom_pointer() {
        let layout = peek_box_layout(1000.0, 800.0, 8.0, 1.0, 100, 50, 100.0, 780.0).unwrap();
        assert_eq!(layout.frame[1], 780.0 - 18.0 - 64.0);
        assert!(layout.frame[3] <= 792.0);
    }

    #[test]
    fn peek_box_layout_caps_large_images_preserving_aspect_and_clamps_horizontally() {
        let layout = peek_box_layout(1000.0, 800.0, 8.0, 1.0, 4000, 2000, 950.0, 100.0).unwrap();
        let width = layout.image[2] - layout.image[0];
        let height = layout.image[3] - layout.image[1];
        assert!(width <= (1000.0 - 16.0) * 0.4 + 1e-3);
        // The extent is whole pixels, so the aspect is preserved to within that quantization.
        assert!((height - width * 0.5).abs() <= 1.0);
        // Pointer near the right edge: the box clamps inside the padded pane.
        assert!(layout.frame[2] <= 992.0 + 1e-3);
        assert!(layout.frame[0] >= 8.0);
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
        assert!(peek_box_layout(30.0, 30.0, 8.0, 1.0, 10, 10, 10.0, 10.0).is_none());
        assert!(peek_box_layout(1000.0, 800.0, 8.0, 1.0, 0, 10, 10.0, 10.0).is_none());
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
        // A 4K pane showing three image bands, each already display-sized (14bab58): full padded
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
                start: bt_transcript::TranscriptId(1),
                end: bt_transcript::TranscriptId(1),
            },
            source: key.to_owned(),
            artifact: bt_viewport::ProjectedMathArtifact {
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
            if expected_family == SEGOE_COLOR_EMOJI_FONT_FAMILY {
                let mut swash_cache = SwashCache::new();
                assert!(
                    is_color_cluster_from_family_within_slot(
                        &wide.buffer,
                        &mut font_system,
                        &mut swash_cache,
                        expected_family,
                        2.0 * metrics.cell_width_px,
                        metrics.cell_height_px,
                    ),
                    "{text} Segoe composition must normalize into its double-cell slot"
                );
            } else {
                assert_eq!(
                    raster_content(&mut font_system, &wide.buffer),
                    glyphon::SwashContent::Color,
                    "{text} Noto fallback must remain on glyphon's color atlas"
                );
            }
            assert_eq!(
                wide.buffer.monospace_width(),
                Some(2.0 * metrics.cell_width_px)
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn default_text_emoji_uses_cell_height_size_centered_over_its_narrow_cell() {
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
            assert_eq!(glyphs[0].font_size, metrics.cell_height_px);
            assert!(
                occupied_width_px(&mut font_system, &shaped[0].buffer) > metrics.cell_width_px,
                "scale {scale_factor}: ⚠ must retain square emoji size and may overhang one cell"
            );
            let mut swash_cache = SwashCache::new();
            let [left, top, right, bottom] =
                glyph_ink_bounds(&shaped[0].buffer, &mut font_system, &mut swash_cache).unwrap();
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
        assert_eq!(glyph_family(&font_system, &glyph), PRIMARY_FONT_FAMILY);
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
                BASE_FONT_SIZE_LOGICAL_PX * scale_factor as f32
            );
            assert_eq!(
                metrics.cell_height_px,
                (BASE_LINE_HEIGHT_LOGICAL_PX * scale_factor as f32).ceil()
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
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 18 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(1),
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 36 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(2),
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 54 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(3),
                },
            ],
            selection_spans: vec![bt_viewport::SelectionSpan {
                row: 2,
                start_column: 0,
                end_column: 2,
            }],
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
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 11 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: Some(1),
                },
                bt_viewport::FrameVisualRow {
                    top_subpixels: 29 * unit,
                    height_subpixels: 18 * unit,
                    live_grid_row: None,
                },
            ],
            selection_spans: Vec::new(),
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
                })
                .collect(),
            cursor: bt_viewport::GridCursor {
                row: 0,
                column: 0,
                visible: true,
            },
            selection_spans: Vec::new(),
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
        assert_eq!(DEFAULT_CURSOR_RGB, [0xd4, 0xd4, 0xd4]);
        assert_eq!(
            terminal_color(TerminalColor::Named(18), true),
            DEFAULT_CURSOR_RGB,
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
    /// expression `Renderer::resize` applies to `self.seat`.
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
        changed_text.text = "y".to_owned();
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

    #[test]
    fn theme_switch_recomposes_an_ansi_colored_rendered_row() {
        static THEME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        struct RestoreTheme(Theme);
        impl Drop for RestoreTheme {
            fn drop(&mut self) {
                let _ = set_theme(self.0);
            }
        }

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
            Color::rgb(0xab, 0x64, 0x00)
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
                math_blocks: Vec::new(),
                math_failures: Vec::new(),
                status_text: None,
                viewport_origin: FrameViewportOrigin::Bottom,
                scroll_offset_rows: 0,
                layout_key: bt_doc_layout_key(1),
                view_generation: bt_doc::ViewGeneration(1),
            };
            let cell = frame_cell_bounds_px(metrics, &frame, 0, 0);
            let bar = focused_cursor_pixel_bounds(metrics, cell, CursorStyle::Bar);
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
                focused_cursor_pixel_bounds(metrics, cell, CursorStyle::Block),
                vec![cell],
                "at {dpi_milli} milli-DPI block is the whole cell"
            );
            let underline = focused_cursor_pixel_bounds(metrics, cell, CursorStyle::Underline);
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

    #[test]
    fn unfocused_cursor_is_a_visible_hollow_outline_and_focus_restores_the_caret() {
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
            math_blocks: Vec::new(),
            math_failures: Vec::new(),
            status_text: None,
            viewport_origin: FrameViewportOrigin::Bottom,
            scroll_offset_rows: 0,
            layout_key: bt_doc_layout_key(1),
            view_generation: bt_doc::ViewGeneration(1),
        };

        assert_eq!(
            cursor_pixel_bounds(metrics, &frame, true),
            vec![[4.0, 4.0, 5.0, 24.0]],
            "a focused caret is the one-logical-pixel bar"
        );
        let outline = cursor_pixel_bounds(metrics, &frame, false);
        assert_eq!(outline.len(), 4);
        assert_eq!(outline[0], [4.0, 4.0, 12.0, 5.0]);
        assert_eq!(outline[1], [4.0, 23.0, 12.0, 24.0]);
        assert_eq!(outline[2], [4.0, 5.0, 5.0, 23.0]);
        assert_eq!(outline[3], [11.0, 5.0, 12.0, 23.0]);
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
        let layouts =
            shape_chrome_labels(font_system, std::slice::from_ref(label), cap_height_ratio);
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
            text: "✳ PowerShell".to_owned(),
            rect: [0.0, 0.0, 400.0, 34.0],
            font_size_px: WINDOW_TAB_FONT_LOGICAL_PX,
            color: [255, 255, 255],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
        };
        let layouts = shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7);
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
                text: "PowerShell".to_owned(),
                rect: [64.0, tab_top, 400.0, title],
                font_size_px: WINDOW_TAB_FONT_LOGICAL_PX * scale,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
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
            let bar = SEAT_TITLE_BAR_LOGICAL_PX * scale;
            let label = ChromeLabel {
                text: "Terminal".to_owned(),
                rect: [48.0, 0.0, 400.0, bar],
                font_size_px: SEAT_TITLE_FONT_LOGICAL_PX * scale,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
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
                text: text.to_owned(),
                rect: [0.0, 0.0, 40.0, 28.0],
                font_size_px: size,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: 0.0,
            };
            let layouts = shape_chrome_labels(&mut font_system, std::slice::from_ref(&label), 0.7);
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
                text: text.to_owned(),
                rect: [0.0, 0.0, 4000.0, 20.0],
                font_size_px: size,
                color: [255, 255, 255],
                align_right: false,
                align_center: false,
                letter_spacing_em: spacing,
            };
            shape_chrome_labels(font_system, std::slice::from_ref(&label), 0.7)[0]
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
}
