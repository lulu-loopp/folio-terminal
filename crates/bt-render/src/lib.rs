//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

use std::{
    num::{NonZeroI64, NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use bt_transcript::{CapturedCell, CellFlags, CellStyle, TerminalColor};
use bt_viewport::{SUBPIXELS_PER_PX, ViewportFrame};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, AttrsOwned, Buffer, Cache, Color, Family, FontSystem, Metrics, PrepareError, Resolution,
    Shaping, Style, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
    Wrap,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [9, 11, 14];

const BASE_FONT_SIZE_LOGICAL_PX: f32 = 16.0;
const BASE_LINE_HEIGHT_LOGICAL_PX: f32 = 22.0;
const PADDING_LOGICAL_PX: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub cell_width_px: f32,
    pub cell_height_px: f32,
    pub font_size_px: f32,
    pub padding_px: f32,
    pub scale_factor: f64,
    glyph_advance_px: f32,
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
        Ok(Self {
            cell_width_px: line.w.max(1.0).ceil(),
            cell_height_px: cell_height_px.ceil(),
            font_size_px,
            padding_px: (PADDING_LOGICAL_PX * scale).ceil(),
            scale_factor,
            glyph_advance_px: line.w.max(1.0),
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
    #[error("native presentation setup failed: {0}")]
    NativePresentation(String),
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
    buffer: Buffer,
    has_visible_text: bool,
}

struct StyledRun {
    text: String,
    attrs: AttrsOwned,
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
    source_size: (u32, u32),
    dxgi_presentation: bt_platform::DxgiPresentationState,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    rect_pipeline: wgpu::RenderPipeline,
    metrics: CellMetrics,
    init_timings: RendererInitTimings,
    text_rows: Vec<TextRow>,
    glyph_degraded_frames: u64,
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
        capacity_width: u32,
        capacity_height: u32,
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
        let allocation_width = capacity_width.max(width).max(1);
        let allocation_height = capacity_height.max(height).max(1);
        let source_size = (width.max(1), height.max(1));
        let mut config = surface
            .get_default_config(&adapter, allocation_width, allocation_height)
            .ok_or_else(|| RenderError::Wgpu("surface has no default configuration".to_owned()))?;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);
        let dxgi_presentation =
            bt_platform::configure_dxgi_presentation(&surface, DEFAULT_BACKGROUND_RGB, source_size)
                .map_err(RenderError::NativePresentation)?;
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
            configured_size: (allocation_width, allocation_height),
            source_size,
            dxgi_presentation,
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
            glyph_degraded_frames: 0,
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn init_timings(&self) -> RendererInitTimings {
        self.init_timings
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.source_size = (width, height);
        self.config.width = surface_capacity_for_request(self.config.width, width);
        self.config.height = surface_capacity_for_request(self.config.height, height);
        if self.configured_size == (self.config.width, self.config.height) {
            bt_platform::set_dxgi_source_size(&self.surface, width, height)
                .map_err(RenderError::NativePresentation)?;
            self.dxgi_presentation.source_size = (width, height);
        }
        Ok(())
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.metrics = CellMetrics::measure(&mut self.font_system, scale_factor)?;
        self.text_rows.clear();
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
        let cell_height = self.metrics.cell_height_px;
        let text_right =
            (padding + frame.columns.get() as f32 * self.metrics.cell_width_px).ceil() as i32;
        let text_areas = self
            .text_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.has_visible_text)
            .map(|(index, row)| {
                let [left, top, _, _] = cell_bounds_px(self.metrics, index, 0);
                TextArea {
                    buffer: &row.buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: padding.floor() as i32,
                        top: top.floor() as i32,
                        right: text_right,
                        bottom: (top + cell_height).ceil() as i32,
                    },
                    default_color: Color::rgb(218, 222, 230),
                    custom_glyphs: &[],
                }
            });
        let text_prepared = match self.text_renderer.prepare(
            &self.device,
            &self.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
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
                        // M0 uses the same channel/255 convention as the rectangle shader. This
                        // intentionally has no explicit sRGB conversion yet; color management is
                        // deferred, but default-background cells and the clear are numerically one
                        // theme color instead of relying on rounded decimal coincidence.
                        load: wgpu::LoadOp::Clear(theme_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
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
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(metrics.font_size_px, metrics.cell_height_px),
            );
            buffer.set_wrap(Wrap::None);
            self.text_rows.push(TextRow {
                cells: Vec::new(),
                buffer,
                has_visible_text: false,
            });
        }
        self.text_rows.truncate(rows);

        for (row_index, row) in self.text_rows.iter_mut().enumerate() {
            let start = row_index * columns;
            let cells = &frame.cells[start..start + columns];
            if !row_needs_reshaping(&row.cells, cells) {
                continue;
            }

            let runs = styled_runs(cells, metrics);
            row.has_visible_text = !runs.is_empty();
            reshape_text_row(
                &mut row.buffer,
                &mut self.font_system,
                &runs,
                metrics,
                columns,
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
        self.dxgi_presentation = bt_platform::configure_dxgi_presentation(
            &self.surface,
            DEFAULT_BACKGROUND_RGB,
            self.source_size,
        )
        .map_err(RenderError::NativePresentation)?;
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
            rects.push(self.cell_rect(
                frame.cursor.row as usize,
                frame.cursor.column as usize,
                [180, 190, 205],
            ));
        }
        rects
    }

    fn cell_rect(&self, row: usize, column: usize, color: [u8; 3]) -> RectInstance {
        let [left, top, right, bottom] = cell_bounds_px(self.metrics, row, column);
        let width = self.config.width.max(1) as f32;
        let height = self.config.height.max(1) as f32;
        RectInstance {
            rect: [
                left / width * 2.0 - 1.0,
                1.0 - top / height * 2.0,
                right / width * 2.0 - 1.0,
                1.0 - bottom / height * 2.0,
            ],
            color: [
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
                1.0,
            ],
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

#[cfg(target_os = "windows")]
fn terminal_font_system() -> FontSystem {
    // M0 renders ASCII with the Windows terminal monospace family. Loading these four bounded
    // files avoids cosmic-text's eager scan and parse of every installed system font during the
    // launch critical path. Broader fallback belongs with the later width/IME work.
    let windows = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    let fonts = std::path::PathBuf::from(windows).join("Fonts");
    let mut db = glyphon::fontdb::Database::new();
    for file in [
        "consola.ttf",
        "consolab.ttf",
        "consolai.ttf",
        "consolaz.ttf",
    ] {
        let _ = db.load_font_file(fonts.join(file));
    }
    if db.is_empty() {
        return FontSystem::new();
    }
    db.set_monospace_family("Consolas");
    FontSystem::new_with_locale_and_db("en-US".to_owned(), db)
}

#[cfg(not(target_os = "windows"))]
fn terminal_font_system() -> FontSystem {
    FontSystem::new()
}

fn styled_runs(cells: &[CapturedCell], metrics: CellMetrics) -> Vec<StyledRun> {
    let Some(last_visible) = cells.iter().rposition(|cell| {
        !cell.wide_spacer
            && !cell.style.flags.contains(CellFlags::HIDDEN)
            && !cell.text.is_empty()
            && !cell.text.chars().all(char::is_whitespace)
    }) else {
        return Vec::new();
    };

    let mut runs: Vec<StyledRun> = Vec::new();
    for cell in &cells[..=last_visible] {
        if cell.wide_spacer {
            continue;
        }
        let text = if cell.style.flags.contains(CellFlags::HIDDEN)
            || cell.text.is_empty()
            || cell.text.chars().all(char::is_whitespace)
        {
            if cell.style.flags.contains(CellFlags::WIDE_CHAR) {
                "  "
            } else {
                " "
            }
        } else {
            cell.text.as_str()
        };
        let attrs = text_attrs(&cell.style, metrics);
        if let Some(run) = runs.last_mut()
            && run.attrs == attrs
        {
            run.text.push_str(text);
        } else {
            runs.push(StyledRun {
                text: text.to_owned(),
                attrs,
            });
        }
    }
    runs
}

fn row_needs_reshaping(previous: &[CapturedCell], next: &[CapturedCell]) -> bool {
    previous != next
}

fn reshape_text_row(
    buffer: &mut Buffer,
    font_system: &mut FontSystem,
    runs: &[StyledRun],
    metrics: CellMetrics,
    columns: usize,
) {
    buffer.set_size(
        Some(metrics.cell_width_px * columns as f32),
        Some(metrics.cell_height_px),
    );
    buffer.set_monospace_width(Some(metrics.cell_width_px));
    let default_attrs = Attrs::new().family(Family::Monospace);
    if runs.is_empty() {
        buffer.set_text("", &default_attrs, Shaping::Advanced, None);
    } else {
        buffer.set_rich_text(
            runs.iter()
                .map(|run| (run.text.as_str(), run.attrs.as_attrs())),
            &default_attrs,
            Shaping::Advanced,
            None,
        );
    }
    buffer.shape_until_scroll(font_system, false);
}

fn text_attrs(style: &CellStyle, metrics: CellMetrics) -> AttrsOwned {
    let (foreground, _) = resolve_colors(style);
    let tracking_em = (metrics.cell_width_px - metrics.glyph_advance_px) / metrics.font_size_px;
    let mut attrs = Attrs::new()
        .family(Family::Monospace)
        // cosmic-text's monospace_width normalizes fallback font size but does not quantize the
        // primary font's glyph advances. Track the measured advance up to the integer terminal
        // cell width so long rows cannot accumulate a high-DPI cursor drift.
        .letter_spacing(tracking_em)
        .color(Color::rgb(foreground[0], foreground[1], foreground[2]));
    if style.flags.contains(CellFlags::BOLD) {
        attrs = attrs.weight(Weight::BOLD);
    }
    if style.flags.contains(CellFlags::ITALIC) {
        attrs = attrs.style(Style::Italic);
    }
    AttrsOwned::new(&attrs)
}

fn theme_clear_color() -> wgpu::Color {
    let [r, g, b] = default_background();
    wgpu::Color {
        r: f64::from(r) / 255.0,
        g: f64::from(g) / 255.0,
        b: f64::from(b) / 255.0,
        a: 1.0,
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
    [218, 222, 230]
}

fn default_background() -> [u8; 3] {
    DEFAULT_BACKGROUND_RGB
}

fn surface_capacity_for_request(current: u32, requested: u32) -> u32 {
    if requested <= current {
        current
    } else {
        requested.max(current.saturating_mul(3).saturating_div(2))
    }
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
        TerminalColor::Named(18) => [180, 190, 205],
        TerminalColor::Named(28) => [145, 148, 153],
        TerminalColor::Named(code @ 19..=26) => {
            indexed_color(code - 19).map(|channel| channel.saturating_mul(2) / 3)
        }
        TerminalColor::Named(code) => indexed_color(code.min(15)),
    }
}

fn indexed_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];
    if index < 16 {
        return ANSI[index as usize];
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
    fn grid_dimensions_are_nonzero_and_derived_from_metrics() {
        let metrics = CellMetrics {
            cell_width_px: 10.0,
            cell_height_px: 20.0,
            font_size_px: 16.0,
            padding_px: 5.0,
            scale_factor: 1.0,
            glyph_advance_px: 10.0,
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
    fn surface_clear_is_exactly_the_default_cell_background() {
        let clear = theme_clear_color();
        let [r, g, b] = default_background();
        assert_eq!(clear.r, f64::from(r) / 255.0);
        assert_eq!(clear.g, f64::from(g) / 255.0);
        assert_eq!(clear.b, f64::from(b) / 255.0);
        assert_eq!(clear.a, 1.0);
    }

    #[test]
    fn surface_capacity_grows_geometrically_only_after_source_exceeds_it() {
        assert_eq!(surface_capacity_for_request(1920, 1919), 1920);
        assert_eq!(surface_capacity_for_request(1920, 2000), 2880);
        assert_eq!(surface_capacity_for_request(1920, 4000), 4000);
        assert_eq!(surface_capacity_for_request(u32::MAX, u32::MAX), u32::MAX);
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
    fn row_runs_preserve_blank_columns_and_style_boundaries() {
        let mut red = CapturedCell::plain("A");
        red.style.foreground = TerminalColor::Rgb(255, 0, 0);
        let cells = [
            red,
            CapturedCell::plain(""),
            CapturedCell::plain("B"),
            CapturedCell::plain(" "),
        ];
        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let runs = styled_runs(&cells, metrics);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "A");
        assert_eq!(runs[1].text, " B");
    }

    #[test]
    fn shaped_ascii_glyphs_stay_on_integer_cell_columns() {
        const COLUMNS: usize = 80;
        const X_TOLERANCE: f32 = 0.0001;

        let mut font_system = terminal_font_system();
        let metrics = CellMetrics::measure(&mut font_system, 1.0).unwrap();
        let cells = vec![CapturedCell::plain("M"); COLUMNS];
        let runs = styled_runs(&cells, metrics);
        let mut buffer = Buffer::new(
            &mut font_system,
            Metrics::new(metrics.font_size_px, metrics.cell_height_px),
        );
        buffer.set_wrap(Wrap::None);

        reshape_text_row(&mut buffer, &mut font_system, &runs, metrics, COLUMNS);

        let layout_runs = buffer.layout_runs().collect::<Vec<_>>();
        assert_eq!(layout_runs.len(), 1);
        assert_eq!(layout_runs[0].glyphs.len(), COLUMNS);
        for (column, glyph) in layout_runs[0].glyphs.iter().enumerate() {
            let expected_x = column as f32 * metrics.cell_width_px;
            assert!(
                (glyph.x - expected_x).abs() <= X_TOLERANCE,
                "column {column}: glyph x={} but cell-grid x={expected_x}",
                glyph.x
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
            let runs = styled_runs(&cells, metrics);
            let mut buffer = Buffer::new(
                &mut font_system,
                Metrics::new(metrics.font_size_px, metrics.cell_height_px),
            );
            buffer.set_wrap(Wrap::None);
            reshape_text_row(&mut buffer, &mut font_system, &runs, metrics, cells.len());

            let glyphs = buffer
                .layout_runs()
                .flat_map(|run| run.glyphs.iter())
                .collect::<Vec<_>>();
            assert_eq!(glyphs.len(), cells.len());
            for (column, glyph) in glyphs.into_iter().enumerate() {
                assert_eq!(
                    glyph.x,
                    column as f32 * metrics.cell_width_px,
                    "scale factor {scale_factor}, column {column}"
                );
            }

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
            glyph_advance_px: 10.0,
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
