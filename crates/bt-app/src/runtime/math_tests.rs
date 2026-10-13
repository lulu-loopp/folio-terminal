//! **A formula's change of face, and the journeys a frame carries, driven**
//! (T-HEADLESS-RUNTIME-SEAM) on a headless window: a typeset display block in a shell's
//! transcript, its `‹›` flight, and the frames composed around it.

use crate::computation_lane;
use crate::formula_tools::FormulaToggleMotion;
use crate::headless::HeadlessFolio;
use crate::test_support::{leaf_saying, tab_holding};
use crate::{Motion, PasteTarget, pace, toast};
use bt_render::{FrameSource, FrameTrigger};
use bt_viewport::MathBlockAnchor;
use std::time::{Duration, Instant};

/// **A window of one shell whose transcript holds one typeset display formula, scrolled into
/// view**: the window, the pane the block stands in, and the block.
///
/// The decoration requests go to a computation lane the test watches; the formula's answer — the
/// product's own typesetting — is handed to the session the way the window hands a completion,
/// through its `complete_worker_result`.
fn a_typeset_formula(tag: &str) -> (HeadlessFolio, PasteTarget, MathBlockAnchor) {
    let tab = crate::test_support::cross_tab(1, &["公式 formula", "旁边 beside"]);
    let seat = tab.seats.terminals()[0];
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let mut watched = computation_lane::fixtures::watched(crate::lane::Executor::scripted(
        std::sync::Arc::new(crate::lane::Gate::default()),
    ));
    folio.app.math_worker = std::sync::Arc::clone(&watched.lane);
    folio.app.motion = Motion::Full;
    // The pane's first size, told and quiet.
    let mut now = Instant::now();
    folio.draw();
    for _ in 0..8 {
        if !folio.runtime().sessions[&seat]
            .session
            .resize_transaction_open()
        {
            break;
        }
        now += Duration::from_millis(250);
        folio
            .runtime()
            .turn(now, true)
            .expect("the turn is answered");
        folio.draw();
    }
    {
        let mut runtime = folio.runtime();
        let pane = runtime.sessions.get_mut(&seat).expect("the shell");
        let mut printed = b"\r\n$$x^2+y^2=z^2$$\r\n".to_vec();
        printed.extend(b"\r\n".repeat(64));
        pane.session.feed(&printed).expect("the formula is printed");
        pane.session.refresh_projection(&mut pane.projection);
        pane.session
            .viewport_frame(&mut pane.projection)
            .expect("the pane projects");
        pane.projection.scroll_to_top();
    }
    folio.draw();
    let (_, _, terminal, _) = watched.next_ended(
        computation_lane::Share::Decoration,
        computation_lane::fixtures::nobody,
    );
    let answer =
        computation_lane::fixtures::answer(terminal).expect("the lane typesets the formula");
    let crate::DecorationWorkerCompletion::Math { task, result } = answer.completion else {
        panic!("the lane's answer is a formula's");
    };
    let bt_term::SessionMathTask::Frozen(task) = *task else {
        panic!("the formula stands in the transcript");
    };
    assert!(
        folio
            .runtime()
            .sessions
            .get_mut(&seat)
            .expect("the shell")
            .session
            .complete_worker_result(task, result),
        "the fixture's own precondition: the session takes the formula's picture"
    );
    folio.draw();
    let runtime = folio.runtime();
    let anchor = runtime.sessions[&seat]
        .last_presented_frame
        .as_ref()
        .expect("the pane has drawn")
        .math_blocks
        .iter()
        .find(|block| matches!(block.anchor, MathBlockAnchor::History { .. }))
        .expect("the typeset block is on the frame")
        .anchor
        .clone();
    let target = runtime.paste_target(seat).expect("the shell");
    (folio, target, anchor)
}

/// The flight of `anchor` towards its source, begun at `started`.
fn a_flight(
    folio: &mut HeadlessFolio,
    target: PasteTarget,
    anchor: &MathBlockAnchor,
    started: Instant,
) -> FormulaToggleMotion {
    let faces = folio
        .runtime()
        .math_toggle_faces(target, anchor)
        .expect("the block measures both faces");
    assert!(
        !faces.showing_source,
        "the fixture's block wears its picture"
    );
    FormulaToggleMotion::begin(
        target,
        anchor.clone(),
        faces.heights(),
        true,
        faces.source,
        started,
    )
}

/// The band height `anchor` stands at on the pane's last presented frame.
fn presented_band(folio: &mut HeadlessFolio, target: PasteTarget, anchor: &MathBlockAnchor) -> i64 {
    folio.runtime().sessions[&target.seat]
        .last_presented_frame
        .as_ref()
        .expect("the pane has drawn")
        .math_blocks
        .iter()
        .find(|block| block.anchor.same_block(anchor))
        .expect("the block is on the frame")
        .artifact
        .height_subpixels
}

/// RED (review 2026-09-18, P1) — **a frame composed for any reason carries the formula flight it is
/// built from.** A flight half way over, and a frame composed for something else entirely (an
/// expose, with no journey reported running at the last turn): the session's presentation is the
/// flight's sample at that compose, and the band on the frame stands at exactly that height —
/// not at the picture's, which is what a frame projected before the carry would show.
///
/// MUTATIONS: move `self.carry_live_journeys(Instant::now())` below the projection in
/// `Runtime::publish_frame_inner` — the band stands at the picture's height; drop
/// `self.sample_math_toggle(now)` from `Runtime::carry_live_journeys`, or move it below its
/// `if !running.any()` — nothing presents the flight at all.
#[test]
fn a_frame_composed_for_any_reason_carries_the_formula_flight_it_is_built_from() {
    let (mut folio, target, anchor) = a_typeset_formula("headless-carry");
    let rendered = presented_band(&mut folio, target, &anchor);
    let started = Instant::now()
        .checked_sub(Duration::from_millis(45))
        .expect("the process is older than a frame");
    let flight = a_flight(&mut folio, target, &anchor, started);
    let source = folio
        .runtime()
        .math_toggle_heights(target, &anchor)
        .expect("the block measures")[1];
    assert_ne!(rendered, source, "the fixture's two faces differ in height");
    let mut runtime = folio.runtime();
    runtime.window.math_toggle = Some(flight);
    runtime
        .window
        .frame_clock
        .note_running(pace::Lanes::default());
    runtime
        .publish_frame(FrameTrigger {
            occurred_at: Instant::now(),
            source: FrameSource::Expose,
        })
        .expect("the frame is composed");
    runtime.redraw().expect("and reaches the glass");
    let carried = runtime.sessions[&target.seat]
        .session
        .math_toggle_presentation()
        .expect("the compose carried the flight into the session")
        .height_subpixels;
    assert!(
        carried != rendered && carried != source,
        "the flight is half way over: {rendered} → {carried} → {source}"
    );
    assert_eq!(
        presented_band(&mut folio, target, &anchor),
        carried,
        "the frame is built from the sample the compose carried"
    );
}

/// RED (review 2026-09-18, P1) — **a journey that has arrived lands on the turn it is due, whether
/// or not the frame gate admits the turn.** The landing is owed to the document — the block
/// switches face and the presentation is put back — and not to the glass, so a picture that
/// reached the glass a moment ago (the gate refusing) does not postpone it.
///
/// MUTATION: put the `if landed || !still_measures` landing below the
/// `if !self.animation_frame_is_due()` gate in `Runtime::advance_math_toggle_if_due` — the flight
/// is still in the air and the block still wears its picture.
#[test]
fn a_flight_that_has_arrived_lands_on_a_turn_the_frame_gate_refuses() {
    let (mut folio, target, anchor) = a_typeset_formula("headless-landing");
    let started = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("the process is older than a second");
    let flight = a_flight(&mut folio, target, &anchor, started);
    let now = Instant::now();
    let mut runtime = folio.runtime();
    assert!(flight.lands_at() <= now, "the fixture's flight has arrived");
    runtime.window.math_toggle = Some(flight);
    // A picture reached the glass this very instant: the turn is not one an animation may draw on.
    runtime.window.frame_clock.open(Some(now), now);
    assert!(
        !runtime.window.frame_clock.admits(),
        "the gate refuses this turn"
    );
    runtime
        .advance_math_toggle_if_due(now)
        .expect("the turn is answered");
    assert!(
        runtime.window.math_toggle.is_none(),
        "the flight has landed"
    );
    assert!(
        runtime
            .math_toggle_faces(target, &anchor)
            .expect("the block measures")
            .showing_source,
        "and the block wears the face it was travelling to"
    );
}

/// RED (review 2026-09-18, P1) — **the loop is woken for a landing on the landing's own clock**,
/// and for whichever comes first of it and the travel's next frame: a flight five milliseconds
/// from its end asks to be woken at its end, not a frame later.
///
/// MUTATIONS: answer only the travel's next frame from `Runtime::math_toggle_deadline` (drop
/// `flight.lands_at()`) — the wake is a frame after the landing; answer only the landing (drop
/// `frame.min(landing)`'s frame) — a flight just begun sleeps until it lands.
#[test]
fn the_loop_is_woken_for_a_landing_on_its_own_clock() {
    let (mut folio, target, anchor) = a_typeset_formula("headless-landing-wake");
    let now = Instant::now();
    let span = a_flight(&mut folio, target, &anchor, now).lands_at() - now;
    let started = (now + Duration::from_millis(5))
        .checked_sub(span)
        .expect("the process is older than a flight");
    let flight = a_flight(&mut folio, target, &anchor, started);
    let lands = flight.lands_at();
    let runtime = folio.runtime();
    runtime.window.math_toggle = Some(flight);
    assert_eq!(
        runtime.math_toggle_deadline(now),
        Some(lands),
        "the loop is woken at the landing"
    );

    // And a flight just begun is woken for its next frame, well before it lands.
    let flight = a_flight(&mut folio, target, &anchor, now);
    let lands = flight.lands_at();
    let runtime = folio.runtime();
    runtime.window.math_toggle = Some(flight);
    let woken = runtime
        .math_toggle_deadline(now)
        .expect("a flight in the air wakes the loop");
    assert!(
        woken < lands,
        "a flight in the air is woken for its next frame, not its landing"
    );
}

/// RED (review 2026-09-18 rounds 2 and 3) — **what is running is told once a turn from the
/// journeys' own predicates; waiting is not moving, and asking the gate is not evidence.** A
/// notice resting in its life has a deadline and moves nothing; a notice fading in moves the
/// overlay, and the turn tells the frame clock so; a turn's gate, asked, reports nothing.
///
/// MUTATIONS: read `self.toast_deadline(now).is_some()` as running in `Runtime::running_journeys`
/// — a resting notice is a running one; drop `self.window.frame_clock.note_running(running)` from
/// `Runtime::turn` — the clock never hears of the fade; note a lane running in
/// `Runtime::animation_frame_is_due` — the question becomes the evidence.
#[test]
fn what_is_running_is_told_once_a_turn_and_waiting_is_not_moving() {
    let mut folio = HeadlessFolio::with_tabs(
        "headless-running",
        vec![tab_holding(leaf_saying("运行 running"))],
        0,
    );
    folio.app.motion = Motion::Full;
    let now = Instant::now();
    let mut runtime = folio.runtime();

    // A notice two seconds into its life: its fade is over and its life is not.
    let born = now
        .checked_sub(Duration::from_secs(2))
        .expect("the process is older than two seconds");
    runtime.window.toasts.raise(
        toast::ToastKind::Info,
        toast::ToastAnchor::Window,
        None,
        "静 resting",
        None,
        Motion::Full,
        born,
    );
    assert!(
        runtime.toast_deadline(now).is_some(),
        "the fixture's resting notice waits for the end of its life"
    );
    assert_eq!(
        runtime.running_journeys(now, false, false),
        pace::Lanes::default(),
        "a notice waiting out its life moves nothing"
    );

    // Asking the gate says nothing about what runs.
    runtime
        .window
        .frame_clock
        .note_running(pace::Lanes::default());
    let _ = runtime.animation_frame_is_due();
    assert_eq!(
        runtime.window.frame_clock.running(),
        pace::Lanes::default(),
        "the gate, asked, is not evidence"
    );

    // A notice fading in moves the overlay, and the turn says so.
    runtime.window.toasts.raise(
        toast::ToastKind::Info,
        toast::ToastAnchor::Window,
        None,
        "动 arriving",
        None,
        Motion::Full,
        now,
    );
    runtime.turn(now, true).expect("the turn is answered");
    assert!(
        runtime.window.frame_clock.running().overlay,
        "the turn tells the frame clock the overlay is moving"
    );
}

/// RED (review 2026-09-18 round 2) — **one compose builds the overlay once**, whichever lanes are
/// running: a chrome rebuild ends in the overlay, so the ordinary overlay lane is its `else`; and
/// while a formula is in flight its overlay is paid at the present door, so the carry builds none
/// of its own.
///
/// MUTATIONS: make the overlay arm of `Runtime::carry_live_journeys` an `if` of its own, or drop
/// its `!formula_present` — two overlay builds in one compose; carry the chrome with
/// `refresh_chrome()` while a formula is in flight — the same; drop `include_overlay &&
/// self.refresh_overlay()` from `Runtime::refresh_chrome_with_overlay` — a chrome lane builds no
/// overlay at all.
#[test]
fn one_compose_builds_the_overlay_once() {
    let (mut folio, target, anchor) = a_typeset_formula("headless-one-overlay");
    let started = Instant::now()
        .checked_sub(Duration::from_millis(45))
        .expect("the process is older than a frame");
    let flight = a_flight(&mut folio, target, &anchor, started);
    let both = pace::Lanes {
        chrome: true,
        overlay: true,
    };
    let chrome = pace::Lanes {
        chrome: true,
        overlay: false,
    };
    let overlay = pace::Lanes {
        chrome: false,
        overlay: true,
    };
    let mut flight = Some(flight);
    for (lanes, in_flight) in [
        (both, false),
        (chrome, false),
        (overlay, false),
        (both, true),
        (chrome, true),
        (overlay, true),
    ] {
        let mut runtime = folio.runtime();
        // A block pressed is a block hovered: the formula's overlay is lit while it travels.
        runtime.window.math_toggle = if in_flight { flight.take() } else { None };
        runtime.window.math_hover_anchor = in_flight.then(|| anchor.clone());
        runtime.window.frame_clock.note_running(lanes);
        let before = runtime.window.renderer.overlay_builds;
        runtime
            .publish_frame(FrameTrigger {
                occurred_at: Instant::now(),
                source: FrameSource::Expose,
            })
            .expect("the frame is composed");
        runtime.redraw().expect("and reaches the glass");
        let built = runtime.window.renderer.overlay_builds - before;
        assert_eq!(
            built, 1,
            "{lanes:?}, a formula in flight: {in_flight} — the overlay is built once a compose"
        );
        if in_flight {
            flight = runtime.window.math_toggle.take();
        }
    }
}

/// A glance card over `file`, raised from a files column, on the clock `clock`.
fn card_on(clock: crate::PeekClock) -> crate::FilePeek {
    crate::FilePeek {
        host: crate::RowHost::Column(bt_layout::SeatId(1)),
        source: crate::preview::PreviewSource::file("计划 plan.md".to_owned()),
        key: "计划 plan.md".to_owned(),
        name: "计划 plan.md".to_owned(),
        rect: [10.0, 10.0, 200.0, 30.0],
        clock,
        frame: None,
        body: None,
        head: None,
        foot: None,
        foot_lit: false,
        column: None,
        closing_at: None,
        dwell: None,
    }
}

/// RED (review 2026-09-18 rounds 2 and 3) — **each journey's own "am I moving" is what says it
/// runs, and a journey that is only waiting runs nothing.** A tip settling and a glance card
/// settling each owe the loop a wake-up (their deadlines) and move nothing; and each of the
/// moving predicates the window reads — a tip fading in, the card hint's nudge, a command rail's
/// tween, the formula marks arriving, a glance card fading in — puts its own lane in motion, one
/// at a time, on a window where nothing else moves.
///
/// MUTATIONS: read `tooltip_deadline` or `file_peek_deadline` as running in
/// `Runtime::running_journeys` — a waiting tip or card is a running one; drop any one of
/// `tooltip.is_fading`, `card_hint.nudge_moving`, `command_rails_are_moving`,
/// `math_tools_owe_frames` or `file_peek_is_fading` from it — that journey runs and nothing says
/// so.
#[test]
fn each_journeys_own_predicate_says_it_runs_and_a_wait_does_not() {
    let mut folio = HeadlessFolio::with_tabs(
        "headless-predicates",
        vec![tab_holding(leaf_saying("旅程 journeys"))],
        0,
    );
    folio.app.motion = Motion::Full;
    let now = Instant::now();
    let none = pace::Lanes::default();
    let overlay = pace::Lanes {
        chrome: false,
        overlay: true,
    };
    let chrome = pace::Lanes {
        chrome: true,
        overlay: false,
    };
    let runtime = folio.runtime();
    assert_eq!(
        runtime.running_journeys(now, false, false),
        none,
        "the fixture moves nothing"
    );

    // Waits.
    let tip = (
        crate::tooltip::TooltipAnchorId::Tab(0),
        crate::tooltip::TipFace::Chrome,
    );
    runtime.window.tooltip.observe(Some(tip), now);
    assert!(
        runtime.tooltip_deadline(now).is_some(),
        "a settling tip waits for its intent"
    );
    assert_eq!(
        runtime.running_journeys(now, false, false),
        none,
        "and a wait is not motion"
    );
    runtime.window.tooltip.hide();
    runtime.window.file_peek = Some(card_on(crate::PeekClock::Settling(
        now + Duration::from_millis(300),
    )));
    assert!(
        runtime.file_peek_deadline(now).is_some(),
        "a settling card waits for its intent"
    );
    assert_eq!(
        runtime.running_journeys(now, false, false),
        none,
        "and a wait is not motion"
    );
    runtime.window.file_peek = None;

    // Each moving predicate, alone.
    let due = now + tip.1.intent_delay();
    runtime.window.tooltip.observe(Some(tip), now);
    assert!(
        runtime.window.tooltip.activate_if_due(due),
        "the tip comes up when its intent is due"
    );
    assert_eq!(
        runtime.running_journeys(due, false, false),
        overlay,
        "a tip fading in"
    );
    runtime.window.tooltip.hide();

    runtime.window.card_hint.observe(true, true, now);
    assert_eq!(
        runtime.running_journeys(now, false, false),
        chrome,
        "the card hint's nudge"
    );
    runtime.window.card_hint.observe(false, false, now);

    let mut rail = crate::cmdrail::RailCache::default();
    rail.pointer_mut().aim(3, Some(1), true, now, Motion::Full);
    runtime
        .window
        .command_rails
        .insert(bt_layout::SeatId(1), rail);
    assert_eq!(
        runtime.running_journeys(now, false, false),
        overlay,
        "a command rail's tween"
    );
    runtime.window.command_rails.clear();

    let boxes = bt_render::MathToolBoxes {
        anchor: MathBlockAnchor::History {
            run: None,
            start: bt_transcript::TranscriptId(1),
            end: bt_transcript::TranscriptId(1),
        },
        display: bt_viewport::MathBlockDisplay::Rendered,
        block: [40.0, 10.0, 300.0, 59.0],
        source: [230.0, 25.0, 249.0, 44.0],
        copy: [251.0, 25.0, 270.0, 44.0],
    };
    runtime.window.math_tools = Some(crate::formula_tools::FormulaToolFollow::arriving(
        &boxes, None, now,
    ));
    assert_eq!(
        runtime.running_journeys(now, false, false),
        overlay,
        "the formula marks arriving"
    );
    runtime.window.math_tools = None;

    runtime.window.file_peek = Some(card_on(crate::PeekClock::Shown(now)));
    assert_eq!(
        runtime.running_journeys(now, false, false),
        overlay,
        "a glance card fading in"
    );
}

/// GUARD (kept, T-HEADLESS-RUNTIME-SEAM round 2) — **the carry hands neither formula lane a
/// frame.** `Runtime::carry_live_journeys` runs at the head of every compose, before any pane has
/// been projected, so it has no frame of this compose to place the formula marks or the source
/// face against; those two lanes are paid at the present door (`refresh_formula_overlay_for_present`),
/// which `one_compose_builds_the_overlay_once` drives. A call to `sync_math_tools` or
/// `math_tool_placement` from the carry would place them against the last compose's frame, and
/// what that changes on the glass is a one-frame lag the headless renderer does not measure — so
/// this one fact is a count of call sites, read from the source, and only this one.
#[test]
fn the_carry_hands_neither_formula_lane_a_frame() {
    let carry = crate::test_support::method_body("Runtime", "carry_live_journeys");
    assert!(
        !carry.contains("self.sync_math_tools(") && !carry.contains("self.math_tool_placement("),
        "the carry has no frame of this compose to hand to either formula lane:\n{carry}"
    );
}
