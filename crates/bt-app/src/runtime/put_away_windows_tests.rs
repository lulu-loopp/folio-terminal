//! **The put-away run on the roads only Windows has** (0.4.10 plan row 87, parts 87a and 87c;
//! T-TRAY-87C-MACOS, the coordinator's R11 of 2026-11-20).
//!
//! Split out of `put_away_tests` when 87c took that module's gate off: everything there runs on
//! every host, each test naming the platform it pins (`FolioApp::platform`, R1). These are the
//! tests whose road is a Windows message and no value: Folio's quit from outside (the registered
//! message older builds and the Restart Manager send,
//! `bt_platform::session_end::answer_outside_quit`), the system's question at the end of the
//! session (`WM_QUERYENDSESSION`, row 18) and the Restart Manager's close, and the quit asked from
//! the run's icon in the notification area (87e's T10f and T10h, moved here with their plants
//! 87e-P35 and 87e-P37). A Mac's end of the session is AppKit's quit request, which
//! `put_away_dock_tests` drives through its own door.

use super::put_away_tests::PutAwayRun;
use crate::headless::WindowEffect;
use bt_platform::tray::TrayGesture;
use winit::event::WindowEvent;
use winit::window::WindowId;

impl PutAwayRun {
    /// Whether no quit was begun or asked for: nothing could have asked (87e's T10f; here and not
    /// in `put_away_tests`, whose every host would find it unused).
    fn no_quit(&mut self) -> bool {
        let app = self.app();
        app.quit.is_none() && !app.quit_requested
    }
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
    let mut run = PutAwayRun::start_the_run("quit-close-one", true);
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
    let mut run = PutAwayRun::start_the_run("quit-gate", true).with_a_second_window();
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
    let mut run = PutAwayRun::start_the_run("quit-close", true).with_a_second_window();
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

/// RED (T11; R9) — **Folio's quit from outside takes the Restart Manager's close road on a
/// put-away run**: the layout is written, nothing is asked, every window is let go and the loop
/// is asked to exit.
///
/// PLANT P11: answer `None` from `bt_platform::session_end::answer_outside_quit` (the message read
/// as a close, which a put-away window ignores) — nothing is heard, the run goes on.
#[test]
fn the_outside_quit_writes_the_layout_and_leaves() {
    let _news = crate::test_support::EndSessionHome::new("put-away-quit");
    let mut run = PutAwayRun::start_the_run("quit", true);
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
    let mut run = PutAwayRun::start_the_run("end", true);
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

/// RED (T10f; the coordinator's item 18, the judgement's risk 1) — **the system's end, the Restart
/// Manager's close and the quit from outside never ask**, a command running or not: nobody is
/// there to answer. None of the three begins Folio's quit: the system's question is answered and
/// the window stays; the close and the outside quit leave, every window let go. **And a person's
/// card already standing holds none of them**: with the running-work card up, the close and the
/// outside quit still leave, and the system's end leaves the window and its card as they were.
///
/// PLANT 87e-P35: in `FolioApp::settle_session_end_keeping_in`, answer `Settled::Closed` by leaving
/// only when no quit is under way (`closed = !a_quit_holds_it && app.quit.is_none()`) — the
/// standing card holds the close and the outside quit.
#[test]
fn the_system_end_the_restart_managers_close_and_the_outside_quit_never_ask() {
    let _news = crate::test_support::EndSessionHome::new("87e-never-ask");
    for (road, standing) in [
        ("end", false),
        ("close", false),
        ("outside", false),
        ("end", true),
        ("close", true),
        ("outside", true),
    ] {
        let road_now = if standing {
            format!("{road} under a card")
        } else {
            road.to_owned()
        };
        let mut run = PutAwayRun::start_the_run(
            &format!(
                "87e-never-{road}-{}",
                if standing { "card" } else { "bare" }
            ),
            true,
        );
        run.a_command_runs();
        assert!(
            run.application
                .folio
                .every_windows_running_work()
                .expect("the windows are read")
                .any(),
            "{road_now}: the fixture's own precondition: a command runs"
        );
        if standing {
            run.app().ask_to_quit();
            run.application
                .folio
                .begin_quit_if_asked()
                .expect("the quit begins");
            assert!(
                run.application.runtime().quit_card_layout().is_some(),
                "{road_now}: the fixture's own precondition: the running-work card stands"
            );
        }
        match road {
            "end" => assert_eq!(crate::test_support::the_system_asks(), Some(1)),
            "close" => {
                assert_eq!(crate::test_support::the_system_asks(), Some(1));
                assert_eq!(
                    bt_platform::session_end::answer(
                        bt_platform::session_end::WM_ENDSESSION,
                        1,
                        bt_platform::session_end::ENDSESSION_CLOSEAPP,
                        &crate::session_end::hear
                    ),
                    Some(0)
                );
            }
            _ => {
                let quit = 0xC123;
                assert_eq!(
                    bt_platform::session_end::answer_outside_quit(
                        quit,
                        quit,
                        &crate::session_end::hear,
                        &|_| {}
                    ),
                    Some(0)
                );
            }
        }
        let recovery = run.dir.join(crate::preview::RECOVERED_FOLDER);
        let _ = run
            .application
            .folio
            .settle_session_end_keeping_in(&run.application.event_loop, &recovery);
        assert_eq!(
            run.application.folio.windows.len(),
            usize::from(road == "end"),
            "{road_now}: leaves, or stays for the end"
        );
        if standing {
            if road == "end" {
                assert!(
                    run.application.runtime().quit_card_layout().is_some(),
                    "{road_now}: the card still stands"
                );
            }
        } else {
            assert!(run.no_quit(), "{road_now}: nothing asks");
        }
    }
}

/// RED (T10h; R7; the review's P2-5) — **a put-away run's quit brings the window back before it
/// asks**: a question needs a window on the glass, so the one door back is taken first, and the
/// card stands on it. A quit that asks nothing leaves without showing it. Each quit is the icon's
/// Quit, as its menu's post delivers it to the loop's turn (`FolioApp::settle_tray`).
///
/// PLANT 87e-P37: drop the bring-back from `FolioApp::begin_quit_if_asked` — the card stands on a
/// window nobody can see.
#[test]
fn a_put_away_runs_tray_quit_brings_the_window_back_before_it_asks() {
    let mut run = PutAwayRun::start_the_run("87e-brought", true);
    let _ = run.close();
    assert!(run.window().put_away, "the fixture's window is put away");
    run.a_command_runs();
    run.turn_the_tray().expect("the icon is put up");
    let _ = run.application.event_loop.ledger.take();
    run.poke(TrayGesture::Quit);
    run.turn_the_tray().expect("the icon's turn");
    assert!(
        run.app().quit_requested,
        "the icon's Quit asks for the quit"
    );
    run.application
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    let effects = run.application.event_loop.ledger.take();
    assert!(
        effects.contains(&WindowEffect::Visible(true)),
        "the window is shown: {effects:?}"
    );
    assert!(!run.window().put_away, "it is back on the glass");
    assert!(
        run.application.runtime().quit_card_layout().is_some(),
        "and the card stands on it"
    );

    let mut idle = PutAwayRun::start_the_run("87e-not-brought", true);
    let _ = idle.close();
    idle.turn_the_tray().expect("the icon is put up");
    let _ = idle.application.event_loop.ledger.take();
    idle.poke(TrayGesture::Quit);
    idle.turn_the_tray().expect("the icon's turn");
    assert!(
        idle.app().quit_requested,
        "the icon's Quit asks for the quit"
    );
    idle.application
        .folio
        .begin_quit_if_asked()
        .expect("the quit begins");
    assert!(
        !idle
            .application
            .event_loop
            .ledger
            .take()
            .contains(&WindowEffect::Visible(true)),
        "a quit that asks nothing does not show the window"
    );
    assert!(idle.window().put_away);
}
