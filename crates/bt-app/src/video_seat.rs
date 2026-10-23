//! **One video, one engine, one control bar — and three surfaces that borrow
//! all three** (user ruling 2026-08-28, route B slice ②; `docs/DESIGN.md`
//! §7.44).
//!
//! # What this module is the answer to
//!
//! A recording can be looked at in three places in this window: the preview
//! pane, a floating window, and the glance card that comes up under the pointer.
//! Route A gave two of those nothing and the third a browser. The user's ruling
//! for this slice was one sentence — *the same engine, the same picture and the
//! same gestures on all three* — and this module is the shape that sentence
//! forces: **one model, dispatched by surface**, which is exactly
//! [`crate::preview_select`]'s arrangement for a selection and is not a
//! coincidence. A selection, a scroll offset and a playhead are all facts about
//! *a view of a file*, and this window has three kinds of view.
//!
//! So there is one [`VideoSeat`] per playing recording; it holds the engine, the
//! newest frame and the state of the bar; and it does not know which surface it
//! is on. The surface knows: it hands the seat a rectangle each frame and asks
//! for a layer and a bar. [`VideoSeats`] is the map from
//! [`crate::PreviewSurface`] to seat, and **the map is what makes the tear-off
//! work**: a card dragged into a floating window is
//! [`VideoSeats::rehome`] — one key changed, the engine untouched — rather than
//! a shutdown and a re-open, which is why the picture does not restart and the
//! playhead does not go back to zero.
//!
//! # Why the bar is drawn here and not by each surface
//!
//! §7.23 ⑪ drew this bar once already, in CSS, inside the shell page. The
//! ruling that retired the shell page did not retire the bar's design: the
//! hairline, the four controls, the two tracks, the two waits and the two
//! motion spans are the same ones, re-expressed where this window can draw them.
//! What changed is that there are now three places to draw it, and three copies
//! of a scrubber would be three scrubbers that disagree the first time one of
//! them is touched.
//!
//! [`VideoSeat::bar`] therefore returns a [`crate::marks::Band`] — one layer of
//! quads, marks and captions in whole-surface pixels, fading as one surface —
//! and each surface stacks it where it belongs: a pane's on its own, a float's
//! over the float's layer, a card's over the card. The bundle is
//! surface-neutral because the bar is.
//!
//! # The one thing the engine is not asked twice
//!
//! Every number on the bar is read out of [`bt_platform::video::engine::EngineState`]
//! at paint time, and nothing here remembers what it last told the engine. That
//! is §7.42 ④'s rule — "every number has one authority" — reaching the face: a
//! bar that remembered it had pressed play is a bar that lies the moment a video
//! reaches its end, which is a pause nobody pressed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_platform::video::engine::{Engine, EngineError, EngineState, Length};
use bt_render::{ChromeLabel, ChromeLabelWeight, ChromePalette, OverlayQuad, VideoStage};

use crate::marks::{Band, ChromeMark, ChromeSprite, OverlayLayer};

// ── the two waits, and where they came from ───────────────────────────────

/// **How long a pointer has to be over the picture before the bar comes up.**
///
/// Carried over unchanged from `player::PLAYER_BAR_REVEAL_INTENT_MS`, which is
/// the constant this one replaces, and it is still the house's own hover intent
/// to the millisecond — `profiles::CHEVRON_HOVER_OPEN_DELAY`, the rest a `⌄`
/// asks for before it opens its menu. A player's bar and a chevron's menu are
/// the same promise made twice: *this is what your hand was reaching for*, and
/// two different answers to it would be this window disagreeing with itself
/// about how long a hand takes to mean something.
///
/// Armed once when the pointer starts moving and **not** restarted by the moves
/// after it. Restarting on every move would mean a bar that never came up while
/// the pointer was travelling, which is the whole of the time a reader is
/// reaching for it.
pub const VIDEO_BAR_REVEAL_INTENT: Duration = Duration::from_millis(250);

/// **How long the bar stands after the last thing the reader did.**
///
/// The dwell *after*, on `termscroll::THUMB_REST`'s precedent: a control with no
/// reason left to be up goes away, and two seconds is the reading time for the
/// one thing on the bar a reader might have come to read — the clock.
///
/// Three states are not "no action" and hold it up: a **paused** video (a player
/// with nothing happening has no second way to say what it is doing), a pointer
/// resting **on the bar itself**, and a **track being dragged**.
pub const VIDEO_BAR_IDLE_REST: Duration = Duration::from_millis(2_000);

/// The bar's fade, in and out. [`bt_render::MOTION_FAST`] — the same ninety
/// milliseconds every hover fade in this window is drawn on, and the same span
/// the shell page's stylesheet used through `--fast`.
pub const VIDEO_BAR_FADE: Duration = bt_render::MOTION_FAST;

// ── the bar's shape, in logical pixels ────────────────────────────────────
//
// Every one of these is the number the shell page's stylesheet carried, so that
// retiring the page changed where the bar is drawn and not what it looks like.

/// UI-SPEC.md H6: a strip is 30px tall.
pub const VIDEO_BAR_HEIGHT_LOGICAL_PX: f32 = 30.0;
/// UI-SPEC.md S7: strip side padding is 12px.
const BAR_PADDING_X_LOGICAL_PX: f32 = 12.0;
/// `#bar{gap:8px}`.
const BAR_GAP_LOGICAL_PX: f32 = 8.0;
/// `.ib{width:22px;height:22px}` — the hit box of an icon button.
const BAR_BUTTON_LOGICAL_PX: f32 = 22.0;
/// UI-SPEC.md I2: the toolbar mark uses the 14px slot.
const BAR_MARK_LOGICAL_PX: f32 = 14.0;
/// `.tb{min-width:32px}` — the speed button, which is text and not a mark.
const BAR_RATE_WIDTH_LOGICAL_PX: f32 = 32.0;
/// `#vol{flex:0 0 54px}`.
const BAR_VOLUME_WIDTH_LOGICAL_PX: f32 = 54.0;
/// `.track::before{height:2px}` — the rail both tracks are struck on.
const BAR_RAIL_LOGICAL_PX: f32 = 2.0;
/// `.track .knob{width:7px;height:7px}`.
const BAR_KNOB_LOGICAL_PX: f32 = 7.0;
/// `#bar{border-top:1px solid}` — the one hairline on the whole bar.
const BAR_EDGE_LOGICAL_PX: f32 = 1.0;
/// `.track{min-width:24px}` — below this a scrubber is a decoration.
const BAR_MIN_SEEK_LOGICAL_PX: f32 = 24.0;
/// The tolerance a track's grab box is grown by, top and bottom.
///
/// A two-pixel rail is not a thing a hand can hit, which is the same problem
/// `termscroll::TerminalScrollBar::grab` solves the same way: what is drawn is
/// thin and what answers is not.
const BAR_TRACK_REACH_LOGICAL_PX: f32 = 9.0;

/// **How wide one figure of the clock is reserved at**, as a fraction of the
/// font size.
///
/// The two clocks are set with [`ChromeLabel::tabular_numerals`], so every
/// figure is one advance and a box that fits the widest reading fits every
/// reading — which is the whole reason a running clock asks for tabular figures
/// in the first place. `0.62em` is measured generously rather than exactly: a
/// clock reserved a pixel too wide moves nothing, and a clock reserved a pixel
/// too narrow clips the last digit of an hour-long recording.
const BAR_FIGURE_ADVANCE_EM: f32 = 0.62;

/// **The speeds, in the order they come round** (user ruling 2026-08-27, §7.23
/// ⑪, unchanged by this slice).
pub const VIDEO_RATES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];

/// How far `←` and `→` move the playhead.
const VIDEO_STEP_SECONDS: f64 = 5.0;

/// How far `↑` and `↓` move the volume.
const VIDEO_GAIN_STEP: f64 = 0.05;

/// **The frame rate a frame step is measured in when the engine reports none**
/// (owner's ruling 2026-10-23, row 42): a thirtieth of a second, the commonest
/// rate a screen recording is made at.
const VIDEO_FALLBACK_FRAME_RATE: f64 = 30.0;

/// What the mute button restores when it is pressed on a video whose volume is
/// already nothing — the same half the shell page's script used, and for its
/// reason: un-muting into silence is a button that does nothing.
const VIDEO_UNMUTE_FLOOR: f64 = 0.5;

// ── the controls, and where they are ──────────────────────────────────────

/// **One pressable thing on the bar.**
///
/// The set is the ruling's, and the two that are absent are absent by the same
/// ruling: there is no fullscreen button, because a pane zooms from its own
/// head, and no picture-in-picture, because a pane's content does not leave the
/// window that is showing it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BarSlot {
    PlayPause,
    /// The scrubber. Draggable — see [`VideoSeat::grab`].
    Seek,
    Mute,
    /// The volume track. Draggable.
    Volume,
    Rate,
}

impl BarSlot {
    /// Whether a press on this slot begins a drag rather than doing something
    /// once.
    #[must_use]
    pub const fn is_track(self) -> bool {
        matches!(self, Self::Seek | Self::Volume)
    }
}

/// **Every box on the bar, in whole-surface physical pixels.**
///
/// One struct rather than a function per control, for the reason
/// [`crate::seats::PreviewRailGeometry`] is one struct: the boxes are laid out
/// against each other from one end to the other, so computing one means
/// computing all of them, and a second entry point would be a second layout.
///
/// The five optional boxes are the ones that **give way as the box narrows** —
/// see [`bar_layout`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarLayout {
    /// The whole bar, hairline included.
    pub bar: [f32; 4],
    /// The hairline along its top edge.
    pub edge: [f32; 4],
    pub play: [f32; 4],
    /// The mark inside [`Self::play`], inset to its 16 logical pixels.
    pub play_mark: [f32; 4],
    pub at: Option<[f32; 4]>,
    pub seek: [f32; 4],
    /// The two-pixel rail [`Self::seek`] is struck on.
    pub seek_rail: [f32; 4],
    /// What a hand may hit to take hold of the scrubber — [`Self::seek`] grown.
    pub seek_grab: [f32; 4],
    pub duration: Option<[f32; 4]>,
    pub mute: Option<[f32; 4]>,
    pub mute_mark: Option<[f32; 4]>,
    pub volume: Option<[f32; 4]>,
    pub volume_rail: Option<[f32; 4]>,
    pub volume_grab: Option<[f32; 4]>,
    pub rate: Option<[f32; 4]>,
    /// **What the recording is** — `MP4 · 38 MB`, at the bar's right end beside
    /// the speed (owner's ruling 2026-09-12; §7.44).
    ///
    /// It stood under the picture until that ruling put the bar flush on the
    /// bottom edge of the stage: a line under a frame whose bar is drawn *on*
    /// the frame is either behind the bar or is the band the ruling retired. It
    /// is not a control — nothing presses it — and it is first of all of these
    /// to give way, because it is the only one that is a fact rather than a
    /// verb.
    pub meta: Option<[f32; 4]>,
    /// **Whether the scrubber is a control.** `false` for a recording that has
    /// loaded and declares no length: there is nothing to scrub along, the bar
    /// says so in the scrubber's place ([`bar_layout_of`]), and a press there
    /// takes hold of nothing.
    pub seekable: bool,
}

impl BarLayout {
    /// **Which control a point is on**, or `None` for the bar's own ground and
    /// for anywhere off it.
    ///
    /// The two tracks answer through their grown boxes and the buttons through
    /// the boxes they are drawn in, which is the ordinary difference between a
    /// thing that is two pixels tall and a thing that is twenty-two.
    #[must_use]
    pub fn slot_at(&self, at: [f32; 2]) -> Option<BarSlot> {
        let hit = |rect: [f32; 4]| {
            at[0] >= rect[0] && at[0] < rect[2] && at[1] >= rect[1] && at[1] < rect[3]
        };
        if hit(self.play) {
            return Some(BarSlot::PlayPause);
        }
        if self.seekable && hit(self.seek_grab) {
            return Some(BarSlot::Seek);
        }
        if self.mute.is_some_and(hit) {
            return Some(BarSlot::Mute);
        }
        if self.volume_grab.is_some_and(hit) {
            return Some(BarSlot::Volume);
        }
        if self.rate.is_some_and(hit) {
            return Some(BarSlot::Rate);
        }
        None
    }

    /// Whether a point is anywhere on the bar at all — the answer that holds the
    /// bar up while a hand is resting on it.
    #[must_use]
    pub fn holds(&self, at: [f32; 2]) -> bool {
        at[0] >= self.bar[0] && at[0] < self.bar[2] && at[1] >= self.bar[1] && at[1] < self.bar[3]
    }

    /// The drawn box of a track — what a fraction is read against.
    #[must_use]
    fn track_of(&self, slot: BarSlot) -> Option<[f32; 4]> {
        match slot {
            BarSlot::Seek => Some(self.seek),
            BarSlot::Volume => self.volume,
            _ => None,
        }
    }
}

/// **Where along a track a pointer is**, `0.0 ..= 1.0`.
///
/// A zero-width track answers zero rather than dividing, which is the same
/// guard the shell page's `frac()` carried and for the same reason: a track
/// mid-collapse is a track a hand can still be on.
#[must_use]
fn fraction_along(track: [f32; 4], x: f32) -> f64 {
    let width = track[2] - track[0];
    if width <= 0.0 {
        return 0.0;
    }
    f64::from(((x - track[0]) / width).clamp(0.0, 1.0))
}

/// **What a recording's bar says about its file, and how wide it is drawn**
/// (owner's ruling 2026-09-12; §7.44 ⑯).
///
/// A pair rather than two arguments because they are one fact measured twice
/// over — the sentence and the room it needs — and because only something
/// holding a font can produce the second, so the two always travel together
/// from the one pass that measures them (`Runtime::measure_video_meta`).
///
/// [`Self::none`] is a recording whose facts this frame has not measured yet,
/// and it reserves nothing: the bar is laid out exactly as it was before the
/// ruling until the sentence arrives.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BarMeta<'a> {
    pub text: &'a str,
    pub width: f32,
}

impl BarMeta<'_> {
    /// Nothing measured, nothing reserved.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// **The bar, laid out in a body** — or `None` where there is no room for one.
///
/// # What gives way, and in what order
///
/// The bar is the same bar on a nine-hundred-pixel pane and on a three-hundred
/// pixel glance card, which means something has to go. Rather than two designs
/// with a threshold between them, the controls **drop from the right in one
/// fixed order** as the box narrows: the speed, then the volume track, then the
/// mute button, then the duration, then the elapsed clock. Play and the
/// scrubber never go, because a player with neither is not a player.
///
/// One order and not a table of tiers, for the ordinary reason: a tier list is a
/// second place the design lives, and the day somebody adds a control they would
/// have to decide where it goes in every tier rather than once in this list.
/// The order itself is by *how much of the ruling each control is*: the speed is
/// a convenience, the volume has a keyboard and a mute button standing for it,
/// the mute has a keyboard standing for it, and the two clocks are the reading
/// the bar exists for — so they go last, longest reading first.
///
/// `None` when even play plus a minimum scrubber will not fit, which is a box
/// too small to be a player and is drawn as a picture with no bar.
#[must_use]
fn bar_layout(body: [f32; 4], scale: f32, clock_figures: usize, meta_px: f32) -> Option<BarLayout> {
    let px = |logical: f32| logical * scale;
    let height = px(VIDEO_BAR_HEIGHT_LOGICAL_PX).round().max(1.0);
    let body_width = body[2] - body[0];
    let body_height = body[3] - body[1];
    if body_width <= 0.0 || body_height < height {
        return None;
    }
    let bar = [body[0], (body[3] - height).max(body[1]), body[2], body[3]];
    let edge_thickness = px(BAR_EDGE_LOGICAL_PX).max(1.0);
    let edge = [bar[0], bar[1], bar[2], bar[1] + edge_thickness];

    let padding = px(BAR_PADDING_X_LOGICAL_PX).round();
    let gap = px(BAR_GAP_LOGICAL_PX).round();
    let button = px(BAR_BUTTON_LOGICAL_PX).round().max(1.0);
    let clock = (clock_figures as f32 * BAR_FIGURE_ADVANCE_EM * px(font_logical_px()))
        .ceil()
        .max(1.0);
    let volume_width = px(BAR_VOLUME_WIDTH_LOGICAL_PX).round();
    let rate_width = px(BAR_RATE_WIDTH_LOGICAL_PX).round();
    let min_seek = px(BAR_MIN_SEEK_LOGICAL_PX).round().max(1.0);

    // The controls in the order they stand, each with the width it wants. The
    // first two are not in the list because they never give way.
    let mut wants_at = true;
    let mut wants_duration = true;
    let mut wants_mute = true;
    let mut wants_volume = true;
    let mut wants_rate = true;
    let mut wants_meta = meta_px > 0.0;
    // Widths of everything that is not the scrubber, plus one gap for each of
    // them, plus the two paddings, plus the scrubber's floor.
    let needed = |at: bool, duration: bool, mute: bool, volume: bool, rate: bool, meta: bool| {
        let mut width = 2.0 * padding + button + gap + min_seek;
        for (present, extent) in [
            (at, clock),
            (duration, clock),
            (mute, button),
            (volume, volume_width),
            (rate, rate_width),
            (meta, meta_px),
        ] {
            if present {
                width += extent + gap;
            }
        }
        width
    };
    // Drop from the right until it fits. Written as a loop over the five flags
    // rather than five `if`s so that adding a control is adding a line here and
    // nowhere else.
    for dropped in 0..=6 {
        if needed(
            wants_at,
            wants_duration,
            wants_mute,
            wants_volume,
            wants_rate,
            wants_meta,
        ) <= body_width
        {
            break;
        }
        match dropped {
            // **The facts go first** (owner's ruling 2026-09-12): every other
            // thing on this bar is a control, and this one is a sentence about
            // the file that the path row can still be asked for.
            0 => wants_meta = false,
            1 => wants_rate = false,
            2 => wants_volume = false,
            3 => wants_mute = false,
            4 => wants_duration = false,
            5 => wants_at = false,
            // Everything optional is gone and it still does not fit: this box is
            // not a player.
            _ => return None,
        }
    }
    if needed(
        wants_at,
        wants_duration,
        wants_mute,
        wants_volume,
        wants_rate,
        wants_meta,
    ) > body_width
    {
        return None;
    }

    let middle = (edge[3] + bar[3]) / 2.0;
    let box_of = |left: f32, width: f32, height: f32| {
        let top = (middle - height / 2.0).round();
        [left, top, left + width, top + height]
    };
    let rail_thickness = px(BAR_RAIL_LOGICAL_PX).round().max(1.0);
    let reach = px(BAR_TRACK_REACH_LOGICAL_PX);
    let rail_of = |track: [f32; 4]| {
        let top = (middle - rail_thickness / 2.0).round();
        [track[0], top, track[2], top + rail_thickness]
    };
    let grab_of = |track: [f32; 4]| [track[0], middle - reach, track[2], middle + reach];
    let mark_inset = ((button - px(BAR_MARK_LOGICAL_PX)) / 2.0).max(0.0).round();
    let mark_of = |rect: [f32; 4]| {
        [
            rect[0] + mark_inset,
            rect[1] + mark_inset,
            rect[2] - mark_inset,
            rect[3] - mark_inset,
        ]
    };

    // From the left.
    let mut cursor = bar[0] + padding;
    let play = box_of(cursor, button, button);
    cursor = play[2] + gap;
    let at = wants_at.then(|| {
        let rect = box_of(cursor, clock, button);
        cursor = rect[2] + gap;
        rect
    });
    // From the right, so that the scrubber gets what is left over.
    let mut right = bar[2] - padding;
    // **Outermost, beside the speed** — the mock's own arrangement: the controls
    // run in from the edge and the fact stands at the end of them.
    let meta = wants_meta.then(|| {
        let rect = box_of(right - meta_px, meta_px, button);
        right = rect[0] - gap;
        rect
    });
    let rate = wants_rate.then(|| {
        let rect = box_of(right - rate_width, rate_width, button);
        right = rect[0] - gap;
        rect
    });
    let volume = wants_volume.then(|| {
        let rect = box_of(right - volume_width, volume_width, button);
        right = rect[0] - gap;
        rect
    });
    let mute = wants_mute.then(|| {
        let rect = box_of(right - button, button, button);
        right = rect[0] - gap;
        rect
    });
    let duration = wants_duration.then(|| {
        let rect = box_of(right - clock, clock, button);
        right = rect[0] - gap;
        rect
    });
    let seek = box_of(cursor, (right - cursor).max(min_seek), button);

    Some(BarLayout {
        bar,
        edge,
        play,
        play_mark: mark_of(play),
        at,
        seek_rail: rail_of(seek),
        seek_grab: grab_of(seek),
        seek,
        duration,
        mute,
        mute_mark: mute.map(mark_of),
        volume_rail: volume.map(rail_of),
        volume_grab: volume.map(grab_of),
        volume,
        rate,
        meta,
        seekable: true,
    })
}

/// **The bar of a recording whose engine stands at `state`**, laid out — the
/// one door the painter and the hit test both lay the bar out through, so the
/// boxes a hand is answered by are the boxes that were drawn.
///
/// The clocks are reserved for the wider of the two readings
/// ([`clock_figures`]), and a recording that has loaded and declares no length
/// ([`Length::Unknown`]) has no scrubber to take hold of.
#[must_use]
pub fn bar_layout_of(
    state: &EngineState,
    body: [f32; 4],
    scale: f32,
    meta_px: f32,
) -> Option<BarLayout> {
    let figures = clock_figures(state.position_secs, state.duration_secs);
    bar_layout(body, scale, figures, meta_px).map(|layout| BarLayout {
        seekable: state.length() != Length::Unknown,
        ..layout
    })
}

/// The face the bar's two clocks and its speed are set at — the same one a
/// pane's own title is set at, which is what the shell page's `--ui-size`
/// carried across the boundary that no longer exists.
const fn font_logical_px() -> f32 {
    bt_render::SEAT_TITLE_FONT_LOGICAL_PX
}

/// The same face, for the one caller outside this module that has to measure a
/// word before it can be laid out here (owner's ruling 2026-09-12): a width
/// measured against a different face is a box the text does not fit.
#[must_use]
pub const fn bar_font_logical_px() -> f32 {
    font_logical_px()
}

// ── whether the bar is up ─────────────────────────────────────────────────

/// **The bar's presence, as a number** — `0.0` gone, `1.0` up, in between while
/// it is on its way to one of those.
///
/// [`crate::termscroll::Thumb`]'s shape and its discipline: a *sampled* value
/// rather than a tween somebody advances, so that a frame drawn late and a frame
/// drawn early are both drawn correctly, and so the deadline that asks for the
/// next frame ([`BarPresence::deadline`]) and the opacity that is painted are
/// read off the same two durations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarPresence {
    pub opacity: f32,
    /// When this value stops changing on its own, or `None` when it already has.
    pub settled_at: Option<Instant>,
    /// **Whether the opacity above is changing at the instant it was read**
    /// (review round 3, 2026-09-18).
    ///
    /// A third answer and not a reading of the second, because the two say
    /// different things and the window needs both. [`Self::settled_at`] is when
    /// the bar next needs a frame and is `Some` through the bar's two **waits**
    /// as well — the intent a still pointer is serving out before the bar is
    /// shown at all, and the two seconds a shown bar stands before it begins to
    /// go. This is `true` only while the fade is running. The window used to
    /// take `bar_deadline(…).is_some()` as its liveness, so a bar standing at
    /// full strength over a paused recording kept the chrome lane alive — and
    /// the chrome is the expensive lane, walked tab by tab with text measured on
    /// the way.
    pub moving: bool,
}

/// Everything the bar's presence is decided from, gathered so that the decision
/// is one function of one struct rather than six fields read in three places.
#[derive(Clone, Copy, Debug)]
struct BarSituation {
    /// The pointer has been still on the picture for at least the intent.
    revealed_at: Option<Instant>,
    /// The last thing the reader did that counts as doing something.
    acted_at: Instant,
    /// A hand is resting on the bar.
    over_bar: bool,
    /// A track is in hand.
    grabbing: bool,
    /// The video is not running. A paused player has no second way to say so.
    paused: bool,
}

impl BarSituation {
    /// Whether the bar has a reason to be up that has nothing to do with the
    /// clock — the three states the ruling named, plus "the reader just did
    /// something".
    fn held(self) -> bool {
        self.over_bar || self.grabbing || self.paused
    }

    /// **Whether the bar has finished leaving** — the dwell run out *and* the
    /// fade run out, with nothing holding it up.
    ///
    /// The one question [`VideoSeat::tick`] asks before it forgets a reveal, and
    /// it is a named predicate rather than the inline `opacity <= 0.0` it was
    /// first written as, because that reading was wrong on the machine in a way
    /// no reasoning found and one screenshot did: a bar **one frame into its
    /// rise** is also at zero opacity, so the tick that granted a reveal cleared
    /// it in the same breath and the bar never appeared. "Not up yet" and "gone
    /// again" are opposite ends of one number, and only the clock tells them
    /// apart.
    fn has_finished_leaving(self, now: Instant) -> bool {
        !self.held()
            && now.saturating_duration_since(self.acted_at) >= VIDEO_BAR_IDLE_REST + VIDEO_BAR_FADE
    }

    fn presence(self, now: Instant, motion: crate::Motion) -> BarPresence {
        let Some(revealed_at) = self.revealed_at else {
            return BarPresence {
                opacity: 0.0,
                settled_at: None,
                moving: false,
            };
        };
        if self.held() {
            // Rising, or already up. Under reduced motion a fade is exactly the
            // part that is refused, so it arrives whole.
            return Self::rising(revealed_at, now, motion);
        }
        let idle = now.saturating_duration_since(self.acted_at);
        if idle < VIDEO_BAR_IDLE_REST {
            // **The rest is added to the deadline and never to the motion**
            // (review round 3, 2026-09-18): the bar has to be woken when its two
            // seconds run out, and for the whole of those two seconds it is a
            // picture standing still.
            let mut presence = Self::rising(revealed_at, now, motion);
            let rests_at = self.acted_at + VIDEO_BAR_IDLE_REST;
            presence.settled_at = Some(match presence.settled_at {
                Some(earlier) => earlier.min(rests_at),
                None => rests_at,
            });
            return presence;
        }
        // Resting. The fade out is the same span as the fade in.
        if motion == crate::Motion::Reduced {
            return BarPresence {
                opacity: 0.0,
                settled_at: None,
                moving: false,
            };
        }
        let fading = idle - VIDEO_BAR_IDLE_REST;
        if fading >= VIDEO_BAR_FADE {
            return BarPresence {
                opacity: 0.0,
                settled_at: None,
                moving: false,
            };
        }
        BarPresence {
            opacity: 1.0 - fraction_of(fading, VIDEO_BAR_FADE),
            settled_at: Some(self.acted_at + VIDEO_BAR_IDLE_REST + VIDEO_BAR_FADE),
            moving: true,
        }
    }

    fn rising(revealed_at: Instant, now: Instant, motion: crate::Motion) -> BarPresence {
        if motion == crate::Motion::Reduced {
            return BarPresence {
                opacity: 1.0,
                settled_at: None,
                moving: false,
            };
        }
        let risen = now.saturating_duration_since(revealed_at);
        if risen >= VIDEO_BAR_FADE {
            return BarPresence {
                opacity: 1.0,
                settled_at: None,
                moving: false,
            };
        }
        BarPresence {
            opacity: fraction_of(risen, VIDEO_BAR_FADE),
            settled_at: Some(revealed_at + VIDEO_BAR_FADE),
            moving: true,
        }
    }
}

fn fraction_of(elapsed: Duration, whole: Duration) -> f32 {
    if whole.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / whole.as_secs_f32()).clamp(0.0, 1.0)
}

// ── the seat ──────────────────────────────────────────────────────────────

/// **A recording that is open, the engine decoding it, and the bar over it.**
///
/// Ordinary Rust all the way down: no COM crosses this type, so it may be held
/// by a pane, **moved between surfaces** and dropped on any thread — which is
/// the property [`VideoSeats::rehome`] is built on and the property
/// `bt_platform::video::engine::Engine` was written to have. It ends through
/// [`VideoSeat::shutdown`]; its drop waits for nothing.
pub struct VideoSeat {
    engine: Engine,
    path: PathBuf,
    /// The renderer's name for this seat's one texture. Fixed for the life of
    /// the seat and **not** derived from the surface, so a tear-off does not
    /// throw a megabyte of pixels away and upload them again.
    key: String,
    /// The newest decoded picture, kept so that a redraw caused by something
    /// other than the video still paints the frame that is standing.
    frame: Option<bt_render::VideoFrameUpload>,
    /// When the pointer settled on the picture, once the intent has elapsed.
    revealed_at: Option<Instant>,
    /// When the intent was armed, before it has.
    arming_at: Option<Instant>,
    acted_at: Instant,
    over_bar: bool,
    grab: Option<BarSlot>,
    /// Which of [`VIDEO_RATES`] is in force. An index and not a number, because
    /// the button cycles and a float that had drifted would never match.
    rate_index: usize,
}

impl std::fmt::Debug for VideoSeat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VideoSeat")
            .field("path", &self.path)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl VideoSeat {
    /// **Open `path` and start playing it.**
    ///
    /// Playing, not paused, and for the shell page's own reason: the reader
    /// already pressed play — on this window's button, or on the card's mark, or
    /// by double-clicking the picture — and a player that then waited to be
    /// pressed a second time would be asking them to say the same thing twice.
    ///
    /// `serial` is a number this window never reuses; it is the whole of the
    /// texture's identity, so two panes showing one file are two pictures.
    pub fn open(path: &Path, serial: u64, now: Instant) -> Result<Self, EngineError> {
        let engine = Engine::open(path)?;
        engine.play();
        Ok(Self {
            engine,
            path: path.to_path_buf(),
            key: format!("video:{serial}"),
            frame: None,
            // Born with the bar up: the gesture that opened this seat *is* the
            // reader doing something, and a player that appeared with no
            // controls would be one nobody could pause.
            revealed_at: Some(now),
            arming_at: None,
            acted_at: now,
            over_bar: false,
            grab: None,
            rate_index: 0,
        })
    }

    /// The file this seat is playing — the spelling the surface knows it by.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The renderer's name for this seat's texture.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Everything knowable about the engine at this instant.
    #[must_use]
    pub fn state(&self) -> EngineState {
        self.engine.state()
    }

    /// **Wait for the engine to publish a state `until` accepts**, within
    /// [`crate::lane::PATIENCE`] — the engine's own door
    /// (`Engine::state_reaching`), so a test waits on the engine thread and not
    /// on a clock. Answers the state the wait ended on.
    #[cfg(test)]
    pub fn state_reaching(&self, until: impl Fn(&EngineState) -> bool) -> EngineState {
        self.engine.state_reaching(until, crate::lane::PATIENCE)
    }

    /// **Wait for the engine's terminal answer about its load** — ready, or
    /// refused — within `LOAD_ANSWER_PATIENCE`, which is derived from the
    /// engine's own `LOAD_BUDGET` and poll so that the wait cannot end a moment
    /// before the engine's own refusal lands. Answers the state the wait ended
    /// on.
    #[cfg(test)]
    pub fn state_answering_its_load(&self) -> EngineState {
        self.engine.state_reaching(
            |state| state.ready || state.error.is_some(),
            bt_platform::video::engine::LOAD_ANSWER_PATIENCE,
        )
    }

    /// **Whether this seat has stopped being a player, and what its surface
    /// says instead** — see [`retirement`].
    #[must_use]
    pub fn retirement(&self) -> Option<(EngineError, crate::i18n::Text)> {
        retirement(&self.engine.state())
    }

    /// **Whether the sound this seat is making should light the tab's speaker**
    /// (§7.44 ②, replacing `IsDocumentPlayingAudio`).
    ///
    /// Read off the engine rather than off a browser: a video that is running
    /// and not muted is one making a sound, and a video that is paused, ended,
    /// muted or turned to nothing is not. `has_audio` is asked because a
    /// screen capture with no sound track is not a tab worth pointing at.
    #[must_use]
    pub fn is_sounding(&self) -> bool {
        let state = self.engine.state();
        state.playing && !state.muted && state.volume > 0.0 && state.has_audio
    }

    /// **Take the newest decoded picture, if there is one this seat has not
    /// taken yet.** `true` when the glass owes a redraw.
    ///
    /// Called once per animation tick per seat. The engine's own `frame()` is
    /// what makes this cheap when nothing has arrived: one atomic load and no
    /// lock.
    pub fn pump(&mut self) -> bool {
        let Some(frame) = self.engine.frame() else {
            return false;
        };
        self.frame = Some(bt_render::VideoFrameUpload {
            bgra: frame.bgra,
            width_px: frame.width,
            height_px: frame.height,
            generation: frame.generation,
        });
        true
    }

    /// **This seat as a layer for the renderer.**
    ///
    /// `ground` is `None` where something underneath has already painted the box
    /// — a preview pane, whose letterbox bars are the pane's own body colour —
    /// and `Some` where nothing has, which is a float and a card.
    #[must_use]
    pub fn layer(
        &self,
        box_: bt_render::SeatViewport,
        clip: bt_render::SeatViewport,
        ground: Option<[u8; 3]>,
        radius_px: f32,
        opacity: f32,
        stage: VideoStage,
    ) -> bt_render::VideoLayer {
        bt_render::VideoLayer {
            key: self.key.clone(),
            box_,
            clip,
            frame: self.frame.clone(),
            ground,
            radius_px,
            opacity,
            stage,
        }
    }

    // ── the verbs ─────────────────────────────────────────────────────────

    /// **Play if paused (or at the end, from the start), pause if playing** —
    /// every play door's verb: the bar's button, Space, a click on the picture
    /// (row 43).
    ///
    /// "At the end" is [`stands_at_end`]: the engine's `ended`, or a playhead
    /// put on the last frame by `End` or the scrubber, which is a recording
    /// with nothing left to play just as surely.
    pub fn toggle(&mut self, now: Instant) {
        let state = self.engine.state();
        if state.playing {
            self.engine.pause();
        } else {
            if stands_at_end(&state) {
                self.engine.seek(0.0);
            }
            self.engine.play();
        }
        self.acted(now);
    }

    /// **One gesture on the picture** (owner's ruling 2026-10-23, row 38) —
    /// the door a pane and a floating window both reach, so the two faces
    /// answer a hand the same way by construction.
    ///
    /// A click is [`Self::toggle`].
    pub fn picture(&mut self, gesture: PictureGesture, now: Instant) {
        match gesture {
            PictureGesture::Click => self.toggle(now),
        }
    }

    /// The playhead to the start — `Home`.
    pub fn seek_start(&mut self, now: Instant) {
        self.engine.seek(0.0);
        self.acted(now);
    }

    /// **The playhead to the end — `End`.** A playing recording goes on
    /// playing into it, so the engine reaches its end and says `ended` itself,
    /// as it does when the end is reached by watching; a paused one stands on
    /// its last frame. A recording that declares no length has no end to go to.
    pub fn seek_end(&mut self, now: Instant) {
        if let Some(duration) = self.engine.state().duration_secs
            && duration.is_finite()
        {
            self.engine.seek(duration);
        }
        self.acted(now);
    }

    /// **One frame back or forward — `,` and `.` — while paused**, and nothing
    /// while playing: a frame step is a reading of one picture, and a running
    /// clock has already moved past it.
    pub fn step_frame(&mut self, frames: f64, now: Instant) {
        if let Some(seconds) = frame_step(&self.engine.state(), frames) {
            self.step(seconds, now);
        }
    }

    /// Move the playhead by `seconds`, clamped to the recording.
    pub fn step(&mut self, seconds: f64, now: Instant) {
        let state = self.engine.state();
        let mut target = state.position_secs + seconds;
        if target < 0.0 {
            target = 0.0;
        }
        if let Some(duration) = state.duration_secs
            && duration.is_finite()
            && target > duration
        {
            target = duration;
        }
        self.engine.seek(target);
        self.acted(now);
    }

    /// Put the playhead a fraction of the way through — what a scrubber does.
    pub fn seek_to(&mut self, fraction: f64, now: Instant) {
        let state = self.engine.state();
        if let Some(duration) = state.duration_secs
            && duration.is_finite()
            && duration > 0.0
        {
            self.engine.seek(fraction.clamp(0.0, 1.0) * duration);
        }
        self.acted(now);
    }

    /// Set the volume outright — what the volume track does. A volume moved off
    /// zero un-mutes, because a reader dragging a slider up is asking to hear it.
    pub fn set_volume(&mut self, volume: f64, now: Instant) {
        let volume = volume.clamp(0.0, 1.0);
        self.engine.set_volume(volume);
        self.engine.set_muted(volume <= 0.0);
        self.acted(now);
    }

    /// Move the volume by `delta` — what `↑` and `↓` do.
    pub fn nudge_volume(&mut self, delta: f64, now: Instant) {
        let volume = (self.engine.state().volume + delta).clamp(0.0, 1.0);
        self.set_volume(volume, now);
    }

    /// Mute, or un-mute — and un-muting a video whose volume is nothing gives it
    /// something to un-mute into.
    pub fn toggle_mute(&mut self, now: Instant) {
        let state = self.engine.state();
        let muted = !state.muted;
        self.engine.set_muted(muted);
        if !muted && state.volume <= 0.0 {
            self.engine.set_volume(VIDEO_UNMUTE_FLOOR);
        }
        self.acted(now);
    }

    /// The next speed round, out of [`VIDEO_RATES`].
    pub fn cycle_rate(&mut self, now: Instant) {
        self.rate_index = (self.rate_index + 1) % VIDEO_RATES.len();
        self.engine.set_rate(VIDEO_RATES[self.rate_index]);
        self.acted(now);
    }

    /// **The five keys the ruling named**, answered only by the surface that is
    /// holding the keyboard. `true` when the key was one of them.
    ///
    /// A modified key is never one of these: `Ctrl`, `Alt` and `Win` chords are
    /// the window's, and a player that swallowed `Ctrl+←` would be eating one of
    /// them.
    ///
    /// `physical` is the key's place on the keyboard, which the mute's letter
    /// falls back to on a layout that prints another script ([`player_key`]).
    pub fn key_press(
        &mut self,
        key: &winit::keyboard::Key,
        physical: winit::keyboard::PhysicalKey,
        modified: bool,
        now: Instant,
    ) -> bool {
        if modified {
            return false;
        }
        let Some(verb) = player_key(key, physical) else {
            return false;
        };
        match verb {
            PlayerKey::Toggle => self.toggle(now),
            PlayerKey::Start => self.seek_start(now),
            PlayerKey::End => self.seek_end(now),
            PlayerKey::FrameBack => self.step_frame(-1.0, now),
            PlayerKey::FrameForward => self.step_frame(1.0, now),
            PlayerKey::Back => self.step(-VIDEO_STEP_SECONDS, now),
            PlayerKey::Forward => self.step(VIDEO_STEP_SECONDS, now),
            PlayerKey::Louder => self.nudge_volume(VIDEO_GAIN_STEP, now),
            PlayerKey::Quieter => self.nudge_volume(-VIDEO_GAIN_STEP, now),
            PlayerKey::Mute => self.toggle_mute(now),
        }
        true
    }

    // ── the bar's own state ───────────────────────────────────────────────

    /// The reader did something. Holds the bar up and restarts the dwell.
    pub fn acted(&mut self, now: Instant) {
        self.acted_at = now;
        self.arming_at = None;
        if self.revealed_at.is_none() {
            self.revealed_at = Some(now);
        }
    }

    /// **The pointer moved over the picture.**
    ///
    /// Arms the intent once and does not restart it, which is the difference
    /// between a bar that comes up while a hand is travelling towards it and one
    /// that never comes up at all.
    pub fn pointer_moved(&mut self, over_bar: bool, now: Instant) {
        self.over_bar = over_bar;
        if over_bar {
            self.acted(now);
            return;
        }
        if self.revealed_at.is_some() {
            self.acted_at = now;
            return;
        }
        if self.arming_at.is_none() {
            self.arming_at = Some(now);
        }
    }

    /// The pointer left the picture altogether: the intent is disarmed and the
    /// dwell is allowed to run out.
    pub fn pointer_left(&mut self) {
        self.arming_at = None;
        self.over_bar = false;
    }

    /// Let an armed intent become a reveal, and let a bar that has finished
    /// leaving be forgotten. Called on the animation tick, which is the same
    /// tick the frames arrive on.
    ///
    /// **No `Motion` argument**, and the ninety milliseconds that costs are
    /// deliberate: under `Reduced` the bar leaves without a fade, so it is
    /// forgotten one archived span later than it strictly could be. A parameter
    /// threaded through two call sites to move a *forgetting* ninety
    /// milliseconds earlier would be a branch nobody could see the effect of.
    pub fn tick(&mut self, now: Instant) {
        if let Some(armed) = self.arming_at
            && now.saturating_duration_since(armed) >= VIDEO_BAR_REVEAL_INTENT
        {
            self.arming_at = None;
            self.revealed_at = Some(now);
            self.acted_at = now;
        }
        // **A bar that has *finished leaving* is a bar that has to be asked for
        // again.**
        //
        // Without this, `revealed_at` is set once at the first reveal and never
        // cleared — so the *second* time a reader's hand comes back the bar is
        // up on the first pointer move with no intent in front of it, which is a
        // bar that flashes at a hand travelling across the picture on its way
        // somewhere else.
        //
        // **The condition is the dwell and the fade, and not "the opacity is
        // zero"**, which was this line's first form and was wrong on the
        // machine: a bar that has just been asked for is *also* at zero — it is
        // one frame into its ninety-millisecond rise — so that reading cleared
        // the reveal on the very tick that granted it and the bar never appeared
        // at all. Photographed as "pressing play starts the video and draws no
        // controls"; the two states are told apart by which way the clock is
        // running, so the clock is what this asks about.
        if self.revealed_at.is_some() && self.situation().has_finished_leaving(now) {
            self.revealed_at = None;
        }
    }

    fn situation(&self) -> BarSituation {
        BarSituation {
            revealed_at: self.revealed_at,
            acted_at: self.acted_at,
            over_bar: self.over_bar,
            grabbing: self.grab.is_some(),
            paused: !self.engine.state().playing,
        }
    }

    /// How present the bar is, right now. See [`BarPresence`].
    #[must_use]
    pub fn presence(&self, now: Instant, motion: crate::Motion) -> BarPresence {
        self.situation().presence(now, motion)
    }

    /// **When this seat next needs a frame drawn for a reason that is not a new
    /// picture** — the bar rising, or resting, or fading.
    ///
    /// `None` while the bar is settled; a playing video still asks for frames,
    /// but it asks for them because it is decoding, which is a different
    /// question with a different answer.
    #[must_use]
    pub fn bar_deadline(&self, now: Instant, motion: crate::Motion) -> Option<Instant> {
        let armed = self
            .arming_at
            .map(|armed| armed + VIDEO_BAR_REVEAL_INTENT)
            .into_iter();
        armed
            .chain(self.presence(now, motion).settled_at)
            .min()
            .filter(|deadline| *deadline > now)
    }

    /// **Whether this seat's bar is in flight at `now`** — the half of
    /// [`Self::bar_deadline`] that is a tween (review round 3, 2026-09-18).
    ///
    /// The deadline folds two waits in with the fade: the intent a still pointer
    /// serves out before the bar is offered at all, and the rest a shown bar
    /// stands before it starts to go. Neither is motion, so neither may put the
    /// window's chrome lane in flight — see [`BarPresence::moving`].
    #[must_use]
    pub fn bar_is_moving(&self, now: Instant, motion: crate::Motion) -> bool {
        self.presence(now, motion).moving
    }

    /// Take hold of a track. The fraction under the pointer applies at once,
    /// which is what makes a click on a scrubber a seek and not only the
    /// beginning of a drag.
    pub fn grab(&mut self, slot: BarSlot, layout: &BarLayout, at: [f32; 2], now: Instant) {
        if !slot.is_track() {
            return;
        }
        self.grab = Some(slot);
        self.drag_to(layout, at, now);
    }

    /// Follow a held track.
    pub fn drag_to(&mut self, layout: &BarLayout, at: [f32; 2], now: Instant) {
        let Some(slot) = self.grab else { return };
        let Some(track) = layout.track_of(slot) else {
            return;
        };
        let fraction = fraction_along(track, at[0]);
        match slot {
            BarSlot::Seek => self.seek_to(fraction, now),
            BarSlot::Volume => self.set_volume(fraction, now),
            _ => {}
        }
    }

    /// Let go. `true` when something was in hand.
    pub fn release(&mut self, now: Instant) -> bool {
        let held = self.grab.take().is_some();
        if held {
            self.acted(now);
        }
        held
    }

    /// **Tell the engine to stop, and let it go** — its shutdown door
    /// (T-DROP-DOORS, D-80), which waits for nothing.
    ///
    /// The verb a surface calls when it closes or is handed a different file, on
    /// the window's own thread. Idempotent. A seat dropped without it drops its
    /// engine, whose own `Drop` makes the same request and says the door was
    /// skipped — §7.42 ⑦'s ledger still comes back to where it was, and the one
    /// wait for an engine to leave is the process's, at exit.
    pub fn shutdown(&mut self) {
        self.engine.shutdown();
    }

    // ── the bar, painted ──────────────────────────────────────────────────

    /// **The bar as chrome**, in whole-surface physical pixels — or an empty
    /// band when it is not up or there is no room for it.
    ///
    /// A [`crate::marks::Band`] of one layer because that is the one bundle
    /// every surface in this window can pour into: a float stacks it over its
    /// own layer, a pane gets it on its own, and the glance card stacks it over
    /// the card. The bundle is surface-neutral because the bar is.
    ///
    /// **The bar fades as one surface** (the fade audit's F2, ticket 46). Its
    /// presence was written by hand into each fill's alpha and each mark's
    /// opacity, and a [`ChromeLabel`] has no alpha to write it into — so in a
    /// docked pane the clock, the rate and the file's line stood solid while
    /// the panel faded in and out round them. The layer is now drawn at full
    /// strength and the presence is the band's group: every channel of it,
    /// letters included, arrives and leaves together. Inside a float or on the
    /// card it stands inside that surface's own group, and the two multiply.
    ///
    /// **Every colour is one of the palette's**, and they are the same six the
    /// shell page's `:root` carried, borrowed from the same entries: the panel
    /// and its hairline are a markdown code fence's, because this bar *is* a
    /// panel laid over a preview body and that is the one surface pairing this
    /// window already has a resolved hairline for.
    #[must_use]
    pub fn bar(
        &self,
        body: [f32; 4],
        scale: f32,
        now: Instant,
        motion: crate::Motion,
        palette: &ChromePalette,
        meta: BarMeta<'_>,
    ) -> Band {
        paint_bar(
            &self.engine.state(),
            self.grab,
            self.presence(now, motion).opacity,
            body,
            scale,
            palette,
            meta,
        )
    }
}

/// **Why a seat whose engine stands at `state` is retired, and the sentence its
/// surface prints in the player's place** — `None` while it is still a player:
/// loading, playing, ended, or loaded with no length to show.
///
/// An engine's error is sticky, so a seat on one is a rectangle that will never
/// receive another picture (§7.44 ⑥). Two sentences, because they are two
/// facts: a decoder that would not open the file — which on a machine missing a
/// Store codec is [`EngineError::Unsupported`], the honest source of
/// `Text::VideoFormatCannotPlay` now that no table can know it — and a source
/// that never finished loading ([`EngineError::NotLoaded`], the platform's own
/// refusal once its load budget has passed), which is not a claim about the
/// format at all.
#[must_use]
pub fn retirement(state: &EngineState) -> Option<(EngineError, crate::i18n::Text)> {
    let error = state.error?;
    let says = match error {
        EngineError::NotLoaded => crate::i18n::Text::VideoDidNotLoad,
        _ => crate::i18n::Text::VideoFormatCannotPlay,
    };
    Some((error, says))
}

/// [`VideoSeat::bar`] with its three readings of the seat already taken — the
/// engine's state, the hand on a track, and how present the bar is — so that
/// what it paints is a function of what it was handed and nothing else.
fn paint_bar(
    state: &EngineState,
    grab: Option<BarSlot>,
    presence: f32,
    body: [f32; 4],
    scale: f32,
    palette: &ChromePalette,
    meta: BarMeta<'_>,
) -> Band {
    if presence <= 0.0 {
        return Band::default();
    }
    let elapsed = clock_text(state.position_secs);
    let whole = clock_text(state.duration_secs.unwrap_or(0.0));
    let Some(layout) = bar_layout_of(state, body, scale, meta.width) else {
        return Band::default();
    };
    let font_px = font_logical_px() * scale;

    let mut quads: Vec<OverlayQuad> = Vec::with_capacity(12);
    let mut sprites: Vec<ChromeSprite> = Vec::with_capacity(2);
    let mut labels: Vec<ChromeLabel> = Vec::with_capacity(3);

    // The panel and its one hairline. No radius on either: the bar sits on
    // the bottom edge of a box that has already been rounded by whatever
    // drew it, and a second rounding here would be a second authority for
    // the same corner.
    quads.push(OverlayQuad {
        rect: layout.bar,
        color: palette.preview_code_ground,
        alpha: 1.0,
    });
    quads.push(OverlayQuad {
        rect: layout.edge,
        color: palette.preview_code_border,
        alpha: 1.0,
    });

    // The two two-faced controls read the engine rather than remembering
    // what they were told — a player that remembered it had pressed play
    // lies the moment a video reaches its end.
    let playing = state.playing && !state.ended;
    sprites.push(ChromeSprite::new(
        if playing {
            ChromeMark::Pause
        } else {
            ChromeMark::Play
        },
        layout.play_mark,
        palette.preview_body_text,
    ));
    if let Some(mark) = layout.mute_mark {
        let silent = state.muted || state.volume <= 0.0;
        sprites.push(ChromeSprite::new(
            if silent {
                ChromeMark::SpeakerMuted
            } else {
                ChromeMark::Speaker
            },
            mark,
            palette.preview_body_text,
        ));
    }

    let clock_label = |text: String, rect: [f32; 4], align_right: bool| ChromeLabel {
        text,
        rect,
        clip: None,
        font_size_px: font_px,
        color: palette.preview_code_text,
        align_right,
        align_center: false,
        letter_spacing_em: 0.0,
        weight: ChromeLabelWeight::Regular,
        // The one thing on this bar that changes under a fixed layout.
        tabular_numerals: true,
        mono: false,
    };
    if let Some(rect) = layout.at {
        labels.push(clock_label(elapsed, rect, false));
    }
    // **A recording that declares no length has no length to show** and
    // nothing to scrub along: its scrubber's place says so in words and its
    // duration clock stands empty, rather than a `0:00` that reads as a load
    // that has not finished.
    let lengthless = !layout.seekable;
    if let Some(rect) = layout.duration
        && !lengthless
    {
        labels.push(clock_label(whole, rect, true));
    }
    if lengthless {
        labels.push(ChromeLabel {
            text: crate::i18n::Text::VideoNoLength.text().to_owned(),
            rect: layout.seek,
            clip: Some(layout.seek),
            font_size_px: font_px,
            color: palette.files_row_muted,
            align_right: false,
            align_center: true,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            mono: false,
        });
    }
    if let Some(rect) = layout.rate {
        labels.push(clock_label(rate_text(state.rate), rect, true));
    }
    // **What the file is, at the end of the row** (owner's ruling
    // 2026-09-12). Quieter than the controls beside it, because it is the
    // one thing here that is not one: `--ink2` against their body ink, which
    // is the same step the path row's own facts take away from its
    // breadcrumbs.
    if let Some(rect) = layout.meta {
        labels.push(ChromeLabel {
            text: meta.text.to_owned(),
            rect,
            clip: Some(rect),
            font_size_px: font_px,
            color: palette.files_row_muted,
            align_right: true,
            align_center: false,
            letter_spacing_em: 0.0,
            weight: ChromeLabelWeight::Regular,
            tabular_numerals: false,
            mono: false,
        });
    }

    // The two tracks: a rail of the bar's own hairline, a fill of the
    // accent, and a dot only while the hand is on it.
    let progress = state
        .duration_secs
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map_or(0.0, |duration| {
            (state.position_secs / duration).clamp(0.0, 1.0)
        });
    let gain = if state.muted {
        0.0
    } else {
        state.volume.clamp(0.0, 1.0)
    };
    for (rail, track, fraction, slot) in [
        (!lengthless).then_some((layout.seek_rail, layout.seek, progress, BarSlot::Seek)),
        layout
            .volume_rail
            .zip(layout.volume)
            .map(|(rail, track)| (rail, track, gain, BarSlot::Volume)),
    ]
    .into_iter()
    .flatten()
    {
        quads.push(OverlayQuad {
            rect: rail,
            color: palette.preview_code_border,
            alpha: 1.0,
        });
        let filled = rail[0] + (rail[2] - rail[0]) * fraction as f32;
        if filled > rail[0] {
            quads.push(OverlayQuad {
                rect: [rail[0], rail[1], filled, rail[3]],
                color: palette.accent,
                alpha: 1.0,
            });
        }
        if grab == Some(slot) {
            let radius = (BAR_KNOB_LOGICAL_PX * scale / 2.0).max(1.0);
            let middle = (track[1] + track[3]) / 2.0;
            quads.extend(bt_render::rounded_overlay_fill(
                [
                    filled - radius,
                    middle - radius,
                    filled + radius,
                    middle + radius,
                ],
                radius,
                palette.accent,
                1.0,
            ));
        }
    }

    Band::surface(
        vec![OverlayLayer {
            quads,
            labels,
            sprites,
            ..OverlayLayer::default()
        }],
        presence,
        [0.0, 0.0],
    )
}

/// **How long one frame of this recording is**: the engine's declared rate when
/// it has one, else [`VIDEO_FALLBACK_FRAME_RATE`].
#[must_use]
fn frame_secs(state: &EngineState) -> f64 {
    1.0 / state
        .frame_rate
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .unwrap_or(VIDEO_FALLBACK_FRAME_RATE)
}

/// **How far `frames` frame steps move the playhead**, or `None` while the
/// recording plays — see [`VideoSeat::step_frame`].
#[must_use]
fn frame_step(state: &EngineState, frames: f64) -> Option<f64> {
    (!state.playing).then(|| frames * frame_secs(state))
}

/// **Whether a recording has nothing left to play**: the engine says it ended,
/// or its playhead stands on the last frame of a declared length — where `End`
/// puts a paused recording and where a scrubber dragged to the right leaves one.
/// Within half a frame, because the last frame *is* the end: a playhead the
/// engine reports a hair short of the length is on that frame.
#[must_use]
fn stands_at_end(state: &EngineState) -> bool {
    state.ended
        || state
            .duration_secs
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .is_some_and(|duration| state.position_secs >= duration - frame_secs(state) / 2.0)
}

/// **One gesture on a recording's picture**, off its bar (owner's ruling
/// 2026-10-23, row 38) — see [`VideoSeat::picture`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PictureGesture {
    /// One click: play or pause.
    Click,
}

/// **`m:ss`, and `h:mm:ss` once there is an hour to say.**
///
/// [`crate::preview::format_duration`] and not a second formatter: the facts
/// line under a video's card already prints a length, and two spellings of one
/// duration in one window is the kind of small disagreement nobody notices and
/// nobody can unsee. Seconds are truncated and never rounded — a clip does not
/// reach the second it has not got to.
#[must_use]
fn clock_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return crate::preview::format_duration(0);
    }
    crate::preview::format_duration((seconds * 1000.0) as u64)
}

/// **How many figures the two clocks have to be reserved for** — the wider of
/// the two readings.
///
/// Both boxes get the same width, which is what stops the scrubber changing
/// length as the elapsed clock crosses a minute. The hit test and the painter
/// both reach it through [`bar_layout_of`], so the boxes they compute are the
/// same boxes.
#[must_use]
fn clock_figures(position_secs: f64, duration_secs: Option<f64>) -> usize {
    let elapsed = clock_text(position_secs);
    let whole = clock_text(duration_secs.unwrap_or(0.0));
    elapsed.chars().count().max(whole.chars().count())
}

/// `1×`, `1.25×` — the speed as the button says it.
#[must_use]
fn rate_text(rate: f64) -> String {
    let rounded = (rate * 100.0).round() / 100.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{}\u{d7}", rounded.trunc() as i64)
    } else {
        format!("{rounded}\u{d7}")
    }
}

// ── the three surfaces ────────────────────────────────────────────────────

/// **Every video this window is playing, keyed by the surface it is playing
/// on.**
///
/// A map and not three fields, which is the whole of "one model, three
/// surfaces": every verb below takes a [`crate::PreviewSurface`] and none of
/// them knows which kind it is, so a gesture written once works on all three and
/// a fourth surface would be a fourth key rather than a fourth copy.
#[derive(Default)]
pub struct VideoSeats {
    seats: BTreeMap<crate::PreviewSurface, VideoSeat>,
    /// The next texture identity. Never reused for the life of the window, so
    /// two seats over one file are two pictures and a re-opened seat does not
    /// inherit the last one's frames.
    serial: u64,
}

impl VideoSeats {
    /// **Open `path` on `surface`, replacing whatever was there.**
    ///
    /// The replacement is a shutdown: a surface handed a second recording is a
    /// surface whose first engine has nothing left to decode for, and §7.42 ⑦'s
    /// counters are what would notice if it were merely dropped on the floor.
    pub fn open(
        &mut self,
        surface: crate::PreviewSurface,
        path: &Path,
        now: Instant,
    ) -> Result<(), EngineError> {
        self.close(surface);
        self.serial += 1;
        let seat = VideoSeat::open(path, self.serial, now)?;
        self.seats.insert(surface, seat);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, surface: crate::PreviewSurface) -> Option<&VideoSeat> {
        self.seats.get(&surface)
    }

    pub fn get_mut(&mut self, surface: crate::PreviewSurface) -> Option<&mut VideoSeat> {
        self.seats.get_mut(&surface)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (crate::PreviewSurface, &VideoSeat)> {
        self.seats.iter().map(|(surface, seat)| (*surface, seat))
    }

    /// **Shut down whatever is playing on `surface`.** `true` when there was
    /// something.
    pub fn close(&mut self, surface: crate::PreviewSurface) -> bool {
        match self.seats.remove(&surface) {
            Some(mut seat) => {
                seat.shutdown();
                true
            }
            None => false,
        }
    }

    /// **Carry a playing video from one surface to another** (user ruling
    /// 2026-08-28: *「拖头转浮窗时把引擎带走(不重开,位置不丢)」*).
    ///
    /// The tear-off, and the reason [`VideoSeats`] is a map rather than three
    /// fields. A glance card dragged into a floating window is the same
    /// recording at the same instant on a different surface, and the honest
    /// expression of that is **one key changing**: the engine is not touched,
    /// the decoder does not restart, the playhead does not go back to zero, and
    /// the texture keeps its name so not one frame is uploaded twice.
    ///
    /// Anything already on `to` is shut down first — a surface cannot hold two
    /// recordings, and the one arriving is the one the reader asked for.
    pub fn rehome(&mut self, from: crate::PreviewSurface, to: crate::PreviewSurface) -> bool {
        if from == to {
            return self.seats.contains_key(&from);
        }
        let Some(seat) = self.take(from) else {
            return false;
        };
        self.put(to, seat);
        true
    }

    /// **A recording lifted off its surface, still running** — the half of
    /// [`Self::rehome`] a *transaction* needs (§7.44 ⑮, 2026-08-30).
    ///
    /// A pane dropped on the centre of another tab's pane **trades**: two
    /// journeys in one gesture, and the second one's destination is the first
    /// one's origin. Run as two `rehome`s the second would arrive at a key the
    /// first had just filled and `put` would shut that engine down — a live
    /// decoder lost, with the reader watching one of the two pictures go black.
    /// So the caller takes every travelling recording off its surface before it
    /// puts any of them down, exactly as the page table's carrier does.
    ///
    /// Nothing is shut down here and nothing is started: what comes back is the
    /// same engine at the same playhead holding the same texture name, and a
    /// caller that dropped it on the floor would be caught by §7.42 ⑦'s
    /// counters.
    pub fn take(&mut self, surface: crate::PreviewSurface) -> Option<VideoSeat> {
        self.seats.remove(&surface)
    }

    /// **A recording put down on a surface**, shutting down whatever was there.
    ///
    /// [`Self::take`]'s other half, and the shutdown is the same sentence
    /// [`Self::open`] makes: a surface cannot hold two recordings, and the one
    /// arriving is the one the reader asked for.
    pub fn put(&mut self, surface: crate::PreviewSurface, seat: VideoSeat) {
        self.close(surface);
        self.seats.insert(surface, seat);
    }

    /// Take every new frame. `true` when any surface owes a redraw.
    pub fn pump(&mut self, now: Instant) -> bool {
        let mut owed = false;
        for seat in self.seats.values_mut() {
            seat.tick(now);
            owed |= seat.pump();
        }
        owed
    }

    /// **Whether any seat is still decoding**, which is what keeps the window's
    /// animation deadline live while a video plays.
    #[must_use]
    pub fn any_playing(&self) -> bool {
        self.seats.values().any(|seat| seat.state().playing)
    }

    /// The soonest a bar needs redrawing for a reason that is not a new picture.
    #[must_use]
    pub fn bar_deadline(&self, now: Instant, motion: crate::Motion) -> Option<Instant> {
        self.seats
            .values()
            .filter_map(|seat| seat.bar_deadline(now, motion))
            .min()
    }

    /// Whether any seat's bar is in flight — see [`VideoSeat::bar_is_moving`].
    #[must_use]
    pub fn bar_is_moving(&self, now: Instant, motion: crate::Motion) -> bool {
        self.seats
            .values()
            .any(|seat| seat.bar_is_moving(now, motion))
    }

    /// **Every seat shut down: each engine told to stop and let go of.** The door
    /// §7.42 ⑦'s exit protocol comes through when a window closes
    /// (`Runtime::let_go_of_this_window`, every road that ends a window); it
    /// waits for nothing (T-DROP-DOORS, D-80).
    pub fn shutdown_all(&mut self) {
        for (_, mut seat) in std::mem::take(&mut self.seats) {
            seat.shutdown();
        }
    }
}

/// **The player's keys**, as [`VideoSeat::key_press`] answers them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlayerKey {
    Toggle,
    /// `Home`: the start.
    Start,
    /// `End`: the end.
    End,
    /// `,`: one frame back, while paused.
    FrameBack,
    /// `.`: one frame forward, while paused.
    FrameForward,
    Back,
    Forward,
    Louder,
    Quieter,
    Mute,
}

/// Which of the player's keys one unmodified press is: Space, the four arrows,
/// `Home` and `End`, `,` and `.`, and `M` — the mute's letter read by
/// [`crate::input::verb_letter`], so on a layout that prints another script it
/// is the key at `M` (owner ruling 2026-10-11 on H2 Q6).
///
/// **`,` and `.` are read by the character they type** (owner's ruling
/// 2026-10-23, row 42), not by `verb_letter`: they are punctuation, which every
/// layout prints, and AZERTY prints `,` on the key a US keyboard calls `M`.
pub(crate) fn player_key(
    key: &winit::keyboard::Key,
    physical: winit::keyboard::PhysicalKey,
) -> Option<PlayerKey> {
    use winit::keyboard::{Key, NamedKey};
    match key {
        Key::Named(NamedKey::Space) => Some(PlayerKey::Toggle),
        Key::Named(NamedKey::ArrowLeft) => Some(PlayerKey::Back),
        Key::Named(NamedKey::ArrowRight) => Some(PlayerKey::Forward),
        Key::Named(NamedKey::ArrowUp) => Some(PlayerKey::Louder),
        Key::Named(NamedKey::ArrowDown) => Some(PlayerKey::Quieter),
        Key::Named(NamedKey::Home) => Some(PlayerKey::Start),
        Key::Named(NamedKey::End) => Some(PlayerKey::End),
        Key::Character(text) if text.as_str() == "," => Some(PlayerKey::FrameBack),
        Key::Character(text) if text.as_str() == "." => Some(PlayerKey::FrameForward),
        Key::Character(_) => {
            (crate::input::verb_letter(key, physical) == Some('m')).then_some(PlayerKey::Mute)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED (31) — **The video_seat values follow their UI roles.**
    ///
    /// These product constants differ from their role values on BASE.
    /// MUTATION: restore VIDEO_BAR_HEIGHT_LOGICAL_PX to 34.0.
    /// MUTATION: restore BAR_PADDING_X_LOGICAL_PX to 10.0.
    /// MUTATION: restore BAR_MARK_LOGICAL_PX to 16.0.
    #[test]
    fn ui_spec_video_seat_values_follow_the_rule() {
        let rules = [
            (
                VIDEO_BAR_HEIGHT_LOGICAL_PX,
                bt_render::SEAT_TITLE_BAR_LOGICAL_PX,
                "VIDEO_BAR_HEIGHT_LOGICAL_PX: UI-SPEC.md H6 strip",
            ),
            (
                BAR_PADDING_X_LOGICAL_PX,
                bt_render::SEAT_TITLE_PADDING_LOGICAL_PX,
                "BAR_PADDING_X_LOGICAL_PX: UI-SPEC.md S7 strip leading",
            ),
            (
                BAR_MARK_LOGICAL_PX,
                14.0,
                "BAR_MARK_LOGICAL_PX: UI-SPEC.md I2 icons::MarkSlot toolbar",
            ),
        ];
        let failures: Vec<_> = rules
            .into_iter()
            .filter(|(actual, expected, _)| actual != expected)
            .collect();
        assert!(failures.is_empty(), "{failures:?}");
    }

    /// RED (H2-NITS, owner ruling 2026-10-11 on H2 Q6; AUDIT-hardcode PRV-6) —
    /// **the player's mute is the key at `M` on a layout that prints another
    /// script there**, and a Latin layout's own `m` wherever it stands.
    ///
    /// MUTATIONS: (1) drop the physical fallback from `input::verb_letter` —
    /// the Russian `ь` goes red; (2) apply it to every letter — a US `n` on
    /// the physical `M` becomes the mute and goes red.
    #[test]
    fn the_mute_is_the_key_at_m_on_a_non_latin_layout() {
        use winit::keyboard::{Key, KeyCode, PhysicalKey};
        let at =
            |text: &str, code| player_key(&Key::Character(text.into()), PhysicalKey::Code(code));
        for (layout, m) in [("Russian", "ь"), ("Greek", "μ"), ("Hebrew", "צ")] {
            assert_eq!(at(m, KeyCode::KeyM), Some(PlayerKey::Mute), "{layout}");
            assert_eq!(
                at(&m.to_uppercase(), KeyCode::KeyM),
                Some(PlayerKey::Mute),
                "{layout} shifted"
            );
            assert_eq!(at(m, KeyCode::KeyN), None, "{layout} on another key");
        }
        assert_eq!(at("m", KeyCode::KeyM), Some(PlayerKey::Mute));
        assert_eq!(at("M", KeyCode::KeyM), Some(PlayerKey::Mute));
        // AZERTY prints `m` on the key a US keyboard calls `;`, and `,` at `M` —
        // which is the frame step back, by the character it types (row 42).
        assert_eq!(at("m", KeyCode::Semicolon), Some(PlayerKey::Mute));
        assert_eq!(at(",", KeyCode::KeyM), Some(PlayerKey::FrameBack));
        // A US keyboard's other letters are not the mute.
        assert_eq!(at("n", KeyCode::KeyM), None);
    }

    fn a_body() -> [f32; 4] {
        [100.0, 40.0, 1_060.0, 640.0]
    }

    /// RED — **the bar is laid out the way the register's numbers say, and the
    /// two waits are the ones this window already keeps** (user ruling
    /// 2026-08-28; §7.44 ②).
    ///
    /// Three claims:
    ///
    /// ① **The reveal is the house's hover intent.** A player's bar and a `⌄`'s
    /// menu are the same promise made twice, and two answers to it would be the
    /// window disagreeing with itself about how long a hand takes to mean
    /// something. This is the assertion the retired shell page carried in
    /// `the_bars_waits_are_the_ones_the_motion_register_holds`, and it survives
    /// the page it was written for.
    ///
    /// ② **The rest is the dwell after**, on `termscroll::THUMB_REST`'s
    /// precedent.
    ///
    /// ③ **The fade is an archived span** and not a fourth number: ninety
    /// milliseconds, the same one every hover fade in this window is drawn on.
    ///
    /// RED GATE: type a number into any of the three — 300ms for the intent is
    /// the natural mistake, because it is what a player "feels like" — and the
    /// assertion that names the register's own constant fails while the bar goes
    /// on working.
    #[test]
    fn the_bar_rises_on_hover_and_rests_by_the_registers_numbers() {
        assert_eq!(
            VIDEO_BAR_REVEAL_INTENT,
            crate::profiles::CHEVRON_HOVER_OPEN_DELAY,
            "a player's bar and a `⌄`'s menu are the same promise, made twice"
        );
        assert_eq!(VIDEO_BAR_IDLE_REST, Duration::from_millis(2_000));
        assert_eq!(VIDEO_BAR_FADE, bt_render::MOTION_FAST);
        assert!(
            bt_render::MOTION_ARCHIVE_MS.contains(&(VIDEO_BAR_FADE.as_millis() as u64)),
            "the bar's fade is one of the three archived spans, not a fourth"
        );

        // And the state machine those three durations drive, end to end on one
        // clock — arming, rising, standing, resting, gone.
        let start = Instant::now();
        let mut situation = BarSituation {
            revealed_at: None,
            acted_at: start,
            over_bar: false,
            grabbing: false,
            paused: false,
        };
        assert_eq!(situation.presence(start, crate::Motion::Full).opacity, 0.0);
        situation.revealed_at = Some(start);
        situation.acted_at = start;
        assert_eq!(situation.presence(start, crate::Motion::Full).opacity, 0.0);
        let risen = start + VIDEO_BAR_FADE;
        assert_eq!(situation.presence(risen, crate::Motion::Full).opacity, 1.0);
        // It stands for the whole dwell and is still whole at the end of it.
        let resting = start + VIDEO_BAR_IDLE_REST;
        assert_eq!(
            situation.presence(resting, crate::Motion::Full).opacity,
            1.0
        );
        // Then it goes, over one archived span.
        let half = resting + VIDEO_BAR_FADE / 2;
        let midway = situation.presence(half, crate::Motion::Full).opacity;
        assert!((0.1..0.9).contains(&midway), "{midway}");
        let gone = resting + VIDEO_BAR_FADE;
        assert_eq!(situation.presence(gone, crate::Motion::Full).opacity, 0.0);

        // The three states that are not "no action" hold it up for ever.
        for (over_bar, grabbing, paused) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let held = BarSituation {
                revealed_at: Some(start),
                acted_at: start,
                over_bar,
                grabbing,
                paused,
            };
            assert_eq!(
                held.presence(gone + VIDEO_BAR_IDLE_REST, crate::Motion::Full)
                    .opacity,
                1.0,
                "over_bar={over_bar} grabbing={grabbing} paused={paused}"
            );
        }

        // Reduced motion refuses the fade and not the bar: it arrives whole and
        // leaves whole.
        assert_eq!(
            situation.presence(start, crate::Motion::Reduced).opacity,
            1.0
        );
        assert_eq!(
            situation.presence(gone, crate::Motion::Reduced).opacity,
            0.0
        );
    }

    /// RED — **a bar that is standing still is not a bar in motion** (review
    /// round 3, 2026-09-18).
    ///
    /// The window's audit of what is mid-flight could not reach this seat — it
    /// owns a decoder on another thread — so the property every other journey is
    /// put through in `journeys_tests` is pinned here, on the arithmetic: the
    /// bar is moving exactly while its opacity is changing, and the two clocks
    /// that are not fades report nothing at all. The window read
    /// `bar_deadline(…).is_some()` as this seat's liveness, which is `Some`
    /// through both of those waits, so a paused recording with its bar up held
    /// the chrome lane open and a neighbouring pane printing rebuilt the strip
    /// on every present.
    ///
    /// MUTATION: answer `settled_at.is_some()` in [`BarPresence::moving`] and
    /// the dwell and the held bar below both report motion.
    #[test]
    fn a_resting_bar_is_not_a_moving_one() {
        let start = Instant::now();
        let situation = BarSituation {
            revealed_at: Some(start),
            acted_at: start,
            over_bar: false,
            grabbing: false,
            paused: false,
        };
        let moving = |at: Instant| situation.presence(at, crate::Motion::Full).moving;
        let woken = |at: Instant| {
            situation
                .presence(at, crate::Motion::Full)
                .settled_at
                .is_some()
        };

        // Rising: moving, and woken for the end of the rise.
        assert!(moving(start) && woken(start));
        assert!(moving(start + VIDEO_BAR_FADE / 2));
        // Up, and standing out its two seconds. The loop is still woken — the
        // rest ends at an instant — and nothing is in flight.
        let risen = start + VIDEO_BAR_FADE;
        assert!(!moving(risen) && woken(risen), "the rise has landed");
        let resting = start + VIDEO_BAR_IDLE_REST;
        assert!(!moving(resting - VIDEO_BAR_FADE), "standing is not moving");
        assert!(woken(resting - VIDEO_BAR_FADE), "and it is still woken");
        // Going, and then gone: moving for exactly one archived span.
        assert!(moving(resting));
        assert!(moving(resting + VIDEO_BAR_FADE / 2));
        let gone = resting + VIDEO_BAR_FADE;
        assert!(
            !moving(gone) && !woken(gone),
            "a bar that has gone is quiet"
        );

        // A hand on the bar, a track in hand or a paused player holds it up for
        // ever — and a bar held up is a bar standing still.
        for (over_bar, grabbing, paused) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let held = BarSituation {
                revealed_at: Some(start),
                acted_at: start,
                over_bar,
                grabbing,
                paused,
            };
            assert!(
                !held.presence(gone, crate::Motion::Full).moving,
                "over_bar={over_bar} grabbing={grabbing} paused={paused}: a bar that is \
                 held up is not travelling"
            );
        }

        // And under reduced motion there is no fade to be in the middle of.
        for at in [start, risen, resting, gone] {
            assert!(!situation.presence(at, crate::Motion::Reduced).moving);
        }
    }

    /// RED — **the video bar is flush with the stage's bottom edge, and leaves
    /// nothing behind it when it hides** (owner's ruling 2026-09-12; §7.1.3x ④;
    /// §7.44 ⑯; mock §一).
    ///
    /// The owner photographed an empty band under a video's controls. The bar
    /// itself was always drawn on the picture; what stood under it was the
    /// preview's foot, and that is what the ruling retired. The bar's own half
    /// of the ruling is the two facts below: it sits on the bottom edge of the
    /// box it is laid out in and spans it, so when it fades there is picture
    /// underneath and nothing else; and the recording's own facts — `MP4 ·
    /// 38 MB`, which used to stand on a line under the frame — are at its right
    /// end beside the speed.
    ///
    /// RED GATE ①: lay the bar anywhere but on the box's floor and the first
    /// block fails. RED GATE ②: put the facts inboard of the speed, or drop the
    /// speed before them as the box narrows, and the rest does.
    #[test]
    fn the_video_bar_is_flush_with_the_stage_bottom_and_leaves_nothing_when_hidden() {
        let scale = 1.0;
        let body = a_body();
        let layout = bar_layout(body, scale, 4, 90.0).expect("a pane fits the whole bar");
        // ① Flush on the bottom edge, spanning it: there is no rectangle here
        // that is neither picture nor bar, so a bar that has faded leaves the
        // picture running to the floor.
        assert_eq!(
            layout.bar[3], body[3],
            "the bar floats above the stage's floor"
        );
        assert_eq!([layout.bar[0], layout.bar[2]], [body[0], body[2]]);
        assert_eq!(
            layout.bar[3] - layout.bar[1],
            (VIDEO_BAR_HEIGHT_LOGICAL_PX * scale).round(),
            "the bar is not its own height"
        );
        // ② The facts at the right end, beside the speed.
        let meta = layout.meta.expect("a recording says what it is");
        let rate = layout.rate.expect("and how fast it is running");
        assert!(
            rate[2] <= meta[0],
            "the facts stand inboard of the speed: {meta:?} {rate:?}"
        );
        assert!(
            meta[2] <= layout.bar[2],
            "the facts run off the end of the bar"
        );
        assert!(
            meta[2] - meta[0] >= 90.0,
            "and they are cut to less than they measured"
        );
        // ③ They are the first thing to give way, because they are the one thing
        // on this row that is a fact rather than a verb.
        let mut narrow = body;
        narrow[2] = body[0] + 320.0;
        let squeezed = bar_layout(narrow, scale, 4, 90.0).expect("a narrow pane is still a player");
        assert_eq!(
            squeezed.meta, None,
            "the speed was dropped before the facts were"
        );
        assert!(squeezed.rate.is_some(), "and the speed went with them");
        // A recording whose facts have not been measured yet reserves nothing
        // for them, which is the honest answer on the first frame.
        assert_eq!(bar_layout(body, scale, 4, 0.0).expect("a bar").meta, None);
    }

    /// RED — **the bar keeps play and the scrubber and gives up the rest from
    /// the right** (user ruling 2026-08-28; §7.44 ②).
    ///
    /// The bar is the same bar on a pane and on a three-hundred-pixel glance
    /// card, so something has to go as the box narrows. What is pinned here is
    /// the *order*: speed, volume, mute, duration, elapsed — and that play and
    /// the scrubber never go, because a player with neither is not a player.
    ///
    /// RED GATE: drop the elapsed clock before the speed — which is what a
    /// layout that shed from the left would do — and the widths at which each
    /// control disappears come back in the wrong order.
    #[test]
    fn the_bar_sheds_its_controls_from_the_right_and_never_its_player() {
        let scale = 1.0;
        let wide = bar_layout(a_body(), scale, 4, 0.0).expect("a pane fits the whole bar");
        assert!(wide.at.is_some() && wide.duration.is_some());
        assert!(wide.mute.is_some() && wide.volume.is_some() && wide.rate.is_some());
        // The scrubber took the leftover, and every box is inside the bar.
        assert!(wide.seek[2] > wide.seek[0]);
        for rect in [wide.play, wide.seek, wide.edge] {
            assert!(rect[0] >= wide.bar[0] && rect[2] <= wide.bar[2], "{rect:?}");
        }

        // Narrowing the body sheds them in one order and only in that order.
        let mut lost: Vec<&'static str> = Vec::new();
        let mut width = a_body()[2] - a_body()[0];
        let mut previous = wide;
        while width > 1.0 {
            width -= 4.0;
            let body = [100.0, 40.0, 100.0 + width, 640.0];
            let Some(layout) = bar_layout(body, scale, 4, 0.0) else {
                break;
            };
            for (name, was, now) in [
                ("rate", previous.rate.is_some(), layout.rate.is_some()),
                ("volume", previous.volume.is_some(), layout.volume.is_some()),
                ("mute", previous.mute.is_some(), layout.mute.is_some()),
                (
                    "duration",
                    previous.duration.is_some(),
                    layout.duration.is_some(),
                ),
                ("at", previous.at.is_some(), layout.at.is_some()),
            ] {
                if was && !now {
                    lost.push(name);
                }
            }
            // These two are never lost while there is a bar at all.
            assert!(layout.play[2] > layout.play[0]);
            assert!(layout.seek[2] > layout.seek[0]);
            previous = layout;
        }
        assert_eq!(lost, ["rate", "volume", "mute", "duration", "at"]);

        // A box with no room for a bar has none, rather than a bar drawn off its
        // own edge.
        assert!(bar_layout([0.0, 0.0, 40.0, 400.0], scale, 4, 0.0).is_none());
        assert!(bar_layout([0.0, 0.0, 900.0, 4.0], scale, 4, 0.0).is_none());
    }

    /// RED — **a bar that has just been asked for is not forgotten on the tick
    /// that granted it** (found on the machine, 2026-08-28; §7.44 ②).
    ///
    /// The defect, in full, because it is the one this slice shipped and had to
    /// photograph to find. [`VideoSeat::tick`] forgets a reveal once the bar has
    /// gone, so that a second hover has to serve out the intent again rather
    /// than flashing the bar at a hand on its way past. The first spelling of
    /// "has gone" was `presence(now).opacity <= 0.0` — and a bar **one frame
    /// into its ninety-millisecond rise is also at zero**. So pressing play
    /// granted a reveal and cleared it in the same tick, for ever: the recording
    /// played and drew no controls at all, on every surface, every time.
    ///
    /// Both ends of the number are asserted here, because the whole mistake was
    /// reading one end for the other:
    ///
    /// ① **At the instant of the reveal, and all the way through the rise**, the
    ///    bar has *not* finished leaving.
    /// ② **After the dwell and the fade**, it has.
    /// ③ **And never while something is holding it up**, however long it has
    ///    been since the reader last did anything — a paused player's bar does
    ///    not expire.
    ///
    /// RED GATE: put `opacity <= 0.0` back and ① fails at the first instant,
    /// which is exactly the frame the machine failed on.
    #[test]
    fn a_bar_that_was_just_asked_for_is_not_forgotten_on_the_same_tick() {
        let start = Instant::now();
        let fresh = BarSituation {
            revealed_at: Some(start),
            acted_at: start,
            over_bar: false,
            grabbing: false,
            paused: false,
        };
        // ① the rise, from its first instant to its last.
        assert_eq!(fresh.presence(start, crate::Motion::Full).opacity, 0.0);
        for ms in [0_u64, 1, 45, 89, 90, 500, 1_999] {
            assert!(
                !fresh.has_finished_leaving(start + Duration::from_millis(ms)),
                "at {ms}ms the bar is still owed"
            );
        }
        // ② and then it is over — the dwell plus the fade, and not a moment
        //    before.
        let over = VIDEO_BAR_IDLE_REST + VIDEO_BAR_FADE;
        assert!(!fresh.has_finished_leaving(start + over - Duration::from_millis(1)));
        assert!(fresh.has_finished_leaving(start + over));
        // ③ and never while anything is holding it up.
        for (over_bar, grabbing, paused) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let held = BarSituation {
                over_bar,
                grabbing,
                paused,
                ..fresh
            };
            assert!(
                !held.has_finished_leaving(start + over + VIDEO_BAR_IDLE_REST),
                "over_bar={over_bar} grabbing={grabbing} paused={paused}"
            );
        }
    }

    /// PIN — **a press lands on the control it looks like it landed on.**
    ///
    /// The two tracks answer through a grown box and the buttons through the box
    /// they are drawn in, which is the ordinary difference between a two-pixel
    /// rail and a twenty-two-pixel button. Asserted at the centre of every
    /// control and at one point that is on the bar and on nothing.
    #[test]
    fn every_control_answers_where_it_is_drawn() {
        let layout = bar_layout(a_body(), 1.0, 4, 0.0).expect("a bar");
        let centre = |rect: [f32; 4]| [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0];
        assert_eq!(
            layout.slot_at(centre(layout.play)),
            Some(BarSlot::PlayPause)
        );
        assert_eq!(layout.slot_at(centre(layout.seek)), Some(BarSlot::Seek));
        assert_eq!(
            layout.slot_at(centre(layout.mute.expect("wide"))),
            Some(BarSlot::Mute)
        );
        assert_eq!(
            layout.slot_at(centre(layout.volume.expect("wide"))),
            Some(BarSlot::Volume)
        );
        assert_eq!(
            layout.slot_at(centre(layout.rate.expect("wide"))),
            Some(BarSlot::Rate)
        );
        // The rail is two pixels and the reach is eighteen: a hand four pixels
        // above the seek rail is still on the scrubber.
        let rail = layout.seek_rail;
        let above = [(rail[0] + rail[2]) / 2.0, rail[1] - 4.0];
        assert_eq!(layout.slot_at(above), Some(BarSlot::Seek));
        // The bar's own ground is not a control, and it still holds the bar up.
        let ground = [layout.bar[0] + 1.0, layout.bar[1] + 1.0];
        assert_eq!(layout.slot_at(ground), None);
        assert!(layout.holds(ground));
        assert!(!layout.holds([layout.bar[0] + 1.0, layout.bar[1] - 10.0]));
    }

    /// PIN — **a fraction along a track is a fraction, and a collapsed track is
    /// not a division.**
    #[test]
    fn a_track_reads_a_fraction_and_a_collapsed_one_reads_nothing() {
        let track = [100.0, 0.0, 300.0, 10.0];
        assert!((fraction_along(track, 100.0) - 0.0).abs() < 1e-6);
        assert!((fraction_along(track, 200.0) - 0.5).abs() < 1e-6);
        assert!((fraction_along(track, 300.0) - 1.0).abs() < 1e-6);
        // Off either end is clamped, not extrapolated.
        assert!((fraction_along(track, -50.0) - 0.0).abs() < 1e-6);
        assert!((fraction_along(track, 900.0) - 1.0).abs() < 1e-6);
        assert!((fraction_along([100.0, 0.0, 100.0, 10.0], 100.0) - 0.0).abs() < 1e-6);
    }

    /// PIN — **the clock and the speed read the way a player says them.**
    ///
    /// The clock is [`crate::preview::format_duration`]'s and not a second
    /// spelling, which is what stops the bar and the facts line under a card
    /// printing one duration two ways.
    #[test]
    fn the_clock_and_the_speed_say_what_they_are() {
        assert_eq!(clock_text(0.0), "0:00");
        assert_eq!(clock_text(6.2), "0:06");
        assert_eq!(clock_text(65.0), "1:05");
        assert_eq!(clock_text(3_600.0), "1:00:00");
        // A duration that has not arrived is not an hour of nothing.
        assert_eq!(clock_text(f64::NAN), "0:00");
        assert_eq!(clock_text(-4.0), "0:00");
        assert_eq!(clock_text(1.0), crate::preview::format_duration(1_000));

        assert_eq!(rate_text(1.0), "1\u{d7}");
        assert_eq!(rate_text(1.25), "1.25\u{d7}");
        assert_eq!(rate_text(1.5), "1.5\u{d7}");
        assert_eq!(rate_text(2.0), "2\u{d7}");
        assert_eq!(VIDEO_RATES, [1.0, 1.25, 1.5, 2.0]);
    }

    /// RED (46) — **the docked bar's clock fades with its panel, because the
    /// bar is one surface.**
    ///
    /// The fade audit's gate (f) and its F2. The bar's presence was written by
    /// hand into each fill's alpha and each mark's opacity, and a label has no
    /// alpha — so in a docked pane the clock, the rate and the file's line
    /// stood solid while the panel faded in and out round them, popping off at
    /// the end. The bar is now drawn at full strength and handed over as one
    /// span at its presence, over the one layer that holds the panel *and* the
    /// letters; the renderer's `overlay_groups` readbacks show a letter inside a
    /// span arriving with its plate.
    ///
    /// MUTATION: write `presence` into the quads' alpha and the marks' opacity
    /// again and return the layer with no span — the parts carry the fade and
    /// the letters do not.
    #[test]
    fn the_docked_bars_clock_fades_with_its_panel() {
        let state = EngineState {
            duration_secs: Some(95.0),
            position_secs: 12.0,
            playing: true,
            ready: true,
            has_video: true,
            has_audio: true,
            volume: 1.0,
            rate: 1.0,
            ..EngineState::default()
        };
        let palette = bt_render::chrome_palette();
        let bar = paint_bar(
            &state,
            None,
            0.5,
            [0.0, 0.0, 900.0, 500.0],
            1.0,
            &palette,
            BarMeta::none(),
        );
        assert_eq!(
            bar.layers.len(),
            1,
            "the panel and its letters are one layer"
        );
        let layer = &bar.layers[0];
        assert!(
            layer.labels.iter().any(|label| label.text == "0:12"),
            "the clock is on the bar: {:?}",
            layer.labels
        );
        assert_eq!(bar.groups.len(), 1);
        assert_eq!(
            bar.groups[0].layers,
            0..1,
            "one surface over panel and clock"
        );
        assert!((bar.groups[0].opacity - 0.5).abs() < 1e-6);
        assert!(
            layer
                .quads
                .iter()
                .all(|quad| (quad.alpha - 1.0).abs() < 1e-6)
                && layer
                    .sprites
                    .iter()
                    .all(|mark| (mark.opacity - 1.0).abs() < 1e-6),
            "and no part wears the fade itself"
        );
        assert!(
            paint_bar(
                &state,
                None,
                0.0,
                [0.0, 0.0, 900.0, 500.0],
                1.0,
                &palette,
                BarMeta::none()
            )
            .is_empty(),
            "a bar that is not there is no surface"
        );
    }

    /// RED — **a recording that declares no length says so where its scrubber
    /// would be** (T-VIDEO-ENGINE-NO-DURATION).
    ///
    /// The platform says a loaded source with no length — a live stream, a
    /// container written without a duration — as `ready` with no
    /// `duration_secs`. Drawn as a loading player would be, that is a `0:00`
    /// clock and an empty scrubber that never fills: a load that looks as
    /// though it has not finished. The bar says "no length" in the scrubber's
    /// place instead, strikes no track a hand could think it scrubs, and leaves
    /// the duration clock empty; the elapsed clock still runs. A recording with
    /// a length, and one still loading, keep the bar they had.
    ///
    /// The press agrees with the picture: where the words stand, a hand takes
    /// hold of no scrubber.
    ///
    /// MUTATION: make `bar_layout_of` answer `seekable: true` for every state —
    /// a source with no length drawn and pressed as one still loading — and the
    /// first assertion goes red.
    #[test]
    fn a_recording_with_no_length_says_so_where_its_scrubber_would_be() {
        let body = [0.0, 0.0, 900.0, 500.0];
        let palette = bt_render::chrome_palette();
        let state_of = |duration_secs: Option<f64>, ready: bool| EngineState {
            duration_secs,
            position_secs: 12.0,
            playing: true,
            ready,
            has_video: true,
            volume: 1.0,
            rate: 1.0,
            ..EngineState::default()
        };
        let no_length = crate::i18n::Text::VideoNoLength.text();
        let painted = |state: &EngineState| {
            let layout = bar_layout_of(state, body, 1.0, 0.0).expect("a pane fits the whole bar");
            let bar = paint_bar(state, None, 1.0, body, 1.0, &palette, BarMeta::none());
            (layout, bar.layers[0].clone())
        };

        let lengthless = state_of(None, true);
        assert_eq!(lengthless.length(), Length::Unknown);
        let (layout, layer) = painted(&lengthless);
        assert!(
            layer
                .labels
                .iter()
                .any(|label| label.text == no_length && label.rect == layout.seek),
            "the scrubber's place says there is no length: {:?}",
            layer.labels
        );
        let duration = layout.duration.expect("a wide bar has its duration clock");
        assert!(
            layer.labels.iter().all(|label| label.rect != duration),
            "no duration clock for a recording with no length: {:?}",
            layer.labels
        );
        assert!(
            layer.quads.iter().all(|quad| quad.rect != layout.seek_rail),
            "no track is struck for a recording with nothing to scrub along"
        );
        let on_the_words = [
            (layout.seek[0] + layout.seek[2]) / 2.0,
            (layout.seek[1] + layout.seek[3]) / 2.0,
        ];
        assert_eq!(
            layout.slot_at(on_the_words),
            None,
            "a press on the words takes hold of nothing"
        );
        assert!(
            layer.labels.iter().any(|label| label.text == "0:12"),
            "the elapsed clock still runs: {:?}",
            layer.labels
        );

        for (state, what) in [
            (state_of(Some(95.0), true), "a recording with a length"),
            (state_of(None, false), "a recording still loading"),
        ] {
            let (layout, layer) = painted(&state);
            assert!(
                layer.labels.iter().all(|label| label.text != no_length),
                "{what} is not said to have no length: {:?}",
                layer.labels
            );
            assert!(
                layer.quads.iter().any(|quad| quad.rect == layout.seek_rail),
                "{what} keeps its scrubber"
            );
            let on_the_track = [
                (layout.seek[0] + layout.seek[2]) / 2.0,
                (layout.seek[1] + layout.seek[3]) / 2.0,
            ];
            assert_eq!(
                layout.slot_at(on_the_track),
                Some(BarSlot::Seek),
                "{what}'s scrubber answers a press"
            );
        }
    }

    /// RED — **a video that never finished loading is retired with its own
    /// sentence, and a player that is still one is not**
    /// (T-VIDEO-ENGINE-NO-DURATION).
    ///
    /// The platform refuses a source that said nothing within its load budget
    /// as `EngineError::NotLoaded`. Until then a surface showing it was a player
    /// at `0:00` that never started; the sweep (`Runtime::sweep_video_seats`)
    /// takes every seat [`retirement`] names off its surface and prints the
    /// sentence it names in the player's place. A refusal of the *format* and a
    /// load that never finished are two sentences, because they are two facts;
    /// a player still loading, and one loaded with no length, are not retired.
    ///
    /// MUTATION: answer `None` for `EngineError::NotLoaded` in [`retirement`] —
    /// the never-starting player kept on the surface — and the first assertion
    /// goes red.
    #[test]
    fn a_video_that_never_loaded_is_retired_with_its_own_sentence() {
        let refused = EngineState {
            error: Some(EngineError::NotLoaded),
            ..EngineState::default()
        };
        assert_eq!(
            retirement(&refused),
            Some((EngineError::NotLoaded, crate::i18n::Text::VideoDidNotLoad)),
            "a load that never finished is retired, and says so"
        );
        assert_eq!(
            retirement(&EngineState {
                error: Some(EngineError::Unsupported),
                ..EngineState::default()
            }),
            Some((
                EngineError::Unsupported,
                crate::i18n::Text::VideoFormatCannotPlay
            )),
            "a format the decoder refused keeps its own sentence"
        );
        assert_eq!(
            retirement(&EngineState::default()),
            None,
            "a player still loading is still a player"
        );
        assert_eq!(
            retirement(&EngineState {
                ready: true,
                has_video: true,
                natural_size: Some((160, 120)),
                ..EngineState::default()
            }),
            None,
            "a player with no length is still a player"
        );
    }
    /// RED (T-VIDEO-POINTER-AND-KEYS, row 42) — **`Home`, `End`, `,` and `.`
    /// are the player's**, `Home`/`End` by name and the two punctuation keys by
    /// the character they type, wherever a layout prints them.
    ///
    /// MUTATION: drop the `","` arm from [`player_key`] — `,` falls to the
    /// mute's letter test and answers nothing, and the first line goes red.
    #[test]
    fn home_end_and_the_two_frame_keys_are_the_players() {
        use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
        let at = |key: Key, code| player_key(&key, PhysicalKey::Code(code));
        let typed = |text: &str| Key::Character(text.into());
        assert_eq!(at(typed(","), KeyCode::Comma), Some(PlayerKey::FrameBack));
        assert_eq!(
            at(typed("."), KeyCode::Period),
            Some(PlayerKey::FrameForward)
        );
        // AZERTY: `,` at the US `M`, and `.` is shifted `;` — read by what it types.
        assert_eq!(at(typed(","), KeyCode::KeyM), Some(PlayerKey::FrameBack));
        assert_eq!(
            at(typed("."), KeyCode::Comma),
            Some(PlayerKey::FrameForward)
        );
        // What a US keyboard types shifted on those keys is not a frame step.
        assert_eq!(at(typed("<"), KeyCode::Comma), None);
        assert_eq!(at(typed(">"), KeyCode::Period), None);
        assert_eq!(
            at(Key::Named(NamedKey::Home), KeyCode::Home),
            Some(PlayerKey::Start)
        );
        assert_eq!(
            at(Key::Named(NamedKey::End), KeyCode::End),
            Some(PlayerKey::End)
        );
    }

    /// RED (row 42) — **a frame is the engine's declared rate's, else a
    /// thirtieth of a second; and a playhead on the last frame is at the end**.
    ///
    /// MUTATION: compare `position_secs >= duration` in [`stands_at_end`] — a
    /// playhead the engine reports a hair short of the length reads as not at
    /// the end, and the third assertion block goes red.
    #[test]
    fn a_frame_is_the_declared_rates_and_the_last_frame_is_the_end() {
        let at = |position_secs: f64, frame_rate: Option<f64>| EngineState {
            duration_secs: Some(5.0),
            position_secs,
            frame_rate,
            ..EngineState::default()
        };
        assert!((frame_secs(&at(0.0, Some(5.0))) - 0.2).abs() < 1e-12);
        assert!((frame_secs(&at(0.0, Some(25.0))) - 0.04).abs() < 1e-12);
        for nothing in [None, Some(0.0), Some(f64::NAN), Some(-30.0)] {
            assert!(
                (frame_secs(&at(0.0, nothing)) - 1.0 / 30.0).abs() < 1e-12,
                "{nothing:?} is no rate, so a frame is a thirtieth"
            );
        }
        // On the last frame, or a hair short of it, is the end.
        assert!(stands_at_end(&at(5.0, Some(5.0))));
        assert!(stands_at_end(&at(4.95, Some(5.0))));
        // One frame short is not: there is a picture left to see.
        assert!(!stands_at_end(&at(4.8, Some(5.0))));
        assert!(!stands_at_end(&at(0.0, Some(5.0))));
        // The engine's word is enough, and a recording with no length has no
        // last frame to stand on.
        assert!(stands_at_end(&EngineState {
            ended: true,
            ..at(1.0, None)
        }));
        assert!(!stands_at_end(&EngineState {
            duration_secs: None,
            ..at(1_000.0, None)
        }));
    }

    /// RED (row 42) — **a frame step moves a paused playhead by one frame and
    /// a playing one not at all.**
    ///
    /// MUTATION: drop the `!state.playing` test from [`frame_step`] — a
    /// playing recording jumps a frame and the first assertion goes red.
    #[test]
    fn a_frame_step_is_one_frame_and_only_while_paused() {
        let state = |playing: bool, frame_rate: Option<f64>| EngineState {
            playing,
            frame_rate,
            duration_secs: Some(5.0),
            ..EngineState::default()
        };
        assert_eq!(frame_step(&state(true, Some(5.0)), 1.0), None);
        assert_eq!(frame_step(&state(true, None), -1.0), None);
        let forward = frame_step(&state(false, Some(5.0)), 1.0).expect("paused");
        assert!((forward - 0.2).abs() < 1e-12, "{forward}");
        let back = frame_step(&state(false, Some(5.0)), -1.0).expect("paused");
        assert!((back + 0.2).abs() < 1e-12, "{back}");
        let unrated = frame_step(&state(false, None), 1.0).expect("paused");
        assert!((unrated - 1.0 / 30.0).abs() < 1e-12, "{unrated}");
    }

    /// The fixture every host's engine decodes: 160×120, five seconds, five
    /// frames a second (`tests/assets/PROVENANCE.md`).
    fn the_fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets/folio-video-test.mp4")
    }

    /// A seat on [`the_fixture`], loaded and playing as a seat opens.
    fn a_playing_seat() -> VideoSeat {
        let seat = VideoSeat::open(&the_fixture(), 1, Instant::now()).expect("the fixture opens");
        let state = seat.state_reaching(|state| state.ready && state.playing);
        assert!(state.ready && state.playing, "the fixture plays: {state:?}");
        seat
    }

    /// Within one frame's tolerance of `seconds`.
    fn near(state: &EngineState, seconds: f64) -> bool {
        (state.position_secs - seconds).abs() < 0.05
    }

    /// A key the player answers, pressed with no modifier.
    fn press(seat: &mut VideoSeat, key: winit::keyboard::Key, code: winit::keyboard::KeyCode) {
        let physical = winit::keyboard::PhysicalKey::Code(code);
        assert!(
            seat.key_press(&key, physical, false, Instant::now()),
            "{key:?}"
        );
    }

    /// RED (T-VIDEO-POINTER-AND-KEYS, row 42) — **on a real engine, `Home`
    /// and `End` bound the recording and `,`/`.` step a paused one a frame at
    /// a time, at the rate the file declares**; `End` on a playing recording
    /// plays into its end, which the engine then says.
    ///
    /// MUTATION: measure a frame by [`VIDEO_FALLBACK_FRAME_RATE`] always — the
    /// fixture's 0.2 s frames step by 0.033 s and the first `.` goes red.
    #[test]
    fn the_frame_keys_and_home_and_end_drive_a_real_recording() {
        use winit::keyboard::{Key, KeyCode, NamedKey};
        let _ledger = crate::test_support::ledger_gate();
        let before = bt_platform::video::engine::engines_outstanding();
        let mut seat = a_playing_seat();
        seat.toggle(Instant::now());
        seat.state_reaching(|state| !state.playing && state.frame_rate.is_some());

        press(&mut seat, Key::Named(NamedKey::Home), KeyCode::Home);
        let start = seat.state_reaching(|state| near(state, 0.0));
        assert!(near(&start, 0.0), "Home is the start: {start:?}");
        for frame in 1..=2 {
            press(&mut seat, Key::Character(".".into()), KeyCode::Period);
            let at = f64::from(frame) * 0.2;
            let state = seat.state_reaching(|state| near(state, at));
            assert!(near(&state, at), "`.` #{frame} stands at {at}: {state:?}");
        }
        press(&mut seat, Key::Character(",".into()), KeyCode::Comma);
        let back = seat.state_reaching(|state| near(state, 0.2));
        assert!(near(&back, 0.2), "`,` is one frame back: {back:?}");
        assert!(!back.playing, "a frame step does not play");

        press(&mut seat, Key::Named(NamedKey::End), KeyCode::End);
        let end = seat.state_reaching(|state| state.position_secs >= 4.9);
        assert!(end.position_secs >= 4.9, "End is the end: {end:?}");
        assert!(
            stands_at_end(&end),
            "and a paused one stands there: {end:?}"
        );

        // Playing, `,` and `.` change nothing a hand would see as a step, and
        // `End` lets it run out.
        press(&mut seat, Key::Named(NamedKey::Home), KeyCode::Home);
        seat.toggle(Instant::now());
        let playing = seat.state_reaching(|state| state.playing && state.position_secs < 2.0);
        assert!(playing.playing, "{playing:?}");
        press(&mut seat, Key::Named(NamedKey::End), KeyCode::End);
        let ended = seat.state_reaching(|state| state.ended);
        assert!(
            ended.ended,
            "End on a playing recording makes it end: {ended:?}"
        );
        seat.shutdown();
        crate::test_support::engines_settling_to(before);
    }
}
