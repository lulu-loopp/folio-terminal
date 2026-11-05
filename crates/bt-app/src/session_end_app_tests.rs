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
#[derive(Clone, Debug, Eq, PartialEq)]
enum AtTheEnd {
    Said(String),
    Fenced,
}

/// **The question's turn as the loop's wake brings it**: every piece of parked news settled on
/// `store`, the outcome said as `FolioApp::settle_session_end_keeping_in` says it (plan row 201).
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

/// The question as the window procedure hears it: the platform's answer and `hear`, then the
/// wake taken, and the question's probe line said into `done`, with an empty queue as the
/// question found it.
fn the_system_asks_and_the_wake_is_posted(done: &std::cell::RefCell<Vec<AtTheEnd>>) {
    assert_eq!(the_system_asks(), Some(1), "the question is answered TRUE");
    session_end::posted_with(
        bt_platform::session_end::SessionEnd::Asked,
        Some(false),
        true,
        &mut |line| {
            done.borrow_mut().push(AtTheEnd::Said(line.to_owned()));
        },
    );
}

/// A probe as the end might read it, so that its line is known.
const AT_THE_END: session_end::Probe = session_end::Probe {
    callbacks: (0, None),
    wake: session_end::Wake::TurnBegan,
    queued: Some(false),
};

/// The end's probe line for [`AT_THE_END`].
const AT_THE_END_LINE: &str = "BT_SESSION_END probe at WM_ENDSESSION(TRUE): Folio's winit \
                               callbacks on the stack: 0; the question's wake: taken by its \
                               turn; posted messages queued: no";

/// The end, with its lines and fence recorded into `done`.
fn the_end(done: &std::cell::RefCell<Vec<AtTheEnd>>) -> session_end::Road {
    session_end::end_with(
        &AT_THE_END,
        &mut |line| done.borrow_mut().push(AtTheEnd::Said(line.to_owned())),
        &mut || done.borrow_mut().push(AtTheEnd::Fenced),
    )
}

/// RED (plan row 201) — **the question writes the last change and drops the sentinel, and the end
/// says its probe line and what became of the question's save before it fences the log.**
///
/// Both VM reboots of 2026-11-03 ended Folio about half a second after the question, with no
/// outcome line in the log: what the log was owed was still in its writer's queue. Here a change
/// is recorded inside the debounce's quiet window, so no debounced save carried it; the question
/// comes and its turn writes it (the turn's instant and the write's budget are the test's own);
/// then the end. Every line said from the question on comes before the fence, in order.
///
/// MUTATION (plant `plant-201-a-the-end-says-no-road-line.py`): `end_with` drops the line on the
/// question's save — the order has no road line. MUTATION (plant
/// `plant-201-b-the-end-does-not-fence.py`): `end_with` drops `fence()` — the lines are said
/// and never fenced. MUTATION (plants `plant-201-c-the-end-says-no-probe-line.py` and
/// `plant-201-d-the-question-says-no-probe-line.py`): the end's, or the question's, probe line is
/// not said — the order is short of it.
#[test]
fn the_question_writes_and_the_end_says_its_probe_and_road_and_fences_the_log() {
    on_the_window_thread();
    let home = EndSessionHome::new("end-fences-the-log");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    let last_second = shells_document(&home.0, 2);
    record_as_the_app_does(&mut store, last_second.clone());
    assert!(
        !home.session().exists(),
        "the change is inside its quiet window: nothing has written it"
    );
    let done = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&done);
    the_questions_turn(&mut store, Instant::now(), &done);
    assert_eq!(
        windows_on_disk(&home.session()),
        last_second.windows,
        "the question's turn wrote the last change"
    );
    assert!(!home.sentinel().exists(), "and the run's clean exit stands");

    let road = the_end(&done);
    assert_eq!(road, session_end::Road::Landed);
    let said = done.into_inner();
    let Some(AtTheEnd::Said(question_probe)) = said.first() else {
        panic!("the question's probe line comes first: {said:?}");
    };
    assert_eq!(
        question_probe,
        "BT_SESSION_END probe at WM_QUERYENDSESSION: Folio's winit callbacks on the stack: 0; the \
         question's wake: posted, its turn not begun; posted messages queued: no"
    );
    assert_eq!(
        said[1..],
        [
            AtTheEnd::Said(
                "Folio: the session is ending and its layout was saved: windows 1, tabs 2, \
                 pinned 2"
                    .to_owned()
            ),
            AtTheEnd::Said(AT_THE_END_LINE.to_owned()),
            AtTheEnd::Said(
                "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save had landed".to_owned()
            ),
            AtTheEnd::Fenced,
        ],
        "the outcome, then the end's probe, then the road, then the fence"
    );
}

/// RED (plan row 201) — **the end says what became of the question's save on every road it can
/// find, and fences each time**: a question whose turn never ran, a write that ran out of its
/// budget, a question left to a quit under way (review N3), and no question at all. Each is a
/// span of its own (a home of its own forgets the last).
///
/// MUTATION: in `the_road`, answer `Landed` for a question with no answer — the turn that never
/// ran is said as a landed save.
#[test]
fn the_end_says_what_became_of_the_questions_save_on_every_road() {
    on_the_window_thread();
    let said_at_the_end = |done: std::cell::RefCell<Vec<AtTheEnd>>| -> Vec<AtTheEnd> {
        done.into_inner()
            .into_iter()
            .filter(|entry| match entry {
                AtTheEnd::Said(line) => line.starts_with("BT_SESSION_END WM_ENDSESSION(TRUE)"),
                AtTheEnd::Fenced => true,
            })
            .collect()
    };
    let road = |text: &str| AtTheEnd::Said(format!("BT_SESSION_END WM_ENDSESSION(TRUE): {text}"));

    // Asked, and the turn never ran: nothing written, the sentinel stands.
    let home = EndSessionHome::new("end-turn-not-run");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    let done = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&done);
    assert_eq!(the_end(&done), session_end::Road::TurnNotRun);
    assert_eq!(
        said_at_the_end(done),
        vec![
            road("the question's save had not landed: its turn had not written it"),
            AtTheEnd::Fenced
        ]
    );
    assert!(!home.session().exists(), "nothing was written");
    assert!(
        home.sentinel().is_file(),
        "and the next start offers the layout back"
    );

    // The storage worker held and the budget zero: the question's write ran out of it.
    let stalled = EndSessionHome::new("end-not-landed");
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
    let done = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&done);
    the_questions_turn(&mut store, Instant::now(), &done);
    assert_eq!(the_end(&done), session_end::Road::NotLanded);
    assert_eq!(
        said_at_the_end(done),
        vec![
            road(
                "the question's save had not landed: its write was refused or ran out of its \
                 budget"
            ),
            AtTheEnd::Fenced
        ]
    );
    gate.release();

    // A quit past its photograph writes the layout.
    let quitting = EndSessionHome::new("end-left-to-the-quit");
    let mut store = persist::SessionStore::armed_at(quitting.session(), quitting.sentinel());
    record_as_the_app_does(&mut store, shells_document(&quitting.0, 2));
    let done = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&done);
    for end in session_end::take() {
        assert_eq!(
            session_end::settle(end, &mut store, true, Instant::now()),
            session_end::Settled::LeftToTheQuit
        );
    }
    assert_eq!(the_end(&done), session_end::Road::LeftToTheQuit);
    assert_eq!(
        said_at_the_end(done),
        vec![
            road("the question's save was left to the quit under way"),
            AtTheEnd::Fenced
        ]
    );

    // No question at all.
    let _unasked = EndSessionHome::new("end-never-asked");
    let done = std::cell::RefCell::new(Vec::new());
    assert_eq!(the_end(&done), session_end::Road::NeverAsked);
    assert_eq!(
        said_at_the_end(done),
        vec![
            road("the question's save was never asked"),
            AtTheEnd::Fenced
        ]
    );
}

/// RED (plan row 201, review F2) — **the end fences the log once for the process, however many
/// windows hear it; a shutdown taken back opens a span that fences again.**
///
/// The system sends `WM_ENDSESSION(TRUE)` to every top-level window, and a starved log writer
/// makes each fence wait out its second. Here three windows hear one end: the first says its probe
/// and road lines and fences; the other two say the log was fenced and return. Then the shutdown
/// is taken back and asked again, and the next end fences.
///
/// MUTATION: in `end_with`, drop the `FENCED` check — every window fences again. Drop the
/// `FENCED` reset on `TakenBack` in `settle` — the next span's end fences nothing.
#[test]
fn the_end_fences_the_log_once_however_many_windows_hear_it() {
    on_the_window_thread();
    let home = EndSessionHome::new("end-fences-once");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    let asked = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&asked);
    the_questions_turn(&mut store, Instant::now(), &asked);
    let done = std::cell::RefCell::new(Vec::new());
    let roads: Vec<_> = (0..3).map(|_window| the_end(&done)).collect();
    assert_eq!(
        roads,
        vec![
            session_end::Road::Landed,
            session_end::Road::FencedBefore,
            session_end::Road::FencedBefore,
        ]
    );
    let fenced_before = AtTheEnd::Said(
        "BT_SESSION_END WM_ENDSESSION(TRUE): an earlier window's end fenced the log".to_owned(),
    );
    assert_eq!(
        done.take(),
        vec![
            AtTheEnd::Said(AT_THE_END_LINE.to_owned()),
            AtTheEnd::Said(
                "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save had landed".to_owned()
            ),
            AtTheEnd::Fenced,
            fenced_before.clone(),
            fenced_before,
        ],
        "one probe, one road line and one fence for the three windows"
    );

    // Taken back, then asked again: a span of its own, whose end fences.
    assert_eq!(
        bt_platform::session_end::answer(
            bt_platform::session_end::WM_ENDSESSION,
            0,
            0,
            &session_end::hear,
        ),
        Some(0)
    );
    for end in session_end::take() {
        let _ = session_end::settle(end, &mut store, false, Instant::now());
    }
    the_system_asks_and_the_wake_is_posted(&asked);
    the_questions_turn(&mut store, Instant::now(), &asked);
    let mut fences = 0_u32;
    let road = session_end::end_with(&AT_THE_END, &mut |_| {}, &mut || fences += 1);
    assert_eq!(road, session_end::Road::Landed);
    assert_eq!(fences, 1, "the next span's end fences again");
}

/// RED (plan row 201, re-review RR-N4) — **a question heard after a shutdown taken back opens
/// the next span on the window procedure's stack**: its probe line is said and its end fences
/// again, though no loop turn has settled the shutdown taken back between them; and when the
/// turn comes, settling that news does not let go of the new span's hold.
///
/// MUTATION: in `hear`, drop the span's reset — the second end says `an earlier window's end
/// fenced the log` and the second question says no probe line. In `settle`'s `TakenBack` arm,
/// let go of the hold whatever span is open — the document is not held under the new question.
#[test]
fn a_question_after_a_shutdown_taken_back_opens_the_next_span_before_any_turn() {
    on_the_window_thread();
    let home = EndSessionHome::new("next-span");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    store.set_save_budget(crate::lane::PATIENCE);
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    let asked = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&asked);
    the_questions_turn(&mut store, Instant::now(), &asked);
    let mut fences = 0_u32;
    assert_eq!(
        session_end::end_with(&AT_THE_END, &mut |_| {}, &mut || fences += 1),
        session_end::Road::Landed
    );

    // Taken back and asked again on the procedure's stack, with no turn between them.
    assert_eq!(
        bt_platform::session_end::answer(
            bt_platform::session_end::WM_ENDSESSION,
            0,
            0,
            &session_end::hear,
        ),
        Some(0)
    );
    let again = std::cell::RefCell::new(Vec::new());
    the_system_asks_and_the_wake_is_posted(&again);
    assert!(
        matches!(again.borrow().as_slice(), [AtTheEnd::Said(line)]
            if line.starts_with("BT_SESSION_END probe at WM_QUERYENDSESSION:")),
        "the next span's question says its probe line: {:?}",
        again.borrow()
    );
    let mut said = Vec::new();
    assert_eq!(
        session_end::end_with(
            &AT_THE_END,
            &mut |line| said.push(line.to_owned()),
            &mut || fences += 1
        ),
        session_end::Road::TurnNotRun
    );
    assert_eq!(fences, 2, "the next span's end fences again: {said:?}");

    // The turn comes: the shutdown taken back, then the new question.
    let news = session_end::take();
    assert_eq!(
        news,
        [
            bt_platform::session_end::SessionEnd::TakenBack,
            bt_platform::session_end::SessionEnd::Asked
        ]
    );
    assert_eq!(
        session_end::settle(news[0], &mut store, false, Instant::now()),
        session_end::Settled::TakenBack
    );
    assert!(
        session_end::holds_the_document(),
        "the new span's hold stands through the old span's taken-back news"
    );
    assert!(matches!(
        session_end::settle(news[1], &mut store, false, Instant::now()),
        session_end::Settled::Saved(Ok(()))
    ));
    assert!(!home.sentinel().exists(), "the new span's question wrote");
}

/// RED (plan row 201) — **the probe counts `FolioApp`'s winit callbacks on the stack, follows the
/// question's wake from its post to its turn, and the question's probe line is said once per
/// span.**
///
/// The probe is plan row 206's evidence: whether the question can arrive inside one of Folio's
/// callbacks, and whether its wake was still queued, held by winit or taken by its turn when the
/// end came.
///
/// MUTATION: in `InACallback::drop`, leave the count where it is — the stack reads 1 after the
/// callback returned. In `posted_with`, drop the `NeverPosted` check — the second window's
/// question says a second probe line. In `take`, leave the wake `Posted` — the turn that took it
/// is never seen. In `posted_with`, read the queue again (`Probe::now()`) instead of the one the
/// question found — the line says what the thread's queue holds after Folio's own post
/// (re-review RR-F1), not `yes` as handed.
#[test]
fn the_probe_counts_folios_callbacks_and_follows_the_questions_wake() {
    on_the_window_thread();
    let _home = EndSessionHome::new("probe");
    assert_eq!(session_end::Probe::now().callbacks, (0, None));
    {
        let _outer = session_end::in_a_callback("user_event");
        assert_eq!(session_end::Probe::now().callbacks, (1, Some("user_event")));
        {
            let _inner = session_end::in_a_callback("window_event");
            assert_eq!(
                session_end::Probe::now().callbacks,
                (2, Some("window_event"))
            );
        }
        assert_eq!(session_end::Probe::now().callbacks, (1, Some("user_event")));
    }
    assert_eq!(session_end::Probe::now().callbacks, (0, None));

    assert_eq!(
        session_end::Probe::now().wake,
        session_end::Wake::NeverPosted
    );
    let mut said = Vec::new();
    let _callback = session_end::in_a_callback("user_event");
    assert_eq!(the_system_asks(), Some(1));
    session_end::posted_with(
        bt_platform::session_end::SessionEnd::Asked,
        Some(true),
        true,
        &mut |line| said.push(line.to_owned()),
    );
    // The same question, sent to the next window.
    session_end::posted_with(
        bt_platform::session_end::SessionEnd::Asked,
        Some(false),
        true,
        &mut |line| said.push(line.to_owned()),
    );
    assert_eq!(
        said,
        [
            "BT_SESSION_END probe at WM_QUERYENDSESSION: Folio's winit callbacks on the stack: 1 \
          (innermost user_event); the question's wake: posted, its turn not begun; posted \
          messages queued: yes"
        ],
        "one probe line for the span, with the queue as the question found it"
    );
    assert_eq!(session_end::Probe::now().wake, session_end::Wake::Posted);
    let _ = session_end::take();
    assert_eq!(session_end::Probe::now().wake, session_end::Wake::TurnBegan);

    // A refused post, in a span of its own.
    let _refused = EndSessionHome::new("probe-refused");
    said.clear();
    assert_eq!(the_system_asks(), Some(1));
    session_end::posted_with(
        bt_platform::session_end::SessionEnd::Asked,
        None,
        false,
        &mut |line| said.push(line.to_owned()),
    );
    assert_eq!(session_end::Probe::now().wake, session_end::Wake::Refused);
    assert_eq!(said.len(), 1);
    assert_eq!(
        session_end::probe_line(
            "WM_ENDSESSION(TRUE)",
            &session_end::Probe {
                callbacks: (2, Some("window_event")),
                wake: session_end::Wake::Refused,
                queued: None,
            }
        ),
        "BT_SESSION_END probe at WM_ENDSESSION(TRUE): Folio's winit callbacks on the stack: 2 \
         (innermost window_event); the question's wake: its post refused; posted messages \
         queued: unknown"
    );
}

/// RED (0.4.10 plan row 17) — **before AppKit is answered `NSTerminateNow`, the quit says what
/// became of its save and then fences the log, once for the process; and the request says why
/// AppKit asked.**
///
/// On macOS `-[NSApplication terminate:]` goes on to `exit()` once it is answered, and `fn main`'s
/// way out, its fence among it, is never reached (X-4 r3 and r5: `WINIT exiting` 7–12 ms after
/// the answer, then nothing). So the one line on the save and the fence are said and run before
/// the answer, on every road the close can leave, and a second end in the process fences nothing.
///
/// MUTATION (plants 17-a, 17-b): drop the fence from `the_systems_quit_ends_with`, or drop its
/// road line.
#[test]
fn the_systems_quit_says_what_became_of_its_save_and_fences_the_log() {
    for (save, line) in [
        (
            persist::ClosedSave::Landed,
            "BT_SESSION_END applicationShouldTerminate: answering NSTerminateNow; the quit's save \
             had landed and session.lock is gone",
        ),
        (
            persist::ClosedSave::NotLanded,
            "BT_SESSION_END applicationShouldTerminate: answering NSTerminateNow; the quit's save \
             had not landed within its budget, and session.lock stands",
        ),
        (
            persist::ClosedSave::NoSession,
            "BT_SESSION_END applicationShouldTerminate: answering NSTerminateNow; this run had no \
             session to save",
        ),
    ] {
        session_end::forget();
        let done = std::cell::RefCell::new(Vec::new());
        let fenced = session_end::the_systems_quit_ends_with(
            save,
            &mut |said| done.borrow_mut().push(AtTheEnd::Said(said.to_owned())),
            &mut || done.borrow_mut().push(AtTheEnd::Fenced),
        );
        assert!(fenced, "the first end fences: {save:?}");
        assert_eq!(
            done.take(),
            vec![AtTheEnd::Said(line.to_owned()), AtTheEnd::Fenced],
            "the line on the save, then the fence: {save:?}"
        );
        let again = session_end::the_systems_quit_ends_with(
            save,
            &mut |said| done.borrow_mut().push(AtTheEnd::Said(said.to_owned())),
            &mut || done.borrow_mut().push(AtTheEnd::Fenced),
        );
        assert!(!again, "a second end fences nothing: {save:?}");
        assert_eq!(
            done.take(),
            vec![AtTheEnd::Said(
                "BT_SESSION_END applicationShouldTerminate: an earlier end fenced the log"
                    .to_owned()
            )],
            "and says the log was fenced: {save:?}"
        );
    }
    session_end::forget();
    assert_eq!(
        session_end::the_systems_quit_asked_line(bt_platform::QuitReason::LogOut),
        "BT_SESSION_END applicationShouldTerminate: asked (log out)"
    );
    assert_eq!(
        session_end::the_systems_quit_asked_line(bt_platform::QuitReason::NoneGiven),
        "BT_SESSION_END applicationShouldTerminate: asked (no reason given)"
    );
}
