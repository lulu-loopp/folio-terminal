//! **The window and the screen, on macOS** — the AppKit twin of the window and
//! screen half of `windows_impl` (ticket M1-3).
//!
//! # What this file is, and what is still next door
//!
//! M1-1 gave every door `bt-app` writes a body on a machine with no Win32, and
//! filed the window and screen group as *refuse and name the ticket*
//! (`portable_impl.rs`). This is that ticket. Twenty-two of those doors now
//! answer for a real window on a real desktop, and the ones that remain in the
//! portable module on this platform are the ones whose subject is not a window:
//! the composition tree (M1-4, M4-1), notifications and the Dock tile (M4-6),
//! stdio (M3-7). The watches went to [`macos_watch`](super::macos_watch) in
//! M2-1, and the two choosers, the formula menu and the last-resort alert went
//! to [`macos_dialogs`](super::macos_dialogs) in M2-3 — a file of its own for
//! the reason that one is: everything here answers in the turn it was asked in,
//! and everything there puts something in front of the reader and waits. It
//! reaches AppKit through this module's own [`window_thread`] and
//! [`window_for`], which is why those two are `pub(crate)`. Every
//! door that is here is `#[cfg(not(any(windows, target_os = "macos")))]` over
//! there, which is what makes the two lists one list —
//! `a_window_or_screen_door_has_one_arm_per_platform` walks both and says so.
//!
//! # The coordinate rule, stated once
//!
//! **Every rectangle and every point that crosses this module is in physical
//! pixels, in the global space CoreGraphics puts displays in — origin at the
//! top-left of the zero screen, `y` growing downward — at the backing scale of
//! the display the rectangle or point is on.** AppKit's own space is points with
//! the origin at the bottom-left of the zero screen; the flip and the scaling
//! happen here, at the edge, and nowhere else in this workspace.
//!
//! That is not one convention among several. It is **winit's**, and it has to be
//! winit's, because `bt-app` reads the two interchangeably: `dpi_snapshot` asks
//! this crate for the window's rectangle and falls back to `Window::outer_position()`
//! when it will not say, and `restore_monitors` divides a `work_area_at` answer
//! by the scale factor of a **winit** `MonitorHandle` whose position it read
//! from winit. A second convention would not be a different opinion, it would be
//! two halves of one rectangle measured with different rulers. So the arithmetic
//! below is winit 0.30.13's arithmetic, restated:
//!
//! * a window's rectangle is `flip(NSWindow.frame)` multiplied by that window's
//!   `backingScaleFactor` (`window_delegate.rs`'s `outer_position` / `outer_size`),
//! * a display's rectangle is `flip(NSScreen.frame)` multiplied by **that
//!   display's** `backingScaleFactor` (`monitor.rs`'s `position` / `size`,
//!   which reads `CGDisplayBounds` — already flipped — for the same numbers),
//! * the flip constant is the height of the zero screen in points, which is the
//!   one screen whose origin is `(0, 0)` in both spaces.
//!
//! **A window's scale is the display's scale.** `NSWindow.backingScaleFactor`
//! *is* the backing scale of the screen the window is mostly on, so
//! `docs/DESIGN.md` §7.50's rule — one authoritative number per window — is not
//! something this module has to arrange. It is the same reading twice.
//!
//! **What the rule costs, said out loud.** On a desktop whose displays have
//! different backing scales the resulting space is not injective: the 4K at 2×
//! occupies `0..3840` while the 1280×720 panel beside it at 1× starts at its own
//! points offset, so two displays can claim the same "physical" coordinate. That
//! is winit's fiction and this module inherits it rather than inventing a second
//! one; `screen_at` resolves a point by asking the displays in order, exactly as
//! `MonitorFromPoint` resolves a shared edge to whichever neighbour it likes.
//! The one place it can be felt is a summon across a seam, and
//! [`stand_window_at`] answers it the way the Windows arm answers `WM_DPICHANGED`:
//! say the rectangle again, and read it back.
//!
//! # The thread
//!
//! **Every call in this file is AppKit, and AppKit is the main thread's.** The
//! gate is [`window_thread`], which is `NSThread.isMainThread` asked through
//! `MainThreadMarker::new()`, and a door asked from anywhere else refuses with a
//! reason naming the thread rather than the platform. It refuses rather than
//! asserting because the one build where an assertion would fire is the test
//! build, where every case runs on a thread the harness made — so a
//! `debug_assert!` here would turn "this door is main-thread only" into a
//! crash in the only place that says so out loud. The refusal carries the same
//! sentence to the same reader, and `a_window_door_asked_off_the_window_thread_refuses`
//! holds it.
//!
//! The inventory's ownership column says `window` for every door here except six
//! — `work_area_at`, `virtual_screen_rect`, `dpi_at`, `monitor_id_at`,
//! `pointer_position`, `top_level_window_at` — which are `any` on Windows
//! because Win32 answers them from any thread. They are **not** `any` here, and
//! `bt-app` does not need them to be: the three quake readers run on the event
//! loop (`quake.rs`, from `Runtime::turn`), `restore_monitors` runs on the loop
//! with an `ActiveEventLoop` in its hand, and `pointer_is_on_our_own_glass` runs
//! inside a mouse event. The one door `bt-app` really does call from another
//! thread is `hide_every_window_of_this_process`, from the panic hook, and it is
//! deliberately **not** here: it stays a no-op in the portable module until
//! M4-11 decides what a dying process may say to AppKit from a thread that is
//! not the main one.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class,
    msg_send, sel,
};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSAutoresizingMaskOptions, NSButton, NSColor, NSEvent, NSEventType,
    NSFloatingWindowLevel, NSNormalWindowLevel, NSScreen, NSView, NSWindow, NSWindowButton,
    NSWindowDelegate, NSWindowDidBecomeKeyNotification, NSWindowDidEnterFullScreenNotification,
    NSWindowDidExitFullScreenNotification, NSWindowDidResignKeyNotification,
    NSWindowDidResizeNotification, NSWindowDidUpdateNotification, NSWindowOcclusionState,
    NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_core_foundation::{CFRetained, CFUUID};
use objc2_core_graphics::CGDirectDisplayID;
use objc2_foundation::{
    NSArray, NSDictionary, NSKeyValueObservingOptions, NSLocale, NSNotification,
    NSNotificationCenter, NSNumber, NSObjectNSKeyValueObserverRegistration, NSPoint, NSRect,
    NSSize, NSString, NSUserDefaults, ns_string,
};

use crate::{NativeWindow, WheelScrollAmount, WindowRect};

// ── the two sentences this module refuses with ─────────────────────────────

/// A door reached from a thread that does not own the window.
///
/// Its own sentence rather than the platform's, because the two are different
/// facts about different mistakes: *not on this platform* is something the
/// reader's machine cannot do, and this is something this program did wrong.
pub(crate) fn off_the_window_thread(what: &str) -> String {
    format!("{what} was asked from a thread that is not the window's")
}

/// A door given a handle whose window is gone, or a desktop with no displays.
pub(crate) fn nothing_to_ask(what: &str) -> String {
    format!("{what}: this handle names no window on the screen")
}

/// The main-thread gate every door in this file passes through.
///
/// `MainThreadMarker::new()` is `NSThread.isMainThread` with the answer carried
/// in the type system: every AppKit class this module touches takes one, so a
/// door that has this value has proved its thread to the compiler rather than to
/// a comment.
pub(crate) fn window_thread(what: &str) -> Result<MainThreadMarker, String> {
    MainThreadMarker::new().ok_or_else(|| off_the_window_thread(what))
}

// ── the handle, and the window behind it ───────────────────────────────────

/// The `NSWindow` a [`NativeWindow`] names, through the view it actually holds.
///
/// `NativeWindow` carries winit's `ns_view` (see its own note): the view is what
/// winit owns and what wgpu draws to, and the window is reached from it. A view
/// that has been pulled out of its hierarchy has no window, and that is the one
/// failure this conversion has.
///
/// # Safety
///
/// The caller must be on the window's thread — the `MainThreadMarker` is the
/// proof — and the handle must be one `bt-app` got from a live winit window, which
/// is the same contract `windows_impl`'s `as_hwnd` relies on.
pub(crate) fn window_of(
    window: NativeWindow,
    _mtm: MainThreadMarker,
) -> Option<Retained<NSWindow>> {
    // SAFETY: the handle is winit's live `ns_view` pointer, held for as long as
    // the `Window` it came from is alive, and this is the window's own thread.
    let view: &NSView = unsafe { window.as_ns_view().as_ref() };
    view.window()
}

/// [`window_of`], with the refusal already written.
pub(crate) fn window_for(
    window: NativeWindow,
    what: &str,
) -> Result<(MainThreadMarker, Retained<NSWindow>), String> {
    let mtm = window_thread(what)?;
    let window = window_of(window, mtm).ok_or_else(|| nothing_to_ask(what))?;
    Ok((mtm, window))
}

// ── the coordinate rule, as arithmetic ─────────────────────────────────────

/// The height of the zero screen, in points — the whole of the flip.
///
/// The zero screen is `NSScreen.screens[0]`: the display whose origin is
/// `(0, 0)` in AppKit's space and whose top-left is the origin of CoreGraphics'.
/// **Not `NSScreen.mainScreen`**, which is the screen the key window is on and
/// therefore moves when the reader clicks somewhere else — winit's own comment
/// makes the same exclusion for the same reason.
///
/// `None` on a desktop with no displays at all, which is a machine no window is
/// standing on and every caller here reads as "nothing is known".
fn flip_height(mtm: MainThreadMarker) -> Option<f64> {
    let screens = NSScreen::screens(mtm);
    let zero = screens.firstObject()?;
    Some(zero.frame().size.height)
}

/// One AppKit frame, in the space this module speaks.
///
/// The three lines winit's `outer_position` and `outer_size` make together, kept
/// together: the origin is flipped and rounded, the size is rounded on its own,
/// and the far edge is the near edge plus the size. Rounding each of the two
/// rather than the corners is what makes this the same integer winit reports —
/// a window whose rectangle this crate will not state falls through to
/// `winit_outer_rect`, and the two answers have to be the same rectangle.
fn physical_rect(frame: NSRect, flip: f64, scale: f64) -> WindowRect {
    let top = flip - (frame.origin.y + frame.size.height);
    let left = (frame.origin.x * scale).round() as i32;
    let top = (top * scale).round() as i32;
    let width = (frame.size.width * scale).round() as i32;
    let height = (frame.size.height * scale).round() as i32;
    WindowRect {
        left,
        top,
        right: left.saturating_add(width),
        bottom: top.saturating_add(height),
    }
}

/// The inverse: one rectangle of this module's, as AppKit wants it.
///
/// Division and no rounding, which is winit's `set_outer_position` exactly — a
/// logical coordinate is a real number and the integer was only ever the way it
/// was written down on the way out.
fn appkit_frame(rect: WindowRect, flip: f64, scale: f64) -> NSRect {
    let width = f64::from(rect.right.saturating_sub(rect.left).max(1)) / scale;
    let height = f64::from(rect.bottom.saturating_sub(rect.top).max(1)) / scale;
    let x = f64::from(rect.left) / scale;
    let top = f64::from(rect.top) / scale;
    NSRect::new(
        NSPoint::new(x, flip - top - height),
        NSSize::new(width, height),
    )
}

/// Whether a point is inside a rectangle, half-open on the far edges the way
/// every window manager's containment test is.
fn holds(rect: WindowRect, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

/// How far outside a rectangle a point is, squared — the ordering
/// `MONITOR_DEFAULTTONEAREST` is defined by.
fn distance_outside(rect: WindowRect, x: i32, y: i32) -> i64 {
    // In `i64` throughout: the coordinates are `i32` and a hand-edited session
    // file may hold any of them, so the difference of two of them is not an
    // `i32` quantity.
    let (x, y) = (i64::from(x), i64::from(y));
    let dx = (i64::from(rect.left) - x)
        .max(x - i64::from(rect.right) + 1)
        .max(0);
    let dy = (i64::from(rect.top) - y)
        .max(y - i64::from(rect.bottom) + 1)
        .max(0);
    dx * dx + dy * dy
}

/// **The display a point in this module's space is on**, with its own scale and
/// its own rectangle.
///
/// The nearest display and never `None` while the desktop has one, because that
/// is what `MonitorFromPoint(MONITOR_DEFAULTTONEAREST)` promises every caller of
/// [`work_area_at`], [`dpi_at`] and [`monitor_id_at`] on the other platform: a
/// point off the edge of the desktop resolves to the display it is nearest to
/// rather than to nothing.
///
/// The displays are asked **in order**, first match wins, because on a desktop
/// of mixed backing scales the physical rectangles can overlap (see the module
/// header). That is the same tie-break a shared edge gets on Windows, made in
/// the same place, and the zero screen is asked first.
fn screen_at(
    mtm: MainThreadMarker,
    x: i32,
    y: i32,
) -> Option<(Retained<NSScreen>, f64, WindowRect)> {
    let flip = flip_height(mtm)?;
    let screens = NSScreen::screens(mtm);
    let mut nearest: Option<(Retained<NSScreen>, f64, WindowRect, i64)> = None;
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let scale = screen.backingScaleFactor();
        let rect = physical_rect(screen.frame(), flip, scale);
        if holds(rect, x, y) {
            return Some((screen, scale, rect));
        }
        let distance = distance_outside(rect, x, y);
        if nearest
            .as_ref()
            .is_none_or(|(_, _, _, best)| distance < *best)
        {
            nearest = Some((screen, scale, rect, distance));
        }
    }
    nearest.map(|(screen, scale, rect, _)| (screen, scale, rect))
}

// ── the window's rectangle (M1-3) ──────────────────────────────────────────

/// The window's outer rectangle. `NSWindow.frame`, flipped and scaled.
///
/// `NSWindow.frame` **is** the outer rectangle — AppKit has no second rectangle
/// for the frame the way Win32 does, so the contract `windows_impl` had to make
/// `WM_NCCALCSIZE` produce (the outer rect and the client rect are one
/// rectangle, so that save → restore → save is the identity) is simply what a
/// window here already is. What `contentRectForFrameRect:` subtracts is the
/// title bar, and that is the *inner* rectangle nothing in this group reads.
pub fn get_window_rect(window: NativeWindow) -> Result<WindowRect, String> {
    let what = "reading a window's rectangle";
    let (mtm, window) = window_for(window, what)?;
    let flip = flip_height(mtm).ok_or_else(|| nothing_to_ask(what))?;
    Ok(physical_rect(
        window.frame(),
        flip,
        window.backingScaleFactor(),
    ))
}

/// Place the window's outer rectangle exactly. `setFrame:display:`.
///
/// `display: true` and not `false`: the caller is placing a window that is
/// already on the screen (a restore, a settle, a summon), and the redisplay is
/// what keeps the frame from being drawn for the rectangle it has left.
///
/// **AppKit may constrain what it is given**, and this door does not pretend
/// otherwise: a titled window whose frame would put its title bar under the menu
/// bar is moved down by `constrainFrameRect:toScreen:` before it is placed. The
/// Windows arm does not verify its `SetWindowPos` either; the caller that needs
/// the rectangle it asked for is [`stand_window_at`], and that one reads it back.
pub fn set_window_outer_rect(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let what = "placing a window";
    let (mtm, window) = window_for(window, what)?;
    let flip = flip_height(mtm).ok_or_else(|| nothing_to_ask(what))?;
    let frame = appkit_frame(rect, flip, window.backingScaleFactor());
    window.setFrame_display(frame, true);
    Ok(())
}

/// **Stand this window at this rectangle, across a backing-scale seam as well as
/// within one** — the quake drop.
///
/// Word for word the Windows arm's algorithm, because the fact underneath it is
/// the same fact: the first statement of the rectangle is made while the window
/// still belongs to the display it is leaving, so it is converted at *that*
/// display's backing scale, and a window that lands on a 1× panel after being
/// measured at 2× is standing at half the rectangle it was asked for. The second
/// statement is made after the move, when `backingScaleFactor` already reports
/// the display the window arrived on, so it converts with the target's own scale
/// and is honoured verbatim.
///
/// **Bounded, and it reads the answer back** ([`STANDING_ATTEMPTS`], three, for
/// the Windows arm's reasons): one for the ordinary case, two for the seam, and
/// a third for the window that arrives from the second still straddling. The
/// read-back is what makes this a measurement rather than a hope — and here it
/// also catches the other thing that can move a rectangle after it is stated,
/// which is `constrainFrameRect:toScreen:`.
///
/// A rectangle that would not be taken is reported and **not** an error the
/// caller should stop for: the window is standing somewhere, and a summon a few
/// pixels off is enormously better than a summon that refused to come down.
pub fn stand_window_at(window: NativeWindow, rect: WindowRect) -> Result<(), String> {
    let mut last = None;
    for _ in 0..STANDING_ATTEMPTS {
        set_window_outer_rect(window, rect)?;
        let standing = get_window_rect(window)?;
        if standing == rect {
            return Ok(());
        }
        last = Some(standing);
    }
    Err(format!(
        "the window would not stand at {rect:?}; it is at {last:?}"
    ))
}

/// How many times the rectangle is stated before the difference is reported.
const STANDING_ATTEMPTS: usize = 3;

// ── the displays (M1-3) ────────────────────────────────────────────────────

/// The work area — the display minus the menu bar and the Dock — of the display
/// this window is on. `NSScreen.visibleFrame`.
///
/// The window's own screen and not the pointer's, and at the **window's**
/// backing scale, which is the same number as that screen's: the caller divides
/// this rectangle by the scale its renderer is solving at to get a logical size,
/// and a rectangle measured at another display's scale would be a minimum
/// computed for a screen the window is not on.
///
/// **A window that is not on the glass yet is still standing somewhere**, and
/// that is the display its rectangle is on. `NSWindow.screen` answers `nil` for
/// a window that has not been ordered in — which every Folio window is for the
/// whole of its first sixteen steps, because it is created
/// `with_visible(false)` — while `MonitorFromWindow` answers for an invisible
/// window on the other platform. So the rectangle is asked instead, at its own
/// centre, which is the one point of a window that is certainly on the display
/// the window is mostly on.
///
/// A desktop with no displays at all has no work area, and `bt-app` reads the
/// refusal as "keep the last answer", which is tiny-window §4.4's own rule.
pub fn get_work_area(window: NativeWindow) -> Result<WindowRect, String> {
    let what = "reading a display's work area";
    let (mtm, ns_window) = window_for(window, what)?;
    let flip = flip_height(mtm).ok_or_else(|| nothing_to_ask(what))?;
    let (visible, scale) = match ns_window.screen() {
        Some(screen) => (screen.visibleFrame(), screen.backingScaleFactor()),
        None => {
            let standing = physical_rect(ns_window.frame(), flip, ns_window.backingScaleFactor());
            let centre_x = standing
                .left
                .saturating_add(standing.right.saturating_sub(standing.left) / 2);
            let centre_y = standing
                .top
                .saturating_add(standing.bottom.saturating_sub(standing.top) / 2);
            let (screen, scale, _) =
                screen_at(mtm, centre_x, centre_y).ok_or_else(|| nothing_to_ask(what))?;
            (screen.visibleFrame(), scale)
        }
    };
    Ok(physical_rect(visible, flip, scale))
}

/// The work area of the display under a point.
///
/// [`get_work_area`]'s sibling, and it exists for the Windows arm's reason: a
/// tear-out computes its rectangle *before* the window that will stand in it
/// exists, so the display that decides both the scale and the clamp is the one
/// the pointer is over.
pub fn work_area_at(x: i32, y: i32) -> Result<WindowRect, String> {
    let what = "reading a display's work area";
    let mtm = window_thread(what)?;
    let (screen, scale, _) = screen_at(mtm, x, y).ok_or_else(|| nothing_to_ask(what))?;
    let flip = flip_height(mtm).ok_or_else(|| nothing_to_ask(what))?;
    Ok(physical_rect(screen.visibleFrame(), flip, scale))
}

/// **The whole desktop**, as the bounding box of every display.
///
/// The union of each display's own physical rectangle — each measured at its own
/// backing scale, which is what makes this the union of the rectangles winit
/// reports for its monitors rather than a second survey of the same desks.
///
/// An empty rectangle when nothing can be read, which is what the portable arm
/// answers and what the one caller reads as "nothing is known about the
/// desktop"; the one thing it must not be is a made-up screen.
#[must_use]
pub fn virtual_screen_rect() -> WindowRect {
    let empty = WindowRect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let Ok(mtm) = window_thread("reading the desktop's extent") else {
        return empty;
    };
    let Some(flip) = flip_height(mtm) else {
        return empty;
    };
    let screens = NSScreen::screens(mtm);
    let mut union: Option<WindowRect> = None;
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let rect = physical_rect(screen.frame(), flip, screen.backingScaleFactor());
        union = Some(match union {
            None => rect,
            Some(so_far) => WindowRect {
                left: so_far.left.min(rect.left),
                top: so_far.top.min(rect.top),
                right: so_far.right.max(rect.right),
                bottom: so_far.bottom.max(rect.bottom),
            },
        });
    }
    union.unwrap_or(empty)
}

/// The dpi of the display a point is on — its backing scale, in the unit every
/// caller already divides by.
///
/// `96` is not a fallback dressed as an answer: it is *scale 1.0* said in the
/// unit `bt-app` reads, and it is the honest reading for a machine that has no
/// display to ask about. The Windows arm answers the same number for the same
/// reason when `GetDpiForMonitor` refuses.
#[must_use]
pub fn dpi_at(x: i32, y: i32) -> u32 {
    let Ok(mtm) = window_thread("reading a display's backing scale") else {
        return WIN32_DEFAULT_DPI;
    };
    screen_at(mtm, x, y).map_or(WIN32_DEFAULT_DPI, |(_, scale, _)| dpi_for_scale(scale))
}

/// **This window's authoritative backing scale**, in the same unit.
///
/// M1-1 refused this door on the ground that `bt-app` asks it for a *second
/// opinion* about a scale winit has already stated, and that on macOS there is
/// no second source to disagree with — winit's `scale_factor()` is
/// `NSWindow.backingScaleFactor` read by winit itself. That is still true, and
/// it is the reason this answer is safe rather than the reason to withhold it:
/// what comes back is the same reading, taken here, so `authoritative_scale`
/// and `winit_scale` are one number by construction and §7.50's rule holds
/// without anything having to reconcile them. What the reader gains is a
/// `BT_DPI` line that says which display the window is on instead of `n/a`.
pub fn get_dpi_for_window(window: NativeWindow) -> Result<u32, String> {
    let what = "a window's backing scale";
    let (_, window) = window_for(window, what)?;
    Ok(dpi_for_scale(window.backingScaleFactor()))
}

/// The unit `bt-app` divides by — 96 dots to one logical inch, which is Win32's
/// number and the one the whole `BT_DPI` trace is written in.
const WIN32_DEFAULT_DPI: u32 = 96;

/// One backing scale as a dpi. `2.0` is `192`, `1.0` is `96`, and the division
/// back to a scale is exact for every factor a Mac reports.
fn dpi_for_scale(scale: f64) -> u32 {
    let dpi = (scale * f64::from(WIN32_DEFAULT_DPI)).round();
    if dpi >= 1.0 {
        dpi as u32
    } else {
        WIN32_DEFAULT_DPI
    }
}

/// **Which display a point is on, as a name that survives a reboot and a
/// replug** (M3-4).
///
/// The **display's UUID**, not its `CGDirectDisplayID`. M1-3 answered with the
/// display number's decimal spelling on the ground that Apple derives it from
/// the panel's own vendor, model and serial; that is true of the *ingredients*
/// and not of the number. A `CGDirectDisplayID` names "a framebuffer, a colour
/// correction table, and possibly an attached monitor" — the window server hands
/// them out per session, and unplugging a display and plugging it back in can
/// return a different one for the same panel. A session file keyed on it is a
/// file whose rows stop matching the day a cable is moved, which is the one
/// failure the key exists to prevent.
///
/// `CGDisplayCreateUUIDFromDisplayID` is the stable key — the same one winit
/// identifies its own `MonitorHandle`s by, for the same reason — and its
/// canonical `CFUUID` spelling is what this door answers: eight-four-four-four-
/// twelve upper-case hexadecimal, which **cannot be confused with a display
/// number** and cannot be confused with the other platform's
/// `\\.\DISPLAY1` either. The promise to the reader is unchanged and is still
/// §3.1's: best effort, stable across a restart, *not* guaranteed across a
/// driver change — every reader degrades to a computed answer rather than
/// trusting it.
///
/// The Windows arm is untouched: its ids are `DISPLAY_DEVICE.DeviceName`, they
/// mean what they always meant, and nothing here invents a namespace for them.
#[must_use]
pub fn monitor_id_at(x: i32, y: i32) -> Option<String> {
    let mtm = window_thread("naming a display").ok()?;
    let (screen, _, _) = screen_at(mtm, x, y)?;
    display_uuid(display_number(&screen)?)
}

/// The `CGDirectDisplayID` behind one `NSScreen` — the handle, which is the
/// thing [`display_uuid`] turns into a name.
fn display_number(screen: &NSScreen) -> Option<CGDirectDisplayID> {
    let description = screen.deviceDescription();
    let number = description.objectForKey(ns_string!("NSScreenNumber"))?;
    number
        .downcast_ref::<NSNumber>()
        .map(NSNumber::unsignedIntValue)
}

/// **One display's UUID, in `CFUUID`'s own canonical spelling.**
///
/// Two Create-rule handles and both are released here: the `CFUUID` the first
/// call makes, and the `CFString` the second does. `CFRetained` is what owns
/// them, so the release happens on every path out including the `None` one.
fn display_uuid(display: CGDirectDisplayID) -> Option<String> {
    // SAFETY: `CGDisplayCreateUUIDFromDisplayID` takes a display number by
    // value, is documented to answer `NULL` for a number that names no display
    // (`kCGNullDirectDisplay` included, which it checks itself), and follows the
    // Create rule — so the non-null answer is an owned `CFUUID` and `from_raw`
    // is the right way to take it.
    let uuid = unsafe { CGDisplayCreateUUIDFromDisplayID(display) }?;
    let uuid = unsafe { CFRetained::from_raw(uuid) };
    CFUUID::new_string(None, Some(&uuid)).map(|spelling| spelling.to_string())
}

// SAFETY: one CoreGraphics function, declared with the signature
// `CGDirectDisplay.h` gives it.
//
// **`ApplicationServices` and not `CoreGraphics`**, which is winit's own choice
// in `platform_impl/macos/ffi.rs` and is made here for its reason: the symbol
// lives in `ColorSync`, which has only been a framework of its own since macOS
// 10.13 and has always been a sub-framework of `ApplicationServices`. Linking
// the umbrella is what links on every version this product could be asked to
// run on, and it adds no package to `Cargo.lock` — a framework is not a crate.
//
// `objc2-core-graphics` 0.3.2 does not generate this one (its `CGDirectDisplay`
// module stops at the configuration calls), so it is declared rather than
// imported; the type it answers is `objc2-core-foundation`'s `CFUUID`, which is
// the same type `CFUUIDCreateString` above takes, so nothing here invents a
// second spelling of a CoreFoundation type.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C-unwind" {
    fn CGDisplayCreateUUIDFromDisplayID(display: CGDirectDisplayID) -> Option<NonNull<CFUUID>>;
}

/// Where the pointer is.
///
/// `NSEvent.mouseLocation` is the live location in AppKit's space; it is flipped
/// and scaled at the display it lands on, so that the three readings a summon
/// takes together — this, [`dpi_at`] and [`work_area_at`] — are three facts
/// about one display rather than three coordinate systems.
#[must_use]
pub fn pointer_position() -> Option<(i32, i32)> {
    let mtm = window_thread("reading the pointer's position").ok()?;
    let flip = flip_height(mtm)?;
    let location = NSEvent::mouseLocation();
    // **The display is found in AppKit's own space and the point is then stated
    // in that display's.** Asking `screen_at` would be circular: it takes the
    // answer this function is computing. The containment test is the same one,
    // made one conversion earlier.
    let scale = scale_of_screen_holding(mtm, location);
    Some((
        (location.x * scale).round() as i32,
        ((flip - location.y) * scale).round() as i32,
    ))
}

/// The backing scale of the display an AppKit point is on, and `1.0` for a point
/// on none of them — which is the same direction [`dpi_at`] takes for a machine
/// with nothing to read.
fn scale_of_screen_holding(mtm: MainThreadMarker, point: NSPoint) -> f64 {
    let screens = NSScreen::screens(mtm);
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let frame = screen.frame();
        if point.x >= frame.origin.x
            && point.x < frame.origin.x + frame.size.width
            && point.y >= frame.origin.y
            && point.y < frame.origin.y + frame.size.height
        {
            return screen.backingScaleFactor();
        }
    }
    1.0
}

/// **Which top-level window the window server puts under a screen point** — of
/// this application's, and the divergence is written down here.
///
/// `+[NSWindow windowNumberAtPoint:belowWindowWithWindowNumber:]` answers for
/// every window on the desktop, including other applications'; `NSApp
/// windowWithWindowNumber:` then turns that number into a window **only when it
/// is ours**. So this door answers `Some(our window)` when one of ours is
/// frontmost at the point and `None` otherwise — where the Windows arm answers
/// `Some(somebody else's HWND)` for the third case, because an `HWND` names
/// another process's window and a [`NativeWindow`] on this platform is an
/// `NSView` pointer, which cannot.
///
/// **The one reader on this platform takes that safely.**
/// `bt_app::pointer_is_on_our_own_glass` asks whether the window under the
/// pointer is its own and reads `None` as "yes, carry on here" — the
/// conservative half it already documents for a window it cannot ask about. (The
/// other reader the Windows arm has, `exposed_from_probe`, has no macOS caller:
/// [`window_is_exposed`] asks the window server directly here instead of hit
/// testing.) So what a foreign window in front of a Folio window costs today is
/// that a torn-out tab dropped on it lands in the window it came from. Naming
/// another application's window is M2-5's if a reader ever wants it.
#[must_use]
pub fn top_level_window_at(x: i32, y: i32) -> Option<NativeWindow> {
    let mtm = window_thread("asking which window is under a point").ok()?;
    let flip = flip_height(mtm)?;
    let (_, scale, _) = screen_at(mtm, x, y)?;
    let point = NSPoint::new(f64::from(x) / scale, flip - f64::from(y) / scale);
    let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, mtm);
    if number == 0 {
        return None;
    }
    let application = NSApplication::sharedApplication(mtm);
    let window = application.windowWithWindowNumber(number)?;
    let view = window.contentView()?;
    Some(NativeWindow::from_appkit(NonNull::from(&*view).cast()))
}

// ── what the window is doing (M1-3) ────────────────────────────────────────

/// Whether this window is iconic. `isMiniaturized`.
///
/// **`false` for a window that cannot be asked**, which is the direction the
/// portable arm argues for and the one the Windows arm takes: of the two wrong
/// answers, a window wrongly called minimised has its geometry thrown away and a
/// window wrongly called normal has its geometry saved, and only one of those is
/// recoverable. Unlike Win32, `NSWindow.frame` keeps describing the window while
/// it is in the Dock rather than describing an icon parked off-screen, so the
/// caller's rule — do not record the rectangle of a minimised window — is a
/// product decision here rather than a defence against a lie.
#[must_use]
pub fn is_window_minimized(window: NativeWindow) -> bool {
    window_for(window, "asking whether a window is minimised")
        .is_ok_and(|(_, window)| window.isMiniaturized())
}

/// **Whether the reader can actually see this window.**
///
/// `NSWindowOcclusionState` is the window server's own answer to the question
/// the Windows arm has to assemble out of hit tests: the bit is clear when the
/// window is entirely covered, on another Space, or otherwise not being
/// composited onto a screen anybody is looking at. `isVisible` is asked with it
/// because a window that has been ordered out reports no occlusion at all.
///
/// **`true` for a window that cannot be asked**, for the Windows arm's reason:
/// of the two wrong answers, one leaves the marks inside a window the reader is
/// looking at and the other puts a notification on a desktop they can see.
#[must_use]
pub fn window_is_exposed(window: NativeWindow) -> bool {
    let Ok((_, window)) = window_for(window, "asking whether a window is showing") else {
        return true;
    };
    window.isVisible()
        && window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible)
}

/// Put the keyboard back on this window itself.
///
/// **`makeFirstResponder:` and not `makeKeyAndOrderFront:`**, which is what the
/// inventory guessed and is a different act. The one caller is a hosted page
/// saying `Tab` walked off the end of its own controls, and what it needs is
/// what `SetFocus` does on Windows: the *first responder* moves back to the
/// window's own content view, so the next key arrives at winit. Raising and
/// activating the window would be this program taking the foreground because a
/// page pressed Tab.
pub fn take_keyboard_focus(window: NativeWindow) -> Result<(), String> {
    let what = "taking the keyboard focus";
    let (_, ns_window) = window_for(window, what)?;
    let view = ns_window
        .contentView()
        .ok_or_else(|| nothing_to_ask(what))?;
    if ns_window.makeFirstResponder(Some(&view)) {
        Ok(())
    } else {
        Err(format!("{what}: the window would not take it"))
    }
}

/// **Ask the window to close** — `performClose:`, which reaches winit's own
/// close-request path.
///
/// The door M1-1 left refusing, and the refusal was the thing that ended a run:
/// closing the last tab propagated it with `?` and the launch exited before the
/// window did. `performClose:` is the twin of `PostMessageW(WM_CLOSE)` down to
/// the part that matters — it asks the delegate `windowShouldClose:`, and
/// winit's delegate answers that by queueing `WindowEvent::CloseRequested` and
/// returning `NO`, so **AppKit does not close anything**; the application
/// decides, exactly as it does on Windows.
///
/// **A window with no close button is asked through the same delegate
/// directly.** `performClose:` beeps and returns for a window whose style mask
/// has no `Closable` bit, which is what a self-drawn frame will be the moment
/// M3-3 takes the title bar off; sending `windowShouldClose:` to the delegate is
/// what `performClose:` itself does minus the beep, and it reaches the same
/// `CloseRequested`. A window with no delegate at all is a window winit is not
/// driving, and there is nothing honest to do with it.
pub fn request_window_close(window: NativeWindow) -> Result<(), String> {
    let what = "asking a window to close";
    let (_, ns_window) = window_for(window, what)?;
    if ns_window.styleMask().contains(NSWindowStyleMask::Closable) {
        ns_window.performClose(None);
        return Ok(());
    }
    let delegate = ns_window
        .delegate()
        .ok_or_else(|| format!("{what}: the window has no delegate to ask"))?;
    if !delegate.respondsToSelector(sel!(windowShouldClose:)) {
        return Err(format!("{what}: the window's delegate does not answer it"));
    }
    // The method is optional on the protocol, which is why it is asked for by
    // selector first; answered, it is the one `performClose:` would have sent.
    let _ = delegate.windowShouldClose(&ns_window);
    Ok(())
}

/// Keep this window above the others. `NSWindow.level`.
///
/// `NSFloatingWindowLevel` and `NSNormalWindowLevel`, which are the two levels
/// winit itself uses for `WindowLevel::AlwaysOnTop` and `WindowLevel::Normal`,
/// so a window whose row is switched on lands where every other program's
/// always-on-top window lands.
///
/// **Order and nothing else.** Setting the level does not activate the window or
/// move the keyboard, which is the same promise `SWP_NOACTIVATE` makes on the
/// other platform: switching the row on while another window has the keyboard
/// must not steal it, and switching it off must not either.
pub fn set_window_topmost(window: NativeWindow, topmost: bool) -> Result<(), String> {
    let (_, ns_window) = window_for(window, "keeping a window above the others")?;
    ns_window.setLevel(if topmost {
        NSFloatingWindowLevel
    } else {
        NSNormalWindowLevel
    });
    Ok(())
}

/// **Which of the two canvases this window is wearing** — `NSWindow.appearance`.
///
/// The twin of `DWMWA_USE_IMMERSIVE_DARK_MODE`, and the same statement: the
/// system draws this window's chrome — its title bar, its traffic lights, its
/// scrollers and every system control in it — for the canvas the window declares
/// rather than for the one the desktop is set to. The caller derives `dark` from
/// the ground colour actually in force, so a light scheme on a dark desktop gets
/// a light title bar.
///
/// **This is also the one door that changes what winit will tell us**, and the
/// whole of why [`SystemSettingsWatch`] exists on this platform. winit's window
/// delegate observes `effectiveAppearance` and posts `WindowEvent::ThemeChanged`
/// — *unless* the window's own `appearance` is set, in which case it returns
/// early, on the reasoning that a customized window's appearance only ever
/// changes because the application changed it. That reasoning is right and its
/// consequence is that the moment Folio states its canvas, winit stops telling
/// it the desktop's. So the watch below observes the **application's**
/// appearance, which no window override touches.
pub fn set_window_dark_mode(window: NativeWindow, dark: bool) -> Result<(), String> {
    let what = "the window's system appearance";
    let (_, ns_window) = window_for(window, what)?;
    let name = if dark {
        unsafe { NSAppearanceNameDarkAqua }
    } else {
        unsafe { NSAppearanceNameAqua }
    };
    let appearance = NSAppearance::appearanceNamed(name)
        .ok_or_else(|| format!("{what}: this system has no appearance named {name}"))?;
    ns_window.setAppearance(Some(&appearance));
    Ok(())
}

/// **The window's backing colour** — what is on the glass where nothing else has
/// been drawn yet.
///
/// The twin of the class brush, and the same two cases. `Some(rgb)` is an opaque
/// window painted in the theme's own ground, which is what keeps the band a
/// resize opens showing the theme rather than the desktop until the swapchain
/// reaches it. `None` is the translucent ground (§7.1.6c-4b): the window is
/// declared not opaque and its colour is cleared, so what shows through a
/// surface that is 30 % there is the desktop and not this window's own opaque
/// rectangle.
///
/// sRGB and not the calibrated space: the bytes are the theme's, already in the
/// colour space every other statement of them is made in.
pub fn install_window_class_background(
    window: NativeWindow,
    rgb: Option<[u8; 3]>,
) -> Result<(), String> {
    let (_, ns_window) = window_for(window, "the window's backing colour")?;
    match rgb {
        Some([r, g, b]) => {
            let colour = NSColor::colorWithSRGBRed_green_blue_alpha(
                f64::from(r) / 255.0,
                f64::from(g) / 255.0,
                f64::from(b) / 255.0,
                1.0,
            );
            ns_window.setOpaque(true);
            ns_window.setBackgroundColor(Some(&colour));
        }
        None => {
            ns_window.setOpaque(false);
            ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
        }
    }
    Ok(())
}

// ── the view Folio's frames are drawn into (M1-4) ──────────────────────────

define_class!(
    // SAFETY:
    // - `NSView` has no subclassing requirements beyond being used on the main
    //   thread, which it declares itself (`#[thread_kind = MainThreadOnly]`)
    //   and which this subclass inherits.
    // - This class does not implement `Drop` and has no ivars.
    #[unsafe(super(NSView))]
    #[name = "FolioSurfaceView"]
    struct SurfaceView;

    impl SurfaceView {
        /// **This view is a surface and not a place a click can land.**
        ///
        /// It lies over the whole of winit's content view, and AppKit routes a
        /// press to the frontmost view whose `hitTest:` claims the point — so
        /// left alone, every click, drag, scroll and cursor update in Folio
        /// would stop at a view that has no `mouseDown:` and end there.
        /// Answering `nil` takes this view out of the search entirely and the
        /// point falls through to the view underneath, which is winit's, which
        /// is the one this program has always been answering presses from.
        ///
        /// It is the same answer the drag rule needs: `press_title_bar`'s note
        /// records that AppKit asks a *view* whether a press may move the
        /// window, and the view it must reach is winit's.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }
);

/// **The view Folio's frames are drawn into, made once per window and found
/// again afterwards** (M1-4, on X-1's measurements).
///
/// # Why a view of Folio's own, rather than winit's
///
/// Two reasons, and X-1 measured both.
///
/// **The web preview.** On Windows a page composes *under* the swapchain
/// because the swapchain hangs off a visual in a tree `Compositor` owns. macOS
/// has no such tree to build: the arrangement that puts a `WKWebView` behind
/// Folio's frame is **two sibling subviews in one window**, later subviews in
/// front, and a transparent pixel in the frame is then a pixel of the page.
/// That is what M4-1 and M4-2 will attach the page to, and it needs the frame
/// to be in a subview rather than in the content view itself.
///
/// **The rebuild.** A dropped `wgpu::Surface` does not take its `CAMetalLayer`
/// off the view it was attached to. X-1 dropped one and made another: the host
/// view came back with *two* layers and the frame composited twice, to the byte
/// — a half-alpha edge over a half-alpha edge. Whoever rebuilds the surface
/// therefore has to own the view it is rebuilt on, which is what
/// [`clear_surface_layers`] is for.
///
/// # What it does, and what it deliberately does not
///
/// The view is sized to the content view's bounds and given the width and
/// height autoresizing masks, so it tracks the window through every resize
/// without this crate hearing about one — and `raw-window-metal`'s own
/// observers then track *it*, keeping the `CAMetalLayer`'s bounds and
/// `contentsScale` on the view (X-1 measured a 1800×1200 → 1360×1500 resize
/// with `contentsScale` 2 throughout and every sample unchanged). Nothing here
/// makes the layer: wgpu does, inside `create_surface`, because the layer is
/// the surface's and this crate does not depend on wgpu.
///
/// **Called again for the same window, it answers the same view.** The rebuild
/// path goes through the same door as the two constructors — `bt-app`'s
/// `window_surface_target` — so this has to be idempotent or a device loss
/// would leave a stack of dead views behind the live one. The view is found by
/// its class, which is what makes "Folio's own" a question the runtime can
/// answer rather than a position in a list.
/// **What comes back is a pointer and not a [`NativeWindow`]**, for the reason
/// `Compositor::gpu_visual_ptr` returns one on the other platform: its one
/// caller has to hand it to a graphics API that takes a raw handle, and a
/// handle type whose whole contract is that a caller may do nothing with it but
/// give it back would have to grow a reader to be of any use here. The pointer
/// is read at the moment of use and never stored — `bt-app`'s
/// `window_surface_target` builds the target out of it and hands it straight to
/// `bt_render::create_surface` — which is the same contract the visual has.
pub fn surface_view(window: NativeWindow) -> Result<*mut c_void, String> {
    let (_mtm, _content, view) =
        folios_surface_view(window, "the view Folio's frames are drawn into")?;
    Ok(NonNull::from(&*view).as_ptr().cast())
}

/// **The one door that makes Folio's own view**, as the objects rather than as
/// a pointer (M1-4, and M4-1's second caller).
///
/// [`surface_view`] is this function with the pointer taken out of it, and
/// `macos_compose::Compositor::new` is the other caller: the page slot has to be
/// added to the same content view and ordered *under* the same surface view, so
/// the compositor needs both objects and not a number. Splitting it this way
/// rather than letting the second caller walk the hierarchy again is what keeps
/// "Folio's own view" one question with one answer — both doors reach
/// [`folios_own_view`], and neither can decide it differently on a bad day.
///
/// The three values are the window's thread, the content view and the surface
/// view, in that order, because a caller that has the surface view almost always
/// needs the other two in the same breath.
pub(crate) fn folios_surface_view(
    window: NativeWindow,
    what: &str,
) -> Result<(MainThreadMarker, Retained<NSView>, Retained<NSView>), String> {
    let (mtm, ns_window) = window_for(window, what)?;
    let content = ns_window
        .contentView()
        .ok_or_else(|| format!("{what}: this window has no content view"))?;
    if let Some(existing) = folios_own_view(&content) {
        return Ok((mtm, content, existing));
    }
    let view: Retained<SurfaceView> =
        unsafe { msg_send![SurfaceView::alloc(mtm), initWithFrame: content.bounds()] };
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    // `addSubview:` retains it, and the view hierarchy is what keeps it alive
    // for as long as the window is — which is longer than the surface, because
    // the surface is dropped by the renderer the window owns.
    content.addSubview(&view);
    let view: Retained<NSView> = Retained::into_super(view);
    Ok((mtm, content, view))
}

/// Folio's own surface view under a content view, if it has been made yet.
///
/// **By class, and that is the point.** The rebuild has to find the same view
/// the first surface was built on, and every other way of naming it is a rule
/// about position — first subview, last subview, index — that starts naming the
/// `WKWebView` the moment M4-2 puts one into the same content view.
fn folios_own_view(content: &NSView) -> Option<Retained<NSView>> {
    content
        .subviews()
        .iter()
        .find(|view| view.isKindOfClass(SurfaceView::class()))
}

/// **Empty the surface view's layer before a surface is built on it** (M1-4,
/// X-1's finding).
///
/// The thing being removed is not a view and not a subview: `raw-window-metal`
/// makes the view layer-backed and inserts the `CAMetalLayer` as a **sublayer
/// of the view's own layer** (its own "Reasoning behind creating a sublayer"),
/// so what a stale surface leaves behind is one sublayer there. Dropping the
/// `wgpu::Surface` releases the crate's reference to it and nothing else; the
/// superlayer still holds it, it still has the last frame in it, and the next
/// surface's layer goes in *above* it.
///
/// The count comes back rather than being swallowed, and it is the whole
/// evidence this door leaves: **0 at a window's first surface and 1 at every
/// rebuild** is the shape of a program that owns its layer, and any other
/// number is the shape of one that does not.
///
/// A view that is not layer-backed yet has nothing to clear and answers 0 —
/// which is exactly the state [`surface_view`] hands over, since it is wgpu
/// that makes the view layer-backed and it has not been called yet. A window
/// that has no surface view at all answers 0 for the same reason and is not an
/// error: that is a window on its way to its first surface.
///
/// **It takes the window and finds the view itself**, rather than taking the
/// pointer [`surface_view`] just returned. The caller then holds no pointer
/// across two calls, and the two doors cannot disagree about which view is
/// Folio's — they ask [`folios_own_view`] the same question.
pub fn clear_surface_layers(window: NativeWindow) -> Result<usize, String> {
    let what = "clearing the surface view's old layers";
    let (_mtm, ns_window) = window_for(window, what)?;
    let Some(content) = ns_window.contentView() else {
        return Ok(0);
    };
    let Some(view) = folios_own_view(&content) else {
        return Ok(0);
    };
    let Some(layer) = view.layer() else {
        return Ok(0);
    };
    // SAFETY: `sublayers` and `setSublayers:` are `CALayer`'s own accessors for
    // its own array; the objc2 bindings mark them unsafe because a `CALayer`
    // may be any subclass with any invariants, and this one is the plain layer
    // AppKit made for the view above.
    let removed = unsafe { layer.sublayers() }.map_or(0, |layers| layers.len());
    unsafe { layer.setSublayers(None) };
    Ok(removed)
}

// ── the title bar, taken over (M3-3) ───────────────────────────────────────

/// **Take this window's native title bar into Folio's chrome, and measure what
/// AppKit goes on drawing in it** (M3-3, owner ruling 2026-09-12).
///
/// The ruling is that the window must not carry two sets of window controls,
/// and the way it is kept here is the opposite of the way it is kept on
/// Windows. There the frame is taken away outright: `WM_NCCALCSIZE` hands the
/// application the whole outer rectangle and Folio draws all four caption
/// slots itself. Here the title bar is **kept and made transparent** —
/// `NSFullSizeContentView` so the content view reaches under it,
/// `titlebarAppearsTransparent` so nothing of it is painted, `titleVisibility
/// = .hidden` because Folio draws the tab's own name — which leaves exactly
/// one thing of AppKit's standing in that band: the three traffic lights,
/// where macOS puts them and where every other application's are.
///
/// **`isMovableByWindowBackground` is turned off on purpose.** With it on, a
/// drag anywhere over Folio's own surface would move the window; the drag
/// region is the strip's empty part and nothing else, and it is asked for
/// explicitly through [`press_title_bar`].
///
/// **What comes back is a measurement, not a platform name.** The inset is the
/// right edge of the rightmost standard window button, in this module's own
/// units (physical pixels at the window's backing scale), and it is what
/// `bt-app` starts its tab strip from. A window whose style mask carries no
/// title bar has no standard buttons at all — `standardWindowButton:` answers
/// `nil` for each — and the honest answer for it is
/// [`crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR`], the same answer
/// Windows gives, so nothing downstream has to know which platform it is on.
///
/// **The outer rectangle does not move.** `NSWindow.frame` is the outer
/// rectangle before and after (M1-3 ①), so the session round trip this changes
/// nothing about is still the identity; what changes is that the *content*
/// view now fills that rectangle instead of stopping below a title bar, which
/// is the same client-equals-outer contract `WM_NCCALCSIZE` produces on the
/// other platform.
///
/// **The lights move onto the strip's axis, and the watch is what keeps them
/// there** (T-MAC-PILL). `bar_logical_px` is the height of the bar Folio draws
/// in that band — the one number this module cannot know, because it is a fact
/// about the design and not about the window. The watch comes back with the
/// measurement and is dropped with the frame; see [`WindowButtonsWatch`].
pub fn adopt_window_chrome(
    window: NativeWindow,
    bar_logical_px: f64,
    wake: Box<dyn Fn()>,
) -> Result<(crate::PlatformChrome, Option<WindowButtonsWatch>), String> {
    let what = "taking over a window's title bar";
    let (mtm, ns_window) = window_for(window, what)?;
    ns_window.setStyleMask(ns_window.styleMask() | NSWindowStyleMask::FullSizeContentView);
    ns_window.setTitlebarAppearsTransparent(true);
    ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    // The window's `title` is left alone: Mission Control, the window menu and
    // the Dock still read it, and none of them draws it in this bar.
    ns_window.setMovableByWindowBackground(false);
    // **The pitch is read before anything moves, and the run is measured after
    // everything has** (owner ruling 2026-09-13, §13.48). Both halves of that
    // sentence are load-bearing and they pull in opposite directions: the
    // spacing between the buttons has to be macOS's own, so it is taken from a
    // window this product has not touched; the inset `bt-app` starts its strip
    // after has to be where the buttons *end up*, so it is taken from a window
    // this product has finished with. While only `y` moved these were the same
    // reading; since the red light goes onto the corner's diagonal they are not.
    let pitch = read_button_pitch(&ns_window);
    // **And the band the buttons are placed on is the band the window wears**
    // (the owner's two rulings of 2026-09-12 read together). `bar_logical_px` is
    // the caller's, because which of its layouts this window is wearing is a
    // fact about the design. The window can change layout without relaunching,
    // and [`WindowButtonsWatch::follow_the_band`] is how the buttons follow.
    centre_window_buttons(&ns_window, bar_logical_px, pitch);
    let Some(chrome) = measure_window_chrome(&ns_window, mtm) else {
        return Ok((crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR, None));
    };
    let watch = WindowButtonsWatch::install(&ns_window, bar_logical_px, pitch, chrome, wake);
    Ok((chrome, Some(watch)))
}

/// **What the platform is drawing in this window's bar, right now** — the whole
/// of [`crate::PlatformChrome`], measured off the live window.
///
/// `None` for a window that has no standard buttons at all: a style mask with no
/// title bar in it answers `nil` three times, and the honest answer for that
/// window is [`crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR`] — the same
/// answer Windows gives, which is what keeps the whole thing a capability rather
/// than a platform name.
///
/// **A window that has the buttons but is not showing them is a third answer,
/// and it is not `None`** (owner ruling 2026-09-13, §13.48). In full screen
/// macOS takes the three lights off the window until the pointer reaches the top
/// edge; the header stays, so the band is unchanged, but the leading run is
/// gone. That window's honest answer is a run with a height and no width, and
/// [`button_stands_in_this_window`] is where the difference is read. Answering `None`
/// there would say the platform draws nothing in this bar at all, which would
/// take the band with it and stand the panes eight points too high.
fn measure_window_chrome(
    ns_window: &NSWindow,
    mtm: MainThreadMarker,
) -> Option<crate::PlatformChrome> {
    // The three are asked for individually rather than assumed to be in order:
    // a window may carry any subset of them, and what the strip has to clear is
    // whichever of them is furthest right. Their frames are in the frame view's
    // own space, whose origin is the window's leading edge, so `maxX` is
    // already the inset measured from there.
    let buttons: Vec<Retained<NSButton>> = STANDARD_WINDOW_BUTTONS
        .into_iter()
        .filter_map(|which| ns_window.standardWindowButton(which))
        .collect();
    if buttons.is_empty() {
        return None;
    }
    let inset = buttons
        .iter()
        .filter(|button| button_stands_in_this_window(button, ns_window))
        .map(|button| {
            let frame = button.frame();
            frame.origin.x + frame.size.width
        })
        .fold(0.0_f64, f64::max);
    let band = title_bar_height(ns_window, mtm);
    Some(platform_chrome_of(
        inset,
        band,
        ns_window.backingScaleFactor(),
    ))
}

/// **Whether one of those buttons is standing at this window's leading edge**
/// (§13.48).
///
/// Asked of the view and not of the window's full-screen bit, because what the
/// strip needs to know is whether there is anything at its head to stand clear
/// of, and that is a fact about the button. Three ways it can fail to be, and
/// they are three different things AppKit does rather than one written three
/// times:
///
/// * **It is out of the view tree** — `superview` is `nil`.
/// * **It is hidden, or something above it is.** `isHiddenOrHasHiddenAncestor`
///   is AppKit's own name for the pair: the three live inside a title-bar
///   container, and a container that is hidden hides them without any of them
///   carrying the flag itself.
/// * **It is in a different window.** Going full screen is AppKit moving the
///   title bar into a window of its own, above the screen's top edge, and a
///   button that has gone there is not at the leading edge of *this* window
///   however visible it is in that one. This is the case the measurement is
///   really about, and it is the one a `isHidden` check alone would miss.
fn button_stands_in_this_window(button: &NSView, ns_window: &NSWindow) -> bool {
    // SAFETY: a view's own superview, asked on the window's thread — the marker
    // every door in this file passes through. The call is `unsafe` in these
    // bindings because `NSView` is main-thread-only, which is the condition this
    // module has already proved.
    if unsafe { button.superview() }.is_none() {
        return false;
    }
    if button.isHiddenOrHasHiddenAncestor() {
        return false;
    }
    button.window().is_some_and(|host| &*host == ns_window)
}

/// **How tall the bar those buttons stand in is** (T-MAC-LIGHTS) — asked of
/// AppKit rather than assumed to be the 32 points this machine measured.
///
/// Asked as arithmetic on a *style mask* and not as a rectangle off this
/// window, which is what makes it independent of the order
/// [`adopt_window_chrome`] does its work in: `frameRectForContentRect:` on the
/// live window answers the mask it currently has, and the mask it currently has
/// is `FullSizeContentView` — under which the content *is* the frame and the
/// honest answer to "how tall is the title bar" would be zero. So the one bit
/// the take-over sets is taken back out, and the question is put to the class.
///
/// This is §13.11 ①'s own measurement generalised: a `contentRect` 960×600 window
/// reported a 960×632 frame before `FullSizeContentView` and 960×600 after, and
/// the 32 points that left the outer rectangle are exactly this band.
fn title_bar_height(ns_window: &NSWindow, mtm: MainThreadMarker) -> f64 {
    let bar_style = ns_window
        .styleMask()
        .difference(NSWindowStyleMask::FullSizeContentView);
    // Any content rectangle answers this: the difference between a frame and its
    // content is the chrome around it, and it does not depend on the size.
    let content = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
    let framed = NSWindow::frameRectForContentRect_styleMask(content, bar_style, mtm);
    (framed.size.height - content.size.height).max(0.0)
}

/// The two sides of the platform's own run, struck together (T-MAC-LIGHTS).
///
/// A function of three measurements rather than three lines inside
/// [`adopt_window_chrome`], because the pair is a claim that can be checked:
/// the run has a width and a height, both are this window's own numbers at this
/// window's own backing scale, and a window with a run has a band or it is
/// [`crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR`] — there is no window
/// whose buttons occupy a band of no height.
///
/// **The other way round is a state and not a contradiction** (§13.48): a run of
/// no width in a band that has one is the window in full screen, where macOS has
/// taken the three lights away and left the header. That window still has a
/// band for Folio's header to be, and nothing at its leading edge to stand clear
/// of.
fn platform_chrome_of(inset: f64, band: f64, scale: f64) -> crate::PlatformChrome {
    crate::PlatformChrome {
        // Up and not to nearest: the strip may begin one pixel clear of the
        // buttons, never one pixel into them.
        strip_left_px: (inset * scale).ceil() as i32,
        // And the same direction on the other axis, for the same reason: what
        // stands below this band may start one pixel clear of it, never one
        // pixel inside it.
        band_px: (band * scale).ceil() as i32,
        buttons_are_the_platforms: true,
    }
}

/// **Put the three traffic lights on the axis of Folio's own strip, and the red
/// one on the window corner's diagonal** (T-MAC-PILL, owner ruling 2026-09-12;
/// the horizontal half is the owner's ruling of 2026-09-13, `docs/DESIGN.md`
/// §13.48, off `docs/plans/port/corners-2026-09-13.md`).
///
/// macOS sizes its window buttons for a 32-point title bar: a 14-point button
/// with nine points of air above and below it, centred at y 16. Folio's strip is
/// **40** points tall, and the tabs on it are pills centred at y 20. Three
/// buttons sitting four points high of everything beside them is the one thing
/// the reader would see before anything else in that bar, so the buttons move to
/// the strip's axis — top at 13, centre at 20.
///
/// **And then the leading edge, which is one line of arithmetic and a ruling.**
/// The shipping window's corner is rounded at 12.1 points, so the corner circle's
/// centre is near `(12, 12)` and the corner's own 45° line is `x = y`. A button
/// already 20 down is on that line when it is 20 across — so the red light's
/// centre is `(20, 20)` and **its left air is its top air**. That identity *is*
/// the rule, and it is why there is no second number here: `above` is computed
/// once and used on both axes. On the 40-point strip it is 13; on a 32-point
/// band it is 9, which is where macOS itself had them, so a window wearing that
/// band needs no exception.
///
/// **The other two keep macOS's own pitch, read before anything moved.** The
/// caller hands in the offsets the three buttons stood at when the window was
/// first taken over ([`read_button_pitch`]), so miniaturize and zoom keep the 23
/// points AppKit spaced them by rather than a 23 written down here — and the
/// pitch cannot drift, because it is read once and never re-read off buttons
/// this function has already moved. At the standard metrics the run becomes
/// **13 / 36 / 59** and ends at 73.
///
/// **`strip_left_px` is measured after this runs, and that ordering is now
/// load-bearing.** It was not while only `y` moved — the inset is an `x` — and
/// `adopt_window_chrome` places the buttons before it asks the window what they
/// occupy.
///
/// **Measured against the view they are in, and the view decides its own axis.**
/// A button's frame is in its superview's coordinates — `NSTitlebarView`, whose
/// top edge is the window's top edge — and AppKit's default is an unflipped
/// view, where `origin.y` grows upward from the bottom. `isFlipped` is asked
/// rather than assumed, because the whole of what this computes is a distance
/// from the *top* and a view that answers `true` measures it directly.
///
/// At the standard metrics the button lands 13..27 inside a 32-point bar, so it
/// is moved four points down and still stands wholly inside the view it is in —
/// which is why this is a move rather than `NSTitlebarContainerView`-resizing
/// surgery. The same is true across: the run ends at 73 inside a view that is
/// the window's whole width.
///
/// **The band is the caller's, and "centred on it" is the whole rule** (T-MAC-PILL
/// × T-MAC-LIGHTS, the owner's rulings of 2026-09-12, amended 2026-09-13 —
/// `docs/DESIGN.md` §13.48). Folio's own header is 40 and puts the button at
/// `(40 - 14) / 2 = 13`, four points below where macOS had it, and it is 40 in
/// every layout: the vertical ones wore the platform's own 32 for a day and the
/// owner ruled that a header shorter than the strip it replaces is two different
/// windows. So there is no second arm here and no exception — there is one
/// number, and this is the arithmetic it goes through. It remains the caller's
/// and remains a door rather than an argument to `install`, because which header
/// a window wears is a fact about the design and
/// [`WindowButtonsWatch::follow_the_band`] is where the design says it.
fn centre_window_buttons(ns_window: &NSWindow, bar_logical_px: f64, pitch: ButtonPitch) {
    for (index, which) in STANDARD_WINDOW_BUTTONS.into_iter().enumerate() {
        let Some(button) = ns_window.standardWindowButton(which) else {
            continue;
        };
        // SAFETY: a view's own superview, asked on the window's thread — the
        // marker every door in this file passes through. The call is `unsafe`
        // in these bindings because `NSView` is main-thread-only, which is the
        // condition this module has already proved.
        let Some(host) = (unsafe { button.superview() }) else {
            continue;
        };
        let frame = button.frame();
        let above = button_air(bar_logical_px, frame.size.height);
        let y = if host.isFlipped() {
            above
        } else {
            host.bounds().size.height - above - frame.size.height
        };
        let x = above + pitch.0[index];
        // **Written only when it is not already there** (measured 2026-09-12,
        // at the merge with T-MAC-LIGHTS). This runs from `NSWindowDidUpdate`
        // as well now, and a frame *set* inside an update is a reason for
        // another layout and another update: the window would chase itself for
        // as long as it was on screen. Comparing first makes the common case —
        // the buttons are already where this puts them — cost three reads and
        // write nothing, which is what ends the chase.
        if (frame.origin.y - y).abs() > 0.01 || (frame.origin.x - x).abs() > 0.01 {
            button.setFrameOrigin(NSPoint::new(x, y));
        }
    }
}

/// The three, in the order macOS lays them out, named once.
const STANDARD_WINDOW_BUTTONS: [NSWindowButton; 3] = [
    NSWindowButton::CloseButton,
    NSWindowButton::MiniaturizeButton,
    NSWindowButton::ZoomButton,
];

/// **The air a button of this height keeps from the bar it stands in** — and it
/// is one number for both axes, which is the whole of the owner's 2026-09-13
/// ruling (§13.48).
///
/// `(40 - 14) / 2 = 13` at the standard metrics on Folio's own strip, `(32 - 14)
/// / 2 = 9` on the platform's own band. Rounded, so a bar of odd height puts the
/// button on a whole point rather than half of one.
///
/// Pure, and separated from the placement for the reason every other pure half
/// in this file is separated: it is the part that can be wrong without a window.
fn button_air(bar_logical_px: f64, button_height: f64) -> f64 {
    ((bar_logical_px - button_height) / 2.0).round()
}

/// **How far apart macOS itself spaced this window's buttons**, as offsets from
/// the leading one (§13.48).
///
/// Read **once**, before this product moves anything, and carried from there: a
/// pitch re-read off buttons that have already been placed is a pitch that
/// drifts every time the placement is re-stated, and the placement is re-stated
/// on six notifications. `0` for a button this window does not carry, which is
/// also the offset the leading one has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonPitch([f64; 3]);

fn read_button_pitch(ns_window: &NSWindow) -> ButtonPitch {
    let xs = STANDARD_WINDOW_BUTTONS.map(|which| {
        ns_window
            .standardWindowButton(which)
            .map(|b| b.frame().origin.x)
    });
    let lead = xs.iter().flatten().copied().fold(f64::INFINITY, f64::min);
    if !lead.is_finite() {
        return ButtonPitch::default();
    }
    ButtonPitch(xs.map(|x| x.map_or(0.0, |x| x - lead)))
}

/// **The re-application, because AppKit puts them back** (T-MAC-PILL).
///
/// A frame set on a standard window button is not a setting AppKit remembers: it
/// lays the title bar out again on its own schedule and the buttons return to
/// the 32-point bar's axis. So the placement is re-stated on every event that is
/// known to re-lay it — a resize, leaving full screen (where the buttons are
/// taken away and given back), and the key/resign pair, which is where AppKit
/// swaps the buttons' own appearance.
///
/// **Notifications and not a timer.** A timer would be a guess about when AppKit
/// is done, restated sixty times a second for the life of the window; these are
/// the events AppKit posts *after* it has finished, on the thread that did it,
/// and a placement that still will not hold against them is a fact about this
/// platform worth reporting rather than papering over.
///
/// **And the sixth is `NSWindowDidUpdate`, because those five are not all of
/// them** — measured on the Mac on 2026-09-12, at the merge with T-MAC-LIGHTS.
/// The five hold the placement through a resize and a full-screen round trip,
/// and they did not hold it through a launch: the trace shows the frames set to
/// the strip's axis, read back from AppKit at 9 again a moment later, set once
/// more when the window became key — and the window that was then photographed
/// still had them at 9. Something in the display pass lays the title bar out
/// again and posts none of the five. `NSWindowDidUpdate` is posted at the end of
/// every pass in which this window was updated, which is *after* whatever did
/// it, so it is the one subscription that cannot be outrun. It is affordable for
/// the reason [`centre_window_buttons`] is written the way it is: a placement
/// that is already right writes nothing.
///
/// The registration is scoped to this window (`object:`), so a second window's
/// resize does not wake this one's observer.
///
/// **What it re-states is a `Cell` and not the number it was built with**
/// (T-MAC-LIGHTS): the band a window wears follows its layout, the layout
/// changes without a relaunch, and a watch that kept re-applying the band the
/// window opened with would spend the rest of the session dragging the buttons
/// back to the bar the window no longer has. [`Self::follow_the_band`] is the
/// one writer.
///
/// **And on two of those events the measurement itself has changed** (owner
/// ruling 2026-09-13, §13.48). Entering and leaving full screen is the one
/// transition in which macOS takes the three lights off this window and gives
/// them back, and `PlatformChrome` is the sentence "what the platform is drawing
/// in this bar" — so on those two, and only those two, the watch measures the
/// window again and tells the application to lay its bar out against the new
/// answer. Every other event in the list re-states a placement and changes
/// nothing that was measured.
pub struct WindowButtonsWatch {
    observer: Retained<WindowButtonsObserver>,
}

impl WindowButtonsWatch {
    fn install(
        ns_window: &NSWindow,
        bar_logical_px: f64,
        pitch: ButtonPitch,
        chrome: crate::PlatformChrome,
        wake: Box<dyn Fn()>,
    ) -> Self {
        let observer = WindowButtonsObserver::new(WindowButtonsPlacement {
            window: ns_window.retain(),
            bar_logical_px: Cell::new(bar_logical_px),
            pitch,
            chrome: Cell::new(chrome),
            wake,
        });
        let centre = NSNotificationCenter::defaultCenter();
        // SAFETY: AppKit's own notification names, read the way the other
        // subscription in this file reads `NSWorkspace`'s — constants in the
        // framework this process is linked against, never written by anybody.
        //
        // The two full-screen names carry the other selector, which re-measures
        // the window before it re-states the placement: they are the transition
        // in which what was measured has moved. One registration each rather
        // than both selectors on both names, so every notification costs exactly
        // one pass over three buttons.
        let placements = unsafe {
            [
                NSWindowDidResizeNotification,
                NSWindowDidBecomeKeyNotification,
                NSWindowDidResignKeyNotification,
                NSWindowDidUpdateNotification,
            ]
        };
        let measurements = unsafe {
            [
                NSWindowDidEnterFullScreenNotification,
                NSWindowDidExitFullScreenNotification,
            ]
        };
        for (name, selector) in placements
            .into_iter()
            .map(|name| (name, sel!(folioWindowButtonsNeedCentring:)))
            .chain(
                measurements
                    .into_iter()
                    .map(|name| (name, sel!(folioWindowChromeChanged:))),
            )
        {
            // SAFETY: the observer outlives every registration — `Drop` below
            // removes it first — and it answers the selector named here.
            unsafe {
                centre.addObserver_selector_name_object(
                    &observer,
                    selector,
                    Some(name),
                    Some(ns_window),
                );
            }
        }
        Self { observer }
    }

    /// **What the platform is drawing in this window's bar, as this watch last
    /// measured it** (§13.48).
    ///
    /// The live number, and the reason the frame keeps no second copy: a window
    /// that has entered full screen has a different answer from the one taken at
    /// `install`, and two copies of it would be the window drawing its strip
    /// against one and hit-testing it against the other.
    #[must_use]
    pub fn chrome(&self) -> crate::PlatformChrome {
        self.observer.ivars().chrome.get()
    }

    /// **This window wears a different band now** (T-MAC-LIGHTS × T-MAC-PILL,
    /// owner rulings 2026-09-12) — put the buttons on it, and re-state *this*
    /// number from here on.
    ///
    /// Both halves matter and neither is enough alone. Without the move the
    /// buttons stay on the band the window has stopped wearing until AppKit
    /// happens to re-lay the bar; without the write the very next resize would
    /// undo the move, because the observer re-applies whatever it is holding.
    ///
    /// Idempotent by arithmetic rather than by a guard: placing a button on the
    /// band it is already centred on computes the origin it already has. So the
    /// caller is free to say it on every layout decision instead of keeping a
    /// record of what it last said.
    pub fn follow_the_band(&self, band_logical_px: f64) {
        let placement = self.observer.ivars();
        placement.bar_logical_px.set(band_logical_px);
        centre_window_buttons(&placement.window, band_logical_px, placement.pitch);
    }
}

impl Drop for WindowButtonsWatch {
    fn drop(&mut self) {
        // SAFETY: dropped on the thread that installed it (the value is not
        // `Send`), and the registrations are the ones made above.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(&self.observer) };
    }
}

/// What the subscriptions carry: the window whose buttons are being placed, and
/// the bar they are being placed on — the latter in a `Cell`, because the window
/// may change which of its bars it is wearing while it is open
/// (`WindowButtonsWatch::follow_the_band`). No lock: every reader and the one
/// writer are on the window's own thread, which is the thread `NSWindow` is
/// reachable from at all.
struct WindowButtonsPlacement {
    window: Retained<NSWindow>,
    bar_logical_px: Cell<f64>,
    /// **The spacing macOS gave this window's buttons**, read before any of them
    /// was moved (§13.48). Not a `Cell`: it is a fact about the platform's own
    /// layout of this window and nothing that happens later can change it.
    pitch: ButtonPitch,
    /// **What the platform is drawing in this window's bar** (§13.48), kept here
    /// for the reason the band is: it changes while the window is open, and this
    /// is the one object that hears about it. A `Cell` and no lock, for the same
    /// reason — every reader and the one writer are on the window's own thread.
    chrome: Cell<crate::PlatformChrome>,
    /// How this observer asks the event loop for a turn. All an AppKit callback
    /// may do (X-4): the measurement above is a read of AppKit's own state on
    /// AppKit's own thread, and laying a window's bar out again is the loop's.
    wake: Box<dyn Fn()>,
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its ivars do, and the macro's
    //   generated `dealloc` runs them.
    #[unsafe(super(NSObject))]
    #[ivars = WindowButtonsPlacement]
    struct WindowButtonsObserver;

    impl WindowButtonsObserver {
        /// AppKit has just laid this window's title bar out again.
        #[unsafe(method(folioWindowButtonsNeedCentring:))]
        fn buttons_need_centring(&self, _notification: Option<&NSNotification>) {
            let placement = self.ivars();
            centre_window_buttons(
                &placement.window,
                placement.bar_logical_px.get(),
                placement.pitch,
            );
        }

        /// **This window has just gone into full screen or come back out of
        /// it**, which is the transition in which macOS takes its three window
        /// buttons away and gives them back (§13.48).
        ///
        /// So the measurement is taken again — the band is unchanged and the
        /// leading run is not — and the application is asked for a turn, because
        /// the run it leads its strip in by has just moved and nothing else on
        /// this window is going to happen to make it draw. The placement is
        /// re-stated too, exactly as the other four notifications do it: this is
        /// one of the events AppKit lays the bar out on, and the buttons coming
        /// back come back on macOS's own axis.
        #[unsafe(method(folioWindowChromeChanged:))]
        fn chrome_changed(&self, _notification: Option<&NSNotification>) {
            let placement = self.ivars();
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // Placed first and measured second, which is `adopt_window_chrome`'s
            // own order and for its reason (§13.48): the inset this reports is
            // where the buttons end up, and coming out of full screen AppKit has
            // just put them back on its own axis.
            centre_window_buttons(
                &placement.window,
                placement.bar_logical_px.get(),
                placement.pitch,
            );
            if let Some(chrome) = measure_window_chrome(&placement.window, mtm) {
                placement.chrome.set(chrome);
            }
            (placement.wake)();
        }
    }

    unsafe impl NSObjectProtocol for WindowButtonsObserver {}
);

impl WindowButtonsObserver {
    fn new(placement: WindowButtonsPlacement) -> Retained<Self> {
        let this = Self::alloc().set_ivars(placement);
        // SAFETY: `NSObject`'s designated initializer, called on a fresh
        // allocation whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// **Answer a press on the empty part of the window's own title bar** —
/// `performWindowDragWithEvent:`, or the reader's double-click action.
///
/// The macOS counterpart of `HTCAPTION`, and it has to be a call rather than a
/// hit-test answer because there is no hit test to answer: AppKit asks a
/// *view* whether a press may move the window (`mouseDownCanMoveWindow`), and
/// the view under Folio's title bar is winit's, which answers `NO` because it
/// implements `mouseDown:` itself. So `bt-app` decides — it already knows,
/// pixel for pixel, which part of its own strip is empty — and says so here,
/// inside the press.
///
/// **The double click is this door's too, and that is a measurement rather
/// than a preference.** `performWindowDragWithEvent:` is widely described as
/// carrying the standard title-bar behaviours with it, the reader's
/// `AppleActionOnDoubleClick` included, and this ticket wrote it that way
/// first. Measured on macOS 26.6 on 2026-09-12, in a probe with no Folio in it
/// at all — a plain `NSWindow` whose own view calls this selector from
/// `mouseDown:` with `clickCount == 2` — **it does not**: the window stays
/// where it is, while a double click on AppKit's own title bar in the same run
/// zooms the window beside it. So the action is performed here, off the one
/// preference that decides it, and the two verbs are `NSWindow`'s own.
///
/// The event is `NSApp.currentEvent`, which during the press `bt-app` is
/// answering **is** that press. Refused rather than guessed when it is not a
/// left mouse-down: a drag started from some other event is a drag the reader
/// did not begin.
pub fn press_title_bar(window: NativeWindow) -> Result<(), String> {
    let what = "a press on the window's own title bar";
    let (mtm, ns_window) = window_for(window, what)?;
    let application = NSApplication::sharedApplication(mtm);
    let event = application
        .currentEvent()
        .ok_or_else(|| format!("{what}: there is no event in hand to answer"))?;
    if event.r#type() != NSEventType::LeftMouseDown {
        return Err(format!(
            "{what}: the event in hand is not a press, so there is nothing to answer"
        ));
    }
    if event.clickCount() >= 2 {
        return act_on_double_click(&ns_window);
    }
    ns_window.performWindowDragWithEvent(&event);
    Ok(())
}

/// **What a double click on a title bar does on this desk** — the reader's
/// `AppleActionOnDoubleClick`, and nothing of this program's own invention.
///
/// Read through the **standard** user defaults rather than off the preferences
/// file, which is the difference between what the reader chose and what the
/// system does: `defaults read -g AppleActionOnDoubleClick` says the key does
/// not exist on a machine nobody has touched it on, and the standard defaults
/// answer `Maximize` there anyway, because that is the value AppKit registers
/// for itself. Asking the file would have made "the default" into "nothing
/// happens".
///
/// The three values are Apple's own spelling. Anything else — including a
/// nothing nobody registered — is left alone rather than guessed at: a title
/// bar that minimised a window on a setting it did not recognise would be worse
/// than one that did not move.
fn act_on_double_click(window: &NSWindow) -> Result<(), String> {
    let action = NSUserDefaults::standardUserDefaults()
        .stringForKey(ns_string!("AppleActionOnDoubleClick"))
        .map(|value| value.to_string());
    match action.as_deref() {
        Some("Maximize") => {
            window.zoom(None);
            Ok(())
        }
        Some("Minimize") => {
            window.miniaturize(None);
            Ok(())
        }
        Some("None") | None => Ok(()),
        Some(other) => Err(format!(
            "a double click on the title bar: this system asks for `{other}`, which this \
             version does not know how to do"
        )),
    }
}

// ── the system's own preferences (M1-3) ────────────────────────────────────

/// Whether the system is in light mode. `NSApp.effectiveAppearance`, matched
/// against the two named appearances.
///
/// **The application's appearance and not the window's**, which is the reading
/// `resolve_theme_mode` wants: the question is what the *desktop* is set to, and
/// a window whose canvas this program has already stated would answer with its
/// own statement. It is also winit's own reading — `appearance_to_theme` asks
/// `bestMatchFromAppearancesWithNames:` over exactly these two names — so the
/// theme this crate reports and the theme winit reports cannot disagree.
///
/// `None` when the system declares neither, which the theme resolver already
/// handles by taking the product's default.
#[must_use]
pub fn system_uses_light_apps() -> Option<bool> {
    let mtm = window_thread("reading the system's appearance").ok()?;
    let application = NSApplication::sharedApplication(mtm);
    appearance_is_light(&application.effectiveAppearance())
}

/// One appearance, as the light/dark bit — `None` when it is neither.
fn appearance_is_light(appearance: &NSAppearance) -> Option<bool> {
    let aqua = unsafe { NSAppearanceNameAqua };
    let dark = unsafe { NSAppearanceNameDarkAqua };
    let names = NSArray::from_slice(&[aqua, dark]);
    let best = appearance.bestMatchFromAppearancesWithNames(&names)?;
    Some(&*best != dark)
}

/// Whether the system wants animation inside a window.
///
/// `accessibilityDisplayShouldReduceMotion` is macOS's name for the preference
/// Windows spells `SPI_GETCLIENTAREAANIMATION` and a browser spells
/// `prefers-reduced-motion` — System Settings → Accessibility → Display →
/// Reduce motion. The polarity is Windows': `true` means animation is *wanted*,
/// and the caller does the mapping.
pub fn client_area_animation_enabled() -> Result<bool, String> {
    let _ = window_thread("the reduce-motion preference")?;
    Ok(!NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion())
}

/// How far one wheel notch scrolls.
///
/// **One line, and it is a statement about this platform rather than a default.**
/// macOS has no lines-per-notch preference to read: the window server applies
/// the reader's own scroll speed (`com.apple.scrollwheel.scaling`) to the event
/// before an application sees it, and hands over `scrollingDeltaY` already
/// expressed in lines for a wheel that is not a precise device. winit passes
/// that number through as `MouseScrollDelta::LineDelta`, so the caller's
/// `lines × line-height` arithmetic is already the distance the system asked
/// for; any multiplier but one would apply the reader's preference twice.
///
/// A trackpad does not come this way at all — it is precise, and winit reports
/// `PixelDelta`, which the caller spends without asking this door anything.
pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
    Ok(WheelScrollAmount::Lines(1))
}

/// The language the operating system is being read in.
///
/// `NSLocale.preferredLanguages`' first entry — a BCP-47 tag such as `en-GB` or
/// `zh-Hans-CN`, which is the shape `i18n::resolve` reads a primary subtag out
/// of. The reader's *language*, which is the fact this door is about, and
/// deliberately not `AppleLocale`: that one is the formatting locale
/// `system_locale_declaration` already reads for a child process's locale, and
/// the two are allowed to differ.
///
/// `en` when the system names none, which is the portable arm's answer and the
/// product's own default.
#[must_use]
pub fn os_ui_language() -> String {
    let Ok(_) = window_thread("reading the system's language") else {
        return "en".to_owned();
    };
    NSLocale::preferredLanguages()
        .firstObject()
        .map_or_else(|| "en".to_owned(), |tag| tag.to_string())
}

// ── the system settings watch (M1-3) ───────────────────────────────────────

/// **The system saying a preference this program reads has moved.**
///
/// The twin of the `WM_SETTINGCHANGE` subclass, and it watches the same two
/// facts that page changes: the appearance (light/dark) and the reduce-motion
/// switch. Two subscriptions rather than one broadcast, because macOS has no
/// single "something in Settings moved" message:
///
/// * **KVO on `NSApplication.effectiveAppearance`** — the application's
///   appearance, which no window's own `appearance` override touches (see
///   [`set_window_dark_mode`] for why that matters: winit stops posting
///   `ThemeChanged` for a window whose canvas this program has stated, and this
///   is what replaces it). AppKit recomputes it when the desktop switches, so
///   this fires for exactly the change the reader made.
/// * **`NSWorkspace`'s accessibility-display notification** — the one the system
///   posts when Reduce Motion and its neighbours move.
///
/// **What the callback may do is nudge, and nothing else.** It runs inside
/// AppKit's own delivery, on a turn this program did not choose; the reading
/// itself belongs to the event loop, and the wake `bt-app` hands in is one
/// `EventLoopProxy::send_event` — winit's user-event channel, delivered on the
/// window thread. That is X-4's rule for every AppKit callback in this port, and
/// `dark_mode_changes_arrive_on_the_window_thread_through_winit` pins it.
///
/// One watch per window, as on Windows: the notification reaches every observer
/// anyway and a duplicate wake costs one re-read on a turn that was going to
/// happen.
pub struct SystemSettingsWatch {
    /// The observer both subscriptions are registered with. Held because
    /// unregistering needs it and because nothing else owns it.
    observer: Retained<SystemSettingsObserver>,
    /// The object the appearance is observed on. Held for the same reason: the
    /// `removeObserver:` has to name it, and reaching for `sharedApplication`
    /// again in `Drop` would need a thread marker `Drop` cannot ask for.
    application: Retained<NSApplication>,
}

impl SystemSettingsWatch {
    /// Install the watch.
    ///
    /// The window is taken and not used, exactly as the Windows arm takes an
    /// `HWND` it subclasses: there the broadcast arrives *at* a window, here it
    /// arrives at the application, and the door's shape is the caller's — one
    /// watch per window, dropped with it.
    pub fn install(window: NativeWindow, wake: Box<dyn Fn()>) -> Result<Self, String> {
        let _ = window;
        let mtm = window_thread("watching the system's preferences")?;
        let observer = SystemSettingsObserver::new(wake);
        let application = NSApplication::sharedApplication(mtm);
        // SAFETY: the observer outlives the registration — `Drop` below removes
        // it first — it answers `observeValueForKeyPath:ofObject:change:context:`,
        // and the context pointer is null, which is what `removeObserver:forKeyPath:`
        // below matches.
        unsafe {
            application.addObserver_forKeyPath_options_context(
                &observer,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::New,
                std::ptr::null_mut::<c_void>(),
            );
        }
        // SAFETY: the observer outlives the registration for the same reason,
        // and it answers the selector named here.
        unsafe {
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserver_selector_name_object(
                    &observer,
                    sel!(folioSystemPreferencesChanged:),
                    Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                    None,
                );
        }
        Ok(Self {
            observer,
            application,
        })
    }
}

impl Drop for SystemSettingsWatch {
    fn drop(&mut self) {
        // SAFETY: dropped on the thread that installed it (the value is not
        // `Send`), and both registrations are the ones made above.
        unsafe {
            self.application
                .removeObserver_forKeyPath(&self.observer, ns_string!("effectiveAppearance"));
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .removeObserver(&self.observer);
        }
    }
}

/// What the two subscriptions call, and all it may do.
struct SystemSettingsWake(Box<dyn Fn()>);

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - This class does not implement `Drop`; its ivars do, and the macro's
    //   generated `dealloc` runs them.
    #[unsafe(super(NSObject))]
    #[ivars = SystemSettingsWake]
    struct SystemSettingsObserver;

    impl SystemSettingsObserver {
        /// The appearance changed. One key path is observed and the guard says
        /// so rather than assuming it: another subscription on this object
        /// later would otherwise silently answer for this one.
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSString, AnyObject>>,
            _context: *mut c_void,
        ) {
            if key_path == Some(ns_string!("effectiveAppearance")) {
                (self.ivars().0)();
            }
        }

        /// An accessibility display option moved.
        #[unsafe(method(folioSystemPreferencesChanged:))]
        fn preferences_changed(&self, _notification: Option<&NSNotification>) {
            (self.ivars().0)();
        }
    }

    unsafe impl NSObjectProtocol for SystemSettingsObserver {}
);

impl SystemSettingsObserver {
    fn new(wake: Box<dyn Fn()>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(SystemSettingsWake(wake));
        // SAFETY: `NSObject`'s designated initializer, called on a fresh
        // allocation whose ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **RED — the platform's run is a rectangle, and both of its numbers are
    /// this window's own** (T-MAC-LIGHTS).
    ///
    /// The numbers are macOS 26.6's, measured on 2026-09-12 and recorded in
    /// `docs/DESIGN.md` §13.11: the zoom button's right edge at 69 points and a
    /// 32-point title bar. What is pinned here is not those two values — the
    /// window is asked for them — but the arithmetic that carries them into the
    /// physical pixels every layout downstream measures in.
    ///
    /// MUTATION: round the band to nearest instead of up and the fractional
    /// case goes red at 31; drop the `* scale` and the 2× case does.
    #[test]
    fn the_platforms_run_is_measured_on_both_axes_at_the_windows_own_scale() {
        let at_one = platform_chrome_of(69.0, 32.0, 1.0);
        assert_eq!(
            (at_one.strip_left_px, at_one.band_px),
            (69, 32),
            "at backing scale 1 the run's two numbers are the points themselves"
        );
        assert!(
            at_one.buttons_are_the_platforms,
            "a window with a run of its own is a window whose buttons are the platform's"
        );
        let at_two = platform_chrome_of(69.0, 32.0, 2.0);
        assert_eq!(
            (at_two.strip_left_px, at_two.band_px),
            (138, 64),
            "and on a 2x display it is the same bar in twice as many pixels"
        );
        // Up on both axes and for one reason: what stands after the run may
        // begin one pixel clear of it, never one pixel inside it.
        let fractional = platform_chrome_of(68.5, 31.5, 1.0);
        assert_eq!((fractional.strip_left_px, fractional.band_px), (69, 32));
    }

    /// **RED — the red light's centre lands on the window corner's diagonal, and
    /// the other two keep macOS's own pitch** (owner ruling 2026-09-13, §13.48).
    ///
    /// The rule is an identity rather than a pair of numbers: the air a button
    /// keeps from the top of the bar is the air the leading one keeps from the
    /// leading edge, so its centre sits on `x = y` — the 45° line of a corner
    /// rounded at 12.1 points, whose circle centres near `(12, 12)`. On Folio's
    /// 40-point strip that is 13, so the light's centre is `(20, 20)`; on the
    /// platform's own 32-point band it is 9, which is where macOS had it, and
    /// that is why there is no second arm in the placement.
    ///
    /// The pitch is asserted as *macOS's own* and not as 23: `read_button_pitch`
    /// takes the offsets off a window nothing has moved, and this asserts that
    /// whatever they were they are carried, which is the difference between a
    /// run that follows the platform and a run that restates a number measured
    /// on one release.
    ///
    /// MUTATION: leave `origin.x` alone and the first triple goes red; write 23
    /// here instead of carrying the pitch and the last assertion does; use the
    /// bar's *width* anywhere in `button_air` and the 32-point case does.
    #[test]
    fn the_leading_light_sits_on_the_corners_diagonal_and_the_rest_keep_the_pitch() {
        // macOS 26.6's own layout, measured 2026-09-12 (§13.11): 9 / 32 / 55,
        // 14 across, a pitch of 23.
        let pitch = ButtonPitch([0.0, 23.0, 46.0]);
        let place = |bar: f64| {
            let air = button_air(bar, 14.0);
            [air + pitch.0[0], air + pitch.0[1], air + pitch.0[2]]
        };
        assert_eq!(
            place(40.0),
            [13.0, 36.0, 59.0],
            "on Folio's own strip the run is 13 / 36 / 59 and ends at 73"
        );
        assert_eq!(
            button_air(40.0, 14.0),
            20.0 - 14.0 / 2.0,
            "which is the same air the button keeps from the top, so its centre \
             is (20, 20) — the corner's own diagonal"
        );
        assert_eq!(
            place(32.0),
            [9.0, 32.0, 55.0],
            "and on the platform's own band it is where macOS itself had them"
        );
        // A bar of odd height puts the button on a whole point, not half of one.
        assert_eq!(button_air(41.0, 14.0), 14.0);
        // Whatever the pitch was, it is the pitch that is carried.
        let other = ButtonPitch([0.0, 20.0, 40.0]);
        let air = button_air(40.0, 14.0);
        assert_eq!(
            [air + other.0[0], air + other.0[1], air + other.0[2]],
            [13.0, 33.0, 53.0],
            "the spacing is read off the window and not written down here"
        );
    }

    /// **RED — a window whose buttons have been taken off it still has a band**
    /// (owner ruling 2026-09-13, §13.48).
    ///
    /// Full screen is macOS withdrawing the three traffic lights until the
    /// pointer reaches the top edge, and leaving the header they stood in. The
    /// measurement that comes out of that window is a run with a height and no
    /// width, and it is *not* `FOLIO_DRAWS_THE_WHOLE_BAR`: the buttons are still
    /// this window's, and the header is still there to be drawn in. What changes
    /// is that nothing stands at the leading edge for Folio's first control to
    /// keep clear of.
    ///
    /// MUTATION: answer the withdrawn window with `FOLIO_DRAWS_THE_WHOLE_BAR`
    /// and the band goes to zero here; keep the inset of a button that is off
    /// the screen and the first assertion does.
    #[test]
    fn a_window_whose_buttons_are_withdrawn_keeps_its_band() {
        let withdrawn = platform_chrome_of(0.0, 32.0, 2.0);
        assert_eq!(
            withdrawn.strip_left_px, 0,
            "there is nothing at the leading edge to stand clear of"
        );
        assert_eq!(
            withdrawn.band_px, 64,
            "and the header the buttons stood in is still the header"
        );
        assert!(
            withdrawn.buttons_are_the_platforms,
            "they are still this window's buttons; they are off the screen"
        );
        assert_ne!(
            withdrawn,
            crate::PlatformChrome::FOLIO_DRAWS_THE_WHOLE_BAR,
            "which is a different window from one that never had any"
        );
    }

    /// The rectangle a window reports and the rectangle winit reports are the
    /// same rectangle, because they are the same arithmetic.
    ///
    /// The numbers are the Mac mini's own desk: a 4K at backing scale 2 as the
    /// zero screen (1920×1080 points), and a 1280×720 panel at scale 1 beside
    /// it. A window 960×600 points at the origin of each.
    ///
    /// MUTATION: drop the `* scale` from either half and the 2× display goes
    /// red; flip the subtraction in the flip and the `top` goes red.
    #[test]
    fn window_geometry_is_physical_pixels_at_the_windows_own_scale() {
        let flip = 1080.0;
        let on_the_retina = physical_rect(
            NSRect::new(NSPoint::new(0.0, 480.0), NSSize::new(960.0, 600.0)),
            flip,
            2.0,
        );
        assert_eq!(
            on_the_retina,
            WindowRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1200,
            },
            "a window at the top-left of the 2x display is 1920x1200 physical pixels"
        );
        let on_the_plain = physical_rect(
            NSRect::new(NSPoint::new(1920.0, 360.0), NSSize::new(960.0, 600.0)),
            flip,
            1.0,
        );
        assert_eq!(
            on_the_plain,
            WindowRect {
                left: 1920,
                top: 120,
                right: 2880,
                bottom: 720,
            },
            "the same window on the 1x panel is 960x600 physical pixels"
        );
    }

    /// And the conversion back is the one that lands on the frame it came from.
    ///
    /// MUTATION: round in `appkit_frame` and a window at an odd physical
    /// coordinate on a 2x display stops coming back.
    #[test]
    fn a_rectangle_stated_and_read_back_is_the_rectangle_that_was_asked_for() {
        let flip = 1080.0;
        for scale in [1.0, 2.0] {
            let asked = WindowRect {
                left: 37,
                top: 101,
                right: 1337,
                bottom: 901,
            };
            let frame = appkit_frame(asked, flip, scale);
            assert_eq!(
                physical_rect(frame, flip, scale),
                asked,
                "a rectangle stated at scale {scale} reads back as itself"
            );
        }
    }

    /// **RED (M3-4) — a display is named by its UUID, and that name is not its
    /// display number.**
    ///
    /// The one door in this file that needs no window and no main thread:
    /// CoreGraphics answers a display number from anywhere, which is what lets
    /// the identity half of `monitor_id_at` be a unit case rather than only a
    /// line in an `.app` run. Every Mac has a main display, so the case has a
    /// subject on any machine this suite runs on.
    ///
    /// Four claims, and the third is the ticket's own:
    ///
    /// ① the main display **has** a name — a machine whose displays cannot be
    ///    named is a machine where every quake arrangement is forgotten;
    /// ② the name is `CFUUID`'s canonical spelling, 8-4-4-4-12 upper-case hex,
    ///    which is what makes it recognisable in a session file by eye;
    /// ③ it is **not** the decimal display number and cannot be read as one —
    ///    M1-3's answer was that number, and a build that fell back to it would
    ///    key the file on a handle the window server reissues;
    /// ④ asking twice in one process gives one answer, because a key that is not
    ///    a function of the display is not a key.
    ///
    /// MUTATION: answer `display_number(&screen).to_string()` again and ② and ③
    /// both go red.
    #[test]
    fn a_display_is_named_by_its_uuid_and_never_by_its_display_number() {
        let display = objc2_core_graphics::CGMainDisplayID();
        let name = display_uuid(display).expect("the main display has a UUID");
        let groups: Vec<&str> = name.split('-').collect();
        assert_eq!(
            groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "a display's name is a UUID in its canonical spelling: {name}"
        );
        assert!(
            groups.iter().all(|group| group
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase() && c.is_ascii_hexdigit())),
            "and it is upper-case hexadecimal: {name}"
        );
        assert_ne!(
            name,
            display.to_string(),
            "the name is not the display number"
        );
        assert!(
            name.parse::<u32>().is_err(),
            "and nothing can read it as one: {name}"
        );
        assert_eq!(
            display_uuid(display).as_deref(),
            Some(name.as_str()),
            "one display has one name"
        );
    }

    /// The work area is the one of the display the window is on, and the point
    /// lookup resolves to the display that holds it.
    ///
    /// The lookup is the pure half — the containment and the nearest-display
    /// tie-break — because the impure half needs a desktop.
    ///
    /// MUTATION: make `holds` inclusive on the far edge and the seam goes to
    /// the wrong display; drop the `distance_outside` ordering and a point off
    /// the desktop stops resolving to its nearest.
    #[test]
    fn the_work_area_is_the_screen_the_window_is_on() {
        let retina = WindowRect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        let plain = WindowRect {
            left: 1920,
            top: 0,
            right: 3200,
            bottom: 720,
        };
        assert!(holds(retina, 0, 0), "the origin is on the zero screen");
        assert!(
            !holds(retina, 3840, 0),
            "the far edge belongs to the next display, not this one"
        );
        assert!(holds(plain, 1920, 0), "and the next display holds it");
        assert_eq!(
            distance_outside(retina, 0, 0),
            0,
            "a point inside is nowhere outside"
        );
        assert!(
            distance_outside(plain, -10, -10) > distance_outside(retina, -10, -10),
            "a point off the top-left of the desktop is nearest the zero screen"
        );
    }

    /// A backing scale is a dpi the way `bt-app` reads one, both ways.
    #[test]
    fn a_backing_scale_is_the_dpi_bt_app_divides_by() {
        assert_eq!(dpi_for_scale(1.0), 96);
        assert_eq!(dpi_for_scale(2.0), 192);
        assert_eq!(
            f64::from(dpi_for_scale(2.0)) / 96.0,
            2.0,
            "and it divides back to the scale exactly"
        );
        assert_eq!(
            dpi_for_scale(0.0),
            96,
            "a display that answers nothing is scale 1, not scale 0"
        );
    }

    /// **Every door in this file refuses when it is asked from a thread that is
    /// not the window's**, rather than calling AppKit from it.
    ///
    /// The case runs on a thread the harness made, which is not the main thread,
    /// so the whole module is in exactly the state it is guarding against.
    ///
    /// MUTATION: drop the `window_thread` gate from any door below and it
    /// reaches AppKit off the main thread — which is undefined behaviour rather
    /// than a failing assertion, and is why the gate is a refusal every door
    /// passes through rather than a comment.
    #[test]
    fn a_window_door_asked_off_the_window_thread_refuses() {
        assert!(
            MainThreadMarker::new().is_none(),
            "a test harness thread is not the main thread; this case has nothing to say otherwise"
        );
        let window = NativeWindow::stand_in(0);
        assert!(get_window_rect(window).is_err(), "the window's rectangle");
        assert!(
            set_window_outer_rect(
                window,
                WindowRect {
                    left: 0,
                    top: 0,
                    right: 1,
                    bottom: 1
                }
            )
            .is_err(),
            "placing a window"
        );
        assert!(get_work_area(window).is_err(), "the work area");
        assert!(work_area_at(0, 0).is_err(), "the work area at a point");
        assert!(get_dpi_for_window(window).is_err(), "the backing scale");
        assert!(request_window_close(window).is_err(), "asking it to close");
        assert!(take_keyboard_focus(window).is_err(), "the keyboard");
        assert!(set_window_topmost(window, true).is_err(), "the level");
        assert!(
            set_window_dark_mode(window, true).is_err(),
            "the appearance"
        );
        assert!(
            install_window_class_background(window, None).is_err(),
            "the backing colour"
        );
        assert!(
            client_area_animation_enabled().is_err(),
            "the reduce-motion preference"
        );
        assert!(
            SystemSettingsWatch::install(window, Box::new(|| {})).is_err(),
            "the settings watch"
        );
        assert_eq!(dpi_at(0, 0), 96, "a dpi with no display to read is scale 1");
        assert_eq!(monitor_id_at(0, 0), None, "no display has a name");
        assert_eq!(pointer_position(), None, "the pointer is nowhere");
        assert_eq!(
            top_level_window_at(0, 0),
            None,
            "no window is under a point"
        );
        assert!(!is_window_minimized(window), "and it is not minimised");
        assert!(window_is_exposed(window), "and it is reported showing");
        assert_eq!(
            virtual_screen_rect(),
            WindowRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0
            },
            "and the desktop has no extent"
        );
        assert_eq!(os_ui_language(), "en", "and the system names no language");
        assert_eq!(system_uses_light_apps(), None, "and no appearance");
    }

    /// The wheel's answer is one line, and it is a statement rather than a
    /// fallback: the caller never sees the error branch on this platform.
    #[test]
    fn one_notch_is_one_line_because_the_system_has_already_scaled_it() {
        assert_eq!(wheel_scroll_amount(), Ok(WheelScrollAmount::Lines(1)));
    }
}
