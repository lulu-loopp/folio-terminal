//! **A window that plays a video with nothing else in it** — the evidence for
//! route B slice ① (`docs/DESIGN.md` §7.42).
//!
//! It is deliberately not part of `folio`. What is being shown is that
//! `bt_platform::video::engine` and `bt_render`'s video layer play a real file
//! between them, with the clock running and the pictures changing, and putting
//! that behind a terminal, a layout solver and a preview seat would be showing
//! it through four things that could each be the reason it worked or did not.
//! The three surfaces are slice ②'s.
//!
//! ```text
//! cargo run -p bt-app --example video-probe -- <out-dir> <video>...
//! ```
//!
//! For each file it opens an engine, plays it, and draws every frame twice: once
//! into a real window on the screen, and once into an offscreen surface it reads
//! back and writes to a PNG. **Both go through the same
//! [`bt_render::WindowRenderer`], the same pipeline and the same shader**, so
//! the PNG is what the window is showing — the second target exists because a
//! swapchain cannot be read back and a screenshot of a window is a screenshot of
//! whatever was in front of it.
//!
//! It exits by itself. Nothing here waits for a person, takes the foreground
//! (`with_active(false)`) or writes outside the directory it was given.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_platform::video::engine::Engine;
use bt_render::{
    FrameSource, FrameTrigger, GpuContext, SeatViewport, VideoFrameUpload, VideoLayer,
    WindowRenderer, WindowTarget,
};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// The window, and therefore the picture, is this many physical pixels.
const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;

/// When a shot is taken, measured from the moment the file started playing.
///
/// **The first one is the point of the list.** This repository's fixture opens
/// on black and turns to colour — which is the whole reason
/// `video::SEEK_FRACTION` exists — so a shot at a sixth of a second and a shot
/// at four fifths are two visibly different pictures out of one file, and that
/// is a *playback* rather than a decode. The three after it are a second apart
/// and carry the clock, which is burned into the frame beside them.
const SHOTS_AT_SECS: [f64; 4] = [0.15, 0.8, 1.8, 2.8];

/// How long one file is given before the probe moves on, whatever it has
/// managed. A bound, not a schedule: everything interesting has happened by
/// three seconds and this only stops a file that never loads from stopping the
/// run.
const PER_FILE_BUDGET: Duration = Duration::from_secs(12);

/// How much of the window the caption keeps for itself, above the video's box.
const CAPTION_STRIP_PX: u32 = 44;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1).map(PathBuf::from);
    let out_dir = arguments
        .next()
        .ok_or("usage: video-probe <out-dir> <video>...")?;
    let videos: Vec<PathBuf> = arguments.collect();
    if videos.is_empty() {
        return Err("usage: video-probe <out-dir> <video>...".into());
    }
    std::fs::create_dir_all(&out_dir)?;
    // The 200 ms the platform costs the first question, spent before a window
    // exists rather than in the middle of the first play.
    bt_platform::video::prewarm();

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut probe = Probe {
        out_dir,
        videos,
        at: 0,
        window: None,
        compositor: None,
        gpu: None,
        surface: None,
        offscreen: None,
        engine: None,
        started: Instant::now(),
        taken: 0,
        frames: 0,
        last_size: None,
    };
    event_loop.run_app(&mut probe)?;
    // §7.35's ordering, and `shutdown_media_session`'s own debug assertion:
    // every engine is gone before the platform is handed back.
    drop(probe.engine.take());
    bt_platform::video::shutdown_media_session();
    Ok(())
}

struct Probe {
    out_dir: PathBuf,
    videos: Vec<PathBuf>,
    /// Which of `videos` is playing.
    at: usize,
    window: Option<Arc<Window>>,
    /// Held for the surface's whole life: the visual under the swapchain is
    /// this object's, and the commit below is what the caller still owes it.
    compositor: Option<bt_platform::Compositor>,
    gpu: Option<GpuContext>,
    /// The renderer on the real window — what a person sees.
    surface: Option<WindowRenderer>,
    /// The renderer on a texture — what the PNG is read out of. Same device,
    /// same pipeline, same layer.
    offscreen: Option<WindowRenderer>,
    engine: Option<Engine>,
    started: Instant,
    taken: usize,
    frames: u64,
    last_size: Option<(u32, u32)>,
}

impl ApplicationHandler for Probe {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Folio video probe")
                .with_inner_size(PhysicalSize::new(WIDTH, HEIGHT))
                // It must not take the foreground: another agent's window, or
                // the user's own, is the thing in front of this one and stays
                // there.
                .with_active(false),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("PROBE the window would not open: {error}");
                event_loop.exit();
                return;
            }
        };
        // **The same door `folio`'s own windows come through**, which is worth
        // more here than a simpler one would be: a swapchain hung off a
        // DirectComposition visual is what the product presents through, and a
        // probe that used the plain-HWND door would be evidence about a path
        // nothing ships. It also keeps every `unsafe` line in this workspace
        // where it already is — `bt_platform::Compositor` makes the visual and
        // `bt_render::create_surface` is the one call that dereferences it.
        let hwnd = match window_hwnd(&window) {
            Some(hwnd) => hwnd,
            None => {
                eprintln!("PROBE the window has no Win32 handle");
                event_loop.exit();
                return;
            }
        };
        let compositor = match bt_platform::Compositor::new(hwnd) {
            Ok(compositor) => compositor,
            Err(error) => {
                eprintln!("PROBE no composition visual: {error}");
                event_loop.exit();
                return;
            }
        };
        let target = WindowTarget::CompositionVisual(compositor.gpu_visual_ptr());
        let opened = pollster::block_on(GpuContext::open(target, WIDTH, HEIGHT, 1.0));
        let (mut gpu, surface) = match opened {
            Ok(pair) => pair,
            Err(error) => {
                eprintln!("PROBE no device: {error:?}");
                event_loop.exit();
                return;
            }
        };
        let format = gpu.format();
        let offscreen = match WindowRenderer::offscreen(&mut gpu, WIDTH, HEIGHT, 1.0, format) {
            Ok(offscreen) => offscreen,
            Err(error) => {
                eprintln!("PROBE no offscreen target: {error:?}");
                event_loop.exit();
                return;
            }
        };
        eprintln!("PROBE surface format {format:?}");
        self.window = Some(window);
        self.compositor = Some(compositor);
        self.gpu = Some(gpu);
        self.surface = Some(surface);
        self.offscreen = Some(offscreen);
        self.begin(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.draw(event_loop);
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        // A frame's worth of rest between turns, so the probe is a player and
        // not a spin loop: the engine publishes at the video's rate and there is
        // nothing to draw between its frames.
        std::thread::sleep(Duration::from_millis(4));
    }
}

impl Probe {
    /// Open the file at `self.at`, or leave if there are none left.
    fn begin(&mut self, event_loop: &ActiveEventLoop) {
        let Some(path) = self.videos.get(self.at).cloned() else {
            event_loop.exit();
            return;
        };
        match Engine::open(&path) {
            Ok(engine) => {
                engine.set_muted(true);
                engine.play();
                let ready = engine.wait_for_metadata(Duration::from_secs(10));
                let state = engine.state();
                eprintln!(
                    "PROBE open {} ready={ready} adapter={:?} size={:?} duration={:?}s error={:?}",
                    path.display(),
                    engine.adapter_in_use(),
                    state.natural_size,
                    state.duration_secs,
                    state.error,
                );
                self.engine = Some(engine);
            }
            Err(error) => eprintln!("PROBE {} would not open: {error:?}", path.display()),
        }
        self.started = Instant::now();
        self.taken = 0;
        self.frames = 0;
        self.last_size = None;
    }

    /// Move to the next file, or leave.
    fn next(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(engine) = self.engine.as_mut() {
            engine.shutdown();
        }
        self.engine = None;
        self.at += 1;
        self.begin(event_loop);
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gpu), Some(surface), Some(offscreen)) = (
            self.gpu.as_mut(),
            self.surface.as_mut(),
            self.offscreen.as_mut(),
        ) else {
            return;
        };
        let elapsed = self.started.elapsed();
        let Some(engine) = self.engine.as_mut() else {
            event_loop.exit();
            return;
        };
        // **The newest frame, or the one that is standing.** A redraw is not a
        // decode: between two of the video's frames this window still has to
        // paint, and what it paints is the picture that is up.
        if let Some(frame) = engine.frame() {
            self.frames += 1;
            self.last_size = Some((frame.width, frame.height));
        }
        let standing = engine.standing_frame();
        let state = engine.state();
        // **Not the whole window**, and the strip it leaves is doing two jobs.
        // It is where the caption goes — the video layer is drawn *after* the
        // chrome lane, so a box over the whole surface would paint the clock out
        // — and it is a box that is not the surface, which is what makes the
        // letterbox and the scissor visible in the shot rather than merely
        // computed.
        let box_ = SeatViewport {
            x: 0,
            y: CAPTION_STRIP_PX,
            width: WIDTH,
            height: HEIGHT - CAPTION_STRIP_PX,
        };
        let layer = VideoLayer {
            key: format!("probe-{}", self.at),
            box_,
            clip: box_,
            frame: standing.map(|frame| VideoFrameUpload {
                bgra: frame.bgra,
                width_px: frame.width,
                height_px: frame.height,
                generation: frame.generation,
            }),
            // Nothing else paints this window, so the letterbox is this layer's
            // — the pane's own body colour, where there is a pane.
            ground: Some(bt_render::chrome_palette().seat_body),
            radius_px: 0.0,
            opacity: 1.0,
        };
        surface.set_video_layers(vec![layer.clone()]);
        offscreen.set_video_layers(vec![layer]);
        // **The clock, in the picture.** A constant-colour recording makes three
        // shots taken a second apart look like one shot taken three times, and
        // the thing being shown is that time is running. So the position, the
        // frame count and the file are drawn over the video by the window's own
        // chrome lane — which is also a second thing being shown: the video
        // layer is *under* the chrome, where a pane's head and buttons live.
        let caption = format!(
            "{}   {:.3}s / {}   frames {}   {}",
            self.videos
                .get(self.at)
                .and_then(|path| path.file_name())
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            state.position_secs,
            state
                .duration_secs
                .map_or_else(|| "?".to_owned(), |seconds| format!("{seconds:.3}s")),
            self.frames,
            if state.playing { "playing" } else { "paused" },
        );
        let label = bt_render::ChromeLabel {
            mono: false,
            text: caption,
            rect: [16.0, 12.0, WIDTH as f32 - 16.0, 40.0],
            font_size_px: 18.0,
            color: [255, 255, 255],
            align_right: false,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: bt_render::ChromeLabelWeight::Regular,
            tabular_numerals: true,
            clip: None,
        };
        surface.set_chrome(Vec::new(), vec![label.clone()], Vec::new());
        offscreen.set_chrome(Vec::new(), vec![label], Vec::new());
        let trigger = FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        };
        let _ = surface.present_frame(gpu, &[], trigger);
        // The compositor's half of the bargain: a swapchain hung off a visual is
        // not on the screen until the visual tree is committed.
        if let Some(compositor) = self.compositor.as_ref() {
            let _ = compositor.commit();
        }
        let _ = offscreen.present_frame(gpu, &[], trigger);

        // The shots, on the playback's own clock.
        while let Some(due) = SHOTS_AT_SECS.get(self.taken).copied() {
            if elapsed.as_secs_f64() < due {
                break;
            }
            // The **whole** file name and not its stem: two recordings called
            // `folio-video-test` in two containers are two recordings, and a
            // stem would have the second overwrite the first's evidence.
            let name = self
                .videos
                .get(self.at)
                .and_then(|path| path.file_name())
                .map_or_else(|| "video".to_owned(), |name| name.to_string_lossy().into())
                .replace('.', "-");
            let file = self
                .out_dir
                .join(format!("{name}-{:.1}s.png", state.position_secs));
            write_png(offscreen.read_back(gpu), WIDTH, HEIGHT, &file);
            eprintln!(
                "PROBE shot {} position={:.3}s playing={} frames={} picture={:?} cost={:?}",
                file.display(),
                state.position_secs,
                state.playing,
                self.frames,
                self.last_size,
                engine.frame_cost().total(),
            );
            self.taken += 1;
        }
        if self.taken >= SHOTS_AT_SECS.len() || elapsed > PER_FILE_BUDGET {
            eprintln!(
                "PROBE done {:?} frames={} position={:.3}s ended={}",
                self.videos.get(self.at),
                self.frames,
                state.position_secs,
                state.ended
            );
            self.next(event_loop);
        }
    }
}

/// The window's own `HWND`, which is what a composition visual is built over.
fn window_hwnd(window: &Window) -> Option<std::num::NonZeroIsize> {
    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd),
        _ => None,
    }
}

/// `[b, g, r, a]` rows out of the renderer, an ordinary PNG on the disk.
fn write_png(pixels: Vec<[u8; 4]>, width: u32, height: u32, file: &std::path::Path) {
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for [blue, green, red, alpha] in pixels {
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    match image::RgbaImage::from_raw(width, height, rgba) {
        Some(picture) => {
            if let Err(error) = picture.save(file) {
                eprintln!("PROBE could not write {}: {error}", file.display());
            }
        }
        None => eprintln!("PROBE the read-back was the wrong size for {width}x{height}"),
    }
}
