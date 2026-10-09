//! **`menubar`, as the application drives it.** Tests whose first assertion is about
//! `menubar`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{RevivedShape, launch_plan_on_disk};

// ── B-RESTORE-PINNED: an unclean exit restores what a clean one does ──

/// A tab in the owner's shape — a files column beside a shell, both standing in
/// `cwd` — pinned or not.
fn saved_split_tab(cwd: &Path, pinned: bool) -> TabV1 {
    let cwd = cwd.to_string_lossy().into_owned();
    TabV1 {
        root: LayoutNodeV1::Split(bt_persist::SplitNodeV1 {
            dir: bt_persist::SplitDirV1::Row,
            ratio: 300_000,
            children: [
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Files(
                    bt_persist::FilesLeafV1 {
                        view: bt_persist::FilesViewV1::Files,
                        root: cwd.clone(),
                        open: Vec::new(),
                        sel: None,
                        width: 240,
                        remotes_open: false,
                    },
                ))),
                Box::new(LayoutNodeV1::Leaf(LeafNodeV1::Term(TermLeafV1 {
                    profile_id: "pwsh".to_owned(),
                    cwd,
                    manual_name: None,
                    card_skip: 0,
                    last_command: String::new(),
                }))),
            ],
        }),
        pinned,
        focused_leaf: "leaf-1".to_owned(),
        preview: None,
    }
}

/// **The launch's whole restore road, from the bytes on disk** — the session
/// written by the real writer into a scratch home, the run's sentinel left
/// standing when `crashed` (the process was killed before its clean-exit path),
/// then the real probe, the real reader, `plan_windows`, `plan_launch` and
/// `revive_plan`. Answers what the probe said, the first window's plan and the
/// shape each opened tab is revived as.
fn launch_from_disk(
    home: &Path,
    tabs: Vec<TabV1>,
    active_tab: u32,
    crashed: bool,
) -> (bt_persist::ExitState, LaunchPlan, Vec<RevivedShape>) {
    let _ = std::fs::remove_dir_all(home);
    std::fs::create_dir_all(home).expect("a scratch home");
    let session_path = home.join("session.json");
    let sentinel_path = home.join("session.lock");
    let document = bt_persist::SessionV1 {
        windows: vec![bt_persist::SessionWindowV1 {
            tabs,
            active_tab,
            ..bt_persist::SessionWindowV1::default()
        }],
        ..bt_persist::SessionV1::default()
    };
    bt_persist::write_session_atomic(&session_path, &document).expect("the session is written");
    if crashed {
        bt_persist::create_sentinel(&sentinel_path).expect("the run's sentinel");
    }
    let exit = bt_persist::probe_sentinel(&sentinel_path).expect("the sentinel is asked about");
    let (plan, shapes) = launch_plan_on_disk(&session_path);
    let _ = std::fs::remove_dir_all(home);
    (exit, plan, shapes)
}

fn restore_home(name: &str) -> PathBuf {
    bt_testpath::temp_path(&format!("bt-restore-pinned-{name}"))
}

/// PIN (B-RESTORE-PINNED) — **after an unclean exit, a pinned tab comes back
/// with its whole saved tree: every leaf, each shell in its own folder.**
///
/// The owner's report (2026-09-27, 0.4.4, after a reboot) was pinned tabs back
/// as one shell each. The launch reads the sentinel only to log it
/// (`SessionStore::open`); nothing on the restore road asks how the last run
/// ended, and a pinned tab is revived by the same `revive_plan` as a Restore, a
/// Recent row and Ctrl+Shift+T. This pins that: two pinned tabs whose roots are
/// `[files | shell]` splits, the sentinel standing, both open with both leaves.
/// Green on BASE as well — the ticket's report says why the loss is not on this
/// road.
///
/// MUTATION: revive a pinned tab from its identity leaf alone (e.g.
/// `Seats::lone_terminal()` for `tab.pinned` in `revive_plan`), and this goes
/// red.
#[test]
fn an_unclean_exit_restores_pinned_tabs_with_their_trees() {
    let home = restore_home("unclean-trees");
    let (exit, plan, shapes) = launch_from_disk(
        &home,
        vec![saved_split_tab(&home, true), saved_split_tab(&home, true)],
        1,
        true,
    );
    assert_eq!(
        exit,
        bt_persist::ExitState::Crashed,
        "the sentinel says the last run did not reach its clean exit"
    );
    assert_eq!(plan.open.len(), 2, "both pinned tabs open");
    assert_eq!(plan.active_open, Some(1), "on the tab that was in front");
    let whole = (
        vec![bt_layout::SeatKind::Files, bt_layout::SeatKind::Terminal],
        vec![Some(home.clone())],
    );
    assert_eq!(
        shapes,
        vec![whole.clone(), whole],
        "each comes back as its files column and its shell, in its folder"
    );
}

/// PIN (B-RESTORE-PINNED) — **whether the restore card is raised is decided by
/// the pins alone, never by how the last run ended; and a window whose tabs are
/// all pinned opens every tree whole.**
///
/// The owner's shape: three pinned `[files | shell]` tabs and one unpinned.
/// After a crash exactly as after a clean exit, the three open and the fourth
/// is the card's question; with all four pinned there is no question on either
/// road and all four trees open. There is no "all pinned → rebuild from the
/// pins" shortcut: `pins.json` is the table of pinned folders and files, and
/// the launch never reads it for tabs.
///
/// MUTATION: make `plan_launch` leave `ask` empty whenever a tab is pinned, or
/// revive pinned tabs from a seed rather than their tree, and this goes red.
#[test]
fn all_pinned_does_not_skip_the_restore_card_or_the_trees() {
    let home = restore_home("all-pinned");
    let owner = |last_pinned: bool| {
        vec![
            saved_split_tab(&home, true),
            saved_split_tab(&home, true),
            saved_split_tab(&home, true),
            saved_split_tab(&home, last_pinned),
        ]
    };
    let whole = (
        vec![bt_layout::SeatKind::Files, bt_layout::SeatKind::Terminal],
        vec![Some(home.clone())],
    );
    for crashed in [true, false] {
        let (_, plan, shapes) = launch_from_disk(&home, owner(false), 0, crashed);
        assert_eq!(
            plan.ask,
            vec![saved_split_tab(&home, false)],
            "the unpinned tab is the card's question (crashed = {crashed})"
        );
        assert_eq!(shapes, vec![whole.clone(); 3], "crashed = {crashed}");

        let (_, plan, shapes) = launch_from_disk(&home, owner(true), 0, crashed);
        assert!(
            plan.ask.is_empty(),
            "nothing unpinned, nothing to ask (crashed = {crashed})"
        );
        assert!(!plan.placeholder, "crashed = {crashed}");
        assert_eq!(shapes, vec![whole.clone(); 4], "crashed = {crashed}");
    }
}

/// PIN (B-RESTORE-PINNED) — **a clean exit's restore is the unclean exit's
/// restore**: the same document, with and without the sentinel, gives the same
/// plan and the same revived trees.
///
/// MUTATION: let the restore road read the sentinel (e.g. `read_session`
/// answering the default document while `session.lock` stands beside the file,
/// or `plan_launch` opening nothing but the pinned tabs' identity leaves after a
/// crash), and this goes red.
#[test]
fn a_clean_exit_restore_is_unchanged() {
    let home = restore_home("clean");
    let tabs = || {
        vec![
            saved_split_tab(&home, true),
            saved_split_tab(&home, false),
            saved_split_tab(&home, false),
        ]
    };
    let clean = launch_from_disk(&home, tabs(), 0, false);
    let unclean = launch_from_disk(&home, tabs(), 0, true);
    assert_eq!(clean.0, bt_persist::ExitState::Normal);
    assert_eq!(unclean.0, bt_persist::ExitState::Crashed);
    assert_eq!(clean.1, unclean.1, "one plan either way");
    assert_eq!(clean.2, unclean.2, "one set of trees either way");
    assert_eq!(clean.1.open, vec![saved_split_tab(&home, true)]);
    assert_eq!(
        clean.1.ask.len(),
        2,
        "the two unpinned tabs are asked about"
    );
}
