//! **The crate root: attention and notifications.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    BOTH_NOTIFICATION_ROWS_ON, on_a_screen, one_turn, request_attention, ringing_tab,
};
use std::time::Duration;

/// PIN (§7.1.5b P1-8) — **`Ctrl+Shift+A` serves the oldest, then walks on,
/// then wraps.**
///
/// The ruling's three clauses against the one function that implements them,
/// with the queue handed over out of order on purpose: the walk is a function
/// of the serials, not of the order anything was collected in.
///
/// Red gate: order by the tuple's first element and the first assertion goes
/// red; drop the pivot and the second does; fall back to `.next()` instead of
/// rotating and the last does.
#[test]
fn the_attention_queue_serves_the_oldest_then_walks_on_and_wraps() {
    let queue = [("c", 9_u64), ("a", 2), ("b", 5)];
    assert_eq!(next_attention_stop::<&str>(&[], None), None);
    assert_eq!(
        next_attention_stop(&queue, None),
        Some("a"),
        "先到先服务 — the oldest serial, whatever order the window walked its \
             tabs in"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(2)),
        Some("b"),
        "已在队列中再按 = 走到下一个"
    );
    assert_eq!(next_attention_stop(&queue, Some(5)), Some("c"));
    assert_eq!(
        next_attention_stop(&queue, Some(9)),
        Some("a"),
        "循环 — past the newest is back to the oldest"
    );
    assert_eq!(
        next_attention_stop(&[("only", 4_u64)], Some(4)),
        Some("only"),
        "a queue of one wraps onto itself rather than answering nothing"
    );
    assert_eq!(
        next_attention_stop(&queue, Some(7)),
        Some("c"),
        "and a pivot that is nobody's serial still names a position in the \
             order — the walk is over serials, not over membership"
    );
}

/// PIN (census-3, `docs/plans/design/ownership-census-2026-09-25.md` §R6) — **the window's
/// attention pass arms a wait's ten minutes when it arrives, forgets them when its clear arrives,
/// and spends them when they run out.**
///
/// The rule and the clock left this crate for `bt-workbench` (`attention::expiry`), where their own
/// pure tests live. What stayed here is *where the clock is driven*: `deliver_attention` arms and
/// forgets it as a hook's line lands, `settle_attention` spends what has come due on the next
/// pass. Neither half can be seen from the other crate, so this walks both through the product's
/// own functions — a real message naming a real pane's capability, looked up in the shipped
/// Claude Code rows — and reads the answer the loop reads, `attention_ledger_deadline`.
///
/// MUTATION: drop the `arm` arm in `deliver_attention` and the first assertion goes red; drop the
/// `forget` arm and the second does; take the `due` loop out of `settle_attention` and the last
/// pair does.
#[test]
fn a_waits_ten_minutes_are_armed_on_arrival_forgotten_on_its_clear_and_spent_when_they_run_out() {
    use attention::expiry::WAIT_TTL;

    let mut tabs = vec![ringing_tab(1, 1)];
    let seat = tabs[0].seats.terminals()[0];
    let capability = attention_wire::mint_capability();
    tabs[0]
        .sessions
        .get_mut(&seat)
        .expect("the fixture's seat holds a shell")
        .attention_capability
        .clone_from(&capability);
    let installed =
        attention_map::installed_rows(attention_map::ROWS, attention_map::CLAUDE_CODE, |_| true);
    let said = |event: &str| attention_wire::Message {
        capability: capability.clone(),
        family: attention_map::CLAUDE_CODE.to_owned(),
        event: event.to_owned(),
        id: None,
        text: None,
    };
    let mut places = attention::Places::default();
    let start = Instant::now();
    let deliver = |tabs: &mut [TabState], places: &mut attention::Places, event: &str| {
        deliver_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            &[said(event)],
            &installed,
            start,
            None,
            &mut Vec::new(),
        );
    };
    let settle = |tabs: &mut [TabState], places: &mut attention::Places, now: Instant| {
        settle_attention(
            tabs,
            1,
            on_a_screen(false),
            BOTH_NOTIFICATION_ROWS_ON,
            places,
            now,
            None,
            &mut Vec::new(),
        );
    };

    deliver(&mut tabs, &mut places, "PermissionRequest");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        Some(start + WAIT_TTL),
        "a wait that arrives starts its ten minutes"
    );
    deliver(&mut tabs, &mut places, "Stop");
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "the clear that ends it takes its clock with it"
    );

    deliver(&mut tabs, &mut places, "PermissionRequest");
    settle(
        &mut tabs,
        &mut places,
        start + WAIT_TTL - Duration::from_secs(1),
    );
    assert_ne!(
        tabs[0].sessions[&seat].attention.state(),
        attention::State::Idle,
        "a second before the ten minutes are up, the pane is still asking"
    );
    settle(&mut tabs, &mut places, start + WAIT_TTL);
    assert_eq!(
        attention_ledger_deadline(&tabs),
        None,
        "a clock that ran out is spent, not repeated"
    );
    assert_eq!(
        tabs[0].sessions[&seat].attention.state(),
        attention::State::Idle,
        "and the wait it stood for is over"
    );
}

/// The window the ending-step pins end: two tabs of one pane each, the second pane still in birth,
/// and both panes holding a standing request a settle pass has queued.
fn a_window_of_two_asking_panes() -> (Vec<TabState>, attention::Places) {
    let mut tabs = vec![ringing_tab(1, 1), ringing_tab(2, 1)];
    let in_birth = tabs[1].seats.terminals()[0];
    tabs[1]
        .sessions
        .get_mut(&in_birth)
        .expect("the fixture's seat holds a pane")
        .birth = Some(PaneBirth {
        seed: LeafSeed::default(),
        probe_input: None,
        typed: std::cell::RefCell::default(),
        waiting: BirthWait::Locale,
    });
    for tab in &mut tabs {
        let seat = tab.seats.terminals()[0];
        request_attention(tab, seat, "yes");
    }
    let mut places = attention::Places::default();
    one_turn(&mut tabs, 0, false, &mut places);
    for tab in &tabs {
        for (seat, leaf) in tab.leaves() {
            assert_ne!(
                leaf.attention.state(),
                attention::State::Idle,
                "the fixture's own precondition: {seat:?} holds a request"
            );
        }
    }
    (tabs, places)
}

/// [`end_leaves_of`] over every tab given, as a window-ending road asks it: the places it gave
/// up, counted per pane — one count per `LeafGone` the step *applied*, which a trace line cannot
/// tell, because the ledger answers a second `LeafGone` with nothing.
fn end_and_count<'t>(
    tabs: impl IntoIterator<Item = (usize, &'t mut TabState)>,
    moving: Option<&DragSource>,
    places: &mut attention::Places,
    counts: &mut BTreeMap<(usize, SeatId), usize>,
) {
    let place = on_a_screen(false);
    for site in end_leaves_of(
        tabs,
        None,
        moving,
        |index| notify::desktop_reach(index == 0, place),
        places,
        Instant::now(),
        None,
    ) {
        *counts.entry((site.tab, site.seat)).or_default() += 1;
    }
}

/// PIN (ownership contract 2026-10-11, (d)2 finding F-1) — **every road that ends a pane emits
/// its `LeafGone` once**: the ending step the five roads share (`close_pane`, `close_tab`,
/// `let_go_of_this_window` for a window close and for the quit, `abandon_a_window_nothing_arrived_in`,
/// and `answer_restore`'s placeholder removal), counted in applications.
///
/// (i) Every leaf of a window, the pane in birth included, is given up once. (ii) The same window
/// walked a second time — a window closed and then retired by a quit before it is reaped — gives
/// nothing more up. (iii) A tab a pending handover is carrying out keeps its places, and gives
/// them up once when it later ends where it landed. (iv) The launch placeholder, which holds a real
/// shell, gives its place up once and has its shell retired after.
///
/// The roads' own calls into this step are not driven here: a `Runtime` is a window, and no test
/// builds one (`docs/plans/structural-debt.md`, the ending-step row).
///
/// MUTATIONS: drop the `attention_ended` mark and (ii) goes red at 2; ignore `moving` and (iii)
/// goes red on the tab in flight; pass over a pane in birth and (i) goes red on it.
#[test]
fn every_road_that_ends_a_pane_emits_leaf_gone_once() {
    use bt_pty::test_shell::TestShell;

    // (i) and (ii): one window, ended, and ended again.
    let (mut tabs, mut places) = a_window_of_two_asking_panes();
    let panes: Vec<(usize, SeatId)> = tabs
        .iter()
        .enumerate()
        .flat_map(|(index, tab)| tab.leaves().map(move |(seat, _)| (index, *seat)))
        .collect();
    let mut counts = BTreeMap::new();
    end_and_count(tabs.iter_mut().enumerate(), None, &mut places, &mut counts);
    for pane in &panes {
        assert_eq!(counts.get(pane), Some(&1), "(i) {pane:?} is ended once");
    }
    end_and_count(tabs.iter_mut().enumerate(), None, &mut places, &mut counts);
    for pane in &panes {
        assert_eq!(
            counts.get(pane),
            Some(&1),
            "(ii) a second walk of the same window ends {pane:?} again"
        );
    }

    // (iii): a handover carrying tab 2 out of a window that closes in the same turn.
    let (mut tabs, mut places) = a_window_of_two_asking_panes();
    let moving = DragSource::Tab(tabs[1].id);
    let mut counts = BTreeMap::new();
    end_and_count(
        tabs.iter_mut().enumerate(),
        Some(&moving),
        &mut places,
        &mut counts,
    );
    let staying = (0, tabs[0].seats.terminals()[0]);
    let moved = tabs[1].seats.terminals()[0];
    assert_eq!(counts.get(&staying), Some(&1), "(iii) the staying tab ends");
    assert_eq!(
        counts.get(&(1, moved)),
        None,
        "(iii) the tab in flight is not ending"
    );
    assert_ne!(
        tabs[1].sessions[&moved].attention.state(),
        attention::State::Idle,
        "(iii) and still holds its request"
    );
    // It lands as the only tab of another window, and is closed there.
    let mut landed = tabs.remove(1);
    let mut there = attention::Places::default();
    let mut counts = BTreeMap::new();
    end_and_count([(0, &mut landed)], None, &mut there, &mut counts);
    assert_eq!(
        counts.get(&(0, moved)),
        Some(&1),
        "(iii) once, where it ended"
    );

    // (iv): the launch placeholder, holding a real shell, taken away once the restored tabs stand.
    let size = PtySize::cells(
        std::num::NonZeroU16::new(48).expect("columns"),
        std::num::NonZeroU16::new(8).expect("rows"),
    );
    let (shell, hygiene) = TestShell::spawn_default(size)
        .expect("a real shell")
        .into_session();
    let mut placeholder = ringing_tab(1, 1);
    let seat = placeholder.seats.terminals()[0];
    placeholder
        .sessions
        .get_mut(&seat)
        .expect("the placeholder's pane")
        .pty = Some(shell);
    request_attention(&mut placeholder, seat, "yes");
    let mut tabs = vec![placeholder];
    let mut places = attention::Places::default();
    one_turn(&mut tabs, 0, false, &mut places);
    let mut counts = BTreeMap::new();
    end_and_count([(0, &mut tabs[0])], None, &mut places, &mut counts);
    assert_eq!(
        counts.get(&(0, seat)),
        Some(&1),
        "the placeholder's pane ends once"
    );
    assert!(
        tabs[0].sessions[&seat].pty.is_none(),
        "and its shell is retired"
    );
    drop(tabs);
    drop(hygiene);
}
