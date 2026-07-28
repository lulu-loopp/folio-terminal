//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

mod procedural;
mod theme;

use std::{
    collections::HashMap,
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

use theme::{ANSI_16_RGB, DEFAULT_CURSOR_RGB, DEFAULT_DIM_FOREGROUND_RGB};
pub use theme::{DEFAULT_BACKGROUND_RGB, background_rgb, foreground_rgb, theme_revision};
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
const MATH_TEXTURE_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
const PRIMARY_FONT_FAMILY: &str = "Consolas";
const COLOR_EMOJI_FONT_FAMILY: &str = "Noto Color Emoji";
const SEGOE_COLOR_EMOJI_FONT_FAMILY: &str = "Segoe UI Emoji";
const TEXT_SYMBOL_FONT_FAMILY: &str = "Segoe UI Symbol";
const NARROW_FALLBACK_SIDE_BEARING_EM: f32 = 0.05;
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
        frame.rows.get(),
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
    let rows = frame.rows.get() as usize;
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

    for (index, cell) in frame.cells.iter().enumerate() {
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
    status_overlay: Option<String>,
}

impl Hash for ComposedRowKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.metrics.hash(state);
        self.font_revision.hash(state);
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
        .saturating_add(
            cell.hyperlink
                .as_ref()
                .map_or(0, |hyperlink| hyperlink.capacity()),
        )
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
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    status_text_renderer: TextRenderer,
    rect_pipeline: wgpu::RenderPipeline,
    math_pipeline: wgpu::RenderPipeline,
    math_bind_group_layout: wgpu::BindGroupLayout,
    math_sampler: wgpu::Sampler,
    math_textures: ByteLru<String, CachedMathTexture>,
    math_texture_evictions: u64,
    metrics: CellMetrics,
    init_timings: RendererInitTimings,
    text_rows: Vec<Arc<ComposedRow>>,
    status_overlay: Option<Arc<ComposedRow>>,
    composed_row_cache: ComposedRowCache,
    font_revision: u64,
    narrow_shaping_cache: NarrowShapingCache,
    wide_shaping_cache: WideShapingCache,
    glyph_degraded_frames: u64,
    window_focused: bool,
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
            if placement.display == MathBlockDisplay::Source {
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
        let font_metrics_time = phase_started.elapsed();
        let phase_started = Instant::now();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let status_text_renderer =
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
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            status_text_renderer,
            rect_pipeline,
            math_pipeline,
            math_bind_group_layout,
            math_sampler,
            math_textures: ByteLru::new(MATH_TEXTURE_CACHE_BUDGET_BYTES),
            math_texture_evictions: 0,
            metrics,
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

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let swapchain_size = surface_config_size(width, height, self.max_texture_dimension_2d);
        self.config.width = swapchain_size.0;
        self.config.height = swapchain_size.1;
        Ok(())
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
        self.viewport.update(
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

        let rects = self.rectangles(frame);
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
            .and_then(|status| status_overlay_geometry(self.metrics, frame, status))
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
        let rectangles_prepared_at = Instant::now();
        let (math_draws, math_vertices) = self.prepare_math_draws(frame);
        let math_vertex_buffer = (!math_vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("visible math block vertices"),
                    contents: bytemuck::cast_slice(&math_vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let math_prepared_at = Instant::now();
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
            pass.set_viewport(
                0.0,
                0.0,
                self.config.width as f32,
                self.config.height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(0, 0, self.config.width, self.config.height);
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
                "BT_PERF_TRACE frame={} source={:?} cells={} nonblank_cells={} first_text_row={} last_text_row={} content_fnv={:016x} alt={} digest_us={} validate_us={} viewport_us={} row_compose_us={} rows_reshaped={} row_cache_hits={} row_cache_misses={} row_cache_evictions={} row_cache_resident_bytes={} shape_miss_us={} narrow_hits={} narrow_misses={} narrow_evictions={} narrow_resident_bytes={} wide_hits={} wide_misses={} wide_evictions={} wide_resident_bytes={} atlas_prepare_upload_us={} atlas_hits=unmeasurable_glyphon_0_12 atlas_misses=unmeasurable_glyphon_0_12 atlas_grows=unmeasurable_glyphon_0_12 atlas_evictions=unmeasurable_glyphon_0_12 atlas_upload_bytes=unmeasurable_glyphon_0_12 rectangles_us={} math_prepare_upload_us={} math_blocks={} math_texture_evictions={} math_texture_resident_bytes={} acquire_us={} encode_us={} submit_present_us={} total_us={}",
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
                (rectangles_prepared_at - atlas_prepared_at).as_micros(),
                (math_prepared_at - rectangles_prepared_at).as_micros(),
                frame.math_blocks.len(),
                self.math_texture_evictions,
                self.math_textures.resident_bytes(),
                (surface_acquired_at - math_prepared_at).as_micros(),
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
            &mut self.font_system,
            &mut self.swash_cache,
            &mut self.narrow_shaping_cache,
            &mut self.wide_shaping_cache,
        )
    }

    fn prepare_math_draws(&mut self, frame: &ViewportFrame) -> (Vec<MathDraw>, Vec<MathVertex>) {
        // UI-UX §7.5c, M1.9a ruling: do not invent automatic math line breaking. With terminal
        // wrapping on (the current native default), the pane clips a left-aligned, max-content
        // raster and therefore acts as the block's horizontal viewport. Scrolling controls are
        // part of the M1.9b interaction slice; default blockMax is unlimited, so no vertical clamp
        // is applied here.
        let mut draws = Vec::new();
        let mut vertices = Vec::new();
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.config.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.config.height as f32;

        for placement in &frame.math_blocks {
            if placement.display == MathBlockDisplay::Source {
                continue;
            }
            let key = &placement.artifact.key;
            if self.math_textures.get(key).is_none()
                && let Some(texture) = self.upload_math_texture(&placement.artifact)
            {
                let (_, evictions) =
                    self.math_textures
                        .insert(key.clone(), texture, placement.artifact.rgba.len());
                self.math_texture_evictions = self.math_texture_evictions.saturating_add(evictions);
            }
            let Some(tile_geometry) = self.math_textures.get(key).map(|texture| {
                texture
                    .tiles
                    .iter()
                    .map(|tile| (tile.x_px, tile.y_px, tile.width_px, tile.height_px))
                    .collect::<Vec<_>>()
            }) else {
                continue;
            };
            let Some(geometry) = self.math_block_geometry(frame, placement) else {
                continue;
            };
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
                    self.config.width,
                    self.config.height,
                ));
                draws.push(MathDraw {
                    key: key.clone(),
                    tile_index,
                    first_vertex,
                });
            }
        }
        (draws, vertices)
    }

    fn math_block_geometry(
        &self,
        frame: &ViewportFrame,
        placement: &MathBlockPlacement,
    ) -> Option<MathBlockGeometry> {
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.config.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.config.height as f32;
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
            self.config.width,
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
        let pane_left = self.metrics.padding_px;
        let pane_right = (pane_left + frame.columns.get() as f32 * self.metrics.cell_width_px)
            .min(self.config.width as f32);
        let pane_top = self.metrics.padding_px;
        let pane_bottom = self.config.height as f32;
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

    fn rectangles(&self, frame: &ViewportFrame) -> Vec<RectInstance> {
        let columns = frame.columns.get() as usize;
        let mut rects = Vec::new();
        for (index, cell) in frame.cells.iter().enumerate() {
            let (_, background) = resolve_colors(&cell.style);
            if background != default_background() {
                rects.push(self.cell_rect(frame, index / columns, index % columns, background));
            }
        }
        for span in &frame.selection_spans {
            let start = span.start_column.min(frame.columns.get()) as usize;
            let end = span.end_column.min(frame.columns.get()) as usize;
            if end > start && span.row < frame.rows.get() {
                rects.push(self.cell_rect_span(
                    frame,
                    span,
                    start,
                    end - start,
                    DEFAULT_SELECTION_BACKGROUND_RGB,
                ));
            }
        }
        for placement in &frame.math_blocks {
            if placement.toolbar_visible
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
        if frame.cursor.visible
            && frame.cursor.row < frame.rows.get()
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
        for (index, cell) in frame.cells.iter().enumerate() {
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
        for (index, cell) in frame.cells.iter().enumerate() {
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
            }
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
        let width = self.config.width.max(1) as f32;
        let height = self.config.height.max(1) as f32;
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
    let rows = frame.rows.get() as usize;
    let mut next_rows = Vec::with_capacity(rows);

    for source_cells in source_rows {
        // Row placement is intentionally absent from this key. Cached rows own shaping only;
        // `prepare_text_atlas` remaps the same Arc through the presented frame's live prefix map.
        let key = ComposedRowKey {
            cells: source_cells.to_vec(),
            metrics: metrics.into(),
            font_revision,
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
) -> Result<(), PrepareError> {
    let Some(status) = frame.status_text.as_deref() else {
        return Ok(());
    };
    let Some(row) = status_overlay else {
        return Ok(());
    };
    let Some(geometry) = status_overlay_geometry(metrics, frame, status) else {
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

/// Validate the complete render frame before exposing exact terminal rows to text shaping.
///
/// This is the shared slice boundary used by `Renderer::prepare_text_rows` and deterministic
/// resize replay tests. `chunks_exact` is only constructed after the rectangularity proof.
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

fn status_overlay_geometry(
    metrics: CellMetrics,
    frame: &ViewportFrame,
    status: &str,
) -> Option<StatusOverlayGeometry> {
    if frame.rows.get() < 2 {
        return None;
    }
    let columns = frame.columns.get() as usize;
    let shown = status.chars().count().min(columns);
    if shown == 0 {
        return None;
    }
    let first_column = columns - shown;
    let right = metrics.padding_px + columns as f32 * metrics.cell_width_px;
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
    let [left, top, right, bottom] = frame_cell_bounds_px(
        metrics,
        frame,
        frame.cursor.row as usize,
        frame.cursor.column as usize,
    );
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
        return vec![[left, top, right, bottom]];
    }

    // Match Windows Terminal's focus cue: retain a visible one-device-pixel hollow caret while
    // allowing the cell contents to remain readable through its center.
    let stroke = 1.0_f32.min((right - left) / 2.0).min((bottom - top) / 2.0);
    vec![
        [left, top, right, top + stroke],
        [left, bottom - stroke, right, bottom],
        [left, top + stroke, left + stroke, bottom - stroke],
        [right - stroke, top + stroke, right, bottom - stroke],
    ]
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
    FontSystem::new_with_locale_and_db("en-US".to_owned(), db)
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
        return ANSI_16_RGB[index as usize];
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
            rows: NonZeroU32::new(rows as u32).unwrap(),
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
        let geometry = status_overlay_geometry(metrics, &frame, "2 rows above").unwrap();
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
                rows: NonZeroU32::new(3).unwrap(),
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
            rows: NonZeroU32::new(4).unwrap(),
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
            vec![[12.0, 40.0, 20.0, 58.0]]
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
    fn real_math_decoration_keeps_inter_block_selection_in_its_own_row_map_band() {
        let started = std::time::Instant::now();
        let mut session = bt_term::DualPlaneSession::new(
            NonZeroU32::new(40).unwrap(),
            NonZeroU32::new(24).unwrap(),
        );
        session
            .feed_at(
                b"\x1b[?1049h$$x0$$\r\nafter-0\r\n$$x1$$\r\nafter-1\r\n$$x2$$\r\nafter-2\r\n$$x3$$\r\nafter-3",
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
        let status_geometry = status_overlay_geometry(metrics, &selected, status).unwrap();
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
            rows: NonZeroU32::new(1).unwrap(),
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
            rows: NonZeroU32::new(rows).unwrap(),
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

    #[test]
    fn publish_composition_and_text_row_boundary_reject_non_rectangular_frames() {
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(2).unwrap(),
            rows: NonZeroU32::new(2).unwrap(),
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
        assert_eq!(DEFAULT_BACKGROUND_RGB, [0x0c, 0x0c, 0x0c]);
        let expected_background = std::env::var("BT_BG")
            .ok()
            .and_then(|value| theme::parse_background_rgb(&value))
            .unwrap_or(DEFAULT_BACKGROUND_RGB);
        assert_eq!(default_background(), expected_background);
        assert_eq!(default_foreground(), foreground_rgb());
        assert_eq!(DEFAULT_CURSOR_RGB, [0xff, 0xff, 0xff]);
        assert_eq!(
            terminal_color(TerminalColor::Named(18), true),
            DEFAULT_CURSOR_RGB,
            "the cursor quad and cursor named color share Campbell white"
        );
        assert_eq!(
            ANSI_16_RGB,
            [
                [0x0c, 0x0c, 0x0c],
                [0xc5, 0x0f, 0x1f],
                [0x13, 0xa1, 0x0e],
                [0xc1, 0x9c, 0x00],
                [0x00, 0x37, 0xda],
                [0x88, 0x17, 0x98],
                [0x3a, 0x96, 0xdd],
                [0xcc, 0xcc, 0xcc],
                [0x76, 0x76, 0x76],
                [0xe7, 0x48, 0x56],
                [0x16, 0xc6, 0x0c],
                [0xf9, 0xf1, 0xa5],
                [0x3b, 0x78, 0xff],
                [0xb4, 0x00, 0x9e],
                [0x61, 0xd6, 0xd6],
                [0xf2, 0xf2, 0xf2],
            ]
        );
        for (index, expected) in ANSI_16_RGB.into_iter().enumerate() {
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
            ANSI_16_RGB[0],
            "explicit ANSI black must resolve through palette slot 0"
        );
        assert_eq!(
            terminal_color(TerminalColor::Indexed(15), true),
            ANSI_16_RGB[15],
            "indexed ANSI bright white must resolve through palette slot 15"
        );
    }

    #[test]
    fn srgb_theme_colors_are_linearized_at_clear_and_rect_upload_boundaries() {
        let clear = theme_clear_color();
        let expected = 0.003_676_507_324_047_436;
        assert!((srgb_channel_to_linear(12) - expected).abs() < f64::EPSILON);
        assert_eq!([clear.r, clear.g, clear.b], [expected; 3]);
        assert_eq!(clear.a, 1.0);

        let rect = rect_gpu_color(default_background());
        assert_eq!(
            rect,
            [expected as f32, expected as f32, expected as f32, 1.0]
        );
        assert_ne!(
            rect[0],
            12.0 / 255.0,
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
            rows: NonZeroU32::new(3).unwrap(),
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
        base.hyperlink = Some("https://example.invalid".to_owned());
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
        let status = ComposedRowKey {
            status_overlay: Some("status".to_owned()),
            ..key.clone()
        };
        assert_ne!(key, scaled);
        assert_ne!(key, revised);
        assert_ne!(key, status);
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
            rows: NonZeroU32::new(2).unwrap(),
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
            rows: NonZeroU32::new(2).unwrap(),
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

    #[test]
    fn cursor_on_either_half_of_a_wide_cell_covers_both_cells() {
        let mut lead = CapturedCell::plain("中");
        lead.style.flags.insert(CellFlags::WIDE_CHAR);
        let mut spacer = CapturedCell::plain("");
        spacer.wide_spacer = true;
        let mut frame = ViewportFrame {
            columns: NonZeroU32::new(3).unwrap(),
            rows: NonZeroU32::new(1).unwrap(),
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
    }

    #[test]
    fn unfocused_cursor_is_a_visible_hollow_outline_and_focus_restores_the_block() {
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
            rows: NonZeroU32::new(1).unwrap(),
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
            vec![[4.0, 4.0, 12.0, 24.0]]
        );
        let outline = cursor_pixel_bounds(metrics, &frame, false);
        assert_eq!(outline.len(), 4);
        assert_eq!(outline[0], [4.0, 4.0, 12.0, 5.0]);
        assert_eq!(outline[1], [4.0, 23.0, 12.0, 24.0]);
        assert_eq!(outline[2], [4.0, 5.0, 5.0, 23.0]);
        assert_eq!(outline[3], [11.0, 5.0, 12.0, 23.0]);
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
    fn atlas_exhaustion_degrades_the_frame_instead_of_exiting() {
        assert_eq!(
            prepare_failure_policy(PrepareError::AtlasFull),
            PrepareFailurePolicy::PresentWithoutText
        );
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
