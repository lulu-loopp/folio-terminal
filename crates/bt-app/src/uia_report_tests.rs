//! What a window publishes to UI Automation, and when (T-UIA-SKELETON, 88a).

use super::{PaneKey, UiaReport};
use crate::headless::HeadlessFolio;
use crate::test_support::{leaf_saying, tab_holding};

fn key(seat: u64, holds_keyboard: bool) -> PaneKey {
    PaneKey {
        seat,
        body: [0, 0, 100, 100],
        holds_keyboard,
        name: "pwsh".to_owned(),
    }
}

/// RED (88a) — **a turn publishes only when a client has asked, and then only when something it
/// says moved**: nothing before the first question; the first wanted turn; a frame presented; the
/// keyboard moving. A turn that changes none of it builds nothing.
///
/// MUTATION: drop the `!wanted` arm of `UiaReport::due`, or its keys comparison.
#[test]
fn a_turn_publishes_only_what_moved_after_a_client_asked() {
    let mut report = UiaReport::new(None);
    let keys = [key(1, true)];
    report.presented = true;
    assert!(!report.due(false, &keys), "nobody asked: nothing is built");
    assert!(report.due(true, &keys), "the first question publishes");
    assert!(!report.due(true, &keys), "an idle turn builds nothing");
    report.presented = true;
    assert!(report.due(true, &keys), "a frame reached the glass");
    assert!(!report.due(true, &keys), "and the bit is spent");
    assert!(report.due(true, &[key(1, false)]), "the keyboard moved");
    assert!(report.due(true, &[key(1, false), key(2, true)]), "a pane came");
}

/// RED (88a) — **the publication is the pane as it was presented**: after a frame reaches the
/// glass, the window owes a publication, and it names the pane by its title, says it holds the
/// keyboard while the window has the focus, carries the frame's text — a wide glyph one unit over
/// two cells — and puts the caret where the frame drew it.
///
/// MUTATION: build the rows from the live grid rather than the presented frame, or drop the
/// `presented` write in `Runtime::redraw`.
#[test]
fn the_publication_is_the_presented_pane() {
    let mut folio =
        HeadlessFolio::with_tabs("uia-publication", vec![tab_holding(leaf_saying("门 door"))], 0);
    folio.window.uia.presented = false;
    folio.draw();
    assert!(folio.window.uia.presented, "a present owes the next publication");
    let runtime = folio.runtime();
    let keys = runtime.uia_keys();
    assert_eq!(keys.len(), 1, "the one terminal: {keys:?}");
    let seat = bt_layout::SeatId(keys[0].seat);
    assert!(keys[0].holds_keyboard, "the window has the focus");
    assert_eq!(keys[0].name, runtime.pane_title(seat));
    let text = runtime.uia_publication(&keys);
    let pane = text.pane(seat.0).expect("the pane is published");
    assert!(
        pane.text(0, pane.len(), -1).starts_with("门 door"),
        "{:?}",
        pane.text(0, pane.len(), -1)
    );
    assert_eq!(pane.caret(), Some(6), "after door: 门, the space and four letters");
    let width = f64::from(runtime.sessions[&seat].view.presented_metrics.cell_width_px);
    let glyph = pane.rectangles(0, 1);
    assert_eq!(glyph.len(), 1);
    assert!(
        (glyph[0].width - 2.0 * width).abs() < 1e-6,
        "门 covers two cells: {glyph:?}"
    );
}

/// RED (88a) — **a window without the focus publishes no pane holding the keyboard** (the touch
/// keyboard and Narrator follow the element that holds it).
#[test]
fn an_unfocused_window_publishes_no_keyboard() {
    let mut folio =
        HeadlessFolio::with_tabs("uia-unfocused", vec![tab_holding(leaf_saying("x"))], 0);
    folio.draw();
    folio.window.window_focused = false;
    let runtime = folio.runtime();
    let keys = runtime.uia_keys();
    assert!(keys.iter().all(|key| !key.holds_keyboard), "{keys:?}");
    assert_eq!(runtime.uia_publication(&keys).focused(), None);
}

/// RED (88a) — **a window whose subclass did not install builds nothing at all**: the clock run's
/// step returns at once and the owed bit stays where it is.
#[test]
fn a_window_with_no_hook_builds_nothing() {
    let mut folio = HeadlessFolio::with_tabs("uia-no-hook", vec![tab_holding(leaf_saying("x"))], 0);
    folio.draw();
    assert!(folio.window.uia.hook.is_none(), "a headless window installs nothing");
    folio.runtime().flush_uia().expect("nothing to do");
    assert!(folio.window.uia.presented, "nothing was built, so nothing was spent");
}
