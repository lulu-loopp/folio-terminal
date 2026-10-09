//! **`search`, as the application drives it.** Tests whose first assertion is about
//! `search`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{leaf_saying, method_body};

// ── ticket 51: the find box reads a bounded slice of history per keystroke ──

/// A shell-less pane whose history holds `lines` generated lines, each narrow enough never to
/// wrap on the fixture's forty columns; `name(index)` writes line `index`. Generated, not
/// recorded (standing rules: fixtures).
fn pane_with_history(lines: usize, name: impl Fn(usize) -> String) -> LeafSession {
    let mut text = String::with_capacity(lines * 24);
    for index in 0..lines {
        text.push_str(&name(index));
        text.push_str("\r\n");
    }
    leaf_saying(&text)
}

/// The history hits of a whole-plane scan, as the set of `(line, start, end)` a hit is known by.
fn whole_answer(
    compiled: &bt_transcript::search::CompiledSearch,
    leaf: &LeafSession,
) -> std::collections::BTreeSet<(bt_viewport::SearchLine, u32, u32)> {
    search::scan_history(compiled, leaf.session.transcript())
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect()
}

/// RED (51) — **A keystroke in the find box runs the pattern over at most one slice of history,
/// however long the history is.**
///
/// The pane holds a hundred thousand generated lines, the default scrollback. Before ticket 51
/// a changed question re-ran the pattern over every one of them on the keystroke's frame, once
/// per character typed (`ARCHITECTURE` §5.3 row 6). Now the keystroke's road
/// ([`SearchRefresh::Asked`]) reads one slice, each turn's ([`SearchRefresh::Walk`]) one more,
/// and a published frame ([`SearchRefresh::Output`]) none — and the walk those slices make ends
/// on the answer a scan from scratch gives. The real leaf, the real transcript, the real
/// [`rescan_leaf_for_search`] that `Runtime::refresh_search` calls.
///
/// MUTATION: make the keystroke road call the unbounded scan (`SearchRefresh::walk_budget`
/// answering `usize::MAX` for `Asked`) — `lines_scanned` reads the whole plane and the first
/// assertion goes red.
#[test]
fn a_keystroke_in_the_find_box_reads_at_most_one_slice_of_history() {
    let leaf = pane_with_history(100_000, |index| format!("worker {index} ok"));
    let frozen = leaf.session.transcript().frozen().len();
    assert!(
        frozen > 4 * search::SEARCH_HISTORY_SLICE,
        "the history is long enough to need a walk: {frozen} lines"
    );
    let compiled = search::engine(search::SearchFlags::default(), "worker 9").unwrap();
    let seat = SeatId(1);

    let asked =
        rescan_leaf_for_search(&compiled, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    assert!(
        asked.lines_scanned <= search::SEARCH_HISTORY_SLICE,
        "the keystroke read {} of {frozen} lines",
        asked.lines_scanned
    );
    assert!(!asked.cache.history.is_complete(), "the rest is owed");
    assert!(!asked.unchanged);

    // A frame published after the keystroke carries the answer and reads no slice of it.
    let published = rescan_leaf_for_search(
        &compiled,
        &leaf,
        seat,
        1,
        Some(&asked.cache),
        SearchRefresh::Output,
        false,
    );
    assert_eq!(published.lines_scanned, 0);
    assert!(
        published.unchanged,
        "nothing moved, so nothing is re-installed"
    );

    // Each turn after it reads one slice, and the walk ends on the whole answer.
    let mut cache = asked.cache;
    let mut turns = 0;
    while cache.walking() {
        let walked = rescan_leaf_for_search(
            &compiled,
            &leaf,
            seat,
            1,
            Some(&cache),
            SearchRefresh::Walk,
            false,
        );
        assert!(walked.lines_scanned <= search::SEARCH_HISTORY_SLICE);
        assert!(!walked.unchanged, "a slice is never mistaken for nothing");
        assert!(!walked.carried_complete);
        cache = walked.cache;
        turns += 1;
    }
    assert_eq!(turns, (frozen - 1) / search::SEARCH_HISTORY_SLICE);
    let found: std::collections::BTreeSet<_> = cache
        .history
        .hits()
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect();
    assert_eq!(found, whole_answer(&compiled, &leaf));
    assert_eq!(cache.history.hits().len(), found.len());
}

/// RED (A4) — **A search walk stops when the turn's allowance is spent and resumes on the next
/// turn, with its progress kept and nothing dropped.**
///
/// A slice is one unit of deferrable work (budget note §R-B): a turn that has run past its 16 ms
/// when the walk asks reads nothing, and the cache keeps the walk's cursor, so the next turn
/// reads the slice this one would have. Every other turn here is spent. The walk ends on the
/// answer a scan from scratch gives, in exactly as many slices as it would have taken without
/// the yields. The real leaf, the real `rescan_leaf_for_search`, the real heartbeat on the test
/// clock — and `advance_search_scan` asks through the heartbeat's verb.
///
/// MUTATION: run the unit in `Heartbeat::deferrable` whatever `unit_may_start` answers and no
/// turn yields.
#[test]
fn the_search_walk_stops_when_the_allowance_is_spent_and_resumes_next_turn() {
    let before_the_heart = Instant::now();
    let leaf = pane_with_history(4 * search::SEARCH_HISTORY_SLICE, |index| {
        format!("worker {index} ok")
    });
    let compiled = search::engine(search::SearchFlags::default(), "worker 1").unwrap();
    let seat = SeatId(1);
    let asked =
        rescan_leaf_for_search(&compiled, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    let heart = hang_watch::Heartbeat::on_test_clock();
    let mut cache = asked.cache;
    let (mut walked, mut yielded) = (0, 0);
    let mut turn_ns = 0;
    while cache.walking() {
        hang_watch::set_test_clock_ns(turn_ns);
        heart.woke();
        heart.allow_turn(std::iter::empty());
        let spent = (walked + yielded) % 2 == 0;
        hang_watch::set_test_clock_ns(turn_ns + if spent { 17_000_000 } else { 1_000_000 });
        let kept = cache.clone();
        match heart.deferrable(|| {
            rescan_leaf_for_search(
                &compiled,
                &leaf,
                seat,
                1,
                Some(&cache),
                SearchRefresh::Walk,
                false,
            )
        }) {
            None => {
                assert!(spent, "a turn with time left reads its slice");
                assert_eq!(cache, kept, "the walk's progress is kept");
                assert_eq!(
                    heart.ms_at(heart.deferred_until(before_the_heart)),
                    (turn_ns + 16_000_000) / 1_000_000,
                    "and asked for again at the turn's boundary"
                );
                yielded += 1;
            }
            Some(walk) => {
                assert!(!spent, "a spent turn reads nothing");
                assert!(walk.lines_scanned <= search::SEARCH_HISTORY_SLICE);
                cache = walk.cache;
                walked += 1;
            }
        }
        heart.park(hang_watch::Park::Indefinite);
        turn_ns += 100_000_000;
    }
    let frozen = leaf.session.transcript().frozen().len();
    assert_eq!(walked, (frozen - 1) / search::SEARCH_HISTORY_SLICE);
    assert_eq!(yielded, walked, "every other turn yielded");
    let found: std::collections::BTreeSet<_> = cache
        .history
        .hits()
        .iter()
        .map(|hit| (hit.line, hit.start, hit.end))
        .collect();
    assert_eq!(found, whole_answer(&compiled, &leaf), "nothing dropped");
    assert!(
        method_body("Runtime", "advance_search_scan")
            .contains("hang_watch::deferrable(|| self.refresh_search(SearchRefresh::Walk))"),
        "the turn's slice is asked for through the allowance"
    );
}

/// RED (51) — **A new question abandons the walk of the old one.**
///
/// A walk for `ab` is under way when `c` is typed. The new question has its own revision, and
/// the old answer is no answer to it: no hit of `ab` that is not a hit of `abc` may appear in
/// anything installed after the keystroke, on the keystroke's own frame or on any turn of the new
/// walk — the cursor of the old walk is never read again. Half the lines hold `ab` alone, so a
/// carried hit would be caught on the first install.
///
/// MUTATION: continue the old cursor under the new revision (drop `cache.revision == revision`
/// from the filter in `rescan_leaf_for_search`) — the `ab` hits the old walk found are carried
/// into the first install, and so is its cursor.
#[test]
fn a_new_question_abandons_the_walk_of_the_old_one() {
    let lines = 3 * search::SEARCH_HISTORY_SLICE;
    let leaf = pane_with_history(lines, |index| {
        if index % 2 == 0 {
            format!("ab {index}")
        } else {
            format!("abc {index}")
        }
    });
    let seat = SeatId(1);
    let ab = search::engine(search::SearchFlags::default(), "ab").unwrap();
    let old = rescan_leaf_for_search(&ab, &leaf, seat, 1, None, SearchRefresh::Asked, false);
    assert!(old.cache.walking(), "the old question is mid-walk");

    let abc = search::engine(search::SearchFlags::default(), "abc").unwrap();
    let allowed = whole_answer(&abc, &leaf);
    let mut rescan = rescan_leaf_for_search(
        &abc,
        &leaf,
        seat,
        2,
        Some(&old.cache),
        SearchRefresh::Asked,
        false,
    );
    assert!(rescan.lines_scanned <= search::SEARCH_HISTORY_SLICE);
    loop {
        for hit in rescan.cache.history.hits() {
            assert!(
                allowed.contains(&(hit.line, hit.start, hit.end)),
                "a hit of the old question was installed under the new one: {hit:?}"
            );
        }
        if !rescan.cache.walking() {
            break;
        }
        rescan = rescan_leaf_for_search(
            &abc,
            &leaf,
            seat,
            2,
            Some(&rescan.cache),
            SearchRefresh::Walk,
            false,
        );
    }
    assert_eq!(rescan.cache.history.hits().len(), allowed.len());
}
