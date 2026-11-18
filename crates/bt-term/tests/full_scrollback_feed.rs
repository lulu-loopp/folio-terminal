//! **A full scrollback is fed at the same cost however long it is** (0.4.10 plan row 105,
//! T-FULL-SCROLLBACK-FEEDS-SLOW-AND-GROWS).
//!
//! Once a pane's history is full, every line it freezes evicts the oldest one, and the eviction
//! deletes that line's staging relocations. The clean-VM run of 2026-11-18 found deletion
//! walking every relocation in the document to find them — 500,000 to 800,000 per evicted line at
//! the default capacity — and a flood falling from 10,000 lines a second to 143. The work is
//! counted here, not timed: the document's relocation visits per evicted line, at two capacities
//! ten times apart.

#![allow(clippy::disallowed_methods)]

use std::num::{NonZeroU32, NonZeroUsize};

use bt_term::DualPlaneSession;

/// PowerShell's `'x' * 200`: at 40 columns, a logical line of exactly five rows.
const LINE: &str = concat!(
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "\r\n"
);
const COLUMNS: u32 = 40;
const ROWS: u32 = 24;
const ROWS_PER_LINE: u64 = 5;
/// What one drain reads: `bt_pty::TERM_READ_SLICE`.
const SLICE: usize = 8 * 1024;

fn print(session: &mut DualPlaneSession, lines: usize) {
    let flood = LINE.repeat(lines);
    for slice in flood.as_bytes().chunks(SLICE) {
        session.feed(slice).expect("the flood is plain text");
    }
}

fn oldest(session: &DualPlaneSession) -> u64 {
    session
        .transcript()
        .frozen()
        .front()
        .expect("history is not empty")
        .id
        .0
}

#[test]
fn the_feed_costs_the_same_before_and_after_the_cap() {
    assert_eq!(LINE.len(), 202, "two hundred characters and the line's end");
    let mut per_line = Vec::new();
    for capacity in [1_000, 10_000] {
        let mut session = DualPlaneSession::with_frozen_quota(
            NonZeroU32::new(COLUMNS).expect("a positive width"),
            NonZeroU32::new(ROWS).expect("a positive height"),
            NonZeroUsize::new(capacity).expect("a positive capacity"),
        );
        print(&mut session, capacity + 1_000);
        assert_eq!(
            session.transcript().frozen().len(),
            capacity,
            "the history is full before the count begins"
        );

        let visits = session.document().relocation_visits();
        let first = oldest(&session);
        print(&mut session, 2_000);
        let evicted = oldest(&session) - first;
        let visited = session.document().relocation_visits() - visits;
        assert_eq!(
            evicted, 2_000,
            "every line printed past the cap evicted one"
        );
        per_line.push((capacity, visited / evicted, visited % evicted));
    }
    assert_eq!(
        per_line,
        vec![(1_000, ROWS_PER_LINE, 0), (10_000, ROWS_PER_LINE, 0)],
        "relocations visited per evicted line, at each capacity (capacity, quotient, remainder)"
    );
}
