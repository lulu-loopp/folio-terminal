//! **How far a finger's pan moves what is under it** (0.4.9 plan row 117,
//! T-TOUCH-PAN-SCROLLS-TOO-FAR), on a headless window: a pan of N physical pixels moves a shell's
//! scrollback N pixels, a position the system reports twice moves it once, and a program that
//! reads the mouse is sent one wheel report per notch of travel, not one per cell.

use super::*;
use crate::headless::{Headless, HeadlessFolio};
use crate::runtime::mouse::{pixels_per_reported_notch, recoverable_wheel_scroll_amount};
use crate::test_support::cross_tab;
use crate::{BirthWait, LeafSeed, PaneBirth};
use bt_layout::SeatId;

/// One window of one tab with two shells, drawn once.
fn a_window(tag: &str) -> HeadlessFolio {
    let mut folio = HeadlessFolio::with_tabs(tag, vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    folio.ledger.take();
    folio
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

/// The window's second shell, handed `preamble` and then two hundred lines to scroll back
/// through, drawn; and a point well inside it in the window's client pixels.
fn a_shell_with_history(
    runtime: &mut Runtime<'_, Headless>,
    preamble: &[u8],
) -> (SeatId, (i32, i32)) {
    let seat = *runtime.sessions.keys().nth(1).expect("two shells");
    let mut text = preamble.to_vec();
    text.extend((0..200).flat_map(|line| format!("line {line}\r\n").into_bytes()));
    runtime
        .sessions
        .get_mut(&seat)
        .expect("the shell")
        .session
        .session
        .feed(&text)
        .expect("the shell prints");
    runtime
        .publish_frame(bt_render::FrameTrigger {
            occurred_at: Instant::now(),
            source: bt_render::FrameSource::Expose,
        })
        .expect("a headless frame composes");
    runtime.redraw().expect("and reaches the glass");
    let corner = first_point(runtime, |runtime, point| {
        seats::pane_at(&runtime.seat_layout, point.x, point.y) == Some(seat)
    });
    (seat, (corner.x as i32 + 100, corner.y as i32 + 300))
}

/// `seat`'s cell height, in physical pixels.
fn cell_px(runtime: &Runtime<'_, Headless>, seat: SeatId) -> i32 {
    runtime.pane(seat).view.metrics.cell_height_px as i32
}

/// A pan parked at `at`: its opening step, then one step of each of `travels` downwards.
fn a_pan_parked(runtime: &Runtime<'_, Headless>, at: (i32, i32), travels: &[i32]) {
    let mut parked = runtime.window.parked_pans.borrow_mut();
    parked.push(bt_platform::PanStep {
        began_at: Some(at),
        travel: (0, 0),
        inertia: false,
    });
    parked.extend(travels.iter().map(|&travel| bt_platform::PanStep {
        began_at: None,
        travel: (0, travel),
        inertia: false,
    }));
}

/// The parked pans spent the way the loop spends them, and the burst they made spent as the
/// turn's end spends it.
fn spent(runtime: &mut Runtime<'_, Headless>) {
    runtime
        .spend_parked_pans()
        .expect("the parked pans are spent");
    runtime.flush_wheel().expect("and the wheel burst too");
}

/// PIN (0.4.9 plan row 117) — **a pan of N physical pixels moves a shell's scrollback N pixels**,
/// which is N ÷ the cell rows: five cells of finger, in five steps, scroll the view back exactly
/// five rows. Green before row 117 too (§1: the local road was already one for one); it holds the
/// line row 117's edits stand beside, so neither of them can reach the local road.
///
/// MUTATION: put any factor on the local road's pixel arm (`mouse.rs`, `event_subpixels`) —
/// double it and the view moves ten rows.
#[test]
fn a_pan_of_n_pixels_moves_a_shell_pane_n_pixels() {
    let mut folio = a_window("pan-n-pixels");
    let mut runtime = folio.runtime();
    let (seat, at) = a_shell_with_history(&mut runtime, b"");
    let cell = cell_px(&runtime, seat);
    a_pan_parked(&runtime, at, &[cell; 5]);
    spent(&mut runtime);
    let projection = &runtime.pane(seat).view.projection;
    assert_eq!(
        projection.scroll_offset_subpixels(),
        i64::from(5 * cell) * bt_viewport::SUBPIXELS_PER_PX,
        "five cells of finger move the content five cells' worth of pixels"
    );
    assert_eq!(
        projection.scroll_offset_subpixels(),
        5 * projection.cell_height_subpixels().get(),
        "which is five rows"
    );
}

/// PIN (0.4.9 plan row 117) — **a position the system reports twice moves the pane once**: the
/// door's own arithmetic ([`bt_platform::PanTrack`]) answers a repeated position with no step, so
/// a begin and three moves of 40, 0 (the same point again) and 40 pixels scroll the pane 80 pixels
/// back, not 120. Green before row 117 too (§1: no road spends a pan twice).
///
/// MUTATION: measure each step from the begin instead of from the previous message (leave
/// `last` at the begin's point in `PanTrack::step`) — the repeated position is travel again and
/// the pane moves 200 pixels.
#[test]
fn a_pan_position_reported_twice_moves_the_pane_once() {
    let mut folio = a_window("pan-twice");
    let mut runtime = folio.runtime();
    let (seat, at) = a_shell_with_history(&mut runtime, b"");
    let mut track = bt_platform::PanTrack::default();
    let steps: Vec<_> = [(true, 500), (false, 540), (false, 540), (false, 580)]
        .into_iter()
        .filter_map(|(begins, y)| track.step(begins, false, false, (400, y), at))
        .collect();
    runtime.window.parked_pans.borrow_mut().extend(steps);
    spent(&mut runtime);
    assert_eq!(
        runtime.pane(seat).view.projection.scroll_offset_subpixels(),
        80 * bt_viewport::SUBPIXELS_PER_PX,
        "40 + 0 + 40 pixels of finger, 80 pixels of content"
    );
}

/// GUARD (0.4.9 plan row 117) — **one forwarded report stands for one notch of the local road**:
/// the system's lines per notch times the pane's cell, never less than one line, and a whole
/// screen for "one screen at a time".
///
/// MUTATION: answer `cell_px` for every arm (the rule before row 117) — three lines read 20
/// pixels, not 60.
#[test]
fn a_forwarded_report_stands_for_a_notch_of_the_local_road() {
    use bt_platform::WheelScrollAmount::{Lines, Page};
    assert_eq!(pixels_per_reported_notch(Lines(3), 20.0, 30.0), 60.0);
    assert_eq!(pixels_per_reported_notch(Lines(1), 20.0, 30.0), 20.0);
    assert_eq!(
        pixels_per_reported_notch(Lines(0), 20.0, 30.0),
        20.0,
        "a wheel set to scroll nothing still leaves a finger's report a row tall"
    );
    assert_eq!(pixels_per_reported_notch(Page, 20.0, 30.0), 600.0);
}

/// The SGR wheel reports `bytes` holds (`ESC [ < 64 ;` up, `ESC [ < 65 ;` down).
fn wheel_reports(bytes: &[u8]) -> usize {
    bytes
        .windows(6)
        .filter(|window| *window == b"\x1b[<64;" || *window == b"\x1b[<65;")
        .count()
}

/// RED (0.4.9 plan row 117) — **a pan over a program that reads the mouse sends it one wheel
/// report per notch of travel**: six cells of finger over a shell that has turned SGR mouse
/// tracking on are six ÷ (the system's lines per notch) reports, the number a wheel turned that
/// far on the local road would have sent, so a program that steps its rows per report the way the
/// system's lines-per-notch says moves the content about as far as the finger went. The pane is
/// held in birth so the bytes meant for its shell are kept where the test can read them.
///
/// RED on 840cacbb6 (lines per notch 3 on the machine that ran it): six reports, one per cell.
/// On macOS the system's answer is one line a notch, so the count is six there before and after.
///
/// MUTATION: divide the pixel arm of `take_forward_wheel_notches` by the cell again — six reports.
#[test]
fn a_pan_over_a_program_reading_the_mouse_sends_one_report_per_notch_of_travel() {
    let mut folio = a_window("pan-mouse-program");
    let mut runtime = folio.runtime();
    let (seat, at) = a_shell_with_history(&mut runtime, b"\x1b[?1000h\x1b[?1006h");
    runtime
        .sessions
        .get_mut(&seat)
        .expect("the shell")
        .session
        .life
        .begin_birth(PaneBirth {
            seed: LeafSeed::default(),
            probe_input: None,
            typed: std::cell::RefCell::default(),
            waiting: BirthWait::Locale,
        });
    let cell = cell_px(&runtime, seat);
    a_pan_parked(&runtime, at, &[cell; 6]);
    spent(&mut runtime);
    let sent = runtime
        .sessions
        .get(&seat)
        .and_then(|pane| pane.session.life.birth())
        .map(|birth| birth.typed.borrow().clone())
        .expect("the pane is still in birth");
    let lines = match recoverable_wheel_scroll_amount(bt_platform::wheel_scroll_amount()) {
        bt_platform::WheelScrollAmount::Lines(lines) => lines.max(1) as usize,
        bt_platform::WheelScrollAmount::Page => usize::MAX,
    };
    assert_eq!(
        wheel_reports(&sent),
        6 / lines,
        "six cells of finger at {lines} lines a notch: {:?}",
        String::from_utf8_lossy(&sent)
    );
    assert!(
        !runtime.pane(seat).view.projection.is_scrolled(),
        "and the program's pane is the program's: its own view does not move"
    );
}
