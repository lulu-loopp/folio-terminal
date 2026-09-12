//! **The `CALayer` composition on a real window, read back out of the window
//! server** — the claims M4-1 makes that no `#[test]` can hold (ticket M4-1,
//! `docs/DESIGN.md` §13.24).
//!
//! # Why this is a target of its own rather than a handful of `#[test]`s
//!
//! The same reason `macos_sheet.rs` is one, and its header states it in full:
//! everything here is AppKit, AppKit is **the main thread's**, and libtest does
//! not give a case the main thread — a `#[test]` on this toolchain runs on a
//! thread the harness spawned, where `MainThreadMarker::new()` is `None`.
//! `harness = false` hands this file the process's own `main`.
//!
//! It is a second file rather than two more functions in that one because the
//! two ask different things of the machine: that one puts a panel in front of a
//! reader and waits, and this one builds a composition on a window at backing
//! scale 2 and measures the pixels the window server composited.
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing. `main` returns before it touches AppKit unless **`BT_MAC_GUI`** is
//! set, so an ordinary `cargo test -p bt-platform` on the Mac runs this binary,
//! prints one line and exits. **`BT_MAC_GUI_SHOT`**, when it names a directory,
//! additionally writes each capture into it as a `.ppm`, for a reader who wants
//! to look rather than to read numbers. Both names are `macos_sheet.rs`'s
//! already and `docs/BT-ENVIRONMENT.md` §4 says where they live.
//!
//! # The instrument, and the one thing it is not
//!
//! `CGWindowListCreateImage` over **this process's own window**, which macOS
//! allows without a Screen Recording grant and which X-1 established as the
//! better instrument anyway: it returns the window's own composited buffer at
//! the backing scale rather than a scaled region of a screen. The Mac mini has
//! no Screen Recording grant and an agent cannot obtain one (X-1, "TCC").
//!
//! **What stands in for Folio's frame is a `CALayer`, not wgpu's.** This crate
//! has no wgpu dependency and must not gain one — a dev-dependency on the
//! renderer would put three hundred crates into
//! `cargo check -p bt-platform --target aarch64-apple-darwin --all-targets`,
//! which is a gate every later ticket would pay for. So the frame here is a
//! layer added **exactly where wgpu adds its `CAMetalLayer`** — as a sublayer
//! of Folio's own surface view's layer — carrying opaque bands with a gap
//! between them and one half-alpha panel across the gap. What that measures is
//! CoreAnimation's composition of a non-opaque layer on that view over the page
//! slot beneath it, which is M4-1's whole claim; what it does not re-measure is
//! that wgpu's layer is such a layer, which X-1 measured with real wgpu on this
//! machine and M1-4 measured again in the product (§13.14 ④).
//!
//! # What it proves, in order
//!
//! ① the page slot is **under** Folio's surface and shows through the gap —
//!    and shows **only** where the slot is, which is the half a picture of the
//!    gap alone would not say;
//! ② the floor is the slot's own colour before any page arrives (§7.14), and
//!    the page's own view covers it once M4-2's half exists — modelled here by
//!    a plain coloured `NSView` handed to `attach_page_view`;
//! ③ a half-alpha panel over the slot composites **premultiplied, on encoded
//!    sRGB bytes** — X-1's arithmetic, predicted from this run's own measured
//!    endpoints and therefore exact rather than approximate;
//! ④ a resize moves the slot and nothing else changes;
//! ⑤ `clear_surface_layers` — the door `bt-app`'s rebuild goes through — takes
//!    the frame's layer away and **leaves the slot standing**, and the frame
//!    rebuilt on the emptied view reads as it did before.

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use bt_platform::{Compositor, NativeWindow, PageVisual};
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSScreen, NSView, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_graphics::CGColor;
    use objc2_foundation::{
        NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, ns_string,
    };
    use objc2_quartz_core::CALayer;

    // ── the window server's own picture ────────────────────────────────────

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

    // The entry points are declared here rather than reached through
    // `objc2-core-graphics` because turning that crate's `CGWindow` feature on
    // would be a feature added to the *library* for a test's sake — the same
    // reason X-1's probe declared them, and the same shape `macos_watch`
    // declares CoreServices in.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowListCreateImage(
            screen_bounds: CgRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut c_void;
        fn CGImageGetWidth(image: *mut c_void) -> usize;
        fn CGImageGetHeight(image: *mut c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *mut c_void) -> usize;
        fn CGImageGetDataProvider(image: *mut c_void) -> *mut c_void;
        fn CGDataProviderCopyData(provider: *mut c_void) -> *mut c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *mut c_void) -> *const u8;
        fn CFDataGetLength(data: *mut c_void) -> isize;
        fn CFRelease(cf: *mut c_void);
    }

    /// One composited window, as the window server has it: RGB, row-major, top
    /// row first.
    struct Shot {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    }

    impl Shot {
        fn at(&self, x: usize, y: usize) -> [u8; 3] {
            if x >= self.width || y >= self.height {
                return [0, 0, 0];
            }
            let index = (y * self.width + x) * 3;
            [
                self.pixels[index],
                self.pixels[index + 1],
                self.pixels[index + 2],
            ]
        }

        /// A picture a reader can open, in the one format that needs no
        /// encoder.
        fn write_ppm(&self, path: &std::path::Path) {
            let mut bytes = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
            bytes.extend_from_slice(&self.pixels);
            let _ = std::fs::write(path, bytes);
        }
    }

    /// Read this process's own window back out of the window server.
    fn capture(window_number: u32) -> Shot {
        // SAFETY: a window number this process owns, the two documented option
        // flags, and a null rectangle asking for the window's own bounds.
        let image = unsafe {
            CGWindowListCreateImage(
                CG_RECT_NULL,
                K_LIST_INCLUDING_WINDOW,
                window_number,
                K_IMAGE_BOUNDS_IGNORE_FRAMING | K_IMAGE_BEST_RESOLUTION,
            )
        };
        assert!(!image.is_null(), "the window server returned no image");
        // SAFETY: `image` is the live CGImage the call above returned.
        let (width, height, stride) = unsafe {
            (
                CGImageGetWidth(image),
                CGImageGetHeight(image),
                CGImageGetBytesPerRow(image),
            )
        };
        // SAFETY: as above.
        let provider = unsafe { CGImageGetDataProvider(image) };
        assert!(!provider.is_null(), "the image has no data provider");
        // SAFETY: `provider` belongs to the image, which is still alive.
        let data = unsafe { CGDataProviderCopyData(provider) };
        assert!(!data.is_null(), "the image data could not be copied");
        // SAFETY: `data` is a live CFData and the length is its own.
        let length = unsafe { CFDataGetLength(data) } as usize;
        // SAFETY: as above; the slice does not outlive the `CFRelease` below.
        let source = unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(data), length) };
        // 32 bits per pixel, little-endian, alpha first: the bytes are B, G, R,
        // A — X-1 read the same buffer the same way.
        let mut pixels = vec![0_u8; width * height * 3];
        for y in 0..height {
            for x in 0..width {
                let from = y * stride + x * 4;
                let to = (y * width + x) * 3;
                pixels[to] = source[from + 2];
                pixels[to + 1] = source[from + 1];
                pixels[to + 2] = source[from];
            }
        }
        // SAFETY: both were created by this function and are not used again.
        unsafe {
            CFRelease(data);
            CFRelease(image);
        }
        Shot {
            width,
            height,
            pixels,
        }
    }

    // ── the figure ─────────────────────────────────────────────────────────

    /// The window, in points. Borderless, so the content area *is* the window
    /// and the capture needs no correction for a title bar.
    const CONTENT: (f64, f64) = (600.0, 400.0);
    /// The same window after the resize.
    const RESIZED: (f64, f64) = (700.0, 500.0);
    /// The gap Folio's frame leaves, in points, as a band of `x`. **Full
    /// height, on purpose**: a `CALayer`'s own `y` axis runs the opposite way
    /// from a view's and this file is not the place to settle which, so the
    /// frame is symmetric in `y` and every claim about where something is
    /// vertically is made about the page slot — which is an `NSView`, and whose
    /// placement is exactly what this ticket owns.
    const GAP: (f64, f64) = (100.0, 500.0);
    /// The half-alpha panel, in points, also a full-height band of `x`.
    const PANEL: (f64, f64) = (220.0, 260.0);

    /// Sample columns, in **physical pixels from the left of the window** — the
    /// space every rectangle in this program is written in, which at backing
    /// scale 2 is twice the points above.
    ///
    /// `IN_THE_GAP` is inside the gap (200..1000), left of the panel
    /// (440..520), and inside both of the two pane rectangles this file places.
    const IN_THE_FRAME_LEFT: f64 = 100.0;
    const IN_THE_FRAME_RIGHT: f64 = GAP.1 * 2.0 + 40.0;
    const IN_THE_GAP: f64 = 350.0;
    const IN_THE_PANEL: f64 = PANEL.0 + PANEL.1;

    /// The four colours, as the bytes they are asked for in.
    const WINDOW_GREY: [u8; 3] = [60, 60, 60];
    const FRAME_BLUE: [u8; 3] = [0, 0, 255];
    const FLOOR_YELLOW: [u8; 3] = [255, 255, 0];
    const PAGE_GREEN: [u8; 3] = [0, 255, 0];

    /// Which of the named colours a measured pixel is, by nearest neighbour.
    ///
    /// **Nearest and not equal**, and the reason is measured rather than
    /// defensive: a window's backing store carries the display's colour
    /// profile, so a colour asked for in sRGB does not always arrive as the
    /// same three bytes — X-1 read a page's `#00ff00` back as `(33,252,36)`.
    /// The claim these assertions make is about *which* of four widely
    /// separated colours is at a point, which is the claim M4-1 makes; the
    /// arithmetic claim below is made against this run's own measurements and
    /// is exact.
    fn nearest(pixel: [u8; 3]) -> &'static str {
        let references: [(&str, [u8; 3]); 4] = [
            ("window", WINDOW_GREY),
            ("frame", FRAME_BLUE),
            ("floor", FLOOR_YELLOW),
            ("page", PAGE_GREEN),
        ];
        let distance = |reference: [u8; 3]| {
            (0..3)
                .map(|channel| {
                    let difference = i32::from(pixel[channel]) - i32::from(reference[channel]);
                    difference * difference
                })
                .sum::<i32>()
        };
        references
            .into_iter()
            .min_by_key(|(_, reference)| distance(*reference))
            .map(|(name, _)| name)
            .expect("four references")
    }

    // ── the content view this file makes ───────────────────────────────────

    define_class!(
        // SAFETY:
        // - `NSView` has no subclassing requirement beyond the main thread,
        //   which it declares itself and which this subclass inherits.
        // - This class has no ivars and does not implement `Drop`.
        #[unsafe(super(NSView))]
        #[name = "FolioComposeTestContentView"]
        struct FlippedContent;

        impl FlippedContent {
            /// **winit's answer**, and the reason this file wears it: the
            /// window `bt-app` hands the composition has a content view whose
            /// origin is the top-left, so a proof built on a content view with
            /// AppKit's default origin would be a proof about a window this
            /// product never opens. The other branch is measured too, without a
            /// capture, in `the_unflipped_branch_of_the_placement_is_a_flip`.
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool {
                true
            }
        }
    );

    // ── the run loop ───────────────────────────────────────────────────────

    /// Turn the main run loop for `seconds`, so that what has been asked for is
    /// actually drawn.
    ///
    /// Turning it rather than sleeping matters: a window is only composited by
    /// a loop that is running, and a `sleep` here would photograph a window
    /// that had not been asked to paint.
    fn settle(seconds: f64) {
        let loops = NSRunLoop::currentRunLoop();
        let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
        while std::time::Instant::now() < until {
            autoreleasepool(|_| {
                let date = NSDate::dateWithTimeIntervalSinceNow(0.01);
                // SAFETY: AppKit's own default-mode constant and a live date,
                // on the thread that owns the loop.
                unsafe { loops.runMode_beforeDate(NSDefaultRunLoopMode, &date) };
            });
        }
    }

    // ── Folio's frame, as layers on Folio's own view ───────────────────────

    /// Put the stand-in for Folio's frame on the surface view's layer, and hand
    /// back the one sublayer that was added.
    ///
    /// **One sublayer and not three**, so that the rebuild case measures the
    /// number the product measures: `clear_surface_layers` reports `1` for a
    /// window with one stale `CAMetalLayer` on it, and any other number there
    /// is the shape of a program that does not own its layer (§13.14 ④).
    fn dress_the_frame(surface: &NSView, size: (f64, f64), scale: f64) -> Retained<CALayer> {
        surface.setWantsLayer(true);
        let host = surface.layer().expect("a layer-backed view has a layer");
        let frame = CALayer::new();
        frame.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(size.0, size.1)));
        frame.setContentsScale(scale);
        let band = |from: f64, to: f64, colour: &CGColor| {
            let layer = CALayer::new();
            layer.setFrame(NSRect::new(
                NSPoint::new(from, 0.0),
                NSSize::new(to - from, size.1),
            ));
            layer.setContentsScale(scale);
            layer.setBackgroundColor(Some(colour));
            frame.addSublayer(&layer);
        };
        let blue = CGColor::new_srgb(0.0, 0.0, 1.0, 1.0);
        band(0.0, GAP.0, &blue);
        band(GAP.1, size.0, &blue);
        // And the one non-opaque thing in the picture: half alpha over the gap.
        // A `CGColor` is straight alpha and CoreAnimation premultiplies it, so
        // what lands on the glass is `0.5·blue + 0.5·whatever is underneath` —
        // premultiplied arithmetic on the encoded sRGB bytes, which is exactly
        // what X-1 measured a frame's own half-alpha edge doing.
        let half = CGColor::new_srgb(0.0, 0.0, 1.0, 0.5);
        band(PANEL.0, PANEL.1, &half);
        host.addSublayer(&frame);
        frame
    }

    // ── reading one capture ────────────────────────────────────────────────

    /// One measured point, printed as it is read.
    struct Reading {
        name: &'static str,
        pixel: [u8; 3],
    }

    /// Sample the capture at a point given in **physical pixels from the
    /// top-left of the window**.
    fn read(shot: &Shot, tag: &str, name: &'static str, at: (f64, f64), of: (f64, f64)) -> Reading {
        let sx = shot.width as f64 / (of.0 * 2.0);
        let sy = shot.height as f64 / (of.1 * 2.0);
        let pixel = shot.at((at.0 * sx).round() as usize, (at.1 * sy).round() as usize);
        println!(
            "  [{tag}] {name:<24} px({:>5.0},{:>5.0}) -> rgb({:>3},{:>3},{:>3})  nearest={}",
            at.0,
            at.1,
            pixel[0],
            pixel[1],
            pixel[2],
            nearest(pixel)
        );
        Reading { name, pixel }
    }

    fn is(reading: &Reading, expected: &str) {
        assert_eq!(
            nearest(reading.pixel),
            expected,
            "{} reads rgb{:?}, which is nearer {} than {expected}",
            reading.name,
            reading.pixel,
            nearest(reading.pixel)
        );
    }

    /// **X-1's arithmetic, against this run's own endpoints.**
    ///
    /// The prediction is `0.5·front + 0.5·behind` on the **encoded sRGB bytes**
    /// — half of the frame's colour as this window really composited it, plus
    /// half of the backdrop as this window really composited it. Predicting
    /// from measurements rather than from the numbers the colours were asked
    /// for in is what makes the tolerance three bytes instead of forty: the
    /// display's colour profile is already inside both endpoints, so it cancels.
    ///
    /// A straight-alpha compositor would answer the *front* colour unattenuated
    /// (X-1's rectangle A, `min(255, 255 + 0.5·backdrop)`), which is nowhere
    /// near this prediction — so the claim is not a rounding check.
    fn composites_premultiplied(panel: &Reading, front: &Reading, behind: &Reading) {
        let predicted: [u8; 3] = std::array::from_fn(|channel| {
            ((f64::from(front.pixel[channel]) + f64::from(behind.pixel[channel])) * 0.5).round()
                as u8
        });
        let off = (0..3)
            .map(|channel| (i32::from(panel.pixel[channel]) - i32::from(predicted[channel])).abs())
            .max()
            .unwrap_or_default();
        println!(
            "    {} : 0.5·rgb{:?} + 0.5·rgb{:?} = rgb{:?}, measured rgb{:?}, worst channel {off}",
            panel.name, front.pixel, behind.pixel, predicted, panel.pixel
        );
        assert!(
            off <= 3,
            "{} is {off} bytes off premultiplied arithmetic on encoded sRGB; straight alpha would \
             have answered rgb{:?}",
            panel.name,
            front.pixel
        );
    }

    /// Every point in one capture, in one place, so that the captures are
    /// measured the same way.
    ///
    /// `slot` is the pane's rectangle in physical pixels as `(left, top, right,
    /// bottom)`, `of` is the window's size in points, and `through` is what the
    /// gap is expected to show where the pane stands.
    fn measure(shot: &Shot, tag: &str, of: (f64, f64), slot: (f64, f64, f64, f64), through: &str) {
        let inside_y = (slot.1 + slot.3) / 2.0;
        let above_y = slot.1 / 2.0;
        let below_y = (slot.3 + of.1 * 2.0) / 2.0;

        let frame_left = read(
            shot,
            tag,
            "frame, left band",
            (IN_THE_FRAME_LEFT, inside_y),
            of,
        );
        is(&frame_left, "frame");
        is(
            &read(
                shot,
                tag,
                "frame, right band",
                (IN_THE_FRAME_RIGHT, inside_y),
                of,
            ),
            "frame",
        );
        let over_the_slot = read(shot, tag, "gap over the pane", (IN_THE_GAP, inside_y), of);
        is(&over_the_slot, through);
        let above = read(shot, tag, "gap above the pane", (IN_THE_GAP, above_y), of);
        is(&above, "window");
        is(
            &read(shot, tag, "gap below the pane", (IN_THE_GAP, below_y), of),
            "window",
        );

        let panel_over_slot = read(
            shot,
            tag,
            "half alpha over the pane",
            (IN_THE_PANEL, inside_y),
            of,
        );
        composites_premultiplied(&panel_over_slot, &frame_left, &over_the_slot);
        let panel_over_window = read(
            shot,
            tag,
            "half alpha over the window",
            (IN_THE_PANEL, above_y),
            of,
        );
        composites_premultiplied(&panel_over_window, &frame_left, &above);
    }

    // ── the cases ──────────────────────────────────────────────────────────

    /// The owner's consent, and the main thread this file is written for.
    fn asked_for(name: &str) -> Option<MainThreadMarker> {
        if std::env::var_os("BT_MAC_GUI").is_none() {
            println!("{name}: skipped — set BT_MAC_GUI to open real windows");
            return None;
        }
        let mtm = MainThreadMarker::new().expect(
            "this target is `harness = false` precisely so that it owns the main thread, and it \
             is not on it",
        );
        static LAUNCHED: std::sync::Once = std::sync::Once::new();
        LAUNCHED.call_once(|| NSApplication::sharedApplication(mtm).finishLaunching());
        Some(mtm)
    }

    /// Where a capture goes, when the run asked for pictures.
    fn shot_path(name: &str) -> Option<std::path::PathBuf> {
        let into = std::env::var_os("BT_MAC_GUI_SHOT")?;
        let mut path = std::path::PathBuf::from(into);
        let _ = std::fs::create_dir_all(&path);
        path.push(format!("{name}.ppm"));
        Some(path)
    }

    /// A borderless window on a display at backing scale 2.
    ///
    /// **Borderless** so that the window is its content area and the capture
    /// needs no correction for a title bar, and **at scale 2** because every
    /// number in this file is a physical pixel of a scale-2 window — this
    /// machine has a scale-1 display as well, and a window that opened on it
    /// would be answering a different question.
    fn a_window_at_scale_two(mtm: MainThreadMarker, flipped: bool) -> Retained<NSWindow> {
        let screens = NSScreen::screens(mtm);
        let target = screens
            .iter()
            .find(|screen| (screen.backingScaleFactor() - 2.0).abs() < 0.01)
            .expect("this proof needs a display at backing scale 2 and this machine has none");
        let origin = target.frame().origin;
        let frame = NSRect::new(
            NSPoint::new(origin.x + 140.0, origin.y + 140.0),
            NSSize::new(CONTENT.0, CONTENT.1),
        );
        // SAFETY: `NSWindow`'s designated initializer, on the main thread, with
        // a style mask and a backing store the class documents.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("Folio M4-1 composition proof"));
        window.setOpaque(true);
        window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(WINDOW_GREY[0]) / 255.0,
            f64::from(WINDOW_GREY[1]) / 255.0,
            f64::from(WINDOW_GREY[2]) / 255.0,
            1.0,
        )));
        // The content view winit would have put here, and the one the
        // composition is written against — or AppKit's own, whose origin is the
        // bottom-left, for the case that measures the other branch.
        let bounds = NSRect::new(NSPoint::ZERO, frame.size);
        if flipped {
            // SAFETY: `NSView`'s designated initializer, on the main thread, on
            // an instance of a subclass that adds no ivars.
            let content: Retained<FlippedContent> =
                unsafe { msg_send![FlippedContent::alloc(mtm), initWithFrame: bounds] };
            window.setContentView(Some(&content));
        } else {
            let content = NSView::initWithFrame(NSView::alloc(mtm), bounds);
            window.setContentView(Some(&content));
        }
        window.orderFrontRegardless();
        window
    }

    /// The handle `bt-app` would have handed the backend: winit's is the
    /// content view, and so is this.
    fn handle_of(window: &NSWindow) -> NativeWindow {
        let view = window.contentView().expect("the window has a content view");
        NativeWindow::from_appkit(NonNull::from(&*view).cast())
    }

    /// The page slot, found the way a reader would: it is the **first** subview
    /// of the content view, because `NSView.subviews` runs back to front and
    /// the slot was added at the back.
    ///
    /// Asserting the position rather than looking the class up is the point —
    /// this is the ordering claim, stated where it can fail.
    fn the_slot_behind_everything(content: &NSView) -> Retained<NSView> {
        let subviews = content.subviews();
        assert_eq!(
            subviews.count(),
            2,
            "the content view holds exactly the page's slot and Folio's own surface view"
        );
        let first = subviews.objectAtIndex(0);
        assert!(
            !std::ptr::eq(&*first, &*subviews.objectAtIndex(1)),
            "two subviews, not one twice"
        );
        first
    }

    /// **The whole of the pixel proof**, in the order the header names.
    fn the_page_slot_shows_through_folios_frame_and_only_where_it_stands() {
        let name = "the_page_slot_shows_through_folios_frame_and_only_where_it_stands";
        let Some(mtm) = asked_for(name) else {
            return;
        };
        let window = a_window_at_scale_two(mtm, true);
        let handle = handle_of(&window);
        let number = window.windowNumber() as u32;
        println!(
            "{name}: window {number}, backing scale {}",
            window.backingScaleFactor()
        );

        let compositor = Compositor::new(handle).expect("the composition builds on a real window");
        let page = PageVisual { tab: 1, seat: 1 };
        compositor
            .set_page_ground_color([
                f32::from(FLOOR_YELLOW[0]) / 255.0,
                f32::from(FLOOR_YELLOW[1]) / 255.0,
                f32::from(FLOOR_YELLOW[2]) / 255.0,
                1.0,
            ])
            .expect("the ground colour is stated before any page exists");
        compositor
            .attach_web_visual(page)
            .expect("a page joins the composition");

        // The pane, in physical pixels from the top-left of the client area —
        // deliberately **not** centred vertically, so that a placement which
        // got the flip wrong could not pass by symmetry.
        let slot = (300.0_f64, 120.0_f64, 900.0_f64, 560.0_f64);
        let place = |slot: (f64, f64, f64, f64)| {
            compositor
                .place_web_visual(
                    page,
                    (slot.0 as i32, slot.1 as i32),
                    (0.0, 0.0, (slot.2 - slot.0) as f32, (slot.3 - slot.1) as f32),
                )
                .expect("the pane is placed");
        };
        place(slot);

        let content = window.contentView().expect("a content view");
        let slot_view = the_slot_behind_everything(&content);

        // Folio's own frame, where wgpu's layer goes.
        let surface = {
            let pointer = compositor.gpu_visual_ptr();
            // SAFETY: the pointer is the view the composition made in `new` and
            // holds for its own lifetime, and this is that window's thread.
            unsafe { &*(pointer.cast::<NSView>()) }
        };
        let frame_layer = dress_the_frame(surface, CONTENT, 2.0);
        settle(0.6);

        // ① and ② — the floor, before any page view has arrived.
        let shot = capture(number);
        println!(
            "  [01-floor] capture {}x{} for a {}x{} point window",
            shot.width, shot.height, CONTENT.0, CONTENT.1
        );
        if let Some(path) = shot_path("01-floor") {
            shot.write_ppm(&path);
        }
        measure(&shot, "01-floor", CONTENT, slot, "floor");

        // ② — and the page's own view, once there is one. A plain coloured
        // `NSView` is exactly what M4-2 will hand over: a `WKWebView` is an
        // `NSView`, and this door does not care which subclass.
        let page_view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        page_view.setWantsLayer(true);
        page_view
            .layer()
            .expect("a layer-backed view has a layer")
            .setBackgroundColor(Some(&CGColor::new_srgb(
                f64::from(PAGE_GREEN[0]) / 255.0,
                f64::from(PAGE_GREEN[1]) / 255.0,
                f64::from(PAGE_GREEN[2]) / 255.0,
                1.0,
            )));
        compositor
            .attach_page_view(
                page,
                NativeWindow::from_appkit(NonNull::from(&*page_view).cast()),
            )
            .expect("the page's own view goes into the slot");
        // SAFETY: `superview` does not retain what it hands back, and what it
        // hands back here is the slot — held by the content view's subview list
        // for the rest of this case.
        let standing_in = unsafe { page_view.superview() };
        assert!(
            std::ptr::eq(
                &*standing_in.expect("the page's view is in the hierarchy"),
                &*slot_view
            ),
            "and it goes into the slot rather than beside it"
        );
        settle(0.4);
        let shot = capture(number);
        if let Some(path) = shot_path("02-page") {
            shot.write_ppm(&path);
        }
        measure(&shot, "02-page", CONTENT, slot, "page");

        // ④ — the resize. The window changes shape, the pane is placed again,
        // and the frame is re-laid exactly as `raw-window-metal`'s observers
        // re-lay the real one.
        window.setContentSize(NSSize::new(RESIZED.0, RESIZED.1));
        let moved = (200.0_f64, 260.0_f64, 800.0_f64, 760.0_f64);
        place(moved);
        frame_layer.removeFromSuperlayer();
        let frame_layer = dress_the_frame(surface, RESIZED, 2.0);
        settle(0.6);
        let shot = capture(number);
        println!("  [03-resized] capture {}x{}", shot.width, shot.height);
        if let Some(path) = shot_path("03-resized") {
            shot.write_ppm(&path);
        }
        measure(&shot, "03-resized", RESIZED, moved, "page");

        // ⑤ — the rebuild. `clear_surface_layers` is the door `bt-app`'s
        // device-loss path goes through (`window_surface_target`), and what it
        // empties is the surface view's layer. The slot is a subview of the
        // content view, so it cannot be in what is emptied — and this is where
        // that stops being an argument and becomes a measurement.
        drop(frame_layer);
        let cleared =
            bt_platform::clear_surface_layers(handle).expect("the surface view is emptied");
        println!("  BT_SURFACE cleared={cleared}");
        assert_eq!(
            cleared, 1,
            "exactly one stale layer, which is the number the product's own log line carries"
        );
        settle(0.4);
        let bare = capture(number);
        if let Some(path) = shot_path("04-cleared") {
            bare.write_ppm(&path);
        }
        let survivor = read(
            &bare,
            "04-cleared",
            "the pane, frame removed",
            ((moved.0 + moved.2) / 2.0, (moved.1 + moved.3) / 2.0),
            RESIZED,
        );
        is(&survivor, "page");
        let after = the_slot_behind_everything(&content);
        assert!(
            std::ptr::eq(&*after, &*slot_view),
            "the rebuild left the same slot standing, not a new one"
        );

        let frame_layer = dress_the_frame(surface, RESIZED, 2.0);
        settle(0.6);
        let shot = capture(number);
        if let Some(path) = shot_path("05-rebuilt") {
            shot.write_ppm(&path);
        }
        measure(&shot, "05-rebuilt", RESIZED, moved, "page");
        drop(frame_layer);

        compositor
            .detach_web_visual(page)
            .expect("the page leaves the window");
        assert_eq!(
            content.subviews().count(),
            1,
            "and it takes its slot with it, leaving Folio's own surface view"
        );
        window.close();
        println!("{name}: ok");
    }

    /// **The other branch of the placement: a content view with AppKit's own
    /// origin gets the flip.**
    ///
    /// No capture — the claim is arithmetic, and it is the half the proof above
    /// cannot make because the window it opens wears winit's flipped content
    /// view on purpose. What would otherwise go unnoticed is a pane placed from
    /// the bottom of any window this crate is handed that winit did not make.
    fn the_unflipped_branch_of_the_placement_is_a_flip() {
        let name = "the_unflipped_branch_of_the_placement_is_a_flip";
        let Some(mtm) = asked_for(name) else {
            return;
        };
        // **On the same 2x display**, because the arithmetic below divides by
        // the backing scale and this machine's other display is at 1.
        let window = a_window_at_scale_two(mtm, false);
        assert!(
            (window.backingScaleFactor() - 2.0).abs() < 0.01,
            "the premise of every number in this case"
        );
        let content = window.contentView().expect("the window has a content view");
        assert!(!content.isFlipped(), "the other premise of this case");
        let handle = handle_of(&window);
        let compositor = Compositor::new(handle).expect("the composition builds");
        let page = PageVisual { tab: 7, seat: 2 };
        compositor.attach_web_visual(page).expect("a page joins");
        compositor
            .place_web_visual(page, (300, 120), (0.0, 0.0, 600.0, 440.0))
            .expect("the pane is placed");
        // 120 physical pixels from the top at backing scale 2 is 60 points from
        // the top; the content view is 400 points tall and the pane is 220
        // tall, so an AppKit origin puts it at 400 - 60 - 220 = 120.
        let placed = the_slot_behind_everything(&content).frame();
        println!(
            "  unflipped placement: origin ({}, {}), size {}x{}",
            placed.origin.x, placed.origin.y, placed.size.width, placed.size.height
        );
        assert!((placed.origin.x - 150.0).abs() < 0.01, "{placed:?}");
        assert!((placed.origin.y - 120.0).abs() < 0.01, "{placed:?}");
        assert!((placed.size.width - 300.0).abs() < 0.01, "{placed:?}");
        assert!((placed.size.height - 220.0).abs() < 0.01, "{placed:?}");
        window.close();
        println!("{name}: ok");
    }

    pub fn run() {
        the_page_slot_shows_through_folios_frame_and_only_where_it_stands();
        the_unflipped_branch_of_the_placement_is_a_flip();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_compose: nothing to run on this platform");
}
