//! wgpu + cosmic-text rendering for viewport-owned terminal frames.

use std::{
    num::{NonZeroI64, NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use bt_transcript::{CellFlags, CellStyle, TerminalColor};
use bt_viewport::{SUBPIXELS_PER_PX, ViewportFrame};
use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, Style,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, Wrap,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

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
    #[error("glyph atlas preparation failed: {0}")]
    GlyphPrepare(String),
    #[error("glyph rendering failed: {0}")]
    GlyphRender(String),
    #[error("no usable monospace font metrics were produced")]
    MissingMonospaceMetrics,
    #[error("surface was lost and must be recreated")]
    SurfaceLost,
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

struct TextPlacement {
    left: f32,
    top: f32,
    bounds: TextBounds,
    color: Color,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    rect_pipeline: wgpu::RenderPipeline,
    metrics: CellMetrics,
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
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("BetterTerminal device"),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::Wgpu(error.to_string()))?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| RenderError::Wgpu("surface has no default configuration".to_owned()))?;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&device, &config);

        let mut font_system = FontSystem::new();
        let metrics = CellMetrics::measure(&mut font_system, scale_factor)?;
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);
        let rect_pipeline = create_rect_pipeline(&device, config.format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            rect_pipeline,
            metrics,
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn update_scale_factor(&mut self, scale_factor: f64) -> Result<CellMetrics, RenderError> {
        self.metrics = CellMetrics::measure(&mut self.font_system, scale_factor)?;
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
        let (mut buffers, placements) = self.text_buffers(frame);
        let text_areas = buffers
            .iter_mut()
            .zip(&placements)
            .map(|(buffer, placement)| TextArea {
                buffer,
                left: placement.left,
                top: placement.top,
                scale: 1.0,
                bounds: placement.bounds,
                default_color: placement.color,
                custom_glyphs: &[],
            })
            .collect::<Vec<_>>();
        self.text_renderer
            .prepare(
                &self.device,
                &self.queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|error| RenderError::GlyphPrepare(error.to_string()))?;

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
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                self.surface.configure(&self.device, &self.config);
                texture
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(PresentOutcome::Skipped);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(PresentOutcome::Reconfigure);
            }
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::SurfaceLost),
            wgpu::CurrentSurfaceTexture::Validation => return Err(RenderError::SurfaceValidation),
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
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.035,
                            g: 0.043,
                            b: 0.055,
                            a: 1.0,
                        }),
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
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|error| RenderError::GlyphRender(error.to_string()))?;
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

    fn text_buffers(&mut self, frame: &ViewportFrame) -> (Vec<Buffer>, Vec<TextPlacement>) {
        let columns = frame.columns.get() as usize;
        let mut buffers = Vec::new();
        let mut placements = Vec::new();
        for (index, cell) in frame.cells.iter().enumerate() {
            if cell.wide_spacer
                || cell.text.is_empty()
                || cell.text.chars().all(char::is_whitespace)
                || cell.style.flags.contains(CellFlags::HIDDEN)
            {
                continue;
            }
            let row = index / columns;
            let column = index % columns;
            let slot_cells = if cell.style.flags.contains(CellFlags::WIDE_CHAR) {
                2.0
            } else {
                1.0
            };
            let (foreground, _) = resolve_colors(&cell.style);
            let color = Color::rgb(foreground[0], foreground[1], foreground[2]);
            let mut attrs = Attrs::new().family(Family::Monospace).color(color);
            if cell.style.flags.contains(CellFlags::BOLD) {
                attrs = attrs.weight(Weight::BOLD);
            }
            if cell.style.flags.contains(CellFlags::ITALIC) {
                attrs = attrs.style(Style::Italic);
            }
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(self.metrics.font_size_px, self.metrics.cell_height_px),
            );
            buffer.set_wrap(Wrap::None);
            buffer.set_size(
                Some(self.metrics.cell_width_px * slot_cells),
                Some(self.metrics.cell_height_px),
            );
            buffer.set_text(&cell.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            let left = self.metrics.padding_px + column as f32 * self.metrics.cell_width_px;
            let top = self.metrics.padding_px + row as f32 * self.metrics.cell_height_px;
            placements.push(TextPlacement {
                left,
                top,
                bounds: TextBounds {
                    left: left.floor() as i32,
                    top: top.floor() as i32,
                    right: (left + self.metrics.cell_width_px * slot_cells).ceil() as i32,
                    bottom: (top + self.metrics.cell_height_px).ceil() as i32,
                },
                color,
            });
            buffers.push(buffer);
        }
        (buffers, placements)
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
        let left = self.metrics.padding_px + column as f32 * self.metrics.cell_width_px;
        let top = self.metrics.padding_px + row as f32 * self.metrics.cell_height_px;
        let right = left + self.metrics.cell_width_px;
        let bottom = top + self.metrics.cell_height_px;
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
    [9, 11, 14]
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
}
