//! **`seed`, as the application drives it.** Tests whose first assertion is about
//! `seed`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{
    buffer_saying, cross_merge, cross_metrics, cross_solve, cross_tab, tab_with_a_files_column,
    tab_with_a_preview,
};

/// **T5/§7.1.4 — a tab is seeded by the pane it is reopened as, and all three
/// shapes now exist.**
///
/// The vault's own sentence read against the three tab shapes: `TabState::seed`
/// answers off the *identity* seat, so a `[files | shell]` tab is still seeded
/// by its shell (that is what identity ordering is for) while a folder tab is
/// seeded by its folder and a file tab by its file.
///
/// The fourth answer is `None`, and it is the one worth writing a test for
/// because nothing else in the product produces it: a tab whose identity pane
/// is a leaf this build cannot read has no profile, no place and no path, so
/// `close_tab` writes no vault row rather than one that reopens as a guess.
///
/// Red gate: key `seed` on "does this tab hold a shell" instead of on the
/// identity pane's kind, and the first assertion turns into `Seed::Files` —
/// a split tab that would reopen as a bare column with its shell forgotten.
#[test]
fn a_tab_is_seeded_by_the_pane_it_would_be_reopened_as() {
    let split = tab_with_a_files_column(1, "D:\\work\\folio");
    assert!(
        matches!(split.seed(), Some(seed::Seed::Term { .. })),
        "a tab holding a shell beside a column is still reopened as the shell"
    );

    let mut source = tab_with_a_files_column(2, "D:\\work\\folio");
    let column = source.seats.files()[0];
    let folder = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a files column may become a tab of its own");
    assert_eq!(
        folder.seed(),
        Some(seed::Seed::Files {
            root: "D:\\work\\folio".to_owned()
        }),
        "and a folder tab as the place it was standing"
    );

    let (mut source, pane) = tab_with_a_preview(
        3,
        vec![buffer_saying("D:\\work\\notes.md", "notes.md", "hello")],
    );
    let file = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        pane,
        TabId(10),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a preview may become a tab of its own");
    assert_eq!(
        file.seed(),
        Some(seed::Seed::Preview {
            path: "D:\\work\\notes.md".to_owned(),
            source: bt_persist::PreviewSourceV1::File,
        }),
        "and a file tab as the file it was on — the vault's third shape, \
             without which Ctrl+Shift+T would be a door onto an empty store"
    );
}

/// **T5 — a tab with no shell may not be renamed, and the guard that turns it
/// away was written for this day.**
///
/// `Seed::can_be_named` has answered `false` for a files place since it was
/// written, with its own note recording that "today this never answers
/// `false` for a real tab" and that the case would exist once T5 landed. It
/// has now. A preview-root tab is turned away on the same footing: its
/// identity is a path on disk, and the manual name is a slot on the terminal
/// seed and on nothing else.
#[test]
fn a_sessionless_tab_has_no_name_slot_for_the_editor_to_write_to() {
    assert!(
        seed::Seed::Term {
            profile_id: "pwsh.exe".to_string(),
            cwd: "D:\\".to_string(),
            manual_name: None,
        }
        .can_be_named()
    );
    assert!(
        !seed::Seed::Files {
            root: "D:\\work".to_string()
        }
        .can_be_named(),
        "a place is identified by its root, which is a fact about the disk"
    );
    assert!(
        !seed::Seed::Preview {
            path: "D:\\work\\notes.md".to_string(),
            source: bt_persist::PreviewSourceV1::File,
        }
        .can_be_named(),
        "and so is a file"
    );
}

/// PIN — **F57 survives N160①: a pin that arrives by merge still leads the
/// strip.**
///
/// The user's report, as a list. Four tabs, the first one pinned; a pinned
/// tab is dragged into the *last* one's layout, so "pin follows content"
/// (N160①) pins a tab that does not move. What the strip then read was
/// `pinned, unpinned, unpinned, pinned` — the pins at slots 1 and 4 with
/// unpinned tabs between them, which is the screenshot exactly.
///
/// The two clamps cannot catch this and it is worth saying why: both
/// [`strip_insert_slot`] and [`partition_clamped`] rule on where a tab may
/// *land*, and here nothing landed. Only a flag changed, on a tab standing
/// still, so the repair has to be the one thing neither clamp does — put the
/// run back in its partition.
///
/// Order is asserted by identity and not merely by flag: a normalization that
/// got the partition right by shuffling the two unpinned tabs would satisfy
/// `pins_are_normalized` and still be wrong, because `normalize_pins` is
/// documented stable and the tabs nobody touched must not move.
///
/// Red gate: drop the `settle_pin_partition` call at the foot of
/// [`absorb_tab_into_strip`] and the merged tab stays at slot 3 behind two
/// unpinned tabs.
#[test]
fn a_pin_arriving_by_merge_still_leads_the_strip() {
    // A partitioned strip: two pinned tabs lead, two plain ones follow. The
    // second pinned tab is the one about to be dragged away.
    let mut tabs = vec![
        cross_tab(2, &["ALREADY"]),
        cross_tab(1, &["SRCA"]),
        cross_tab(3, &["PLAIN"]),
        cross_tab(5, &["TARGET"]),
    ];
    tabs[0].pinned = true;
    tabs[1].pinned = true;
    assert!(
        seed::pins_are_normalized(&tabs, |tab| tab.pinned),
        "the strip starts partitioned, so nothing below is an inherited mess"
    );

    // The target is the tab on screen — the merge's own precondition (K129)
    // — and it is the last one in the run, which is what leaves the arriving
    // pin behind an unpinned tab.
    let (source_index, mut active_tab) = (1, 3);
    let source_seats = tabs[source_index].seats.clone();
    let arrived = cross_merge(
        &source_seats,
        &mut tabs[active_tab],
        seats::LayoutAim::SeatEdge(SeatId(1), seats::DropEdge::Right),
    );
    let (from, into) = two_tabs_mut(&mut tabs, source_index, active_tab);
    let ejected = absorb_tab_into_layout(from, into, &arrived, None, TabId(9), cross_solve);
    assert!(ejected.is_none(), "an edge landing displaces nothing");
    assert!(
        tabs[active_tab].pinned,
        "N160(1) still holds: the pin followed the content"
    );

    absorb_tab_into_strip(&mut tabs, &mut active_tab, source_index, ejected);

    assert!(
        seed::pins_are_normalized(&tabs, |tab| tab.pinned),
        "F57: the pinned run leads the strip"
    );
    assert_eq!(
        tabs.iter().map(|tab| tab.id).collect::<Vec<_>>(),
        vec![TabId(2), TabId(5), TabId(3)],
        "the newly pinned tab joins the pinned run, and the tab nobody \
             touched keeps its place behind it"
    );
    assert_eq!(
        tabs[active_tab].id,
        TabId(5),
        "the active tab is followed by identity across the reorder, not by index"
    );
}
