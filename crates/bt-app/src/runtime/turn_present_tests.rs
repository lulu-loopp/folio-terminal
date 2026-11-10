//! **A present a window's own turn asks for is presented by that turn**
//! (T-TWO-BARE-TURNS-PER-MARK-STEP, 0.4.9 plan row 196).
//!
//! A step of the working mark, a caret blink and a pane's held output each publish from inside
//! `Runtime::turn`. They used to ask the platform for a paint, and the paint came back as a
//! second turn of the loop: a wake, the `RedrawRequested`, and every clock of `about_to_wait` run
//! again. On the VM that second turn cost 515 µs of a 3774 µs mark step with three panes, and half
//! of the window's wakes while a session worked. Each case below drives one of them through the
//! turn it comes due on, and counts the turns its present took: one when the turn presented, two
//! when it asked the platform for the paint (delivered here as winit would deliver it).
//!
//! The instants are values handed to the turn; nothing here sleeps or reads a duration back.
//! The one real-clock reading is the stamp `trace_present` puts on a present
//! (`last_present_at`, `Instant::now()`): the mark test requires it to be no later than the
//! turn's instant, which the handed instants keep by running ahead of the clock, and the
//! refused-frame test reads it back as the instant the fold books from.

use std::time::{Duration, Instant};

use bt_render::FrameSource;

use crate::headless::{HeadlessFolio, PlatformAnswers, WindowEffect};
use crate::test_support::{leaf_saying, tab_holding};
use crate::wake_owner::WakeOwner;

/// Far enough apart that every turn is a new frame for the window's frame clock.
const STEP: Duration = Duration::from_millis(250);

/// A shell whose command is running (OSC 133 C with no D yet), so its tab's mark breathes.
const WORKING: &str = "\x1b]133;A\x07PS> \x1b]133;B\x07cargo build\x1b]133;C\x07";

/// **One window holding one shell that says `text`**: its first frame on the glass, the size its
/// layout gave the shell told and quiet, its ledger empty; with the instant of its last turn.
///
/// The tab takes its layout when the window is made, and that opens a resize its shell is told of
/// over the next turns; each of those turns publishes the resize's own frames, which would stand
/// in the slot over the frame a test drives. So the window turns, a step apart, until no shell
/// has a resize open, as `every_visible_pane_settles_its_rows_and_schedules_its_own_artifacts`
/// settles its panes.
///
/// The window's first-run card is spent first, as the composition fixtures spend it
/// (`keyboard_tests.rs`, `spend_the_first_run_card`) and as the mark-over-a-quiet-pane test does.
/// A headless window is a first launch, and every turn runs `raise_first_run_if_due`: on a Mac,
/// whose profile table has no agent rows for the card to wait on, the card comes up on the first
/// counted turn. It is a modal: it takes the keyboard from the shell, so the caret stops
/// blinking, and the turn that raises it asks the platform for a paint of the card's own
/// (`Redraw` on that turn's ledger). On Windows the agent rows' unknown answers hold the card
/// down.
fn window_saying(tag: &str, text: &str) -> (HeadlessFolio, Instant) {
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf_saying(text))], 0);
    folio.app.first_run_attempted = true;
    folio.draw();
    let mut now = Instant::now();
    for _ in 0..8 {
        if !a_resize_is_open(&mut folio) {
            break;
        }
        now += STEP;
        turn_at(&mut folio, now);
    }
    assert!(
        !a_resize_is_open(&mut folio),
        "the fixture's own precondition: the shell has settled its first size"
    );
    folio.ledger.take();
    (folio, now)
}

/// Whether any shell on screen still has a resize open.
fn a_resize_is_open(folio: &mut HeadlessFolio) -> bool {
    folio
        .runtime()
        .sessions
        .values()
        .any(|leaf| leaf.session.session.resize_transaction_open())
}

/// **One turn of the window at `now`**, as the loop takes it (its own clocks, then the loop's
/// hand-over after it): the wake it booked, and what it left on the ledger.
fn turn_at(
    folio: &mut HeadlessFolio,
    now: Instant,
) -> (Option<(WakeOwner, Instant)>, Vec<WindowEffect>) {
    let mut runtime = folio.runtime();
    let wake = runtime.turn(now, false).expect("the turn is answered");
    runtime.end_the_turns_presents();
    (wake, folio.ledger.take())
}

/// How many presents a ledger holds.
fn presents(effects: &[WindowEffect]) -> usize {
    effects
        .iter()
        .filter(|effect| matches!(effect, WindowEffect::Presented { .. }))
        .count()
}

/// **How many turns of the loop the turn at `now` took to reach the glass**: one when it
/// presented, two when it asked the platform for a paint (the paint is the second turn, delivered
/// here), none when nothing was presented or asked for. With the turn's own wake and ledger.
fn turns_to_the_glass(
    folio: &mut HeadlessFolio,
    now: Instant,
) -> (usize, Option<(WakeOwner, Instant)>, Vec<WindowEffect>) {
    let (wake, effects) = turn_at(folio, now);
    if presents(&effects) > 0 {
        return (1, wake, effects);
    }
    if effects.contains(&WindowEffect::Redraw) {
        folio.runtime().redraw().expect("the paint is answered");
        let painted = folio.ledger.take();
        return (if presents(&painted) > 0 { 2 } else { 0 }, wake, effects);
    }
    (0, wake, effects)
}

/// The one present a turn's ledger holds, by its source.
fn presented_source(effects: &[WindowEffect]) -> Option<FrameSource> {
    effects.iter().find_map(|effect| match effect {
        WindowEffect::Presented { source, .. } => Some(*source),
        _ => None,
    })
}

/// RED — **a step of the working mark reaches the glass on the turn that ticked it.**
///
/// The strip's tick redraws the chrome and owes the glass a present of the picture already
/// there (`publish_chrome_frame`). That present is the turn's to pay, at its own end; asking the
/// platform for it is the second turn per step this row removes.
///
/// And the wake the turn returns is the mark's own next step, one strip interval after the tick:
/// not the turn's own instant, and not a bare frame booked for a debt the turn has paid.
///
/// MUTATION: make `Runtime::ask_for_present` always ask the platform (drop its deferral) — every
/// step takes two turns and asks for a paint.
#[test]
fn a_working_mark_step_reaches_the_glass_in_its_own_turn() {
    let (mut folio, mut now) = window_saying("turn-present-mark", WORKING);
    assert!(
        folio.window.tabs[0].fleet_working(),
        "the fixture's own precondition: the shell's command is running, so the mark breathes"
    );
    let interval = folio.window.frame_clock.interval();
    for step in 0..3 {
        now += STEP;
        let (turns, wake, effects) = turns_to_the_glass(&mut folio, now);
        assert_eq!(
            folio.window.strip_animation_ticked_at,
            Some(now),
            "step {step}: the fixture's own precondition: the strip ticked on this turn"
        );
        assert!(
            folio.window.last_present_at.is_some_and(|at| at <= now),
            "step {step}: the fixture's own precondition: the turn's present is stamped no later \
             than the turn's instant, as on the loop"
        );
        assert_eq!(
            turns, 1,
            "step {step}: the mark's step reaches the glass on the turn that ticked it: \
             {effects:?}"
        );
        assert!(
            !effects.contains(&WindowEffect::Redraw),
            "step {step}: and the turn asks the platform for no paint: {effects:?}"
        );
        assert_eq!(
            presents(&effects),
            1,
            "step {step}: one present: {effects:?}"
        );
        assert_eq!(
            wake,
            Some((WakeOwner::StripAnimation, now + interval)),
            "step {step}: the turn's next wake is the mark's next step"
        );
    }
}

/// RED — **a caret blink reaches the glass on the turn it came due on.**
///
/// The blink is a phase in the renderer, and it too owes a present of the picture already on the
/// glass (`advance_cursor_blink_if_due` → `publish_chrome_frame`).
///
/// And, as for the mark, the wake the blink's turn returns is the caret's own next phase.
///
/// MUTATION: as above — each blink takes two turns.
#[test]
fn a_caret_blink_reaches_the_glass_in_its_own_turn() {
    let (mut folio, _) = window_saying("turn-present-caret", "PS> ");
    assert!(
        folio.runtime().keyboard_owner_is_a_shell(),
        "the fixture's own precondition: a shell holds the keyboard, so its caret blinks"
    );
    for blink in 0..3 {
        let due = folio
            .window
            .cursor_blink
            .deadline()
            .expect("a blinking caret has its next phase");
        let lit = folio.window.cursor_blink.visible();
        let (turns, wake, effects) = turns_to_the_glass(&mut folio, due);
        assert_ne!(
            folio.window.cursor_blink.visible(),
            lit,
            "blink {blink}: the fixture's own precondition: the caret changed phase"
        );
        assert_eq!(
            turns, 1,
            "blink {blink}: the blink reaches the glass on its own turn: {effects:?}"
        );
        assert!(
            !effects.contains(&WindowEffect::Redraw),
            "blink {blink}: and the turn asks the platform for no paint: {effects:?}"
        );
        assert_eq!(
            presents(&effects),
            1,
            "blink {blink}: one present: {effects:?}"
        );
        let next = folio
            .window
            .cursor_blink
            .deadline()
            .expect("a blinking caret has its next phase");
        assert!(
            next > due,
            "blink {blink}: the fixture's own precondition: the caret's next phase is later"
        );
        assert_eq!(
            wake,
            Some((WakeOwner::ShellCaret, next)),
            "blink {blink}: the turn's next wake is the caret's next phase"
        );
    }
}

/// The window's one shell is handed `bytes`, as the drain feeds a pane, and the publication is
/// held for the rest of the burst until `until` — the drain's own deferral (`coalesce::Pending`).
fn output_held_until(folio: &mut HeadlessFolio, bytes: &[u8], until: Instant) {
    folio.window.tabs[0]
        .sessions
        .values_mut()
        .next()
        .expect("the tab's one shell")
        .session
        .session
        .feed(bytes)
        .expect("the fixture's bytes parse");
    folio.window.pty_coalesce.first_unpublished = Some(until);
    folio.window.pty_coalesce.until = Some(until);
}

/// RED — **a pane's output reaches the glass on the turn that publishes it.**
///
/// The drain's frame enters the slot in `publish_frame_inner` and is presented by the turn that
/// published it, not by a paint the platform sends afterwards. Driven through the drain's own
/// deferral, released on its turn (`finish_pty_coalesce_if_due`), which publishes through the same
/// door the drain does: this headless shell has no reader thread to drain. On a window whose
/// shell has settled its first size, so the present is the drain's own frame (`PtyOutput`) and no
/// resize's.
///
/// MUTATION: as above — the output's frame takes two turns.
#[test]
fn a_panes_output_reaches_the_glass_in_the_turn_that_publishes_it() {
    let (mut folio, settled) = window_saying("turn-present-output", "PS> ");
    let now = settled + STEP;
    output_held_until(&mut folio, b"   Compiling bt-app v0.0.0\r\n", now);
    let (turns, _, effects) = turns_to_the_glass(&mut folio, now);
    assert_eq!(
        turns, 1,
        "the output reaches the glass on the turn that published it: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::Redraw),
        "and the turn asks the platform for no paint: {effects:?}"
    );
    assert_eq!(presents(&effects), 1, "one present: {effects:?}");
    assert_eq!(
        presented_source(&effects),
        Some(FrameSource::PtyOutput),
        "the present is the output's own frame: {effects:?}"
    );
}

/// RED — **a turn that refused a frame books the refused frame from the present it paid.**
///
/// The frame gate turns the mark's step away when the glass took a picture less than a frame
/// ago, and the fold books the refused frame's wake one frame after the window's last present
/// (`FrameClock::owes_a_frame` → `next_animation_deadline`). On this turn the shell's output
/// also reaches the glass, and that present is the turn's to pay at its own end. Paid before the
/// fold, the refused frame is booked a frame after the present the turn made; paid after it, the
/// fold books it from the picture before, which the turn's own present has already replaced.
///
/// The strip has not ticked yet (`strip_animation_ticked_at` is `None`), so its next tick is the
/// frame deadline itself and the wake is the strip's, at that instant. The instant is read back
/// from the window: a present is stamped with the clock `trace_present` reads, not with the
/// instant handed to the turn.
///
/// MUTATION: move the `pay_the_turns_present` call in `Runtime::turn` below the deadline fold —
/// the wake is booked a frame after the held picture.
#[test]
fn a_refused_frame_is_booked_from_the_present_its_turn_paid() {
    let (mut folio, settled) = window_saying("turn-present-refused", WORKING);
    let now = settled + STEP;
    let interval = folio.window.frame_clock.interval();
    let held = now - interval / 2;
    folio.window.last_present_at = Some(held);
    folio.window.strip_animation_ticked_at = None;
    output_held_until(&mut folio, b"   Compiling bt-app v0.0.0\r\n", now);
    let (wake, effects) = turn_at(&mut folio, now);
    assert!(
        folio.window.frame_clock.owes_a_frame() && folio.window.strip_animation_ticked_at.is_none(),
        "the fixture's own precondition: the gate turned the mark's step away"
    );
    assert_eq!(
        presented_source(&effects),
        Some(FrameSource::PtyOutput),
        "the fixture's own precondition: the turn presented the output: {effects:?}"
    );
    assert_eq!(presents(&effects), 1, "one present: {effects:?}");
    let presented = folio
        .window
        .last_present_at
        .expect("the turn's present is stamped");
    assert_ne!(
        presented, held,
        "the fixture's own precondition: the turn's present replaced the held one"
    );
    assert_eq!(
        wake,
        Some((WakeOwner::StripAnimation, presented + interval)),
        "the refused step is booked a frame after the present this turn paid"
    );
}

/// RED — **a window on no screen keeps asking the platform** (ruling R2).
///
/// A minimized or cloaked window, or one not shown yet, gets no paint from the platform until it
/// can be seen. Its present stays the platform's to send, exactly as before this row; the turn
/// presents nothing.
///
/// MUTATION: drop the `window_shown && !window_hidden` test in `Runtime::pay_the_turns_present`
/// — each case presents on the turn.
#[test]
fn a_window_on_no_screen_still_asks_the_platform_for_its_paint() {
    for (case, answers, shown) in [
        (
            "minimized",
            PlatformAnswers {
                iconic: true,
                ..PlatformAnswers::default()
            },
            true,
        ),
        (
            "cloaked",
            PlatformAnswers {
                cloaked: true,
                ..PlatformAnswers::default()
            },
            true,
        ),
        ("not shown yet", PlatformAnswers::default(), false),
    ] {
        let (mut folio, settled) = window_saying("turn-present-hidden", "PS> ");
        folio.window.window.answers.set(answers);
        folio.window.window_shown = shown;
        let now = settled + STEP;
        output_held_until(&mut folio, b"   Compiling bt-app v0.0.0\r\n", now);
        let (_, effects) = turn_at(&mut folio, now);
        assert!(
            effects.contains(&WindowEffect::Redraw),
            "{case}: the window asks the platform for the paint: {effects:?}"
        );
        assert_eq!(
            presents(&effects),
            0,
            "{case}: and the turn presents nothing: {effects:?}"
        );
    }
}

/// RED — **a turn that left before its end hands the present it was asked for to the platform,
/// and the window stops deferring.**
///
/// A turn that returns an error has not reached its end, where it pays. The loop's hand-over
/// after it (`end_the_turns_presents`) asks the platform for what was deferred, and an ask after
/// it (an event handler's) is the platform's again: a window kept after a lost device must not
/// keep deferring presents no turn end will pay.
///
/// MUTATION: make `end_the_turns_presents` only lower the flag — the deferred present is never
/// asked for.
#[test]
fn a_turn_that_left_early_hands_its_present_to_the_platform() {
    let (mut folio, _) = window_saying("turn-present-early", "PS> ");
    // A turn that raised its flag and returned before its end.
    folio.window.turn_pays_presents = true;
    folio.runtime().ask_for_present();
    assert!(
        !folio.ledger.take().contains(&WindowEffect::Redraw),
        "inside a turn the ask is the turn's to pay"
    );
    folio.runtime().end_the_turns_presents();
    assert_eq!(
        folio.ledger.take(),
        vec![WindowEffect::Redraw],
        "the hand-over asks the platform for the present the turn did not pay"
    );
    folio.runtime().ask_for_present();
    assert_eq!(
        folio.ledger.take(),
        vec![WindowEffect::Redraw],
        "and an ask outside any turn is the platform's again"
    );
}

/// RED — **the loop hands over every window's deferred present right after that window's turn.**
///
/// `end_the_turns_presents` is what lowers the flag after a turn that left before its end (an
/// error the loop answers with `fail`, a window it keeps). Without the call, that window would go
/// on deferring the presents its event handlers ask for until its next turn, and in Windows'
/// modal size loop, where `about_to_wait` does not run, that is no turn at all. The test above
/// calls the function itself; this one pins where the loop calls it: after `runtime.turn` and
/// before the turn's answer is looked at, so an error's early `return` cannot skip it.
///
/// MUTATION: delete the call in `FolioApp::about_to_wait_inner`, or move it into the `Ok` arm.
#[test]
fn the_loop_hands_over_each_windows_deferred_present_after_its_turn() {
    // Comments and whitespace squeezed out, so a comment that quotes the call cannot stand in
    // for it.
    let body = crate::test_support::squeezed_body("FolioApp", "about_to_wait_inner");
    let turn = body
        .find("letturn=runtime.turn(now,application_clocks);")
        .unwrap_or_else(|| panic!("the windows' own turn has moved:\n{body}"));
    let hand_over = body
        .find("runtime.end_the_turns_presents();")
        .unwrap_or_else(|| {
            panic!("the loop no longer hands over a turn's deferred present:\n{body}")
        });
    let answer = body
        .find("matchturn{")
        .unwrap_or_else(|| panic!("the turn's answer is no longer matched:\n{body}"));
    assert!(
        turn < hand_over && hand_over < answer,
        "the hand-over stands between the turn and the look at its answer:\n{body}"
    );
}
