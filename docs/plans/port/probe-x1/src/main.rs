//! X-1 — Metal alpha and composition, against the locked wgpu.
//!
//! A scratch probe, outside Folio's workspace. It asks one question: can the
//! Windows compositing model — one frame drawn with a hole in it, a web view
//! behind the hole — be built on macOS through a `CAMetalLayer` that wgpu-hal
//! 30.0.0 will only ever offer `Opaque` or `PostMultiplied`?
//!
//! The view hierarchy is the point. winit's content view gets two subviews: a
//! `WKWebView` first, and a plain `NSView` second. Later subviews are in front,
//! so the web view's layer is composited under the layer wgpu attaches to the
//! second view, and a transparent pixel in the Metal frame is a pixel of the
//! page.
//!
//! Everything is measured from the window server's own composite, read back
//! with `CGWindowListCreateImage` over this process's own window — the Mac
//! refuses `screencapture` to a session with no Screen Recording grant.

use std::ffi::c_void;
use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSScreen, NSView};
use objc2_foundation::{NSString, NSURL};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------- logging ---

static LOG: OnceLock<Mutex<File>> = OnceLock::new();

fn out_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    let dir = PathBuf::from(home).join("folio-port/logs/x1");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn logfile() -> &'static Mutex<File> {
    LOG.get_or_init(|| {
        let path = out_dir().join("probe-x1-run.log");
        Mutex::new(File::create(path).expect("probe log"))
    })
}

macro_rules! say {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        eprintln!("{line}");
        if let Ok(mut f) = logfile().lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }};
}

// ------------------------------------------------------- CoreGraphics FFI ---

#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CgSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    origin: CgPoint,
    size: CgSize,
}

const CG_RECT_NULL: CgRect = CgRect {
    origin: CgPoint {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
    size: CgSize {
        width: 0.0,
        height: 0.0,
    },
};

const K_LIST_INCLUDING_WINDOW: u32 = 1 << 3;
const K_IMAGE_BOUNDS_IGNORE_FRAMING: u32 = 1 << 0;
const K_IMAGE_BEST_RESOLUTION: u32 = 1 << 3;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCreateImage(
        screen_bounds: CgRect,
        list_option: u32,
        window_id: u32,
        image_option: u32,
    ) -> *mut c_void;
    fn CGImageGetWidth(image: *mut c_void) -> usize;
    fn CGImageGetHeight(image: *mut c_void) -> usize;
    fn CGImageGetBytesPerRow(image: *mut c_void) -> usize;
    fn CGImageGetBitsPerPixel(image: *mut c_void) -> usize;
    fn CGImageGetBitmapInfo(image: *mut c_void) -> u32;
    fn CGImageGetDataProvider(image: *mut c_void) -> *mut c_void;
    fn CGDataProviderCopyData(provider: *mut c_void) -> *mut c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetBytePtr(data: *mut c_void) -> *const u8;
    fn CFDataGetLength(data: *mut c_void) -> isize;
    fn CFRelease(cf: *mut c_void);
}

/// One composited window, as the window server has it.
struct Shot {
    width: usize,
    height: usize,
    /// RGBA8, row-major, top row first.
    pixels: Vec<u8>,
}

impl Shot {
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let i = (y * self.width + x) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    fn write_png(&self, path: &std::path::Path) {
        let file = File::create(path).expect("png file");
        let mut enc = png::Encoder::new(file, self.width as u32, self.height as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(&self.pixels).expect("png data");
    }
}

/// Read this process's own window back out of the window server.
///
/// Without a Screen Recording grant macOS excludes every other process's window
/// from a window-list image; a process's own windows are still returned, which
/// is exactly and only what this probe needs.
fn capture_window(window_number: u32) -> Option<Shot> {
    let image = unsafe {
        CGWindowListCreateImage(
            CG_RECT_NULL,
            K_LIST_INCLUDING_WINDOW,
            window_number,
            K_IMAGE_BOUNDS_IGNORE_FRAMING | K_IMAGE_BEST_RESOLUTION,
        )
    };
    if image.is_null() {
        say!("  capture: CGWindowListCreateImage returned NULL");
        return None;
    }
    let (w, h, stride, bpp, info) = unsafe {
        (
            CGImageGetWidth(image),
            CGImageGetHeight(image),
            CGImageGetBytesPerRow(image),
            CGImageGetBitsPerPixel(image),
            CGImageGetBitmapInfo(image),
        )
    };
    let provider = unsafe { CGImageGetDataProvider(image) };
    if provider.is_null() {
        unsafe { CFRelease(image) };
        say!("  capture: image has no data provider");
        return None;
    }
    let data = unsafe { CGDataProviderCopyData(provider) };
    if data.is_null() {
        unsafe { CFRelease(image) };
        say!("  capture: could not copy the image data");
        return None;
    }
    let len = unsafe { CFDataGetLength(data) } as usize;
    let ptr = unsafe { CFDataGetBytePtr(data) };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    say!("  capture: {w}x{h}, stride {stride}, {bpp} bpp, bitmapInfo 0x{info:08x}, {len} bytes");

    // 32 bits per pixel, little-endian, alpha first: the bytes are B, G, R, A.
    let mut pixels = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let s = y * stride + x * 4;
            let d = (y * w + x) * 4;
            pixels[d] = bytes[s + 2];
            pixels[d + 1] = bytes[s + 1];
            pixels[d + 2] = bytes[s];
            pixels[d + 3] = bytes[s + 3];
        }
    }
    unsafe {
        CFRelease(data);
        CFRelease(image);
    }
    Some(Shot {
        width: w,
        height: h,
        pixels,
    })
}

// ------------------------------------------------------------- the figure ---

/// Where the three test rectangles sit, in drawable (physical) pixels.
#[derive(Clone, Copy)]
struct Figure {
    /// Hole with a straight-alpha red edge: rgb 255,0,0 at alpha 0.5.
    a: [f32; 4],
    /// Hole with a premultiplied-style red edge: rgb 128,0,0 at alpha 0.5.
    b: [f32; 4],
    /// Hole whose interior is alpha 0 but blue: invisible if the compositor
    /// reads straight alpha, an additive blue wash if it reads premultiplied.
    c: [f32; 4],
}

fn figure(w: u32, h: u32) -> Figure {
    let (wf, hf) = (w as f32, h as f32);
    let y0 = (hf * 0.30).floor();
    let y1 = (hf * 0.65).floor();
    let band = |x0: f32, x1: f32| [(wf * x0).floor(), y0, (wf * x1).floor(), y1];
    Figure {
        a: band(0.06, 0.30),
        b: band(0.38, 0.62),
        c: band(0.70, 0.94),
    }
}

const SHADER: &str = r#"
struct U {
  size: vec4<f32>,
  ra:   vec4<f32>,
  rb:   vec4<f32>,
  rc:   vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  return vec4<f32>(p[i], 0.0, 1.0);
}

// The surface format is sRGB, so the value written here is linear and the
// hardware encodes it. Every colour in this shader is named as the byte we want
// to land in the framebuffer and converted back to linear here, so that a
// measurement can be compared against an arithmetic prediction.
fn enc(c: f32) -> f32 {
  if (c <= 0.04045) { return c / 12.92; }
  return pow((c + 0.055) / 1.055, 2.4);
}
fn rgb(r: f32, g: f32, b: f32) -> vec3<f32> {
  return vec3<f32>(enc(r), enc(g), enc(b));
}

// 0 outside, 1 on the one-pixel edge, 2 inside.
fn band(p: vec2<f32>, r: vec4<f32>) -> i32 {
  if (p.x < r.x || p.x > r.z || p.y < r.y || p.y > r.w) { return 0; }
  if (p.x < r.x + 1.0 || p.x > r.z - 1.0 || p.y < r.y + 1.0 || p.y > r.w - 1.0) { return 1; }
  return 2;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let p = floor(pos.xy);

  // Two opaque calibration patches, top left: pure red, and byte 128 grey.
  if (p.y < 40.0) {
    if (p.x < 40.0) { return vec4<f32>(rgb(1.0, 0.0, 0.0), 1.0); }
    if (p.x < 80.0) { return vec4<f32>(rgb(0.502, 0.502, 0.502), 1.0); }
  }

  let a = band(p, u.ra);
  if (a == 1) { return vec4<f32>(rgb(1.0, 0.0, 0.0), 0.502); }
  if (a == 2) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

  let b = band(p, u.rb);
  if (b == 1) { return vec4<f32>(rgb(0.502, 0.0, 0.0), 0.502); }
  if (b == 2) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

  let c = band(p, u.rc);
  if (c == 1) { return vec4<f32>(rgb(1.0, 0.0, 0.0), 0.502); }
  if (c == 2) { return vec4<f32>(rgb(0.0, 0.0, 1.0), 0.0); }

  return vec4<f32>(rgb(0.055, 0.098, 0.216), 1.0);
}
"#;

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>x1 backdrop</title><style>
 html,body{margin:0;padding:0;height:100%;background:#000;overflow:hidden}
 .band{position:absolute;left:0;right:0;top:15%;height:70%;background:#00ff00}
 .top{position:absolute;left:0;right:0;top:0;height:15%;display:flex}
 .top i{flex:1;display:block}
 .bot{position:absolute;left:0;right:0;bottom:0;height:15%;
      background:repeating-linear-gradient(45deg,#ffffff 0 24px,#001a66 24px 48px)}
 .label{position:absolute;top:17%;left:3%;font:700 26px -apple-system,Helvetica,sans-serif;color:#004d00}
</style></head><body>
 <div class="band"></div>
 <div class="top"><i style="background:#ff0000"></i><i style="background:#ff8800"></i>
  <i style="background:#ffee00"></i><i style="background:#00ccff"></i><i style="background:#cc00ff"></i></div>
 <div class="bot"></div>
 <div class="label">WKWebView, behind the Metal layer</div>
</body></html>
"#;

// -------------------------------------------------------------- the probe ---

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    format: wgpu::TextureFormat,
}

struct State {
    window: Window,
    host: Retained<NSView>,
    _webview: Retained<WKWebView>,
    window_number: u32,
    gpu: Gpu,
    surface: Option<wgpu::Surface<'static>>,
    size: PhysicalSize<u32>,
}

struct App {
    state: Option<State>,
    t0: Instant,
    step: usize,
    verdicts: Vec<String>,
}

/// The autonomous timeline. Nobody is at the machine, so the probe drives
/// itself; the same actions are on the keys named beside them.
const STEPS: &[(f32, &str)] = &[
    (2.0, "render"),
    (3.4, "capture:01-initial"),
    (4.2, "resize"),
    (5.8, "render"),
    (6.8, "capture:02-resized"),
    (7.6, "lose-surface"),
    (8.8, "render"),
    (9.8, "capture:03-reconstructed"),
    (10.6, "lose-surface-owned"),
    (11.8, "render"),
    (12.8, "capture:04-reconstructed-owned"),
    (13.8, "finish"),
];

fn main() {
    say!("probe-x1 start, pid {}", std::process::id());
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        state: None,
        t0: Instant::now(),
        step: 0,
        verdicts: Vec::new(),
    };
    event_loop.run_app(&mut app).expect("run");
    say!("PROBE_X1_DONE");
}

impl App {
    fn build(&mut self, el: &ActiveEventLoop) {
        let mtm = MainThreadMarker::new().expect("main thread");

        // The 4K panel presents at backing scale 2; the other display on this
        // machine does not, and a scale-1 window would not answer the ticket.
        let target = el
            .available_monitors()
            .find(|m| (m.scale_factor() - 2.0).abs() < 0.01);
        for m in el.available_monitors() {
            say!(
                "monitor {:?}: scale {}, size {:?}, position {:?}",
                m.name(),
                m.scale_factor(),
                m.size(),
                m.position()
            );
        }

        let mut attrs = Window::default_attributes()
            .with_title("Folio X-1 probe")
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size(PhysicalSize::new(1800u32, 1200u32));
        if let Some(m) = &target {
            let p = m.position();
            attrs = attrs.with_position(winit::dpi::PhysicalPosition::new(p.x + 160, p.y + 160));
        }
        let window = el.create_window(attrs).expect("window");
        window.focus_window();
        say!(
            "window: scale {}, inner {:?}",
            window.scale_factor(),
            window.inner_size()
        );

        // --- the view hierarchy: web view first, Metal host above it.
        let handle = window.window_handle().expect("handle").as_raw();
        let RawWindowHandle::AppKit(appkit) = handle else {
            panic!("not an AppKit window handle");
        };
        let content: &NSView = unsafe { &*(appkit.ns_view.as_ptr().cast::<NSView>()) };
        let bounds = content.bounds();
        say!(
            "content view bounds {}x{} points",
            bounds.size.width,
            bounds.size.height
        );

        let sizable = NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable;

        let page_path = out_dir().join("x1-page.html");
        std::fs::write(&page_path, PAGE).expect("page");
        let cfg = unsafe { WKWebViewConfiguration::new(mtm) };
        let webview =
            unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), bounds, &cfg) };
        webview.setAutoresizingMask(sizable);
        content.addSubview(&webview);
        let url =
            unsafe { NSURL::fileURLWithPath(&NSString::from_str(&page_path.to_string_lossy())) };
        let dir = unsafe { NSURL::fileURLWithPath(&NSString::from_str(&out_dir().to_string_lossy())) };
        let _ = unsafe { webview.loadFileURL_allowingReadAccessToURL(&url, &dir) };

        let host = unsafe { NSView::initWithFrame(NSView::alloc(mtm), bounds) };
        host.setWantsLayer(true);
        host.setAutoresizingMask(sizable);
        content.addSubview(&host);
        say!("subviews attached: WKWebView below, Metal host above");

        for screen in NSScreen::screens(mtm).iter() {
            say!(
                "NSScreen {}: backingScaleFactor {}",
                screen.localizedName(),
                screen.backingScaleFactor()
            );
        }
        let window_number = content
            .window()
            .map(|w| w.windowNumber())
            .unwrap_or_default() as u32;
        say!("NSWindow number {window_number}");

        // --- wgpu, on the host view rather than on the winit window.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = make_surface(&instance, &host);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("adapter");
        let info = adapter.get_info();
        say!(
            "adapter: {} ({:?}, {:?}), driver {}",
            info.name,
            info.backend,
            info.device_type,
            info.driver_info
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("x1"),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .expect("device");

        let caps = surface.get_capabilities(&adapter);
        say!("surface alpha_modes reported: {:?}", caps.alpha_modes);
        say!(
            "  PreMultiplied offered: {}",
            caps.alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        );
        say!(
            "  PostMultiplied offered: {}",
            caps.alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        );
        say!("surface formats: {:?}", caps.formats);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .expect("an sRGB surface format");
        say!("chosen format {format:?}");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("x1"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("x1-u"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("x1"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let size = window.inner_size();
        let mut state = State {
            window,
            host,
            _webview: webview,
            window_number,
            gpu: Gpu {
                instance,
                adapter,
                device,
                queue,
                pipeline,
                uniform,
                bind_group,
                format,
            },
            surface: Some(surface),
            size,
        };
        state.configure("first");
        self.state = Some(state);
    }

    fn act(&mut self, what: &str, el: &ActiveEventLoop) {
        let elapsed = self.t0.elapsed().as_secs_f32();
        say!("--- t={elapsed:.1}s {what}");
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match what {
            "render" => state.render(),
            "resize" => {
                let want = PhysicalSize::new(1360u32, 1500u32);
                let got = state.window.request_inner_size(want);
                say!("  requested {want:?}, immediate answer {got:?}");
            }
            "lose-surface-owned" => {
                say!("  dropping the surface and taking the stale layer with it");
                state.surface = None;
                state.strip_metal_sublayers();
                let surface = make_surface(&state.gpu.instance, &state.host);
                state.surface = Some(surface);
                state.configure("rebuilt-owned");
                state.render();
            }
            "lose-surface" => {
                say!("  dropping the surface (the device-loss stand-in)");
                state.surface = None;
                let surface = make_surface(&state.gpu.instance, &state.host);
                let caps = surface.get_capabilities(&state.gpu.adapter);
                say!("  rebuilt surface alpha_modes: {:?}", caps.alpha_modes);
                state.surface = Some(surface);
                state.configure("rebuilt");
                state.render();
            }
            "finish" => {
                say!("=== measurements ===");
                let lines = self.verdicts.clone();
                for v in &lines {
                    say!("{v}");
                }
                el.exit();
            }
            _ if what.starts_with("capture:") => {
                let tag = what.trim_start_matches("capture:").to_owned();
                let lines = state.measure(&tag);
                self.verdicts.extend(lines);
            }
            other => say!("  unknown action {other}"),
        }
    }
}

fn make_surface(instance: &wgpu::Instance, host: &NSView) -> wgpu::Surface<'static> {
    let ptr: NonNull<c_void> = NonNull::from(host).cast();
    let window_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(ptr));
    let display_handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display_handle),
                raw_window_handle: window_handle,
            })
            .expect("surface")
    }
}

impl State {
    fn configure(&mut self, tag: &str) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let size = self.window.inner_size();
        self.size = size;
        let mut config = surface
            .get_default_config(&self.gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("default config");
        config.format = self.gpu.format;
        config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
        config.desired_maximum_frame_latency = 1;
        surface.configure(&self.gpu.device, &config);
        say!(
            "  configure[{tag}]: {}x{} px, format {:?}, alpha {:?}, present {:?}",
            config.width,
            config.height,
            config.format,
            config.alpha_mode,
            config.present_mode
        );
        self.report_layer();
    }

    /// Take away every layer the Metal backend has left on the host view.
    ///
    /// Dropping a `wgpu::Surface` does not: the sublayer it added stays where it
    /// is, and the next surface adds a second one on top of the first.
    fn strip_metal_sublayers(&self) {
        let Some(layer) = self.host.layer() else {
            return;
        };
        let count = unsafe { layer.sublayers() }.map_or(0, |subs| subs.len());
        say!("  removing {count} stale sublayer(s) in one call");
        unsafe { layer.setSublayers(None) };
    }

    /// What the Metal backend did to the layer — the other half of
    /// `wgpu-hal-30.0.0/src/metal/surface.rs:231`.
    fn report_layer(&self) {
        let Some(layer) = self.host.layer() else {
            say!("  host view has no layer");
            return;
        };
        let Some(subs) = (unsafe { layer.sublayers() }) else {
            say!("  host layer has no sublayers");
            return;
        };
        for sub in subs.iter() {
            say!(
                "  sublayer {:?}: opaque {}, contentsScale {}, bounds {}x{}",
                sub.class().name(),
                sub.isOpaque(),
                sub.contentsScale(),
                sub.bounds().size.width,
                sub.bounds().size.height
            );
        }
    }

    fn render(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            say!("  render skipped: no surface");
            return;
        };
        let size = self.size;
        let fig = figure(size.width, size.height);
        let mut u = [0f32; 16];
        u[0] = size.width as f32;
        u[1] = size.height as f32;
        u[4..8].copy_from_slice(&fig.a);
        u[8..12].copy_from_slice(&fig.b);
        u[12..16].copy_from_slice(&fig.c);
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(u.as_ptr().cast::<u8>(), std::mem::size_of_val(&u))
        };
        self.gpu.queue.write_buffer(&self.gpu.uniform, 0, bytes);

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                say!("  get_current_texture: suboptimal");
                texture
            }
            other => {
                say!("  get_current_texture refused: {other:?}");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.gpu.pipeline);
            pass.set_bind_group(0, &self.gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([enc.finish()]);
        self.gpu.queue.present(frame);
        say!("  rendered {}x{}", size.width, size.height);
    }

    fn measure(&mut self, tag: &str) -> Vec<String> {
        let mut out = Vec::new();
        say!(
            "  window scale {}, inner {:?}",
            self.window.scale_factor(),
            self.window.inner_size()
        );
        let Some(shot) = capture_window(self.window_number) else {
            out.push(format!("[{tag}] NO CAPTURE"));
            return out;
        };
        let path = out_dir().join(format!("x1-{tag}.png"));
        shot.write_png(&path);
        say!("  wrote {}", path.display());

        let size = self.size;
        let sx = shot.width as f32 / size.width as f32;
        let sy = shot.height as f32 / size.height as f32;
        say!("  capture/drawable ratio {sx:.3} x {sy:.3}");
        let fig = figure(size.width, size.height);
        let ymid = ((fig.a[1] + fig.a[3]) * 0.5).floor();
        let mut probe = |name: &str, x: f32, y: f32| {
            let px = (x * sx).round() as usize;
            let py = (y * sy).round() as usize;
            let [r, g, b, a] = shot.at(px, py);
            let line = format!("[{tag}] {name:<14} drawable({x},{y}) -> rgba({r},{g},{b},{a})");
            say!("  {line}");
            out.push(line);
        };
        probe("frame", (size.width / 2) as f32, 8.0);
        probe("cal-red-255", 10.0, 10.0);
        probe("cal-grey-128", 60.0, 10.0);

        probe("A-outside", fig.a[0] - 2.0, ymid);
        probe("A-edge", fig.a[0], ymid);
        probe("A-edge+1", fig.a[0] + 1.0, ymid);
        probe("A-edge+2", fig.a[0] + 2.0, ymid);
        probe("A-hole", (fig.a[0] + fig.a[2]) * 0.5, ymid);
        probe("A-top-edge", (fig.a[0] + fig.a[2]) * 0.5, fig.a[1]);
        probe("A-top-edge+1", (fig.a[0] + fig.a[2]) * 0.5, fig.a[1] + 1.0);

        probe("B-edge", fig.b[0], ymid);
        probe("B-hole", (fig.b[0] + fig.b[2]) * 0.5, ymid);

        probe("C-edge", fig.c[0], ymid);
        probe("C-hole-blue0", (fig.c[0] + fig.c[2]) * 0.5, ymid);
        out
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.state.is_none() {
            self.t0 = Instant::now();
            self.build(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                say!("  WindowEvent::Resized {size:?}");
                if let Some(state) = self.state.as_mut() {
                    state.configure("resized");
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                say!("  WindowEvent::ScaleFactorChanged {scale_factor}");
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    state.render();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyD) => self.act("lose-surface", el),
                    PhysicalKey::Code(KeyCode::KeyR) => self.act("resize", el),
                    PhysicalKey::Code(KeyCode::KeyC) => self.act("capture:key", el),
                    PhysicalKey::Code(KeyCode::KeyQ) | PhysicalKey::Code(KeyCode::Escape) => {
                        el.exit()
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.state.is_none() {
            return;
        }
        let now = self.t0.elapsed().as_secs_f32();
        while self.step < STEPS.len() && now >= STEPS[self.step].0 {
            let what = STEPS[self.step].1;
            self.step += 1;
            self.act(what, el);
        }
        el.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(40),
        ));
    }
}
