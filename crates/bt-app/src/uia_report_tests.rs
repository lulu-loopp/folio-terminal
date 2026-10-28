//! What a window publishes to UI Automation, and when (T-UIA-SKELETON, 88a).

use super::{PaneKey, UiaReport};
use crate::headless::{HeadlessFolio, HeadlessUia};
use crate::test_support::{cross_tab, leaf_saying, tab_holding};

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
    let mut report = UiaReport::<HeadlessUia>::new(None);
    let keys = [key(1, true)];
    report.presented = true;
    assert!(!report.due(false, &keys), "nobody asked: nothing is built");
    assert!(report.due(true, &keys), "the first question publishes");
    assert!(!report.due(true, &keys), "an idle turn builds nothing");
    report.presented = true;
    assert!(report.due(true, &keys), "a frame reached the glass");
    assert!(!report.due(true, &keys), "and the bit is spent");
    assert!(report.due(true, &[key(1, false)]), "the keyboard moved");
    assert!(
        report.due(true, &[key(1, false), key(2, true)]),
        "a pane came"
    );
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
    let mut folio = HeadlessFolio::with_tabs(
        "uia-publication",
        vec![tab_holding(leaf_saying("门 door"))],
        0,
    );
    folio.window.uia.presented = false;
    folio.draw();
    assert!(
        folio.window.uia.presented,
        "a present owes the next publication"
    );
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
    assert_eq!(
        pane.caret(),
        Some(6),
        "after door: 门, the space and four letters"
    );
    let width = f64::from(runtime.sessions[&seat].view.presented_metrics.cell_width_px);
    let glyph = pane.rectangles(0, 1);
    assert_eq!(glyph.len(), 1);
    assert!(
        (glyph[0].width - 2.0 * width).abs() < 1e-6,
        "门 covers two cells: {glyph:?}"
    );
}

/// The window's recorder: the headless window's answer to UI Automation.
fn recorder(folio: &HeadlessFolio) -> &HeadlessUia {
    folio
        .window
        .uia
        .hook
        .as_ref()
        .expect("a headless window answers through its recorder")
}

/// RED (88a) — **nothing is built until a client asks**: a hundred turns of frames publish
/// nothing while no client has asked; the first turn after the question publishes the pane.
///
/// MUTATION: drop the `!wanted` return in `Runtime::flush_uia` and the `!wanted` arm of
/// `UiaReport::due`.
#[test]
fn nothing_is_built_until_automation_asks() {
    let mut folio =
        HeadlessFolio::with_tabs("uia-lazy", vec![tab_holding(leaf_saying("quiet"))], 0);
    for _ in 0..100 {
        folio.draw();
        folio.runtime().flush_uia().expect("a turn's clock run");
    }
    assert!(recorder(&folio).published().is_empty(), "nobody asked");
    recorder(&folio).ask();
    folio.runtime().flush_uia().expect("a turn's clock run");
    let published = recorder(&folio).published();
    assert_eq!(
        published.len(),
        1,
        "the first turn after the question publishes"
    );
    assert_eq!(published[0].panes.len(), 1);
}

/// RED (88a) — **a move of the keyboard between two idle panes publishes, with no frame owed**:
/// the move is in the keys the frames do not carry, so the publication does not wait for a
/// present; and a turn after it that changes nothing publishes nothing.
///
/// MUTATION: gate `UiaReport::due` on `presented` alone.
#[test]
fn a_focus_only_change_publishes_without_a_frame() {
    let mut folio = HeadlessFolio::with_tabs("uia-focus-only", vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    recorder(&folio).ask();
    folio.runtime().flush_uia().expect("a turn's clock run");
    let terminals = folio.runtime().seats.terminals();
    let first = recorder(&folio).published();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].focused(), Some(terminals[0].0));
    folio
        .runtime()
        .focus_seat(terminals[1])
        .expect("the pane chord's door");
    // The focus door may repaint; this turn is held to what the frames do not say, so the
    // present it made is spent before the clock run.
    folio.window.uia.presented = false;
    folio.runtime().flush_uia().expect("a turn's clock run");
    let published = recorder(&folio).published();
    assert_eq!(published.len(), 2, "the move publishes");
    assert_eq!(published[1].focused(), Some(terminals[1].0));
    folio.runtime().flush_uia().expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        2,
        "an idle turn publishes nothing"
    );
}

/// RED (88a) — **a client's focus request is the pane chord's focus**: the parked seat is taken
/// on the next clock run and focused through `Runtime::focus_seat`, and the publication says so.
///
/// MUTATION: drop the `focus_seat` call in `Runtime::flush_uia`.
#[test]
fn a_clients_focus_request_focuses_the_pane() {
    let mut folio = HeadlessFolio::with_tabs("uia-set-focus", vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    recorder(&folio).ask();
    let terminals = folio.runtime().seats.terminals();
    recorder(&folio).ask_focus(terminals[1].0);
    folio.runtime().flush_uia().expect("a turn's clock run");
    assert_eq!(folio.runtime().focused_leaf, terminals[1]);
    let published = recorder(&folio).published();
    assert_eq!(
        published
            .last()
            .and_then(bt_platform::uia::WindowText::focused),
        Some(terminals[1].0)
    );
}

/// RED (88a) — **the caret is published where the input method is told it, in the window's
/// axis**: in a row of two panes, the right pane's caret stands at its own body's origin plus the
/// cell arithmetic — the point `window_ime_cursor_area` gives the input method for that pane.
///
/// MUTATION: start `pane_text`'s rows at `padding` alone (forgetting `body.x`).
#[test]
fn the_snapshot_caret_carries_the_seat_origin() {
    let mut folio = HeadlessFolio::with_tabs("uia-origin", vec![cross_tab(1, &["A", "B"])], 0);
    let terminals = folio.runtime().seats.terminals();
    folio
        .runtime()
        .focus_seat(terminals[1])
        .expect("the pane chord's door");
    folio.draw();
    let runtime = folio.runtime();
    let keys = runtime.uia_keys();
    let key = keys
        .iter()
        .find(|key| key.seat == terminals[1].0)
        .expect("the right pane is published");
    let body = bt_render::SeatViewport {
        x: key.body[0],
        y: key.body[1],
        width: key.body[2],
        height: key.body[3],
    };
    assert!(
        body.x > 0,
        "the fixture: the right pane does not start at the window's edge"
    );
    let view = &runtime.sessions[&terminals[1]].view;
    let frame = view.last_presented_frame.as_ref().expect("presented");
    assert!(
        frame.cursor_in_view,
        "the fixture: the caret is on the glass"
    );
    let area = crate::runtime::window_ime_cursor_area(
        body,
        bt_render::ime_cursor_area_for_metrics(view.presented_metrics, frame),
    );
    let text = runtime.uia_publication(&keys);
    let pane = text.pane(terminals[1].0).expect("published");
    let caret = pane.caret().expect("the cursor is on the glass");
    let rect = pane.rectangles(caret, caret)[0];
    // Pixels on the glass: far inside `i32`.
    #[allow(clippy::cast_possible_truncation)]
    let at = (rect.left.floor() as i32, rect.top.floor() as i32);
    assert_eq!(
        at,
        (area.x, area.y),
        "the caret the input method is told for this pane"
    );
}

/// RED (88a) — **only the focused shell pane holds the keyboard**: in a row of two, the focused
/// one and not the other; none while the search capsule holds the keyboard; none while the
/// window is unfocused (the touch keyboard and Narrator follow the element that holds it).
///
/// MUTATION: read `holds_keyboard` from `seat == focused` alone in `Runtime::uia_keys`.
#[test]
fn the_focused_pane_reports_keyboard_focus_and_no_other() {
    let mut folio = HeadlessFolio::with_tabs("uia-keyboard", vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    let terminals = folio.runtime().seats.terminals();
    let holding = |folio: &mut HeadlessFolio| -> Vec<u64> {
        let runtime = folio.runtime();
        let keys = runtime.uia_keys();
        assert_eq!(keys.len(), 2, "both panes are published: {keys:?}");
        let holding: Vec<u64> = keys
            .iter()
            .filter(|key| key.holds_keyboard)
            .map(|key| key.seat)
            .collect();
        assert_eq!(
            runtime.uia_publication(&keys).focused(),
            holding.first().copied()
        );
        holding
    };
    assert_eq!(
        holding(&mut folio),
        [terminals[0].0],
        "the focused pane only"
    );
    assert!(folio.window.search.open(terminals[0]), "the capsule opens");
    folio.window.search.focus();
    assert!(folio.window.search.is_focused(), "and holds the keyboard");
    assert_eq!(
        holding(&mut folio),
        Vec::<u64>::new(),
        "the capsule holds it"
    );
    folio.window.search.close();
    assert_eq!(holding(&mut folio), [terminals[0].0], "and gives it back");
    folio.window.window_focused = false;
    assert_eq!(
        holding(&mut folio),
        Vec::<u64>::new(),
        "an unfocused window"
    );
}
