//! **`launch_wire`, as the application drives it.** Tests whose first assertion is about
//! `launch_wire`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{cross_metrics, cross_solve, cross_tab};

/// **K121 as re-ruled, both sides of the line in one test.**
///
/// The wash belongs to a *hand-over* and to nothing else. A row reordered
/// inside its own strip was already put where it is, slot by slot, as it
/// travelled (K122); a pane torn out of a layout arrives in a run you were
/// not looking at, and is the arrival the wash exists to announce. The
/// mock-up spends the class in exactly one place for the same reason —
/// `extractPaneToTab` (3517-3542) adds `.landing`, `releaseGrabbed`
/// (6672-6685) never does.
///
/// Both halves are asserted as *tweens the strip would sample*, not as
/// fields that were set, because `sample` is the only question the paint
/// layer ever asks; and both name `Motion::Full`, so a reduced-motion zero
/// cannot pass the reorder half by accident.
///
/// Red gate: put `landing.start` back into `release_drag`'s `Commit` arm and
/// the reorder half goes red; take it out of `tear_pane_into_tab` and the
/// hand-over half does.
#[test]
fn a_reorder_settles_where_a_hand_over_washes() {
    let now = Instant::now();

    let mut reordered = cross_tab(1, &["ALPHA"]);
    reordered.settle_into_slot(40.0, now, Motion::Full);
    let (offset, sliding) = reordered.flip.sample(now, Motion::Full);
    assert!(
        sliding && offset.abs() > 0.0,
        "the last few pixels of travel are the whole of a reorder's release: \
             offset {offset}, sliding {sliding}"
    );
    assert_eq!(
        reordered.landing.sample(now, Motion::Full),
        (0.0, false),
        "and no wash rides along with them — a reorder announces nothing \
             because nothing arrived"
    );

    let mut source = cross_tab(2, &["ALPHA", "BETA"]);
    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        SeatId(2),
        TabId(9),
        now,
        Motion::Full,
        cross_solve,
    )
    .expect("a two-pane tab can spare one");
    let (wash, washing) = torn.landing.sample(now, Motion::Full);
    assert!(
        washing && wash > 0.0,
        "a pane that became a tab crossed a boundary, and that is exactly \
             what the wash is for: wash {wash}, washing {washing}"
    );
}
