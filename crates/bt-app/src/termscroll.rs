//! **The scroll bar a terminal pane wears in the lane reserved for it**
//! (P2-9 slice 1).
//!
//! # The lane was declared before the instrument
//!
//! [`bt_render::TERMINAL_SCROLL_LANE_LOGICAL_PX`] has held eight logical pixels
//! of every terminal pane's right edge since D-14, against the day this file
//! would exist, and its doc comment says why: the mock-up's own stylesheet
//! carries an accident report — *"the rail and the thumb are different
//! instruments and may not share a lane (user report 2026-07-18 — ticks sat on
//! top of the thumb)"* — so the command rail measures its inset from the lane
//! rather than from the pane. This module is the other half of that arithmetic.
//! Everything it draws and everything it reaches for is inside
//! `lane + `[`cmdrail::RAIL_LANE_GAP_LOGICAL_PX`], which is exactly where the
//! rail's own box stops. The two instruments stand side by side and neither can
//! be moved without the other, because both are derived from the one constant.
//!
//! # A mark on the content, not a piece of furniture
//!
//! The whole of the design's scroll bar is two lines of the mock-up (`design/
//! ui-mockup.html` 86-95): `scrollbar-width: thin` with
//! `scrollbar-color: var(--thumb) transparent`, under a comment that says what
//! they are for — *"the UA dark bar is still a chunky opaque thing, and a
//! scrollbar in a terminal should be a mark on the text, not a piece of
//! furniture beside it"*. Three things follow, and all three are here rather
//! than in a stylesheet nobody executes:
//!
//! * **There is no track.** `transparent` is stated, not defaulted. What is
//!   drawn is a thumb and nothing else; the lane below it shows the terminal.
//! * **Hover brightens, it does not widen.** `thin` is declared once for every
//!   state the design has. A bar that grew under the pointer would cover the
//!   right-most character cell, which is the thing the reserved lane exists to
//!   stop happening.
//! * **It is not always there.** An overlay bar that never leaves is furniture
//!   again; see [`visibility`] for the four facts that decide.
//!
//! # One derivation, in somebody else's units
//!
//! The along-axis arithmetic — the proportional length, the floor under it, the
//! travel that remains, and the linear map a drag reads backwards — is
//! [`preview::scroll_bar`] and [`preview::scroll_dragged_to`], called and not
//! copied. That module's own doc states the rule this obeys: *"copying it into a
//! second function is how two scrollbars that are the same scrollbar drift
//! apart"*. What this module adds is the cross-axis — a lane, where the preview
//! draws an overlay rule — and a **change of units**: a preview scrolls in
//! pixels and a terminal scrolls in [`bt_viewport`] subpixels, so the two
//! numbers the projection owns ([`bt_viewport::ViewportProjection::scroll_extent_subpixels`]
//! and its offset) are converted once, at this boundary, and converted back by
//! the same factor when a hand puts them down somewhere else.
//!
//! # What this slice is not
//!
//! P2-9 also owns the capacity expansion, regex search and no-wrap horizontal
//! scrolling. None of them is here. This is the vertical bar, and the only claim
//! it makes about the scrollback is how far it goes.

use std::time::{Duration, Instant};

use bt_render::{ChromePalette, TERMINAL_SCROLL_LANE_LOGICAL_PX};

use crate::Motion;
use crate::cmdrail::RAIL_LANE_GAP_LOGICAL_PX;
use crate::marks::OverlayLayer;
use crate::preview::{self, ScrollAxis, ScrollBar};

/// How wide the thumb is **drawn**, inside the eight-pixel lane.
///
/// `scrollbar-width: thin` is a keyword rather than a number, and the number
/// browsers give it is the gutter's — the mock-up's own comment reads it as
/// "thin ≈ 8px", which is where [`TERMINAL_SCROLL_LANE_LOGICAL_PX`] came from.
/// The mark inside that gutter is narrower than the gutter in every engine that
/// draws one, and it has to be here too: a fill that took the whole reserved
/// band would be a rule down the pane's edge, which is a *border*, and the one
/// adjective the design gives this thing is "a mark".
///
/// Four with [`THUMB_LANE_MARGIN_LOGICAL_PX`] on either side is the lane's own
/// eight, which is why neither number is free.
pub const THUMB_WIDTH_LOGICAL_PX: f32 = 4.0;
/// The lane either side of the mark — two pixels of terminal between the thumb
/// and the pane's edge, and two more between the thumb and the text.
pub const THUMB_LANE_MARGIN_LOGICAL_PX: f32 = 2.0;
/// A four-pixel bar with a two-pixel radius is a stadium, which is what every
/// engine's thin thumb is and what [`cmdrail::TICK_RADIUS_LOGICAL_PX`] makes of
/// the tick beside it.
pub const THUMB_RADIUS_LOGICAL_PX: f32 = 2.0;
/// **How far in from the pane's right edge this instrument reaches for a hand**
/// — the reserved lane, plus the gap that separates it from the rail, and not
/// one pixel more.
///
/// This is the number that keeps D-14's promise executable. The rail's box ends
/// at `lane + gap` from the same edge ([`cmdrail::lay_out`]), so a band of
/// exactly `lane + gap` is every pixel the lane arithmetic set aside and none of
/// anybody else's: a pointer on a tick is never also on the thumb, and a pointer
/// in the lane is never also on the rail. There is no arbitration between them
/// because there is no overlap to arbitrate.
///
/// **What it costs at the window's own edge.** A bar this far out runs into the
/// finding [`preview::BODY_SCROLL_INWARD_HIT_LOGICAL_PX`] records: the outer
/// eight logical pixels of a restored window are the resize border, answered in
/// `WM_NCHITTEST` before this window's code is asked at all. The preview's bar
/// answers by growing sixteen pixels inward; this one cannot, because at eleven
/// it is already touching the rail. So the honest statement of the reach is: a
/// maximised window has the whole lane (`IsZoomed` puts the border at zero), a
/// pane with a neighbour to its right keeps everything outside the divider's
/// seven-pixel seam, and the right-most pane of a restored window keeps the
/// three pixels inboard of the border. The alternative — taking five pixels off
/// every tick — is the bug the lane was reserved to prevent, and a scroll bar
/// that is hard to grab in one window state is a smaller failure than a rail
/// that cannot be clicked in all of them.
pub const THUMB_REACH_LOGICAL_PX: f32 = TERMINAL_SCROLL_LANE_LOGICAL_PX + RAIL_LANE_GAP_LOGICAL_PX;

/// **How long a thumb with no reason left to be up stays up anyway.**
///
/// Nine hundred milliseconds, and a new constant rather than a borrowed one.
/// Everything in this window that carries a duration is a *transition* — the
/// rail's `.18s`, a pin's 120ms, a tooltip's 90 — and a transition is the
/// travel, not the wait before it. The two clocks this window already runs that
/// really are waits belong to intent (`PEEK_INTENT_MS`, the float's 180) and
/// answer the opposite question: how long a hand must stay before something
/// appears. This is a dwell *after*, and nothing here was that.
///
/// It is deliberately longer than any of them. The bar's whole job in this
/// moment is to have been seen, and a mark that leaves as fast as a hover
/// highlight is a mark you have to scroll again to read.
pub const THUMB_REST: Duration = Duration::from_millis(900);
/// The fade itself, once the rest has run out.
///
/// A transition, so it is in the family the constants above are: the rail's
/// `.18s ease` is the longest thing this window fades and this is the same
/// length, because the two are the same gesture — a piece of a pane's own edge
/// going quiet. Under [`Motion::Reduced`] it does not run at all; the thumb is
/// simply gone when the rest ends, which is what `prefers-reduced-motion` asks
/// for (mock-up 359-361).
pub const THUMB_FADE: Duration = Duration::from_millis(180);

/// The bar, in the one shape the painter, the hit test and the drag all read.
///
/// A thumb drawn somewhere the pointer is not tested is a thumb that looks
/// draggable and is not — [`preview::ScrollBar`]'s own words, and the defect
/// that shape exists to prevent. This adds the lane and the units and keeps the
/// property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalScrollBar {
    /// The shared derivation, in pixels down the pane's own height.
    pub bar: ScrollBar,
    /// The mark as it is drawn: the stadium inside the lane.
    pub thumb: [f32; 4],
    /// The thumb's target — the whole reach across, the thumb's own span (grown
    /// by [`preview::BLOCK_SCROLL_HIT_LOGICAL_PX`]'s tolerance) down.
    pub grab: [f32; 4],
    /// The reach, full height: the band a pointer counts as "near" in, and the
    /// track a click outside the thumb pages from.
    pub lane: [f32; 4],
    /// The thumb's corner radius, in physical pixels.
    pub radius: f32,
    /// How far into history the view may go, in subpixels.
    pub extent: i64,
    /// One screenful, in subpixels — what a track click moves by.
    pub page: i64,
    /// Physical pixels per subpixel, the one factor the unit change runs on.
    scale_of_subpixel: f64,
}

/// The bar for a pane whose `body` shows one `page` of a document with `extent`
/// more above it, currently `offset` subpixels up from the bottom — or `None`
/// when there is no scrollback and therefore nothing to say.
///
/// **The offset runs the other way from the preview's**, and that is the only
/// subtlety in the conversion: a document is scrolled *down* from its top and a
/// terminal is scrolled *up* from its bottom, so `extent - offset` is the
/// distance from the top that the shared derivation wants. Getting this backwards
/// draws a thumb that rises when the text rises, which is a bar that is wrong in
/// a way that looks almost right.
#[must_use]
pub fn bar(
    body: [f32; 4],
    extent: i64,
    page: i64,
    offset: i64,
    scale: f32,
) -> Option<TerminalScrollBar> {
    if extent <= 0 || page <= 0 {
        return None;
    }
    let height = f64::from(body[3] - body[1]);
    if height <= 0.0 {
        return None;
    }
    // One screenful of subpixels is the pane's own height, which is what makes
    // this a change of units and not a second geometry: every length below is
    // the projection's own number multiplied by this one factor.
    let scale_of_subpixel = height / page as f64;
    let content = (page.saturating_add(extent)) as f64 * scale_of_subpixel;
    let from_top = (extent - offset.clamp(0, extent)) as f64 * scale_of_subpixel;
    let inner = preview::scroll_bar(
        body,
        ScrollAxis::Vertical,
        from_top as f32,
        content as f32,
        scale,
    )?;
    let right = body[2];
    let margin = THUMB_LANE_MARGIN_LOGICAL_PX * scale;
    let width = THUMB_WIDTH_LOGICAL_PX * scale;
    let reach = right - THUMB_REACH_LOGICAL_PX * scale;
    let thumb = [
        right - margin - width,
        inner.thumb[1],
        right - margin,
        inner.thumb[3],
    ];
    Some(TerminalScrollBar {
        bar: inner,
        thumb,
        // Across: the whole reach, because a hand aiming at a four-pixel mark
        // aims at the lane it is in. Down: the tolerance the shared bar already
        // put on its own ends, clamped into the pane so a thumb at the very top
        // does not claim a strip of the pane head above it.
        grab: [
            reach,
            inner.grab[1].max(body[1]),
            right,
            inner.grab[3].min(body[3]),
        ],
        lane: [reach, body[1], right, body[3]],
        radius: THUMB_RADIUS_LOGICAL_PX * scale,
        extent,
        page,
        scale_of_subpixel,
    })
}

impl TerminalScrollBar {
    /// Whether a pointer at `at` is on the thumb.
    #[must_use]
    pub fn thumb_holds(&self, at: [f32; 2]) -> bool {
        within(self.grab, at)
    }

    /// Whether a pointer at `at` is within the lane's reach — the fact
    /// [`visibility`] calls `near`, and the one that decides a press is this
    /// instrument's at all.
    #[must_use]
    pub fn lane_holds(&self, at: [f32; 2]) -> bool {
        within(self.lane, at)
    }

    /// Where the offset lands when the thumb is dragged to `y`, held `grab`
    /// pixels below its own top edge.
    ///
    /// [`preview::scroll_dragged_to`] and then the unit change back, which is
    /// the whole of it: the shared map already clamps by the same numbers the
    /// wheel clamps by, so the only thing that can be wrong here is the
    /// direction, and the direction is the same inversion [`bar`] applied on the
    /// way in. The round trip is pinned by
    /// `a_thumb_dragged_where_the_geometry_put_it_lands_on_the_offset_it_came_from`.
    #[must_use]
    pub fn dragged_to(&self, y: f32, grab: f32) -> i64 {
        let from_top =
            f64::from(preview::scroll_dragged_to(&self.bar, y, grab)) / self.scale_of_subpixel;
        (self.extent - from_top.round() as i64).clamp(0, self.extent)
    }

    /// Where `offset` lands when the track is clicked at `y`.
    ///
    /// **The platform's convention, which is a page and not a jump.** Windows
    /// pages by a screenful toward the click and stops; macOS can be set either
    /// way and defaults to the same. A click above the thumb is a click on the
    /// part of the document that is *earlier*, which in a terminal is further up
    /// the history, so it takes the offset up. A click on the thumb itself moves
    /// nothing — the drag that press begins is what moves it.
    #[must_use]
    pub fn paged_from(&self, offset: i64, y: f32) -> i64 {
        if y < self.thumb[1] {
            offset.saturating_add(self.page).min(self.extent)
        } else if y > self.thumb[3] {
            offset.saturating_sub(self.page).max(0)
        } else {
            offset
        }
    }

    /// Where a hand that took the thumb at `y` is holding it, measured from the
    /// thumb's own top edge — the `grab` [`Self::dragged_to`] wants.
    ///
    /// Clamped into the thumb, so a press on the tolerance band beyond either
    /// end behaves as a press on the end itself rather than teleporting the
    /// document by the width of the tolerance.
    #[must_use]
    pub fn grip(&self, y: f32) -> f32 {
        (y - self.thumb[1]).clamp(0.0, self.thumb[3] - self.thumb[1])
    }
}

fn within(rect: [f32; 4], at: [f32; 2]) -> bool {
    rect[0] <= at[0] && at[0] <= rect[2] && rect[1] <= at[1] && at[1] <= rect[3]
}

/// Everything that decides whether the bar is on the glass, and how brightly.
///
/// A struct of facts rather than a method on the runtime, so the state machine
/// can be asked the questions a hand asks it without a window, a GPU or a shell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbSituation {
    /// Whether this pane has any scrollback at all.
    pub has_history: bool,
    /// Whether the pane is showing the alternate screen.
    pub alternate_screen: bool,
    /// Whether the view is anywhere other than the live bottom.
    pub scrolled: bool,
    /// Whether the pointer is within the lane's reach.
    pub near: bool,
    /// Whether a hand is holding the thumb.
    pub held: bool,
    /// How long since the last moment any of the three above was true.
    pub since_rest: Duration,
}

/// What the painter is owed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Thumb {
    /// Nothing is drawn, and nothing is owed a frame.
    Hidden,
    /// The mark, at `alpha` of its own colour; `lit` picks
    /// [`ChromePalette::scroll_thumb_hover`] over [`ChromePalette::scroll_thumb`].
    Shown { alpha: f32, lit: bool },
}

/// **The visibility rule, whole** (P2-9 slice 1).
///
/// Two suppressions and three reasons, in that order:
///
/// 1. **No scrollback, no bar.** A pane whose whole document fits has nothing to
///    be a picture of, and a bar that said so by sitting full-length in the lane
///    would be furniture claiming to be information.
/// 2. **No bar on the alternate screen.** The same rule the search capsule is
///    held to (`Runtime::seat_can_search`, D-5) and the rail with it: §3.2 keeps
///    the two screens in isolated anchor namespaces, so the primary history's
///    extent is not a statement about what `vim` is drawing, and a thumb tracking
///    it would be a picture of another document laid over this one.
/// 3. **A hand, or a place that is not the bottom, keeps it up.** Holding the
///    thumb, hovering the lane, and being scrolled away from the live bottom are
///    each a standing reason: while any of them is true the bar is at full
///    strength, and the first two also light it.
/// 4. **Otherwise it rests, then fades.** At the bottom with the pointer
///    elsewhere there is no standing reason, so the bar keeps the last one's
///    moment for [`THUMB_REST`] and then eases out over [`THUMB_FADE`]. This is
///    the path a wheel at the bottom takes, and the path back down from history:
///    the mark is up for as long as the gesture plus nine hundred milliseconds,
///    and then the pane is a pane again.
///
/// Read the other way round, which is how the ruling was written: the bar is
/// hidden when there is no scrollback, or when the view is at the bottom and the
/// pointer is not near — once the rest has run out.
#[must_use]
pub fn visibility(
    situation: ThumbSituation,
    motion: Motion,
    curve: impl FnOnce(f32) -> f32,
) -> Thumb {
    if !situation.has_history || situation.alternate_screen {
        return Thumb::Hidden;
    }
    if situation.held || situation.near || situation.scrolled {
        return Thumb::Shown {
            alpha: 1.0,
            lit: situation.held || situation.near,
        };
    }
    if situation.since_rest < THUMB_REST {
        return Thumb::Shown {
            alpha: 1.0,
            lit: false,
        };
    }
    // A wait is not a transition, so `Reduced` shortens nothing above this line
    // and removes everything below it.
    if motion == Motion::Reduced {
        return Thumb::Hidden;
    }
    let fading = situation.since_rest - THUMB_REST;
    if fading >= THUMB_FADE {
        return Thumb::Hidden;
    }
    Thumb::Shown {
        alpha: 1.0 - curve(fading.as_secs_f32() / THUMB_FADE.as_secs_f32()),
        lit: false,
    }
}

/// When a thumb whose last reason ended at `rest` next owes a frame.
///
/// The **same** two durations [`visibility`] reads, deliberately shared rather
/// than restated: the deadline that wakes the loop and the paint that runs when
/// it does have to agree exactly, or the window either spins for ever on a bar
/// that has finished or leaves one half-faded on the glass. `None` once the fade
/// has landed, which is what makes a resting terminal cost no wake-ups at all.
#[must_use]
pub fn fade_deadline(rest: Instant, now: Instant, motion: Motion) -> Option<Instant> {
    let since = now.saturating_duration_since(rest);
    if since < THUMB_REST {
        return Some(rest + THUMB_REST);
    }
    if motion == Motion::Reduced || since >= THUMB_REST + THUMB_FADE {
        return None;
    }
    Some(now + crate::STRIP_ANIMATION_FRAME)
}

/// The mark, on a layer of its own.
///
/// A layer for [`crate::scroll_bar_layer`]'s reason, which is a z-order fact
/// rather than tidiness: within one layer the fills are drawn before the
/// document's own, so a thumb pushed onto the pane's quads would be painted
/// under the very text it has to ride over.
///
/// **No track quad.** `scrollbar-color: var(--thumb) transparent` — the second
/// half of that declaration is drawn by drawing nothing.
#[must_use]
pub fn layer(
    bar: &TerminalScrollBar,
    thumb: Thumb,
    palette: &ChromePalette,
) -> Option<OverlayLayer> {
    let Thumb::Shown { alpha, lit } = thumb else {
        return None;
    };
    let ink = if lit {
        palette.scroll_thumb_hover
    } else {
        palette.scroll_thumb
    };
    Some(OverlayLayer {
        quads: bt_render::rounded_overlay_fill(bar.thumb, bar.radius, ink, alpha),
        ..OverlayLayer::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: f32 = 2.0;
    /// A pane six hundred physical pixels tall, thirty rows of twenty pixels.
    const BODY: [f32; 4] = [100.0, 40.0, 900.0, 640.0];
    /// Twenty logical pixels a row at `SCALE`, in `bt-viewport`'s subpixels.
    const ROW: i64 = 20 * bt_viewport::SUBPIXELS_PER_PX;
    /// One screenful: thirty of them.
    const PAGE: i64 = 30 * ROW;

    fn linear(x: f32) -> f32 {
        x
    }

    fn resting(since_rest: Duration) -> ThumbSituation {
        ThumbSituation {
            has_history: true,
            alternate_screen: false,
            scrolled: false,
            near: false,
            held: false,
            since_rest,
        }
    }

    /// The lane the whole design turns on: everything this instrument draws is
    /// inside `TERMINAL_SCROLL_LANE_LOGICAL_PX`, and everything it reaches for
    /// stops where the rail's own box begins.
    ///
    /// MUTATION: give the reach `preview::BODY_SCROLL_INWARD_HIT_LOGICAL_PX`'s
    /// sixteen pixels, the way the preview's bar grows, and a hand aiming at a
    /// tick lands on the thumb — the 2026-07-18 report, in the other direction.
    #[test]
    fn the_thumb_and_the_rail_stand_side_by_side_in_the_lane_that_was_reserved() {
        let bar = bar(BODY, 500 * ROW, PAGE, 0, SCALE).expect("five hundred rows of history");
        let right = BODY[2];
        assert!(
            bar.thumb[0] >= right - TERMINAL_SCROLL_LANE_LOGICAL_PX * SCALE,
            "the mark is drawn inside the reserved lane, not beside it"
        );
        assert!(bar.thumb[2] <= right, "and never past the pane's own edge");
        // The right edge of the rail's own band, from `cmdrail::lay_out`: its
        // tick column starts at `body[2] - px(lane + gap) - px(padding)` and the
        // band is that column grown back out by its padding, so the band ends
        // exactly `lane + gap` in from the pane.
        let rail_right =
            right - (TERMINAL_SCROLL_LANE_LOGICAL_PX + RAIL_LANE_GAP_LOGICAL_PX) * SCALE;
        assert_eq!(
            bar.grab[0], rail_right,
            "the thumb's reach ends exactly where the rail's box begins"
        );
        assert_eq!(bar.lane[0], bar.grab[0], "and the track's reach with it");
    }

    /// The proportion is the viewport's share of the document, and the floor
    /// under it is the same ruling the preview's bar carries (2026-08-14): a
    /// mark nobody can see is a mark nobody can take.
    #[test]
    fn the_thumb_is_the_viewports_share_of_the_document_until_it_would_be_a_sliver() {
        let page_height = BODY[3] - BODY[1];
        // Three screenfuls of history: the thumb should be a quarter of the pane.
        let quarter = bar(BODY, 3 * PAGE, PAGE, 0, SCALE).expect("three pages of history");
        let length = quarter.thumb[3] - quarter.thumb[1];
        assert!(
            (length - page_height / 4.0).abs() < 0.5,
            "a document four screens long wants a thumb a quarter of the pane, got {length}"
        );
        // Sixty thousand rows: the honest share is under a pixel, so the floor
        // takes over.
        let long = bar(BODY, 60_000 * ROW, PAGE, 0, SCALE).expect("sixty thousand rows");
        let floored = long.thumb[3] - long.thumb[1];
        assert!(
            floored >= preview::BLOCK_SCROLL_MIN_THUMB_LOGICAL_PX * SCALE,
            "a {floored}px thumb is a sliver, not a handle"
        );
    }

    /// Where the thumb sits is where the view is: at the bottom it is at the
    /// bottom of the track, at the top of the history it is at the top, and the
    /// two ends are exact rather than nearly.
    #[test]
    fn the_marks_place_in_the_track_is_the_views_place_in_the_document() {
        let extent = 500 * ROW;
        let bottom = bar(BODY, extent, PAGE, 0, SCALE).expect("history");
        assert!(
            (bottom.thumb[3] - BODY[3]).abs() < 0.5,
            "at the live bottom the mark is at the foot of the track"
        );
        let top = bar(BODY, extent, PAGE, extent, SCALE).expect("history");
        assert!(
            (top.thumb[1] - BODY[1]).abs() < 0.5,
            "scrolled to the oldest line the mark is at the head of it"
        );
    }

    /// A pane whose document fits has no bar — not a full-length one, not a
    /// zero-length one. `None` is the whole answer.
    #[test]
    fn a_pane_with_no_scrollback_has_no_bar_to_draw() {
        assert_eq!(bar(BODY, 0, PAGE, 0, SCALE), None);
    }

    /// The drag is the geometry read backwards, and the round trip proves it:
    /// every offset the bar can be at draws a thumb which, taken by its own top
    /// edge and put down where it already was, lands on the offset it came from.
    ///
    /// MUTATION: drop the `extent - from_top` inversion in `dragged_to` and this
    /// fails everywhere except the exact middle.
    #[test]
    fn a_thumb_dragged_where_the_geometry_put_it_lands_on_the_offset_it_came_from() {
        let extent = 500 * ROW;
        for step in 0..=20 {
            let offset = extent * step / 20;
            let bar = bar(BODY, extent, PAGE, offset, SCALE).expect("history");
            let landed = bar.dragged_to(bar.thumb[1], 0.0);
            // Within half a subpixel row: the thumb's own position is rounded to
            // the pixel it is drawn on, so the offset it maps back to is the
            // document position that pixel stands for.
            assert!(
                (landed - offset).abs() <= ROW,
                "offset {offset} drew a thumb that read back as {landed}"
            );
        }
    }

    /// Both ends of the track are reachable, and neither overshoots.
    #[test]
    fn dragging_the_thumb_to_either_end_of_the_track_reaches_that_end_and_stops() {
        let extent = 500 * ROW;
        let bar = bar(BODY, extent, PAGE, extent / 2, SCALE).expect("history");
        assert_eq!(bar.dragged_to(BODY[1] - 400.0, 0.0), extent, "the top");
        assert_eq!(bar.dragged_to(BODY[3] + 400.0, 0.0), 0, "and the bottom");
    }

    /// A hand keeps the point it grabbed: taking the thumb by its middle and
    /// moving nothing moves the document by nothing.
    #[test]
    fn a_grip_taken_in_the_middle_of_the_thumb_is_kept_while_it_travels() {
        let extent = 500 * ROW;
        let bar = bar(BODY, extent, PAGE, extent / 3, SCALE).expect("history");
        let middle = (bar.thumb[1] + bar.thumb[3]) / 2.0;
        let grip = bar.grip(middle);
        assert!(
            (bar.dragged_to(middle, grip) - extent / 3).abs() <= ROW,
            "a thumb held in the middle and not moved must not jump"
        );
    }

    /// The track's convention: a click above pages toward the older lines, a
    /// click below pages back toward the live bottom, and a click on the thumb
    /// itself is the drag's, not the track's.
    #[test]
    fn a_click_in_the_track_pages_toward_it_by_one_screenful() {
        let extent = 500 * ROW;
        let offset = 10 * PAGE;
        let bar = bar(BODY, extent, PAGE, offset, SCALE).expect("history");
        assert_eq!(bar.paged_from(offset, bar.thumb[1] - 10.0), offset + PAGE);
        assert_eq!(bar.paged_from(offset, bar.thumb[3] + 10.0), offset - PAGE);
        let on_thumb = (bar.thumb[1] + bar.thumb[3]) / 2.0;
        assert_eq!(bar.paged_from(offset, on_thumb), offset);
    }

    /// And it stops at the ends rather than running past them.
    #[test]
    fn paging_stops_at_the_oldest_line_and_at_the_live_bottom() {
        let extent = PAGE + ROW;
        let bar = bar(BODY, extent, PAGE, extent, SCALE).expect("history");
        assert_eq!(bar.paged_from(extent, BODY[1] + 1.0), extent);
        assert_eq!(bar.paged_from(0, BODY[3] - 1.0), 0);
    }

    /// The two suppressions, and neither is a fade: there is no bar to fade.
    #[test]
    fn a_pane_with_no_history_or_on_the_alternate_screen_shows_no_thumb_at_all() {
        let mut situation = resting(Duration::ZERO);
        situation.has_history = false;
        situation.scrolled = true;
        situation.near = true;
        assert_eq!(
            visibility(situation, Motion::Full, linear),
            Thumb::Hidden,
            "no scrollback, no bar — however hard the pointer looks at it"
        );
        let mut situation = resting(Duration::ZERO);
        situation.alternate_screen = true;
        situation.scrolled = true;
        situation.near = true;
        assert_eq!(
            visibility(situation, Motion::Full, linear),
            Thumb::Hidden,
            "vim's canvas is not this document, so this document's extent is not drawn on it"
        );
    }

    /// At the bottom with the pointer away, the bar rests and then goes: the
    /// ruling's own sentence, read forwards.
    #[test]
    fn at_the_bottom_with_the_pointer_away_the_thumb_rests_then_fades_then_is_gone() {
        assert_eq!(
            visibility(resting(Duration::from_millis(100)), Motion::Full, linear),
            Thumb::Shown {
                alpha: 1.0,
                lit: false
            },
            "still up while the rest runs"
        );
        let half = visibility(resting(THUMB_REST + THUMB_FADE / 2), Motion::Full, linear);
        match half {
            Thumb::Shown { alpha, lit } => {
                assert!(!lit, "a fading thumb is not a lit one");
                assert!(
                    (alpha - 0.5).abs() < 0.01,
                    "halfway through the fade is half the ink, got {alpha}"
                );
            }
            Thumb::Hidden => panic!("the fade had not finished"),
        }
        assert_eq!(
            visibility(resting(THUMB_REST + THUMB_FADE), Motion::Full, linear),
            Thumb::Hidden,
            "and then the pane is a pane again"
        );
    }

    /// Each of the three standing reasons holds it up on its own, and the two
    /// that involve a hand light it.
    #[test]
    fn a_hand_or_a_place_that_is_not_the_bottom_keeps_the_thumb_up_indefinitely() {
        let long_gone = Duration::from_secs(60);
        let mut scrolled = resting(long_gone);
        scrolled.scrolled = true;
        assert_eq!(
            visibility(scrolled, Motion::Full, linear),
            Thumb::Shown {
                alpha: 1.0,
                lit: false
            },
            "a view parked in history keeps its picture, unlit"
        );
        let mut near = resting(long_gone);
        near.near = true;
        assert_eq!(
            visibility(near, Motion::Full, linear),
            Thumb::Shown {
                alpha: 1.0,
                lit: true
            },
            "the pointer in the lane brings the mark forward"
        );
        let mut held = resting(long_gone);
        held.held = true;
        assert_eq!(
            visibility(held, Motion::Full, linear),
            Thumb::Shown {
                alpha: 1.0,
                lit: true
            },
            "and a hand that has left the lane still holds what it took"
        );
    }

    /// Reduced motion removes the travel and keeps the wait — CSS's own reading
    /// of `prefers-reduced-motion`, which is about movement and not about time.
    #[test]
    fn reduced_motion_keeps_the_rest_and_drops_the_fade() {
        assert_eq!(
            visibility(
                resting(THUMB_REST - Duration::from_millis(1)),
                Motion::Reduced,
                linear
            ),
            Thumb::Shown {
                alpha: 1.0,
                lit: false
            }
        );
        assert_eq!(
            visibility(resting(THUMB_REST), Motion::Reduced, linear),
            Thumb::Hidden
        );
    }

    /// The deadline and the paint read the same two durations, so a window stops
    /// waking exactly when there is nothing left to draw.
    #[test]
    fn the_fade_asks_for_frames_until_it_lands_and_not_one_after() {
        let rest = Instant::now();
        assert_eq!(
            fade_deadline(rest, rest, Motion::Full),
            Some(rest + THUMB_REST),
            "the first wake-up owed is the end of the rest"
        );
        assert!(
            fade_deadline(rest, rest + THUMB_REST, Motion::Full).is_some(),
            "the fade's own frames follow it"
        );
        assert_eq!(
            fade_deadline(rest, rest + THUMB_REST + THUMB_FADE, Motion::Full),
            None,
            "and a landed fade owes nothing"
        );
        assert_eq!(
            fade_deadline(rest, rest + THUMB_REST, Motion::Reduced),
            None,
            "under reduced motion there was never a fade to wake for"
        );
    }

    /// The hit test agrees with the picture — the property the shared shape
    /// exists for, checked across the lane rather than only down it.
    #[test]
    fn every_pixel_of_the_lane_beside_the_mark_is_the_marks_to_take() {
        let bar = bar(BODY, 500 * ROW, PAGE, 200 * ROW, SCALE).expect("history");
        let middle = (bar.thumb[1] + bar.thumb[3]) / 2.0;
        for x in [bar.lane[0], bar.thumb[0], bar.thumb[2], BODY[2]] {
            assert!(
                bar.thumb_holds([x, middle]),
                "a hand at {x} is beside the mark and must be able to take it"
            );
        }
        assert!(
            !bar.thumb_holds([bar.lane[0] - 1.0, middle]),
            "and a hand one pixel inboard of the reach is the rail's"
        );
        assert!(
            bar.lane_holds([BODY[2] - 1.0, BODY[1] + 1.0]),
            "the lane runs the pane's whole height, because the track does"
        );
    }
}
