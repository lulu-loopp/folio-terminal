//! What a window publishes to UI Automation, and when (T-UIA-SKELETON, 88a).

use std::time::{Duration, Instant};

use super::{PaneKey, UIA_FRAME_INTERVAL, UiaReport};
use crate::headless::{HeadlessFolio, HeadlessUia};
use crate::test_support::{cross_tab, leaf_saying, tab_holding};
use bt_platform::uia::{ClientThreads, PaneId};

fn key(seat: u64, holds_keyboard: bool) -> PaneKey {
    PaneKey {
        id: PaneId { tab: 1, seat },
        body: [0, 0, 100, 100],
        holds_keyboard,
        name: "pwsh".to_owned(),
    }
}

/// RED (88a) — **what the frames do not say is published on its own turn**: the first look, then
/// nothing while the keys stand still and no frame came, then each move of the keys at once — the
/// keyboard, a pane coming — however soon after the last publication.
///
/// MUTATION: drop the keys comparison of `UiaReport::due`, or hold a key change for the frames'
/// interval.
#[test]
fn a_key_change_publishes_at_once_and_an_idle_turn_looks_at_nothing() {
    let mut report = UiaReport::<HeadlessUia>::new(None);
    let now = Instant::now();
    let keys = [key(1, true)];
    assert!(report.looks(&keys, now), "nothing published yet");
    assert!(report.due(&keys, &[], now), "the first look publishes");
    assert!(!report.looks(&keys, now), "an idle turn looks at nothing");
    let moved = [key(1, false)];
    assert!(report.looks(&moved, now), "the keyboard moved");
    assert!(report.due(&moved, &[], now), "and is published at once");
    let more = [key(1, false), key(2, true)];
    assert!(report.due(&more, &[], now), "a pane came");
    assert_eq!(report.deadline(), None, "nothing is held");
}

/// The text of the last publication's first pane.
fn last_text(folio: &HeadlessFolio) -> String {
    let published = recorder(folio).published();
    let pane = &published.last().expect("published").panes[0];
    pane.text(0, pane.len(), -1)
}

/// RED (the review's F2) — **a frame that says nothing new builds nothing, and new text is
/// published at most once an interval**: a present of the same text is looked at and dropped; new
/// text a second after the last publication is published at once; more text within the interval
/// after that is held, with the interval's end as its deadline, and published at it with no
/// further frame; a turn after that builds nothing.
///
/// MUTATION: answer `true` from `UiaReport::due` whenever a frame was presented, or publish a
/// changed frame before `UIA_FRAME_INTERVAL` has passed.
#[test]
fn an_unchanged_frame_builds_nothing_and_new_text_is_paced() {
    let mut folio =
        HeadlessFolio::with_tabs("uia-paced", vec![tab_holding(leaf_saying("quiet"))], 0);
    folio.draw();
    recorder(&folio).ask();
    let start = Instant::now();
    folio
        .runtime()
        .flush_uia(start)
        .expect("a turn's clock run");
    assert_eq!(recorder(&folio).published().len(), 1, "the first look");
    let seat = folio.runtime().seats.terminals()[0];
    let feed = |folio: &mut HeadlessFolio, bytes: &[u8]| {
        folio
            .runtime()
            .sessions
            .get_mut(&seat)
            .expect("the pane")
            .session
            .feed(bytes)
            .expect("feed the shell");
        folio.draw();
    };

    let second = start + Duration::from_secs(1);
    folio.draw();
    assert!(
        folio.window.uia.presented,
        "the fixture: a frame reached the glass"
    );
    folio
        .runtime()
        .flush_uia(second)
        .expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        1,
        "the same text again builds nothing"
    );
    assert!(!folio.window.uia.presented, "the frame was looked at");

    feed(&mut folio, b" loud");
    folio
        .runtime()
        .flush_uia(second)
        .expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        2,
        "new text a second after the last publication goes at once"
    );
    assert!(
        last_text(&folio).contains("quiet loud"),
        "{:?}",
        last_text(&folio)
    );

    feed(&mut folio, b" again");
    let soon = second + Duration::from_millis(10);
    folio.runtime().flush_uia(soon).expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        2,
        "more text within the interval is held"
    );
    let due = second + UIA_FRAME_INTERVAL;
    assert_eq!(folio.window.uia.deadline(), Some(due), "and books its wake");
    folio
        .runtime()
        .flush_uia(second + Duration::from_millis(50))
        .expect("a turn's clock run");
    assert_eq!(recorder(&folio).published().len(), 2, "not yet");
    folio.runtime().flush_uia(due).expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        3,
        "at the deadline, with no further frame"
    );
    assert!(
        last_text(&folio).contains("loud again"),
        "{:?}",
        last_text(&folio)
    );
    assert_eq!(folio.window.uia.deadline(), None);
    folio
        .runtime()
        .flush_uia(due + UIA_FRAME_INTERVAL)
        .expect("a turn's clock run");
    assert_eq!(recorder(&folio).published().len(), 3, "and nothing after");
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
    let seat = bt_layout::SeatId(keys[0].id.seat);
    assert!(keys[0].holds_keyboard, "the window has the focus");
    assert_eq!(keys[0].name, runtime.pane_title(seat));
    let text = runtime.uia_publication(&keys);
    let pane = text.pane(keys[0].id).expect("the pane is published");
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
    let now = Instant::now();
    for _ in 0..100 {
        folio.draw();
        folio.runtime().flush_uia(now).expect("a turn's clock run");
    }
    assert!(recorder(&folio).published().is_empty(), "nobody asked");
    recorder(&folio).ask();
    folio.runtime().flush_uia(now).expect("a turn's clock run");
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
    let now = Instant::now();
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    let terminals = folio.runtime().seats.terminals();
    let first = recorder(&folio).published();
    assert_eq!(first.len(), 1);
    let seat_of = |text: &bt_platform::uia::WindowText| text.focused().map(|id| id.seat);
    assert_eq!(seat_of(&first[0]), Some(terminals[0].0));
    folio
        .runtime()
        .focus_seat(terminals[1])
        .expect("the pane chord's door");
    // The focus door may repaint; this turn is held to what the frames do not say, so the
    // present it made is spent before the clock run.
    folio.window.uia.presented = false;
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    let published = recorder(&folio).published();
    assert_eq!(published.len(), 2, "the move publishes");
    assert_eq!(seat_of(&published[1]), Some(terminals[1].0));
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    assert_eq!(
        recorder(&folio).published().len(),
        2,
        "an idle turn publishes nothing"
    );
}

/// RED (88a) — **a client's focus request is the pane chord's focus**: the parked seat is taken
/// on the next clock run and focused through `Runtime::focus_seat`, and the publication says so;
/// a request naming that seat in another tab (the review's F1) focuses nothing.
///
/// MUTATION: drop the `focus_seat` call in `Runtime::flush_uia`, or its tab comparison.
#[test]
fn a_clients_focus_request_focuses_the_pane() {
    let mut folio = HeadlessFolio::with_tabs("uia-set-focus", vec![cross_tab(1, &["A", "B"])], 0);
    folio.draw();
    recorder(&folio).ask();
    let terminals = folio.runtime().seats.terminals();
    let tab = folio.runtime().id.0;
    let now = Instant::now();
    recorder(&folio).ask_focus(PaneId {
        tab: tab + 1,
        seat: terminals[1].0,
    });
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    assert_eq!(
        folio.runtime().focused_leaf,
        terminals[0],
        "another tab's pane of that seat is not this one"
    );
    let asked = PaneId {
        tab,
        seat: terminals[1].0,
    };
    recorder(&folio).ask_focus(asked);
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    assert_eq!(folio.runtime().focused_leaf, terminals[1]);
    let published = recorder(&folio).published();
    assert_eq!(
        published
            .last()
            .and_then(bt_platform::uia::WindowText::focused),
        Some(asked)
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
        .find(|key| key.id.seat == terminals[1].0)
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
    let pane = text.pane(key.id).expect("published");
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
            .map(|key| key.id.seat)
            .collect();
        assert_eq!(
            runtime.uia_publication(&keys).focused().map(|id| id.seat),
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

/// RED (the review's F1) — **a pane is named by its tab and its seat**: two single-pane tabs each
/// hold a seat 1; switching from one to the other publishes the other tab's pane under another
/// identity and owes a focus event for it, so a client hears that the keyboard moved.
///
/// MUTATION: name `PaneKey`'s pane by its seat alone in `Runtime::uia_keys`.
#[test]
fn switching_tabs_moves_the_focus_to_another_element() {
    let mut folio = HeadlessFolio::with_tabs(
        "uia-tabs",
        vec![cross_tab(1, &["A"]), cross_tab(2, &["B"])],
        0,
    );
    folio.draw();
    recorder(&folio).ask();
    let now = Instant::now();
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    folio
        .runtime()
        .activate_tab(1, false)
        .expect("the tab strip's door");
    folio.draw();
    folio.runtime().flush_uia(now).expect("a turn's clock run");
    let owed = recorder(&folio).owed();
    assert_eq!(owed.len(), 2, "one focus event per tab: {owed:?}");
    assert_eq!(
        owed[0].seat, owed[1].seat,
        "the fixture: both tabs' panes are the same seat"
    );
    assert_ne!(owed[0], owed[1], "two panes, two identities");
    let published = recorder(&folio).published();
    assert_eq!(
        published
            .last()
            .and_then(bt_platform::uia::WindowText::focused),
        Some(owed[1])
    );
}

/// RED (the review's F6) — **an owed focus event reaches the events lane and is raised there**:
/// the publication's event goes through `App::uia_events` to its thread, which raises it; once
/// the application is gone, the one event raised is the one owed.
///
/// MUTATION: drop the `uia_events.request` call in `Runtime::flush_uia`.
#[test]
fn an_owed_focus_event_is_raised_on_the_events_lane() {
    let mut folio =
        HeadlessFolio::with_tabs("uia-events", vec![tab_holding(leaf_saying("ready"))], 0);
    folio.draw();
    let heard = recorder(&folio).listen();
    recorder(&folio).ask();
    folio
        .runtime()
        .flush_uia(Instant::now())
        .expect("a turn's clock run");
    let owed = recorder(&folio).owed();
    assert_eq!(owed.len(), 1, "the first focus is owed");
    // Every sender but the raised event's goes with the application: the receiver then holds what
    // the lane's thread raised, and ends.
    drop(folio);
    let raised: Vec<PaneId> = heard.iter().collect();
    assert_eq!(raised, owed, "raised on the lane, once");
}

/// RED (the review's F3) — **the thread of a client's first call is said once**: one line in
/// `diagnostics.log` with both thread numbers, and none on the turns after.
///
/// MUTATION: drop the `threads_said` guard in `Runtime::flush_uia`.
#[test]
fn a_clients_first_thread_is_said_once() {
    let mut folio = HeadlessFolio::with_tabs("uia-threads", vec![tab_holding(leaf_saying("x"))], 0);
    let _ = folio.ledger.take();
    recorder(&folio).called_from(ClientThreads {
        window: 7,
        first_call: 9,
    });
    for _ in 0..3 {
        folio
            .runtime()
            .flush_uia(Instant::now())
            .expect("a turn's clock run");
    }
    let said: Vec<String> = folio
        .ledger
        .take()
        .into_iter()
        .filter_map(|effect| match effect {
            crate::headless::WindowEffect::Noted(line) if line.contains("UI Automation") => {
                Some(line)
            }
            _ => None,
        })
        .collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("thread 9") && said[0].contains("is 7"),
        "{said:?}"
    );
}
