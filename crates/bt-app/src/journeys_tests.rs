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

use std::time::{Duration, Instant};

use winit::keyboard::ModifiersState;

use super::{Motion, PasteTarget, PeekClock, RevealTween, SeatId, TabId};
use crate::pace::{FrameClock, Lanes};
use crate::{cardhint, cmdrail, float, formula_tools, keyhint, termscroll, toast, tooltip};

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
                vec!["$$".to_owned(), "x^2".to_owned(), "$$".to_owned()],
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
    let source: &str = include_str!("main.rs");
    let fold = source
        .split("fn strip_animation_work(")
        .nth(1)
        .expect("the strip answers its two questions from one walk");
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
            termscroll::fade_deadline(start, start + at, Motion::Full).is_some(),
            "the rest ends at an instant, so the loop is woken for it"
        );
        assert!(
            !termscroll::fade_is_moving(start, start + at, Motion::Full),
            "and a bar standing at full strength is not fading"
        );
    }

    // The glance card's dwell, which is the same shape once more.
    let armed = PeekClock::Settling(start + Duration::from_millis(350));
    assert_eq!(
        peek_opacity(armed, start),
        None,
        "a card that is still settling has nothing on the glass to move"
    );
}
