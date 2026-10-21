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

/// Whether the second shell's view has left its live end.
fn scrolled(runtime: &Runtime<'_, Headless>, seat: SeatId) -> bool {
    runtime.leaf(seat).projection.is_scrolled()
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
        let text: String = (0..40)
            .map(|line| {
                format!(
                    "line {line}
"
                )
            })
            .collect();
        runtime
            .sessions
            .get_mut(&seat)
            .expect("the shell")
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
