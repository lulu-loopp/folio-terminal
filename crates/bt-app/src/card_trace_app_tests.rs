//! **`card_trace`, as the application drives it.** Tests whose first assertion is about
//! `card_trace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{card_restore_first, card_restore_fixture, card_restore_widen};

/// Reflow shrinks the reachable maximum from 136 to 116. The drawing is held
/// to 116 and the stored 130 is not (T-CARD-NO-PASSIVE-CLAMP): a number cut
/// down by a width the pane wore on its way somewhere else is a reader's
/// place spent by nobody.
#[test]
fn card_restore_keeps_stored_skip_across_reflow() {
    let mut leaf = card_restore_fixture();
    card_restore_widen(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "H001");
    assert_eq!(
        leaf.card_skip, 130,
        "the reflow drew the card, not the leaf"
    );
    // And the hand still has no debt to pay off: the notch clamps on the way
    // in, so the first reverse from the visible 116 moves one row.
    aim_card_window(&mut leaf, 4, -1, card_trace::Card::untraced());
    assert_eq!(leaf.card_skip, 115);
    assert_eq!(card_restore_first(&leaf), "H002");
}
