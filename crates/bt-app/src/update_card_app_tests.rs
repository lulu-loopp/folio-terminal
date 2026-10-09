//! **`update_card`, as the application drives it.** Tests whose first assertion is about
//! `update_card`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{arriving_as, buffer_saying, cross_tab, tab_with_a_preview};

/// **P121/§7.1.3 「若是原 tab 最后一个预览 pane 则整池随行」.**
///
/// A merge takes every seat the source had, so its last preview pane is
/// leaving by construction. The two buffers no pane was showing are the point
/// of the clause: they are the tab's *history*, one of them dirty, and the
/// ruling's own argument is that an orphaned dirty buffer must stay reachable
/// somewhere. Left behind on a tab that is about to stop existing, it is
/// reachable nowhere — and the dirty gate that would have named it goes down
/// with the same tab.
///
/// MUTATION ②: drop the `merge_from` line from `absorb_tab_sessions` and the
/// count goes to 0 while the source keeps all three — the pool stranded on a
/// dissolving tab, which is the shape of the bug.
#[test]
fn the_last_preview_pane_leaving_a_tab_takes_the_whole_pool_with_it() {
    let shown = buffer_saying(r"D:\notes\todo.txt", "todo.txt", "milk\n");
    let history = buffer_saying(r"D:\notes\README.md", "README.md", "# hi\n");
    let mut stranded = buffer_saying(r"D:\notes\draft.txt", "draft.txt", "half a thought");
    stranded.dirty = true;

    let (mut source, _) = tab_with_a_preview(1, vec![shown, history, stranded]);
    let arrived = arriving_as(&source, 90);
    let mut target = cross_tab(2, &["ALPHA"]);
    absorb_tab_sessions(&mut source, &mut target, &arrived);

    assert_eq!(
        target.preview_pool.len(),
        3,
        "the whole pool moved, not just the buffer on screen"
    );
    assert_eq!(
        target.preview_pool.dirty_names(None).collect::<Vec<_>>(),
        vec!["draft.txt"],
        "so the gate that speaks for the unsaved one still has it to name"
    );
    assert_eq!(
        source.preview_pool.len(),
        0,
        "and it moved — a second copy on a dissolving tab is the fork the law forbids"
    );
}
