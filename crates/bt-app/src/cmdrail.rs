//! **The command marks rail** — the column of ticks down a terminal pane's right
//! edge, one per command the shell reported (`design/ui-mockup.html` 1346-1383
//! and 4603-4707; `docs/DESIGN.md` §7.1.5c).
//!
//! # What it is, in the one sentence the ruling gives it
//!
//! *"Codex-style (user ruling 2026-07-18): an ORDINAL list, **not a minimap** —
//! evenly spaced ticks in a block centred on the pane's right edge."* Everything
//! awkward about this file follows from that sentence and from nothing else. The
//! ticks are **not** at the height their commands are at: position carries
//! *order*, and the ordinal stack is oldest at the top, which is why nothing here
//! ever consults a scroll offset or a projected y. A minimap would have to be
//! rebuilt whenever the viewport moved; this has to be rebuilt when the ledger
//! changes and when the pane is resized, and those are the only two, which is
//! what [`RailKey`] exists to state.
//!
//! # Why the lane, and why the number is not in this file
//!
//! [`bt_render::TERMINAL_SCROLL_LANE_LOGICAL_PX`] carries the whole argument: the
//! stylesheet's own comment beside `right: 11px` is an accident report — *"the
//! rail and the thumb are different instruments and may not share a lane (user
//! report 2026-07-18 — ticks sat on top of the thumb)"* — and eleven is that
//! lane's eight plus a three-pixel gap. Deriving the inset here rather than
//! writing `11.0` is what makes the accident unrepeatable when the terminal's own
//! scroll bar lands: there is one number, and moving it moves both instruments.
//!
//! # The pointer, and why it is a *curve* and not a hit test (S2)
//!
//! Four things arrive together because they are one gesture. The rail colours as
//! a whole (`hot` is a rail-level class, mock 1371 — not something a tick wears);
//! the tick nearest the pointer's *ordinal* becomes the crest and its neighbours
//! grow along a cosine skirt (mock 8452-8457); a bucket the pointer lands on
//! unfolds into its members with the neighbours compressing to keep the block's
//! extent (mock 8438-8451); and a card appears beside the crest saying what the
//! command was (`#cmd-peek`, mock 8465-8477). None of them is separable: the card
//! names the tick the crest picked, the crest is picked in the geometry the
//! fisheye produced, and all three are only ever true while the rail is hot.
//!
//! **Selection first, curve second** is the ruling that shapes the code (five
//! rounds of errata, 2026-07-18): the bell is a function of the selected
//! *ordinal* and never of pointer pixels, so at every instant there is exactly
//! one peak and a pointer resting on the exact midpoint between two ticks lights
//! one of them fully rather than both at half height.
//!
//! # What this slice does not do
//!
//! S3 owns the search capsule and S4 the takeover, where matches join this same
//! ordinal stack and the peek's noun changes from `commands` to `lines`. This
//! file draws the rail at every temperature, answers which mark a press or a
//! chord means, and says what the card beside the crest reads.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use bt_render::{
    ChromePalette, OverlayQuad, TERMINAL_SCROLL_LANE_LOGICAL_PX, rounded_overlay_fill,
};
use bt_term::{CommandMark, CommandMarkId};

use crate::marks::OverlayLayer;
use crate::tooltip::NAME_PLACE_SEPARATOR;
use crate::{Motion, RevealTween};

/// `.cmdtick { width: 9px }` — a tick's resting **length**.
///
/// Length and not height: the bar lies sideways, and the terminology was untangled
/// on 2026-07-18 after a round of errata in which the user's "height" turned out
/// to mean the long axis. The stylesheet's own comment says which way round it
/// settled — *"width animates, thickness stays 2px"* — and this file uses the
/// mock-up's property names so the two can be read side by side.
pub const TICK_LENGTH_LOGICAL_PX: f32 = 9.0;
/// `.cmdtick { height: 2px }` — and it never changes. Every growth in this file
/// is horizontal.
pub const TICK_THICKNESS_LOGICAL_PX: f32 = 2.0;
/// `.cmdtick { border-radius: 2px }` — a two-pixel bar with a two-pixel radius is
/// a stadium, which is what the mock-up draws.
pub const TICK_RADIUS_LOGICAL_PX: f32 = 2.0;
/// The crest's length: `9 · (1 + 2·cos²(0))` = 27, the peak of the mock-up's own
/// magnification curve (8452-8457) evaluated at the tick the pointer selected.
///
/// The rest of that curve — the ±1 and ±2 neighbours at 22.5 and 13.5 — comes out
/// of [`curve_length`], which is this same expression at a non-zero distance.
pub const TICK_CREST_LENGTH_LOGICAL_PX: f32 = 27.0;
/// How many ordinals the bell reaches before it is back at the resting length —
/// the `3` of `u = min(|i − nearest| / 3, 1)` (mock 8453).
///
/// Five ticks take part in the magnification and no more: the crest, and two
/// neighbours on each side. The sixth is already at `u = 1`, where `cos(π/2)` is
/// zero and the curve has returned to nine pixels.
pub const CREST_CURVE_REACH: f32 = 3.0;
/// `.cmdtick.sub { margin-right: 4px }` — how far an unfolded member steps *out*
/// of the right-aligned column.
///
/// The stylesheet's comment is a bug report with the fix attached: *"the expanded
/// group STEPS OUT of the right-aligned rail — visibly 'opened' even when a
/// bucket only holds two members (user report 2026-07-18: the expansion was
/// imperceptible; and never shorter than the crest — **one width base for
/// everyone**)"*. The second half is the part that is easy to get wrong: an
/// unfolded member is **not** drawn thinner or shorter than a folded tick. It is
/// the same nine pixels, moved four to the left, and the group reads as opened
/// because its whole column is offset rather than because its ticks are smaller.
/// `docs/DESIGN.md` §7.1.5c said "one step thinner, 7px" for a while; the
/// stylesheet is the executable artefact and it says otherwise.
pub const TICK_SUB_OFFSET_LOGICAL_PX: f32 = 4.0;
/// The gap between the reserved scroll lane and the rail's own box — the other
/// half of the mock-up's `right: 11px`.
pub const RAIL_LANE_GAP_LOGICAL_PX: f32 = 3.0;
/// `.cmdrail { padding: 8px 3px }`, horizontal half.
pub const RAIL_PADDING_X_LOGICAL_PX: f32 = 3.0;
/// `.cmdrail { padding: 8px 3px }`, vertical half.
pub const RAIL_PADDING_Y_LOGICAL_PX: f32 = 8.0;
/// `avail = max(60, paneH * .8)` (mock 4642) — the block may take four fifths of
/// the pane and no more, so a rail never runs edge to edge and never reads as a
/// scroll bar.
pub const RAIL_BLOCK_FRACTION: f32 = 0.8;
/// The other half of the same expression: below sixty pixels the fraction stops
/// shrinking, because a pane can be short enough that four fifths of it cannot
/// hold four ticks.
pub const RAIL_BLOCK_MIN_LOGICAL_PX: f32 = 60.0;
/// `pitch = max(4, min(9, avail / N))` (mock 4643) — the density floor.
pub const TICK_PITCH_MIN_LOGICAL_PX: f32 = 4.0;
/// The same expression's ceiling: ticks never spread further apart than nine
/// pixels however few of them there are, so two commands do not produce two marks
/// at opposite ends of the pane.
pub const TICK_PITCH_MAX_LOGICAL_PX: f32 = 9.0;
/// `gap = max(2, pitch - 2)` (mock 4644) — pitch less the tick's own thickness,
/// floored so that ticks never touch.
pub const TICK_GAP_MIN_LOGICAL_PX: f32 = 2.0;
/// `.cmdtick { opacity: .45 }` — at rest the ticks are grey, because *"queries
/// stay quiet"*.
pub const TICK_REST_OPACITY: f32 = 0.45;
/// `.cmdtick.fail { opacity: .6 }` — *"signals earn permanent colour"*. A failed
/// command is red without being pointed at, and this is the one thing on the rail
/// that is.
pub const TICK_FAIL_REST_OPACITY: f32 = 0.6;
/// `.cmdrail.hot .cmdtick { opacity: .55 }` — the whole rail lifts when the
/// pointer is on it, which is the mock-up's *"hovering the rail colours them"*.
pub const TICK_HOT_OPACITY: f32 = 0.55;
/// `.cmdrail.hot .cmdtick.fail { opacity: .65 }` — a failure lifts too, in its
/// own hue and by its own five hundredths.
pub const TICK_FAIL_HOT_OPACITY: f32 = 0.65;
/// `.cmdrail.hot .cmdtick.crest { opacity: 1 }` — *"the SELECTED tick:
/// unmistakable"*.
pub const TICK_CREST_OPACITY: f32 = 1.0;
/// `transition: width .1s ease` (mock 1369) — the crest travelling from one
/// ordinal to the next.
///
/// It is what makes the mock-up's own sentence true: *"指针跨中点时峰整刻度切换,
/// 宽度过渡使切换读作运动"* — the peak jumps a whole ordinal at the midpoint, and
/// the tenth of a second is what turns that jump into a movement rather than a
/// flicker.
pub const TICK_WIDTH_TRANSITION: Duration = Duration::from_millis(100);
/// `transition: background .14s ease` (mock 1369) — grey to accent as the rail
/// warms, and accent to the deepened crest under the pointer.
pub const TICK_BACKGROUND_TRANSITION: Duration = Duration::from_millis(140);
/// `transition: opacity .12s ease` (mock 1369) — `.45 → .55 → 1`.
///
/// **A separate span from the background's**, and deliberately not rounded to
/// match it: the stylesheet declares three durations on one line and they differ
/// by a fortieth of a second each. Collapsing them would be an aesthetic decision
/// taken by an implementer, and the sum of the three is what the rail's warming
/// actually looks like.
pub const TICK_OPACITY_TRANSITION: Duration = Duration::from_millis(120);
/// `left = r.left − peek.offsetWidth − 12` (mock 8476) — the card stands this far
/// to the **left** of the tick it names.
///
/// Left and not below, because the rail is already on the pane's right edge: a
/// card below would have nowhere to go, and a card to the right would be off the
/// window. The whole of `#cmd-peek`'s placement is this and [`PEEK_RISE_LOGICAL_PX`].
pub const PEEK_GAP_LOGICAL_PX: f32 = 12.0;
/// `top = r.top − 8` (mock 8475) — the card's top edge sits eight pixels *above*
/// the two-pixel bar it names, so a 24-pixel card is roughly centred on it
/// without the arithmetic pretending to be a centring rule it is not.
pub const PEEK_RISE_LOGICAL_PX: f32 = 8.0;
/// What the card says about a mark whose text the ledger never got
/// (`command_marks.rs` leaves it empty when `C` and the output that scrolled the
/// prompt away arrived in one PTY read).
///
/// Drawn in the muted ink, because the word is a *category* and not a quotation:
/// the card's whole contract is "this is the command", and a card that said
/// nothing at all would read as a bug in the card rather than as an honest gap in
/// the ledger.
pub const PEEK_EMPTY_TEXT: &str = "command";
/// How many times [`resolve`] may lay the rail out again before it answers.
///
/// The mock-up's own `depth < 2` (8443, 8449), kept as a number because the
/// ruling names it — but this file does not need it the way the mock-up does. See
/// [`resolve`] for why: its rule has no feedback loop to bound, and two is simply
/// the count it reaches by construction.
pub const FISHEYE_RELAYOUT_CAP: usize = 2;
/// `.term > div.cmd-jump { animation: cmdflash .95s ease }` — how long the row a
/// jump landed on says so.
pub const JUMP_FLASH: Duration = Duration::from_millis(950);
/// `@keyframes cmdflash { 0% { background: color-mix(in srgb, var(--accent) 22%,
/// transparent) } }` — the flash's opening alpha, easing to nothing.
pub const JUMP_FLASH_OPACITY: f32 = 0.22;
/// `.term > div.cmd-jump { border-radius: 4px }`.
pub const JUMP_FLASH_RADIUS_LOGICAL_PX: f32 = 4.0;
/// `term.scrollTo({ top: max(0, line.offsetTop - 8) })` (mock 4686) — the jumped-to
/// row lands eight pixels below the top of the viewport rather than flush with it,
/// so it reads as a line *in* a page instead of a page that begins there.
pub const JUMP_TOP_INSET_LOGICAL_PX: f32 = 8.0;

/// One drawn tick, and the command a press on it means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// Which command this tick jumps to.
    ///
    /// For a bucket of several it is the **newest** member, which is the mock-up's
    /// own `data-line="${members[members.length - 1]}"` (4666). A collapsed bucket
    /// is a range of history and the newest end of it is the one a reader is
    /// looking for; S2's fisheye is what makes the older members reachable exactly.
    pub mark: CommandMarkId,
    /// How many commands this tick stands for. One at every density the pane can
    /// actually hold; more only once the four-pixel floor has been passed.
    pub members: usize,
    /// Whether any member of this tick failed — **maximum wins**, the same rule
    /// the tab dot has followed since 2026-07-18. A bucket carries its strongest
    /// member's signal, and a failure is the strongest signal a command has.
    pub failed: bool,
    /// Which **bucket** this tick belongs to — the mock-up's `data-slot`.
    ///
    /// It is the identity the fisheye is keyed on, and the reason the members of
    /// an unfolded bucket all carry the slot of the bucket they came out of
    /// rather than a number of their own: *"Members keep the bucket's slot id, so
    /// hovering inside the expansion never collapses it"* (mock 8440-8442). One
    /// tick per command means slot and index agree; past the density floor they
    /// part company, and every question about *staying open* is asked of the slot.
    pub slot: usize,
    /// `.cmdtick.sub` — one member of an unfolded bucket, drawn
    /// [`TICK_SUB_OFFSET_LOGICAL_PX`] further left than the column it stepped out
    /// of.
    pub sub: bool,
    /// The resting rectangle, in physical pixels.
    ///
    /// **Resting**, so its right edge is the one thing about it that never moves:
    /// the crest and the whole cosine skirt grow leftward out of `rect[2]`, and
    /// [`paint`] reads exactly that edge. Vertical layout is a function of this
    /// rectangle alone, which is what *"width only grows leftward, so vertical
    /// layout never shifts and there is no jitter"* buys.
    pub rect: [f32; 4],
}

/// What is drawn on one pane's right edge, and the answer to "which tick is the
/// pointer on".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rail {
    /// The rail's pointer surface, in physical pixels — empty (all zeros) when
    /// there is nothing to draw.
    ///
    /// **The rail is the surface, not the tick** (mock 8404-8409): a bar two
    /// pixels tall is not something a hand can be asked to land on, so the band
    /// takes the press and the nearest tick answers it. That is the mock-up's own
    /// mechanism, minus its bell curve.
    ///
    /// It is the width of the mock-up's *resting* box — a tick and its two
    /// three-pixel paddings — and [`Self::hot_bounds`] is the same band grown to
    /// the crest's own width. Which of the two answers depends on whether the
    /// rail is already hot, and that asymmetry is the whole ruling: see
    /// [`band`].
    pub bounds: [f32; 4],
    /// The band **while the rail is hot** — [`Self::bounds`] grown leftward to
    /// hold a crest, and an unfolded member's four-pixel step besides.
    ///
    /// **D-17, the middle path (ruled 2026-08-16).** A CSS flex box is sized by
    /// its widest child, so the browser's rail grows the instant a tick expands;
    /// S1 refused to reproduce that, because thirty-three logical pixels down
    /// every pane's right edge in which a hyperlink stops underlining and a
    /// Ctrl+click stops opening is a real cost, and the mock-up only gets away
    /// with it because its `.term` carries an 18px right padding no native grid
    /// has. But a fixed band has its own cost, and S1 wrote it down: a crest is
    /// drawn twelve pixels left of where the band reaches, so a hand walking
    /// *onto* the crest leaves the rail and the crest relaxes under it.
    ///
    /// Both costs are avoidable because they are not costs of the same state. At
    /// **rest** the band is the small one, so nothing about the pane's right edge
    /// changes for a reader who is not pointing at the rail — that is the
    /// hyperlink's case, and it is the case that is true almost all the time.
    /// While **hot** the band is the crest's, so the crest is a thing a hand can
    /// be on. The growth cannot flap: it happens only on the way in, and the
    /// shrink happens only once the pointer has left the *larger* box, so the two
    /// thresholds are never the same pixel.
    pub hot_bounds: [f32; 4],
    /// Oldest at the top — *"position carries order, not scroll geometry"*.
    pub ticks: Vec<Tick>,
    /// `k` — how many commands each tick stands for (mock 4646). One until the
    /// density floor is passed.
    pub bucket_size: usize,
    /// The vertical space between one tick and the next, in physical pixels —
    /// `max(2, pitch − 2)` (mock 4644), or the compressed gap an unfolded bucket
    /// leaves its neighbours.
    ///
    /// Kept on the rail rather than recomputed because two readers need it and
    /// they must agree: the layout places the ticks with it, and [`resolve`]
    /// measures an open group's *dominion* — the zone that keeps it open — half a
    /// gap beyond its outermost members.
    pub gap: f32,
    /// The scale the geometry above was laid out at, kept so the paint can round
    /// its radii the same way the layout rounded its boxes.
    pub scale: f32,
}

/// What a laid-out rail is a function of, and therefore what may invalidate one.
///
/// Four things and no fifth. **Not the scroll offset** — an ordinal stack does
/// not move when the page does — and **not the crest**, whose whole travel is a
/// set of widths grown leftward out of rectangles this key already fixed. What
/// *is* here beside the ledger and the pane is the unfolded bucket, because a
/// fisheye genuinely moves ticks: it is a different arrangement of the same
/// marks and not a different colour on the same arrangement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailKey {
    /// [`bt_term::DualPlaneSession::command_marks_revision`] — bumped by ledger
    /// changes and by nothing else, which is the only reason it exists.
    pub revision: u64,
    /// The pane's body, in physical pixels.
    pub body: [f32; 4],
    pub scale: f32,
    /// Which bucket, if any, is open under the pointer.
    pub expanded: Option<usize>,
}

/// A vector of numbers easing to a vector of numbers, on one clock.
///
/// [`RevealTween`] is the scalar of this and stays the type for everything with
/// one value to move. A rail has one value *per tick* — every tick's length
/// changes when the crest travels, and every tick's crest-ness changes with it —
/// and the three ways to express that are all worse than this one: a tween per
/// tick multiplies the clock by the tick count and lets two of them disagree
/// about `now`; a single scalar plus a formula cannot ease *from* the widths a
/// half-finished travel was actually showing; and easing the pointer's position
/// instead of the widths would put the curve's peak between two ordinals, which
/// is the ambiguous state the whole selection-first ruling exists to forbid.
///
/// A length change means the drawn ticks are not the ticks that were drawn — a
/// bucket opened, the ledger grew, the pane was resized — so the travel is
/// **reseeded from [`Self::rest`]** rather than continued between two sets that do
/// not correspond. Continuing would be easing tick 4's width toward what is now
/// tick 7's; and rest is the right origin rather than the target because rest is
/// what [`build`] actually draws a freshly laid-out rail at, which makes the
/// seeding a statement about the picture on the glass instead of a convention.
#[derive(Clone, Debug)]
struct VecTween {
    from: Vec<f32>,
    to: Vec<f32>,
    started: Option<Instant>,
    span: Duration,
    /// The value a tick has when the rail is cold — nine pixels of length, no
    /// crest at all.
    rest: f32,
}

impl VecTween {
    fn over(span: Duration, rest: f32) -> Self {
        Self {
            from: Vec::new(),
            to: Vec::new(),
            started: None,
            span,
            rest,
        }
    }

    fn retarget(&mut self, target: Vec<f32>, now: Instant, motion: Motion) {
        if self.to == target {
            return;
        }
        let from = if self.to.len() == target.len() {
            self.sample(now, motion)
        } else {
            vec![self.rest; target.len()]
        };
        // Nothing to travel — a rail that gained a tick while nobody was pointing
        // at it is already showing the answer, and a clock started here would ask
        // for a hundred milliseconds of frames that all draw the same picture.
        self.started = (motion == Motion::Full && from != target).then_some(now);
        self.from = from;
        self.to = target;
    }

    fn sample(&self, now: Instant, motion: Motion) -> Vec<f32> {
        let Some(eased) = self.progress(now, motion) else {
            return self.to.clone();
        };
        self.from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| from + (to - from) * eased)
            .collect()
    }

    fn is_running(&self, now: Instant, motion: Motion) -> bool {
        self.progress(now, motion).is_some()
    }

    /// The eased fraction, or `None` when this tween is standing still.
    fn progress(&self, now: Instant, motion: Motion) -> Option<f32> {
        let started = self.started.filter(|_| motion == Motion::Full)?;
        if self.from.len() != self.to.len() {
            return None;
        }
        let elapsed = now.saturating_duration_since(started);
        (elapsed < self.span).then(|| {
            crate::cubic_bezier(elapsed.as_secs_f32() / self.span.as_secs_f32(), crate::EASE)
        })
    }
}

/// Everything about one rail that the *pointer* decides, and the four clocks it
/// decides them on.
///
/// Kept beside the geometry rather than in it because the two invalidate on
/// different events: a rail is rebuilt when the ledger or the pane moves, and
/// this changes when a hand does. The one place they meet is the unfolded bucket,
/// which is why [`RailKey`] carries [`Self::expanded`] and nothing else from here.
#[derive(Clone, Debug)]
pub struct RailPointer {
    expanded: Option<usize>,
    /// `background .14s` at the rail level — `0` grey, `1` the hot ink.
    hot_ink: RevealTween,
    /// `opacity .12s` at the rail level — `.45 → .55`, or `.6 → .65` for a
    /// failure.
    hot_alpha: RevealTween,
    /// `background .14s` per tick — `0` the hot ink, `1` the deepened crest.
    crest_ink: VecTween,
    /// `opacity .12s` per tick — `0` the hot alpha, `1` fully opaque.
    crest_alpha: VecTween,
    /// `width .1s` per tick, in **logical** pixels: the cosine skirt.
    width: VecTween,
}

impl Default for RailPointer {
    /// A cold rail with nothing in flight. Written out rather than derived
    /// because none of the four clocks has a meaningful zero: a tween with no
    /// span is either an instant nobody asked for or somebody else's duration
    /// borrowed by accident, which is the same reason [`RevealTween`] has no
    /// `Default` either.
    fn default() -> Self {
        Self {
            expanded: None,
            hot_ink: RevealTween::over(TICK_BACKGROUND_TRANSITION),
            hot_alpha: RevealTween::over(TICK_OPACITY_TRANSITION),
            crest_ink: VecTween::over(TICK_BACKGROUND_TRANSITION, 0.0),
            crest_alpha: VecTween::over(TICK_OPACITY_TRANSITION, 0.0),
            width: VecTween::over(TICK_WIDTH_TRANSITION, TICK_LENGTH_LOGICAL_PX),
        }
    }
}

impl RailPointer {
    /// Which bucket is open under the pointer.
    #[must_use]
    pub fn expanded(&self) -> Option<usize> {
        self.expanded
    }

    /// Aim every clock at the state `hot` and `nearest` describe.
    ///
    /// One call for all four, because they are one gesture: a rail that warmed
    /// its colour on one frame and moved its crest on another would read as two
    /// things happening to it.
    pub fn aim(
        &mut self,
        ticks: usize,
        nearest: Option<usize>,
        hot: bool,
        now: Instant,
        motion: Motion,
    ) {
        let crest = nearest.filter(|_| hot);
        let widths = (0..ticks)
            .map(|index| match crest {
                Some(peak) => curve_length(index as i64 - peak as i64),
                None => TICK_LENGTH_LOGICAL_PX,
            })
            .collect();
        let peaks: Vec<f32> = (0..ticks)
            .map(|index| f32::from(u8::from(crest == Some(index))))
            .collect();
        self.width.retarget(widths, now, motion);
        self.crest_ink.retarget(peaks.clone(), now, motion);
        self.crest_alpha.retarget(peaks, now, motion);
        let warmth = f32::from(u8::from(hot));
        self.hot_ink.retarget(warmth, now, motion);
        self.hot_alpha.retarget(warmth, now, motion);
    }

    /// Remember which bucket is open. Returns whether the answer moved, because
    /// the rail's geometry has to be laid out again when it did.
    pub fn expand(&mut self, expanded: Option<usize>) -> bool {
        let moved = self.expanded != expanded;
        self.expanded = expanded;
        moved
    }

    /// Whether any of the four clocks still owes a frame.
    #[must_use]
    pub fn is_animating(&self, now: Instant, motion: Motion) -> bool {
        self.hot_ink.sample(now, motion).1
            || self.hot_alpha.sample(now, motion).1
            || self.width.is_running(now, motion)
            || self.crest_ink.is_running(now, motion)
            || self.crest_alpha.is_running(now, motion)
    }

    /// Whether this rail is drawn exactly as [`build`] would draw it — cold, and
    /// with nothing left to ease. The one condition under which a cached picture
    /// is still the true one.
    #[must_use]
    pub fn is_resting(&self, now: Instant, motion: Motion) -> bool {
        !self.is_animating(now, motion)
            && self.hot_ink.sample(now, motion).0 == 0.0
            && self
                .crest_ink
                .sample(now, motion)
                .iter()
                .all(|mix| *mix == 0.0)
    }
}

/// One pane's rail, the key it was built under, and what the pointer is doing to
/// it.
#[derive(Clone, Debug, Default)]
pub struct RailCache {
    key: Option<RailKey>,
    rail: Rail,
    layer: OverlayLayer,
    pointer: RailPointer,
}

impl RailCache {
    /// Whether the rail this cache is holding was built for something else.
    ///
    /// **Asked separately from [`Self::install`] on purpose**, and the reason is a
    /// borrow rather than a taste: the ledger a rail is laid out from and the
    /// cache it is stored in hang off the same window, so a `&mut` cache cannot be
    /// handed a closure that reads the `&` session beside it. Splitting the
    /// question from the answer lets the caller lay out under a shared borrow and
    /// store under an exclusive one, which is also the honest shape — a cache
    /// decides *whether*, and a painter decides *what*.
    #[must_use]
    pub fn needs_rebuild(&self, key: RailKey) -> bool {
        self.key != Some(key)
    }

    /// Take a freshly laid-out rail and paint its resting picture.
    ///
    /// Nothing is done to the pointer here, deliberately. A rebuild that changed
    /// the tick count reseeds the per-tick travels the next time they are aimed —
    /// see [`VecTween`], whose seed is the resting length this very call has just
    /// drawn the new rail at — and the rail-level warmth is not a fact about any
    /// tick, so a bucket opening under a hand must not make the whole column blink
    /// back to grey on its way open.
    pub fn install(&mut self, key: RailKey, rail: Rail, palette: &ChromePalette) {
        self.layer = build(&rail, palette);
        self.rail = rail;
        self.key = Some(key);
    }

    pub fn rail(&self) -> &Rail {
        &self.rail
    }

    pub fn layer(&self) -> &OverlayLayer {
        &self.layer
    }

    pub fn pointer(&self) -> &RailPointer {
        &self.pointer
    }

    pub fn pointer_mut(&mut self) -> &mut RailPointer {
        &mut self.pointer
    }

    /// Take the rail's own picture for this frame: the cached resting layer while
    /// nothing is happening to it, a fresh one while something is.
    #[must_use]
    pub fn picture(&self, palette: &ChromePalette, now: Instant, motion: Motion) -> OverlayLayer {
        if self.pointer.is_resting(now, motion) {
            return self.layer().clone();
        }
        paint(&self.rail, &self.pointer, palette, now, motion)
    }

    /// Forget everything. The palette is not part of [`RailKey`] — a theme change
    /// is not a fact about a pane — so the one thing that can silently outlive a
    /// rebuild is the ink, and this is how the theme switch says so.
    pub fn clear(&mut self) {
        self.key = None;
        self.rail = Rail::default();
        self.layer = OverlayLayer::default();
        self.pointer = RailPointer::default();
    }
}

/// The pane rectangle a rail hangs off, or `None` when this pane is not showing
/// one.
///
/// **The alternate screen suspends the picture and not the data**, and the two are
/// worth keeping apart. The ledger already refuses to record an alternate-screen
/// marker — §3.2 puts that screen in an isolated namespace, and a full-screen TUI
/// emitting `A/B/C/D` for its own redraws is describing its own canvas rather than
/// this session's history — so a pane running `vim` still holds a ledger full of
/// perfectly true marks: the commands that ran *before* `vim` started. What must
/// not happen is those being drawn over somebody else's canvas, over rows they say
/// nothing about, beside a scrollback that is not there. Switch back and every tick
/// is where it was.
///
/// It is a function rather than an `if` at the one call site so the ruling has a
/// name and a test, and so that the next surface to ask the same question — the
/// search capsule (S3), whose own ruling is the same "not on the alternate screen"
/// — asks it here instead of restating it.
#[must_use]
pub fn host_rect(body: [f32; 4], alternate_screen: bool) -> Option<[f32; 4]> {
    (!alternate_screen).then_some(body)
}

/// The mock-up's magnification curve: how long the tick `delta` ordinals from the
/// crest is drawn, in logical pixels.
///
/// `u = min(|Δi| / 3, 1)`, `w = 9 · (1 + 2·cos²(u·π/2))` — copied out of mock
/// 8453-8455 rather than re-derived, because the shape it makes is a ruling and
/// not an implementation detail: 27 at the crest, 22.5 and 13.5 for the first two
/// neighbours, and back to the resting 9 from the third outward. *"ONE base: the
/// crest is always the longest."*
///
/// **A function of the ordinal distance and never of the pointer's pixels.** That
/// is the five-rounds-of-errata ruling in one signature: the argument is an
/// integer, so between two ticks the answer does not slide — it belongs wholly to
/// one of them, and the transition on the width is what makes the handover read
/// as movement rather than as a flicker.
#[must_use]
pub fn curve_length(delta: i64) -> f32 {
    let u = (delta.unsigned_abs() as f32 / CREST_CURVE_REACH).min(1.0);
    TICK_LENGTH_LOGICAL_PX * (1.0 + 2.0 * (u * PI / 2.0).cos().powi(2))
}

/// Place the ticks for `marks` inside a pane body, in physical pixels, with
/// bucket `expanded` — if there is one, and if it holds more than one command —
/// unfolded into its members.
///
/// # The arithmetic, and the one place it knowingly leaves the mock-up standing
///
/// Everything here is `renderRailTicks` (mock 4640-4670) with the search branch
/// removed: `avail` from the pane, `capacity` from the four-pixel density floor,
/// `k` from `capacity`, `pitch` from `avail`, and `gap` from `pitch`.
///
/// **The mock-up's own arithmetic is not self-consistent, and S2 is where it gets
/// corrected (inventory B15).** `pitch` is computed there from the *unaggregated*
/// count while `ceil(N/k)` ticks are actually drawn, so past the density floor the
/// block comes out far shorter than the space it was given: two hundred commands
/// in an eight-hundred-pixel pane draw a hundred ticks at the four-pixel floor and
/// fill 398 of their 640 pixels — sixty-two per cent of a rail, centred, reading
/// as a pane whose history stopped somewhere in the middle. S1 reproduced it
/// deliberately, because changing it changes the *density* the rail reads at and
/// density was tier ②'s question — which is this slice. Here it is answered:
/// **pitch comes from the number of ticks actually drawn**, so a dense rail fills
/// `avail` and the aggregation is legible as ticks standing closer together
/// rather than as a rail that shrank.
///
/// # What an unfolded bucket does to the block
///
/// It does not lengthen it. `gap` is recomputed for the drawn count against the
/// *folded* block's extent, so the members open into room the neighbours give up —
/// the fisheye's own bargain, and the reason the rail's outer edges do not move
/// when a bucket opens under the pointer. Only once the compression reaches the
/// two-pixel floor does the block grow, and then it grows symmetrically about the
/// pane's middle, which is where it was centred to begin with.
#[must_use]
pub fn lay_out(body: [f32; 4], marks: &[CommandMark], scale: f32, expanded: Option<usize>) -> Rail {
    // An empty ledger draws nothing at all — no box, no band, no error (inventory
    // C13). `cmd.exe` never sends an OSC 133 and a PowerShell without the
    // integration script never sends one either, so this is the *ordinary* state
    // of a large fraction of panes and not an edge case. The mock-up emits the
    // rail element regardless because S4's search results need a surface to hang
    // off; there is no element here to emit, so the empty rail is simply empty.
    if marks.is_empty() || body[2] <= body[0] || body[3] <= body[1] {
        return Rail {
            scale,
            ..Rail::default()
        };
    }
    let px = |logical: f32| logical * scale;
    let height = body[3] - body[1];
    let avail = px(RAIL_BLOCK_MIN_LOGICAL_PX).max(height * RAIL_BLOCK_FRACTION);
    let thickness = px(TICK_THICKNESS_LOGICAL_PX).round().max(1.0);
    // `capacity = max(4, floor(avail / 4))` — how many ticks fit at the density
    // floor; `k = ceil(N / capacity)` — how many commands each one has to stand
    // for. Four is a floor on the floor: a pane too short to hold four ticks still
    // gets four rather than one, because a rail of one tick says nothing at all.
    let capacity = (avail / px(TICK_PITCH_MIN_LOGICAL_PX)).floor().max(4.0) as usize;
    let bucket_size = marks.len().div_ceil(capacity).max(1);
    // B15's correction: the *drawn* count, not `marks.len()`.
    let folded = marks.len().div_ceil(bucket_size);
    let pitch =
        (avail / folded as f32).clamp(px(TICK_PITCH_MIN_LOGICAL_PX), px(TICK_PITCH_MAX_LOGICAL_PX));
    let folded_gap = px(TICK_GAP_MIN_LOGICAL_PX).max(pitch - px(TICK_THICKNESS_LOGICAL_PX));
    // The extent the folded rail would occupy, and the extent an unfolded one is
    // held to: the members open into room their neighbours give up.
    let block = folded as f32 * thickness + (folded - 1) as f32 * folded_gap;
    let open = expanded.filter(|slot| {
        marks
            .chunks(bucket_size)
            .nth(*slot)
            .is_some_and(|chunk| chunk.len() > 1)
    });
    let mut ticks: Vec<Tick> = Vec::with_capacity(folded + bucket_size);
    for (slot, members) in marks.chunks(bucket_size).enumerate() {
        if open == Some(slot) {
            ticks.extend(members.iter().map(|mark| Tick {
                mark: mark.id,
                members: 1,
                failed: mark.failed(),
                slot,
                sub: true,
                rect: [0.0; 4],
            }));
        } else {
            ticks.push(Tick {
                // The newest member (mock 4666), which for a chunk is its last.
                mark: members[members.len() - 1].id,
                members: members.len(),
                failed: members.iter().any(CommandMark::failed),
                slot,
                sub: false,
                rect: [0.0; 4],
            });
        }
    }
    let drawn = ticks.len();
    let gap = if drawn > 1 {
        ((block - drawn as f32 * thickness) / (drawn - 1) as f32).max(px(TICK_GAP_MIN_LOGICAL_PX))
    } else {
        folded_gap
    };
    let extent = drawn as f32 * thickness + (drawn - 1) as f32 * gap;
    // `top: 50%; transform: translateY(-50%)` — the *block* is centred, not each
    // tick, which is what makes a rail of three ticks and a rail of thirty read as
    // the same instrument at different densities.
    let top = ((body[1] + body[3]) / 2.0 - extent / 2.0).round();
    let right = (body[2]
        - px(TERMINAL_SCROLL_LANE_LOGICAL_PX + RAIL_LANE_GAP_LOGICAL_PX)
        - px(RAIL_PADDING_X_LOGICAL_PX))
    .round();
    for (index, tick) in ticks.iter_mut().enumerate() {
        let tick_top = (top + index as f32 * (thickness + gap)).round();
        // `.cmdtick.sub { margin-right: 4px }` — the opened group's own column,
        // one step out of the right-aligned one it came from.
        let tick_right = right
            - if tick.sub {
                px(TICK_SUB_OFFSET_LOGICAL_PX)
            } else {
                0.0
            };
        tick.rect = [
            tick_right - px(TICK_LENGTH_LOGICAL_PX),
            tick_top,
            tick_right,
            tick_top + thickness,
        ];
    }
    let last = ticks[ticks.len() - 1].rect;
    // The band, padded on all four sides the way `.cmdrail { padding: 8px 3px }`
    // pads the flex box, and at the resting width its ticks are — see
    // [`Rail::bounds`] and [`Rail::hot_bounds`] for what those two widths are a
    // decision about.
    let band_right = right + px(RAIL_PADDING_X_LOGICAL_PX);
    let band_top = ticks[0].rect[1] - px(RAIL_PADDING_Y_LOGICAL_PX);
    let band_bottom = last[3] + px(RAIL_PADDING_Y_LOGICAL_PX);
    // The four pixels an unfolded member steps out by are held open at every
    // temperature the rail can be bucketed at, so that the hot band's own edge
    // does not move when a bucket opens inside it — a target that resizes because
    // it was pointed at is a target that can shake off the hover that reached it.
    let step_out = if bucket_size > 1 {
        TICK_SUB_OFFSET_LOGICAL_PX
    } else {
        0.0
    };
    Rail {
        bounds: [
            band_right - px(TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0),
            band_top,
            band_right,
            band_bottom,
        ],
        hot_bounds: [
            band_right
                - px(TICK_CREST_LENGTH_LOGICAL_PX + step_out + RAIL_PADDING_X_LOGICAL_PX * 2.0),
            band_top,
            band_right,
            band_bottom,
        ],
        ticks,
        bucket_size,
        gap,
        scale,
    }
}

/// The band that answers the pointer at this temperature — see
/// [`Rail::hot_bounds`] for why there are two of them and why the asymmetry
/// cannot flap.
#[must_use]
pub fn band(rail: &Rail, hot: bool) -> [f32; 4] {
    if hot { rail.hot_bounds } else { rail.bounds }
}

/// Which tick a pointer at `(x, y)` means, or `None` when the pointer is not on
/// the rail at all.
///
/// **Selection first, curve second** (user ruling 2026-07-18, after five rounds of
/// errata): the answer is a function of the *ordinal* the pointer is nearest to,
/// never of the pixels themselves. There is exactly one peak at any moment and no
/// ambiguous state where a pointer resting between two ticks lights both at half
/// strength. [`curve_length`] is applied on top of this answer; it does not
/// replace it.
#[must_use]
pub fn nearest(rail: &Rail, hot: bool, x: f32, y: f32) -> Option<usize> {
    let bounds = band(rail, hot);
    if rail.ticks.is_empty() || x < bounds[0] || x > bounds[2] || y < bounds[1] || y > bounds[3] {
        return None;
    }
    nearest_ordinal(rail, y)
}

/// The same answer with no band test at all — the tick a given height belongs to.
///
/// Split out because [`resolve`] asks it of geometries the pointer has not been
/// admitted to yet: deciding whether a bucket should open means asking the
/// *folded* rail which tick the hand is over, and that rail is a hypothesis rather
/// than the surface under the pointer.
#[must_use]
pub fn nearest_ordinal(rail: &Rail, y: f32) -> Option<usize> {
    let centre = |tick: &Tick| (tick.rect[1] + tick.rect[3]) / 2.0;
    // Ties go to the older tick — the earlier index — so that a pointer exactly on
    // a midpoint always answers the same way rather than depending on iteration.
    rail.ticks
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (centre(a) - y).abs().total_cmp(&(centre(b) - y).abs()))
        .map(|(index, _)| index)
}

/// The rail, the bucket left open, and the crest — all three settled together.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub rail: Rail,
    pub expanded: Option<usize>,
    pub nearest: Option<usize>,
}

/// The vertical zone that keeps bucket `slot` open, or `None` when it is not open
/// in this geometry.
///
/// Half a gap past the outermost member on each side, which is the same boundary
/// [`nearest_ordinal`] would draw between two ticks — so the zone is exactly the
/// union of the members' own dominions and never a hand-picked margin.
#[must_use]
fn group_span(rail: &Rail, slot: usize) -> Option<[f32; 2]> {
    let mut members = rail
        .ticks
        .iter()
        .filter(|tick| tick.sub && tick.slot == slot)
        .peekable();
    let first = members.peek().copied()?;
    let last = members.last()?;
    Some([
        first.rect[1] - rail.gap / 2.0,
        last.rect[3] + rail.gap / 2.0,
    ])
}

/// Settle the fisheye and the crest for a pointer at height `y`.
///
/// # The rule, and why it cannot flap
///
/// *"Members keep the bucket's slot id, so hovering inside the expansion never
/// collapses it (hysteresis); moving to another slot swaps the expansion"* (mock
/// 8440-8442). Two conditions, and they are deliberately asymmetric:
///
/// * **Staying open** is asked of the *open* geometry — the pointer is anywhere
///   inside the unfolded group's own zone, which is `k` ticks tall.
/// * **Opening** is asked of the *folded* geometry — the tick the pointer is
///   nearest to, when the rail is drawn with nothing unfolded, is a bucket.
///
/// The second half is a pure function of `y`: it consults a rail that does not
/// depend on what is currently open, so it cannot disagree with itself. That is
/// what makes the whole thing a fixed point after a single re-layout, and it is
/// where this parts company with the mock-up, whose rule feeds its own output back
/// in and needs `depth < 2` to stop the recursion. A guard on a runaway is not
/// hysteresis: it bounds the work per frame and leaves the *state* free to
/// alternate between frames, which is exactly the boundary flicker the ruling is
/// about. [`FISHEYE_RELAYOUT_CAP`] is kept as the count this reaches, and a test
/// stands a pointer on a group's edge and asserts two consecutive frames agree.
#[must_use]
pub fn resolve(
    body: [f32; 4],
    marks: &[CommandMark],
    scale: f32,
    expanded: Option<usize>,
    y: f32,
) -> Resolved {
    // The folded rail: the hypothesis every question about *opening* is asked of,
    // and the one geometry here that does not depend on what is currently open.
    // It is laid out once, before the loop, because it is the same rail on every
    // pass — which is the same sentence as "this terminates".
    let folded = lay_out(body, marks, scale, None);
    let opens = nearest_ordinal(&folded, y)
        .and_then(|index| (folded.ticks[index].members > 1).then_some(folded.ticks[index].slot));
    let mut open = expanded;
    let mut rail = match open {
        Some(slot) => lay_out(body, marks, scale, Some(slot)),
        None => folded.clone(),
    };
    for _ in 0..FISHEYE_RELAYOUT_CAP {
        let held = open.is_some_and(|slot| {
            group_span(&rail, slot).is_some_and(|span| y >= span[0] && y <= span[1])
        });
        if held || open == opens {
            break;
        }
        open = opens;
        rail = match open {
            Some(slot) => lay_out(body, marks, scale, Some(slot)),
            None => folded.clone(),
        };
    }
    let nearest = nearest_ordinal(&rail, y);
    Resolved {
        rail,
        expanded: open,
        nearest,
    }
}

/// The resting rail: every tick at its own ink, nothing lit.
///
/// [`paint`] with a pointer that has never been anywhere, and written as a
/// function of it rather than beside it so that the picture a cold rail is cached
/// as and the picture a cooling one arrives at are the same code — a resting state
/// that two functions each have their own opinion of is a state that goes
/// subtly wrong the day one of them is edited.
#[must_use]
pub fn build(rail: &Rail, palette: &ChromePalette) -> OverlayLayer {
    paint(
        rail,
        &RailPointer::default(),
        palette,
        Instant::now(),
        Motion::Reduced,
    )
}

/// The rail at whatever moment its four clocks have reached.
///
/// # The three transitions, and what each of them is a transition *of*
///
/// `transition: width .1s ease, background .14s ease, opacity .12s ease` (mock
/// 1369) is declared on `.cmdtick`, so all three belong to the ticks — the glance
/// card has no transition at all, it is `display: none` and then `display: block`.
/// They compose in the order the stylesheet's cascade does:
///
/// * the **rail** warms from `--ink3` to `--accent` and from `.45` to `.55` (a
///   failure from its red at `.6` to the same red at `.65` — *"signals earn
///   permanent colour"*, so it moves in opacity only);
/// * the **crest** then deepens out of whatever that left, to the 86%-black accent
///   mix at full opacity;
/// * every tick's **length** travels along [`curve_length`] toward its distance
///   from the crest.
///
/// *"Width only grows leftward, so vertical layout never shifts and there is no
/// jitter"* (mock 1367-1368): the right edge of every quad here is the right edge
/// of the resting tick, and all the growth opens into the pane.
#[must_use]
pub fn paint(
    rail: &Rail,
    pointer: &RailPointer,
    palette: &ChromePalette,
    now: Instant,
    motion: Motion,
) -> OverlayLayer {
    let radius = TICK_RADIUS_LOGICAL_PX * rail.scale;
    let widths = pointer.width.sample(now, motion);
    let crest_ink = pointer.crest_ink.sample(now, motion);
    let crest_alpha = pointer.crest_alpha.sample(now, motion);
    let (hot_ink, _) = pointer.hot_ink.sample(now, motion);
    let (hot_alpha, _) = pointer.hot_alpha.sample(now, motion);
    OverlayLayer {
        quads: rail
            .ticks
            .iter()
            .enumerate()
            .flat_map(|(index, tick)| {
                let (rest, rest_alpha) = if tick.failed {
                    (palette.status_err, TICK_FAIL_REST_OPACITY)
                } else {
                    (palette.command_tick, TICK_REST_OPACITY)
                };
                let (warm, warm_alpha) = if tick.failed {
                    (palette.status_err, TICK_FAIL_HOT_OPACITY)
                } else {
                    (palette.accent, TICK_HOT_OPACITY)
                };
                let peak = if tick.failed {
                    palette.command_tick_fail_crest
                } else {
                    palette.command_tick_crest
                };
                let deepen = crest_ink.get(index).copied().unwrap_or(0.0);
                let lift = crest_alpha.get(index).copied().unwrap_or(0.0);
                let ink = mix(mix(rest, warm, hot_ink), peak, deepen);
                let alpha = lerp(
                    lerp(rest_alpha, warm_alpha, hot_alpha),
                    TICK_CREST_OPACITY,
                    lift,
                );
                let length =
                    widths.get(index).copied().unwrap_or(TICK_LENGTH_LOGICAL_PX) * rail.scale;
                rounded_overlay_fill(
                    [
                        tick.rect[2] - length,
                        tick.rect[1],
                        tick.rect[2],
                        tick.rect[3],
                    ],
                    radius,
                    ink,
                    alpha,
                )
            })
            .collect(),
        ..OverlayLayer::default()
    }
}

/// The box the glance card is placed against: the crest, at its full length.
///
/// The *target* length rather than the tweened one, so that a card settling
/// beside a tick that is still growing does not slide the last twelve pixels with
/// it. The mock-up reads `nearest.getBoundingClientRect()` after the widths are
/// written and gets the same answer for the same reason.
#[must_use]
pub fn peek_host(rail: &Rail, index: usize) -> Option<[f32; 4]> {
    let tick = rail.ticks.get(index)?;
    Some([
        tick.rect[2] - TICK_CREST_LENGTH_LOGICAL_PX * rail.scale,
        tick.rect[1],
        tick.rect[2],
        tick.rect[3],
    ])
}

/// What the glance card says about one tick, and whether it says it in the muted
/// ink.
///
/// Four readings, and the mock-up only had two of them because the mock-up has no
/// exit codes and no ledger to be honestly empty:
///
/// * a **folded bucket** — `"{k} commands · latest: {text}"` (mock 8468). The noun
///   is a constant here and becomes a variable in S4, where the same stack also
///   carries search matches and the word is `lines`.
/// * a command **still running** — `D` never arrived, so there is no status to
///   report and the card says the one thing that is true of it.
/// * a mark whose **text the ledger never got**, which is a real state and not a
///   failure: `command_marks.rs` leaves `command_text` empty when `C` and the
///   output that scrolled the prompt off the grid arrived in the same PTY read.
///   The card says [`PEEK_EMPTY_TEXT`] in the muted ink — *"a one-liner that says
///   nothing would read as a broken card, and the gap is the ledger's, honestly"*.
/// * anything else — the command, verbatim.
#[must_use]
pub fn peek_text(tick: &Tick, marks: &[CommandMark]) -> (String, bool) {
    let latest = marks.iter().find(|mark| mark.id == tick.mark);
    let text = latest.map_or("", |mark| mark.command_text.trim());
    let muted = text.is_empty();
    let body = if muted { PEEK_EMPTY_TEXT } else { text };
    let text = if tick.members > 1 {
        format!(
            "{} commands{NAME_PLACE_SEPARATOR}latest: {body}",
            tick.members
        )
    } else if latest.is_some_and(|mark| mark.finished.is_none()) {
        format!("running{NAME_PLACE_SEPARATOR}{body}")
    } else {
        body.to_owned()
    };
    (text, muted)
}

/// Two inks, `t` of the way from the first to the second.
fn mix(from: [u8; 3], to: [u8; 3], t: f32) -> [u8; 3] {
    std::array::from_fn(|channel| {
        lerp(f32::from(from[channel]), f32::from(to[channel]), t)
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

/// How strong the jump flash is `elapsed` after it began, or `None` once it is
/// over.
///
/// `curve` is the house `EASE`, which is CSS's own `ease` and therefore the
/// literal reading of `animation: cmdflash .95s ease`. It is passed in rather than
/// imported so that this file has no opinion about the window's timing and the
/// test can state the curve it is asserting.
///
/// **No reduced-motion branch, deliberately.** The mock-up declares five
/// `prefers-reduced-motion` overrides and none of them is this one, which is the
/// stylesheet saying that a fading row band is not the kind of motion that rule is
/// about: it does not travel, and what it carries is a *reading* — which of a
/// thousand rows the jump landed on — rather than polish. Killing it would leave a
/// silent scroll with nothing to point at the answer.
#[must_use]
pub fn flash_alpha(elapsed: Duration, curve: impl FnOnce(f32) -> f32) -> Option<f32> {
    if !flash_is_running(elapsed) {
        return None;
    }
    let progress = elapsed.as_secs_f32() / JUMP_FLASH.as_secs_f32();
    Some(JUMP_FLASH_OPACITY * (1.0 - curve(progress)))
}

/// Whether a flash that began `elapsed` ago is still owed a frame.
///
/// The **same** predicate [`flash_alpha`] answers `None` by, deliberately shared
/// rather than restated: the deadline that wakes the loop and the paint that runs
/// when it does have to agree exactly, or the window either spins for ever on a
/// flash that draws nothing or stops waking one frame before it has finished
/// fading — and the second of those is invisible until someone looks closely at a
/// band that never quite reaches transparent.
#[must_use]
pub fn flash_is_running(elapsed: Duration) -> bool {
    elapsed < JUMP_FLASH
}

/// The band a jump paints across the row it landed on.
///
/// A **full-width** quad on the row, because the mock-up's `.cmd-jump` is a
/// background on the logical line's own `div` and a `div` in a terminal is the
/// whole width of it. Clipped to the pane's body by the caller: a row partly
/// scrolled off the top must flash the part that is on screen and not a strip of
/// the pane head above it.
#[must_use]
pub fn flash_quads(
    row: [f32; 4],
    alpha: f32,
    scale: f32,
    palette: &ChromePalette,
) -> Vec<OverlayQuad> {
    if row[2] <= row[0] || row[3] <= row[1] {
        return Vec::new();
    }
    rounded_overlay_fill(
        row,
        JUMP_FLASH_RADIUS_LOGICAL_PX * scale,
        palette.accent,
        alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_doc::AnchorId;

    const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];

    fn mark(id: u64, exit_code: Option<i32>) -> CommandMark {
        CommandMark {
            id: CommandMarkId(id),
            prompt: Some(AnchorId(id * 10)),
            start: AnchorId(id * 10 + 1),
            executed: None,
            finished: exit_code.map(|_| AnchorId(id * 10 + 3)),
            command_text: String::new(),
            exit_code,
        }
    }

    fn ok(id: u64) -> CommandMark {
        mark(id, Some(0))
    }

    fn failed(id: u64) -> CommandMark {
        mark(id, Some(1))
    }

    fn said(id: u64, text: &str) -> CommandMark {
        CommandMark {
            command_text: text.to_owned(),
            ..ok(id)
        }
    }

    /// A pane 800 physical pixels tall at scale 1, which makes `avail` 640 and
    /// every number below readable against the mock-up's own arithmetic.
    const BODY: [f32; 4] = [100.0, 50.0, 500.0, 850.0];

    fn rail_of(marks: &[CommandMark]) -> Rail {
        lay_out(BODY, marks, 1.0, None)
    }

    /// The x every pointer test uses: the middle of the resting band, which is
    /// inside the hot one too.
    fn on_band(rail: &Rail) -> f32 {
        (rail.bounds[0] + rail.bounds[2]) / 2.0
    }

    fn centre(tick: &Tick) -> f32 {
        (tick.rect[1] + tick.rect[3]) / 2.0
    }

    /// A full-screen program owns its canvas, and the rail steps off it.
    ///
    /// MUTATION: return the body unconditionally and a pane running `vim` wears a
    /// column of ticks about commands that ran before `vim` started, beside rows
    /// they say nothing about.
    #[test]
    fn a_pane_showing_the_alternate_screen_hangs_no_rail_on_it() {
        assert_eq!(host_rect(BODY, false), Some(BODY));
        assert_eq!(host_rect(BODY, true), None);
    }

    #[test]
    fn a_pane_with_no_command_marks_draws_nothing_at_all() {
        let rail = rail_of(&[]);
        assert!(rail.ticks.is_empty());
        assert_eq!(rail.bounds, [0.0; 4]);
        assert_eq!(rail.hot_bounds, [0.0; 4]);
        assert!(build(&rail, &bt_render::chrome_palette()).quads.is_empty());
        assert_eq!(nearest(&rail, false, 490.0, 450.0), None);
        assert_eq!(nearest(&rail, true, 490.0, 450.0), None);
    }

    /// The ordinal stack: one tick per command, oldest at the top, the whole block
    /// centred on the pane's own middle.
    #[test]
    fn a_handful_of_marks_becomes_an_evenly_spaced_block_centred_on_the_pane() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(rail.ticks.len(), 5);
        assert_eq!(rail.bucket_size, 1);
        // `avail / N` = 128, capped at the nine-pixel ceiling, so pitch is 9 and
        // gap is 7 — the stylesheet's own resting `gap: 7px`.
        let pitch: Vec<f32> = rail
            .ticks
            .windows(2)
            .map(|pair| pair[1].rect[1] - pair[0].rect[1])
            .collect();
        assert_eq!(pitch, vec![9.0; 4]);
        assert_eq!(rail.gap, 7.0);
        assert!(
            rail.ticks
                .iter()
                .all(|tick| tick.rect[3] - tick.rect[1] == 2.0)
        );
        assert!(
            rail.ticks
                .iter()
                .all(|tick| tick.rect[2] - tick.rect[0] == TICK_LENGTH_LOGICAL_PX)
        );
        // Oldest at the top.
        assert_eq!(rail.ticks[0].mark, CommandMarkId(1));
        assert_eq!(rail.ticks[4].mark, CommandMarkId(5));
        // 5 ticks × 2px + 4 gaps × 7px = 38, centred on the body's middle of 450.
        let block = (rail.ticks[0].rect[1], rail.ticks[4].rect[3]);
        assert_eq!(block, (431.0, 469.0));
        // One tick per command means slot and index agree, and nothing is a member
        // of an expansion.
        assert!(
            rail.ticks
                .iter()
                .enumerate()
                .all(|(index, tick)| tick.slot == index && !tick.sub)
        );
    }

    /// The right inset is the lane plus its gap plus the rail's own padding, and it
    /// is *derived* — the test states the derivation rather than the number, so
    /// widening the lane moves this assertion with the pixels.
    #[test]
    fn the_ticks_sit_inboard_of_the_reserved_scroll_lane() {
        let rail = lay_out(BODY, &[ok(1)], 2.0, None);
        let inset = BODY[2] - rail.ticks[0].rect[2];
        assert_eq!(
            inset,
            (TERMINAL_SCROLL_LANE_LOGICAL_PX
                + RAIL_LANE_GAP_LOGICAL_PX
                + RAIL_PADDING_X_LOGICAL_PX)
                * 2.0
        );
        assert!(
            rail.ticks[0].rect[2] + TERMINAL_SCROLL_LANE_LOGICAL_PX * 2.0 <= BODY[2],
            "a thumb in its own lane must never meet a tick"
        );
    }

    /// Past the four-pixel density floor the rail aggregates: one tick per bucket
    /// of `k`, with `k = ceil(N / capacity)`.
    #[test]
    fn more_commands_than_the_block_can_hold_are_bucketed_at_the_density_floor() {
        // avail 640 ⇒ capacity 160.
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(rail.bucket_size, 3, "ceil(400 / 160)");
        assert_eq!(rail.ticks.len(), 134, "ceil(400 / 3)");
        assert_eq!(rail.ticks[0].members, 3);
        assert_eq!(rail.ticks[133].members, 1, "400 = 133×3 + 1");
        // A bucket jumps to its newest member.
        assert_eq!(rail.ticks[0].mark, CommandMarkId(3));
        assert_eq!(rail.ticks[1].mark, CommandMarkId(6));
        // Under the floor nothing is bucketed at all.
        let few: Vec<_> = (1..=160).map(ok).collect();
        assert_eq!(rail_of(&few).bucket_size, 1);
    }

    /// **B15, corrected.** The pitch comes from the ticks that are drawn, so a
    /// dense rail fills the four fifths of the pane it was given instead of the
    /// sixty-two per cent of them the mock-up's own arithmetic leaves it at.
    ///
    /// MUTATION: divide `avail` by `marks.len()` instead of by the drawn count and
    /// the block collapses to 398 of its 640 pixels — the mock-up's bug, reproduced
    /// in S1 on purpose and answered here.
    #[test]
    fn two_hundred_commands_fill_the_block_they_were_given_rather_than_five_eighths_of_it() {
        let marks: Vec<_> = (1..=200).map(ok).collect();
        let rail = rail_of(&marks);
        // capacity 160 ⇒ k = 2 ⇒ 100 ticks drawn.
        assert_eq!(rail.bucket_size, 2);
        assert_eq!(rail.ticks.len(), 100);
        let avail = (BODY[3] - BODY[1]) * RAIL_BLOCK_FRACTION;
        assert_eq!(avail, 640.0);
        let block = rail.ticks[99].rect[3] - rail.ticks[0].rect[1];
        assert!(
            block / avail > 0.98,
            "the drawn block fills its allowance: {block} of {avail}"
        );
        // And the mock-up's own number, stated so the correction is legible: the
        // uncorrected pitch is the four-pixel floor, which is a 398-pixel block.
        let mock_pitch = (avail / marks.len() as f32).clamp(4.0, 9.0);
        let mock_block = 100.0 * 2.0 + 99.0 * (mock_pitch - 2.0).max(2.0);
        assert_eq!(mock_block, 398.0);
        assert!(block > mock_block * 1.5);
    }

    /// Maximum wins: one failure anywhere in a bucket colours the whole tick, and
    /// it colours it at rest.
    #[test]
    fn a_bucket_holding_one_failure_stays_red_without_being_pointed_at() {
        let mut marks: Vec<_> = (1..=400).map(ok).collect();
        marks[4] = failed(5);
        let rail = rail_of(&marks);
        assert_eq!(rail.bucket_size, 3);
        assert!(rail.ticks[1].failed, "marks 4,5,6 — the middle one failed");
        assert!(!rail.ticks[0].failed);
        assert!(!rail.ticks[2].failed);
        let palette = bt_render::chrome_palette();
        let layer = build(&rail, &palette);
        assert!(
            layer
                .quads
                .iter()
                .any(|quad| quad.color == palette.status_err),
            "the failure earns its colour with no pointer anywhere near it"
        );
    }

    /// A command still in flight — `D` never arrived — has no exit status, and no
    /// exit status is not a failure.
    #[test]
    fn a_command_still_running_is_not_a_failed_command() {
        let rail = rail_of(&[mark(1, None)]);
        assert_eq!(rail.ticks.len(), 1, "the tick is drawn all the same");
        assert!(!rail.ticks[0].failed);
    }

    /// The pointer picks an ordinal, not a pixel: one peak, ties to the older tick,
    /// and nothing at all off the band.
    #[test]
    fn the_rail_answers_with_the_tick_nearest_the_pointer_and_only_on_its_own_band() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(
            rail.bounds[2] - rail.bounds[0],
            TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert!(
            rail.ticks.iter().all(|tick| tick.rect[0] >= rail.bounds[0]
                && tick.rect[2] <= rail.bounds[2]
                && tick.rect[1] >= rail.bounds[1]
                && tick.rect[3] <= rail.bounds[3]),
            "every resting tick is inside the band that answers for it"
        );
        let x = on_band(&rail);
        for index in 0..5 {
            assert_eq!(
                nearest(&rail, false, x, centre(&rail.ticks[index])),
                Some(index)
            );
            // Four pixels off a tick is still that tick — the gap is only seven.
            assert_eq!(
                nearest(&rail, false, x, centre(&rail.ticks[index]) + 3.0),
                Some(index)
            );
        }
        // The pane's own body is not the rail.
        let midpoint = (rail.ticks[0].rect[3] + rail.ticks[1].rect[1]) / 2.0;
        assert_eq!(nearest(&rail, false, rail.bounds[0] - 1.0, midpoint), None);
        assert_eq!(nearest(&rail, false, x, rail.bounds[1] - 1.0), None);
        assert_eq!(nearest(&rail, false, x, rail.bounds[3] + 1.0), None);
        // Inside the band but above every tick still answers the topmost: the band
        // is padded, and a press on its padding is a press on the rail.
        assert_eq!(nearest(&rail, false, x, rail.bounds[1] + 1.0), Some(0));
    }

    /// **The exact midpoint yields exactly one crest.** The ruling's own sentence,
    /// as an assertion: "指针停在两条之间不出现并列半高的含糊态".
    ///
    /// MUTATION: light every tick within half a pitch and a pointer parked on a
    /// boundary lights two at once, which is the ambiguity five rounds of errata
    /// were spent removing.
    #[test]
    fn a_pointer_resting_exactly_between_two_ticks_still_lights_exactly_one() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let x = on_band(&rail);
        for pair in 0..4 {
            let midpoint = (centre(&rail.ticks[pair]) + centre(&rail.ticks[pair + 1])) / 2.0;
            // Ties go to the older tick, every time and not by iteration order.
            assert_eq!(nearest(&rail, false, x, midpoint), Some(pair));
            let mut pointer = RailPointer::default();
            let now = Instant::now();
            pointer.aim(rail.ticks.len(), Some(pair), true, now, Motion::Reduced);
            let widths = pointer.width.sample(now, Motion::Reduced);
            assert_eq!(
                widths
                    .iter()
                    .filter(|width| **width == TICK_CREST_LENGTH_LOGICAL_PX)
                    .count(),
                1,
                "one crest and no half heights: {widths:?}"
            );
        }
    }

    /// The mock-up's own five widths, to the tenth of a pixel it prints them at.
    #[test]
    fn the_magnification_curve_is_the_mock_ups_cosine_and_reaches_three_ordinals() {
        for (delta, expected) in [(0, 27.0), (1, 22.5), (2, 13.5), (3, 9.0)] {
            for signed in [delta, -delta] {
                assert!(
                    (curve_length(signed) - expected).abs() < 0.05,
                    "Δ{signed} is {} and should be {expected}",
                    curve_length(signed)
                );
            }
        }
        // Past the reach the curve has already returned to the resting length and
        // stays there — `u` is clamped, so a rail of a hundred ticks costs nothing
        // beyond the five that take part.
        for delta in [4, 40, 400] {
            assert_eq!(curve_length(delta), TICK_LENGTH_LOGICAL_PX);
        }
        assert_eq!(curve_length(0), TICK_CREST_LENGTH_LOGICAL_PX);
    }

    /// **`hot` is a rail-level state.** Every tick changes ink, not just the one
    /// under the pointer.
    ///
    /// MUTATION: colour only the crest and the rail stops answering the hand as a
    /// single instrument — "hovering the rail colours them" becomes "hovering the
    /// rail colours one of them".
    #[test]
    fn pointing_at_the_rail_colours_the_whole_rail_and_not_one_tick_of_it() {
        let palette = bt_render::chrome_palette();
        let mut marks: Vec<_> = (1..=5).map(ok).collect();
        marks[3] = failed(4);
        let rail = rail_of(&marks);
        let now = Instant::now();
        let cold = build(&rail, &palette);
        assert!(
            cold.quads
                .iter()
                .all(|quad| quad.color == palette.command_tick || quad.color == palette.status_err),
            "at rest the ticks are grey and a failure is red"
        );
        let mut pointer = RailPointer::default();
        pointer.aim(rail.ticks.len(), Some(1), true, now, Motion::Reduced);
        let hot = paint(&rail, &pointer, &palette, now, Motion::Reduced);
        // Every ordinary tick is accent, including the four the pointer is not on.
        assert!(hot.quads.iter().any(|quad| quad.color == palette.accent));
        for (index, tick) in rail.ticks.iter().enumerate() {
            let quad = hot
                .quads
                .iter()
                .find(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3])
                .expect("every tick draws");
            let expected = match (index == 1, tick.failed) {
                (true, true) => palette.command_tick_fail_crest,
                (true, false) => palette.command_tick_crest,
                (false, true) => palette.status_err,
                (false, false) => palette.accent,
            };
            assert_eq!(quad.color, expected, "tick {index}");
        }
        // The crest is opaque; the rest of the rail lifts to .55, and a failure to
        // its own .65.
        let alpha_of = |index: usize| {
            let tick = rail.ticks[index].rect;
            hot.quads
                .iter()
                .find(|quad| quad.rect[1] >= tick[1] && quad.rect[3] <= tick[3])
                .expect("every tick draws")
                .alpha
        };
        assert!((alpha_of(1) - TICK_CREST_OPACITY).abs() < 0.001);
        assert!((alpha_of(0) - TICK_HOT_OPACITY).abs() < 0.001);
        assert!((alpha_of(3) - TICK_FAIL_HOT_OPACITY).abs() < 0.001);
    }

    /// The crest covers its own resting tick exactly, and the skirt lifts its
    /// neighbours — all of it leftward, none of it downward.
    #[test]
    fn the_crest_and_its_neighbours_grow_leftward_over_the_ticks_they_replace() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=6).map(ok).collect();
        let rail = rail_of(&marks);
        let now = Instant::now();
        let mut pointer = RailPointer::default();
        pointer.aim(rail.ticks.len(), Some(2), true, now, Motion::Reduced);
        let layer = paint(&rail, &pointer, &palette, now, Motion::Reduced);
        for (index, tick) in rail.ticks.iter().enumerate() {
            let quads: Vec<_> = layer
                .quads
                .iter()
                .filter(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3])
                .collect();
            let left = quads
                .iter()
                .map(|quad| quad.rect[0])
                .fold(f32::INFINITY, f32::min);
            let right = quads
                .iter()
                .map(|quad| quad.rect[2])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!(
                right, tick.rect[2],
                "tick {index}: the right edge never moves"
            );
            // Within a pixel: the quads are snapped to the grid on their way out
            // of `rounded_overlay_fill`, and the curve's own halves (22.5, 13.5)
            // cannot survive that exactly.
            assert!(
                (right - left - curve_length(index as i64 - 2)).abs() <= 1.0,
                "tick {index} is {} long",
                right - left
            );
            assert!(
                quads
                    .iter()
                    .all(|quad| quad.rect[1] >= tick.rect[1] && quad.rect[3] <= tick.rect[3]),
                "thickness stays 2px — only the length answers the pointer"
            );
        }
    }

    /// **The band grows only while the rail is hot** (D-17's middle path).
    ///
    /// MUTATION: return the hot band unconditionally and thirty-seven logical
    /// pixels of every pane's right edge stop underlining hyperlinks for a reader
    /// who never went near the rail. Return the resting one unconditionally and a
    /// hand walking onto the crest it just raised drops off the rail.
    #[test]
    fn the_band_takes_the_crests_width_only_while_the_rail_is_already_hot() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        assert_eq!(
            rail.bounds[2] - rail.bounds[0],
            TICK_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert_eq!(
            rail.hot_bounds[2] - rail.hot_bounds[0],
            TICK_CREST_LENGTH_LOGICAL_PX + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
        assert_eq!(
            rail.bounds[2], rail.hot_bounds[2],
            "the right edge is one edge"
        );
        assert_eq!(
            [rail.bounds[1], rail.bounds[3]],
            [rail.hot_bounds[1], rail.hot_bounds[3]]
        );
        // A point over the drawn crest but outside the resting box: nothing while
        // cold, the crest's own tick while hot.
        let y = centre(&rail.ticks[2]);
        let on_crest = rail.bounds[0] - 4.0;
        assert!(on_crest > rail.hot_bounds[0]);
        assert_eq!(nearest(&rail, false, on_crest, y), None);
        assert_eq!(nearest(&rail, true, on_crest, y), Some(2));
        // Leaving the *larger* box is what cools it, so the two thresholds are
        // never the same pixel and the growth cannot flap.
        assert_eq!(nearest(&rail, true, rail.hot_bounds[0] - 1.0, y), None);
        // A bucketed rail holds the unfolded members' four-pixel step open too.
        let dense: Vec<_> = (1..=400).map(ok).collect();
        let dense = rail_of(&dense);
        assert_eq!(
            dense.hot_bounds[2] - dense.hot_bounds[0],
            TICK_CREST_LENGTH_LOGICAL_PX
                + TICK_SUB_OFFSET_LOGICAL_PX
                + RAIL_PADDING_X_LOGICAL_PX * 2.0
        );
    }

    /// Pointing at a bucket opens it: its members become ticks of their own, one
    /// step out of the column, and the neighbours give up the room.
    #[test]
    fn a_bucket_under_the_pointer_unfolds_into_its_members_and_the_block_holds_its_extent() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        assert_eq!(folded.bucket_size, 3);
        let open = lay_out(BODY, &marks, 1.0, Some(7));
        // Two more ticks than the folded rail: one bucket of three became three.
        assert_eq!(open.ticks.len(), folded.ticks.len() + 2);
        let members: Vec<_> = open.ticks.iter().filter(|tick| tick.sub).collect();
        assert_eq!(members.len(), 3);
        assert!(
            members
                .iter()
                .all(|tick| tick.slot == 7 && tick.members == 1),
            "members keep the bucket's slot id — that is what the hysteresis is keyed on"
        );
        assert_eq!(
            members.iter().map(|tick| tick.mark).collect::<Vec<_>>(),
            vec![CommandMarkId(22), CommandMarkId(23), CommandMarkId(24)],
            "slot 7 of buckets of three is commands 22, 23 and 24"
        );
        // One width base for everyone: an unfolded member is the same length as
        // any other tick, moved four pixels left.
        assert!(
            members
                .iter()
                .all(|tick| tick.rect[2] - tick.rect[0] == TICK_LENGTH_LOGICAL_PX)
        );
        let column = folded.ticks[0].rect[2];
        assert!(
            members
                .iter()
                .all(|tick| tick.rect[2] == column - TICK_SUB_OFFSET_LOGICAL_PX)
        );
        assert!(
            open.ticks
                .iter()
                .filter(|tick| !tick.sub)
                .all(|tick| tick.rect[2] == column)
        );
        // Neighbours compress: the block keeps its extent rather than growing by
        // two more rows.
        let extent = |rail: &Rail| rail.ticks[rail.ticks.len() - 1].rect[3] - rail.ticks[0].rect[1];
        assert!(
            (extent(&open) - extent(&folded)).abs() <= 1.0,
            "folded {} vs open {}",
            extent(&folded),
            extent(&open)
        );
        assert!(open.gap < folded.gap, "the neighbours gave the room up");
    }

    /// **Hysteresis.** A pointer standing on the boundary of an open group answers
    /// the same on the next frame, and on the frame after that.
    ///
    /// MUTATION: decide the expansion from the *open* geometry's nearest tick and
    /// the rail alternates between folded and unfolded on consecutive frames — the
    /// mock-up's own recursion guard bounds the work and leaves precisely this
    /// flicker in place.
    #[test]
    fn the_unfolded_bucket_does_not_flap_when_the_pointer_sits_on_its_boundary() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        // Every boundary of every drawn tick, and the two edges of the group that
        // opens under each — the exhaustive version of "a pointer on the seam".
        for slot in [0_usize, 1, 7, 60, 133] {
            let seed = centre(&folded.ticks[slot]);
            let first = resolve(BODY, &marks, 1.0, None, seed);
            let mut state = first.expanded;
            let mut rail = first.rail.clone();
            for frame in 0..4 {
                let next = resolve(BODY, &marks, 1.0, state, seed);
                assert_eq!(
                    next.expanded, first.expanded,
                    "slot {slot} changed its mind on frame {frame}"
                );
                assert_eq!(next.rail, rail, "slot {slot} redrew on frame {frame}");
                state = next.expanded;
                rail = next.rail;
            }
            let Some(open) = first.expanded else { continue };
            let span = group_span(&first.rail, open).expect("the group is open");
            for edge in [
                span[0],
                span[1],
                span[0] - 0.5,
                span[1] + 0.5,
                (span[0] + span[1]) / 2.0,
            ] {
                let once = resolve(BODY, &marks, 1.0, Some(open), edge);
                let twice = resolve(BODY, &marks, 1.0, once.expanded, edge);
                assert_eq!(
                    once.expanded, twice.expanded,
                    "slot {open} flapped at {edge}"
                );
                assert_eq!(once.rail, twice.rail);
                assert_eq!(once.nearest, twice.nearest);
            }
        }
    }

    /// Hovering *inside* an expansion keeps it open, and moving to another bucket
    /// swaps it rather than closing it.
    #[test]
    fn walking_through_an_expansion_keeps_it_open_and_leaving_it_hands_it_to_the_next_bucket() {
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let folded = rail_of(&marks);
        let opened = resolve(BODY, &marks, 1.0, None, centre(&folded.ticks[7]));
        assert_eq!(opened.expanded, Some(7));
        // Every member of the open group holds it open, including the far ends,
        // which is the whole of the mock-up's hysteresis sentence.
        let members: Vec<f32> = opened
            .rail
            .ticks
            .iter()
            .filter(|tick| tick.sub)
            .map(centre)
            .collect();
        assert_eq!(members.len(), 3);
        for y in members {
            let held = resolve(BODY, &marks, 1.0, Some(7), y);
            assert_eq!(held.expanded, Some(7));
            assert!(held.rail.ticks[held.nearest.expect("a crest")].sub);
        }
        // Far away is another bucket, and the expansion moves with the hand.
        let elsewhere = resolve(BODY, &marks, 1.0, Some(7), centre(&folded.ticks[60]));
        assert_eq!(elsewhere.expanded, Some(60));
        // A rail with nothing to aggregate never opens anything.
        let few: Vec<_> = (1..=5).map(ok).collect();
        let single = rail_of(&few);
        let plain = resolve(BODY, &few, 1.0, None, centre(&single.ticks[2]));
        assert_eq!(plain.expanded, None);
        assert_eq!(plain.nearest, Some(2));
        // And a stale slot — the ledger shrank under an open group — folds back
        // rather than laying out a bucket that is not there.
        let stale = resolve(BODY, &few, 1.0, Some(90), centre(&single.ticks[2]));
        assert_eq!(stale.expanded, None);
    }

    /// The card's four readings.
    #[test]
    fn the_glance_card_says_the_command_the_count_the_running_state_or_the_honest_gap() {
        let marks = vec![
            said(1, "cargo test --workspace"),
            said(2, "git status"),
            said(3, "  "),
            CommandMark {
                command_text: "sleep 30".to_owned(),
                finished: None,
                exit_code: None,
                ..ok(4)
            },
        ];
        let tick = |mark: u64, members: usize| Tick {
            mark: CommandMarkId(mark),
            members,
            failed: false,
            slot: 0,
            sub: false,
            rect: [0.0; 4],
        };
        assert_eq!(
            peek_text(&tick(1, 1), &marks),
            ("cargo test --workspace".to_owned(), false)
        );
        assert_eq!(
            peek_text(&tick(2, 4), &marks),
            ("4 commands · latest: git status".to_owned(), false)
        );
        assert_eq!(
            peek_text(&tick(4, 1), &marks),
            ("running · sleep 30".to_owned(), false)
        );
        // The ledger's honest empty — a word, in the muted ink.
        assert_eq!(peek_text(&tick(3, 1), &marks), ("command".to_owned(), true));
        assert_eq!(
            peek_text(&tick(3, 9), &marks),
            ("9 commands · latest: command".to_owned(), true)
        );
        // A mark that is no longer in the ledger says the same honest thing rather
        // than nothing at all.
        assert_eq!(
            peek_text(&tick(99, 1), &marks),
            ("command".to_owned(), true)
        );
    }

    /// The card is placed against the crest, not against the two-pixel bar.
    #[test]
    fn the_glance_card_stands_against_the_crest_at_its_full_length() {
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let host = peek_host(&rail, 2).expect("a tick to stand against");
        assert_eq!(host[2], rail.ticks[2].rect[2]);
        assert_eq!(host[2] - host[0], TICK_CREST_LENGTH_LOGICAL_PX);
        assert_eq!(
            [host[1], host[3]],
            [rail.ticks[2].rect[1], rail.ticks[2].rect[3]]
        );
        assert_eq!(peek_host(&rail, 9), None);
    }

    /// The three timelines, each on its own span and each still owing frames until
    /// its own is up.
    #[test]
    fn the_three_transitions_run_for_their_own_durations_and_then_stop_asking_for_frames() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let rail = rail_of(&marks);
        let start = Instant::now();
        let mut pointer = RailPointer::default();
        assert!(pointer.is_resting(start, Motion::Full));
        pointer.aim(rail.ticks.len(), Some(2), true, start, Motion::Full);
        assert!(pointer.is_animating(start, Motion::Full));
        assert!(!pointer.is_resting(start, Motion::Full));
        // Halfway through the shortest span everything is between its two ends.
        let mid = start + Duration::from_millis(50);
        let widths = pointer.width.sample(mid, Motion::Full);
        assert!(
            widths[2] > TICK_LENGTH_LOGICAL_PX && widths[2] < TICK_CREST_LENGTH_LOGICAL_PX,
            "the crest is on its way: {}",
            widths[2]
        );
        let (warmth, moving) = pointer.hot_ink.sample(mid, Motion::Full);
        assert!(warmth > 0.0 && warmth < 1.0 && moving);
        // Each span ends on its own schedule, longest last.
        assert!(pointer.is_animating(start + TICK_WIDTH_TRANSITION, Motion::Full));
        assert!(pointer.is_animating(start + TICK_OPACITY_TRANSITION, Motion::Full));
        assert!(!pointer.is_animating(start + TICK_BACKGROUND_TRANSITION, Motion::Full));
        assert!(
            TICK_WIDTH_TRANSITION < TICK_OPACITY_TRANSITION
                && TICK_OPACITY_TRANSITION < TICK_BACKGROUND_TRANSITION
        );
        // Landed, the picture is the one the aim asked for.
        let landed = start + TICK_BACKGROUND_TRANSITION;
        let quads = paint(&rail, &pointer, &palette, landed, Motion::Full).quads;
        assert!(
            quads
                .iter()
                .any(|quad| quad.color == palette.command_tick_crest)
        );
        assert!(quads.iter().any(|quad| quad.color == palette.accent));
        // Cooling brings it all the way back to the resting picture, and only then
        // is the cached layer true again.
        pointer.aim(rail.ticks.len(), None, false, landed, Motion::Full);
        assert!(!pointer.is_resting(landed, Motion::Full));
        let cold = landed + TICK_BACKGROUND_TRANSITION;
        assert!(pointer.is_resting(cold, Motion::Full));
        assert_eq!(
            paint(&rail, &pointer, &palette, cold, Motion::Full).quads,
            build(&rail, &palette).quads
        );
        // Reduced motion has no timeline at all: the aim is simply true.
        let mut still = RailPointer::default();
        still.aim(rail.ticks.len(), Some(2), true, start, Motion::Reduced);
        assert!(!still.is_animating(start, Motion::Reduced));
    }

    /// A rebuild drops the travels the old ticks owned; the rail's own warmth
    /// survives it.
    #[test]
    fn a_bucket_opening_mid_hover_reseeds_the_widths_and_keeps_the_rail_warm() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=400).map(ok).collect();
        let now = Instant::now();
        let mut cache = RailCache::default();
        let key = RailKey {
            revision: 1,
            body: BODY,
            scale: 1.0,
            expanded: None,
        };
        cache.install(key, rail_of(&marks), &palette);
        let ticks = cache.rail().ticks.len();
        cache
            .pointer_mut()
            .aim(ticks, Some(7), true, now, Motion::Full);
        assert!(cache.pointer().is_animating(now, Motion::Full));
        // The bucket opens: more ticks, so the widths in flight belonged to
        // somebody else and are dropped.
        cache.pointer_mut().expand(Some(7));
        let opened = RailKey {
            expanded: Some(7),
            ..key
        };
        cache.install(opened, lay_out(BODY, &marks, 1.0, Some(7)), &palette);
        let ticks = cache.rail().ticks.len();
        assert_eq!(ticks, 136, "the bucket of three opened");
        let later = now + Duration::from_millis(20);
        cache
            .pointer_mut()
            .aim(ticks, Some(7), true, later, Motion::Full);
        // The travel starts over from the resting length rather than continuing
        // from widths that belonged to a different set of ticks — and it starts
        // over from *rest* because rest is what `install` has just drawn.
        let widths = cache.pointer().width.sample(later, Motion::Full);
        assert_eq!(widths.len(), ticks);
        assert!(widths.iter().all(|width| *width == TICK_LENGTH_LOGICAL_PX));
        assert!(cache.pointer().width.is_running(later, Motion::Full));
        // But the rail is still hot — a column that blinked back to grey because a
        // bucket opened would read as the hover having been lost.
        assert!(cache.pointer().hot_ink.sample(later, Motion::Full).0 > 0.0);
        assert!(!cache.pointer().is_resting(later, Motion::Full));
    }

    /// The cache is a function of the ledger, the pane and the open bucket, and of
    /// nothing else.
    #[test]
    fn the_rail_cache_rebuilds_on_the_revision_and_the_geometry_and_on_nothing_else() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let key = RailKey {
            revision: 7,
            body: BODY,
            scale: 1.0,
            expanded: None,
        };
        let mut cache = RailCache::default();
        assert!(cache.needs_rebuild(key));
        cache.install(
            key,
            lay_out(key.body, &marks, key.scale, key.expanded),
            &palette,
        );
        assert_eq!(cache.rail().ticks.len(), 5);
        assert_eq!(
            cache.layer().quads.len(),
            build(cache.rail(), &palette).quads.len()
        );
        assert!(
            !cache.needs_rebuild(key),
            "the same frame asks again for free"
        );
        // The four things a rail is a function of, one at a time.
        assert!(cache.needs_rebuild(RailKey { revision: 8, ..key }));
        assert!(cache.needs_rebuild(RailKey {
            body: [BODY[0], BODY[1], BODY[2], BODY[3] + 1.0],
            ..key
        }));
        assert!(cache.needs_rebuild(RailKey { scale: 2.0, ..key }));
        assert!(cache.needs_rebuild(RailKey {
            expanded: Some(2),
            ..key
        }));
        // And the things it is not: the pointer's *height* over the rail and the
        // viewport scrolling are neither of them in the key — the crest travels as
        // a set of widths over geometry this key already fixed.
        cache.clear();
        assert!(cache.needs_rebuild(key));
        assert_eq!(cache.pointer().expanded(), None);
    }

    /// A cold, still rail is handed back its cached picture; anything else is
    /// painted fresh.
    #[test]
    fn the_cache_hands_back_its_own_layer_only_while_nothing_is_happening_to_the_rail() {
        let palette = bt_render::chrome_palette();
        let marks: Vec<_> = (1..=5).map(ok).collect();
        let now = Instant::now();
        let mut cache = RailCache::default();
        cache.install(
            RailKey {
                revision: 1,
                body: BODY,
                scale: 1.0,
                expanded: None,
            },
            rail_of(&marks),
            &palette,
        );
        assert_eq!(
            cache.picture(&palette, now, Motion::Full).quads,
            cache.layer().quads
        );
        let ticks = cache.rail().ticks.len();
        cache
            .pointer_mut()
            .aim(ticks, Some(0), true, now, Motion::Reduced);
        assert_ne!(
            cache.picture(&palette, now, Motion::Reduced).quads,
            cache.layer().quads
        );
    }

    /// The 950 ms row flash, from its opening alpha to nothing.
    #[test]
    fn the_jump_flash_runs_for_950ms_and_then_stops_existing() {
        let ease = |x: f32| crate::cubic_bezier(x, EASE);
        assert_eq!(flash_alpha(Duration::ZERO, ease), Some(JUMP_FLASH_OPACITY));
        let midway = flash_alpha(Duration::from_millis(475), ease).expect("still running");
        assert!(
            midway > 0.0 && midway < JUMP_FLASH_OPACITY,
            "halfway is between the two ends, not at either: {midway}"
        );
        let late = flash_alpha(Duration::from_millis(900), ease).expect("still running");
        assert!(late < midway, "the curve only ever fades");
        assert_eq!(flash_alpha(JUMP_FLASH, ease), None);
        assert_eq!(flash_alpha(Duration::from_secs(3), ease), None);
        // The deadline the loop wakes on and the paint that runs when it does are
        // the same predicate, at every instant on both sides of the end.
        for millis in [0, 1, 474, 949, 950, 951, 3_000] {
            let elapsed = Duration::from_millis(millis);
            assert_eq!(
                flash_is_running(elapsed),
                flash_alpha(elapsed, ease).is_some(),
                "the wake-up and the picture disagree at {millis}ms"
            );
        }
    }

    #[test]
    fn a_flash_paints_the_whole_row_and_nothing_at_all_for_an_empty_one() {
        let palette = bt_render::chrome_palette();
        let quads = flash_quads([100.0, 200.0, 500.0, 218.0], 0.22, 1.0, &palette);
        assert!(!quads.is_empty());
        assert!(quads.iter().all(|quad| quad.color == palette.accent));
        let left = quads
            .iter()
            .map(|quad| quad.rect[0])
            .fold(f32::INFINITY, f32::min);
        let right = quads
            .iter()
            .map(|quad| quad.rect[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((left, right), (100.0, 500.0));
        assert!(flash_quads([100.0, 200.0, 100.0, 218.0], 0.22, 1.0, &palette).is_empty());
        assert!(flash_quads([100.0, 218.0, 500.0, 218.0], 0.22, 1.0, &palette).is_empty());
    }
}
