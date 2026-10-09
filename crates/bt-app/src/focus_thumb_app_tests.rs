//! **`focus_thumb`, as the application drives it.** Tests whose first assertion is about
//! `focus_thumb`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    card_restore_first, card_restore_fixture, card_restore_resize, card_restore_settle,
};

#[test]
fn card_restore_alternate_screen_excludes_primary_history() {
    let mut leaf = card_restore_fixture();
    leaf.session.feed(b"\x1b[?1049h").unwrap();
    let text = (1..=20)
        .map(|number| format!("A{number:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    leaf.session.feed(text.as_bytes()).unwrap();
    // The card sees only alternate rows, never primary history: the stored
    // 130 (a place among the primary's 120 rows) is *drawn* at the alternate
    // screen's own 36, and the top of that screen is the blank the twenty
    // lines scrolled past on their way up from the saved cursor. The number
    // itself stands, because an app that took the screen for a moment is not
    // a reader deciding to read somewhere else (T-CARD-NO-PASSIVE-CLAMP) —
    // the clamp that meets the alternate screen is the next notch's, on the
    // way in.
    assert_eq!(card_restore_first(&leaf), "");
    assert_eq!(leaf.card_skip, 130);
    aim_card_window(&mut leaf, 4, i32::MIN, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "A017");
    aim_card_window(&mut leaf, 4, 10, card_trace::Card::untraced());
    assert_eq!(card_restore_first(&leaf), "A007");
    card_restore_resize(&mut leaf, 40, 40, LeafOnStage::Shown);
    assert_eq!(card_restore_first(&leaf), "A007");
    card_restore_settle(&mut leaf);
    assert_eq!(card_restore_first(&leaf), "A007");
    let assembled = focus_thumb::transcript_tail(&leaf.session, 40, 200, 0).0;
    let nonblank = assembled
        .iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(nonblank.len(), 20);
    assert!(
        nonblank
            .iter()
            .all(|line| line.trim_start().starts_with('A')),
        "primary content leaked into the alternate card: {assembled:?}"
    );
}
