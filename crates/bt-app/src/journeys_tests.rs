//! **Every journey this window runs, asked the one question the carry asks it**
//! (review round 3, 2026-09-18).
//!
//! # Why one table and not thirteen tests
//!
//! Three review rounds found three defects in this work and the last two were
//! the *same* obligation failing twice: **a journey that has finished adds no
//! work to anybody's publish**. Round 2 caught the report being set by the gate,
//! so it never came down. Round 3 caught `Toasts::is_animating` answering for
//! every card whose `leaving` was set, without ever asking how long ago it had
//! been set — and the code that clears `leaving` lives behind the pacing gate,
//! which a neighbouring pane printing every five milliseconds refuses for ever.
//! A toast that finished fading at ninety milliseconds therefore kept the
//! overlay lane alive, and `carry_live_journeys` rebuilt the overlay on every
//! incoming publish: nine hundred and eighty-three of a thousand of them after
//! the journey had ended.
//!
//! Two failures of one obligation is a rule that is not being kept rather than a
//! line that is wrong, so this file states the rule as a property and puts every
//! journey the window has through it at once:
//!
//! > **Whether a journey is moving at `now` is a pure function of its own
//! > clock** — `start <= now < lands_at` — and never a function of a flag that
//! > some other code has to clear. Retirement may live wherever it is convenient
//! > and may be delayed for ever; it must be *irrelevant* to the answer.
//!
//! # The schedule every row is put through
//!
//! The worst one there is, which is the one the review built: a neighbouring
//! pane presents every five milliseconds for five seconds, so [`FrameClock`]
//! refuses **every** turn and no gated advance — no cleanup, no retirement —
//! ever runs. What the flood does run is the carry, because that is the half of
//! the gate that is not gated: a frame composed for somebody else rebuilds the
//! lane of whatever is reported as moving. So each row is driven by the real
//! host, on an injected clock, through the three things a turn can do to it:
//!
//! * [`Journey::moving`] — what `Runtime::running_journeys` reads;
//! * [`Journey::carried`] — what `Runtime::carry_live_journeys` does when it
//!   reads `true`, which for the three surfaces that compare against a *drawn*
//!   receipt is what settles the receipt;
//! * [`Journey::admitted_turn`] — the advance behind the gate, which the flood
//!   never lets run and which the first turn after it does.
//!
//! and four things are asserted of every one of them: it is moving while its own
//! clock says so, it is **not** moving from `lands_at` onward, the flood
//! therefore carries it exactly zero times after its endpoint, and the first
//! admitted turn after the flood leaves the host settled — nothing is lost by
//! the deferral, because the endpoint picture was already delivered by the
//! flood's own frames.
//!
//! # What is not here
//!
//! The video bar's two clocks cannot be driven from outside `video_seat`: a
//! [`crate::video_seat::VideoSeat`] owns a decoder on another thread and
//! `BarSituation` is private to that module, so the same property is pinned
//! there, beside the arithmetic, in `a_resting_bar_is_not_a_moving_one`. The
//! strip's two periodics and this window's waits are the two tests at the foot
//! of this file: neither is a journey, and the rule for both is that they are
//! read fresh and never latched.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::keyboard::ModifiersState;

use super::{
    AnimationCache, AnimationEntry, MAX_ANIMATION_CACHE_BYTES, Motion, PaneMotion, PasteTarget,
    PeekClock, RevealTween, SeatId, TabId, advance_drawn_animations, present_drawn_animations,
};
use crate::pace::{FrameClock, Lanes};
use crate::{
    animation, cardhint, cmdrail, float, formula_tools, keyhint, seats, termscroll, toast, tooltip,
};

/// One journey of this window, driven by the host that really owns it.
///
/// A trait and not a struct of closures because several rows have to *keep* a
/// host and mutate it — the toast that is dismissed, the marks that are left,
/// the float that is closed — and a row that borrowed one would be a row that
/// could not be put in the same list as the others.
trait Journey {
    /// What this is called in the audit.
    fn name(&self) -> &'static str;

    /// The instant its motion begins. For every row but the thumb's this is the
    /// start of the schedule; the thumb's own journey begins nine hundred
    /// milliseconds later, because what comes first is a **wait** and a wait is
    /// not motion.
    fn starts_at(&self) -> Instant;

    /// The instant its motion is over, derived from its own epoch and its own
    /// span — never from a flag, and never from anything a turn has to do.
    fn lands_at(&self) -> Instant;

    /// **Am I moving at `now`** — asked exactly as `Runtime::running_journeys`
    /// asks it.
    fn moving(&self, now: Instant) -> bool;

    /// What the carry does to this host on a frame composed for somebody else.
    ///
    /// A no-op for every journey whose liveness is its own clock, which after
    /// this review is all of them; the hosts that compare against a drawn
    /// receipt settle it here, which is what `refresh_overlay` does for them
    /// inside the carry.
    fn carried(&mut self, _now: Instant) {}

    /// The advance behind the pacing gate: the cleanup the flood never admits.
    fn admitted_turn(&mut self, _now: Instant) {}

    /// Whether the host has reached the state that cleanup leaves it in.
    fn settled(&self, now: Instant) -> bool;
}

// ── the cards ───────────────────────────────────────────────────────────────

/// A notice arriving. Its entrance is [`toast::TOAST_ENTER`] from its birth.
struct ToastEntrance {
    host: toast::ToastHost,
    born: Instant,
}

impl ToastEntrance {
    fn new(born: Instant) -> Self {
        let mut host = toast::ToastHost::default();
        host.raise(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            "git said no",
            None,
            Motion::Full,
            born,
        );
        Self { host, born }
    }
}

impl Journey for ToastEntrance {
    fn name(&self) -> &'static str {
        "toast entrance"
    }
    fn starts_at(&self) -> Instant {
        self.born
    }
    fn lands_at(&self) -> Instant {
        self.born + toast::TOAST_ENTER
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.is_animating(now, Motion::Full)
    }
    fn admitted_turn(&mut self, now: Instant) {
        self.host.advance(now, Motion::Full);
    }
    fn settled(&self, _now: Instant) -> bool {
        !self.host.is_empty()
    }
}

/// **A notice sent away — the round-3 P1.**
///
/// The `×` is pressed on a card that has already arrived, so the only thing
/// running is the ninety-millisecond exit. What retires the card is
/// `ToastHost::advance`, which is behind the gate and which this schedule never
/// admits; before this review `is_animating` answered `leaving.is_some()`, so
/// the card reported itself moving for the whole five seconds although its
/// opacity had been exactly zero since ninety milliseconds.
struct ToastExit {
    host: toast::ToastHost,
    left: Instant,
}

impl ToastExit {
    fn new(left: Instant) -> Self {
        let born = left - toast::TOAST_ENTER;
        let mut host = toast::ToastHost::default();
        let id = host.raise(
            toast::ToastKind::Error,
            toast::ToastAnchor::Window,
            None,
            "git said no",
            None,
            Motion::Full,
            born,
        );
        assert!(
            host.dismiss(id, left, Motion::Full),
            "the card was there to send away"
        );
        Self { host, left }
    }
}

impl Journey for ToastExit {
    fn name(&self) -> &'static str {
        "toast exit"
    }
    fn starts_at(&self) -> Instant {
        self.left
    }
    fn lands_at(&self) -> Instant {
        self.left + toast::TOAST_EXIT
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.is_animating(now, Motion::Full)
    }
    fn admitted_turn(&mut self, now: Instant) {
        self.host.advance(now, Motion::Full);
    }
    fn settled(&self, _now: Instant) -> bool {
        self.host.is_empty()
    }
}

// ── the two hover cards ─────────────────────────────────────────────────────

/// The tip's fade in, and the drawn receipt that used to be half of its
/// liveness.
struct TipFade {
    host: tooltip::TooltipHost,
    shown: Instant,
    drawn: Option<f32>,
}

impl TipFade {
    fn new(shown: Instant) -> Self {
        let mut host = tooltip::TooltipHost::default();
        host.observe(
            Some((tooltip::TooltipAnchorId::Settings, tooltip::TipFace::Chrome)),
            shown - tooltip::TOOLTIP_DELAY,
        );
        assert!(host.activate_if_due(shown), "the tip is due");
        Self {
            host,
            shown,
            drawn: None,
        }
    }

    /// What `Runtime::tooltip_opacity` answers — `None` when there is no tip.
    fn opacity(&self, now: Instant) -> Option<f32> {
        self.host
            .active()
            .map(|_| self.host.opacity(now, Motion::Full))
    }
}

impl Journey for TipFade {
    fn name(&self) -> &'static str {
        "tip fade"
    }
    fn starts_at(&self) -> Instant {
        self.shown
    }
    fn lands_at(&self) -> Instant {
        self.shown + tooltip::TOOLTIP_FADE
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.is_fading(now, Motion::Full)
    }
    fn carried(&mut self, now: Instant) {
        // `refresh_overlay` builds the tip's layer and records what it painted.
        self.drawn = self.opacity(now);
    }
    fn admitted_turn(&mut self, now: Instant) {
        // `advance_tooltip_if_due`: the debt is what makes the landing frame
        // happen, and it is paid by the advance rather than reported as motion.
        if self.drawn != self.opacity(now) {
            self.drawn = self.opacity(now);
        }
    }
    fn settled(&self, now: Instant) -> bool {
        self.drawn == self.opacity(now)
    }
}

/// The hint card's fade in, on the tip's arrangement exactly.
struct KeyHintFade {
    host: keyhint::KeyHintHost,
    shown: Instant,
    drawn: Option<f32>,
}

impl KeyHintFade {
    fn new(shown: Instant) -> Self {
        let mut host = keyhint::KeyHintHost::default();
        host.observe(
            ModifiersState::CONTROL,
            true,
            shown - keyhint::KEY_HINT_DELAY,
        );
        assert!(host.activate_if_due(shown), "the hold is answered");
        Self {
            host,
            shown,
            drawn: None,
        }
    }

    fn opacity(&self, now: Instant) -> Option<f32> {
        self.host
            .active()
            .map(|_| self.host.opacity(now, Motion::Full))
    }
}

impl Journey for KeyHintFade {
    fn name(&self) -> &'static str {
        "key hint fade"
    }
    fn starts_at(&self) -> Instant {
        self.shown
    }
    fn lands_at(&self) -> Instant {
        self.shown + keyhint::KEY_HINT_FADE
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.is_fading(now, Motion::Full)
    }
    fn carried(&mut self, now: Instant) {
        self.drawn = self.opacity(now);
    }
    fn admitted_turn(&mut self, now: Instant) {
        if self.drawn != self.opacity(now) {
            self.drawn = self.opacity(now);
        }
    }
    fn settled(&self, now: Instant) -> bool {
        self.drawn == self.opacity(now)
    }
}

/// The Cards bubble's nudge — a card's rows moved one row down and back, which
/// is the gesture drawn without being done.
struct CardNudge {
    host: cardhint::CardHintHost,
    shown: Instant,
}

impl CardNudge {
    fn new(shown: Instant) -> Self {
        let mut host = cardhint::CardHintHost::default();
        assert_eq!(
            host.observe(true, true, shown),
            cardhint::CardHint::Raised,
            "the bubble goes up on the frame Cards appears"
        );
        Self { host, shown }
    }
}

impl Journey for CardNudge {
    fn name(&self) -> &'static str {
        "card hint nudge"
    }
    fn starts_at(&self) -> Instant {
        self.shown
    }
    fn lands_at(&self) -> Instant {
        self.shown + cardhint::NUDGE_END
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.nudge_moving(now, Motion::Full)
    }
    fn admitted_turn(&mut self, now: Instant) {
        self.host.expire(now);
    }
    fn settled(&self, now: Instant) -> bool {
        !self.moving(now)
    }
}

// ── the rails and the jump ──────────────────────────────────────────────────

/// A command rail warming under the pointer: four clocks aimed in one gesture.
struct CommandRail {
    pointer: cmdrail::RailPointer,
    aimed: Instant,
}

impl CommandRail {
    fn new(aimed: Instant) -> Self {
        let mut pointer = cmdrail::RailPointer::default();
        pointer.aim(4, Some(2), true, aimed, Motion::Full);
        Self { pointer, aimed }
    }
}

impl Journey for CommandRail {
    fn name(&self) -> &'static str {
        "command rail"
    }
    fn starts_at(&self) -> Instant {
        self.aimed
    }
    fn lands_at(&self) -> Instant {
        // The longest of the four: the colour's, which every one of them is
        // clamped by.
        self.aimed + cmdrail::TICK_BACKGROUND_TRANSITION
    }
    fn moving(&self, now: Instant) -> bool {
        self.pointer.is_animating(now, Motion::Full)
    }
    fn settled(&self, now: Instant) -> bool {
        !self.moving(now)
    }
}

/// The band a jump paints across the row it landed on.
struct CommandFlash {
    started: Instant,
}

impl Journey for CommandFlash {
    fn name(&self) -> &'static str {
        "jump flash"
    }
    fn starts_at(&self) -> Instant {
        self.started
    }
    fn lands_at(&self) -> Instant {
        self.started + cmdrail::JUMP_FLASH
    }
    fn moving(&self, now: Instant) -> bool {
        cmdrail::flash_is_running(now.saturating_duration_since(self.started))
    }
    fn settled(&self, now: Instant) -> bool {
        !self.moving(now)
    }
}

// ── the pane's own edges ────────────────────────────────────────────────────

/// **The scroll thumb's fade, and the nine hundred milliseconds in front of it.**
///
/// The one row whose journey does not begin when the schedule does: a thumb
/// whose last reason ended keeps its full strength for [`termscroll::THUMB_REST`]
/// and only then eases out. That rest is a wait, so this row asserts the general
/// property from `starts_at` — which is the fix round 3 recorded, because the
/// window read the thumb's *deadline* and a deadline is `Some` for the whole of
/// the rest as well.
struct ThumbFade {
    rest: Instant,
}

impl Journey for ThumbFade {
    fn name(&self) -> &'static str {
        "terminal thumb fade"
    }
    fn starts_at(&self) -> Instant {
        self.rest + termscroll::THUMB_REST
    }
    fn lands_at(&self) -> Instant {
        self.rest + termscroll::THUMB_REST + termscroll::THUMB_FADE
    }
    fn moving(&self, now: Instant) -> bool {
        termscroll::fade_is_moving(self.rest, now, Motion::Full)
    }
    fn settled(&self, now: Instant) -> bool {
        !self.moving(now)
    }
}

/// The glance card's ninety milliseconds, once the dwell that armed it is over.
struct GlanceCardFade {
    clock: PeekClock,
    shown: Instant,
    drawn: Option<f32>,
}

impl GlanceCardFade {
    fn new(shown: Instant) -> Self {
        Self {
            clock: PeekClock::Shown(shown),
            shown,
            drawn: None,
        }
    }

    fn opacity(&self, now: Instant) -> Option<f32> {
        peek_opacity(self.clock, now)
    }
}

/// The opacity the glance card would be painted at, read exactly as
/// `Runtime::file_peek_opacity` reads it: the card's own epoch, through the
/// tip's rule. `None` is "there is nothing to paint".
fn peek_opacity(clock: PeekClock, now: Instant) -> Option<f32> {
    let shown = clock.shown_at()?;
    Some(tooltip::hover_fade_opacity(
        now.saturating_duration_since(shown),
        Motion::Full,
    ))
}

impl Journey for GlanceCardFade {
    fn name(&self) -> &'static str {
        "glance card fade"
    }
    fn starts_at(&self) -> Instant {
        self.shown
    }
    fn lands_at(&self) -> Instant {
        self.shown + tooltip::TOOLTIP_FADE
    }
    fn moving(&self, now: Instant) -> bool {
        tooltip::hover_fade_owes_frames(now.saturating_duration_since(self.shown), Motion::Full)
    }
    fn carried(&mut self, now: Instant) {
        self.drawn = self.opacity(now);
    }
    fn admitted_turn(&mut self, now: Instant) {
        if self.drawn != self.opacity(now) {
            self.drawn = self.opacity(now);
        }
    }
    fn settled(&self, now: Instant) -> bool {
        self.drawn == self.opacity(now)
    }
}

// ── the floats ──────────────────────────────────────────────────────────────

fn files_tenant() -> float::FloatTenant {
    float::FloatTenant::Files(Box::new(float::FloatFiles {
        files: crate::seats::FilesLeafState {
            root: "C:/x".to_owned(),
            ..crate::seats::FilesLeafState::default()
        },
        width: bt_layout::LogicalPx::px(240),
        ..float::FloatFiles::default()
    }))
}

/// A floating window rising, and the same window dismissed — the two ends of
/// one span, so they are one row with a flag.
struct FloatJourney {
    host: float::FloatHost,
    id: float::FloatId,
    begun: Instant,
    leaving: bool,
}

impl FloatJourney {
    fn opening(begun: Instant) -> Self {
        let mut host = float::FloatHost::default();
        let id = host.open(
            float::FloatMode::Pinned,
            None,
            files_tenant(),
            [100.0, 100.0, 364.0, 400.0],
            None,
            begun,
        );
        Self {
            host,
            id,
            begun,
            leaving: false,
        }
    }

    fn closing(begun: Instant) -> Self {
        let mut opening = Self::opening(begun - float::FLOAT_ANIMATION);
        assert!(
            opening.host.dismiss(opening.id, begun),
            "the window was there to close"
        );
        Self {
            begun,
            leaving: true,
            ..opening
        }
    }
}

impl Journey for FloatJourney {
    fn name(&self) -> &'static str {
        if self.leaving {
            "float exit"
        } else {
            "float entrance"
        }
    }
    fn starts_at(&self) -> Instant {
        self.begun
    }
    fn lands_at(&self) -> Instant {
        self.begun + float::FLOAT_ANIMATION
    }
    fn moving(&self, now: Instant) -> bool {
        self.host.is_animating(now, Motion::Full, 1.0)
    }
    fn admitted_turn(&mut self, now: Instant) {
        self.host.sweep(now, Motion::Full, 1.0);
    }
    fn settled(&self, _now: Instant) -> bool {
        // A dismissed window is swept off the list; one that only opened is
        // still there, and is not moving any more.
        self.host.live(self.id).is_some() != self.leaving
    }
}

// ── the band and its two marks ──────────────────────────────────────────────

fn math_boxes() -> bt_render::MathToolBoxes {
    bt_render::MathToolBoxes {
        anchor: bt_viewport::MathBlockAnchor::History {
            run: None,
            start: bt_transcript::TranscriptId(1),
            end: bt_transcript::TranscriptId(1),
        },
        display: bt_viewport::MathBlockDisplay::Rendered,
        block: [40.0, 10.0, 300.0, 59.0],
        source: [230.0, 25.0, 249.0, 44.0],
        copy: [251.0, 25.0, 270.0, 44.0],
    }
}

/// The marks arriving beside a band, and the marks leaving it when the pointer
/// does — `Runtime::math_tools_owe_frames`, and the drop that `sync_math_tools`
/// performs once the fade is over.
struct Marks {
    follow: Option<formula_tools::FormulaToolFollow>,
    begun: Instant,
    leaving: bool,
}

impl Marks {
    fn arriving(begun: Instant) -> Self {
        Self {
            follow: Some(formula_tools::FormulaToolFollow::arriving(
                &math_boxes(),
                None,
                begun,
            )),
            begun,
            leaving: false,
        }
    }

    fn leaving(begun: Instant) -> Self {
        let mut marks = Self::arriving(begun - tooltip::TOOLTIP_FADE);
        let follow = marks.follow.as_mut().expect("the marks are up");
        assert!(follow.leave(begun, Motion::Full), "the pointer left");
        Self {
            begun,
            leaving: true,
            ..marks
        }
    }
}

impl Journey for Marks {
    fn name(&self) -> &'static str {
        if self.leaving {
            "formula marks exit"
        } else {
            "formula marks arrival"
        }
    }
    fn starts_at(&self) -> Instant {
        self.begun
    }
    fn lands_at(&self) -> Instant {
        self.begun + tooltip::TOOLTIP_FADE
    }
    fn moving(&self, now: Instant) -> bool {
        self.follow
            .as_ref()
            .is_some_and(|follow| follow.owes_frames(now, Motion::Full))
    }
    fn admitted_turn(&mut self, now: Instant) {
        // `sync_math_tools`: a fade that has finished leaving is a surface that
        // is gone, so the follow is dropped whole.
        if self
            .follow
            .as_ref()
            .is_some_and(|follow| follow.gone(now, Motion::Full))
        {
            self.follow = None;
        }
    }
    fn settled(&self, _now: Instant) -> bool {
        self.follow.is_some() != self.leaving
    }
}

/// A block changing face: the one journey whose sample is cached in the session
/// rather than rebuilt, and the one whose landing is owed to the document.
struct BandChangingFace {
    flight: formula_tools::FormulaToggleMotion,
    begun: Instant,
}

impl BandChangingFace {
    fn new(begun: Instant) -> Self {
        Self {
            flight: formula_tools::FormulaToggleMotion::begin(
                PasteTarget {
                    tab: TabId(1),
                    seat: SeatId(1),
                    incarnation: 0,
                },
                bt_viewport::MathBlockAnchor::History {
                    run: None,
                    start: bt_transcript::TranscriptId(1),
                    end: bt_transcript::TranscriptId(1),
                },
                [4_800, 9_600],
                true,
                bt_viewport::MathSourceFace {
                    rows: vec!["$$".to_owned(), "x^2".to_owned(), "$$".to_owned()],
                    width_cells: 3,
                    height_subpixels: 9_600,
                },
                begun,
            ),
            begun,
        }
    }
}

impl Journey for BandChangingFace {
    fn name(&self) -> &'static str {
        "band changing face"
    }
    fn starts_at(&self) -> Instant {
        self.begun
    }
    fn lands_at(&self) -> Instant {
        self.flight.lands_at()
    }
    fn moving(&self, now: Instant) -> bool {
        self.flight.owes_frames(now, Motion::Full)
    }
    fn settled(&self, now: Instant) -> bool {
        self.flight.landed(now, Motion::Full)
    }
}

// ── the strip ───────────────────────────────────────────────────────────────

/// One of the strip's finite tweens — the rail sliding open, which is the shape
/// every other one of them has.
struct StripTween {
    tween: RevealTween,
    begun: Instant,
    span: Duration,
}

impl StripTween {
    fn new(begun: Instant) -> Self {
        let span = Duration::from_millis(180);
        let mut tween = RevealTween::over(span);
        tween.retarget(1.0, begun, Motion::Full);
        Self { tween, begun, span }
    }
}

impl Journey for StripTween {
    fn name(&self) -> &'static str {
        "strip tween"
    }
    fn starts_at(&self) -> Instant {
        self.begun
    }
    fn lands_at(&self) -> Instant {
        self.begun + self.span
    }
    fn moving(&self, now: Instant) -> bool {
        self.tween.sample(now, Motion::Full).1
    }
    fn settled(&self, now: Instant) -> bool {
        !self.moving(now)
    }
}

// ── the schedule ────────────────────────────────────────────────────────────

/// A neighbouring pane's present, on the review's own period.
const FLOOD: Duration = Duration::from_millis(5);
/// How long it keeps printing — long enough that every span above has ended
/// many times over.
const FLOOD_SPAN: Duration = Duration::from_secs(5);

/// Every journey this window has, at one instant.
fn every_journey(start: Instant) -> Vec<Box<dyn Journey>> {
    vec![
        Box::new(ToastEntrance::new(start)),
        Box::new(ToastExit::new(start)),
        Box::new(TipFade::new(start)),
        Box::new(KeyHintFade::new(start)),
        Box::new(CardNudge::new(start)),
        Box::new(CommandRail::new(start)),
        Box::new(CommandFlash { started: start }),
        Box::new(ThumbFade { rest: start }),
        Box::new(GlanceCardFade::new(start)),
        Box::new(FloatJourney::opening(start)),
        Box::new(FloatJourney::closing(start)),
        Box::new(Marks::arriving(start)),
        Box::new(Marks::leaving(start)),
        Box::new(BandChangingFace::new(start)),
        Box::new(StripTween::new(start)),
    ]
}

/// RED — **a journey that has finished adds no work to anybody's publish**, on
/// the worst schedule this window can be given (review round 3, 2026-09-18).
///
/// One row per journey kind the window runs, every one of them a real host on an
/// injected clock, every one of them put through a flood that refuses the pacing
/// gate for five seconds so that no gated advance and no cleanup ever runs.
///
/// MUTATIONS, each of which is a defect this branch has actually had: answer
/// `leaving.is_some()` in `Toasts::is_animating` and the toast-exit row carries
/// nine hundred and eighty-three times after its endpoint; read the thumb's
/// *deadline* instead of its fade and the thumb row reports motion through the
/// nine-hundred-millisecond rest; fold a drawn receipt back into the report and
/// the tip, the hint card and the glance card each carry one turn past their
/// landing.
#[test]
fn every_journey_stops_being_live_on_the_instant_its_own_clock_lands() {
    let start = Instant::now();
    let clock = {
        let mut clock = FrameClock::default();
        assert!(clock.follow(Some(60_000)));
        clock
    };

    // Every row is run to the end and every complaint is kept, because the point
    // of a table is to say which of the thirteen are wrong rather than which one
    // is wrong first: a round that fixed the toast and left the thumb would read
    // as green until the next review.
    let mut complaints: Vec<String> = Vec::new();
    for mut journey in every_journey(start) {
        let name = journey.name();
        let (starts_at, lands_at) = (journey.starts_at(), journey.lands_at());
        assert!(
            starts_at < lands_at && lands_at <= start + FLOOD_SPAN,
            "{name}: a journey's span is a real span and ends inside the flood"
        );

        // Five seconds of somebody else's frames. The gate refuses every one of
        // them, so nothing gated runs; the carry is not gated, so whatever is
        // reported as moving is rebuilt.
        let mut carries = 0_u32;
        let mut carries_after_landing = 0_u32;
        let mut carries_before_landing = 0_u32;
        let mut wrong_at: Option<Duration> = None;
        let mut turn = start;
        let mut presented = Some(start - FLOOD);
        while turn <= start + FLOOD_SPAN {
            let mut clock = clock;
            clock.open(presented, turn);
            assert!(
                !clock.admits(),
                "{name}: the flood is what makes this schedule the worst one"
            );
            let moving = journey.moving(turn);
            clock.note_running(Lanes {
                chrome: false,
                overlay: moving,
            });
            if moving != (starts_at <= turn && turn < lands_at) && wrong_at.is_none() {
                wrong_at = Some(turn.saturating_duration_since(start));
            }
            if clock.running().any() {
                carries += 1;
                if turn >= lands_at {
                    carries_after_landing += 1;
                } else {
                    carries_before_landing += 1;
                }
                journey.carried(turn);
            }
            presented = Some(turn);
            turn += FLOOD;
        }

        if let Some(at) = wrong_at {
            complaints.push(format!(
                "{name}: whether it is moving is not its own clock — the first \
                 disagreement is {at:?} into the schedule"
            ));
        }
        if carries_before_landing == 0 {
            complaints.push(format!(
                "{name}: nothing was ever carried, so this row started no journey"
            ));
        }
        if carries_after_landing > 0 {
            complaints.push(format!(
                "{name}: {carries_after_landing} of {carries} carries came after it had \
                 landed — a finished journey adds no work to anybody's publish"
            ));
        }

        // The printing stops. The very next turn is admitted, and it is the one
        // that performs everything the flood deferred.
        let settled_at = presented.expect("the flood presented") + clock.interval();
        let mut clock = clock;
        clock.open(presented, settled_at);
        assert!(
            clock.admits(),
            "{name}: the asking comes back one frame after the last of the flood"
        );
        journey.admitted_turn(settled_at);
        if !journey.settled(settled_at) {
            complaints.push(format!(
                "{name}: the first admitted turn did not leave the host settled"
            ));
        }
        if journey.moving(settled_at) {
            complaints.push(format!("{name}: and it is still reported as moving"));
        }
    }
    assert!(
        complaints.is_empty(),
        "{} of this window's journeys break the rule:\n  {}",
        complaints.len(),
        complaints.join("\n  ")
    );
}

/// RED — **a periodic is live exactly while its own condition holds, read fresh
/// every turn** (review round 3, 2026-09-18).
///
/// The strip's two are a page's spinner, which runs until the engine says the
/// navigation ended, and a recording, which runs until it is paused. Neither has
/// a `lands_at` and neither may be bent into the journey shape — but neither may
/// be *latched* either, which is the same defect the journeys had: a flag set
/// when the condition became true and cleared by something that can be refused
/// would keep the strip alive after the video stopped.
///
/// MUTATION: remember `playing` in a field instead of asking the seats, and a
/// paused recording keeps the chrome lane alive for ever.
#[test]
fn a_periodic_is_its_condition_and_never_a_latch() {
    let seats = crate::video_seat::VideoSeats::default();
    for _ in 0..1_000 {
        assert!(
            !seats.any_playing(),
            "a window with no recording open is not decoding anything, however \
             many times it is asked"
        );
    }
    // And the reading is the condition itself: the strip asks the seats and the
    // drawn-animation register on every turn, so there is no state in between
    // for a stopped decoder to be remembered in.
    // The walk, asked for as an item of this crate rather than as everything
    // written after the first `fn strip_animation_work(` in one file. See this
    // module's header for the batch this belongs to.
    let fold = method("strip_animation_work");
    assert!(
        fold.contains("self.window.video.any_playing() || self.an_animation_is_running()"),
        "the condition is read where it is used, and never remembered:\n{fold}"
    );
    assert!(
        fold.contains("self.window.web.values().any(|web| web.page().loading)"),
        "and a page's spinner is the engine's answer, asked fresh:\n{fold}"
    );
}

/// RED — **a wait is not motion, and a pending one reports none** (review round
/// 3, 2026-09-18).
///
/// This window is full of clocks that are not tweens: the tip's three hundred
/// and eighty milliseconds before it is shown, a notice's life running out, the
/// glance card's dwell and its grace, the thumb's nine hundred milliseconds of
/// rest. Every one of them wakes the loop — they are deadlines — and not one of
/// them may be reported as moving, because a window that carried a wait would
/// rebuild its whole interface for the length of it at whatever rate a shell can
/// print.
///
/// MUTATION: report a host's `deadline(…).is_some()` as its liveness — which is
/// what the thumbs did before round 3 — and every line here fails.
#[test]
fn a_wait_is_not_a_journey() {
    let start = Instant::now();

    // The tip's settle: armed, waiting, and not moving.
    let mut tip = tooltip::TooltipHost::default();
    tip.observe(
        Some((tooltip::TooltipAnchorId::Settings, tooltip::TipFace::Chrome)),
        start,
    );
    assert!(
        tip.deadline(start, Motion::Full, Duration::from_millis(16))
            .is_some(),
        "a tip settling wakes the loop"
    );
    assert!(
        !tip.is_fading(start, Motion::Full),
        "and nothing about that is motion"
    );

    // The hint card's eight hundred milliseconds, on the same terms.
    let mut hint = keyhint::KeyHintHost::default();
    hint.observe(ModifiersState::CONTROL, true, start);
    assert!(
        hint.deadline(start, Motion::Full, Duration::from_millis(16))
            .is_some()
    );
    assert!(!hint.is_fading(start, Motion::Full));

    // A notice standing out its life: its clock is running and it is perfectly
    // still.
    let mut toasts = toast::ToastHost::default();
    toasts.raise(
        toast::ToastKind::Error,
        toast::ToastAnchor::Window,
        None,
        "git said no",
        None,
        Motion::Full,
        start,
    );
    let standing = start + toast::TOAST_ENTER;
    assert!(
        toasts.deadline(standing, Motion::Full).is_some(),
        "its life runs out at some instant, so the loop is woken for it"
    );
    assert!(
        !toasts.is_animating(standing, Motion::Full),
        "a card that has arrived and not yet been sent away is waiting, not moving"
    );

    // The thumb's rest: nine hundred milliseconds at full strength, with a
    // deadline the whole way and no motion anywhere in it.
    for at in [Duration::ZERO, termscroll::THUMB_REST / 2] {
        assert!(
            termscroll::fade_wait_deadline(start, start + at).is_some(),
            "the rest ends at an instant, so the loop is woken for it"
        );
        assert!(
            !termscroll::fade_is_moving(start, start + at, Motion::Full),
            "and a bar standing at full strength is not fading"
        );
    }

    // **A transition's own delay**, which is the same shape once more and the
    // one the closure review of 2026-09-18 recorded: Q183 holds the rail's
    // labels for sixty milliseconds while the panel widens under them, and for
    // those sixty milliseconds the value is exactly its start. The loop is woken
    // for the instant the fade begins; nothing is moving until it does.
    let delay = Duration::from_millis(60);
    let span = Duration::from_millis(100);
    let mut delayed = RevealTween::over(span);
    delayed.retarget_after(1.0, start, Motion::Full, delay);
    for waiting in [Duration::ZERO, delay / 2] {
        assert!(
            delayed.owes_a_wake(start + waiting, Motion::Full),
            "a transition that has not started is one the loop must be woken for"
        );
        assert!(
            !delayed.sample(start + waiting, Motion::Full).1,
            "but a delay is a wait, and a wait is not motion"
        );
    }
    assert!(
        delayed.sample(start + delay, Motion::Full).1,
        "and on the instant the curve begins, it is moving"
    );
    assert!(
        !delayed.sample(start + delay + span, Motion::Full).1
            && !delayed.owes_a_wake(start + delay + span, Motion::Full),
        "and when it lands it is neither"
    );

    // The glance card's dwell, which is the same shape once more.
    let armed = PeekClock::Settling(start + Duration::from_millis(350));
    assert_eq!(
        peek_opacity(armed, start),
        None,
        "a card that is still settling has nothing on the glass to move"
    );
}

// ── the third thing an advancer does ────────────────────────────────────────

// ── what this module asks the crate ────────────────────────
//
// **P3's deletion commit for this batch** (`docs/plans/bt-app-split-prep.md`
// §6.3, and §6.0 rule 3). The commit before this one read every body twice —
// once as a slice of `main.rs`, once as the body of an item of this crate —
// and asserted the two were the same bytes, and did the same for the counts
// and the prohibitions; this one removes the older of the two, because two
// implementations of one judgement do not vouch for each other
// (`docs/CONVENTIONS.md` §十 rule 4). The pattern is
// `main.rs::pty_drain_budget_tests`', not re-derived here.
//
// **The owner is an argument now and not a guess**: the slice below is the
// first match for a signature prefix anywhere in the file, which is a method of
// whatever `impl` comes first, and it runs on to the next `\n    fn ` — the
// *next* method's doc comment and declaration. Every pin here turns out to mean
// `Runtime`, and this commit establishes that rather than assuming it.
//
// **The three whole-file readings each keep the scope their claim is about, and
// each scope is written down.**
//
// * The taker of the picture debt is counted over the package, filtered to the
//   files a product build compiles: this file is wholly test, so the number is
//   the same one and a second taker written in another file would now be seen.
// * The retired constant is asked for as a *name* over the whole package.
//   `View::Identifiers` is what makes that safe: `pace.rs` explains the
//   retirement in a doc comment, and raw bytes over the package would have read
//   that sentence as a second copy of the thing and inverted the guard.
// * The two prohibitions on a renewable query-time deadline stay narrow — the
//   crate root together with every module under `crate::runtime` for
//   `PaneMotion`, `crate::termscroll` for the thumb — because both spellings are
//   ones other hosts legitimately carry: three types in this package declare
//   that exact `deadline` signature and nine spell `frame: Duration`. A
//   package-wide zero would have inverted both. `PaneMotion` is not a method of
//   either block Step 2a moves, so the root is still where it is declared
//   afterwards — but what this forbids is code nobody has written yet, and after
//   the move it could be written in any `runtime/*.rs`, which a scope naming
//   only the root would never look at. So the negative names the same union the
//   two register gates of `main.rs::arrival_wiring_tests` name, and
//   `in_the_root_or_the_runtime` is what it is called here.

/// **This crate, indexed once per process** — the workspace read, this
/// package's own `src/` declared as the universe and lowered, on the first ask
/// of the process, behind one call (`bt_source::Index::of_package`).
///
/// The package is named here and nowhere else in the module.
fn source_index() -> &'static bt_source::Index {
    bt_source::Index::of_package("bt-app")
}

/// The body of one inherent method of `Runtime`, braces included — the identity
/// of §2.4 rather than a line of `main.rs`.
fn method_body(name: &str) -> &'static str {
    source_index()
        .body_of(&bt_source::ItemQuery::method("Runtime", name))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// One search over the whole package, refusing loudly rather than
/// answering a smaller question.
fn found(needle: bt_source::Needle, view: bt_source::View) -> bt_source::Found {
    source_index()
        .search(&bt_source::Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// How many of these occurrences stand in a file a product build compiles.
///
/// File-grained, and deliberately so: §2.3 computes product reachability
/// per *declaration path to a file*, and an inline `#[cfg(test)] mod`
/// inside a product file is not a file.
fn in_product(found: &bt_source::Found) -> usize {
    found
        .occurrences()
        .iter()
        .filter(|occurrence| {
            source_index()
                .file_at(occurrence.span.start())
                .is_some_and(bt_source::FileRecord::permits_product)
        })
        .count()
}

/// The package's product count of one raw needle — the view `include_str!`
/// handed this module.
fn in_product_raw(needle: bt_source::Needle) -> usize {
    in_product(&found(needle, bt_source::View::Raw))
}

/// One search over a named scope — a Rust path, never a file.
fn found_in(
    needle: bt_source::Needle,
    view: bt_source::View,
    scope: bt_source::Scope,
) -> bt_source::Found {
    source_index()
        .search(&bt_source::Search::new(needle, view).in_scope(scope))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The body of one method of `Runtime` — the identity of §2.4 rather than a
/// line of `main.rs`, so a method that moves between files is the same method.
fn method(name: &str) -> &'static str {
    method_body(name)
}

/// **Whether this universe holds a module of that path.**
///
/// `Index::modules()` is the record a named scope resolves against, so this asks
/// the same question `Scope::Modules` asks one moment before it is asked — which
/// is what tells "the module is not there yet" apart from "the module is there
/// and holds nothing", two answers a negative reads as the same green.
fn declares(module_path: &str) -> bool {
    source_index()
        .modules()
        .iter()
        .any(|module| module.module_paths().iter().any(|path| path == module_path))
}

/// One playback of `frames` frames, each standing a tenth of a second — the
/// shape of every `.gif` this window draws, with no decoder behind it.
fn a_playback_of(frames: usize, started: Instant) -> AnimationEntry {
    let ring = (0..frames)
        .map(|index| animation::AnimationFrame {
            bgra: Arc::from(vec![index as u8; 4]),
            delay: Duration::from_millis(100),
        })
        .collect();
    AnimationEntry::Ready {
        serial: 1,
        animation: Box::new(animation::Animation::of(ring, 1, 1, started)),
    }
}

/// Which frame of one cached animation is standing.
fn standing_frame(cache: &AnimationCache, key: &str) -> u64 {
    match cache.get(key) {
        Some(AnimationEntry::Ready { animation, .. }) => animation.frame_index(),
        _ => panic!("{key} is not a playing animation"),
    }
}

/// A list long enough to scroll, with the pointer resting in its trailing edge
/// band — where a hand holding a tab at the end of a full strip is.
fn a_run_being_scrolled() -> (seats::TabRun, (f64, f64)) {
    let run = seats::TabRun {
        axis: bt_layout::Axis::Row,
        slots: (0..12)
            .map(|index| {
                let left = index as f32 * 100.0;
                [left, 0.0, left + 100.0, 30.0]
            })
            .collect(),
        viewport: [0.0, 600.0],
        band: [0.0, 0.0, 600.0, 30.0],
        pane_offers: seats::PaneOffers::BOTH,
        max_scroll: 600.0,
    };
    (run, (596.0, 15.0))
}

/// RED — **a service is not a picture, and the pacer has no business gating
/// one** (closure review O4, 2026-09-18).
///
/// Every gated advancer in this window does up to three different things, and
/// only one of them belongs to the frame gate.
///
/// 1. **Service.** Bring time-driven state up to `now`: pump a decoder, walk a
///    decoded animation's clock to the frame that is due, integrate the distance
///    an auto-scroll has travelled. Cheap, idempotent in `now`, and **never**
///    gated — it runs on every turn and at the head of every compose, so
///    whatever frame goes out, for whatever reason, is a picture of now.
/// 2. **Sample and draw.** A pure function of state and `now`, done at compose.
/// 3. **Ask for a frame of its own.** The only gated thing there is.
///
/// This branch had put a service on the wrong side of that line:
/// `advance_strip_animation` returned at the display gate above its one call to
/// `video.pump` and `advance_animations`, so a `.gif` or a recording playing
/// beside a pane printing a build log every five milliseconds stood on whatever
/// frame it was holding — the carry rebuilt the chrome and the overlay around
/// it, the periodic correctly reported it live, and nobody ever moved its clock.
/// Drag auto-scroll had the same shape: its only integration stood below the
/// same gate.
///
/// MUTATIONS: put either service back under `animation_frame_is_due` and block ②
/// is what the window does — frame 0 after five seconds instead of frame 50 —
/// and block ④ names the method that did it.
#[test]
fn an_elapsed_time_service_is_never_gated_and_the_flood_shows_its_state() {
    let start = Instant::now();
    let clock = {
        let mut clock = FrameClock::default();
        assert!(clock.follow(Some(60_000)));
        clock
    };
    let drawn: BTreeMap<String, u64> = [("on the glass".to_owned(), 1)].into();

    // ── ① the flood, and a decoded animation serviced on every turn ──────────
    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    cache.insert("on the glass".to_owned(), a_playback_of(64, start));
    present_drawn_animations(&mut cache, &BTreeMap::new(), &drawn, start);

    let mut refusals = 0_u32;
    let mut turn = start;
    let mut presented = Some(start - FLOOD);
    while turn <= start + FLOOD_SPAN {
        let mut gate = clock;
        gate.open(presented, turn);
        assert!(!gate.admits(), "the flood refuses every turn");
        refusals += 1;
        // The service, ungated, at the head of the turn — and then the compose,
        // which is what the flood's own present draws.
        advance_drawn_animations(&mut cache, &drawn, turn);
        let shown = standing_frame(&cache, "on the glass");
        let due = turn.saturating_duration_since(start).as_millis() as u64 / 100;
        assert_eq!(
            shown,
            due,
            "the frame the flood presents is the frame the clock says, {:?} in",
            turn.saturating_duration_since(start)
        );
        presented = Some(turn);
        turn += FLOOD;
    }
    assert_eq!(refusals, 1_001, "five seconds of somebody else's frames");
    assert_eq!(
        standing_frame(&cache, "on the glass"),
        50,
        "fifty tenths of a second is fifty frames"
    );

    // ── ② the same host on the gated schedule, which is the defect ───────────
    let mut gated = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    gated.insert("on the glass".to_owned(), a_playback_of(64, start));
    present_drawn_animations(&mut gated, &BTreeMap::new(), &drawn, start);
    let mut turn = start;
    let mut presented = Some(start - FLOOD);
    while turn <= start + FLOOD_SPAN {
        let mut gate = clock;
        gate.open(presented, turn);
        if gate.admits() {
            advance_drawn_animations(&mut gated, &drawn, turn);
        }
        presented = Some(turn);
        turn += FLOOD;
    }
    assert_eq!(
        standing_frame(&gated, "on the glass"),
        0,
        "a clock behind the frame gate does not run at all under a flood, which \
         is why a service may never stand there"
    );

    // ── ③ and it costs nothing when nothing is live ──────────────────────────
    let mut idle = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    idle.insert("in another tab".to_owned(), a_playback_of(64, start));
    let nothing_drawn = BTreeMap::new();
    let mut turn = start;
    while turn <= start + FLOOD_SPAN {
        assert!(
            !advance_drawn_animations(&mut idle, &nothing_drawn, turn),
            "a window drawing no animation has no clock to run"
        );
        turn += FLOOD;
    }
    assert_eq!(standing_frame(&idle, "in another tab"), 0);

    // ── ④ and both services are wired above the gate ─────────────────────────
    let pictures = method("service_pictures");
    assert!(
        pictures.contains("self.window.video.pump(now) | self.advance_animations(now)")
            && !pictures.contains("animation_frame_is_due"),
        "the decoders and the decoded animations are serviced, ungated:\n{pictures}"
    );
    assert!(
        pictures.contains("if self.window.video.is_empty() {") || {
            let sweep = method("sweep_video_seats");
            sweep.contains("if self.window.video.is_empty() {")
        },
        "a service that runs on every turn is free when nothing is live"
    );
    let turning = method("turn");
    let serviced = turning
        .find("self.service_pictures(now)")
        .expect("the turn services the pictures");
    let strip = turning
        .find("self.advance_strip_animation(now)")
        .expect("and then the tick asks for its frame");
    assert!(
        serviced < strip,
        "the service runs before the tick that can be refused:\n{turning}"
    );
    let carry = method("carry_live_journeys");
    assert!(
        carry
            .find("self.service_pictures(now);")
            .unwrap_or(usize::MAX)
            < carry.find("let running =").unwrap_or(0),
        "a frame composed for anybody shows the pictures as of now:\n{carry}"
    );
    // And the frame a refused turn could not ask for is still owed afterwards:
    // the service writes the debt down and the first admitted tick takes it.
    assert!(
        pictures.contains("self.window.pictures_owe_a_frame = true;"),
        "a frame that arrived during a refused turn is still owed:\n{pictures}"
    );
    let tick = method("advance_strip_animation");
    assert!(
        tick.contains("std::mem::take(&mut self.window.pictures_owe_a_frame)")
            && !tick.contains("self.window.video.pump(now)"),
        "and the tick pays that debt rather than doing the service itself:\n{tick}"
    );
    let autoscroll = method("service_drag_autoscroll");
    assert!(
        !autoscroll.contains("animation_frame_is_due"),
        "the auto-scroll integrates true elapsed time and must not be refused:\n{autoscroll}"
    );

    // ── ⑤ and every other clock that changes state stands above the gate ─────
    //
    // The same distinction once the two measured cases are closed: a wait
    // maturing, a life running out and a finished thing being swept are *state*,
    // and behind the gate a printing pane keeps a tip from ever appearing, a
    // notice from ever leaving and a flyout from ever opening. What stays paced
    // is the fade each of them starts.
    for (name, clock) in [
        (
            "advance_toasts",
            "self.window.toasts.advance(now, self.app.motion)",
        ),
        (
            "advance_tooltip_if_due",
            "self.window.tooltip.activate_if_due(now)",
        ),
        (
            "advance_key_hint_if_due",
            "self.window.key_hint.activate_if_due(now)",
        ),
        ("advance_card_hint", "self.window.card_hint.expire(now)"),
        ("advance_file_peek", "self.switch_file_peek(now)"),
        ("advance_float", "self.window.float.take_due(now)"),
    ] {
        let body = method(name);
        let state = body
            .find(clock)
            .unwrap_or_else(|| panic!("`{clock}` is this advancer's state clock:\n{body}"));
        let gate = body
            .find("!self.animation_frame_is_due() {")
            .unwrap_or_else(|| panic!("this advancer still asks for a frame:\n{body}"));
        assert!(
            state < gate,
            "`{clock}` stands behind the frame gate, so a printing pane can \
             postpone it for ever:\n{body}"
        );
    }
}

/// RED — **a debt books its own wake** (closure review 2, 2026-09-18, P1).
///
/// The split that closed O4 made the turn that *finds* a picture and the turn
/// that may *present* it two different turns, and a bit written down between
/// them is a debt. A debt that books no wake is a debt nothing comes back for:
/// a decoder's last frame, or the one a pause leaves standing, arrives on an
/// otherwise idle turn, is recorded, and then waits for something unrelated to
/// happen to the window — which on an idle window is nothing at all.
///
/// The review's counterexample is a 144 Hz display, and it is also why this
/// window may not keep two rates. The strip guarded its tick with a flat sixteen
/// milliseconds beside the display's 6.94, so seven milliseconds after a present
/// the pacer's door admitted and the strip's refused — and because the strip
/// returns at its own guard *above* the pacer's, no refusal was recorded either,
/// so not even the frame clock's own debt was booked. Both doors now read one
/// interval, and the picture's debt is folded into the strip's deadline.
///
/// MUTATIONS: drop `pictures_owe` from the fold and nothing books the wake; give
/// the strip its own sixteen milliseconds back and the two doors disagree at
/// 144 Hz, which is the first block below.
#[test]
fn a_picture_that_arrives_between_frames_books_its_own_wake() {
    let hz_144 = {
        let mut clock = FrameClock::default();
        assert!(clock.follow(Some(144_000)));
        clock
    };
    let interval = hz_144.interval();
    let start = Instant::now();

    // ── ① one window, one rate: both doors answer the same at every instant ──
    for micros in [0, 1_000, 3_000, 6_943, 6_944, 7_000, 16_000] {
        let at = start + Duration::from_micros(micros);
        assert_eq!(
            hz_144.is_due(Some(start), at),
            crate::strip_animation_tick_is_due(Some(start), at, interval),
            "{micros}µs after a present the display and the strip disagree, which \
             is how a picture that had arrived found every door shut"
        );
    }

    // ── ② a picture arrives on an idle turn inside the frame ────────────────
    // The last tick and the last present were both at t = 0; the decoder's
    // final picture lands at t = 3 ms, on a turn that publishes nothing.
    let arrived = start + Duration::from_millis(3);
    assert!(
        !crate::strip_animation_tick_is_due(Some(start), arrived, interval),
        "the strip is not due, so it returns above the pacer and records no refusal"
    );
    assert!(
        !hz_144.is_due(Some(start), arrived),
        "and the display would have refused it anyway"
    );

    // So nothing but the debt can bring the loop round. What it books is the
    // strip's next eligible tick — `Runtime::strip_animation_next_tick` — and
    // the instant it books is an instant both doors admit.
    let booked = (start + interval).max(arrived);
    assert!(
        booked > arrived && booked <= arrived + interval,
        "a debt is paid within one frame of being incurred"
    );
    assert!(
        hz_144.is_due(Some(start), booked)
            && crate::strip_animation_tick_is_due(Some(start), booked, interval),
        "and the tick that pays it is a tick both doors let through"
    );

    // ── ③ and the wiring says so ────────────────────────────────────────────
    let work = method("strip_animation_work");
    let deadline = work
        .find("deadline: [")
        .expect("the strip answers when it next needs waking");
    let moving = work
        .find("moving: strip_moving")
        .expect("and what is mid-flight in it");
    let owed = work
        .find("|| pictures_owe)")
        .expect("a picture that is owed books the strip's next tick");
    assert!(
        deadline < owed && owed < moving,
        "the debt is a deadline and never a liveness — nothing is moving, one \
         frame is owed:\n{work}"
    );
    assert!(
        work.contains("self.window.frame_clock.interval()"),
        "and the strip asks for its next frame on the window's one rate:\n{work}"
    );
    let tick = method("advance_strip_animation");
    let guard = tick
        .find("strip_animation_tick_is_due(")
        .expect("the strip still enforces a rate");
    assert!(
        tick[guard..].contains("self.window.frame_clock.interval(),"),
        "and it enforces the window's, not a second one of its own:\n{tick}"
    );

    // ── ④ the debt has exactly one taker, and the fold is not it ────────────
    //
    // A seat removed while the window is idle is the case this matters most in:
    // the sweep says the membership moved, the service hands the shorter list
    // over, and what is owed is the frame that shows the rectangle empty. If the
    // fold *took* the bit instead of reading it, that frame would be booked and
    // then forgotten in the same breath; if two passes took it, whichever ran
    // first would pay a debt the other had already booked a wake for.
    assert!(
        work.contains("let pictures_owe = self.window.pictures_owe_a_frame;"),
        "the fold reads the debt and leaves it where it is:\n{work}"
    );
    // The whole statement rather than the call, so that `main.rs`'s own source
    // pin on the same line is not counted as a second taker.
    let taken = "let pictures_owe = std::mem::take(&mut self.window.pictures_owe_a_frame);";
    assert_eq!(
        in_product_raw(bt_source::Needle::new(bt_source::Pattern::text(taken))),
        1,
        "one taker, and it is the tick that presents"
    );
    assert!(
        tick.contains(taken),
        "and that taker is the strip's own tick:\n{tick}"
    );
    // A removed seat is a membership change, so the service hands the shorter
    // list over and the debt that books this wake is set by the same pass —
    // `crate::pictures_need_handing_over` is what says so, and is counted in
    // `a_service_that_finds_nothing_changed_hands_nothing_over`.
    assert!(crate::pictures_need_handing_over(false, true, false, false));

    // ── ⑤ one window, one rate, with no exception left ─────────────────────
    //
    // Every surface in this window that asks for "the next frame" asks for the
    // *display's*. The sixteen milliseconds survives in exactly one place — the
    // interval `FrameClock` is born with when the platform will not say — and
    // nowhere at all on the road a frame is asked for: two rates in one window
    // is a door that admits and a door that refuses at the same instant.
    assert_eq!(
        found_in(
            bt_source::Needle::new(bt_source::Pattern::identifier("STRIP_ANIMATION_FRAME")),
            bt_source::View::Identifiers,
            bt_source::Scope::Everything,
        )
        .len(),
        0,
        "the strip's own sixteen milliseconds is retired, not merely unused"
    );
    for name in [
        "strip_animation_work",
        "drag_autoscroll_deadline",
        "next_animation_frame",
        "next_animation_deadline",
    ] {
        let body = method(name);
        assert!(
            body.contains("frame_clock"),
            "this has to ask the clock that reads the display:\n{body}"
        );
    }
    // PaneMotion cannot see the window, so it reports only whether a flight is
    // live. The strip fold assigns that flight the one absolute tick derived
    // above from this window's frame clock; the retired helper must not grow
    // back and manufacture a renewable `now + frame` appointment of its own.
    assert!(
        work.contains("let pane_moving = self.window.pane_motion.is_animating(now, motion);")
            && work.contains(
                "let next_tick = self.strip_animation_next_tick(self.window.frame_clock.interval());"
            )
            && work.contains("(bar_moving || pane_moving).then_some(next_tick).flatten()"),
        "a pane in flight is assigned the strip's absolute window-clock tick:\n{work}"
    );
    // The union, and the second member's state, exactly as
    // `main.rs::arrival_wiring_tests::the_root_and_the_runtime` has it: the
    // member is left out rather than written conditionally, because
    // `crate::runtime` does not exist on this tree and a `ModuleSpec` that names
    // no module is `QueryFailure::EmptyScope` on its own name. The assertion is
    // what makes the flip a red test rather than a memory.
    assert!(
        !declares("crate::runtime"),
        "`crate::runtime` is declared, so the relocation landed and this negative \
         has to become `Scope::Modules(vec![ModuleSpec::exact(\"crate\"), \
         ModuleSpec::tree(\"crate::runtime\")])` — the forbidden code can now be \
         written in a file the root does not reach"
    );
    let in_the_root_or_the_runtime = |text: &str| {
        found_in(
            bt_source::Needle::new(bt_source::Pattern::text(text)),
            bt_source::View::Raw,
            bt_source::Scope::Modules(vec![bt_source::ModuleSpec::exact("crate")]),
        )
        .is_empty()
    };
    assert!(
        in_the_root_or_the_runtime(
            "fn deadline(&self, now: Instant, motion: Motion, frame: Duration)"
        ) && in_the_root_or_the_runtime("self.is_animating(now, motion).then(|| now + frame)"),
        "PaneMotion must not own a renewable query-time deadline"
    );

    // The terminal thumb has the same ownership split. `termscroll` reports
    // the absolute end of its full-strength rest and whether the fade is live;
    // the window assigns a live fade its shared absolute frame appointment.
    let thumbs = method("terminal_thumb_work");
    assert!(
        thumbs.contains("if termscroll::fade_is_moving(rest, now, motion) {")
            && thumbs.contains("self.next_animation_deadline()")
            && thumbs.contains("termscroll::fade_wait_deadline(rest, now)"),
        "a thumb fade rides the window clock while its owner retains the rest deadline:\n{thumbs}"
    );
    let in_termscroll = |text: &str| {
        found_in(
            bt_source::Needle::new(bt_source::Pattern::text(text)),
            bt_source::View::Raw,
            bt_source::Scope::Module("crate::termscroll".to_owned()),
        )
        .is_empty()
    };
    assert!(
        in_termscroll("frame: Duration") && in_termscroll("Some(now + frame)"),
        "termscroll must not manufacture a renewable query-time frame"
    );
}

/// RED — **a service that finds nothing changed allocates nothing** (closure
/// review 2, 2026-09-18, P2).
///
/// A service runs on every turn *and* again at the head of every compose, which
/// is what makes it correct — and what makes an unconditional rebuild inside one
/// the most expensive idle thing in the window. A paused recording standing in a
/// still pane was having the whole layer list reconstructed twice per
/// five-millisecond present of a neighbouring shell: two thousand rebuilds and
/// some eight thousand allocations over a thousand presents, for the same pixels
/// in the same rectangle, with nothing in this window moving at all. Ordinary
/// output must pay nothing, and that is the charge this whole branch exists to
/// answer.
///
/// MUTATION: hand the layers over unconditionally — which is what the split's
/// first version did — and the first count below reads 1,000 instead of 0 and
/// the second 1,000 instead of 50.
#[test]
fn a_service_that_finds_nothing_changed_hands_nothing_over() {
    // ── ① a paused seat: nothing new, nothing gone, nothing moving ──────────
    let mut rebuilds = 0_u32;
    for _ in 0..1_000 {
        if crate::pictures_need_handing_over(false, false, false, false) {
            rebuilds += 1;
        }
    }
    assert_eq!(
        rebuilds, 0,
        "a picture that has not changed is a picture the renderer is already \
         holding"
    );

    // ── ② a playing animation: one rebuild per new frame, not per service ───
    // The real host, the real clock: a hundred-millisecond ring serviced every
    // five milliseconds for five seconds.
    let start = Instant::now();
    let drawn: BTreeMap<String, u64> = [("on the glass".to_owned(), 1)].into();
    let mut cache = AnimationCache::with_budget(MAX_ANIMATION_CACHE_BYTES);
    cache.insert("on the glass".to_owned(), a_playback_of(64, start));
    present_drawn_animations(&mut cache, &BTreeMap::new(), &drawn, start);
    let mut services = 0_u32;
    let mut rebuilds = 0_u32;
    let mut turn = start;
    while turn <= start + FLOOD_SPAN {
        let frames_arrived = advance_drawn_animations(&mut cache, &drawn, turn);
        services += 1;
        if crate::pictures_need_handing_over(frames_arrived, false, false, false) {
            rebuilds += 1;
        }
        turn += FLOOD;
    }
    assert_eq!(services, 1_001);
    assert_eq!(
        rebuilds, 50,
        "fifty new frames in five seconds, and {services} services to find them"
    );

    // ── ③ the two other reasons, and there is no fourth ─────────────────────
    assert!(
        crate::pictures_need_handing_over(false, true, false, false),
        "a seat opened, closed or faulted changes the list itself"
    );
    assert!(
        crate::pictures_need_handing_over(false, false, true, false),
        "and a pane in FLIP or a float on its way in moves the rectangle under it"
    );
    assert!(
        crate::pictures_need_handing_over(false, false, false, true),
        "and the frame a travelling box has just stopped on is the one that \
         carries where it stopped"
    );
    let service = method("service_pictures");
    assert!(
        service.contains("if !pictures_need_handing_over(")
            && service.contains("boxes_are_moving,")
            && service.contains("boxes_were_moving,"),
        "the service asks that one question, with both halves of the box fact, \
         before it builds anything:\n{service}"
    );
    assert!(
        service.contains(
            "std::mem::replace(&mut self.window.picture_boxes_were_moving, boxes_are_moving)"
        ),
        "and it remembers this decision's answer for the next one, so a landed \
         tween owes one hand-over and never two:\n{service}"
    );
    let asked = service
        .find("pictures_need_handing_over(")
        .expect("the service asks");
    let built = service
        .find("self.refresh_video_layers()")
        .expect("and only then hands the layers over");
    assert!(asked < built, "it is asked first, or it is not a guard");

    // ── ④ and the per-turn float passes are free with nothing open ──────────
    let host = float::FloatHost::default();
    assert!(host.is_empty() && host.drawn().count() == 0);
    for (pass, name) in [
        ("resize", "resize_floats_to_content"),
        ("directories", "ask_float_directories"),
        ("git", "ask_git_for_floats"),
    ] {
        let body = method(name);
        let empty = body
            .find("self.window.float.is_empty()")
            .expect("every per-turn float pass asks whether there is a float at all");
        assert!(
            empty < body.find("Vec::new()").unwrap_or(usize::MAX)
                && empty < body.find("live_windows").unwrap_or(usize::MAX),
            "the {pass} pass walks or allocates before asking:\n{body}"
        );
    }
}

/// RED — **the frame a travelling box has just stopped on is handed over, and
/// it is the last one that is** (closure review 3, 2026-09-18).
///
/// The service's third reason used to ask "is a box moving *now*", and the one
/// frame a moving box most needs handed over is the one where it has just
/// stopped. The review's geometry: a paused picture in a pane that FLIPs four
/// hundred pixels into a 500×500 box. The last hand-over lands one frame short
/// of the end — the review measured a clip of `x = 49` where the pane comes to
/// rest at `0` — and then the tween reports itself finished, every reason goes
/// false together, and the renderer goes on holding that forty-nine-pixel clip.
/// The tick pays the *pane's* own debt and retires the tween; nothing on the
/// chrome, overlay or pane-draw road hands the video layers over.
///
/// So the fact is "a box has moved since the last decision", which is true on
/// exactly one more service than "is moving" is. Both box movers are driven
/// here, on real hosts: a pane's FLIP and a float's entrance and exit.
///
/// MUTATIONS: drop `boxes_were_moving` from the predicate and the landing
/// hand-over disappears from all three; make it sticky instead of a
/// one-decision memory and the thousand idle services after the landing hand
/// over a thousand times.
#[test]
fn the_landing_frame_of_a_travelling_box_is_handed_over_exactly_once() {
    let start = Instant::now();
    let seat = SeatId(1);
    // Four hundred pixels of travel into the box the pane comes to rest in.
    let before = [(seat, [400.0, 0.0, 900.0, 500.0])];
    let after = [(seat, [0.0, 0.0, 500.0, 500.0])];
    let mut motion = PaneMotion::default();
    motion.begin(&before, &after, start, Motion::Full);

    // The service loop: every five milliseconds, the decision the product
    // makes, and — when it says yes — the geometry the renderer would be handed.
    let mut were_moving = false;
    let mut handed: Vec<(Duration, [f32; 4])> = Vec::new();
    let service = |motion: &PaneMotion, were: &mut bool, handed: &mut Vec<_>, now: Instant| {
        let moving = motion.is_animating(now, Motion::Full);
        let were_before = std::mem::replace(were, moving);
        if crate::pictures_need_handing_over(false, false, moving, were_before) {
            let shape = motion.snapshot(&after, now, Motion::Full)[0].1;
            handed.push((now.saturating_duration_since(start), shape));
        }
    };

    let mut now = start;
    while now <= start + crate::PANE_FLIP + Duration::from_millis(100) {
        service(&motion, &mut were_moving, &mut handed, now);
        now += FLOOD;
    }

    // It was handed over while it travelled...
    assert!(
        handed.len() > 2,
        "a four-hundred-pixel flight is drawn in more than two frames"
    );
    let (at, landed) = *handed.last().expect("the flight was handed over");
    // ...and the last hand-over is the landing, carrying where it lands.
    assert!(
        at >= crate::PANE_FLIP && at < crate::PANE_FLIP + FLOOD,
        "the final hand-over is the first service at or after the landing, not \
         one frame short of it: {at:?}"
    );
    assert_eq!(
        landed, after[0].1,
        "and what it carries is where the pane came to rest"
    );
    let (_, one_short) = handed[handed.len() - 2];
    assert!(
        one_short != after[0].1,
        "the hand-over before it is the stale one the review measured: {one_short:?}"
    );
    let landings = handed
        .iter()
        .filter(|(at, _)| *at >= crate::PANE_FLIP)
        .count();
    assert_eq!(landings, 1, "exactly one, and never a second");

    // And a thousand services afterwards hand over nothing at all: an ended
    // tween does not come back to life.
    let quiet = handed.len();
    for _ in 0..1_000 {
        service(&motion, &mut were_moving, &mut handed, now);
        now += FLOOD;
    }
    assert_eq!(handed.len(), quiet, "an idle window hands over nothing");

    // ── the other box mover, on its own two ends ────────────────────────────
    for leaving in [false, true] {
        let begun = Instant::now();
        let mut host = float::FloatHost::default();
        let id = host.open(
            float::FloatMode::Pinned,
            None,
            files_tenant(),
            [100.0, 100.0, 364.0, 400.0],
            None,
            if leaving {
                begun - float::FLOAT_ANIMATION
            } else {
                begun
            },
        );
        if leaving {
            assert!(host.dismiss(id, begun), "the window was there to close");
        }
        let mut were_moving = false;
        let mut overs = 0_u32;
        let mut landing = 0_u32;
        let mut now = begun;
        while now <= begun + float::FLOAT_ANIMATION + Duration::from_millis(100) {
            let moving = host.is_animating(now, Motion::Full, 1.0);
            let were_before = std::mem::replace(&mut were_moving, moving);
            if crate::pictures_need_handing_over(false, false, moving, were_before) {
                overs += 1;
                if now >= begun + float::FLOAT_ANIMATION {
                    landing += 1;
                }
            }
            now += FLOOD;
        }
        assert!(overs > 2, "a float's fade is drawn in more than two frames");
        assert_eq!(
            landing, 1,
            "and the frame it settles on is handed over once (leaving={leaving})"
        );
    }
}

/// RED — **an integrator does not care how often it is called, only that it is**
/// (closure review O4, 2026-09-18).
///
/// The auto-scroll is the one clock in this window that is not sampled but
/// *integrated*: it multiplies a speed by the time since it last ran and moves
/// the list by that much. A turn it misses therefore costs it no distance, which
/// is exactly why gating it is not a saving but a stop — a schedule that refuses
/// it for five seconds does not slow the list down, it holds it still, and there
/// is no "afterwards" to pay the distance back in while the flood lasts.
///
/// MUTATION: integrate a fixed step per call instead of `now - last` and the two
/// schedules below disagree by the ratio of their rates.
#[test]
fn the_auto_scroll_integrates_the_same_distance_on_any_schedule() {
    let (run, pointer) = a_run_being_scrolled();
    let start = Instant::now();
    let span = Duration::from_millis(500);

    // Integrate the list from a standing start, stepping every `period`.
    let travelled = |period: Duration| {
        let mut scroll = 0.0_f32;
        let mut last = start;
        let mut calls = 0_u32;
        let mut now = start + period;
        while now <= start + span {
            if let Some(moved) = seats::autoscroll_step(
                &run,
                scroll,
                pointer,
                1.0,
                Motion::Full,
                now.saturating_duration_since(last),
            ) {
                scroll = moved;
                calls += 1;
            }
            last = now;
            now += period;
        }
        (scroll, calls)
    };

    // Every turn of a five-millisecond flood, against one admitted turn per
    // display frame: two schedules over the same half-second.
    let (flooded, flood_calls) = travelled(FLOOD);
    let (paced, paced_calls) = travelled(Duration::from_millis(16));
    assert!(flood_calls > paced_calls && paced_calls > 0);
    assert!(
        flooded > 0.0,
        "the hand is in the band, so the list travels"
    );
    let step = flooded / flood_calls as f32;
    assert!(
        (flooded - paced).abs() <= step.max(1.0),
        "one integrator, two schedules, one distance: {flooded} against {paced}"
    );

    // And a hand nowhere near an edge integrates nothing at all, on any
    // schedule: the service is free when nothing is live.
    assert_eq!(
        seats::autoscroll_step(&run, 0.0, (300.0, 15.0), 1.0, Motion::Full, span),
        None,
        "a pointer in the middle of a list is not asking it to travel"
    );
}
