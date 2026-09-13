//! **A real window's rectangle stated, read back and named by the display it
//! stands on** — the claims M3-4 makes that no Windows runner can check
//! (ticket M3-4, `docs/DESIGN.md` §13.34).
//!
//! # Why this is a target of its own rather than a handful of `#[test]`s
//!
//! `macos_sheet.rs`'s reason word for word, and it is the reason every file in
//! this directory is shaped this way: **AppKit is the main thread's and libtest
//! does not hand a case that thread.** A `#[test]` on this toolchain runs on a
//! thread the harness spawned, where `MainThreadMarker::new()` is `None` and
//! every door in `macos_impl` refuses by design — which is exactly what
//! `a_window_door_asked_off_the_window_thread_refuses` proves over there.
//! `harness = false` hands this file the process's own `main`.
//!
//! It is a file of its own rather than more cases in `macos_compose.rs` because
//! the two need different windows: that one needs a **borderless** window, so
//! that its capture is the content area and nothing else, and the whole subject
//! here is a **titled** one — AppKit constrains the frame of a window that has a
//! title bar and constrains nothing about a window that has not, and Folio's
//! windows on this platform keep their title bar (§13.11).
//!
//! # What it costs a run that is not asking for it
//!
//! Nothing. `main` returns before it touches AppKit unless **`BT_MAC_GUI`** is
//! set, so an ordinary `cargo test -p bt-platform` on the Mac runs this binary,
//! prints one line and exits, and off macOS it has no body at all. No new `BT_…`
//! name is introduced — `docs/BT-ENVIRONMENT.md` §4 says where the names in
//! `tests/` live.
//!
//! # What it proves, in order
//!
//! ① **the store → load round trip is the identity** for a rectangle standing
//!    inside the work area — stated through `set_window_outer_rect`, read back
//!    through `get_window_rect`, at the window's own backing scale and at every
//!    parity of the unit a session file is written in, which is the claim
//!    §13.10 ① makes and the one that file rests on;
//! ② **what AppKit does to a rectangle it will not take, and what the doors say
//!    about it.** Two of them: a frame whose title bar would stand under the
//!    menu bar, which `constrainFrameRect:toScreen:` moves before it places,
//!    and a frame that is not a whole number of points, which a window on this
//!    platform cannot have at all. The two doors divide honestly —
//!    `set_window_outer_rect` states and does not check, `stand_window_at`
//!    reads back and answers `Err` with the rectangle the window really got. A
//!    restore that trusted the first door would write a rectangle back to the
//!    file that no window was ever standing at;
//! ③ **a rectangle parked off the side is honoured verbatim**, because the
//!    constraint is vertical: a window a reader pushed half off the right edge
//!    comes back where they pushed it;
//! ④ **a display is named by its UUID**, the same name wherever on that display
//!    it is asked for, in `CFUUID`'s canonical spelling, and never the decimal
//!    display number M1-3 answered with; and
//! ⑤ **a point on no display at all is named by the nearest one**, which is the
//!    promise `MONITOR_DEFAULTTONEAREST` makes to every reader of these three
//!    doors on the other platform and the half of "the display it was saved on
//!    is gone" that lives in this crate.

#[cfg(target_os = "macos")]
mod mac {
    use bt_platform::{NativeWindow, WindowRect};
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSScreen, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize, ns_string};
    use std::ptr::NonNull;

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

    /// **A window shaped like Folio's**: titled, because that is what makes
    /// AppKit constrain its frame, and `FullSizeContentView`, because that is
    /// what M3-3 sets and it is the flag under which the outer rectangle and the
    /// content rectangle are one rectangle.
    ///
    /// Ordered front rather than left hidden: a window that has never been on
    /// the screen has no `screen`, and the question this file asks is what
    /// happens to a rectangle on a display AppKit has an opinion about.
    fn a_window_like_folios(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let screen = NSScreen::screens(mtm)
            .firstObject()
            .expect("this proof needs a display and this machine has none");
        let origin = screen.visibleFrame().origin;
        let frame = NSRect::new(
            NSPoint::new(origin.x + 120.0, origin.y + 120.0),
            NSSize::new(480.0, 300.0),
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
        window.setTitle(ns_string!("Folio M3-4 restore proof"));
        window.orderFrontRegardless();
        window
    }

    /// The handle `bt-app` would have handed the backend: winit's is the content
    /// view, and so is this.
    fn handle_of(window: &NSWindow) -> NativeWindow {
        let view = window.contentView().expect("the window has a content view");
        NativeWindow::from_appkit(NonNull::from(&*view).cast())
    }

    fn spell(rect: WindowRect) -> String {
        format!(
            "{},{} {}x{}",
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top
        )
    }

    /// ① The store → load round trip, at the window's own scale.
    fn a_stated_rectangle_is_the_rectangle_that_comes_back() {
        let name = "a_stated_rectangle_is_the_rectangle_that_comes_back";
        let Some(mtm) = asked_for(name) else { return };
        let window = a_window_like_folios(mtm);
        let native = handle_of(&window);
        let scale = window.backingScaleFactor();
        let work = bt_platform::get_work_area(native).expect("the window is on a display");
        println!("{name}: scale={scale} work={}", spell(work));
        // **Every parity of the unit the session file is written in**, which is
        // the logical pixel — a point on this platform — and therefore a step of
        // `scale` physical pixels. Measured here 2026-09-12: a window's frame is
        // integral in *points*, so at backing scale 2 no window can stand at an
        // odd physical coordinate, and a rectangle that asked to would not be a
        // rectangle any `session.json` can hold (`persisted_window_bounds`
        // records whole logical pixels and `startup_window_rect` multiplies them
        // back). The half-point rectangle is not forgotten — it is the second
        // subject of the case below, where what is asserted is that the doors
        // say so rather than pretend.
        let unit = scale.round().max(1.0) as i32;
        for (dx, dy, dw, dh) in [(0, 0, 0, 0), (1, 0, 1, 0), (0, 1, 0, 1), (1, 1, 1, 1)] {
            let left = work.left + 200 + dx * unit;
            let top = work.top + 200 + dy * unit;
            let (dw, dh) = (dw * unit, dh * unit);
            let asked = WindowRect {
                left,
                top,
                right: left + 960 + dw,
                bottom: top + 600 + dh,
            };
            bt_platform::set_window_outer_rect(native, asked).expect("the window takes a frame");
            let standing = bt_platform::get_window_rect(native).expect("and reports one");
            assert_eq!(
                standing,
                asked,
                "{name}: asked {} and got {}",
                spell(asked),
                spell(standing)
            );
            bt_platform::stand_window_at(native, asked)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        window.close();
        println!("{name}: ok");
    }

    /// ② What AppKit does to a rectangle it will not take, and what the two
    /// doors say about it.
    ///
    /// **Two subjects, and neither of them is a defect** — they are the two ways
    /// a rectangle can be one no window on this platform is allowed to stand at:
    ///
    /// * a frame whose title bar would stand under the menu bar, which
    ///   `constrainFrameRect:toScreen:` moves before it places; and
    /// * a frame that is **not a whole number of points** — measured here on
    ///   2026-09-12, a window's frame is integral in points, so at backing scale
    ///   2 an odd physical coordinate is a rectangle no window has. No
    ///   `session.json` can ask for one (its unit is the logical pixel, which is
    ///   the point), and a summon cannot either (its rectangle comes off a work
    ///   area, which is a display's own points); it is here because the door has
    ///   to be honest about it rather than because anything asks.
    ///
    /// The assertion is not *where* the window ends up — that is macOS's to
    /// decide and this file records it rather than legislating it. The assertion
    /// is the **contract**: `stand_window_at` answers `Ok` only when the window
    /// really is standing at the rectangle it was given, so a caller that reads
    /// its answer is never told a window is somewhere it is not.
    fn a_rectangle_appkit_will_not_take_is_reported_and_not_pretended() {
        let name = "a_rectangle_appkit_will_not_take_is_reported_and_not_pretended";
        let Some(mtm) = asked_for(name) else { return };
        let window = a_window_like_folios(mtm);
        let native = handle_of(&window);
        let scale = window.backingScaleFactor();
        let work = bt_platform::get_work_area(native).expect("the window is on a display");
        // A frame whose top is above the work area: on this platform that is the
        // strip the menu bar stands in, and `constrainFrameRect:toScreen:` is
        // documented to push a titled window's title bar out from under it.
        let above = WindowRect {
            left: work.left + 200,
            top: work.top - 200,
            right: work.left + 1160,
            bottom: work.top + 400,
        };
        // And a frame half a point wide of the grid, which only exists at all on
        // a display whose backing scale is more than one.
        let half_a_point = WindowRect {
            left: work.left + 201,
            top: work.top + 200,
            right: work.left + 1162,
            bottom: work.top + 800,
        };
        let subjects: &[(&str, WindowRect)] = if scale > 1.5 {
            &[
                ("under the menu bar", above),
                ("half a point", half_a_point),
            ]
        } else {
            &[("under the menu bar", above)]
        };
        for (what, asked) in subjects {
            let asked = *asked;
            bt_platform::set_window_outer_rect(native, asked).expect("the window takes a frame");
            let stated = bt_platform::get_window_rect(native).expect("and reports one");
            let answer = bt_platform::stand_window_at(native, asked);
            let standing = bt_platform::get_window_rect(native).expect("and reports one again");
            println!(
                "{name}: {what}: asked {} -> stated {} -> stood {} answer={:?}",
                spell(asked),
                spell(stated),
                spell(standing),
                answer.as_ref().err()
            );
            if answer.is_ok() {
                assert_eq!(
                    standing,
                    asked,
                    "{name}: {what}: the door said yes about a window standing at {}",
                    spell(standing)
                );
            } else {
                assert_ne!(
                    standing, asked,
                    "{name}: {what}: the door said no about a window that is standing exactly \
                     where it was asked to"
                );
            }
        }
        window.close();
        println!("{name}: ok");
    }

    /// ③ A rectangle parked off the side of the display is honoured verbatim.
    fn a_rectangle_parked_off_the_side_is_taken_verbatim() {
        let name = "a_rectangle_parked_off_the_side_is_taken_verbatim";
        let Some(mtm) = asked_for(name) else { return };
        let window = a_window_like_folios(mtm);
        let native = handle_of(&window);
        let work = bt_platform::get_work_area(native).expect("the window is on a display");
        // Half of it off the right edge, which is a shape readers park windows
        // in on purpose and which the restore rule deliberately preserves.
        let asked = WindowRect {
            left: work.right - 480,
            top: work.top + 300,
            right: work.right + 480,
            bottom: work.top + 900,
        };
        bt_platform::stand_window_at(native, asked)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let standing = bt_platform::get_window_rect(native).expect("the window reports a frame");
        assert_eq!(
            standing,
            asked,
            "{name}: a window parked half off the right edge was moved to {}",
            spell(standing)
        );
        window.close();
        println!("{name}: ok");
    }

    /// The canonical `CFUUID` spelling: 8-4-4-4-12, upper-case hexadecimal.
    fn is_a_uuid(name: &str) -> bool {
        let groups: Vec<&str> = name.split('-').collect();
        groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && groups.iter().all(|group| {
                group.chars().all(|c| {
                    c.is_ascii_digit() || (c.is_ascii_uppercase() && c.is_ascii_hexdigit())
                })
            })
    }

    /// ④ A display is named by its UUID, and the name does not depend on where
    /// on that display it was asked for.
    fn a_display_is_named_by_one_uuid_wherever_it_is_asked() {
        let name = "a_display_is_named_by_one_uuid_wherever_it_is_asked";
        let Some(mtm) = asked_for(name) else { return };
        // **The display is found through a window rather than through the screen
        // list**, and that is deliberate: a screen's rectangle in the space these
        // doors speak is the module's own arithmetic, and a test that redid it
        // here would be checking a copy of the rule against the rule. A window is
        // standing somewhere, and `get_work_area` is the door that says where in
        // exactly the units `monitor_id_at` is asked in.
        let window = a_window_like_folios(mtm);
        let native = handle_of(&window);
        let work = bt_platform::get_work_area(native).expect("the window is on a display");
        let here = bt_platform::monitor_id_at(
            work.left + (work.right - work.left) / 2,
            work.top + (work.bottom - work.top) / 2,
        )
        .unwrap_or_else(|| panic!("{name}: the display this window is on has no name"));
        assert!(
            is_a_uuid(&here),
            "{name}: `{here}` is not a UUID in its canonical spelling"
        );
        assert!(
            here.parse::<u32>().is_err(),
            "{name}: `{here}` reads as a display number"
        );
        for (x, y) in [
            (work.left, work.top),
            (work.right - 1, work.top),
            (work.left, work.bottom - 1),
            (work.right - 1, work.bottom - 1),
        ] {
            assert_eq!(
                bt_platform::monitor_id_at(x, y).as_deref(),
                Some(here.as_str()),
                "{name}: one display answered two names, at {x},{y}"
            );
        }
        println!(
            "{name}: the window's display work={} id={here}",
            spell(work)
        );
        // And across the whole desk: no two displays share a name, and every
        // name is a UUID. A coarse grid can miss a small display, so the count is
        // bounded above by the screen list rather than equated with it — what is
        // asserted is that nothing is *shared*, which is the property a session
        // file rests on.
        let desktop = bt_platform::virtual_screen_rect();
        let mut seen: Vec<String> = vec![here];
        for row in 0..16 {
            for column in 0..16 {
                let x = desktop.left + (desktop.right - desktop.left) * column / 16;
                let y = desktop.top + (desktop.bottom - desktop.top) * row / 16;
                let Some(named) = bt_platform::monitor_id_at(x, y) else {
                    continue;
                };
                assert!(
                    is_a_uuid(&named),
                    "{name}: `{named}` at {x},{y} is not a UUID"
                );
                if !seen.contains(&named) {
                    seen.push(named);
                }
            }
        }
        let screens = NSScreen::screens(mtm).count();
        assert!(
            seen.len() <= screens,
            "{name}: {} names came back from {screens} display(s): {seen:?}",
            seen.len()
        );
        window.close();
        println!(
            "{name}: ok — {} name(s) on {screens} display(s)",
            seen.len()
        );
    }

    /// ⑤ A point on no display at all is named by the nearest one — the promise
    /// the other platform's `MONITOR_DEFAULTTONEAREST` makes, kept here.
    fn a_point_on_no_display_is_named_by_the_nearest_one() {
        let name = "a_point_on_no_display_is_named_by_the_nearest_one";
        if asked_for(name).is_none() {
            return;
        }
        let desktop = bt_platform::virtual_screen_rect();
        assert_ne!(
            desktop,
            WindowRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0
            },
            "{name}: this proof needs a desktop"
        );
        // Far off the top-left of everything — where a session saved on a display
        // that has since been unplugged puts its corner.
        let nowhere = (desktop.left - 100_000, desktop.top - 100_000);
        let named = bt_platform::monitor_id_at(nowhere.0, nowhere.1)
            .unwrap_or_else(|| panic!("{name}: a point off the desk was named by nothing"));
        assert!(
            is_a_uuid(&named),
            "{name}: `{named}` is not a UUID in its canonical spelling"
        );
        let work = bt_platform::work_area_at(nowhere.0, nowhere.1)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            bt_platform::monitor_id_at(
                work.left + (work.right - work.left) / 2,
                work.top + (work.bottom - work.top) / 2,
            )
            .as_deref(),
            Some(named.as_str()),
            "{name}: the work area answered for one display and the name for another"
        );
        assert!(
            bt_platform::dpi_at(nowhere.0, nowhere.1) >= 96,
            "{name}: a point off the desk has no scale"
        );
        println!("{name}: ok — {} answered for {:?}", named, nowhere);
    }

    pub fn run() {
        a_stated_rectangle_is_the_rectangle_that_comes_back();
        a_rectangle_appkit_will_not_take_is_reported_and_not_pretended();
        a_rectangle_parked_off_the_side_is_taken_verbatim();
        a_display_is_named_by_one_uuid_wherever_it_is_asked();
        a_point_on_no_display_is_named_by_the_nearest_one();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macos_window_restore: nothing to run on this platform");
}
