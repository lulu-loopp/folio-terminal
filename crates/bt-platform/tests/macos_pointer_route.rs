//! **Where a press over a Folio window goes on this platform** — the claim
//! M3-3 and T-MAC-LIGHTS rest on and nobody had ever measured (ticket
//! T-MAC-POINTER, `docs/DESIGN.md` §13.39).
//!
//! # Why this target exists
//!
//! §13.34 ⑦(d) reported a press on the first-run card's accent button that
//! produced no `WindowEvent::MouseInput` while the same button hovered
//! correctly, and carried the finding forward as a question about "the pointer's
//! route into a Folio window". T-MAC-POINTER measured it: **the route is
//! sound** — the press had never been delivered to the application at all,
//! because another process's floating panels stood over that patch of the desk.
//! What was missing was not a fix but two pins, and they are this file:
//!
//! ① **the transparent title bar does not take a press for the content view.**
//!    `NSTitlebarContainerView` is a real view, it is the *frontmost* subview of
//!    the window's frame view, and it stands over the whole band Folio draws its
//!    header in. If it claimed that band, every control Folio puts up there
//!    would be dead and the drag rule of §13.20 would be unwritable. It does not
//!    — it answers `nil` for every point of the transparent band except the
//!    three traffic lights, which is exactly the division `press_title_bar`'s
//!    note assumes. Measured, so that a later SDK that changes it is caught
//!    here rather than by a reader whose header stopped answering.
//!
//! ② **a press the window server never delivered is named by what stands over
//!    it, at every layer.** The instrument that missed this in §13.34 read the
//!    on-screen list and skipped every window whose `kCGWindowLayer` was not
//!    zero — which is precisely where a floating panel lives. The arithmetic is
//!    a pure function here, pinned against the desk this ticket measured, so
//!    the next agent gets the answer rather than the mistake.
//!
//! ③ **the drag door only ever fires from inside a press.** `press_title_bar`
//!    reads `NSApp.currentEvent` and refuses anything that is not a left mouse
//!    down, because a drag begun from some other event is a drag the reader did
//!    not begin.
//!
//! # Why it is `harness = false`
//!
//! `macos_sheet.rs`'s reason word for word: **AppKit is the main thread's and
//! libtest does not hand a case that thread.** ① and ③ open a real window and
//! put a real question to AppKit, so they need the process's own `main`.
//!
//! # What it costs a run that is not asking for it
//!
//! ② is arithmetic and runs everywhere, on every platform, always. ① and ③
//! return before they touch AppKit unless **`BT_MAC_GUI`** is set, so an
//! ordinary `cargo test -p bt-platform` on the Mac runs this binary, prints
//! three lines and exits. No new `BT_…` name is introduced.

// ── ② the instrument, and it is arithmetic ─────────────────────────────────

/// One row of the window server's on-screen list, in the two fields that decide
/// whether a press lands on you: where the window is and **which layer it is
/// on**.
///
/// The rectangle is in the window server's own units — points, top-left origin,
/// which is what `kCGWindowBounds` answers and what a `CGEvent`'s location is
/// posted in. `number` is `kCGWindowNumber` and `layer` is `kCGWindowLayer`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeskWindow {
    number: i64,
    layer: i32,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

impl DeskWindow {
    fn covers(&self, x: i64, y: i64) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }
}

/// **What stands over a point, in front of the window that wanted the press.**
///
/// `desk` is the on-screen list in the window server's own front-to-back order —
/// `CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, …)` — and the
/// answer is every window ahead of `ours` in that order whose rectangle contains
/// the point. A press at that point is delivered to the first of them and never
/// reaches `ours`.
///
/// **Every layer, and that is the whole of the function.** A floating panel sits
/// at `kCGWindowLayer` 8 and an ordinary window at 0; a reader that filtered the
/// list to layer 0 would be reading the list of windows that cannot be the
/// answer. The order is already the window server's, so nothing here compares
/// layers: being earlier in the list *is* being in front.
fn stands_over(desk: &[DeskWindow], ours: i64, x: i64, y: i64) -> Vec<i64> {
    let mut over = Vec::new();
    for window in desk {
        if window.number == ours {
            break;
        }
        if window.covers(x, y) {
            over.push(window.number);
        }
    }
    over
}

/// ② The desk of 2026-09-13, and the two points this ticket pressed on it.
fn a_press_that_never_arrived_is_named_by_what_stands_over_it_at_every_layer() {
    let name = "a_press_that_never_arrived_is_named_by_what_stands_over_it_at_every_layer";
    // The rows are the ones measured on the venue machine on 2026-09-13, in the
    // window server's own front-to-back order: three of another process's
    // floating panels, then Folio's own window, then the desk behind it.
    let desk = [
        DeskWindow {
            number: 4852,
            layer: 8,
            left: 1129,
            top: 509,
            width: 260,
            height: 192,
        },
        DeskWindow {
            number: 4459,
            layer: 8,
            left: 1063,
            top: 443,
            width: 260,
            height: 192,
        },
        DeskWindow {
            number: 4425,
            layer: 8,
            left: 997,
            top: 377,
            width: 260,
            height: 192,
        },
        DeskWindow {
            number: 5655,
            layer: 0,
            left: 515,
            top: 167,
            width: 960,
            height: 600,
        },
        DeskWindow {
            number: 5639,
            layer: 0,
            left: 955,
            top: 521,
            width: 920,
            height: 464,
        },
    ];
    const OURS: i64 = 5655;

    // The accent button of the first-run card, where the window opened: inside
    // Folio's own window, and inside three panels that are in front of it. They
    // cascade in steps of 66 points and therefore overlap, so a point can be
    // under several at once — the press goes to the first, and the answer is the
    // list rather than the one, because what the reader needs is *what is up
    // there*, not the winner of an argument they are not in.
    assert_eq!(
        stands_over(&desk, OURS, 1163, 532),
        vec![4852, 4459, 4425],
        "{name}: the panels over the button were not named"
    );
    // Eighty points higher — the card's own body, the second press of the run —
    // is clear of the lowest panel of the stack and under the other two.
    assert_eq!(
        stands_over(&desk, OURS, 1163, 452),
        vec![4459, 4425],
        "{name}: the panels over the card's body were not named"
    );
    // The same button after the window was dragged to clear ground. Nothing is
    // in front of it, and this is the press that arrived.
    assert_eq!(
        stands_over(&desk, OURS, 688, 795),
        Vec::<i64>::new(),
        "{name}: clear ground was reported as occluded"
    );

    // **And the reading that missed it.** The instrument §13.34 ⑦(d) concluded
    // "not occlusion" dropped every row whose layer was not zero, which is
    // every row that could have been the answer. Pinned as the mistake, so that
    // nobody writes it twice.
    let layer_zero_only: Vec<DeskWindow> = desk
        .iter()
        .copied()
        .filter(|window| window.layer == 0)
        .collect();
    assert_eq!(
        stands_over(&layer_zero_only, OURS, 1163, 532),
        Vec::<i64>::new(),
        "{name}: this is the reading that reported a clear desk over an occluded press"
    );
    assert_eq!(
        layer_zero_only.first().map(|window| window.number),
        Some(OURS),
        "{name}: and it is the reading under which the occluded window is `z0`"
    );
    println!("{name}: ok — three points, and the layer-0 reading that missed two of them");
}

#[cfg(target_os = "macos")]
mod mac {
    use bt_platform::{CustomFrameGeometry, CustomWindowFrame, NativeWindow};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, MainThreadOnly, define_class};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSScreen, NSView, NSWindow, NSWindowButton,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, ns_string};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// How many presses this file's own view has been handed.
    static PRESSES: AtomicUsize = AtomicUsize::new(0);

    define_class!(
        // SAFETY:
        // - `NSView` has no subclassing requirements beyond being used on the
        //   main thread, which it declares itself and which this inherits.
        // - This class does not implement `Drop` and has no ivars.
        #[unsafe(super(NSView))]
        #[name = "FolioPointerRouteProbeView"]
        struct ProbeView;

        impl ProbeView {
            /// winit's content view answers `true`, and the whole point of this
            /// view is to stand where winit's stands.
            #[unsafe(method(isFlipped))]
            fn is_flipped(&self) -> bool {
                true
            }

            /// **The reason this is a subclass and not an `NSView`.** A view
            /// that implements `mouseDown:` answers `NO` to
            /// `mouseDownCanMoveWindow`, which is the shape winit's view has and
            /// the shape `press_title_bar`'s note is written against: AppKit
            /// will not move the window behind this view's back, so the
            /// application decides, inside the press.
            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: *mut AnyObject) {
                PRESSES.fetch_add(1, Ordering::SeqCst);
            }
        }
    );

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

    /// **A window shaped like Folio's, with a content view shaped like
    /// winit's**: titled and `FullSizeContentView`, because that is what M3-3
    /// sets and what puts the content under the title bar, and a content view
    /// that takes `mouseDown:` itself.
    fn a_window_like_folios(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let screen = NSScreen::screens(mtm)
            .firstObject()
            .expect("this proof needs a display and this machine has none");
        let origin = screen.visibleFrame().origin;
        let frame = NSRect::new(
            NSPoint::new(origin.x + 140.0, origin.y + 140.0),
            NSSize::new(960.0, 600.0),
        );
        // SAFETY: `NSWindow`'s designated initializer, on the main thread, with
        // a style mask and a backing store the class documents.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("Folio T-MAC-POINTER proof"));
        // SAFETY: `initWithFrame:` on a fresh allocation of this file's own
        // subclass, on the main thread.
        let view: Retained<ProbeView> = unsafe {
            objc2::msg_send![ProbeView::alloc(mtm), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), frame.size)]
        };
        window.setContentView(Some(&view));
        window.orderFrontRegardless();
        window
    }

    fn handle_of(window: &NSWindow) -> NativeWindow {
        let view = window.contentView().expect("the window has a content view");
        NativeWindow::from_appkit(NonNull::from(&*view).cast())
    }

    fn class_name(view: &NSView) -> String {
        view.class().name().to_string_lossy().into_owned()
    }

    /// ① The band belongs to the content view, except over the lights.
    fn the_transparent_title_bar_does_not_take_a_press_for_the_content_view() {
        let name = "the_transparent_title_bar_does_not_take_a_press_for_the_content_view";
        let Some(mtm) = asked_for(name) else { return };
        let window = a_window_like_folios(mtm);
        let native = handle_of(&window);
        let frame = CustomWindowFrame::install(
            native,
            CustomFrameGeometry {
                title_bar_logical_px: 40,
                caption_button_logical_px: 46,
            },
        )
        .expect("the frame installs");
        let chrome = frame.platform_chrome();
        assert!(
            chrome.buttons_are_the_platforms,
            "{name}: this window's buttons are not the platform's, so there is no band to ask about"
        );
        let scale = window.backingScaleFactor();
        let band_points = f64::from(chrome.band_px) / scale;
        let lights_right_points = f64::from(chrome.strip_left_px) / scale;
        let content = window.contentView().expect("the window has a content view");
        let content_ptr: *const NSView = &*content;
        // SAFETY: read on the main thread this function owns.
        let frame_view = unsafe { content.superview() }
            .expect("a window's content view stands in the window's frame view");
        let bounds = frame_view.bounds();

        // **The claim is not vacuous, and this is what makes it so**: the title
        // bar's own view is really there, it really is in front of the content
        // view — `subviews` runs back to front, so the last entry is the
        // frontmost — and it really does stand over the band.
        let subviews = frame_view.subviews();
        let frontmost = subviews
            .lastObject()
            .expect("a window's frame view has subviews");
        let frontmost_name = class_name(&frontmost);
        assert!(
            frontmost_name.contains("Titlebar"),
            "{name}: the frontmost subview of the frame view is `{frontmost_name}` and not the \
             title bar's — this case is about a view that is no longer there"
        );
        let bar = frontmost.frame();
        assert!(
            (bar.size.height - band_points).abs() < 1.0 && bar.size.width >= bounds.size.width,
            "{name}: the title bar's view is {:?} and the band this window wears is {band_points}",
            bar
        );

        // The three traffic lights: AppKit's own views, and a press on one of
        // them is AppKit's. Their centres, in the window's own coordinates.
        for which in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            let Some(button) = window.standardWindowButton(which) else {
                continue;
            };
            let in_window = button.convertRect_toView(button.bounds(), None);
            let at = NSPoint::new(
                in_window.origin.x + in_window.size.width / 2.0,
                in_window.origin.y + in_window.size.height / 2.0,
            );
            let hit = frame_view
                .hitTest(at)
                .unwrap_or_else(|| panic!("{name}: nothing at all claimed a window button"));
            let hit_ptr: *const NSView = &*hit;
            assert!(
                !std::ptr::eq(hit_ptr, content_ptr),
                "{name}: the content view claimed the point a {which:?} stands on"
            );
        }

        // Everything else in the band, and everything below it, is the content
        // view's. Walked in points, from clear of the lights to clear of the
        // trailing edge.
        let rows = [
            bounds.size.height - 4.0,
            bounds.size.height - band_points / 2.0,
            bounds.size.height - band_points + 1.0,
            bounds.size.height / 2.0,
            4.0,
        ];
        let mut walked = 0usize;
        for y in rows {
            let mut x = lights_right_points + 8.0;
            while x < bounds.size.width - 8.0 {
                let at = NSPoint::new(x, y);
                let hit = frame_view
                    .hitTest(at)
                    .unwrap_or_else(|| panic!("{name}: nothing claimed {at:?}"));
                let hit_ptr: *const NSView = &*hit;
                assert!(
                    std::ptr::eq(hit_ptr, content_ptr),
                    "{name}: `{}` took the press at {},{} — the content view did not get it",
                    class_name(&hit),
                    x,
                    y
                );
                walked += 1;
                x += 40.0;
            }
        }
        window.close();
        println!(
            "{name}: ok — band {band_points} pt over `{frontmost_name}`, lights to \
             {lights_right_points} pt, {walked} points walked and every one of them the content \
             view's"
        );
    }

    /// ③ The drag door fires from inside a press and from nowhere else.
    fn the_windows_own_drag_door_refuses_when_there_is_no_press_in_hand() {
        let name = "the_windows_own_drag_door_refuses_when_there_is_no_press_in_hand";
        let Some(mtm) = asked_for(name) else { return };
        let window = a_window_like_folios(mtm);
        let frame = CustomWindowFrame::install(
            handle_of(&window),
            CustomFrameGeometry {
                title_bar_logical_px: 40,
                caption_button_logical_px: 46,
            },
        )
        .expect("the frame installs");
        let before = window.frame();
        let refusal = frame
            .press_title_bar()
            .expect_err("there is no press in hand, so this door has nothing to answer");
        assert!(
            refusal.contains("no event in hand") || refusal.contains("not a press"),
            "{name}: the refusal does not say why: {refusal}"
        );
        let after = window.frame();
        assert_eq!(
            (before.origin.x, before.origin.y),
            (after.origin.x, after.origin.y),
            "{name}: a refused drag moved the window"
        );
        window.close();
        println!("{name}: ok — {refusal}");
    }

    pub fn run() {
        the_transparent_title_bar_does_not_take_a_press_for_the_content_view();
        the_windows_own_drag_door_refuses_when_there_is_no_press_in_hand();
    }
}

fn main() {
    a_press_that_never_arrived_is_named_by_what_stands_over_it_at_every_layer();
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_pointer_route: the window half has no body on this platform");
}
