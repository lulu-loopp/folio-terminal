//! **`preview_wrap`, as the application drives it.** Tests whose first assertion is about
//! `preview_wrap`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// **A drop that was aimed at no shell types nothing** (review X-1).
///
/// Chrome, a files column, a preview pane, a pane whose shell has gone: all
/// of them answer `None` at the arrival, and `None` is carried rather than
/// re-asked at the flush. The claim is that the batch is still assembled and
/// still spent — the drop is not *lost*, it is delivered to nobody — which
/// is what keeps a second drop from finding the first one still standing.
#[test]
fn a_drop_aimed_at_no_shell_is_still_collected_and_still_spent() {
    let mut standing: Option<DropBatch> = None;
    let at = PhysicalPosition::new(5.0, 5.0);
    DropBatch::collect(&mut standing, "/one".into(), Some(at), None);
    DropBatch::collect(&mut standing, "/two".into(), Some(at), None);
    let batch = standing.take().expect("the drop opened all the same");
    assert_eq!(batch.target, None, "there was nothing under the hand");
    assert_eq!(batch.point, Some(at), "but the window still knows where");
    assert_eq!(batch.paths.len(), 2);
    assert!(
        standing.is_none(),
        "and the batch is spent, not left to rot"
    );
}
