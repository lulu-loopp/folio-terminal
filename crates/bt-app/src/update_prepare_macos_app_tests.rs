//! **`update_prepare_macos`, as the application drives it.** Tests whose first assertion is about
//! `update_prepare_macos`, written in the crate root's scope rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use crate::test_support::{found, source};
use bt_source::{Pattern, View, needle};

/// RED (ticket 32) — **every seat-to-grid site asks the one function.**
///
/// Four places turn a seat rectangle into a grid — a leaf's birth, the panes on
/// screen, the focused pane and every hidden tab's panes. Each used to call
/// `grid_for_pixels` itself, which is how a rule about the rail could have been
/// honoured at three of them and forgotten at the fourth. Read through
/// `bt_source`, product files only, comments masked.
///
/// MUTATION: put back one raw `renderer.metrics().grid_for_pixels(...)` at any of
/// the four sites — red.
#[test]
fn every_seat_to_grid_site_asks_the_one_function() {
    let calls = found(needle!(Pattern::call("grid_for_pixels")), View::Identifiers)
        .in_the_product(source());
    let owners: Vec<String> = calls
        .owners(source())
        .into_iter()
        .map(|(identity, count)| format!("{}×{count}", identity.name))
        .collect();
    assert_eq!(
        owners,
        vec!["terminal_grid_for×2".to_owned()],
        "{}",
        calls.report(source())
    );
    assert_eq!(calls.outside_items(source()), 0);
}
