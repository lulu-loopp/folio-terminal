//! **The same page of glyphs, down the Metal swapchain and down an offscreen
//! texture, read back and compared** — M2-5's Mac half (`docs/DESIGN.md`
//! §13.22).
//!
//! The rasterizer does not change across this port: Folio shapes and rasters
//! through Swash via glyphon on both platforms
//! (`docs/plans/port/macos-plan-2026-09-12.md` §R4). What changes is the
//! presentation path — a `CAMetalLayer` on a view Folio owns, declared
//! `PostMultiplied`, carrying premultiplied pixels (§13.14, X-1). So the one
//! question this file exists to answer is whether that path *changes the
//! picture*, and the only way to ask it is to draw one frame both ways on one
//! device and put the two sets of bytes beside each other.
//!
//! `crates/bt-render/tests/glyph_output.rs` is the other half and runs on
//! either machine; both are handed the page by
//! `bt_render::glyph_probe::GlyphFixture`, which is the whole reason a
//! difference here can be attributed to the surface.
//!
//! # Why the window comes from winit and not from `NSWindow::alloc`
//!
//! It was written the second way first, because
//! `crates/bt-platform/tests/macos_sheet.rs` opens its own window and that is
//! the pattern here. A sheet is not a swapchain. Measured, twice: a hand-made
//! `NSWindow`, `visible=true` at `scale=2` with a content view of exactly the
//! right bounds, turned through sixty rounds of the run loop before the surface
//! was built and four presents after it, answered `PresentOutcome::Skipped`
//! **every time** — `nextDrawable` handed back nothing — and the photograph came
//! back as the flat colour painted behind the layer. A `CAMetalLayer` wants the
//! window an application of its own is running, so this opens the window the
//! product opens: winit's, inside winit's event loop, redrawing the way a window
//! redraws.
//!
//! # Why this is a target of its own rather than a `#[test]`
//!
//! An event loop is the main thread's, and libtest does not give a case the
//! main thread — M2-3 measured it on this workspace's toolchain: a case run
//! with `--test-threads=1` still executes on a thread libtest spawned.
//! `harness = false` hands this file the process's own `main`, which is that
//! thread.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing. `main` returns before it touches a window unless **`BT_MAC_GUI`**
//! is set, so an ordinary `cargo test -p bt-app` on the Mac runs this binary,
//! prints one line and exits. Off macOS it has no body at all. The variable is
//! consent rather than configuration: this puts a window on somebody's desk.
//!
//! **`BT_MAC_GUI_SHOT=<dir>`** names the directory the photograph is taken
//! into, and is required when `BT_MAC_GUI` is set, because the photograph *is*
//! the swapchain readback — a swapchain cannot be mapped, so the pixels the
//! window server holds are reached from outside the surface or not at all.
//!
//! # Who takes the photograph, and why the loop never stops for it
//!
//! Both `screencapture -l<window>` and `CGWindowListCreateImage` are gated on
//! the Screen Recording grant, and a throwaway bundle assembled by a launcher is
//! a new application to TCC every time it is assembled — M2-3 measured the
//! refusal, and this probe measured it again (`could not create image from
//! window`). The session that builds and starts the probe does hold the grant.
//! So the window number is **published** into `<dir>/glyph.window` and the
//! picture is waited for at `<dir>/glyph.png` — and the waiting happens **one
//! redraw at a time**, because a window that stops drawing is a window the
//! server stops compositing, which is a photograph of nothing.

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use bt_render::glyph_probe::{GlyphFixture, digest, first_difference, report};
    use bt_render::{GpuContext, WindowRenderer, WindowTarget};
    use objc2::rc::Retained;
    use objc2_app_kit::NSView;
    use winit::application::ApplicationHandler;
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::{Window, WindowId};

    /// The drawable, in physical pixels — the same page size the offscreen gate
    /// next door draws, so the two runs' numbers are comparable line for line.
    const WIDTH: u32 = 900;
    const HEIGHT: u32 = 760;
    /// The scale this ticket measures at, and the one the window must land on.
    const SCALE: f64 = 2.0;
    /// How many redraws the page is given before its picture is asked for. A
    /// swapchain frame reaches the glass on a display cycle rather than on the
    /// call that submitted it.
    const REDRAWS_BEFORE_THE_PHOTOGRAPH: u32 = 20;
    /// How many more redraws after the number is published before the picture
    /// is read: the photographer is another process, and a file that exists is
    /// not yet a file that is finished.
    const REDRAWS_BEFORE_READING_IT: u32 = 30;
    /// How long the picture is waited for, in redraws, before the probe gives
    /// up on the photographer.
    const REDRAWS_WAITING: u32 = 3000;

    struct Probe {
        into: PathBuf,
        window: Option<Arc<Window>>,
        gpu: Option<GpuContext>,
        on_screen: Option<WindowRenderer>,
        fixture: GlyphFixture,
        drawn: u32,
        published_at: Option<u32>,
    }

    impl Probe {
        fn shot(&self) -> PathBuf {
            self.into.join("glyph.png")
        }

        /// Open the window on the display whose backing scale is 2 — asked of
        /// winit rather than assumed, because X-1 and X-2 both recorded a window
        /// landing on the owner's other screen, and a page measured at scale 1
        /// is a page of numbers about the wrong thing.
        fn open(&mut self, event_loop: &ActiveEventLoop) {
            let mut attributes = Window::default_attributes()
                .with_title("Folio M2-5 glyph probe")
                .with_decorations(false)
                .with_inner_size(PhysicalSize::new(WIDTH, HEIGHT));
            if let Some(monitor) = event_loop
                .available_monitors()
                .find(|monitor| (monitor.scale_factor() - SCALE).abs() < 0.01)
            {
                let at = monitor.position();
                attributes =
                    attributes.with_position(PhysicalPosition::new(at.x + 120, at.y + 120));
            }
            let window = Arc::new(
                event_loop
                    .create_window(attributes)
                    .expect("a window on this display"),
            );
            println!(
                "macos_glyph_surface: scale={} surface={:?}",
                window.scale_factor(),
                window.inner_size()
            );
            assert!(
                (window.scale_factor() - SCALE).abs() < 0.01,
                "the window opened at scale {} and this is the scale-2 measurement",
                window.scale_factor()
            );

            // The one door `bt-app` itself goes through (§13.14 ③): Folio's own
            // view under the content view, emptied of any layer a surface before
            // this one left on it.
            let native = native_window(&window);
            let view = bt_platform::surface_view(native).expect("Folio's own surface view");
            let cleared = bt_platform::clear_surface_layers(native).expect("the view's layers");
            println!("macos_glyph_surface: cleared={cleared}");

            let size = window.inner_size();
            let (gpu, on_screen) = pollster::block_on(GpuContext::open(
                WindowTarget::MetalLayerOnOwnedView(view),
                size.width,
                size.height,
                window.scale_factor(),
            ))
            .expect("a Metal surface on Folio's own view");
            if let Some(alpha) = on_screen.alpha_report() {
                println!(
                    "macos_glyph_surface: alpha target={:?} offered={:?} chosen={:?}",
                    alpha.target, alpha.offered, alpha.chosen
                );
            }
            self.fixture = GlyphFixture::new(size.width, size.height);
            self.gpu = Some(gpu);
            self.on_screen = Some(on_screen);
            window.request_redraw();
            self.window = Some(window);
        }

        /// Draw the page, then either publish the window number or look for the
        /// picture — one step per redraw, so the loop never stops.
        fn redraw(&mut self, event_loop: &ActiveEventLoop) {
            let fixture = self.fixture;
            let Some(window) = self.window.clone() else {
                return;
            };
            let outcome = {
                let (Some(gpu), Some(on_screen)) = (self.gpu.as_mut(), self.on_screen.as_mut())
                else {
                    return;
                };
                fixture
                    .present(gpu, on_screen)
                    .expect("the page draws into the swapchain")
            };
            self.drawn += 1;
            if self.drawn <= 2 {
                println!("macos_glyph_surface: present {} {outcome:?}", self.drawn);
            }
            window.request_redraw();
            if self.drawn < REDRAWS_BEFORE_THE_PHOTOGRAPH {
                return;
            }
            let Some(published_at) = self.published_at else {
                self.published_at = Some(self.drawn);
                std::fs::create_dir_all(&self.into).expect("the shot directory");
                let number = window_number(&window);
                let _ = std::fs::remove_file(self.shot());
                std::fs::write(self.into.join("glyph.window"), format!("{number}\n"))
                    .expect("publish the window number");
                println!("macos_glyph_surface: window {number}");
                return;
            };
            let waited = self.drawn - published_at;
            if waited > REDRAWS_BEFORE_READING_IT
                && std::fs::metadata(self.shot()).is_ok_and(|meta| meta.len() > 0)
            {
                self.compare(event_loop);
                return;
            }
            assert!(
                waited < REDRAWS_WAITING,
                "no photograph appeared in {} — neither this process nor the session that \
                 started it holds the Screen Recording grant",
                self.into.display()
            );
        }

        /// Read the photograph, draw the same page into a texture on the same
        /// device, and put the two sets of bytes beside each other.
        fn compare(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.exit();
            let fixture = self.fixture;
            let shot = self.shot();
            let gpu = self.gpu.as_mut().expect("a device");
            let (photographed, shot_width, shot_height) = as_bgra(&shot);
            let format = gpu.format();
            let mut offscreen =
                WindowRenderer::offscreen(gpu, fixture.width, fixture.height, SCALE, format)
                    .expect("an offscreen window on the same device");
            fixture
                .present(gpu, &mut offscreen)
                .expect("the page draws into the texture");
            let read_back = offscreen.read_back(gpu).expect("the texture reads back");
            let metrics = offscreen.base_metrics();

            println!("macos_glyph_surface: format={format:?}");
            print!(
                "{}",
                report(&read_back, fixture.width, fixture, metrics)
                    .replace("BT_GLYPH ", "BT_GLYPH offscreen ")
            );
            assert_eq!(
                (shot_width, shot_height),
                (fixture.width, fixture.height),
                "the photograph is {shot_width}x{shot_height} and the drawable is {}x{}",
                fixture.width,
                fixture.height,
            );
            print!(
                "{}",
                report(&photographed, fixture.width, fixture, metrics)
                    .replace("BT_GLYPH ", "BT_GLYPH swapchain ")
            );
            match first_difference(&photographed, &read_back) {
                None => println!(
                    "macos_glyph_surface: IDENTICAL digest={:016x}",
                    digest(&read_back)
                ),
                Some((index, swapchain, texture)) => {
                    let differing = photographed
                        .iter()
                        .zip(&read_back)
                        .filter(|(a, b)| a != b)
                        .count();
                    let worst = photographed
                        .iter()
                        .zip(&read_back)
                        .map(|(a, b)| {
                            (0..4)
                                .map(|channel| i32::from(a[channel]) - i32::from(b[channel]))
                                .map(i32::abs)
                                .max()
                                .unwrap_or(0)
                        })
                        .max()
                        .unwrap_or(0);
                    println!(
                        "macos_glyph_surface: DIFFERENT pixels={differing} worst_channel={worst} \
                         first={index} at ({},{}) swapchain={swapchain:?} texture={texture:?}",
                        index as u32 % fixture.width,
                        index as u32 / fixture.width,
                    );
                    panic!(
                        "the Metal swapchain and the offscreen texture drew the same frame \
                         differently: {differing} pixels, worst channel {worst}"
                    );
                }
            }
            println!("macos_glyph_surface: ok");
        }
    }

    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_none() {
                self.open(event_loop);
            }
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::RedrawRequested => self.redraw(event_loop),
                WindowEvent::CloseRequested => event_loop.exit(),
                _ => {}
            }
        }
    }

    fn native_window(window: &Window) -> bt_platform::NativeWindow {
        match window
            .window_handle()
            .expect("a native window handle")
            .as_raw()
        {
            RawWindowHandle::AppKit(handle) => {
                bt_platform::NativeWindow::from_appkit(handle.ns_view)
            }
            other => panic!("this platform has no AppKit handle: {other:?}"),
        }
    }

    /// The window's number, which is the one thing `screencapture -l` cannot
    /// work out for itself.
    #[allow(
        unsafe_code,
        reason = "winit hands its content view over as a raw pointer it owns for the window's \
                  whole life; this retains it for one message send on the thread that owns it, \
                  which is the same contract `bt_platform::surface_view` is called under"
    )]
    fn window_number(window: &Window) -> isize {
        let RawWindowHandle::AppKit(handle) = window
            .window_handle()
            .expect("a native window handle")
            .as_raw()
        else {
            panic!("this platform has no AppKit handle");
        };
        let view: Retained<NSView> = unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }
            .expect("winit's content view is a live object");
        view.window()
            .expect("a view that is on screen belongs to a window")
            .windowNumber()
    }

    /// A PNG on disk, as the `[b, g, r, a]` rows `read_back` answers in.
    fn as_bgra(path: &Path) -> (Vec<[u8; 4]>, u32, u32) {
        let decoded = image::open(path)
            .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        let pixels = decoded
            .pixels()
            .map(|pixel| {
                let [r, g, b, a] = pixel.0;
                [b, g, r, a]
            })
            .collect();
        (pixels, width, height)
    }

    pub fn run() {
        if std::env::var_os("BT_MAC_GUI").is_none() {
            println!("macos_glyph_surface: skipped — set BT_MAC_GUI to open a real window");
            return;
        }
        let into = PathBuf::from(std::env::var_os("BT_MAC_GUI_SHOT").expect(
            "BT_MAC_GUI_SHOT must name a directory: the swapchain's pixels are reached through a \
             photograph and there is nowhere to put one",
        ));
        let event_loop = EventLoop::new().expect("an event loop");
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut probe = Probe {
            into,
            window: None,
            gpu: None,
            on_screen: None,
            fixture: GlyphFixture::new(WIDTH, HEIGHT),
            drawn: 0,
            published_at: None,
        };
        event_loop.run_app(&mut probe).expect("the probe runs");
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_glyph_surface: nothing to run on this platform");
}
