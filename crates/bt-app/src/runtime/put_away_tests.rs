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
struct PutAway {
    application: HeadlessApplication,
    dir: PathBuf,
}

impl PutAway {
    fn open(tag: &str, keeps_running: bool) -> Self {
        let dir = crate::test_support::disk_scratch(&format!("put-away-{tag}"));
        let mut tab = ringing_tab(1, 2);
        // A name of its own, so a window that goes is a row Recent keeps (`Seed::names_itself`).
        tab.manual_name = Some("build".to_owned());
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
    fn with_a_second_window(mut self) -> Self {
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
            vec![ringing_tab(2, 1)],
            0,
        );
        self.application
            .folio
            .windows
            .insert(WindowId::from(2), second);
        self
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
    fn holding(
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

/// RED (T1; R1, R3) — **with the switch on, the last ordinary window's close puts it away**: the
/// window is hidden and stays the registry's, nothing is asked, no shell is told to end, the run
/// keeps its sentinel, no Recent row is filed and the loop is not asked to exit; and the layout
/// is written down as it was put away.
///
/// PLANT P1: drop the `CloseFate::PutAway` arm of `FolioApp::on_window_event` and the
/// `CloseFate::PutAway` return of `FolioApp::close` — the close ends the run.
#[test]
fn with_the_switch_on_the_last_windows_close_puts_it_away() {
    let mut run = PutAway::open("on", true);
    let effects = run.close();
    assert!(
        effects.contains(&WindowEffect::Visible(false)),
        "the window leaves the glass: {effects:?}"
    );
    assert!(
        !effects.contains(&WindowEffect::Exit),
        "the loop is not asked to exit: {effects:?}"
    );
    let window = run.window();
    assert!(window.put_away, "the window is put away");
    assert!(window.leaving.is_none(), "and not on its way out");
    assert_eq!(window.tabs.len(), 1, "its tab stays");
    assert_eq!(
        window.tabs[0].sessions.len(),
        2,
        "and both its shells, untold"
    );
    assert_eq!(window.dirty_gate.request(), None, "nothing is asked");
    assert_eq!(run.application.folio.windows.len(), 1);
    assert!(run.app().recent.is_empty(), "no Recent row is filed");
    assert!(run.sentinel_stands(), "the run is still running");
}

/// RED (R1, the coordinator's ruling of 2026-11-14 item 3) — **a `WM_CLOSE` that reaches a window
/// already put away is ignored**: there is no `×` on a hidden window, and the tool that sent it
/// does not end the run.
///
/// PLANT: answer `CloseFate::AlreadyAway` as `CloseFate::Shut` in `FolioApp::close_fate` — the
/// second close ends the run.
#[test]
fn a_close_that_reaches_a_put_away_window_is_ignored() {
    let mut run = PutAway::open("again", true);
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
    let mut run = PutAway::open("off", false);
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
    let mut run = PutAway::open("not-last", true).with_a_second_window();
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
    let mut run = PutAway::open("exited", true);
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

/// RED (T6; R10, R13) — **a plain second launch, an explicit `--tab` and a click on one of Folio's
/// notifications each bring the put-away window back as it was**: on the glass and given the
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
    type Road = fn(&mut PutAway) -> Option<WindowId>;
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
    for (road, bring, landed_tabs) in [("plain", plain, 0), ("tab", tab, 1), ("toast", toast, 0)] {
        let mut run = PutAway::open(&format!("back-{road}"), true);
        let (tabs, seats, texts) = run.holding();
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
        let (tabs_after, seats_after, texts_after) = run.holding();
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
    let mut run = PutAway::open("back-minimised", true);
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
    let mut run = PutAway::open("quit", true);
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
    let mut run = PutAway::open("end", true);
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

/// RED (T15; gate 1) — **a put-away window's attention reaches the desktop as a toast**: shown
/// once and not visible now is hidden (`window_is_hidden`), and a hidden window flashes no
/// taskbar button it does not have.
///
/// PLANT P5: drop the visibility term from `runtime::frame::window_is_hidden` — the reach is
/// `Nothing` (the window holds the keyboard and the tab is active).
#[test]
fn a_put_away_windows_reach_is_a_toast() {
    let mut run = PutAway::open("reach", true);
    let _ = run.close();
    let id = run.application.id;
    run.window().window.answers.set(PlatformAnswers {
        visible: Some(false),
        ..PlatformAnswers::default()
    });
    let mut runtime = run.application.folio.runtime(id).expect("the window");
    runtime.observe_window_place();
    let place = runtime.window.observed_place;
    assert!(place.hidden, "the window is hidden");
    assert_eq!(
        crate::notify::desktop_reach(true, place),
        crate::attention::Reach::Toast
    );
    // A window not shown yet is its birth, not its being away.
    runtime.window.window_shown = false;
    runtime.observe_window_place();
    assert!(
        !runtime.window.observed_place.hidden,
        "a window not shown yet"
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
    let mut run = PutAway::open("publish", true);
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
    let (_, _, texts) = run.holding();
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
