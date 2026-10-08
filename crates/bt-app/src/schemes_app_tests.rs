//! **`schemes`, as the application drives it.** Tests whose first assertion is about
//! `schemes`, written in the crate root's scope rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use crate::test_support::{flush_test_wheel, wheel_pane_at_top};

#[test]
fn wheel_flush_at_the_top_clamp_publishes_no_frame() {
    let mut pane = wheel_pane_at_top();
    let revision = pane.content_revision;
    assert!(!flush_test_wheel(&mut pane, 1.0));
    assert_eq!(pane.publications, 0);
    assert_eq!(pane.content_revision, revision);
    assert!(!pane.present_pending());
    // The former unconditional wheel publish fails the zero-frame rule.
    assert!(pane.publish_expose_frame());
    assert_eq!(pane.publications, 1);
}
