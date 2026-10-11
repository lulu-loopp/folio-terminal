//! **The crate root: attention and notifications.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{
    BOTH_NOTIFICATION_ROWS_ON, on_a_screen, one_turn, request_attention, ringing_tab, squeezed_body,
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

/// PIN (ownership contract 2026-10-11, (d)2 finding F-1) — **every road that ends a pane emits
/// its `LeafGone` once.**
///
/// A window of two tabs, one pane each, the second pane still in birth, and both panes holding a
/// standing request — so a `LeafGone` reaching either is a `reason=leaf-gone` line in a recording
/// trace and one that does not is no line at all. Each row ends the whole window by one road, as
/// the window composes it: `close_pane` names one leaf at a time; `close_tab` ends the tab it
/// takes and leaves the last one to the window close it asks for; the window close and the quit's
/// retirement end every tab still there; and the abandoned window's tabs are ended as they are
/// taken, so the close that follows finds none.
///
/// A `Runtime` is a window and a test has none, so the rows drive the walks the roads ask, and the
/// second half reads each road's body for its walk: asked exactly once, and before its shells. The
/// ledger answers a second `LeafGone` with nothing (`Idle` writes no line), which is why "once" is
/// that count and not a line count.
///
/// MUTATIONS: take the walk out of `let_go_of_this_window` and its body row goes red ("gives its
/// panes' places up once": none); call it twice there and the same row goes red on "once"; make
/// the per-tab walk pass over a pane in birth and the first row that walks one goes red on it.
#[test]
fn every_road_that_ends_a_pane_emits_leaf_gone_once() {
    let path = std::env::temp_dir().join(format!(
        "{}.log",
        bt_testpath::unique_name("bt-attention-leaf-gone-roads")
    ));
    let _ = std::fs::remove_file(&path);
    let trace = crate::trace::Trace::create(&path, "# pin");
    let place = on_a_screen(false);
    let now = Instant::now();
    let window = || {
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
        assert!(tabs[1].leaves().all(|(_, leaf)| leaf.birth.is_some()));
        (tabs, places)
    };
    let panes: Vec<String> = {
        let (tabs, _) = window();
        tabs.iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                tab.leaves()
                    .map(move |(seat, _)| format!("tab={index} seat={seat:?} "))
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    type Road = fn(
        &mut Vec<TabState>,
        &mut attention::Places,
        notify::WindowPlace,
        Instant,
        &attention_trace::Trace,
    );
    let roads: [(&str, Road); 4] = [
        ("close_pane", |tabs, places, place, now, trace| {
            for (index, tab) in tabs.iter_mut().enumerate() {
                let reach = notify::desktop_reach(index == 0, place);
                for (seat, leaf) in tab.leaves_mut() {
                    expire_leaf_attention(
                        attention::Site {
                            tab: index,
                            seat: *seat,
                        },
                        reach,
                        leaf,
                        places,
                        now,
                        Some(trace),
                    );
                }
            }
        }),
        (
            "close_tab, the last one closing the window",
            |tabs, places, place, now, trace| {
                let mut removed = tabs.remove(1);
                let reach = notify::desktop_reach(false, place);
                expire_every_leaf_of(&mut removed, 1, reach, places, now, Some(trace));
                expire_every_leaf_of_a_window(tabs, 0, place, places, now, Some(trace));
            },
        ),
        (
            "let_go_of_this_window",
            |tabs, places, place, now, trace| {
                expire_every_leaf_of_a_window(tabs, 0, place, places, now, Some(trace));
            },
        ),
        (
            "abandon_a_window_nothing_arrived_in, then its close",
            |tabs, places, place, now, trace| {
                let mut taken = std::mem::take(tabs);
                expire_every_leaf_of_a_window(&mut taken, 0, place, places, now, Some(trace));
                expire_every_leaf_of_a_window(tabs, 0, place, places, now, Some(trace));
            },
        ),
    ];
    let mut written = 0;
    for (road, end) in roads {
        let (mut tabs, mut places) = window();
        end(&mut tabs, &mut places, place, now, &trace);
        let lines: Vec<String> = std::fs::read_to_string(&path)
            .expect("the trace file was created")
            .lines()
            .skip(1 + written)
            .map(str::to_owned)
            .collect();
        written += lines.len();
        for pane in &panes {
            assert_eq!(
                lines
                    .iter()
                    .filter(
                        |line| line.contains(pane.as_str()) && line.ends_with("reason=leaf-gone")
                    )
                    .count(),
                1,
                "{road}: {pane}is ended once:
{lines:#?}"
            );
        }
    }

    // Each road's own body: its walk asked once, and before the shells are retired.
    for (name, walk, retire) in [
        (
            "close_pane",
            "expire_leaf_attention(",
            "bt_pty::retire_session(",
        ),
        ("close_tab", "expire_every_leaf_of(", "retire_all_shells()"),
        (
            "let_go_of_this_window",
            "expire_every_leaf_of_a_window(",
            "retire_all_shells()",
        ),
        (
            "abandon_a_window_nothing_arrived_in",
            "expire_every_leaf_of_a_window(",
            "retire_all_shells()",
        ),
    ] {
        let body = squeezed_body("Runtime", name);
        assert_eq!(
            body.matches(walk).count(),
            1,
            "`{name}` gives its panes' places up once"
        );
        assert!(
            body.find(walk) < body.find(retire),
            "`{name}` gives its panes' places up before it retires their shells"
        );
    }
    let close_tab = squeezed_body("Runtime", "close_tab");
    assert!(
        close_tab.find("TabCloseAction::Keep") < close_tab.find("expire_every_leaf_of("),
        "a tab closed as the last one leaves its panes to the window close it asks for"
    );
}
