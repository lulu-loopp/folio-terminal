//! **One display frame, and every animation in this window draws on it**
//! (owner's report 2026-09-18: 「还是很卡顿，Windows 比 Mac 更卡，两个图标的移动
//! 还是一顿一顿的」).
//!
//! # A rate nothing enforces is not a rate
//!
//! Every animation in this window books its next wake-up by adding a frame
//! interval to *now* — the instant the turn began — and `about_to_wait` runs
//! after **every** event rather than only when a deadline expires. The present
//! each animated frame asks for is itself an event, so the wake-up that was
//! meant to space frames out was never reached: the turn presented, winit
//! delivered the `RedrawRequested` the present had asked for, the loop came
//! round, the animation found its ease had moved a hair and asked for another
//! one. The deadline arithmetic was correct and inert.
//!
//! `advance_strip_animation` has known this since the ring was measured at 120
//! ticks a second against a declared 62.5, and it fixed it for itself with a
//! pair: a gate that turns a tick away until the rate has elapsed, and a
//! deadline clamped to the same clock so the loop is woken exactly when the
//! next tick is allowed. **This module is that pair, hoisted out of the strip
//! and made the window's**, because it was never a fact about the strip: it is
//! a fact about the glass.
//!
//! # What the two recordings of 2026-09-18 measured
//!
//! Pressing `‹›` on a typeset formula runs a ninety-millisecond journey, and
//! the owner recorded one on each platform with `BT_PERF_TRACE` on.
//!
//! * **macOS.** Presents 16–17 ms apart, every one of them with ~14 ms spent
//!   inside the surface acquire: `CAMetalLayer`'s drawable blocks for a vertical
//!   blank, so the loop was paced by the swapchain rather than by anything this
//!   program decided. Six frames of a ninety-millisecond journey, which is
//!   correct for a 60 Hz display and is why the Mac merely looked coarse.
//! * **Windows.** The acquire costs 10–50 µs — the DXGI back buffer is handed
//!   over without a wait — so nothing threw the loop back at the display's rate.
//!   Thirteen to twenty presents landed 2–5 ms apart, each composing a picture
//!   a hair different from the one before it, and then a single
//!   `IDXGISwapChain::Present` blocked for **78–86 ms** paying for all of them
//!   at once. The eye sees a burst of near-identical steps and then a freeze,
//!   which is exactly the word the report used.
//!
//! So the two platforms differed in where the back-pressure lived, not in what
//! this window asked for. Both asked for frames nobody could see.
//!
//! # What this is, and what it deliberately is not
//!
//! An animation asks for its **next frame**, not for *now*: the wake it books is
//! the last present plus one display frame, and until that instant arrives the
//! gate turns its advance away and books the turn that will draw it. The
//! interval comes from the display the window is on ([`FrameClock::follow`]),
//! so a 144 Hz panel gets fourteen frames of a ninety-millisecond journey where
//! a 60 Hz one gets six.
//!
//! # The gate decides who ASKS for a frame, never who is carried by one
//!
//! This is the correction of review 2026-09-18 (P1), and getting it the wrong
//! way round is a hard failure rather than a rough edge. The clock the gate is
//! read against is *every* present this window makes, a shell's output included
//! — so a pane printing every five milliseconds refuses an animation's turn for
//! as long as it keeps printing. That is **right** and it is the whole economy
//! of the thing: a picture reached the glass a moment ago, so the animation does
//! not need to ask for one of its own. It is right *only* if that picture
//! carried the animation forward.
//!
//! So the rule has two halves and neither works alone:
//!
//! * **Every frame this window composes, for whatever reason, carries every
//!   journey that is running** — sampled from the clock at the instant the frame
//!   is of (`Runtime::carry_live_journeys`). Nothing is ever projected or drawn
//!   from a sample some earlier tick left behind.
//! * **The gate decides only whether a journey has to ask for a frame of its
//!   own**, and its refusal books one at [`FrameClock::next_frame`]. Under a
//!   flood the journey rides the flood's frames; the moment the flood stops it
//!   gets its own.
//!
//! And **a journey's end is not paced at all**. Landing is owed to the document
//! rather than to the glass — the block is told which face it wears, a hold is
//! released, the motion is dropped — so it is a plain deadline in the fold and
//! runs whatever else the window is doing. A gate in front of it is a change of
//! face that a busy neighbour can postpone for ever, which is exactly what the
//! review found.
//!
//! It is **not** a frame budget, a skipped-frame compensator or a second clock
//! animations advance by. Every animation in this window already samples its
//! progress from the wall clock at compose time against an instant it started
//! at, so drawing fewer frames draws the same motion with fewer steps in it —
//! never a slower one. A turn the gate refuses loses no work either: the advance
//! it refused is idempotent and runs at the paced instant with the clock it
//! reads then.
//!
//! It is also not on the road any keystroke or byte of shell output takes. Those
//! publish from their own events through `publish_frame`, which this does not
//! stand in front of; what stands behind the gate is the set of clocks that
//! would otherwise turn the loop with nothing but a tween to show for it.

use std::time::{Duration, Instant};

/// **What one display frame is worth when the platform will not say.**
///
/// Sixty hertz, which is the rate every animation in this window was written
/// against and the rate a monitor that answers nothing is overwhelmingly likely
/// to be running at. It is a fallback and not a target: a window on a display
/// that reports itself gets that display's interval, and this number is never
/// consulted again.
///
/// **It is also the only sixteen milliseconds left in this window** (closure
/// review 2, 2026-09-18). The strip kept a `STRIP_ANIMATION_FRAME` of its own
/// and so did the thumbs' fade, the panes' flight and the drag's auto-scroll;
/// two rates in one window is a door that admits and a door that refuses at the
/// same instant, which at 144 Hz is how a picture that had arrived found every
/// one of them shut. They all read [`FrameClock::interval`] now, and what
/// remains here is the number that clock is born with.
///
/// **What a tick of the strip actually costs, measured rather than budgeted.**
/// The retired constant carried this note and it is still the reason the rate
/// can be held to: it used to read "an animating ring costs one rasterize of a
/// 15px SVG per frame, measured at 16.5µs". The rasterize was measured honestly;
/// what was never measured is what the tick dragged behind it. Every tick called
/// `publish_frame(Expose)`, and `publish_frame_inner` composed a whole terminal
/// picture before anything could opt out: full grid capture, viewport
/// projection, inline-reference scan, IME re-anchor, then a present that
/// re-shaped the grid's text. Sampled on that workspace at **6-8ms of
/// main-thread CPU per tick — a ~400x miss** — with the main thread
/// allocator-bound, and a `Start-Sleep 30` emitting *zero bytes* holding one
/// core at 69%. A tick now re-presents the picture already on the glass instead
/// of composing another one, which is what makes the 16.5µs a budget this rate
/// can be held to; if a tick ever composes a terminal frame again, the display's
/// rate is the wrong rate for it and this number is not the thing to change.
pub const DEFAULT_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// The band a monitor's answer has to fall inside to be believed.
///
/// A refresh rate is read from the platform, and a platform that is confused —
/// a headless session, a virtual display, a monitor handle caught mid-hotplug —
/// can answer zero or something absurd. Zero would divide, and either extreme
/// would be this window pacing itself to a display that does not exist, so an
/// answer outside the band is treated as no answer at all and the default
/// stands. Twenty hertz is slower than any panel sold; a thousand is faster than
/// any panel sold, and both are far enough outside the range of real hardware
/// that nothing genuine is refused here.
const SLOWEST_DISPLAY_MILLIHERTZ: u32 = 20_000;
const FASTEST_DISPLAY_MILLIHERTZ: u32 = 1_000_000;

/// **The interval a display of this refresh rate hands the glass a picture at**,
/// or `None` when the answer cannot be believed.
///
/// winit reports millihertz, so 60 Hz arrives as `60_000` and the period is
/// `10^12 / millihertz` nanoseconds — 16.667 ms rather than the 16 ms the
/// constant above rounds to, which over a ninety-millisecond journey is the
/// difference between six frames and seven.
#[must_use]
pub fn interval_from_millihertz(millihertz: u32) -> Option<Duration> {
    (SLOWEST_DISPLAY_MILLIHERTZ..=FASTEST_DISPLAY_MILLIHERTZ)
        .contains(&millihertz)
        .then(|| Duration::from_nanos(1_000_000_000_000 / u64::from(millihertz)))
}

/// **One window's display frame**, and the gate every animation in it draws
/// through.
///
/// The clock it is read against is the window's own `last_present_at`, which is
/// written by every present whichever door made it — so "one frame since the
/// glass last had one" is measured against pictures that actually reached the
/// glass and not against turns of the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameClock {
    interval: Duration,
    admitted: bool,
    owed: bool,
    running: Lanes,
    skipped: u64,
}

/// **Where the journeys that are running keep their pictures**, and therefore
/// what a frame composed for somebody else has to rebuild to carry them (review
/// 2026-09-18 round 2).
///
/// Two lanes because this window has two, and the difference is a real cost: the
/// chrome is the strip, the rail, every pane head and the focus column's
/// thumbnails, and rebuilding it walks every tab and measures text; the overlay
/// is the layer stack over the panes. A tip fading owes the second and nothing
/// of the first, and a carry that rebuilt both would charge every fade the price
/// of the strip.
///
/// A block changing face is in neither, which is why it is not a variant here:
/// its two numbers go into the session and the *projection* reads them, so
/// carrying it is one write and no rebuild at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lanes {
    /// The strip's ring, a pane in flight, the `˅` turning, a card breathing.
    pub chrome: bool,
    /// A tip, a hint card, a notice, the rails, a thumb, a glance card, a float,
    /// a band's two marks.
    pub overlay: bool,
}

impl Lanes {
    /// Whether anything at all is mid-flight.
    #[must_use]
    pub fn any(self) -> bool {
        self.chrome || self.overlay
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self {
            interval: DEFAULT_FRAME_INTERVAL,
            // A window that has never turned is a window that has never
            // presented, and a window with nothing on its glass is not a frame
            // behind anything. `open` overwrites this on the first turn.
            admitted: true,
            owed: false,
            running: Lanes::default(),
            skipped: 0,
        }
    }
}

impl FrameClock {
    /// What one frame of the display this window is on is worth.
    #[must_use]
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// **Follow the display the window is on**, and answer whether the interval
    /// moved.
    ///
    /// Called when the window is born and again whenever it can have changed
    /// displays — a move, a scale change — because the two panels a window is
    /// dragged between are routinely not the same rate, and an animation paced
    /// to the one it left is the defect this module is about, one display over.
    /// An answer that cannot be believed leaves the interval exactly as it was,
    /// so a monitor handle that goes missing for a moment does not reset a
    /// window that had already been told the truth.
    pub fn follow(&mut self, millihertz: Option<u32>) -> bool {
        let Some(interval) = millihertz.and_then(interval_from_millihertz) else {
            return false;
        };
        let moved = interval != self.interval;
        self.interval = interval;
        moved
    }

    /// **The next instant an animation in this window may draw.**
    ///
    /// A window that has never presented is owed its first frame immediately:
    /// there is no picture on the glass to be a frame behind.
    #[must_use]
    pub fn next_frame(&self, last_present: Option<Instant>, now: Instant) -> Instant {
        last_present.map_or(now, |last| last + self.interval)
    }

    /// **The absolute deadline owned by the last picture on the glass.**
    ///
    /// Unlike [`Self::next_frame`], this has no `now` fallback: a window that
    /// has never presented is already admitted by [`Self::is_due`], so its
    /// current turn must ask for the first picture rather than book a timer for
    /// "now". Once a picture exists, asking twice without another present gives
    /// the same instant even after that instant has passed.
    #[must_use]
    pub fn deadline(&self, last_present: Option<Instant>) -> Option<Instant> {
        last_present.map(|last| last + self.interval)
    }

    /// Whether the display has moved on since the last picture reached it.
    #[must_use]
    pub fn is_due(&self, last_present: Option<Instant>, now: Instant) -> bool {
        last_present.is_none_or(|last| now.saturating_duration_since(last) >= self.interval)
    }

    /// **A turn the gate turned away**, and the record that the loop still owes
    /// the frame it refused.
    ///
    /// Both halves matter. Without the debt a refusal could be the last word: a
    /// band whose shape changed under a motionless pointer owes a frame and has
    /// no clock of its own to ask for one, so the picture it was refused would
    /// wait for the next thing to happen to the window. The debt joins the
    /// window's deadline fold, the loop wakes at [`Self::next_frame`], and the
    /// advance that was refused runs then.
    pub fn refuse(&mut self) {
        self.owed = true;
        self.skipped = self.skipped.saturating_add(1);
    }

    /// **What is mid-flight in this window, as a fact rather than as a side
    /// effect** (review 2026-09-18 round 2, P1).
    ///
    /// Told once a turn, recomputed from scratch from the journeys' own "am I
    /// still moving" predicates — the same ones their deadlines are made of —
    /// and therefore `false` on the very turn after the last one lands. The
    /// first version of this was set by the *gate*, on the reasoning that an
    /// animation only asks when it is animating; four advancers ask before they
    /// have established they have anything at all, so a window with an empty tip
    /// host and an empty float host marked itself running on every turn for
    /// ever, and every ordinary keystroke then rebuilt the whole interface. A
    /// question is not evidence. This is.
    pub fn note_running(&mut self, lanes: Lanes) {
        self.running = lanes;
    }

    /// What this window had mid-flight when its last turn finished.
    ///
    /// The *last* turn, and deliberately: a frame is composed from all sorts of
    /// places — a keystroke's own event, a drain, an expose — and most of them
    /// are nowhere near a turn's clock run. Last turn's answer is the only one
    /// that exists at those moments, and an animation that was running a
    /// millisecond ago is running now.
    #[must_use]
    pub fn running(&self) -> Lanes {
        self.running
    }

    /// Whether a refused frame is still owed.
    #[must_use]
    pub fn owes_a_frame(&self) -> bool {
        self.owed
    }

    /// **Open a turn**, and decide once for all of it whether the animations in
    /// this window may draw.
    ///
    /// Once, and that is the whole reason this is a state on the turn rather
    /// than a question each advance asks for itself. A turn draws several
    /// animations in a row and each of them may present; asked live, the first
    /// one to reach the glass would make every later one a frame behind — the
    /// block's height would move on this frame and the two marks that ride it on
    /// the next, which is a *worse* stutter than the one this module is about,
    /// and it would be invisible to any test that ran one animation at a time.
    /// They are all drawing the same picture of the same instant, so they are
    /// all admitted or all refused together.
    ///
    /// The debt is cleared here for the same reason: it is a fact about the turn
    /// that is beginning, so whatever this turn refuses is what this turn books,
    /// and a turn that refuses nothing books nothing and lets the window go
    /// idle.
    pub fn open(&mut self, last_present: Option<Instant>, now: Instant) {
        self.admitted = self.is_due(last_present, now);
        self.owed = false;
    }

    /// Whether this turn is one the animations in this window may draw on.
    #[must_use]
    pub fn admits(&self) -> bool {
        self.admitted
    }

    /// How many turns the gate has turned away since the last present, read and
    /// reset by the present that reports it (`pace_skipped=`).
    pub fn take_skipped(&mut self) -> u64 {
        std::mem::take(&mut self.skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_FRAME_INTERVAL, FrameClock, Lanes, interval_from_millihertz};
    use std::time::{Duration, Instant};

    /// Every instant in these tests is derived from one base by arithmetic, so
    /// nothing here waits, sleeps or reads the clock twice.
    fn clock_at(millihertz: u32) -> FrameClock {
        let mut clock = FrameClock::default();
        assert!(clock.follow(Some(millihertz)));
        clock
    }

    /// How many frames a journey of `span` is drawn in, counted the way the
    /// loop counts them: draw one, book the next, wake, draw.
    fn frames_in(clock: &FrameClock, span: Duration) -> usize {
        let start = Instant::now();
        let mut presented = None;
        let mut drawn = 0;
        let mut now = start;
        while now < start + span {
            assert!(
                clock.is_due(presented, now),
                "the gate refused a booked wake"
            );
            drawn += 1;
            presented = Some(now);
            now = clock.next_frame(presented, now);
        }
        drawn
    }

    /// RED — **a display that reports itself is the rate**, and one that cannot
    /// be believed is not.
    #[test]
    fn the_interval_comes_from_the_display_and_nonsense_is_refused() {
        assert_eq!(
            interval_from_millihertz(60_000),
            Some(Duration::from_nanos(16_666_666))
        );
        assert_eq!(
            interval_from_millihertz(144_000),
            Some(Duration::from_nanos(6_944_444))
        );
        for refused in [0, 1, 19_999, 1_000_001, u32::MAX] {
            assert_eq!(
                interval_from_millihertz(refused),
                None,
                "{refused} mHz is not a display anybody is looking at"
            );
        }
        let mut clock = FrameClock::default();
        assert!(!clock.follow(None));
        assert!(!clock.follow(Some(0)));
        assert_eq!(
            clock.interval(),
            DEFAULT_FRAME_INTERVAL,
            "a monitor that answers nothing leaves the window on the default"
        );
        assert!(clock.follow(Some(120_000)));
        assert!(
            !clock.follow(Some(120_000)),
            "being told the same rate twice is not a change of display"
        );
        assert!(!clock.follow(None));
        assert_eq!(
            clock.interval(),
            Duration::from_nanos(8_333_333),
            "a handle that goes missing does not reset a window that was told the truth"
        );
    }

    /// RED — **a ninety-millisecond journey is drawn in as many frames as the
    /// display has in ninety milliseconds, and not one more** (owner's report
    /// 2026-09-18).
    ///
    /// The Windows recording put thirteen to twenty presents inside one journey
    /// on a 60 Hz panel. MUTATION: book `now + interval` instead of
    /// `last_present + interval` and the count is whatever the loop can turn.
    #[test]
    fn a_journey_is_paced_to_the_display_it_runs_on() {
        let journey = Duration::from_millis(90);
        assert_eq!(frames_in(&clock_at(60_000), journey), 6);
        assert_eq!(frames_in(&clock_at(144_000), journey), 13);
        assert_eq!(frames_in(&FrameClock::default(), journey), 6);
    }

    /// RED — **the gate refuses everything inside one frame and admits the
    /// instant it is over.**
    #[test]
    fn the_gate_admits_one_turn_per_display_frame() {
        let clock = clock_at(60_000);
        let now = Instant::now();
        assert!(
            clock.is_due(None, now),
            "a window with nothing on its glass is not a frame behind anything"
        );
        assert_eq!(clock.next_frame(None, now), now);
        let presented = Some(now);
        for refused in [0, 1, 8, 16] {
            assert!(
                !clock.is_due(presented, now + Duration::from_millis(refused)),
                "{refused}ms after a present is inside the same display frame"
            );
        }
        let booked = clock.next_frame(presented, now);
        assert_eq!(booked, now + clock.interval());
        assert!(
            clock.is_due(presented, booked),
            "the instant the gate books is an instant the gate admits"
        );
    }

    /// RED — **a deadline is owned by the picture that established its epoch,
    /// not by the turn that asks for it.**
    #[test]
    fn asking_twice_without_a_present_returns_the_same_frame_deadline() {
        let clock = clock_at(60_000);
        let presented = Instant::now();
        let due = presented + clock.interval();
        assert_eq!(clock.deadline(Some(presented)), Some(due));
        assert_eq!(clock.deadline(Some(presented)), Some(due));
        assert_eq!(clock.deadline(None), None);
    }

    /// RED — **a refused turn books the turn that pays it, and a turn that
    /// refuses nothing lets the window sleep.**
    ///
    /// MUTATION: drop the debt and a band that changed shape under a motionless
    /// pointer keeps its marks beside geometry that is not there any more —
    /// the owner's report of 2026-09-14 evening, straight back.
    #[test]
    fn a_refused_frame_is_owed_and_an_idle_turn_owes_nothing() {
        let mut clock = clock_at(60_000);
        let now = Instant::now();
        clock.open(Some(now), now);
        assert!(!clock.owes_a_frame());
        clock.refuse();
        assert!(clock.owes_a_frame());
        clock.refuse();
        assert!(clock.owes_a_frame());
        assert_eq!(clock.take_skipped(), 2);
        assert_eq!(clock.take_skipped(), 0, "the count is reported once");
        clock.open(Some(now), now);
        assert!(
            !clock.owes_a_frame(),
            "the debt belongs to the turn that incurred it"
        );
    }

    /// RED — **a journey under continuous unrelated traffic rides the traffic's
    /// frames, and lands on its own clock** (review 2026-09-18, P1).
    ///
    /// The schedule the review built: a formula begins its ninety milliseconds
    /// in one pane while a neighbouring pane prints every five milliseconds. Every
    /// one of those presents refuses the gate, and each one postpones the debt —
    /// so on the unchanged mechanism the journey was admitted **zero** times in a
    /// thousand turns and could not even land, because the tick that settles it
    /// was behind the same gate.
    ///
    /// That is only a defect if the refused frames draw nothing. They do not:
    /// every frame this window composes carries every live journey
    /// (`Runtime::carry_live_journeys`), so the journey is *sampled by whoever
    /// composes* rather than by the tick. What this pins is the two properties
    /// that makes true — the flood's own frames are enough to draw a smooth
    /// journey, and the landing is never postponed by it — and the third, that
    /// the moment the flood stops the journey gets frames of its own again.
    ///
    /// MUTATIONS: sample the journey from the tick instead of from the compose
    /// and `carried` collapses to one value repeated; gate the landing and
    /// `settled_by` runs past the end of the flood.
    #[test]
    fn a_flood_of_unrelated_frames_takes_the_asking_away_and_gives_it_back() {
        const FLOOD: Duration = Duration::from_millis(5);
        let clock = clock_at(60_000);
        let start = Instant::now();

        let mut presented = None;
        let mut admitted = 0;
        let mut turn = start;
        while turn <= start + Duration::from_millis(1_000) {
            if clock.is_due(presented, turn) {
                admitted += 1;
            }
            // Every one of those turns composed a frame for the neighbour, and
            // every one of them carried the journey: see
            // `Runtime::carry_live_journeys`, which is what makes this refusal
            // an economy rather than a freeze.
            presented = Some(turn);
            turn += FLOOD;
        }
        assert_eq!(
            admitted, 1,
            "a window whose glass is being written every five milliseconds asks \
             for no frames of its own beyond the first"
        );

        // And the asking comes straight back when the printing stops: one
        // interval after the last of the flood's frames, and not one flood
        // period after it.
        let last = presented.expect("the flood presented");
        assert_eq!(clock.next_frame(presented, last), last + clock.interval());
        assert!(!clock.is_due(presented, last + FLOOD));
        assert!(clock.is_due(presented, last + clock.interval()));
    }

    /// RED — **an idle window is never told it is running, and a window whose
    /// last journey has landed stops being told on the very next turn** (review
    /// 2026-09-18 round 2, P1).
    ///
    /// The first version of this flag was set by the *gate*, on the reasoning
    /// that only a running animation would ask. Four of the advancers ask before
    /// they have established that they hold anything at all, so a window with an
    /// empty tip host marked itself running on every turn for ever — and the
    /// carry then rebuilt the whole interface on every ordinary keystroke, which
    /// is worse than the stutter the module was written for. Liveness is now
    /// reported as a fact, recomputed from the journeys' own predicates, and
    /// this pins the two properties that follow from that: nothing is inferred
    /// from asking, and a report retires.
    ///
    /// MUTATION: make the report additive — `self.running.chrome |= …` — and the
    /// flag never comes down again, which is the defect verbatim.
    #[test]
    fn liveness_is_reported_and_retires_rather_than_being_inferred() {
        let mut clock = clock_at(60_000);
        let now = Instant::now();

        // A thousand turns of an idle window: the gate is asked and refused, and
        // nothing about that marks the window as animating.
        let mut turn = now;
        for _ in 0..1_000 {
            clock.open(Some(turn), turn);
            let _ = clock.admits();
            clock.refuse();
            clock.note_running(Lanes::default());
            assert!(
                !clock.running().any(),
                "asking the gate is a question and never evidence"
            );
            turn += Duration::from_millis(1);
        }

        // One journey on one host, and it is the only host carried.
        clock.note_running(Lanes {
            overlay: true,
            ..Lanes::default()
        });
        assert!(clock.running().any());
        assert!(clock.running().overlay);
        assert!(
            !clock.running().chrome,
            "a tip fading owes the overlay and nothing of the strip"
        );

        // And the turn after it lands, the window is quiet again.
        clock.note_running(Lanes::default());
        assert!(!clock.running().any());
    }

    /// RED — **one turn is one answer, and a present inside it does not starve
    /// the animations that come after it.**
    ///
    /// The turn that draws a formula's change of face draws the two marks that
    /// ride its rectangle a few lines later, and the first of those presents
    /// lands before the second is asked. MUTATION: ask `is_due` live at each
    /// advance instead of reading the turn's own answer, and the marks are a
    /// frame behind the block they are supposed to be standing on — every
    /// frame, for the length of the journey.
    #[test]
    fn a_turn_is_one_answer_for_every_animation_in_it() {
        let mut clock = clock_at(60_000);
        let opened = Instant::now();
        let a_frame_ago = opened - clock.interval();
        clock.open(Some(a_frame_ago), opened);
        assert!(clock.admits(), "the glass has had its frame");
        // The first animation of the turn presents; the rest of the turn is
        // still the same picture of the same instant.
        let presented = Some(opened);
        assert!(
            !clock.is_due(presented, opened),
            "the guard the live question would answer with"
        );
        assert!(
            clock.admits(),
            "and the turn's own answer is unchanged by it"
        );
        clock.open(presented, opened);
        assert!(!clock.admits(), "the next turn is inside that frame");
    }
}
