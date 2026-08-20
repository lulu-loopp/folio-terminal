//! The layer above the web: a wgpu swapchain on our own DirectComposition
//! visual, configured `PreMultiplied`, painting rectangles and — where the seat
//! is — painting nothing at all.
//!
//! Nothing here is clever. It exists so that gate 1 can put a real, alpha-blended
//! surface over a real web page and photograph the result, using the same
//! surface-creation door `bt_render::create_surface` uses.

use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use std::ffi::c_void;
use wgpu::util::DeviceExt as _;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Viewport {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// One rectangle to paint, in physical pixels, with straight (not premultiplied)
/// alpha — the premultiply happens here so callers state colours the way a
/// designer does.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

pub struct Overlay {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    viewport: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// What the adapter offered and what we took — the two lines the 2026-08-13
    /// spike printed, reprinted here so gate 1 can compare them.
    pub alpha_offered: Vec<String>,
    pub alpha_chosen: String,
}

impl Overlay {
    /// `visual` is a borrowed `IDCompositionVisual`. wgpu takes its own COM
    /// reference and holds it for the surface's life.
    pub fn new(visual: *mut c_void, width: u32, height: u32) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(visual))
        }
        .context("create_surface_unsafe(CompositionVisual)")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .context("request_adapter")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("w0-overlay"),
            ..Default::default()
        }))
        .context("request_device")?;

        let capabilities = surface.get_capabilities(&adapter);
        let alpha_offered: Vec<String> = capabilities
            .alpha_modes
            .iter()
            .map(|mode| format!("{mode:?}"))
            .collect();
        // The product's rule, restated: a visual target that cannot be
        // PreMultiplied cannot have a hole cut in it, and there is no second
        // choice to fall back to.
        if !capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            anyhow::bail!("visual target did not offer PreMultiplied: {alpha_offered:?}");
        }
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("w0-rects"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let viewport = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("w0-viewport"),
            contents: bytemuck::bytes_of(&Viewport {
                size: [config.width as f32, config.height as f32],
                _pad: [0.0; 2],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("w0-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("w0-bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("w0-pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("w0-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied source over: what the swapchain's alpha mode
                    // says the pixels already are.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            viewport,
            bind_group,
            alpha_offered,
            alpha_chosen: format!("{:?}", wgpu::CompositeAlphaMode::PreMultiplied),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.queue.write_buffer(
            &self.viewport,
            0,
            bytemuck::bytes_of(&Viewport {
                size: [self.config.width as f32, self.config.height as f32],
                _pad: [0.0; 2],
            }),
        );
    }

    /// Paint one frame. The whole surface is cleared to transparent first, so
    /// every pixel no rectangle covers is a hole through to whatever is below —
    /// which for the seat is the web page.
    pub fn draw(&mut self, rects: &[Rect]) -> Result<()> {
        let instances: Vec<Instance> = rects
            .iter()
            .map(|rect| Instance {
                rect: [rect.x, rect.y, rect.width, rect.height],
                color: [
                    rect.color[0] * rect.color[3],
                    rect.color[1] * rect.color[3],
                    rect.color[2] * rect.color[3],
                    rect.color[3],
                ],
            })
            .collect();
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("w0-instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            other => anyhow::bail!("get_current_texture: {other:?}"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("w0-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !instances.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..instances.len() as u32);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }
}

const SHADER: &str = r#"
struct Viewport { size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> viewport: Viewport;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let corner = corners[index];
    let pixel = rect.xy + corner * rect.zw;
    let ndc = vec2<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
    );
    var out: VsOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
