//! **`persist`, as the application drives it.** Tests whose first assertion is about
//! `persist`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    EndSessionHome, launch_plan_on_disk, on_the_window_thread, record_as_the_app_does,
    shells_document, windows_on_disk,
};

/// PIN (B-ENDSESSION) — **without the system's question, a shell that exits still closes its
/// pane, and the layout on the disk says so.**
///
/// The behaviour the hold must not touch: a shell that ends for its own reasons (`exit`, a crash)
/// is a change the reader made, recorded and written as before; the run's sentinel stands.
///
/// MUTATION: make `session_end::holds_the_document` answer `true` — the closed pane never
/// reaches the file.
#[test]
fn an_ordinary_shell_exit_without_a_freeze_still_closes_the_pane() {
    on_the_window_thread();
    let home = EndSessionHome::new("ordinary");
    let mut store = persist::SessionStore::armed_at(home.session(), home.sentinel());
    record_as_the_app_does(&mut store, shells_document(&home.0, 2));
    assert!(bt_platform::admission::exiting());
    assert_eq!(store.flush_judged(), Ok(()));
    record_as_the_app_does(&mut store, shells_document(&home.0, 1));
    assert_eq!(store.flush_judged(), Ok(()));

    assert!(!session_end::holds_the_document());
    assert_eq!(
        windows_on_disk(&home.session()),
        shells_document(&home.0, 1).windows,
        "the closed pane is gone from the layout"
    );
    let (_, shapes) = launch_plan_on_disk(&home.session());
    let one = (
        vec![bt_layout::SeatKind::Terminal],
        vec![Some(home.0.clone())],
    );
    assert_eq!(shapes, vec![one.clone(), one]);
    assert!(
        home.sentinel().is_file(),
        "and nothing claimed a clean exit"
    );
}
