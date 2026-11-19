//! **The last window's close puts it away, and every road brings it back** (0.4.10 plan row 87,
//! part 87a; T-TRAY-87A-WINDOWS-PUT-AWAY; the owner's rulings R1, R2 of 2026-11-14 and the
//! coordinator's R3, R4, R9, R10, R13 of 2026-11-12).
//!
//! On a headless application whose storage is one working storage worker in a scratch folder of
//! the test's own, so that the run's sentinel (`session.lock`) and the layout it writes
//! (`session.json`) are files the test can look at. The switch (`close_keeps_running`) is set by
//! each test, whichever its default.

use crate::headless::{
    HeadlessApplication, HeadlessFolio, HeadlessParts, PlatformAnswers, WindowEffect,
    headless_window,
};
use crate::test_support::{ringing_tab, tab_texts};
use crate::{CloseFate, carry_pass::CarryAsk};
use bt_platform::close_road::OwnCloseRoad;
use bt_platform::tray::TrayGesture;
use std::cell::RefCell;
use std::path::PathBuf;
use winit::event::WindowEvent;
use winit::window::WindowId;

thread_local! {
    /// What the door asked of the carry pass on this test's thread ([`counted_carry_ask`]).
    static CARRY_ASKS: RefCell<Vec<CarryAsk>> = const { RefCell::new(Vec::new()) };
}

/// The application's carry ask in these tests: counted, and no pass started over the account's
/// agent files.
fn counted_carry_ask(ask: CarryAsk) {
    CARRY_ASKS.with_borrow_mut(|asks| asks.push(ask));
}

/// **One application with one window holding a tab of two shells**, its storage on one working
/// worker in `dir` (the sentinel armed), the switch set to `keeps_running`, its ledger empty.
struct PutAwayRun {
    application: HeadlessApplication,
    dir: PathBuf,
}

impl PutAwayRun {
    fn open(tag: &str, keeps_running: bool) -> Self {
        let mut tab = ringing_tab(1, 2);
        // A name of its own, so a window that goes is a row Recent keeps (`Seed::names_itself`).
        tab.manual_name = Some("build".to_owned());
        Self::open_holding(tag, keeps_running, tab)
    }

    /// [`Self::open`] with the window holding `tab`.
    fn open_holding(tag: &str, keeps_running: bool, tab: crate::TabState) -> Self {
        let dir = crate::test_support::disk_scratch(&format!("put-away-{tag}"));
        let mut folio = HeadlessFolio::with_tabs(tag, vec![tab], 0);
        let mut store = crate::persist::SessionStore::at_waking(
            dir.join("session.json"),
            dir.join("session.lock"),
            || {},
        )
        .armed();
        store.set_save_budget(crate::lane::PATIENCE);
        folio.app.storage_on(store);
        folio.app.carry_ask = counted_carry_ask;
        let mut settings = folio.app.settings_store.loaded().clone();
        settings.close_keeps_running = keeps_running;
        // A store of what is already loaded answers `false`: there was nothing to write.
        let _ = folio.app.settings_store.store(settings);
        assert_eq!(
            folio.app.settings_store.loaded().close_keeps_running,
            keeps_running,
            "the switch is set"
        );
        // On the glass, as a launch leaves it.
        folio.window.window_shown = true;
        let mut application = HeadlessApplication::of(folio);
        // The window directory as the loop's turn publishes it: the windows a launch can land in.
        application.folio.publish_window_directory();
        let _ = application.event_loop.ledger.take();
        CARRY_ASKS.with_borrow_mut(Vec::clear);
        Self { application, dir }
    }

    /// A second ordinary window, `2`, beside the first.
    fn with_a_second_window(self) -> Self {
        self.with_a_second_window_holding(vec![ringing_tab(2, 1)])
    }

    /// A second ordinary window, `2`, beside the first, holding `tabs`, on the glass.
    fn with_a_second_window_holding(mut self, tabs: Vec<crate::TabState>) -> Self {
        let app = self
            .application
            .folio
            .app
            .as_ref()
            .expect("the application");
        let second = headless_window(
            app,
            HeadlessParts::new(
                &app.gpu,
                2,
                (1200, 800),
                1.0,
                &self.application.event_loop.ledger,
            ),
            tabs,
            0,
        );
        self.application
            .folio
            .windows
            .insert(WindowId::from(2), second);
        if let Some(window) = self.application.folio.windows.get_mut(WindowId::from(2)) {
            window.window_shown = true;
        }
        self.application.folio.publish_window_directory();
        self
    }

    /// The loop's look at the run's icon (`FolioApp::settle_tray`), on the test's turn.
    fn settle_tray(&mut self) -> anyhow::Result<()> {
        self.application
            .folio
            .settle_tray(&self.application.event_loop)
    }

    /// A person's `gesture` on the run's icon, parked for the loop's next turn.
    fn poke(&mut self, gesture: TrayGesture) {
        self.application.event_loop.tray.make(gesture);
    }

    /// The switch (`close_keeps_running`) set to `on`.
    fn switch(&mut self, on: bool) {
        let store = &mut self.app().settings_store;
        let mut settings = store.loaded().clone();
        settings.close_keeps_running = on;
        let _ = store.store(settings);
    }

    fn window(&mut self) -> &mut crate::WindowRuntime<crate::headless::Headless> {
        let id = self.application.id;
        self.application
            .folio
            .windows
            .get_mut(id)
            .expect("the window is in the registry")
    }

    fn app(&mut self) -> &mut crate::App<crate::headless::Headless> {
        self.application
            .folio
            .app
            .as_mut()
            .expect("the application")
    }

    /// The person's `×` on the window: a `WM_CLOSE`, dispatched as the loop dispatches it.
    fn close(&mut self) -> Vec<WindowEffect> {
        self.application.window_event(WindowEvent::CloseRequested);
        self.application.event_loop.ledger.take()
    }

    /// Folio's own close request by `road` (as its caption `×`, its final tab or its shells' exit
    /// ask it), then the `WM_CLOSE` it posts, dispatched.
    fn close_by(&mut self, road: OwnCloseRoad) -> Vec<WindowEffect> {
        let id = self.application.id;
        self.application
            .folio
            .runtime(id)
            .expect("the window")
            .request_window_close(road)
            .expect("the request is made");
        self.close()
    }

    /// Another program's `WM_CLOSE`, as the close witness reads one posted from outside, then
    /// dispatched.
    fn close_from_another_program(&mut self) -> Vec<WindowEffect> {
        self.window()
            .window
            .closes
            .borrow_mut()
            .push_back(bt_platform::close_road::CloseArrival {
                road: bt_platform::close_road::CloseRoad::CloseMessage,
                delivery: Some(bt_platform::close_road::CloseDelivery::Posted),
                foreground: Some(false),
                pointer: bt_platform::close_road::ClosePointer::Outside,
            });
        self.close()
    }

    /// The layout the store holds for the disk, by its windows, and whether the autosave's wake is
    /// booked for it.
    fn layout_for_the_autosave(&mut self) -> (usize, bool) {
        let store = &self.app().session_store;
        (store.loaded().windows.len(), store.deadline().is_some())
    }

    fn sentinel_stands(&self) -> bool {
        self.dir.join("session.lock").exists()
    }

    /// The windows `session.json` holds, read back as a document.
    fn written_windows(&self) -> usize {
        let text = std::fs::read_to_string(self.dir.join("session.json"))
            .expect("the layout is on the disk");
        let document: serde_json::Value = serde_json::from_str(&text).expect("a document");
        document["windows"]
            .as_array()
            .expect("the document's windows")
            .len()
    }

    /// What the window holds: its tabs as the session writes them, every pane's id, and every
    /// pane's rows.
    fn what_the_window_holds(
        &mut self,
    ) -> (
        Vec<bt_persist::TabV1>,
        Vec<Vec<bt_layout::SeatId>>,
        Vec<String>,
    ) {
        let id = self.application.id;
        let runtime = self
            .application
            .folio
            .runtime(id)
            .expect("the window is open");
        let snapshot = runtime.window_snapshot();
        let seats = runtime
            .window
            .tabs
            .iter()
            .map(|tab| tab.sessions.keys().copied().collect())
            .collect();
        let texts = runtime.window.tabs.iter().flat_map(tab_texts).collect();
        (snapshot.tabs, seats, texts)
    }
}

/// RED (T1; R1, R3; the coordinator's ruling of 2026-11-18 on the roads) — **with the switch on,
/// the last ordinary window's close puts it away**, by the person's `×` and by the final tab's
/// close alike: the window is hidden and stays the registry's, nothing is asked, no shell is told
/// to end, the run keeps its sentinel, no Recent row is filed and the loop is not asked to exit;
/// and the layout, the window in it, is handed to the autosave as it was put away.
///
/// PLANT P1: drop the `CloseFate::PutAway` arm of `FolioApp::on_window_event` and the
/// `CloseFate::PutAway` return of `FolioApp::close` — the close ends the run.
#[test]
fn with_the_switch_on_the_last_windows_close_puts_it_away() {
    type Road = fn(&mut PutAwayRun) -> Vec<WindowEffect>;
    let roads: [(&str, Road); 2] = [
        ("the caption's ×", PutAwayRun::close),
        ("the final tab's close", |run| {
            run.close_by(OwnCloseRoad::FinalTab)
        }),
    ];
    for (index, (road, close)) in roads.into_iter().enumerate() {
        let mut run = PutAwayRun::open(&format!("on-{index}"), true);
        let effects = close(&mut run);
        assert!(
            effects.contains(&WindowEffect::Visible(false)),
            "{road}: the window leaves the glass: {effects:?}"
        );
        assert!(
            !effects.contains(&WindowEffect::Exit),
            "{road}: the loop is not asked to exit: {effects:?}"
        );
        let window = run.window();
        assert!(window.put_away, "{road}: the window is put away");
        assert!(window.leaving.is_none(), "{road}: and not on its way out");
        assert_eq!(window.tabs.len(), 1, "{road}: its tab stays");
        assert_eq!(
            window.tabs[0].sessions.len(),
            2,
            "{road}: and both its shells, untold"
        );
        assert_eq!(
            window.dirty_gate.request(),
            None,
            "{road}: nothing is asked"
        );
        assert_eq!(run.application.folio.windows.len(), 1);
        assert!(
            run.app().recent.is_empty(),
            "{road}: no Recent row is filed"
        );
        assert!(run.sentinel_stands(), "{road}: the run is still running");
        assert_eq!(
            run.layout_for_the_autosave(),
            (1, true),
            "{road}: the layout is handed to the autosave, the window in it"
        );
    }
}

/// The close lines a window said since the ledger was last read: row 103's and the put-away's.
fn close_lines_of(effects: &[WindowEffect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            WindowEffect::Noted(line)
                if line.starts_with("Folio: last window put away")
                    || line.starts_with("Folio: last window closed")
                    || line.starts_with("Folio: window closed") =>
            {
                Some(line.as_str())
            }
            _ => None,
        })
        .collect()
}

/// RED (review P1-2; 0.4.10 plan row 103 with the switch on) — **a close that puts the last window
/// away says a close line of its own, with the road it came by**: the caption `×` and another
/// program's close message each say `Folio: last window put away —` with their `road=`, and
/// nothing says the window closed.
///
/// PLANT P22: say the road-less note in `Runtime::put_the_window_away` instead of the put-away
/// line — no close line names the road.
#[test]
fn a_put_away_close_says_its_road() {
    type Road = fn(&mut PutAwayRun) -> Vec<WindowEffect>;
    let roads: [(&str, Road, &str); 2] = [
        (
            "caption",
            |run| run.close_by(OwnCloseRoad::CaptionButton),
            "road=caption-button; delivery=posted",
        ),
        (
            "close-message",
            PutAwayRun::close_from_another_program,
            "road=close-message; delivery=posted; foreground=no; pointer=outside",
        ),
    ];
    for (road, close, says) in roads {
        let mut run = PutAwayRun::open(&format!("line-{road}"), true);
        let effects = close(&mut run);
        assert!(run.window().put_away, "{road}: the window is put away");
        let lines = close_lines_of(&effects);
        assert_eq!(lines.len(), 1, "{road}: one close line: {lines:?}");
        assert!(
            lines[0].starts_with("Folio: last window put away — ")
                && lines[0].contains("window=ordinary 1;")
                && lines[0].contains(says),
            "{road}: the put-away's line names the road: {lines:?}"
        );
    }
}

/// RED (R1, the coordinator's ruling of 2026-11-14 item 3) — **a `WM_CLOSE` that reaches a window
/// already put away is ignored**: there is no `×` on a hidden window, and the tool that sent it
/// does not end the run.
///
/// PLANT: answer `CloseFate::AlreadyAway` as `CloseFate::Shut` in `FolioApp::close_fate` — the
/// second close ends the run.
#[test]
fn a_close_that_reaches_a_put_away_window_is_ignored() {
    let mut run = PutAwayRun::open("again", true);
    let _ = run.close();
    let id = run.application.id;
    assert_eq!(run.application.folio.close_fate(id), CloseFate::AlreadyAway);
    let effects = run.close();
    assert!(
        !effects.contains(&WindowEffect::Exit),
        "the run goes on: {effects:?}"
    );
    let window = run.window();
    assert!(
        window.put_away && window.leaving.is_none(),
        "still put away"
    );
    assert!(run.sentinel_stands(), "and still running");
}

/// RED (T2; R2's off arm) — **with the switch off, the last window's close ends the run exactly as
/// before**: the window is on its way out, the run's sentinel is spent and nothing is put away.
///
/// PLANT P2: read the switch as on whatever it says in `FolioApp::the_close_keeps_the_run` — the
/// window is put away.
#[test]
fn with_the_switch_off_the_last_windows_close_ends_the_run() {
    let mut run = PutAwayRun::open("off", false);
    let _ = run.close();
    let window = run.window();
    assert!(!window.put_away, "nothing is put away");
    assert!(window.leaving.is_some(), "the window is on its way out");
    assert!(!run.sentinel_stands(), "and the run has ended");
}

/// RED (T3; R4) — **with the switch on, a window that is not the last still closes into Recent**
/// (`Runtime::vault_this_window`), as a window going and not the run ending: only the close that
/// would end the run puts its window away.
///
/// PLANT P3: answer `CloseFate::PutAway` for every window in `FolioApp::close_fate` — the window
/// is put away and nothing is filed.
#[test]
fn a_window_that_is_not_the_last_closes_into_recent_with_the_switch_on() {
    let mut run = PutAwayRun::open("not-last", true).with_a_second_window();
    let _ = run.close();
    let window = run.window();
    assert!(!window.put_away, "nothing is put away");
    assert!(window.leaving.is_some(), "the window is on its way out");
    assert_eq!(run.app().recent.len(), 1, "its tab is filed in Recent");
    assert!(
        run.sentinel_stands(),
        "and the run goes on in the other window"
    );
    let other = run
        .application
        .folio
        .windows
        .get_mut(WindowId::from(2))
        .expect("the other window");
    assert!(
        !other.put_away && other.leaving.is_none(),
        "which is neither put away nor going"
    );
}

/// RED (T4; R4) — **with the switch on, a last window whose final shell exited ends the run**: the
/// close its final tab asked for is the run's end, as before.
///
/// The close is asked as the final tab asks it when every shell has exited: by Folio's own request,
/// whose road (`OwnCloseRoad::ShellsExited`) the request carries (0.4.10 plan row 103).
///
/// PLANT P4: drop the shells'-exit term (`the_close_was_asked_by_the_shells_exit`) from
/// `FolioApp::the_close_keeps_the_run` — the window is put away.
#[test]
fn a_last_window_whose_shells_exited_ends_the_run_with_the_switch_on() {
    let mut run = PutAwayRun::open("exited", true);
    let id = run.application.id;
    run.application
        .folio
        .runtime(id)
        .expect("the window")
        .request_window_close(bt_platform::close_road::OwnCloseRoad::ShellsExited)
        .expect("the request is made");
    let _ = run.close();
    let window = run.window();
    assert!(!window.put_away, "nothing is put away");
    assert!(window.leaving.is_some(), "the window is on its way out");
    assert!(!run.sentinel_stands(), "and the run has ended");
}

/// RED (review P1-3; R4 for a hidden window) — **a put-away window whose last shell exits ends the
/// run**: the close its final tab asks for when every shell has exited reaches the window while it
/// is away, and it is the run's end as it would be from a shown window — the window on its way
/// out, the run's sentinel spent, and the loop asked to exit once the window has gone.
///
/// PLANT P23: answer `CloseFate::AlreadyAway` for that close in `FolioApp::close_fate` — the
/// put-away run stays resident with nothing in it.
#[test]
fn a_put_away_window_whose_shells_exited_ends_the_run() {
    let mut run = PutAwayRun::open("away-exited", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let _ = run.close_by(OwnCloseRoad::ShellsExited);
    assert!(
        run.window().leaving.is_some(),
        "the window is on its way out"
    );
    assert!(!run.sentinel_stands(), "the run's sentinel is spent");
    let _ = run
        .application
        .folio
        .reap_leaving_windows(&run.application.event_loop, std::time::Instant::now())
        .expect("the reap");
    assert!(
        run.application
            .event_loop
            .ledger
            .take()
            .contains(&WindowEffect::Exit),
        "and the loop is asked to exit"
    );
}

/// RED (review P2-2) — **a put-away window whose shells' exit raises a question is brought back
/// through the one door to ask it**: an unsaved preview buffer is asked about on the glass, the
/// window given the keyboard (`bring_owned_window_forward`) and the agents' hooks read again, as on
/// every road back; the window stays, neither put away nor going, with the question up.
///
/// PLANT P28: show the window through `Runtime::bring_the_window_back_on_the_glass` alone in the
/// `CloseFate::ShutWhileAway` arm of `FolioApp::on_window_event` — no keyboard, no hooks round.
#[test]
fn a_put_away_windows_question_is_asked_back_through_the_one_door() {
    let dir = crate::test_support::disk_scratch("put-away-asks-file");
    let path = dir.join("notes.md");
    std::fs::write(&path, "one\n").expect("write the file");
    let mut buffer = crate::test_support::buffer_read_from(&path);
    let mut caret = crate::preview_edit::EditCaret::default();
    let body = buffer.content().cloned().expect("the head landed");
    caret.place(&body, 4, false);
    assert!(
        buffer.edit_by_caret(&mut caret, |content, caret| {
            crate::preview_edit::insert(content, caret, "typed by hand")
        }),
        "the fixture's own precondition: there is unsaved work"
    );
    let (tab, _) = crate::test_support::tab_with_a_preview(1, vec![buffer]);
    let mut run = PutAwayRun::open_holding("away-asks", true, tab);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    CARRY_ASKS.with_borrow_mut(Vec::clear);
    let effects = run.close_by(OwnCloseRoad::ShellsExited);
    assert!(
        effects.contains(&WindowEffect::Visible(true))
            && effects.contains(&WindowEffect::ForegroundTaken),
        "the window is back on the glass and given the keyboard: {effects:?}"
    );
    assert_eq!(
        CARRY_ASKS.with_borrow_mut(std::mem::take),
        vec![CarryAsk::HOOKS],
        "one round for the agents' hooks"
    );
    let window = run.window();
    assert!(
        !window.put_away && window.leaving.is_none(),
        "the window stays, on the glass"
    );
    assert_eq!(
        window.dirty_gate.request(),
        Some(&crate::restore::GateRequest::Shut),
        "with the question up"
    );
}

/// RED (review P2-1) — **a closing window is never brought back**: a put-away window whose shells'
/// exit closed it is on its way out, and the door back, asked before the window has gone (a second
/// launch, a notification's click), brings nothing back and puts nothing on the glass.
///
/// PLANT P25: drop the `leaving` term from `Runtime::bring_the_window_back_on_the_glass` and the
/// put-away's clearing from `Runtime::let_go_of_this_window` — the closing window is shown.
#[test]
fn a_closing_window_is_never_brought_back() {
    let mut run = PutAwayRun::open("away-closing", true);
    let _ = run.close();
    let _ = run.close_by(OwnCloseRoad::ShellsExited);
    assert!(
        run.window().leaving.is_some(),
        "the fixture's window is on its way out"
    );
    assert_eq!(
        run.application
            .folio
            .bring_the_window_back()
            .expect("the door"),
        None,
        "nothing is brought back"
    );
    let effects = run.application.event_loop.ledger.take();
    assert!(
        !effects.contains(&WindowEffect::Visible(true)),
        "nothing is put on the glass: {effects:?}"
    );
}

/// RED (review P1-1) — **while Folio's quit from outside holds the document, a close closes
/// nothing**: the `WM_CLOSE` posted after the quit for older builds reaches a window that is not
/// the last before the settle, and it is neither filed into Recent nor sent on its way out nor put
/// away; the settle then writes the layout the quit held, both windows in it.
///
/// Each of the two guards is held by a case of its own (re-review P2-12): the window's event door
/// keeps a lone window from being put away during the hold, and `FolioApp::close` keeps a gate's
/// answer, re-run during the hold, from closing the window it was asked about.
///
/// PLANT P21: drop the hold's guard from `FolioApp::on_window_event` and from `FolioApp::close` —
/// the window is filed into Recent that the held layout also restores.
///
/// PLANT P30: drop only the window event door's guard (`FolioApp::a_close_the_held_document_spends`
/// answers `false`) — the lone window is put away during the hold.
///
/// PLANT P31: drop only `FolioApp::close`'s guard — the gate's `Discard`, re-run during the hold,
/// sends the window it was asked about on its way out and files it into Recent.
#[test]
fn a_close_while_the_outside_quit_holds_the_document_closes_nothing() {
    held_by_the_outside_quit_two_windows_close_nothing();
    held_by_the_outside_quit_a_lone_window_is_not_put_away();
    held_by_the_outside_quit_a_gates_answer_closes_nothing();
}

/// Folio's quit from outside, heard: the document is held from here until the settle.
fn the_outside_quit_holds_the_document() {
    let quit = 0xC123;
    assert_eq!(
        bt_platform::session_end::answer_outside_quit(
            quit,
            quit,
            &crate::session_end::hear,
            &|_| {}
        ),
        Some(0),
        "the quit from outside is heard"
    );
    assert!(
        crate::session_end::holds_the_document(),
        "and the document is held"
    );
}

/// [`a_close_while_the_outside_quit_holds_the_document_closes_nothing`]'s lone window: the hold,
/// then its close. Not put away, not going.
fn held_by_the_outside_quit_a_lone_window_is_not_put_away() {
    let _news = crate::test_support::EndSessionHome::new("put-away-quit-close-one");
    let mut run = PutAwayRun::open("quit-close-one", true);
    the_outside_quit_holds_the_document();
    let _ = run.close();
    let window = run.window();
    assert!(
        !window.put_away && window.leaving.is_none(),
        "a lone window's close during the hold puts nothing away"
    );
}

/// [`a_close_while_the_outside_quit_holds_the_document_closes_nothing`]'s gate answer: a window's
/// question up, the hold, then its `Discard`, re-run as the window event's tail re-runs it. The
/// window is not filed and not going.
fn held_by_the_outside_quit_a_gates_answer_closes_nothing() {
    let _news = crate::test_support::EndSessionHome::new("put-away-quit-gate");
    let mut run = PutAwayRun::open("quit-gate", true).with_a_second_window();
    run.window()
        .dirty_gate
        .open(crate::restore::GateRequest::Shut);
    the_outside_quit_holds_the_document();
    run.application
        .runtime()
        .answer_dirty_gate(crate::restore::GateAnswer::Discard)
        .expect("the answer is spent");
    run.application.window_event(WindowEvent::Focused(true));
    assert!(
        run.app().recent.is_empty(),
        "the answer's re-run files nothing into Recent"
    );
    assert!(
        run.window().leaving.is_none(),
        "and the window it was asked about is not on its way out"
    );
}

/// [`a_close_while_the_outside_quit_holds_the_document_closes_nothing`]'s two windows: the hold,
/// then the close of one that is not the last. Nothing filed, nothing going or put away, and the
/// settle writes both.
fn held_by_the_outside_quit_two_windows_close_nothing() {
    let _news = crate::test_support::EndSessionHome::new("put-away-quit-close");
    let mut run = PutAwayRun::open("quit-close", true).with_a_second_window();
    // Both windows in the document, as each records itself when it is born.
    for id in [1, 2] {
        run.application
            .folio
            .runtime(WindowId::from(id))
            .expect("the window")
            .mark_session_dirty(std::time::Instant::now());
    }
    the_outside_quit_holds_the_document();
    let _ = run.close();
    assert!(run.app().recent.is_empty(), "nothing is filed into Recent");
    let window = run.window();
    assert!(
        window.leaving.is_none() && !window.put_away,
        "and the window is neither going nor put away"
    );
    let recovery = run.dir.join(crate::preview::RECOVERED_FOLDER);
    let _ = run
        .application
        .folio
        .settle_session_end_keeping_in(&run.application.event_loop, &recovery);
    assert_eq!(
        run.written_windows(),
        2,
        "the layout the quit held is written, both windows in it"
    );
}

/// RED (review P2-3, re-review P1-4) — **a put-away window comes back on a monitor that is
/// there**: the rectangle judged is the window's normal one, as the layout restore judges it.
/// - A normal window put away on a display that has gone since (its rectangle on no monitor the
///   platform names) is stood on the primary monitor, at its own size, before it is shown; one
///   whose rectangle a monitor still sees is left where it is.
/// - A maximised window is never stood anywhere. Its own rectangle, the maximised frame that
///   overhangs the work area, is not judged: on a display that is there nothing is written; with
///   its normal rectangle on a display that has gone, that rectangle is written as the one it
///   takes when it is next shown normal, on the primary monitor, and it is shown maximised.
///
/// PLANT P26: drop the monitor check (`hold_the_window_to_a_present_monitor`) from
/// `Runtime::bring_the_window_back_on_the_glass` — the window comes back off every screen.
///
/// PLANT P29: judge the rectangle the window stands at (`outer_rect`) in every posture again —
/// both maximised arms go red, the frame fitted on a display that is there and moved at its
/// maximised size from one that has gone.
#[test]
fn a_put_away_window_comes_back_on_a_monitor_that_is_there() {
    let monitor = crate::RestoreMonitor {
        left: 0.0,
        top: 0.0,
        right: 1920.0,
        bottom: 1040.0,
    };
    let on_the_monitor = bt_platform::WindowRect {
        left: 100,
        top: 80,
        right: 1100,
        bottom: 780,
    };
    let gone = bt_platform::WindowRect {
        left: 4000,
        top: 200,
        right: 5000,
        bottom: 900,
    };
    let on_the_primary = bt_platform::WindowRect {
        left: 0,
        top: 0,
        right: 1000,
        bottom: 700,
    };
    // A maximised thick frame: its monitor's work area grown by 8 px on each side.
    let maximised_on_the_monitor = bt_platform::WindowRect {
        left: -8,
        top: -8,
        right: 1928,
        bottom: 1048,
    };
    let maximised_on_the_gone_display = bt_platform::WindowRect {
        left: 3992,
        top: -8,
        right: 5928,
        bottom: 1048,
    };
    type Arm<'a> = (
        &'a str,
        bt_platform::WindowRect,
        Option<bt_platform::WindowRect>,
        Option<bt_platform::WindowRect>,
        Option<bt_platform::WindowRect>,
    );
    // (case, its normal rectangle, the frame it stands at maximised, stood at, normal rect written)
    let arms: [Arm; 4] = [
        (
            "normal, on a display that has gone",
            gone,
            None,
            Some(on_the_primary),
            None,
        ),
        (
            "normal, on a display that is there",
            on_the_monitor,
            None,
            None,
            None,
        ),
        (
            "maximised, on a display that is there",
            on_the_monitor,
            Some(maximised_on_the_monitor),
            None,
            None,
        ),
        (
            "maximised, its normal rectangle on a display that has gone",
            gone,
            Some(maximised_on_the_gone_display),
            None,
            Some(on_the_primary),
        ),
    ];
    for (index, (case, normal, maximised, stood, kept)) in arms.into_iter().enumerate() {
        let mut run = PutAwayRun::open(&format!("monitor-{index}"), true);
        // The window stood normal at `normal`, and recorded itself there.
        run.window().window.answers.set(PlatformAnswers {
            outer_rect: Some(normal),
            monitor: Some(monitor),
            ..PlatformAnswers::default()
        });
        run.application
            .runtime()
            .mark_session_dirty(std::time::Instant::now());
        if let Some(frame) = maximised {
            run.window().window.answers.set(PlatformAnswers {
                outer_rect: Some(frame),
                maximized: true,
                monitor: Some(monitor),
                ..PlatformAnswers::default()
            });
        }
        let _ = run.close();
        assert!(run.window().put_away, "{case}: the window is put away");
        assert!(
            run.application
                .folio
                .bring_the_window_back()
                .expect("the door")
                .is_some(),
            "{case}: the window is brought back"
        );
        let effects = run.application.event_loop.ledger.take();
        let stood_at = effects.iter().find_map(|effect| match effect {
            WindowEffect::StoodAt(rect) => Some(*rect),
            _ => None,
        });
        let normal_rect_kept = effects.iter().find_map(|effect| match effect {
            WindowEffect::NormalRectKept(rect) => Some(*rect),
            _ => None,
        });
        assert_eq!(stood_at, stood, "{case}: where it is stood: {effects:?}");
        assert_eq!(
            normal_rect_kept, kept,
            "{case}: the normal rectangle written: {effects:?}"
        );
        assert!(
            effects.contains(&WindowEffect::Visible(true)),
            "{case}: and shown: {effects:?}"
        );
        assert_eq!(
            effects.contains(&WindowEffect::Maximized(true)),
            maximised.is_some(),
            "{case}: maximised as it was put away: {effects:?}"
        );
    }
}

/// RED (review P2-5; the coordinator's ruling of 2026-11-18) — **a put-away window's recording
/// pauses, and plays again when the window is brought back**: a closed window is not heard.
///
/// PLANT P27: drop the pause (`pause_while_away`) from `Runtime::put_the_window_away` — the
/// recording plays on behind the put-away window.
#[test]
fn a_put_away_windows_recording_pauses_and_plays_again_on_the_way_back() {
    let _ledger = crate::test_support::ledger_gate();
    let before = bt_platform::video::engine::engines_outstanding();
    let (tab, seat) = crate::test_support::tab_with_a_preview(1, vec![]);
    let mut run = PutAwayRun::open_holding("video", true, tab);
    let surface = crate::test_support::seat_of(crate::test_support::TAB_ONE, seat);
    let recording = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/assets/folio-video-test.mp4");
    run.window().tabs[0]
        .preview_panes
        .entry(surface)
        .show_picture(recording.clone());
    // Drawn once, its still on the pane.
    {
        let mut runtime = run.application.runtime();
        runtime
            .publish_frame(bt_render::FrameTrigger {
                occurred_at: std::time::Instant::now(),
                source: bt_render::FrameSource::Expose,
            })
            .expect("a frame composes");
        runtime.redraw().expect("and reaches the glass");
    }
    // And played, as the pane's play door seats it.
    run.window()
        .video
        .open(surface, &recording, std::time::Instant::now())
        .expect("the recording opens");
    let playing = |run: &mut PutAwayRun, playing: bool| {
        run.window()
            .video
            .get(surface)
            .expect("the seat stands")
            .state_reaching(|state| state.ready && state.playing == playing)
            .playing
    };
    assert!(playing(&mut run, true), "the recording plays");
    let _ = run.close();
    assert!(run.window().put_away, "the window is put away");
    assert!(!playing(&mut run, false), "the put-away pauses it");
    let _ = run
        .application
        .folio
        .bring_the_window_back()
        .expect("the door");
    assert!(playing(&mut run, true), "and the way back plays it again");
    run.window().video.shutdown_all();
    crate::test_support::engines_settling_to(before);
}

/// RED (T6; R10, R13; 87b's T6) — **a plain second launch, an explicit `--tab`, a click on one of
/// Folio's notifications and a click on the run's icon each bring the put-away window back as it
/// was**: on the glass and given the
/// keyboard, with the same tabs as the session writes them (the resume records among them), the
/// same pane ids and the same rows; and the carry pass is asked once for the agents' hooks. A
/// plain launch lands nothing else; `--tab` then lands its tab.
///
/// PLANT P7: drop the show from `Runtime::bring_the_window_back_on_the_glass` (its
/// `put_the_window_on_the_glass` call) — no `Visible(true)`. PLANT P8: drop the
/// `asks_for_folio_and_nothing_else` return in `FolioApp::land_one_launch_request` — the plain
/// launch lands a tab.
#[test]
fn every_road_brings_the_put_away_window_back_as_it_was() {
    type Road = fn(&mut PutAwayRun) -> Option<WindowId>;
    let plain: Road = |run| {
        let request = crate::launch_wire::LaunchRequest::default();
        run.application
            .folio
            .land_one_launch_request(&run.application.event_loop, &request)
            .expect("the launch lands")
    };
    let tab: Road = |run| {
        let request = crate::launch_wire::LaunchRequest {
            tab: true,
            ..crate::launch_wire::LaunchRequest::default()
        };
        run.application
            .folio
            .land_one_launch_request(&run.application.event_loop, &request)
            .expect("the launch lands")
    };
    let toast: Road = |run| {
        let id = run.application.id;
        let window = run.window();
        let route = crate::notify::NotificationRoute {
            window: u64::from(id),
            tab: window.tabs[0].id.0,
            seat: window.tabs[0].sessions.keys().next().expect("a pane").0,
        };
        run.application
            .folio
            .route_notification_launches(vec![route.launch()])
            .expect("the click is routed");
        Some(id)
    };
    // The run's icon (0.4.10 plan row 87, part 87b): a click, put up on the turn it is clicked.
    let icon: Road = |run| {
        run.poke(TrayGesture::Show);
        run.settle_tray().expect("the icon's turn");
        Some(run.application.id)
    };
    for (road, bring, landed_tabs) in [
        ("plain", plain, 0),
        ("tab", tab, 1),
        ("toast", toast, 0),
        ("icon", icon, 0),
    ] {
        let mut run = PutAwayRun::open(&format!("back-{road}"), true);
        let (tabs, seats, texts) = run.what_the_window_holds();
        let _ = run.close();
        assert!(
            run.window().put_away,
            "{road}: the fixture's window is put away"
        );
        let id = run.application.id;
        assert_eq!(bring(&mut run), Some(id), "{road}: the window it lands in");
        let effects = run.application.event_loop.ledger.take();
        assert!(
            effects.contains(&WindowEffect::Visible(true)),
            "{road}: the window is back on the glass: {effects:?}"
        );
        assert!(
            effects.contains(&WindowEffect::ForegroundTaken),
            "{road}: and given the keyboard: {effects:?}"
        );
        assert!(!run.window().put_away, "{road}: and no longer put away");
        let (tabs_after, seats_after, texts_after) = run.what_the_window_holds();
        assert_eq!(
            tabs_after.len(),
            tabs.len() + landed_tabs,
            "{road}: the tabs it lands"
        );
        assert_eq!(
            tabs_after[..tabs.len()],
            tabs[..],
            "{road}: the same tabs, as the session writes them"
        );
        assert_eq!(
            seats_after[..seats.len()],
            seats[..],
            "{road}: the same panes"
        );
        assert_eq!(
            texts_after[..texts.len()],
            texts[..],
            "{road}: the same rows"
        );
        assert_eq!(
            CARRY_ASKS.with_borrow_mut(std::mem::take),
            vec![CarryAsk::HOOKS],
            "{road}: one round for the agents' hooks"
        );
    }
}

/// RED (T6, the minimised arm) — **a window put away while minimised comes back restored.**
///
/// PLANT: call `put_the_window_on_the_glass` alone in `FolioApp::bring_the_window_back` (drop
/// `bring_owned_window_forward`) — the window comes back minimised.
#[test]
fn a_window_put_away_minimised_comes_back_restored() {
    let mut run = PutAwayRun::open("back-minimised", true);
    let _ = run.close();
    run.window().window.answers.set(PlatformAnswers {
        minimized: Some(true),
        ..PlatformAnswers::default()
    });
    let request = crate::launch_wire::LaunchRequest::default();
    let _ = run
        .application
        .folio
        .land_one_launch_request(&run.application.event_loop, &request)
        .expect("the launch lands");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Minimized(false)),
        "the window is restored: {effects:?}"
    );
}

/// RED (T11; R9) — **Folio's quit from outside takes the Restart Manager's close road on a
/// put-away run**: the layout is written, nothing is asked, every window is let go and the loop
/// is asked to exit.
///
/// PLANT P11: answer `None` from `bt_platform::session_end::answer_outside_quit` (the message read
/// as a close, which a put-away window ignores) — nothing is heard, the run goes on.
#[test]
fn the_outside_quit_writes_the_layout_and_leaves() {
    let _news = crate::test_support::EndSessionHome::new("put-away-quit");
    let mut run = PutAwayRun::open("quit", true);
    let _ = run.close();
    // A registered message's number, as the session gives one (0xC000 and above).
    let quit = 0xC123;
    assert_eq!(
        bt_platform::session_end::answer_outside_quit(
            quit,
            quit,
            &crate::session_end::hear,
            &|_| {}
        ),
        Some(0),
        "the message is processed"
    );
    let recovery = run.dir.join(crate::preview::RECOVERED_FOLDER);
    let _ = run
        .application
        .folio
        .settle_session_end_keeping_in(&run.application.event_loop, &recovery);
    assert_eq!(
        run.written_windows(),
        1,
        "the layout is written, the window in it"
    );
    assert!(!run.sentinel_stands(), "the run claims its clean exit");
    assert_eq!(
        run.application.folio.windows.len(),
        0,
        "every window is let go"
    );
    assert!(
        run.application
            .event_loop
            .ledger
            .take()
            .contains(&WindowEffect::Exit),
        "and the loop is asked to exit"
    );
}

/// RED (T14; row 18's road with the run put away) — **the system's question reaches a put-away
/// run and writes its layout at the question**: the window is still the registry's, its session-end
/// watch with it.
///
/// PLANT P14: return from `FolioApp::settle_session_end_keeping_in` before the settle when a
/// window is put away — no layout is written at the question.
#[test]
fn the_systems_question_writes_a_put_away_runs_layout() {
    let _news = crate::test_support::EndSessionHome::new("put-away-end");
    let mut run = PutAwayRun::open("end", true);
    let _ = run.close();
    let _ = std::fs::remove_file(run.dir.join("session.json"));
    assert_eq!(crate::test_support::the_system_asks(), Some(1));
    let recovery = run.dir.join(crate::preview::RECOVERED_FOLDER);
    let _ = run
        .application
        .folio
        .settle_session_end_keeping_in(&run.application.event_loop, &recovery);
    assert_eq!(
        run.written_windows(),
        1,
        "the layout is written at the question"
    );
    assert!(!run.sentinel_stands(), "and the sentinel goes with it");
    assert!(run.window().put_away, "the run is still put away");
}

/// RED (T15; gate 1) — **a put-away window's attention reaches the desktop as a toast**: a
/// put-away window is hidden (`window_is_hidden` keys on the put-away), and a hidden window
/// flashes no taskbar button it does not have.
///
/// PLANT P5: drop the put-away term from `runtime::frame::window_is_hidden` — the reach is
/// `Nothing` (the window holds the keyboard and the tab is active).
#[test]
fn a_put_away_windows_reach_is_a_toast() {
    let mut run = PutAwayRun::open("reach", true);
    let _ = run.close();
    let id = run.application.id;
    let mut runtime = run.application.folio.runtime(id).expect("the window");
    runtime.observe_window_place();
    let place = runtime.window.observed_place;
    assert!(place.hidden, "the window is hidden");
    assert_eq!(
        crate::notify::desktop_reach(true, place),
        crate::attention::Reach::Toast
    );
}

/// The text a frame shows, its drawable rows joined.
fn frame_says(frame: &bt_viewport::ViewportFrame) -> String {
    (0..frame.drawable_rows())
        .map(|row| crate::test_support::frame_row_text(frame, row))
        .collect::<Vec<_>>()
        .join(" ")
}

/// RED (T5b; the VM's M1 and M3, 2026-11-18) — **a put-away window publishes nothing, and its
/// first frame back is built from the grid then**: what a shell prints while the window is away is
/// fed (the pane's grid holds it), but the drain's publish composes no frame, files nothing in the
/// slot and leaves no coalescing debt standing; the plain launch that brings the window back
/// composes a frame, and that frame shows what was printed.
///
/// PLANT P15: drop the `put_away` gate at the top of `Runtime::publish_frame_inner` — the drain
/// composes a frame for the put-away window.
#[test]
fn a_put_away_window_publishes_nothing_and_its_first_frame_back_is_built_then() {
    let mut run = PutAwayRun::open("publish", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let id = run.application.id;
    let composed = run.window().composed_terminal_frames;
    let revision = run.window().terminal_content_revision;
    let overwrites = run.window().pending_frames.overwrites();
    {
        let mut runtime = run.application.folio.runtime(id).expect("the window");
        let now = std::time::Instant::now();
        // The debt a drain opens when it holds its frame for the coalescing window.
        let _ = runtime.window.pty_coalesce.opened_at(now);
        runtime.window.pty_coalesce.until = Some(now + crate::coalesce::COALESCE_WINDOW);
        runtime
            .shell_mut()
            .session
            .feed(b"printed-while-away")
            .expect("the shell's bytes are fed");
        runtime
            .publish_pty_drain_frame(now, false)
            .expect("the drain's publish");
        runtime
            .publish_pty_drain_frame(now, true)
            .expect("a forced one");
    }
    assert_eq!(
        run.window().composed_terminal_frames,
        composed,
        "no frame is composed off the glass"
    );
    assert_eq!(
        run.window().terminal_content_revision,
        revision,
        "no picture is newer than the glass"
    );
    assert_eq!(
        run.window().pending_frames.overwrites(),
        overwrites,
        "nothing is filed in the slot"
    );
    assert_eq!(
        run.window().pty_coalesce,
        crate::coalesce::Pending::default(),
        "and the PTY's debt is settled: no deadline stands for a picture nobody draws"
    );
    let (_, _, texts) = run.what_the_window_holds();
    assert!(
        texts.iter().any(|text| text.contains("printed-while-away")),
        "the grid holds what was printed: {texts:?}"
    );
    let request = crate::launch_wire::LaunchRequest::default();
    let landed = run
        .application
        .folio
        .land_one_launch_request(&run.application.event_loop, &request)
        .expect("the launch lands");
    assert_eq!(landed, Some(id), "the plain launch brings the window back");
    assert!(
        run.window().composed_terminal_frames > composed,
        "the way back composes a frame"
    );
    let window = run.window();
    let shown = window
        .pending_frames
        .pending_frame()
        .or(window.last_presented_frame.as_ref())
        .expect("a frame for the glass");
    assert!(
        frame_says(shown).contains("printed-while-away"),
        "built from the grid as it stands: {}",
        frame_says(shown)
    );
}

// ── The run's icon in the notification area (0.4.10 plan row 87, part 87b) ─────────────────────

/// The pictures the run's icon was put up showing, and told to show, in these effects.
fn pictures_of(effects: &[WindowEffect]) -> Vec<&bt_platform::tray::TrayPicture> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            WindowEffect::TrayShown(picture) | WindowEffect::TrayChanged(picture) => Some(picture),
            _ => None,
        })
        .collect()
}

/// Where `wanted` stands in `effects`, if it does.
fn place_of(effects: &[WindowEffect], wanted: &WindowEffect) -> Option<usize> {
    effects.iter().position(|effect| effect == wanted)
}

/// RED (T12; R12) — **the icon's Show with nothing put away raises the window the reader was last
/// in**: the window is given the keyboard, nothing is hidden and no window is opened.
///
/// PLANT 87b-P22: drop the raise from `FolioApp::settle_tray`'s `Show` arm — the click is not
/// answered.
#[test]
fn the_icons_show_with_nothing_put_away_raises_the_window() {
    let mut run = PutAwayRun::open("icon-raise", true);
    run.poke(TrayGesture::Show);
    run.settle_tray().expect("the icon's turn");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::ForegroundTaken),
        "the window is given the keyboard: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::Visible(false))
            && !effects.contains(&WindowEffect::WindowOpened),
        "nothing is hidden and nothing opened: {effects:?}"
    );
    assert!(!run.window().put_away);
}

/// RED (T10; item 8) — **the icon's New window opens a window and only that**: the put-away window
/// stays put away (a headless loop opens no second window, so the door's own refusal is the
/// answer).
///
/// PLANT 87b-P23: bring the put-away window back in `FolioApp::settle_tray`'s `NewWindow` arm
/// before the window opens — it is shown.
#[test]
fn the_icons_new_window_leaves_the_put_away_window_away() {
    let mut run = PutAwayRun::open("icon-new", true);
    let _ = run.close();
    run.settle_tray().expect("the icon is put up");
    let _ = run.application.event_loop.ledger.take();
    run.poke(TrayGesture::NewWindow);
    assert!(
        run.settle_tray().is_err(),
        "the headless door opens no second window"
    );
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::WindowOpened),
        "a window is asked for: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::Visible(true)),
        "and the put-away one is not shown: {effects:?}"
    );
    assert!(run.window().put_away, "it stays put away");
}

/// RED (T7, T8; R2) — **the icon is put up showing what the tabs claim, and told only what
/// changed**: a quiet turn tells it nothing; a bell that rang is told once, its badge the strip's
/// bell and its tip one tab waiting; the next quiet turn tells it nothing again.
///
/// PLANT 87b-P10: drop the comparison from `TrayStand::tell` — the quiet turn tells the icon again.
#[test]
fn the_icon_is_told_only_what_changed() {
    let mut run = PutAwayRun::open("icon-told", true);
    run.settle_tray().expect("the icon is put up");
    let effects = run.application.event_loop.ledger.take();
    let plain = bt_platform::tray::TrayPicture {
        badge: None,
        tip: crate::i18n::tray_tooltip(0, 0),
    };
    assert_eq!(
        effects,
        vec![WindowEffect::TrayShown(plain)],
        "put up showing nothing to say"
    );
    run.application.folio.tell_the_tray();
    assert_eq!(
        run.application.event_loop.ledger.take(),
        vec![],
        "a quiet turn tells it nothing"
    );
    let seat = *run.window().tabs[0].sessions.keys().next().expect("a pane");
    crate::test_support::ring(&mut run.window().tabs[0], seat);
    run.application.folio.tell_the_tray();
    let bell = bt_platform::tray::TrayPicture {
        badge: crate::tray_fold::tray_badge_of(crate::tray_fold::TrayState::Bell),
        tip: crate::i18n::tray_tooltip(0, 1),
    };
    assert_eq!(
        run.application.event_loop.ledger.take(),
        vec![WindowEffect::TrayChanged(bell)],
        "the bell is told once"
    );
    run.application.folio.tell_the_tray();
    assert_eq!(
        run.application.event_loop.ledger.take(),
        vec![],
        "and the next quiet turn tells it nothing"
    );
}

/// RED (T7, the put-away arm; R3) — **a put-away window's active tab is read as a tab nobody looks
/// at**: output it has that was never seen lights the icon (the strip's unread), where the same
/// tab on the glass, active, says nothing.
///
/// PLANT 87b-P17: read the active tab as looked at whatever its window (`FolioApp::tray_sights`
/// without its `looked_at` term) — the icon shows nothing.
#[test]
fn a_put_away_windows_active_tab_is_read_as_one_nobody_looks_at() {
    let mut run = PutAwayRun::open("icon-unseen", true);
    let seat = *run.window().tabs[0].sessions.keys().next().expect("a pane");
    let pane = run.window().tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the pane");
    pane.session.output_revision = pane.session.output_revision.wrapping_add(1);
    run.settle_tray().expect("the icon is put up");
    assert_eq!(
        pictures_of(&run.application.event_loop.ledger.take())
            .first()
            .and_then(|picture| picture.badge),
        None,
        "the fixture's own precondition: on the glass, the active tab's output is seen"
    );
    let _ = run.close();
    let _ = run.application.event_loop.ledger.take();
    run.application.folio.tell_the_tray();
    assert_eq!(
        pictures_of(&run.application.event_loop.ledger.take())
            .first()
            .and_then(|picture| picture.badge),
        crate::tray_fold::tray_badge_of(crate::tray_fold::TrayState::Finished),
        "put away, it is output nobody has seen"
    );
}

/// RED (T5; R5, R7; 87d's R9) — **the icon matches the switch**: on, it is put up; off, a put-away
/// window is brought back first and then the icon is taken down; on again, it is put up again;
/// and a platform with no notification area has no icon.
///
/// PLANT 87b-P18: drop the bring-back from `FolioApp::settle_tray`'s switched-off arm — the icon
/// goes and the window stays hidden, with nothing left to bring it back.
#[test]
fn the_icon_follows_the_switch() {
    let mut run = PutAwayRun::open("icon-switch", true);
    let _ = run.close();
    run.settle_tray().expect("the icon's turn");
    let effects = run.application.event_loop.ledger.take();
    assert_eq!(pictures_of(&effects).len(), 1, "on: put up: {effects:?}");
    run.switch(false);
    run.settle_tray().expect("the icon's turn");
    let effects = run.application.event_loop.ledger.take();
    let back = place_of(&effects, &WindowEffect::Visible(true));
    let gone = place_of(&effects, &WindowEffect::TrayRemoved);
    assert!(
        back.is_some() && gone.is_some() && back < gone,
        "off: the window back on the glass, then the icon gone: {effects:?}"
    );
    assert!(!run.window().put_away, "and no longer put away");
    run.settle_tray().expect("the icon's turn");
    assert_eq!(
        run.application.event_loop.ledger.take(),
        vec![],
        "off, a quiet turn"
    );
    run.switch(true);
    run.settle_tray().expect("the icon's turn");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        matches!(effects.as_slice(), [WindowEffect::TrayShown(_)]),
        "on again: put up again: {effects:?}"
    );

    let mut bare = PutAwayRun::open("icon-no-area", true);
    bare.application.event_loop.tray.area.set(false);
    bare.settle_tray().expect("the icon's turn");
    assert_eq!(
        bare.application.event_loop.ledger.take(),
        vec![],
        "no notification area, no icon"
    );
}

/// RED (R17's other half) — **a host that could not be made is said once and not asked again**:
/// the run goes on without an icon.
#[test]
fn a_host_that_could_not_be_made_is_not_asked_again() {
    let mut run = PutAwayRun::open("icon-refused", true);
    run.application.event_loop.tray.refused.set(true);
    run.settle_tray().expect("the icon's turn");
    run.application.event_loop.tray.refused.set(false);
    run.settle_tray().expect("the icon's turn");
    assert_eq!(
        run.application.event_loop.ledger.take(),
        vec![],
        "nothing is put up"
    );
    assert!(run.app().tray.was_refused() && !run.app().tray.is_up());
}

/// RED (T9b; R4, R5) — **the icon's Quit on a put-away run asks its question on the glass**: the
/// window comes back through the one door with the question up; while it asks, the icon's other
/// rows are dropped — no window opens and no second quit is asked.
///
/// PLANT 87b-P19: drop the bring-back from `FolioApp::begin_quit_if_asked` — the question is asked
/// on a hidden window.
#[test]
fn the_icons_quit_asks_its_question_on_the_glass() {
    let dir = crate::test_support::disk_scratch("put-away-icon-quit-file");
    let path = dir.join("notes.md");
    std::fs::write(&path, "one\n").expect("write the file");
    let mut buffer = crate::test_support::buffer_read_from(&path);
    let mut caret = crate::preview_edit::EditCaret::default();
    let body = buffer.content().cloned().expect("the head landed");
    caret.place(&body, 4, false);
    assert!(
        buffer.edit_by_caret(&mut caret, |content, caret| {
            crate::preview_edit::insert(content, caret, "typed by hand")
        }),
        "the fixture's own precondition: there is unsaved work"
    );
    let (tab, _) = crate::test_support::tab_with_a_preview(1, vec![buffer]);
    let mut run = PutAwayRun::open_holding("icon-quit-asks", true, tab);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    run.settle_tray().expect("the icon is put up");
    let _ = run.application.event_loop.ledger.take();
    run.poke(TrayGesture::Quit);
    run.settle_tray().expect("the icon's turn");
    run.application
        .folio
        .settle_quit(&run.application.event_loop, std::time::Instant::now())
        .expect("the quit's turn");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is back on the glass: {effects:?}"
    );
    assert!(!run.window().put_away, "and no longer put away");
    assert_eq!(
        run.app().quit.as_ref().map(crate::quit::Quit::step),
        Some(crate::quit::QuitStep::Ask),
        "with the question up"
    );
    run.poke(TrayGesture::NewWindow);
    run.poke(TrayGesture::Quit);
    run.settle_tray().expect("the icon's turn");
    assert!(
        !run.application
            .event_loop
            .ledger
            .take()
            .contains(&WindowEffect::WindowOpened),
        "while it asks, no window opens"
    );
    assert!(!run.app().quit_requested, "and no second quit is asked");
}

/// RED (T9; R6) — **the icon's Quit on a put-away run writes the layout, leaves, and takes the
/// icon out of the notification area** at the run's end seam (`App::finish`), after the run's
/// last write: the sentinel is already gone when the icon goes.
///
/// PLANT 87b-P15: take the icon down above `session_store.close()` in `App::finish` — the
/// sentinel still stands at the removal.
#[test]
fn the_icons_quit_leaves_and_takes_the_icon_down() {
    let mut run = PutAwayRun::open("icon-quit", true);
    let _ = run.close();
    run.application
        .event_loop
        .tray
        .watch_at_the_removal(run.dir.join("session.lock"));
    run.settle_tray().expect("the icon is put up");
    let _ = run.application.event_loop.ledger.take();
    run.poke(TrayGesture::Quit);
    run.settle_tray().expect("the icon's turn");
    let mut effects = Vec::new();
    // The turns' own clock, a turn apart (nothing here waits on the real one): the quit is
    // judged on the instant each turn hands it.
    let begun = std::time::Instant::now();
    for turn in 0..200u32 {
        let now = begun + std::time::Duration::from_millis(10) * turn;
        // The turn's retirement arm (`FolioApp::about_to_wait_inner`): the browsers' clock, the
        // pages' answer, the quit's settle.
        let folio = &mut run.application.folio;
        if folio
            .app
            .as_ref()
            .and_then(|app| app.quit.as_ref())
            .is_some_and(crate::quit::Quit::is_retiring)
        {
            let _ = folio
                .advance_retirement(now)
                .expect("the retirement's clock");
            let gone = folio.every_page_has_gone();
            folio.report_to_quit(|quit| quit.pages(gone, now));
        }
        folio
            .settle_quit(&run.application.event_loop, now)
            .expect("the quit's turn");
        effects.extend(run.application.event_loop.ledger.take());
        if effects.contains(&WindowEffect::Exit) {
            break;
        }
    }
    assert!(
        effects.contains(&WindowEffect::Exit),
        "the run leaves: {effects:?}"
    );
    assert_eq!(
        run.written_windows(),
        1,
        "the layout is written, the window in it"
    );
    assert!(!run.sentinel_stands(), "the run claims its clean exit");
    assert!(
        effects.contains(&WindowEffect::TrayRemoved),
        "and the icon leaves the notification area: {effects:?}"
    );
    assert_eq!(
        run.application.event_loop.tray.watched_at_the_removal.get(),
        Some(false),
        "after the run's last write: the sentinel was gone when the icon went"
    );
}

/// RED (R8; a change to 87a's rule) — **a window opened beside a put-away one is put away by its
/// own close**: put-away windows are not windows a person can see, so the close of the last one
/// on the glass puts it away too; neither is filed into Recent and the run goes on.
///
/// PLANT 87b-P20: count every window in `FolioApp::close_fate`'s put-away rule
/// (`windows_left_after` for `windows_on_the_glass_after`) — the second window is shut into Recent.
#[test]
fn a_window_beside_a_put_away_one_is_put_away_by_its_close() {
    let mut run = PutAwayRun::open("icon-beside", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let mut run = run.with_a_second_window();
    let _ = run.application.event_loop.ledger.take();
    run.application.folio.on_window_event(
        &run.application.event_loop,
        WindowId::from(2),
        WindowEvent::CloseRequested,
    );
    let effects = run.application.event_loop.ledger.take();
    let second = run
        .application
        .folio
        .windows
        .get_mut(WindowId::from(2))
        .expect("the second window stays the registry's");
    assert!(
        second.put_away && second.leaving.is_none(),
        "the second window is put away"
    );
    assert!(run.app().recent.is_empty(), "no Recent row is filed");
    assert!(
        !effects.contains(&WindowEffect::Exit),
        "and the run goes on: {effects:?}"
    );
}

/// RED (R9; a change to 87a's rule; 87a review N5) — **a tab moved into a put-away window brings
/// it back**, through the one door, holding the tab.
///
/// PLANT 87b-P21: drop the bring-back from `FolioApp::transfer_tab` — the tab lands in a hidden
/// window.
#[test]
fn a_tab_moved_into_a_put_away_window_brings_it_back() {
    let mut run = PutAwayRun::open("icon-move", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    let mut run = run.with_a_second_window_holding(vec![ringing_tab(2, 1), ringing_tab(3, 1)]);
    let _ = run.application.event_loop.ledger.take();
    let moving = run
        .application
        .folio
        .windows
        .get_mut(WindowId::from(2))
        .expect("the second window")
        .tabs[1]
        .id;
    let into = run.application.id;
    let outcome = run
        .application
        .folio
        .transfer_tab(moving, into)
        .expect("the move");
    assert!(
        matches!(outcome, crate::TransferOutcome::Moved(_)),
        "the tab moves"
    );
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true))
            && effects.contains(&WindowEffect::ForegroundTaken),
        "the window is back on the glass and given the keyboard: {effects:?}"
    );
    let window = run.window();
    assert!(!window.put_away, "and no longer put away");
    assert!(
        window.tabs.iter().any(|tab| tab.id == moving),
        "holding the tab"
    );
}
