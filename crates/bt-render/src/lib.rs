//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

mod procedural;
mod theme;

use std::{
    collections::HashMap,
    num::{NonZeroI64, NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_transcript::{CapturedCell, CellFlags, CellStyle, TerminalColor};
use bt_unicode::{cluster_width, graphemes};
use bt_viewport::{SUBPIXELS_PER_PX, ViewportFrame};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, PrepareError, Resolution, Shaping,
    Stretch, Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
    Wrap,
};
use thiserror::Error;
use unicode_properties::emoji::{EmojiStatus, UnicodeEmoji};
use wgpu::util::DeviceExt;

pub use theme::DEFAULT_BACKGROUND_RGB;
use theme::{ANSI_16_RGB, DEFAULT_CURSOR_RGB, DEFAULT_DIM_FOREGROUND_RGB, DEFAULT_FOREGROUND_RGB};

const BASE_FONT_SIZE_LOGICAL_PX: f32 = 16.0;
const BASE_LINE_HEIGHT_LOGICAL_PX: f32 = 22.0;
const PADDING_LOGICAL_PX: f32 = 8.0;
const NARROW_SHAPING_CACHE_CAPACITY: usize = 1024;
const WIDE_SHAPING_CACHE_CAPACITY: usize = 256;
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
            "M",
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
        Ok(Self {
            cell_width_px: primary_advance_px.ceil(),
            cell_height_px: cell_height_px.ceil(),
            font_size_px,
            padding_px: (PADDING_LOGICAL_PX * scale).ceil(),
            scale_factor,
            ascii_baseline_px,
            primary_advance_px,
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

    pub fn dpi_milli(&self) -> NonZeroU32 {
        let value = (self.scale_factor * 1000.0)
            .round()
            .clamp(1.0, u32::MAX as f64);
        NonZeroU32::new(value as u32).expect("DPI scale is clamped above zero")
    }
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
pub fn compose_preedit(frame: &ViewportFrame, preedit: Option<&Preedit>) -> ComposedFrame {
    let Some(preedit) = preedit.filter(|preedit| !preedit.text.is_empty()) else {
        return ComposedFrame {
            frame: frame.clone(),
            ime_caret: frame.cursor,
        };
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
    ComposedFrame {
        frame: composed,
        ime_caret,
    }
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
    pub fn publish(&mut self, frame: ViewportFrame, trigger: FrameTrigger) {
        self.overwrites += u64::from(self.pending.replace((frame, trigger)).is_some());
    }

    pub fn take(&mut self) -> Option<(ViewportFrame, FrameTrigger)> {
        self.pending.take()
    }

    pub fn overwrites(&self) -> u64 {
        self.overwrites
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
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

struct TextRow {
    cells: Vec<CapturedCell>,
    narrow_glyphs: Vec<NarrowGlyph>,
    wide_glyphs: Vec<WideGlyph>,
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
    last_used: u64,
}

struct NarrowShapingCache {
    entries: HashMap<ShapeKey, CachedNarrowShape>,
    access_clock: u64,
    #[cfg(test)]
    color_emoji_trial_shapes: u64,
}

impl NarrowShapingCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            access_clock: 0,
            #[cfg(test)]
            color_emoji_trial_shapes: 0,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.access_clock = 0;
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
        self.access_clock = self.access_clock.saturating_add(1);
        let last_used = self.access_clock;
        if let Some(cached) = self.entries.get_mut(&key) {
            cached.last_used = last_used;
            return (
                Arc::clone(&cached.buffer),
                cached.left_offset_px,
                cached.top_offset_px,
            );
        }

        if self.entries.len() >= NARROW_SHAPING_CACHE_CAPACITY
            && let Some(lru_key) = self
                .entries
                .iter()
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&lru_key);
        }

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
            NarrowSizePolicy::FitCell => {
                let em_scale = cell_fitted_symbol_em_scale(
                    &buffer,
                    font_system,
                    swash_cache,
                    metrics.cell_width_px,
                    metrics.cell_height_px,
                );
                if (em_scale - 1.0).abs() > f32::EPSILON {
                    buffer = shape_narrow_buffer(&key, font_system, metrics, em_scale, family);
                }
                center_ink_offsets(
                    &buffer,
                    font_system,
                    swash_cache,
                    metrics.cell_width_px,
                    metrics.cell_height_px,
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
        self.entries.insert(
            key,
            CachedNarrowShape {
                buffer: Arc::clone(&buffer),
                left_offset_px,
                top_offset_px,
                last_used,
            },
        );
        (buffer, left_offset_px, top_offset_px)
    }
}

struct CachedWideShape {
    buffer: Arc<Buffer>,
    top_offset_px: f32,
    last_used: u64,
}

struct WideShapingCache {
    entries: HashMap<ShapeKey, CachedWideShape>,
    access_clock: u64,
    #[cfg(test)]
    color_emoji_trial_shapes: u64,
}

impl WideShapingCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            access_clock: 0,
            #[cfg(test)]
            color_emoji_trial_shapes: 0,
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.access_clock = 0;
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
        self.access_clock = self.access_clock.saturating_add(1);
        let last_used = self.access_clock;
        if let Some(cached) = self.entries.get_mut(&key) {
            cached.last_used = last_used;
            return (Arc::clone(&cached.buffer), cached.top_offset_px);
        }

        if self.entries.len() >= WIDE_SHAPING_CACHE_CAPACITY
            && let Some(lru_key) = self
                .entries
                .iter()
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&lru_key);
        }

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
        self.entries.insert(
            key,
            CachedWideShape {
                buffer: Arc::clone(&buffer),
                top_offset_px,
                last_used,
            },
        );
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
    configured_size: (u32, u32),
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    rect_pipeline: wgpu::RenderPipeline,
    metrics: CellMetrics,
    init_timings: RendererInitTimings,
    text_rows: Vec<TextRow>,
    narrow_shaping_cache: NarrowShapingCache,
    wide_shaping_cache: WideShapingCache,
    glyph_degraded_frames: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationGeometry {
    /// Physical pixel size requested by the current surface configuration.
    pub swapchain_size: (u32, u32),
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
        let swapchain_size = physical_client_size(width, height);
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
        let rect_pipeline = create_rect_pipeline(&device, config.format);
        let render_resources_time = phase_started.elapsed();
        Ok(Self {
            surface,
            device,
            queue,
            config,
            configured_size: swapchain_size,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            rect_pipeline,
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
            narrow_shaping_cache: NarrowShapingCache::new(),
            wide_shaping_cache: WideShapingCache::new(),
            glyph_degraded_frames: 0,
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn init_timings(&self) -> RendererInitTimings {
        self.init_timings
    }

    pub fn ime_cursor_area(&self, frame: &ViewportFrame) -> ImeCursorArea {
        ime_cursor_area_for_metrics(self.metrics, frame.cursor)
    }

    pub fn presentation_geometry(&self) -> PresentationGeometry {
        PresentationGeometry {
            swapchain_size: (self.config.width, self.config.height),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let swapchain_size = physical_client_size(width, height);
        self.config.width = swapchain_size.0;
        self.config.height = swapchain_size.1;
        Ok(())
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.metrics = CellMetrics::measure(&mut self.font_system, scale_factor)?;
        self.text_rows.clear();
        self.narrow_shaping_cache.clear();
        self.wide_shaping_cache.clear();
        Ok(self.metrics)
    }

    pub fn present(
        &mut self,
        frame: &ViewportFrame,
        trigger: FrameTrigger,
    ) -> Result<PresentOutcome, RenderError> {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        self.prepare_text_rows(frame);
        let padding = self.metrics.padding_px;
        let metrics = self.metrics;
        let text_right =
            (padding + frame.columns.get() as f32 * self.metrics.cell_width_px).ceil() as i32;
        let narrow_text_areas = self
            .text_rows
            .iter()
            .enumerate()
            .flat_map(|(row, text_row)| {
                text_row.narrow_glyphs.iter().map(move |glyph| {
                    let [left, top, _, bottom] = cell_bounds_px(metrics, row, glyph.column);
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
        let wide_text_areas = self
            .text_rows
            .iter()
            .enumerate()
            .flat_map(|(row, text_row)| {
                text_row.wide_glyphs.iter().map(move |wide| {
                    let [left, top, _, bottom] = cell_bounds_px(metrics, row, wide.column);
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
        let text_prepared = match self.text_renderer.prepare(
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            narrow_text_areas.chain(wide_text_areas),
            &mut self.swash_cache,
        ) {
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
            if text_prepared {
                self.text_renderer
                    .render(&self.atlas, &self.viewport, &mut pass)
                    .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
            }
        }
        self.queue.submit([encoder.finish()]);
        let submitted_at = Instant::now();
        self.queue.present(texture);
        let receipt = PresentReceipt {
            trigger,
            submitted_at,
            present_called_at: Instant::now(),
        };
        self.atlas.trim();
        Ok(PresentOutcome::Presented(receipt))
    }

    fn prepare_text_rows(&mut self, frame: &ViewportFrame) {
        let columns = frame.columns.get() as usize;
        let rows = frame.rows.get() as usize;
        let metrics = self.metrics;
        while self.text_rows.len() < rows {
            self.text_rows.push(TextRow {
                cells: Vec::new(),
                narrow_glyphs: Vec::new(),
                wide_glyphs: Vec::new(),
            });
        }
        self.text_rows.truncate(rows);

        for (row_index, row) in self.text_rows.iter_mut().enumerate() {
            let start = row_index * columns;
            let cells = &frame.cells[start..start + columns];
            if !row_needs_reshaping(&row.cells, cells) {
                continue;
            }

            row.narrow_glyphs = shape_narrow_glyphs(
                cells,
                &mut self.font_system,
                &mut self.swash_cache,
                metrics,
                &mut self.narrow_shaping_cache,
            );
            row.wide_glyphs = shape_wide_glyphs(
                cells,
                &mut self.font_system,
                &mut self.swash_cache,
                metrics,
                &mut self.wide_shaping_cache,
            );
            row.cells.clear();
            row.cells.extend_from_slice(cells);
        }
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
                rects.push(self.cell_rect(index / columns, index % columns, background));
            }
        }
        if frame.cursor.visible
            && frame.cursor.row < frame.rows.get()
            && frame.cursor.column < frame.columns.get()
        {
            let (column, span) = cursor_cell_span(frame);
            rects.push(self.cell_rect_span(
                frame.cursor.row as usize,
                column,
                span,
                DEFAULT_CURSOR_RGB,
            ));
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
            let [left, top, right, bottom] = cell_bounds_px(self.metrics, row, column);
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
                self.pixel_rect(rect.left, rect.top, rect.right, rect.bottom, foreground)
            }));
        }
        for (index, cell) in frame.cells.iter().enumerate() {
            if cell.style.flags.contains(CellFlags::UNDERLINE) {
                let row = index / columns;
                let column = index % columns;
                let [left, _, right, bottom] = cell_bounds_px(self.metrics, row, column);
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

    fn cell_rect(&self, row: usize, column: usize, color: [u8; 3]) -> RectInstance {
        let [left, top, right, bottom] = cell_bounds_px(self.metrics, row, column);
        self.pixel_rect(left, top, right, bottom, color)
    }

    fn cell_rect_span(
        &self,
        row: usize,
        column: usize,
        span: usize,
        color: [u8; 3],
    ) -> RectInstance {
        let [left, top, _, bottom] = cell_bounds_px(self.metrics, row, column);
        self.pixel_rect(
            left,
            top,
            left + span as f32 * self.metrics.cell_width_px,
            bottom,
            color,
        )
    }

    fn pixel_rect(
        &self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        color: [u8; 3],
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
            color: rect_gpu_color(color),
        }
    }
}

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

fn ime_cursor_area_for_metrics(
    metrics: CellMetrics,
    cursor: bt_viewport::GridCursor,
) -> ImeCursorArea {
    let [left, top, right, bottom] =
        cell_bounds_px(metrics, cursor.row as usize, cursor.column as usize);
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
                if key.text.chars().any(is_cell_fitted_text_symbol) {
                    NarrowSizePolicy::FitCell
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
    FitCell,
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

fn cell_fitted_symbol_em_scale(
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cell_width_px: f32,
    cell_height_px: f32,
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
    (cell_width_px / ink_width).min(cell_height_px / ink_height)
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

fn row_needs_reshaping(previous: &[CapturedCell], next: &[CapturedCell]) -> bool {
    previous != next
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

    // Keep bare media controls and geometric shapes on a stable monochrome face so their cell-fit
    // policy does not depend on primary-font coverage. An explicit VS16 above still requests color.
    if text.chars().any(is_cell_fitted_text_symbol) {
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

fn is_cell_fitted_text_symbol(character: char) -> bool {
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

fn rect_gpu_color(color: [u8; 3]) -> [f32; 4] {
    let [r, g, b] = srgb_rgb_to_linear(color);
    [r as f32, g as f32, b as f32, 1.0]
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
    })
}

fn default_foreground() -> [u8; 3] {
    DEFAULT_FOREGROUND_RGB
}

fn default_background() -> [u8; 3] {
    DEFAULT_BACKGROUND_RGB
}

fn physical_client_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
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
            assert!(!is_cell_fitted_text_symbol('☆'));

            assert!(procedural::supports_text("│"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn pinned_media_and_geometric_symbols_fit_and_center_in_one_cell() {
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
                    ink_width >= 0.8 * metrics.cell_width_px
                        || ink_height >= 0.8 * metrics.cell_height_px,
                    "scale {scale_factor}: {text} ink must visibly fill its cell"
                );
                assert!(
                    ink_width <= metrics.cell_width_px + 1.0
                        && ink_height <= metrics.cell_height_px + 1.0,
                    "scale {scale_factor}: {text} ink must remain inside one cell"
                );
                let centered_x = (left + right) / 2.0 + glyph.left_offset_px;
                let centered_y = (top + bottom) / 2.0 + glyph.top_offset_px;
                assert!((centered_x - metrics.cell_width_px / 2.0).abs() <= 0.5);
                assert!((centered_y - metrics.cell_height_px / 2.0).abs() <= 0.5);
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
            let area = ime_cursor_area_for_metrics(metrics, cursor);
            let bounds = cell_bounds_px(metrics, 2, 3);
            assert_eq!(area.x, bounds[0].floor() as i32);
            assert_eq!(area.y, bounds[1].floor() as i32);
            assert_eq!(area.width, (bounds[2].ceil() - bounds[0].floor()) as u32);
            assert_eq!(area.height, (bounds[3].ceil() - bounds[1].floor()) as u32);
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
            layout_key: bt_doc_layout_key(),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let mut slot = LatestFrameSlot::default();
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        slot.publish(frame.clone(), trigger);
        slot.publish(frame, trigger);
        assert_eq!(slot.overwrites(), 1);
        assert!(slot.take().is_some());
        assert!(slot.take().is_none());
    }

    fn bt_doc_layout_key() -> bt_doc::LayoutKey {
        bt_doc::LayoutKey {
            width_cells: NonZeroU32::new(1).unwrap(),
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
        assert_eq!(default_background(), [0x0c, 0x0c, 0x0c]);
        assert_eq!(default_foreground(), [0xcc, 0xcc, 0xcc]);
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
    }

    #[test]
    fn swapchain_size_is_always_exactly_the_physical_client_size() {
        for physical_client in [(960, 600), (1440, 900), (1920, 1200), (2560, 1440)] {
            assert_eq!(
                physical_client_size(physical_client.0, physical_client.1),
                physical_client
            );
        }
        assert_eq!(physical_client_size(0, 0), (1, 1));
        assert_ne!(physical_client_size(1920, 1200), (3840, 2160));
    }

    #[test]
    fn text_cache_reshapes_rows_instead_of_cells_and_reuses_unchanged_rows() {
        let columns = 80;
        let rows = 24;
        let frame = vec![CapturedCell::plain("x"); columns * rows];
        let mut cached = vec![Vec::new(); rows];

        let initial_changed = cached
            .iter()
            .enumerate()
            .filter(|(row, previous)| {
                let start = row * columns;
                row_needs_reshaping(previous, &frame[start..start + columns])
            })
            .count();
        assert_eq!(initial_changed, rows);
        assert_ne!(initial_changed, columns * rows);

        for (row, previous) in cached.iter_mut().enumerate() {
            previous.extend_from_slice(&frame[row * columns..(row + 1) * columns]);
        }
        assert_eq!(
            cached
                .iter()
                .enumerate()
                .filter(|(row, previous)| {
                    let start = row * columns;
                    row_needs_reshaping(previous, &frame[start..start + columns])
                })
                .count(),
            0
        );

        let mut changed = frame;
        changed[3 * columns + 7].text = "y".to_owned();
        assert_eq!(
            cached
                .iter()
                .enumerate()
                .filter(|(row, previous)| {
                    let start = row * columns;
                    row_needs_reshaping(previous, &changed[start..start + columns])
                })
                .count(),
            1
        );
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
            layout_key: bt_doc_layout_key(),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let composed = compose_preedit(
            &frame,
            Some(&Preedit {
                text: "nihao".to_owned(),
                cursor_byte: Some(2),
            }),
        );

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
            layout_key: bt_doc_layout_key(),
            view_generation: bt_doc::ViewGeneration(1),
        };
        let text = "👨‍👩‍👧‍👦☆中";
        let composed = compose_preedit(
            &frame,
            Some(&Preedit {
                text: text.to_owned(),
                cursor_byte: Some(text.len()),
            }),
        );

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
            layout_key: bt_doc_layout_key(),
            view_generation: bt_doc::ViewGeneration(1),
        };
        assert_eq!(cursor_cell_span(&frame), (0, 2));
        frame.cursor.column = 1;
        assert_eq!(cursor_cell_span(&frame), (0, 2));
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
