//! **A held gesture freezes every other hover, and no page hears the wheel under it**
//! (T-POINTER-CAPTURE cut 6, R-7 and R-10's capture half), on headless windows: the `⌄` rest
//! clocks and the tip arm nothing under a held gesture, a notch over a page under one is the page
//! layer's to swallow, and a touch pan parked under one is discarded.

use super::*;
use crate::headless::{Headless, HeadlessFolio};
use crate::runtime::pointer::CaptureOwner;
use crate::test_support::cross_tab;
use crate::webhost::{WebBounds, WebSeat};
use crate::{LeafId, TabPress};
use bt_layout::SeatId;
use std::time::Duration;
use winit::event::{MouseButton, MouseScrollDelta};

/// One window of one tab with two shells, drawn once.
fn a_window(tag: &str) -> HeadlessFolio {
    let mut folio = HeadlessFolio::with_tabs(tag, vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    folio.ledger.take();
    folio
}

/// A press on a tab that has not travelled from `at` — the gesture every test here holds.
fn a_tab_press(runtime: &mut Runtime<'_, Headless>, at: PhysicalPosition<f64>) {
    let tab = runtime.id;
    runtime.latch_capture(
        CaptureOwner::TabPress(TabPress::armed(tab, at, Instant::now())),
        MouseButton::Left,
    );
}

/// The first point of the window, row by row, where `wanted` answers.
fn first_point(
    runtime: &mut Runtime<'_, Headless>,
    mut wanted: impl FnMut(&mut Runtime<'_, Headless>, PhysicalPosition<f64>) -> bool,
) -> PhysicalPosition<f64> {
    (0..800)
        .step_by(2)
        .flat_map(|y| (0..1200).step_by(2).map(move |x| (x, y)))
        .map(|(x, y)| PhysicalPosition::new(f64::from(x), f64::from(y)))
        .find(|point| wanted(runtime, *point))
        .expect("the window draws what the test looks for")
}

/// The new-tab button's `⌄`, rested on for longer than its clock, with the hand free or holding a press.
fn rest_on_a_chevron(holding: bool) -> (bool, bool) {
    let mut folio = a_window(if holding {
        "held-chevron"
    } else {
        "free-chevron"
    });
    let mut runtime = folio.runtime();
    let chevron = first_point(&mut runtime, |runtime, point| {
        matches!(
            runtime.chrome_target_at(point),
            Some(seats::ChromeTarget::NewTabMenu)
        )
    });
    if holding {
        a_tab_press(&mut runtime, chevron);
    }
    runtime
        .pointer_moved(chevron)
        .expect("the move is answered");
    let rested = Instant::now() + Duration::from_secs(2);
    runtime.window.tooltip.activate_if_due(rested);
    let tip = runtime.window.tooltip.active().is_some();
    runtime.advance_chevrons(rested).expect("the rest is spent");
    (runtime.window.profile_menu.is_open(), tip)
}

/// RED (T-POINTER-CAPTURE cut 6, R-7) — **a held gesture resting on a `⌄` opens nothing**, and
/// arms no tip: the rest clock that opens a `⌄`'s menu after 250 ms (here the new-tab button's,
/// the profile menu) runs only for a free hand, so a press, a thumb or a selection resting on a
/// chevron does not raise a menu that would then stand where its release is owed (cells A·h). A
/// free hand resting on the same `⌄` opens the menu and arms the tip, so the fixture rests where
/// both answer.
///
/// MUTATION: drop the slot's gate from `observe_chevrons` (back to "a drag or a float carry") —
/// the held press opens the profile menu; drop the tip's gate in `pointer_moved` — the tip arms.
#[test]
fn a_held_gesture_resting_on_a_chevron_opens_nothing() {
    assert_eq!(
        rest_on_a_chevron(false),
        (true, true),
        "a free hand resting on the new-tab `⌄` opens its menu and arms its tip"
    );
    assert_eq!(
        rest_on_a_chevron(true),
        (false, false),
        "a hand holding a press opens no menu and arms no tip"
    );
}

/// The notch the wheel is spent with: three lines back up, as a mouse turns it.
fn a_notch(runtime: &mut Runtime<'_, Headless>) {
    runtime
        .queue_wheel(MouseScrollDelta::LineDelta(0.0, 3.0))
        .expect("the notch is queued");
    runtime.flush_wheel().expect("and spent");
}

/// Whether a page of this window was handed a wheel notch.
fn page_heard_a_wheel(runtime: &Runtime<'_, Headless>) -> bool {
    runtime.window.web.values().any(|page| {
        page.heard
            .iter()
            .any(|event| matches!(event, bt_platform::WebMouseEvent::Wheel(_)))
    })
}

/// Whether the second shell's view has left its live end.
fn scrolled(runtime: &Runtime<'_, Headless>, seat: SeatId) -> bool {
    runtime.pane(seat).view.projection.is_scrolled()
}

/// RED (T-POINTER-CAPTURE cut 6, R-10) — **no page hears the wheel under a held gesture, and
/// nothing under the page scrolls either**: the page is the layer under the hand, so the notch is
/// its to swallow. Here a page stands shown over a shell with lines to scroll back through; with
/// the hand free the page takes the notch, and with a press held the notch goes nowhere — the
/// shell under the page stays at its live end in both (cells A·w).
///
/// MUTATION: ask the wheel's page station with `point_is_on_the_web_page` (the page minus the held
/// gesture) instead of `page_under_the_hand` — under the held press the notch falls through the
/// page to the shell beneath it, which scrolls.
#[test]
fn no_page_but_the_owner_hears_the_wheel_under_a_capture() {
    for holding in [false, true] {
        let mut folio = a_window("held-wheel");
        let mut runtime = folio.runtime();
        let seat = *runtime
            .sessions
            .keys()
            .nth(1)
            .expect("the tab holds two shells");
        let text: String = (0..40).map(|line| format!("line {line}\r\n")).collect();
        runtime
            .sessions
            .get_mut(&seat)
            .expect("the shell")
            .session
            .session
            .feed(text.as_bytes())
            .expect("the shell prints");
        runtime
            .publish_frame(bt_render::FrameTrigger {
                occurred_at: Instant::now(),
                source: bt_render::FrameSource::Expose,
            })
            .expect("a headless frame composes");
        runtime.redraw().expect("and reaches the glass");
        let corner = first_point(&mut runtime, |runtime, point| {
            seats::pane_at(&runtime.seat_layout, point.x, point.y) == Some(seat)
        });
        // Well inside the shell's body, below its head.
        let inside = PhysicalPosition::new(corner.x + 100.0, corner.y + 300.0);
        let bounds = WebBounds {
            x: inside.x as i32 - 50,
            y: inside.y as i32 - 50,
            width: 200,
            height: 200,
        };
        runtime.window.web.insert(
            LeafId {
                tab: runtime.id,
                seat: SeatId(99),
            },
            WebSeat::shown_without_an_engine(bounds),
        );
        runtime.pointer_moved(inside).expect("the move is answered");
        if holding {
            a_tab_press(&mut runtime, inside);
        }
        a_notch(&mut runtime);
        assert!(
            !scrolled(&runtime, seat),
            "a notch over a page scrolls nothing under it (held: {holding})"
        );
        assert_eq!(
            page_heard_a_wheel(&runtime),
            !holding,
            "the page hears the notch with the hand free, and not under a held press"
        );
        // And with the page gone the same notch scrolls the shell, so it was the page that
        // held it.
        runtime.window.web.clear();
        a_notch(&mut runtime);
        assert!(
            scrolled(&runtime, seat),
            "with no page over it the shell takes the notch (held: {holding})"
        );
    }
}

/// RED (T-POINTER-CAPTURE cut 6, §7, cell B19) — **a touch pan parked under a held gesture moves
/// nothing**: its opening point would be a pointer move that drives the held gesture to the
/// finger, so it is discarded, and the pointer stays where the hand left it.
///
/// MUTATION: spend the parked pans whatever the slot holds — the pointer jumps to the finger.
#[test]
fn a_pan_under_a_held_gesture_moves_nothing() {
    let mut folio = a_window("held-pan");
    let mut runtime = folio.runtime();
    let hand = PhysicalPosition::new(300.0, 300.0);
    runtime.pointer_moved(hand).expect("the move is answered");
    a_tab_press(&mut runtime, hand);
    runtime
        .window
        .parked_pans
        .borrow_mut()
        .push(bt_platform::PanStep {
            began_at: Some((600, 500)),
            travel: (0, 40),
        });
    runtime
        .spend_parked_pans()
        .expect("the parked pans are spent");
    assert!(
        runtime.window.parked_pans.borrow().is_empty(),
        "the pan is taken off the door"
    );
    assert_eq!(
        runtime.window.pointer_position,
        Some(hand),
        "and the held press does not move to the finger"
    );
}

// ── round 2: the flyout's gate, the glance's gate, and the slot across windows ──

/// One window of one tab with one shell, which wears the corner ghost (its `🗀` is the flyout's
/// trigger), drawn once.
fn a_lone_pane(tag: &str) -> HeadlessFolio {
    let mut folio = HeadlessFolio::with_tabs(tag, vec![cross_tab(1, &["A"])], 0);
    folio.draw();
    folio.ledger.take();
    folio
}

/// The road the intent is asked on: the pointer's own move, or the re-ask once the glass is free.
#[derive(Clone, Copy, Debug)]
enum Road {
    Move,
    Rearm,
}

/// The pointer at `at`, asked on `road`.
fn ask_at(runtime: &mut Runtime<'_, Headless>, at: PhysicalPosition<f64>, road: Road) {
    match road {
        Road::Move => runtime.pointer_moved(at).expect("the move is answered"),
        Road::Rearm => {
            runtime.window.pointer_position = Some(at);
            runtime
                .rearm_hover_intents(Instant::now())
                .expect("the intents are asked again");
        }
    }
}

/// Whether resting on the corner ghost's `🗀` armed the files flyout, with the hand free or
/// holding a press, asked on `road`.
fn the_flyout_arms(holding: bool, road: Road) -> bool {
    let mut folio = a_lone_pane("held-flyout");
    let mut runtime = folio.runtime();
    let trigger = first_point(&mut runtime, |runtime, point| {
        matches!(
            runtime.chrome_target_at(point),
            Some(seats::ChromeTarget::PaneFiles(_))
        )
    });
    if holding {
        a_tab_press(&mut runtime, trigger);
    }
    ask_at(&mut runtime, trigger, road);
    runtime
        .window
        .float
        .take_due(Instant::now() + Duration::from_secs(10))
        .is_some()
}

/// RED (T-POINTER-CAPTURE cut 6, R-7; round 2) — **a held gesture arms no flyout**: a hand
/// resting on a pane's `🗀` arms the files flyout when it is free, on the move and on the re-ask
/// once the glass came free, and arms nothing while it holds a press (cells A·h).
///
/// MUTATION: drop the slot's gate from the flyout's trigger in `pointer_moved` — the move arms
/// it under the held press; drop it in `rearm_hover_intents` — the re-ask does.
#[test]
fn a_held_gesture_arms_no_flyout() {
    for road in [Road::Move, Road::Rearm] {
        assert!(
            the_flyout_arms(false, road),
            "a free hand on the 🗀 arms the flyout ({road:?})"
        );
        assert!(
            !the_flyout_arms(true, road),
            "a hand holding a press arms no flyout ({road:?})"
        );
    }
}

/// What a settling glance card on a files row is after the pointer is asked at a point no row
/// is under, with the hand free or holding a press, on `road`: still standing as it was.
fn the_glance_card_stands(holding: bool, road: Road) -> bool {
    let mut folio = a_lone_pane("held-glance");
    let mut runtime = folio.runtime();
    let away = PhysicalPosition::new(600.0, 500.0);
    runtime.pointer_moved(away).expect("the move is answered");
    let seat = *runtime.sessions.keys().next().expect("the tab's shell");
    runtime.window.file_peek = Some(crate::test_support::card_over(
        crate::RowHost::Terminal(seat),
        std::path::Path::new("notes.md"),
        crate::PeekClock::Settling(Instant::now() + Duration::from_secs(3600)),
    ));
    if holding {
        a_tab_press(&mut runtime, away);
    }
    ask_at(&mut runtime, away, road);
    runtime
        .window
        .file_peek
        .as_ref()
        .is_some_and(|card| card.closing_at.is_none())
}

/// RED (T-POINTER-CAPTURE cut 6, R-7; round 2) — **a held gesture neither arms nor takes down a
/// glance**: a card settling over a row, with the hand gone to where no row is, is let go of when
/// the hand is free — on the move and on the re-ask — and stands as it was while the hand holds a
/// press: frozen, not armed, not lit, not taken down (cells A·h).
///
/// MUTATION: drop the slot's gate from the glance's intent in `pointer_moved` — the held move
/// takes the card down; drop it in `rearm_hover_intents` — the re-ask does.
#[test]
fn a_held_gesture_neither_arms_nor_takes_down_a_glance() {
    for road in [Road::Move, Road::Rearm] {
        assert!(
            !the_glance_card_stands(false, road),
            "a free hand gone from the row lets the settling card go ({road:?})"
        );
        assert!(
            the_glance_card_stands(true, road),
            "a hand holding a press leaves the card as it was ({road:?})"
        );
    }
}

/// One application with two windows, `1` and `2`, each one tab of two shells, drawn once.
fn two_windows(tag: &str) -> crate::headless::HeadlessApplication {
    use crate::headless::{HeadlessApplication, HeadlessParts, Ledger, headless_window};
    let folio = HeadlessFolio::with_tabs(tag, vec![cross_tab(1, &["A", "B"])], 0);
    let second = headless_window(
        &folio.app,
        HeadlessParts::new(&folio.app.gpu, 2, (1200, 800), 1.0, &Ledger::default()),
        vec![cross_tab(2, &["C", "D"])],
        0,
    );
    let mut application = HeadlessApplication::of(folio);
    application
        .folio
        .windows
        .insert(winit::window::WindowId::from(2), second);
    for id in [1, 2] {
        let mut runtime = application
            .folio
            .runtime(winit::window::WindowId::from(id))
            .expect("both windows are open");
        runtime
            .settle_seat_set_change()
            .expect("the window's tabs take their layout");
        runtime
            .publish_frame(bt_render::FrameTrigger {
                occurred_at: Instant::now(),
                source: bt_render::FrameSource::Expose,
            })
            .expect("a headless frame composes");
        runtime.redraw().expect("and reaches the glass");
    }
    application
}

/// Window `id` of `application`.
fn window(
    application: &mut crate::headless::HeadlessApplication,
    id: u64,
) -> Runtime<'_, Headless> {
    application
        .folio
        .runtime(winit::window::WindowId::from(id))
        .expect("the window is open")
}

/// A page shown over window 2's second shell, and a point well inside it.
fn a_page_in_window_two(
    application: &mut crate::headless::HeadlessApplication,
) -> PhysicalPosition<f64> {
    let mut runtime = window(application, 2);
    let seat = *runtime.sessions.keys().nth(1).expect("two shells");
    let corner = first_point(&mut runtime, |runtime, point| {
        seats::pane_at(&runtime.seat_layout, point.x, point.y) == Some(seat)
    });
    let inside = PhysicalPosition::new(corner.x + 100.0, corner.y + 300.0);
    runtime.window.web.insert(
        LeafId {
            tab: runtime.id,
            seat: SeatId(99),
        },
        WebSeat::shown_without_an_engine(WebBounds {
            x: inside.x as i32 - 50,
            y: inside.y as i32 - 50,
            width: 200,
            height: 200,
        }),
    );
    inside
}

/// RED (T-POINTER-CAPTURE cut 6, R-7, revision (k); round 2) — **a gesture held in one window
/// freezes another window's hover**: with a press held in window 1, a rest on window 2's new-tab
/// `⌄` opens no menu and arms no tip, and a page in window 2 under the pointer hears no hover.
/// With nothing held the same rest opens the menu and the page hears the hand, so window 2 is a
/// window whose hover answers.
///
/// MUTATION: make `a_gesture_holds_the_pointer` ask the window's own capture only — window 2's
/// rest opens its menu under window 1's held press, and its page lights.
#[test]
fn a_gesture_held_in_one_window_freezes_the_hover_of_another() {
    for holding in [false, true] {
        let mut application = two_windows("held-across-hover");
        let inside = a_page_in_window_two(&mut application);
        if holding {
            let mut first = window(&mut application, 1);
            let at = PhysicalPosition::new(300.0, 300.0);
            first.pointer_moved(at).expect("the move is answered");
            a_tab_press(&mut first, at);
        }
        let mut second = window(&mut application, 2);
        let chevron = first_point(&mut second, |runtime, point| {
            matches!(
                runtime.chrome_target_at(point),
                Some(seats::ChromeTarget::NewTabMenu)
            )
        });
        second.pointer_moved(chevron).expect("the move is answered");
        let rested = Instant::now() + Duration::from_secs(2);
        second.window.tooltip.activate_if_due(rested);
        let tip = second.window.tooltip.active().is_some();
        second.advance_chevrons(rested).expect("the rest is spent");
        let menu = second.window.profile_menu.is_open();
        second.pointer_moved(inside).expect("the move is answered");
        let page_hover = second.window.web_pointer_inside;
        assert_eq!(
            (menu, tip, page_hover),
            (!holding, !holding, !holding),
            "window 2's menu, tip and page hover with a press held in window 1: {holding}"
        );
    }
}

/// RED (T-POINTER-CAPTURE cut 6, R-10, revision (k); round 2) — **a gesture held in one window
/// leaves another window's page deaf to the wheel**: with a press held in window 1, a notch over
/// a page in window 2 is swallowed — the page hears no wheel and nothing under it scrolls; with
/// nothing held the page hears it.
///
/// MUTATION: make `a_gesture_holds_the_pointer` ask the window's own capture only — window 2's
/// page hears the notch under window 1's held press.
#[test]
fn a_gesture_held_in_one_window_leaves_another_windows_page_deaf_to_the_wheel() {
    for holding in [false, true] {
        let mut application = two_windows("held-across-wheel");
        let inside = a_page_in_window_two(&mut application);
        if holding {
            let mut first = window(&mut application, 1);
            let at = PhysicalPosition::new(300.0, 300.0);
            first.pointer_moved(at).expect("the move is answered");
            a_tab_press(&mut first, at);
        }
        let mut second = window(&mut application, 2);
        second.pointer_moved(inside).expect("the move is answered");
        a_notch(&mut second);
        assert_eq!(
            page_heard_a_wheel(&second),
            !holding,
            "window 2's page hears the notch only when no window holds a press (held in 1: \
             {holding})"
        );
    }
}

// ── 0.4.9 plan row 107: a capture whose button came up does not swallow the finger ──

/// What the platform says about every button, as the window's seam answers it.
fn the_buttons_are(runtime: &Runtime<'_, Headless>, down: bool) {
    let mut answers = runtime.window.window.answers.get();
    answers.buttons_down = down;
    runtime.window.window.answers.set(answers);
}

/// Window `id`'s second shell, given two hundred lines to scroll back through and drawn, and a point
/// well inside it in the window's client pixels.
fn a_shell_with_history(
    application: &mut crate::headless::HeadlessApplication,
    id: u64,
) -> (SeatId, (i32, i32)) {
    let mut runtime = window(application, id);
    let seat = *runtime.sessions.keys().nth(1).expect("two shells");
    let text: String = (0..200).map(|line| format!("line {line}\r\n")).collect();
    runtime
        .sessions
        .get_mut(&seat)
        .expect("the shell")
        .session
        .session
        .feed(text.as_bytes())
        .expect("the shell prints");
    runtime
        .publish_frame(bt_render::FrameTrigger {
            occurred_at: Instant::now(),
            source: bt_render::FrameSource::Expose,
        })
        .expect("a headless frame composes");
    runtime.redraw().expect("and reaches the glass");
    let corner = first_point(&mut runtime, |runtime, point| {
        seats::pane_at(&runtime.seat_layout, point.x, point.y) == Some(seat)
    });
    (seat, (corner.x as i32 + 100, corner.y as i32 + 300))
}

/// A pan parked on window `id` at `at`, one opening step and then `steps` of `travel` each.
fn a_pan_parked(
    application: &mut crate::headless::HeadlessApplication,
    id: u64,
    at: (i32, i32),
    steps: usize,
    travel: i32,
) {
    let runtime = window(application, id);
    let mut parked = runtime.window.parked_pans.borrow_mut();
    parked.push(bt_platform::PanStep {
        began_at: Some(at),
        travel: (0, 0),
    });
    parked.extend((0..steps).map(|_| bt_platform::PanStep {
        began_at: None,
        travel: (0, travel),
    }));
}

/// The pans spent the way the loop spends them, and the wheel burst they made spent too, as the
/// turn's end spends it; then how far window `id`'s `seat` has scrolled back, in subpixels.
fn spent_and_scrolled(
    application: &mut crate::headless::HeadlessApplication,
    id: u64,
    seat: SeatId,
) -> i64 {
    application
        .folio
        .spend_touch_pans()
        .expect("the parked pans are spent");
    let mut runtime = window(application, id);
    runtime.flush_wheel().expect("the wheel burst is spent");
    runtime.pane(seat).view.projection.scroll_offset_subpixels()
}

/// RED (0.4.9 plan row 107, T-TOUCH-DOES-NOTHING-ON-WINDOWS; R8, R9) — **a pan in one window
/// scrolls the pane under the finger when a press in another window lost its release**: window 1
/// holds a tab press whose button the platform says is up, and a 400-pixel downward pan parked
/// on window 2 over a shell with history scrolls that shell back, ending window 1's stale
/// capture first (`cancel`). With the button still down the same pan is discarded and window 1's
/// press stays held — B19 across windows. And twenty steps of 20 pixels scroll exactly as far as
/// one step of 400, so the pan road loses nothing to its step size.
///
/// RED on 340917494: the slot is the application's and B19 discards every parked pan while it
/// holds anything, so window 2's shell does not move.
///
/// MUTATION: drop the button test from `end_a_capture_that_lost_its_pointer` — window 1's press
/// stays held and the pan is discarded; drop `end_the_capture_that_lost_its_pointer` from
/// `spend_touch_pans` — the same.
#[test]
fn a_pan_after_a_press_in_another_window_lost_its_release_scrolls_the_pane_under_the_finger() {
    let mut scrolled_by = Vec::new();
    for (down, steps, travel) in [(true, 20, 20), (false, 20, 20), (false, 1, 400)] {
        let mut application = two_windows("lost-release-across");
        let (seat, at) = a_shell_with_history(&mut application, 2);
        {
            let mut first = window(&mut application, 1);
            let hand = PhysicalPosition::new(300.0, 300.0);
            first.pointer_moved(hand).expect("the move is answered");
            a_tab_press(&mut first, hand);
            the_buttons_are(&first, down);
            first.app.capture_endings.clear();
        }
        a_pan_parked(&mut application, 2, at, steps, travel);
        let offset = spent_and_scrolled(&mut application, 2, seat);
        let second = window(&mut application, 2);
        let held = second
            .app
            .pointer_capture
            .as_ref()
            .map(|capture| capture.owner.name());
        let endings = second.app.capture_endings.clone();
        if down {
            assert_eq!(offset, 0, "a pan under a press still held scrolls nothing");
            assert_eq!(
                (held, endings),
                (Some("TabPress"), Vec::new()),
                "and the held press is left alone"
            );
        } else {
            assert!(
                offset > 0,
                "a pan after a lost release scrolls window 2's shell back ({steps} × {travel})"
            );
            assert_eq!(
                (held, endings),
                (None, vec![("TabPress", "cancel")]),
                "window 1's stale press is cancelled first"
            );
            scrolled_by.push(offset);
        }
    }
    assert_eq!(
        scrolled_by[0], scrolled_by[1],
        "twenty steps of 20 px scroll as far as one of 400 px"
    );
}

/// RED (0.4.9 plan row 107; R8) — **the same in one window**: a tab press held in the window the
/// finger pans, whose button the platform says is up, is cancelled and the pan scrolls the shell
/// under the finger.
///
/// RED on 340917494: B19 discards the pan; the system's capture sample still reads "held".
///
/// MUTATION: drop the button test from `end_a_capture_that_lost_its_pointer` — nothing scrolls.
#[test]
fn a_pan_after_a_press_in_its_own_window_lost_its_release_scrolls_the_pane_under_the_finger() {
    let mut application = two_windows("lost-release-own");
    let (seat, at) = a_shell_with_history(&mut application, 2);
    {
        let mut second = window(&mut application, 2);
        let hand = PhysicalPosition::new(300.0, 300.0);
        second.pointer_moved(hand).expect("the move is answered");
        a_tab_press(&mut second, hand);
        the_buttons_are(&second, false);
    }
    a_pan_parked(&mut application, 2, at, 20, 20);
    let offset = spent_and_scrolled(&mut application, 2, seat);
    assert!(offset > 0, "the pan scrolls the shell under the finger");
    assert!(
        window(&mut application, 2).app.pointer_capture.is_none(),
        "and the stale press is gone"
    );
}

/// RED (0.4.9 plan row 107; R8) — **the per-turn check alone ends a capture whose button came
/// up**, with no pan and no press to prove it: the turn's resize door asks the platform, and a
/// tab press whose button is up is cancelled (`cancel`, in the endings ledger), while one whose
/// button is down is left held. The system's own capture sample is unchanged in both, so the
/// button is the only thing that differs.
///
/// RED on 340917494: the door compared only the system's capture, which a lost release leaves
/// where winit put it.
///
/// MUTATION: drop the button test from `end_a_capture_that_lost_its_pointer` — the press whose
/// button is up stays held.
#[test]
fn a_capture_whose_button_came_up_is_ended_by_the_next_turn() {
    for down in [true, false] {
        let mut folio = a_window("lost-release-turn");
        let mut runtime = folio.runtime();
        let hand = PhysicalPosition::new(300.0, 300.0);
        runtime.pointer_moved(hand).expect("the move is answered");
        a_tab_press(&mut runtime, hand);
        the_buttons_are(&runtime, down);
        runtime.app.capture_endings.clear();
        runtime
            .flush_pending_pty_resize(Instant::now())
            .expect("the turn's resize door runs");
        let held = runtime
            .app
            .pointer_capture
            .as_ref()
            .map(|capture| capture.owner.name());
        let expected = if down {
            (Some("TabPress"), Vec::new())
        } else {
            (None, vec![("TabPress", "cancel")])
        };
        assert_eq!(
            (held, runtime.app.capture_endings.clone()),
            expected,
            "the turn with the button down: {down}"
        );
    }
}

/// RED (0.4.9 plan row 107; R2's third line) — **a pan discarded under a held gesture says so
/// once in the process**: the first discard writes the line naming the gesture, and every later
/// one writes nothing; a turn with no pan parked discards nothing and says nothing.
///
/// MUTATION: drop the line from `spend_parked_pans` — the process never learns a discard
/// happened; drop the `said` memory — every discard writes it again.
#[test]
fn a_pan_discarded_under_a_held_gesture_says_so_once() {
    let mut said = false;
    assert_eq!(
        crate::runtime::pointer::touch::a_discarded_pan_said_once(&mut said, "TabPress"),
        Some("a touch pan was discarded under a held gesture (owner=TabPress)".to_owned()),
        "the first discard is said, with its owner"
    );
    assert_eq!(
        crate::runtime::pointer::touch::a_discarded_pan_said_once(&mut said, "TabPress"),
        None,
        "and no later one"
    );

    let mut folio = a_window("held-pan-said");
    let said_lines = |folio: &HeadlessFolio| -> Vec<String> {
        folio
            .ledger
            .take()
            .into_iter()
            .filter_map(|effect| match effect {
                crate::headless::WindowEffect::Noted(line) if line.contains("touch pan") => {
                    Some(line)
                }
                _ => None,
            })
            .collect()
    };
    let pan = bt_platform::PanStep {
        began_at: Some((600, 500)),
        travel: (0, 40),
    };
    {
        let mut runtime = folio.runtime();
        let hand = PhysicalPosition::new(300.0, 300.0);
        runtime.pointer_moved(hand).expect("the move is answered");
        a_tab_press(&mut runtime, hand);
        runtime
            .spend_parked_pans()
            .expect("nothing parked is nothing spent");
    }
    assert_eq!(
        said_lines(&folio),
        Vec::<String>::new(),
        "a turn with no pan parked discards nothing and says nothing"
    );
    for expected in [
        vec!["a touch pan was discarded under a held gesture (owner=TabPress)".to_owned()],
        Vec::new(),
    ] {
        {
            let mut runtime = folio.runtime();
            runtime.window.parked_pans.borrow_mut().push(pan);
            runtime
                .spend_parked_pans()
                .expect("the parked pans are spent");
        }
        assert_eq!(
            said_lines(&folio),
            expected,
            "the first discard under the held press is said, and the second is not"
        );
    }
}

/// RED (0.4.9 plan row 107, review F7) — **a gesture ended because its button was up says so
/// once in the process**: the first such ending writes the line naming the gesture, a gesture
/// whose button is still down writes nothing, and a second gesture ended the same way writes
/// nothing either.
///
/// MUTATION: drop the line from `end_a_capture_that_lost_its_pointer` — the owner's log never
/// shows that a release was lost; drop the `said` memory — every ending writes it again.
#[test]
fn a_capture_ended_because_its_button_was_up_says_so_once() {
    let mut said = false;
    assert_eq!(
        crate::runtime::pointer::ending::a_capture_ended_by_its_button_said_once(
            &mut said, "TabPress"
        ),
        Some("a held gesture was ended because its button was up (owner=TabPress)".to_owned()),
        "the first ending is said, with its owner"
    );
    assert_eq!(
        crate::runtime::pointer::ending::a_capture_ended_by_its_button_said_once(
            &mut said, "TabPress"
        ),
        None,
        "and no later one"
    );

    let mut folio = a_window("ended-by-its-button-said");
    let said_lines = |folio: &HeadlessFolio| -> Vec<String> {
        folio
            .ledger
            .take()
            .into_iter()
            .filter_map(|effect| match effect {
                crate::headless::WindowEffect::Noted(line)
                    if line.contains("held gesture was ended") =>
                {
                    Some(line)
                }
                _ => None,
            })
            .collect()
    };
    let said_line =
        vec!["a held gesture was ended because its button was up (owner=TabPress)".to_owned()];
    for (down, expected) in [(true, Vec::new()), (false, said_line), (false, Vec::new())] {
        {
            let mut runtime = folio.runtime();
            let hand = PhysicalPosition::new(300.0, 300.0);
            runtime.pointer_moved(hand).expect("the move is answered");
            if runtime.app.pointer_capture.is_none() {
                a_tab_press(&mut runtime, hand);
            }
            the_buttons_are(&runtime, down);
            runtime
                .flush_pending_pty_resize(Instant::now())
                .expect("the turn's resize door runs");
        }
        assert_eq!(
            said_lines(&folio),
            expected,
            "the turn with the button down: {down}"
        );
    }
}
