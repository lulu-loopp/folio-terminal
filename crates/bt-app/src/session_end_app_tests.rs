//! **`session_end`, as the application drives it.** Tests whose first assertion is about
//! `session_end`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    EndSessionHome, on_the_window_thread, record_as_the_app_does, shells_document, the_system_asks,
    windows_on_disk,
};

/// RED (B-ENDSESSION) — **the system's question holds the document, writes it through the quit's
/// road and drops the run's sentinel; nothing that happens after it changes the file.**
///
/// The owner's reboot of 2026-09-27 came back with every pinned tab short of its panes and the
/// run marked unclean: `WM_QUERYENDSESSION` went to `DefWindowProc`, the system ended the shells,
/// each death closed a pane, and the autosave wrote what was left. Here the real platform answer
/// hears the question, the real store writes the held document through the one writer and the
/// one bounded wait, and the "pane closed" recordings that follow — the reap's — are refused.
///
/// MUTATION: in `session_end::hear`, park the question without holding the document — the
/// shrunken layout is what lands, and `session.lock` is still dropped over it.
#[test]
fn a_query_end_session_freezes_writes_and_drops_the_sentinel() {
    on_the_window_thread();
    let home = EndSessionHome::new("query");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    // The writes are real and awaited within the suite's patience: the hold is the subject, not
    // the product's three-second budget.
    store.set_save_budget(crate::lane::PATIENCE);
    assert!(home.sentinel().is_file(), "the run's sentinel stands");
    let whole = shells_document(&home.0, 2);
    record_as_the_app_does(&mut store, whole.clone());

    assert_eq!(the_system_asks(), Some(1), "the question is answered TRUE");
    // The system ends one shell of each tab; the reap closes their panes and records.
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false, Instant::now()))
        .collect();
    assert_eq!(settled, vec![session_end::Settled::Saved(Ok(()))]);
    assert_eq!(
        windows_on_disk(&home.session()),
        whole.windows,
        "the layout on the disk is the one before the system's question"
    );
    assert!(
        !home.sentinel().exists(),
        "and the run claims its clean exit, as a quit does"
    );

    // Later "pane closed" edits, and a write forced after them: the file does not move.
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    assert_eq!(windows_on_disk(&home.session()), whole.windows);
}

/// RED (plan row 191) — **the system's end says what became of the layout, in one line**: a
/// landed write with the windows, tabs and pins it held, a refused one with why, the quit's own
/// write named; news that wrote nothing says nothing (its arrival is said by the platform).
///
/// MUTATION (plant `plant-191-f-a-landed-save-says-nothing.py`): answer `None` for a landed save —
/// the shutdown that saved is silent again, which is what row 191 had to diagnose from. MUTATION
/// (plant `plant-191-h-a-refused-save-says-nothing.py`): answer `None` for a refused one.
#[test]
fn the_systems_end_says_what_became_of_the_layout() {
    let document = shells_document(Path::new("work"), 1);
    assert_eq!(
        session_end::outcome_line(&session_end::Settled::Saved(Ok(())), &document).as_deref(),
        Some("Folio: the session is ending and its layout was saved: windows 1, tabs 2, pinned 2")
    );
    for refusal in [
        crate::persist::SaveRefusal::Refused("the disk refused the write".to_owned()),
        crate::persist::SaveRefusal::TimedOut("the write did not finish in time".to_owned()),
    ] {
        let said = refusal.message().to_owned();
        assert_eq!(
            session_end::outcome_line(&session_end::Settled::Saved(Err(refusal)), &document),
            Some(format!(
                "Folio: the session is ending and its layout was not saved: {said}"
            )),
            "a refused write says so, with why"
        );
    }
    assert_eq!(
        session_end::outcome_line(&session_end::Settled::LeftToTheQuit, &document).as_deref(),
        Some("Folio: the session is ending and the quit under way writes its layout")
    );
    for silent in [
        session_end::Settled::AlreadySaved,
        session_end::Settled::TakenBack,
        session_end::Settled::Closed,
    ] {
        assert_eq!(
            session_end::outcome_line(&silent, &document),
            None,
            "{silent:?} wrote nothing"
        );
    }
}

/// RED (B-ENDSESSION) — **a shutdown taken back lets the document go and puts the sentinel
/// back: the run goes on, and so do its saves.**
///
/// `WM_ENDSESSION` with `FALSE` — another program, or the person at the shutdown screen, stopped
/// it. Without this the rest of the run would never save its layout again, and a crash after it
/// would pass for a clean exit.
///
/// MUTATION: in `session_end::settle`'s `TakenBack` arm, leave the document held, or drop the
/// `rearm_after_the_systems_end` call.
#[test]
fn a_shutdown_taken_back_lets_the_document_go_and_puts_the_sentinel_back() {
    on_the_window_thread();
    let home = EndSessionHome::new("taken-back");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    assert_eq!(
        bt_platform::session_end::answer(
            bt_platform::session_end::WM_ENDSESSION,
            0,
            0,
            &session_end::hear
        ),
        Some(0),
        "a shutdown taken back is processed"
    );
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false, Instant::now()))
        .collect();
    assert_eq!(
        settled,
        vec![
            session_end::Settled::Saved(Ok(())),
            session_end::Settled::TakenBack
        ]
    );
    assert!(home.sentinel().is_file(), "the run is running again");
    assert!(!session_end::holds_the_document());

    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 1).windows,
        "a change after the shutdown was taken back is saved"
    );
}

/// RED (T-ENDSESSION-SAVES) — **a shutdown taken back after its write's wait ran out leaves a
/// writer that is waited on again**: the next save that waits — a quit's — waits for its
/// document within its budget, instead of answering "did not finish" at once for the rest of the
/// run.
///
/// The storage worker is held where the document's write begins, and the wait has a budget of
/// zero: the question's write does not land, and the writer is marked stalled. The system takes
/// the end back; the worker is let go; a later change is written and waited for.
///
/// MUTATION: in `session_end::settle`'s `TakenBack` arm, drop the
/// `wait_again_after_the_systems_end` call — the later wait answers `TimedOut` at once.
#[test]
fn a_shutdown_taken_back_after_its_wait_ran_out_is_waited_for_again() {
    on_the_window_thread();
    let home = EndSessionHome::new("stall-cleared");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    gate.hold();
    let mut store = persist::SessionStore::at_waking_started_by(
        home.session(),
        home.sentinel(),
        || {},
        crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    store.set_save_budget(Duration::ZERO);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    assert_eq!(
        bt_platform::session_end::answer(
            bt_platform::session_end::WM_ENDSESSION,
            0,
            0,
            &session_end::hear
        ),
        Some(0)
    );
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false, Instant::now()))
        .collect();
    assert!(
        matches!(
            settled.as_slice(),
            [
                session_end::Settled::Saved(Err(persist::SaveRefusal::TimedOut(_))),
                session_end::Settled::TakenBack
            ]
        ),
        "the question's write ran out of its budget, then the end was taken back: {settled:?}"
    );

    gate.release();
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()), "the later wait is a real one");
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 1).windows
    );
}

/// What the end did, in order: each line it said and its fence (plan row 201).
#[derive(Debug, Eq, PartialEq)]
enum AtTheEnd {
    Said(String),
    Fenced,
}

/// **The question's turn as the loop's wake brings it**: every piece of parked news settled on
/// `store`, the outcome said as `FolioApp::settle_session_end_keeping_in` says it — the turn the
/// end's drain runs when it finds the wake still queued (plan row 201).
fn the_questions_turn(
    store: &mut persist::SessionStore,
    asked_at: Instant,
    done: &std::cell::RefCell<Vec<AtTheEnd>>,
) {
    for end in session_end::take() {
        let settled = session_end::settle(end, store, false, asked_at);
        if let Some(line) = session_end::outcome_line(&settled, store.loaded()) {
            done.borrow_mut().push(AtTheEnd::Said(line));
        }
    }
}

/// RED (plan row 201) — **`WM_ENDSESSION(TRUE)` that overtakes the question's wake is answered
/// only after the question's turn has run: the last-second change is on the disk, the sentinel
/// is gone, and the outcome line and the end's own line are said before the log's fence.**
///
/// Both VM reboots of 2026-11-03 ended Folio about half a second after the question, with no
/// outcome line in the log. Here the two messages arrive with no loop turn between them: the
/// question (the real platform answer and `hear`), then the end, whose drain finds the question's
/// wake still queued and runs it. The change was recorded a moment before the question, inside
/// the debounce's quiet window, so no debounced save carried it: only the question's write can
/// put it on the disk. The turn's instant is the test's own (`asked`), and the write's wait is
/// the store's own budget (the suite's patience); nothing in the end waits.
///
/// MUTATION (plant `plant-201-a-the-end-returns-at-once.py`): `hold_at_the_end` answers at once
/// without running the queued turn — the file is absent, the sentinel stands, no outcome line.
/// MUTATION (plant `plant-201-b-the-end-does-not-fence.py`): drop the `fence()` call in
/// `end_with` — the lines are said and never fenced.
#[test]
fn the_end_runs_the_questions_turn_and_fences_the_log_before_it_answers() {
    on_the_window_thread();
    let home = EndSessionHome::new("end-runs-the-turn");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    let last_second = shells_document(&home.0, 2);
    record_as_the_app_does(&mut store, last_second.clone());
    assert!(
        !home.session().exists(),
        "the change is inside its quiet window: nothing has written it"
    );
    assert_eq!(the_system_asks(), Some(1), "the question is answered TRUE");

    let asked = Instant::now();
    let done = std::cell::RefCell::new(Vec::new());
    let mut drained = 0_u32;
    let road = session_end::end_with(
        &mut |answered| {
            assert!(!answered(), "the end came before the question's turn");
            drained += 1;
            the_questions_turn(&mut store, asked, &done);
            answered()
        },
        &mut |line| done.borrow_mut().push(AtTheEnd::Said(line.to_owned())),
        &mut || done.borrow_mut().push(AtTheEnd::Fenced),
    );

    assert_eq!(road, session_end::Road::RanItHere);
    assert_eq!(drained, 1, "the queued messages ran once");
    assert_eq!(
        windows_on_disk(&home.session()),
        last_second.windows,
        "the last-second change is on the disk when the end is answered"
    );
    assert!(!home.sentinel().exists(), "and the run's clean exit stands");
    assert_eq!(
        done.into_inner(),
        vec![
            AtTheEnd::Said(
                "Folio: the session is ending and its layout was saved: windows 1, tabs 2, \
                 pinned 2"
                    .to_owned()
            ),
            AtTheEnd::Said(
                "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had not run; the \
                 messages already queued ran it"
                    .to_owned()
            ),
            AtTheEnd::Fenced,
        ],
        "the outcome, then the road, then the fence"
    );
}

/// RED (plan row 201) — **an end that finds the question's turn already run runs nothing, says
/// so and fences; one whose write ran out of its budget does not spend it again.**
///
/// The first span: the question's turn runs first (the usual order: it holds the window thread
/// until its write lands, so the end cannot be dispatched before). The second: the storage
/// worker is held and the write's budget is zero, so the question's write did not finish — the
/// end does not wait for it again. The second home forgets the first's news, so it is a span of
/// its own.
///
/// MUTATION: in `hold_at_the_end`, drop the `the_question_has_its_answer()` check before the
/// drain — the end runs the queue again, and the drain below is called.
#[test]
fn the_end_after_the_questions_turn_runs_nothing_and_does_not_wait_again() {
    on_the_window_thread();
    let home = EndSessionHome::new("end-after-the-turn");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    let done = std::cell::RefCell::new(Vec::new());
    the_questions_turn(&mut store, Instant::now(), &done);
    let mut unexpected = |_: &dyn Fn() -> bool| -> bool {
        panic!("the question's turn had run: nothing is drained")
    };
    let road = session_end::end_with(
        &mut unexpected,
        &mut |line| done.borrow_mut().push(AtTheEnd::Said(line.to_owned())),
        &mut || done.borrow_mut().push(AtTheEnd::Fenced),
    );
    assert_eq!(road, session_end::Road::AlreadyRan);
    assert_eq!(
        done.into_inner(),
        vec![
            AtTheEnd::Said(
                "Folio: the session is ending and its layout was saved: windows 1, tabs 2, \
                 pinned 2"
                    .to_owned()
            ),
            AtTheEnd::Said(
                "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had already run"
                    .to_owned()
            ),
            AtTheEnd::Fenced,
        ]
    );
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 2).windows
    );

    // A second home: the storage worker held, the write's budget zero.
    let stalled = EndSessionHome::new("end-after-a-spent-budget");
    let gate = std::sync::Arc::new(crate::lane::Gate::default());
    gate.hold();
    let mut store = persist::SessionStore::at_waking_started_by(
        stalled.session(),
        stalled.sentinel(),
        || {},
        crate::lane::Executor::scripted(std::sync::Arc::clone(&gate)),
    );
    store.set_save_budget(Duration::ZERO);
    record_as_the_app_does(&mut store, shells_document(&stalled.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    let settled: Vec<_> = session_end::take()
        .into_iter()
        .map(|end| session_end::settle(end, &mut store, false, Instant::now()))
        .collect();
    assert!(
        matches!(
            settled.as_slice(),
            [session_end::Settled::Saved(Err(
                persist::SaveRefusal::TimedOut(_)
            ))]
        ),
        "the question's write ran out of its budget: {settled:?}"
    );
    let mut said = Vec::new();
    let road = session_end::end_with(
        &mut unexpected,
        &mut |line| said.push(line.to_owned()),
        &mut || {},
    );
    assert_eq!(
        road,
        session_end::Road::AlreadyRan,
        "a spent budget is an answer, and it is not spent again"
    );
    assert_eq!(said.len(), 1);
    gate.release();
}

/// RED (plan row 201) — **an end whose queued messages did not run the question's turn says so,
/// and still fences.** The loop was inside its own handler when the question came, so the wake
/// waits for after the end (winit holds an event that arrives inside its handler): the drain
/// runs what is queued and finds no answer. Nothing is written; the sentinel stands, and the next
/// start offers the layout back.
///
/// MUTATION: in `hold_at_the_end`, answer `RanItHere` whatever the drain says — the log says the
/// turn ran when it did not.
#[test]
fn the_end_says_when_the_queued_messages_did_not_run_the_questions_turn() {
    on_the_window_thread();
    let home = EndSessionHome::new("end-could-not-run");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert_eq!(the_system_asks(), Some(1));
    let done = std::cell::RefCell::new(Vec::new());
    let road = session_end::end_with(
        &mut |answered| answered(),
        &mut |line| done.borrow_mut().push(AtTheEnd::Said(line.to_owned())),
        &mut || done.borrow_mut().push(AtTheEnd::Fenced),
    );
    assert_eq!(road, session_end::Road::CouldNotRun);
    assert_eq!(
        done.into_inner(),
        vec![
            AtTheEnd::Said(
                "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had not run, and the \
                 messages already queued did not run it"
                    .to_owned()
            ),
            AtTheEnd::Fenced,
        ]
    );
    assert!(!home.session().exists(), "nothing was written");
    assert!(
        home.sentinel().is_file(),
        "and the next start offers the layout back"
    );
}
