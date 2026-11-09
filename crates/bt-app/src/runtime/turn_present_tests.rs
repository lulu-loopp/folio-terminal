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

use std::time::{Duration, Instant};

use crate::headless::{HeadlessFolio, PlatformAnswers, WindowEffect};
use crate::test_support::{leaf_saying, tab_holding};

/// Far enough apart that every turn is a new frame for the window's frame clock.
const STEP: Duration = Duration::from_millis(250);

/// A shell whose command is running (OSC 133 C with no D yet), so its tab's mark breathes.
const WORKING: &str = "\x1b]133;A\x07PS> \x1b]133;B\x07cargo build\x1b]133;C\x07";

/// One window holding one shell that says `text`, its first frame on the glass, its ledger empty.
fn window_saying(tag: &str, text: &str) -> HeadlessFolio {
    let mut folio = HeadlessFolio::with_tabs(tag, vec![tab_holding(leaf_saying(text))], 0);
    folio.draw();
    folio.ledger.take();
    folio
}

/// **One turn of the window at `now`**, as the loop takes it (its own clocks, then the loop's
/// hand-over after it), and what it left on the ledger.
fn turn_at(folio: &mut HeadlessFolio, now: Instant) -> Vec<WindowEffect> {
    let mut runtime = folio.runtime();
    runtime.turn(now, false).expect("the turn is answered");
    runtime.end_the_turns_presents();
    folio.ledger.take()
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
/// here), none when nothing was presented or asked for. With the turn's own ledger.
fn turns_to_the_glass(folio: &mut HeadlessFolio, now: Instant) -> (usize, Vec<WindowEffect>) {
    let effects = turn_at(folio, now);
    if presents(&effects) > 0 {
        return (1, effects);
    }
    if effects.contains(&WindowEffect::Redraw) {
        folio.runtime().redraw().expect("the paint is answered");
        let painted = folio.ledger.take();
        return (if presents(&painted) > 0 { 2 } else { 0 }, effects);
    }
    (0, effects)
}

/// Whether the frame on the glass draws the letters `text` anywhere in its cells.
fn the_glass_draws(folio: &HeadlessFolio, text: &str) -> bool {
    folio
        .window
        .last_presented_frame
        .as_ref()
        .expect("the window has presented a frame")
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>()
        .contains(text)
}

/// RED — **a step of the working mark reaches the glass on the turn that ticked it.**
///
/// The strip's tick redraws the chrome and owes the glass a present of the picture already
/// there (`publish_chrome_frame`). That present is the turn's to pay, at its own end; asking the
/// platform for it is the second turn per step this row removes.
///
/// MUTATION: make `Runtime::ask_for_present` always ask the platform (drop its deferral) — every
/// step takes two turns and asks for a paint.
#[test]
fn a_working_mark_step_reaches_the_glass_in_its_own_turn() {
    let mut folio = window_saying("turn-present-mark", WORKING);
    assert!(
        folio.window.tabs[0].fleet_working(),
        "the fixture's own precondition: the shell's command is running, so the mark breathes"
    );
    let mut now = Instant::now();
    for step in 0..3 {
        now += STEP;
        let (turns, effects) = turns_to_the_glass(&mut folio, now);
        assert_eq!(
            folio.window.strip_animation_ticked_at,
            Some(now),
            "step {step}: the fixture's own precondition: the strip ticked on this turn"
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
    }
}

/// RED — **a caret blink reaches the glass on the turn it came due on.**
///
/// The blink is a phase in the renderer, and it too owes a present of the picture already on the
/// glass (`advance_cursor_blink_if_due` → `publish_chrome_frame`).
///
/// MUTATION: as above — each blink takes two turns.
#[test]
fn a_caret_blink_reaches_the_glass_in_its_own_turn() {
    let mut folio = window_saying("turn-present-caret", "PS> ");
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
        let (turns, effects) = turns_to_the_glass(&mut folio, due);
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
/// door the drain does: this headless shell has no reader thread to drain.
///
/// What is checked is the output's letters on the glass, not the present's `source`: the slot
/// holds one frame and the trigger of whichever publish came last, and later in the same turn
/// another publish (the window's opening size released to its shell, a re-projection) composes
/// from the same grid and stands in the slot over the drain's.
///
/// MUTATION: as above — the output's frame takes two turns.
#[test]
fn a_panes_output_reaches_the_glass_in_the_turn_that_publishes_it() {
    let mut folio = window_saying("turn-present-output", "PS> ");
    assert!(
        !the_glass_draws(&folio, "Compiling"),
        "the fixture's own precondition: the output is not on the glass yet"
    );
    let now = Instant::now() + STEP;
    output_held_until(&mut folio, b"   Compiling bt-app v0.0.0\r\n", now);
    let (turns, effects) = turns_to_the_glass(&mut folio, now);
    assert_eq!(
        turns, 1,
        "the output reaches the glass on the turn that published it: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::Redraw),
        "and the turn asks the platform for no paint: {effects:?}"
    );
    assert_eq!(presents(&effects), 1, "one present: {effects:?}");
    assert!(
        the_glass_draws(&folio, "Compiling"),
        "the present carries the output: {effects:?}"
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
        let mut folio = window_saying("turn-present-hidden", "PS> ");
        folio.window.window.answers.set(answers);
        folio.window.window_shown = shown;
        let now = Instant::now() + STEP;
        output_held_until(&mut folio, b"   Compiling bt-app v0.0.0\r\n", now);
        let effects = turn_at(&mut folio, now);
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
    let mut folio = window_saying("turn-present-early", "PS> ");
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
