//! **`files`, as the application drives it.** Tests whose first assertion is about
//! `files`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{cross_metrics, cross_solve, tab_with_a_files_column};

/// PIN (D8(a) of the 2026-09-11 adversarial review) — **the box's advisory
/// asks the folder how it tells names apart, and gets the answer the commit
/// gets.**
///
/// The advisory compared the tree's rows with the typed name by exact bytes
/// while the commit compared with `path.exists()`, which on an ordinary
/// Windows volume folds case. So with `Notes.md` in the folder, `notes.md`
/// drew no red, Enter hit `NewNameRefusal::Taken`, the field stayed open —
/// and sat there looking valid and doing nothing, for ever. Exact matching is
/// *right* on a case-sensitive directory; the defect was that nobody asked
/// the directory which it was.
///
/// Run against a real folder on **this** volume, because that is the only
/// place the question has an answer: a Windows directory can carry the
/// case-sensitivity flag WSL sets, and a pin that hard-coded either answer
/// would be the defect written down as a test.
///
/// RED GATE: compare the rows with `==` and the second assertion goes red
/// wherever `path.exists()` folds case — which is every ordinary volume.
#[test]
fn a_case_different_duplicate_is_refused_in_the_box_on_this_volume() {
    let dir = bt_testpath::temp_path("bt-name-case");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to ask about");
    std::fs::write(dir.join("Notes.md"), b"n").expect("one entry in it");

    let folds = bt_platform::directory_folds_case(&dir);
    let taken = dir.join("notes.md").exists();
    assert_eq!(
        folds, taken,
        "what the folder says about case is what `path.exists()` — the \
             commit's own question — does with a different spelling"
    );
    assert_eq!(
        files::names_are_one("/notes.md", "/Notes.md", folds),
        taken,
        "so the advisory drawn in the box answers the same way the commit will"
    );
    assert!(files::names_are_one("/notes.md", "/notes.md", folds));
    assert!(!files::names_are_one("/notes.md", "/other.md", folds));
    assert!(
        !files::names_are_one("/notes.md", "/Notes.md", false),
        "and a folder that tells case apart is still told apart"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// PIN (F1b, `plan.md` v4 增补 ②; `codex-final.md` §2 "pane 升格换号的活性
/// 仍缺一半") — **a pane promoted into a tab of its own asks its outstanding
/// questions again, under the name it now has.**
///
/// A pane is addressed by [`LeafId`], which is its tab and its seat. Promote
/// it and both change, so everything already out with a worker is addressed
/// to a leaf that no longer exists — and, crucially, **the tab it left is
/// still open**, because it had a sibling. The v3 draft said the old answers
/// would "naturally fail to match once the source tab died"; the source tab
/// does not die, and that sentence is void.
///
/// Safety is had by construction: the old tab no longer has that seat, so an
/// answer addressed to the old leaf is dropped. This test is the other half,
/// **liveness**. Every ledger that says "already asked" travels with the pane
/// — `DirNode::Pending` in its directory cache, a claimed head read on its
/// buffer, a path already out for verification — and a ledger that survives
/// an answer that never comes is a column that stays on "Loading …" for the
/// rest of the session. So the promotion clears them, and the ordinary
/// per-frame asking asks again under the new leaf: `wanted` is exactly what
/// [`files::tree_view`] puts a key on when the cache has never heard of it.
///
/// MUTATION: drop the `forget_work_in_flight_for_seat` call out of
/// `pane_into_new_tab` and the torn-out column keeps its `Pending` node,
/// `wanted` comes back empty, and nothing ever asks again.
#[test]
fn a_pane_promoted_into_its_own_tab_asks_its_outstanding_questions_again() {
    let mut source = tab_with_a_files_column(1, "D:\\work\\folio");
    let column = source.seats.files()[0];
    // One read is out with the worker, addressed to `LeafId { tab: 1, seat:
    // column }`, and the cache says so.
    source
        .file_trees
        .entry(column)
        .or_default()
        .mark_pending("");
    assert!(
        files::tree_view(&source.files[&column].clone(), &source.file_trees[&column])
            .wanted
            .is_empty(),
        "a directory already asked about is not asked about twice — which is \
             the ledger this test is about"
    );

    let torn = tear_pane_into_tab(
        &mut source,
        &cross_metrics(),
        column,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a files column may become a tab of its own");
    let landed = torn.seats.files()[0];

    assert!(
        !source.files.contains_key(&column),
        "safety: the tab it left no longer has that seat, so the answer to \
             the old question lands nowhere"
    );
    assert!(
        !source.sessions.is_empty(),
        "and it is still open, which is why the old address failing to match \
             is not enough on its own"
    );
    assert_eq!(
        files::tree_view(&torn.files[&landed].clone(), &torn.file_trees[&landed]).wanted,
        vec![String::new()],
        "liveness: the promoted column wants its root read again, under the \
             leaf it now is"
    );
}
