//! **No clock in the wake fold is left in the past by the turn it comes due on**
//! (T-WINDOW-THREAD-UNDER-LOAD).
//!
//! The loop parks with `ControlFlow::WaitUntil(the fold's earliest instant)`, and an instant that
//! is already due when it parks is no wait at all: winit returns at once and the loop turns again,
//! thousands of times a second, until something else moves the clock. The owner's log of
//! 2026-10-18 held a deadline fixed in the past for minutes at a time while the window thread
//! turned some 13 500 times a second. Every case below drives one named entry of the fold to its
//! due instant and through the turn that sees it due, and then asks the fold that turn returned:
//! nothing in it may be at or before the turn's own instant, whichever clock it is — the earliest
//! instant is the one the loop parks until, so "the earliest is ahead" is "every entry is ahead".

use std::time::{Duration, Instant};

use crate::headless::HeadlessFolio;
use crate::test_support::{card_over, cross_tab, leaf_saying, tab_holding};
use crate::wake_owner::WakeOwner;
use crate::{PeekClock, RowHost, TabId, foreground_program};

/// **The class's one assertion**: the wake a turn at `now` booked is ahead of `now`, or absent.
fn books_nothing_due(wake: Option<(WakeOwner, Instant)>, now: Instant) {
    if let Some((owner, at)) = wake {
        assert!(
            at > now,
            "the turn handed the loop {owner:?}'s instant {} us in the past — a wait that is no \
             wait, and a loop that spins on it",
            now.saturating_duration_since(at).as_micros()
        );
    }
}

/// A turn of the window at `now`, as the loop takes it, and the wake it booked.
fn turn(folio: &mut HeadlessFolio, now: Instant) -> Option<(WakeOwner, Instant)> {
    folio
        .runtime()
        .turn(now, true)
        .expect("the turn is answered")
}

/// RED — **a frame candidate's foreground-program cadence books no wake, and a turn asks the
/// worker nothing for a pane that publishes nothing.**
///
/// The cadence is a gate on the publish of a pane's own output: `dispatch_tab_decoration_tasks`
/// asks `Cadence::should_request` with the publish's instant. The fold booked its five-second
/// instant for every pane of every tab, and no turn spends it, so a frame candidate (an agent's
/// boxed TUI) in a background tab left that instant in the past and the loop spun on it until
/// the tab published — the entry the owner's blocks fit. Two halves, both held: the turn at the
/// due instant books nothing stale, and the pane's cadence is exactly where it was — no request
/// minted for a pane in a tab that is not on screen, as shipped (a request mints by moving the
/// cadence's next instant on; this fixture's pane has no shell, so nothing would leave the
/// window either way, and the cadence is the counter).
///
/// MUTATIONS: book `foreground_program::deadline` over every leaf in `Runtime::turn`'s fold — the
/// turn hands the loop `ForegroundProgram`; ask `foreground_program::request` for every pane on
/// every turn — the background pane's cadence moves on.
#[test]
fn a_background_frame_candidates_cadence_books_no_wake_and_asks_nothing() {
    let mut framed = tab_holding(leaf_saying("ab │ cd\r\nab │ cd\r\nab │ cd\r\n"));
    framed.id = TabId(2);
    let front = tab_holding(leaf_saying("plain\r\n"));
    let mut folio = HeadlessFolio::with_tabs("wake-fold-foreground", vec![front, framed], 1);
    // The pane's first size, told and quiet: the window's own turns, a generous interval apart,
    // until the resize its layout opened has closed — a pane decorates nothing before that, and
    // the capture that finds the frame candidate is a decoration pass.
    let mut now = Instant::now();
    folio.draw();
    for _ in 0..8 {
        if !folio.window.tabs[1]
            .leaves()
            .any(|(_, leaf)| leaf.session.resize_transaction_open())
        {
            break;
        }
        now += Duration::from_millis(250);
        turn(&mut folio, now);
        folio.draw();
    }
    // Two frames: the first capture finds the candidate, the next publish arms its cadence.
    folio.draw();
    folio.draw();
    assert!(
        folio.window.tabs[1]
            .leaves()
            .any(|(_, leaf)| leaf.session.foreground_program_probe_is_armed()),
        "the fixture's own precondition: the boxed rows are a frame candidate"
    );
    let due = folio.window.tabs[1]
        .leaves()
        .find_map(|(_, leaf)| foreground_program::deadline(leaf))
        .expect("the framed pane's first published frame arms its cadence");
    folio
        .runtime()
        .activate_tab(0, false)
        .expect("the plain tab comes forward");
    // The window's own clocks, without the application's: the turn runs five seconds ahead of
    // the real clock the heartbeat's allowance is taken on, and the web engine's warm-up — a
    // deferrable unit, asked only while that allowance lasts — would be judged against an
    // allowance from another instant.
    let wake = folio
        .runtime()
        .turn(due, false)
        .expect("the turn is answered");
    books_nothing_due(wake, due);
    let after = folio.window.tabs[1]
        .leaves()
        .find_map(|(_, leaf)| foreground_program::deadline(leaf));
    assert_eq!(
        after,
        Some(due),
        "a turn mints no request for a pane in a tab not on screen: its cadence stays put"
    );
}

/// RED — **a pane menu's matured rest is spent when the pointer has left the window.**
///
/// The rest on a heading matures in `advance_pane_menu`, which asked the pointer which row it is
/// on; with the pointer gone (`CursorLeft` clears it) the answer was nothing and the hold was left
/// standing, an instant in the past, until the next pointer move over the window.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_pane_menu` — the turn hands the
/// loop `PaneMenu`.
#[test]
fn a_pane_menus_matured_rest_is_spent_with_the_pointer_gone() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-pane-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let seat = *folio.window.tabs[0]
        .sessions
        .keys()
        .next()
        .expect("the one pane");
    folio
        .runtime()
        .open_pane_menu(seat, [40.0, 40.0])
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    folio
        .window
        .pane_menu
        .as_mut()
        .expect("the menu is up")
        .submenu_hold_until = Some(due);
    assert!(folio.window.pointer_position.is_none());
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a terminal menu's matured rest is spent when its child cannot open** — a class guard
/// over a synthetic state.
///
/// `set_term_submenu(true)` declines a menu whose pane is not alone on the stage (`lone` false),
/// and the hold that asked it was left standing. The product cannot reach that pairing today —
/// the `Split with` heading is drawn only when `lone` — so the state is set by hand: the guard
/// is that `advance_term_menu` spends a matured hold whatever its roads do next, as the pane
/// menu's and the tab menu's do on roads the product does reach.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_term_menu`.
#[test]
fn a_terminal_menus_matured_rest_is_spent_when_its_child_cannot_open() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-term-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let seat = *folio.window.tabs[0]
        .sessions
        .keys()
        .next()
        .expect("the one pane");
    folio
        .runtime()
        .open_term_menu_at(seat, winit::dpi::PhysicalPosition::new(40.0, 40.0))
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    {
        let runtime = folio.runtime();
        let menu = runtime.window.term_menu.as_mut().expect("the menu is up");
        menu.lone = false;
        menu.submenu_hold_until = Some(due);
    }
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a tab menu's matured rest is spent when there is no other window to list.**
///
/// `set_tab_submenu(true)` declines while no other window exists, and the hold that asked it was
/// left standing.
///
/// MUTATION: drop the `menu.submenu_hold_until = None` in `advance_tab_menu`.
#[test]
fn a_tab_menus_matured_rest_is_spent_with_no_other_window() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-tab-menu",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let tab = folio.window.tabs[0].id;
    folio
        .runtime()
        .open_tab_menu_at(tab, winit::dpi::PhysicalPosition::new(40.0, 10.0))
        .expect("the menu opens");
    let due = Instant::now() + Duration::from_millis(250);
    folio
        .window
        .tab_menu
        .as_mut()
        .expect("the menu is up")
        .submenu_hold_until = Some(due);
    assert!(folio.runtime().other_window_ids().is_empty());
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a standing credential's ten minutes are booked no earlier than the turn that spends
/// them.**
///
/// `settle_attention` runs behind the strip's tick and the window's frame gate; a credential due
/// while the gate is shut was booked raw, an instant in the past for as long as the gate stayed
/// shut.
///
/// MUTATION: book `attention_ledger_deadline` raw in `Runtime::turn`'s fold — the turn hands the
/// loop `AttentionCredential`.
#[test]
fn a_due_credential_behind_the_strips_gate_is_booked_for_the_turn_the_gate_admits() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-credential",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let armed = Instant::now();
    let due = armed + bt_workbench::attention::expiry::WAIT_TTL;
    {
        let runtime = folio.runtime();
        let leaf = runtime.window.tabs[0]
            .sessions
            .values_mut()
            .next()
            .expect("the one pane");
        leaf.session.attention.clock.arm(
            &bt_workbench::attention::WaitSlot::Level(bt_workbench::attention::WaitKind::Agent),
            armed,
        );
        // The strip ticked a moment ago, so its gate is shut at `due`.
        runtime.window.strip_animation_ticked_at = Some(due - Duration::from_millis(1));
    }
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **the session's autosave is not booked while a quit holds the storage worker.**
///
/// Its spender does not run while the quit's documents and their copies are on the worker; its
/// instant, booked anyway, was one the loop spun on for the whole of that phase.
///
/// MUTATION: book `session_store.deadline()` under `application_clocks` alone in the fold — the
/// turn hands the loop `SessionSave`.
#[test]
fn the_autosave_is_not_booked_while_a_quit_holds_the_storage_worker() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-autosave",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    // A session that differs from the one the window records, so the window's own record on a
    // turn is a change; after that the document holds still and the store owes one write, due
    // one quiet window after the last change.
    let mut session = folio.app.session_store.loaded().clone();
    session.schema_version = session.schema_version.wrapping_add(1);
    folio.app.session_store.record(session, Instant::now());
    folio.app.quit = Some(crate::quit::Quit::begin(Vec::new()));
    assert!(
        folio
            .app
            .quit
            .as_ref()
            .is_some_and(crate::quit::Quit::holds_the_storage_worker)
    );
    let mut due = Instant::now();
    for _ in 0..4 {
        turn(&mut folio, due);
        let next = folio
            .app
            .session_store
            .deadline()
            .expect("a changed session owes a write");
        if next == due {
            break;
        }
        due = next;
    }
    assert_eq!(
        folio.app.session_store.deadline(),
        Some(due),
        "the fixture's own precondition: the document holds still and its write is due"
    );
    books_nothing_due(turn(&mut folio, due), due);
}

/// RED — **a shown glance card with no buffer behind it is taken down on its turn.**
///
/// The card is a view of a buffer; with none the paint draws nothing, and a card still reported
/// shown owed a frame no frame could pay — booked at the last present's frame, an instant in the
/// past on every turn once nothing presented.
///
/// MUTATION: drop the bufferless-card arm from `advance_file_peek` — the second turn hands the
/// loop `FilePeek`.
#[test]
fn a_shown_glance_card_with_no_buffer_is_taken_down_on_its_turn() {
    let tab = cross_tab(1, &["一 one"]);
    let shell = tab.seats.terminals()[0];
    let mut folio = HeadlessFolio::with_tabs("wake-fold-file-peek", vec![tab], 0);
    folio.draw();
    let shown = Instant::now();
    folio.window.file_peek = Some(card_over(
        RowHost::Terminal(shell),
        std::path::Path::new("nowhere.txt"),
        PeekClock::Shown(shown),
    ));
    let first = shown + Duration::from_millis(200);
    books_nothing_due(turn(&mut folio, first), first);
    let second = first + Duration::from_millis(200);
    books_nothing_due(turn(&mut folio, second), second);
    assert!(folio.window.file_peek.is_none(), "the card is down");
}

/// RED — **the storage watch's re-read is not booked while a quit holds the storage worker.**
///
/// The autosave's twin: the watch's spender (`advance_storage_watch`) does not run while the
/// quit's documents and their copies are on the worker, and its quiet instant, booked anyway,
/// was one the loop spun on for the whole of that phase. The settings file changes, a turn hears
/// it and books the quiet window; then a quit takes the worker and the window comes due.
///
/// MUTATION: book `storage_watch.deadline()` under `application_clocks` alone in the fold — the
/// turn hands the loop `StorageWatch`.
#[test]
fn the_storage_watch_is_not_booked_while_a_quit_holds_the_storage_worker() {
    let mut folio = HeadlessFolio::with_tabs(
        "wake-fold-storage-watch",
        vec![tab_holding(leaf_saying("plain\r\n"))],
        0,
    );
    folio.draw();
    let listener = folio.app.storage_watch.listener_on(Instant::now);
    listener(
        std::path::Path::new("storage"),
        bt_platform::DirChange::Named(&[std::ffi::OsString::from(
            crate::persist::SETTINGS_FILE_NAME,
        )]),
    );
    let heard = Instant::now();
    books_nothing_due(turn(&mut folio, heard), heard);
    let due = folio
        .app
        .storage_watch
        .deadline()
        .expect("the turn heard the change and booked its quiet window");
    folio.app.quit = Some(crate::quit::Quit::begin(Vec::new()));
    books_nothing_due(turn(&mut folio, due), due);
}
