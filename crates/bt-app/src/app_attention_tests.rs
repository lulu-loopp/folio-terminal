//! **The crate root: attention and notifications.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{BOTH_NOTIFICATION_ROWS_ON, on_a_screen, ringing_tab};
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
