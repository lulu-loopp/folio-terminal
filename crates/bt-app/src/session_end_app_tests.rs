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
