//! **A formula's change of face, and the journeys a frame carries, driven**
//! (T-HEADLESS-RUNTIME-SEAM) on a headless window: a typeset display block in a shell's
//! transcript, its `‹›` flight, and the frames composed around it.

use crate::formula_tools::FormulaToggleMotion;
use crate::headless::HeadlessFolio;
use crate::test_support::{leaf_saying, tab_holding};
use crate::{Motion, PasteTarget, pace, toast};
use bt_render::{FrameSource, FrameTrigger};
use bt_viewport::MathBlockAnchor;
use std::time::{Duration, Instant};

/// The picture the test's typesetter answers every display formula with: a band of `24 × 35`
/// pixels, the engine's shape for a display block.
fn typeset_picture() -> bt_math::MathRaster {
    bt_math::MathRaster {
        rgba: vec![255; 24 * 35 * 4],
        width_px: 24,
        height_px: 35,
        content_height_px: 19,
        ascent_px: 12.0,
        descent_px: 4.0,
        baseline_px: 12.0,
        render_time: Duration::from_millis(3),
        inline_runs: Vec::new(),
    }
}

/// **A window of one shell whose transcript holds one typeset display formula, scrolled into
/// view**: the window, the pane the block stands in, and the block.
///
/// The decoration requests go to a channel the test holds, and the formula's is answered with
/// [`typeset_picture`] the way the window answers a completion — through the session's own
/// `complete_worker_result`.
fn a_typeset_formula(tag: &str) -> (HeadlessFolio, PasteTarget, MathBlockAnchor) {
    let tab = crate::test_support::cross_tab(1, &["公式 formula", "旁边 beside"]);
    let seat = tab.seats.terminals()[0];
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
    let (tasks, requests) = std::sync::mpsc::channel();
    let (scale_tasks, _) = std::sync::mpsc::channel();
    let (path_tasks, _) = std::sync::mpsc::channel();
    let (_, results) = std::sync::mpsc::channel();
    folio.app.math_worker = crate::MathWorker {
        tasks,
        scale_tasks,
        path_tasks,
        results,
    };
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
    let mut answered = false;
    for request in requests.try_iter() {
        let crate::MathWorkerRequest::Math { task, .. } = request else {
            continue;
        };
        let bt_term::SessionMathTask::Frozen(mut task) = *task else {
            continue;
        };
        assert!(
            bt_detect::resolve_detection_task(&mut task),
            "the block is proved"
        );
        answered |= folio
            .runtime()
            .sessions
            .get_mut(&seat)
            .expect("the shell")
            .session
            .complete_worker_result(task, Ok(typeset_picture()));
    }
    assert!(
        answered,
        "the fixture's own precondition: the formula was asked for and answered"
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
/// MUTATION: answer only the travel's next frame from `Runtime::math_toggle_deadline` (drop
/// `flight.lands_at()`) — the wake is a frame after the landing.
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
