//! **`BT_MOUSE_TRACE` — one named file, one line per station on a click's road.**
//!
//! A click on a hyperlink in a terminal pane travels through six surfaces before
//! anything opens: the window router, the chrome's own hit test, the mouse route
//! a press arms, the release that spends what the press promised, the activation
//! table, and the landing rule that has to mint a pane for the file to arrive in.
//! Every one of those can decline, and — until this file existed — every one of
//! them declined **silently**. A user on a second monitor reporting "the click
//! does nothing" was reporting the absence of six different possible sentences,
//! and no build could tell which one was missing.
//!
//! So this is forensic apparatus and nothing else: **it changes no behaviour**.
//! Every station writes what it decided and why, and the file is the transcript
//! of a gesture that can be carried back from a machine we do not have.
//!
//! **Named like [`BT_PTY_DUMP`](bt_pty) — the value is a *file*, not a folder.**
//! Handing a directory to that one reports Access denied dressed up as a ConPTY
//! failure; this one says so plainly on stderr and then stays off, because a
//! diagnostic that takes the program down with it is worse than the silence it
//! was built to end.
//!
//! **Off costs one atomic load**, and the file, the clock and the closure gate
//! all live in [`crate::trace`] now — this module is the variable's name, its
//! header, and the stations that call it.
//!
//! # The wheel's own road (§7.60)
//!
//! The stations above were a *button's*. A notch of the wheel travels
//! `MouseWheel → queue_wheel → flush_wheel → mouse_wheel → rail_contains →
//! scroll_rail → aim_focus_card_window`, and until this ticket **not one line of
//! it was written down** — so a report that `Alt`+wheel over a card had stopped
//! aiming could be answered by a dozen surfaces in this window and no log could
//! say which. The vocabulary for that road lives in the second half of this
//! file: one builder per line, each a pure function of what its station read, so
//! that the format is a thing a test can hold rather than a format string
//! scattered over `main.rs`.

use crate::seats;
use crate::trace::Gate;
use winit::event::MouseScrollDelta;

pub use crate::trace::{Trace, emit};

/// The variable that names the file, and the first line of every opened trace so
/// that a file which has collected several runs can still be told what it is and
/// where each run began.
static GATE: Gate = Gate::new(
    "BT_MOUSE_TRACE",
    "# BT_MOUSE_TRACE_V1 elapsed_ms event field=value…",
);

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    GATE.get()
}

/// Whether anything is listening — for the handful of call sites that must
/// *compute* a field (a hit test, say) rather than merely format one.
pub fn is_on() -> bool {
    global().is_some()
}

/// [`emit`] against the process's own trace — what every station calls.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}

/// [`line`], stamped with a window whose id was read **before** any borrow.
///
/// `Runtime::mouse_trace` is the ordinary door and takes `&self`; a station
/// standing inside a live `&mut` borrow of this window's own tabs — which is
/// where the aim spends its notch — cannot take one. So the id is read once at
/// the top of such a function, into a `u64`, and the line is written through
/// here. Same prefix, same file, no borrow.
pub fn window_line(window: u64, message: impl FnOnce() -> String) {
    line(|| format!("window={window} {}", message()));
}

/// **A wheel report in the currency the driver reported it in.**
///
/// The two are not interchangeable and the routes read which one they were
/// given, so a trace that normalised them would erase the one fact that tells a
/// high-resolution wheel from a notched one.
#[must_use]
pub fn delta_word(delta: MouseScrollDelta) -> String {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => format!("lines:{x},{y}"),
        MouseScrollDelta::PixelDelta(at) => format!("pixels:{},{}", at.x, at.y),
    }
}

/// A point, or the word for a window that has not got one.
///
/// `none` is a finding and not a gap: an absent `pointer_position` is exactly
/// the shape of "the hand has not moved since this window was resized", which is
/// one of the readings this apparatus exists to tell from the others.
#[must_use]
pub fn point_word(at: Option<(f64, f64)>) -> String {
    at.map_or_else(|| "none".to_owned(), |(x, y)| format!("{x},{y}"))
}

/// A rectangle `[left, top, right, bottom]` as one field's value.
#[must_use]
pub fn rect_word(rect: [f32; 4]) -> String {
    let [left, top, right, bottom] = rect;
    format!("{left},{top},{right},{bottom}")
}

/// **The wheel's first station: what the window was wearing when the notch
/// arrived.**
///
/// Every field here is read once, before any router has had a chance to move it
/// — `mouse_input`'s own opening station, in the wheel's words. The pointer and
/// the three geometries are the pair a resize can put out of step; the
/// modifiers are what [`crate::column_notch`] reads; `events` and `routings` are
/// the window's own two counters, and they are what pairs this line with the
/// `wheel_queue` lines above it that were merged into the delta it carries.
pub struct WheelEntry {
    pub pointer: Option<(f64, f64)>,
    pub pointer_last_seen: Option<(f64, f64)>,
    pub swapchain: (u32, u32),
    pub inner: (u32, u32),
    pub metrics_scale: f64,
    /// What [`crate::WheelBurst`] handed on, which is what every route below is
    /// asked about.
    pub flushed: MouseScrollDelta,
    /// The same report in detents — the number the aim actually spends.
    pub notches: f32,
    pub events: u64,
    pub routings: u64,
    pub alt: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub focus_mode: bool,
}

impl WheelEntry {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "mouse_wheel flushed_delta={} notches={} pointer={} pointer_last_seen={} \
             metrics_scale={} swapchain_size={}x{} inner_size={}x{} alt={} shift={} ctrl={} \
             focus_mode={} events={} routings={}",
            delta_word(self.flushed),
            self.notches,
            point_word(self.pointer),
            point_word(self.pointer_last_seen),
            self.metrics_scale,
            self.swapchain.0,
            self.swapchain.1,
            self.inner.0,
            self.inner.1,
            u8::from(self.alt),
            u8::from(self.shift),
            u8::from(self.ctrl),
            u8::from(self.focus_mode),
            self.events,
            self.routings,
        )
    }
}

/// One column's geometry, under a prefix — `aim_…` or `paint_…`.
fn column_words(prefix: &str, height: f32, geometry: Option<&seats::FocusRailGeometry>) -> String {
    match geometry {
        None => format!("{prefix}_height={height} {prefix}=none"),
        Some(geometry) => format!(
            "{prefix}_height={height} {prefix}_body={} {prefix}_viewport={},{} \
             {prefix}_cards={} {prefix}_max_scroll={}",
            rect_word(geometry.body),
            geometry.viewport[0],
            geometry.viewport[1],
            geometry.cards.len(),
            geometry.max_scroll,
        ),
    }
}

/// **The rail's own decision, with the painted column beside the aimed one.**
///
/// The two columns are solved from two different heights — the last chrome
/// rebuild's surface ([`seats::chrome_surface_height`]) and the swapchain's
/// (`Runtime::focus_rail_geometry_now`) — and nothing in the program had ever
/// compared them. `agree` is that comparison, made once and printed, so a
/// paint-versus-aim disagreement is one field rather than an arithmetic a reader
/// has to do on two lines of numbers.
pub struct WheelRail<'a> {
    /// What `rail_contains` answered — the gate the whole card road is behind.
    pub contains: bool,
    pub point: (f64, f64),
    pub rail_scroll: f32,
    /// The vertical rail's own run, when this window is wearing one rather than
    /// a card column: `rail_contains` asks it first.
    pub strip_rail: Option<[f32; 4]>,
    pub aim_height: f32,
    pub aim: Option<&'a seats::FocusRailGeometry>,
    pub paint_height: f32,
    pub paint: Option<&'a seats::FocusRailGeometry>,
}

impl WheelRail<'_> {
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "wheel_rail contains={} point={} rail_scroll={} rail_run={} {} {} agree={}",
            u8::from(self.contains),
            point_word(Some(self.point)),
            self.rail_scroll,
            self.strip_rail
                .map_or_else(|| "none".to_owned(), rect_word),
            column_words("aim", self.aim_height, self.aim),
            column_words("paint", self.paint_height, self.paint),
            u8::from(self.aim == self.paint),
        )
    }
}

/// **What one `Alt`+notch did to the card it was turned over.**
///
/// The fields are the ones that tell a stale aim target from a projection that
/// moved and drew nothing: the transient card index beside the stable
/// `LeafId` the carry is filed under, the carry before and after, and the
/// targeted leaf's `card_skip` on both sides of the spend.
pub struct WheelAim {
    /// The card the pointer is over, freshly walked — an index into the visible
    /// column, which is not a tab count and not a stored number.
    pub index: usize,
    /// `TabId`, as it prints.
    pub tab: String,
    /// The mini `SeatId` under the point.
    pub seat: String,
    /// `CardAim.at` **before** the spend, which is the identity a carry is kept
    /// under; `none` when nothing was carried. Spelled `TabId(n)/SeatId(n)`,
    /// because `LeafId`'s own `Debug` has braces and blanks in it and a value
    /// with a blank in it is not a value on a `key=value` line.
    pub at_before: Option<String>,
    pub carried_before: Option<MouseScrollDelta>,
    pub steps: i32,
    pub carried_after: Option<MouseScrollDelta>,
    pub skip_before: usize,
    pub skip_after: usize,
}

impl WheelAim {
    #[must_use]
    pub fn line(&self) -> String {
        let carry = |delta: Option<MouseScrollDelta>| {
            delta.map_or_else(|| "none".to_owned(), delta_word)
        };
        format!(
            "wheel_aim leave={} index={} tab={} seat={} at_before={} carried_before={} \
             steps={} carried_after={} card_skip_before={} card_skip_after={}",
            if self.skip_after == self.skip_before {
                // A notch that completed no detent, or one the end of the
                // scrollback swallowed: both are "the aim took it and nothing
                // moved", and the two numbers beside them say which.
                "carried"
            } else {
                "aimed"
            },
            self.index,
            self.tab,
            self.seat,
            self.at_before.as_deref().unwrap_or("none"),
            carry(self.carried_before),
            self.steps,
            carry(self.carried_after),
            self.skip_before,
            self.skip_after,
        )
    }
}

/// **Which surface took the notch home** — the one word a report that "the
/// gesture does nothing" is read off.
///
/// The vocabulary is closed on purpose and
/// `mouse_trace_station_tests` holds it closed: a `taken=` word nobody
/// declared here is a surface nobody thought about, and that is the exact shape
/// of the silence this whole apparatus exists to end. Which *one* of a surface's
/// several doors answered is the line's `at=`, never a new word.
///
/// **`cfg(test)`, like `pointer_chord_site_tests`' own list of doors**: the
/// stations write their word as a literal beside the decision it describes, and
/// this is the declaration those literals are checked against. A copy compiled
/// into the product would be a second list for somebody to forget.
#[cfg(test)]
pub const WHEEL_ROUTES: [&str; 10] = [
    // The card column's aim spent it on a mini window (`Alt`+wheel).
    "rail-aim",
    // The card column or the vertical rail scrolled — or would have.
    "rail-scroll",
    // The horizontal tab strip.
    "tab-strip",
    // A surface standing over the panes: the hover card, the first-run card,
    // the settings sheet, a notice, the palette, a files float.
    "overlay",
    // A hosted web page.
    "page",
    // A pane's own body that is not a terminal: a files tree, a git page, a
    // graph, a preview body, a picture's zoom.
    "pane",
    // A terminal the **pointer** was over, scrolling its own window.
    "terminal-pane",
    // A terminal the window fell back to because the pointer named none.
    "focused-leaf-fallback",
    // Forwarded to a shell, as mouse reports or as arrow keys.
    "pty",
    // Swallowed on purpose: no surface under it wanted it.
    "nobody",
];
