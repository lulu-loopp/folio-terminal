//! **A resize settles while its child keeps writing** (0.4.10 plan row 108,
//! T-RESIZE-DURING-A-FLOOD-NEVER-SETTLES).
//!
//! A resize transaction used to close only on 200 ms of output silence after the child was told
//! its size. A child that writes more often than that (an agent's spinner, a progress bar, a
//! flood) kept it open for as long as it wrote: every row it scrolled stayed in the transaction's
//! tail beside the transcript, the tail was staged again at every feed, the history quota did not
//! apply, and no formula was typeset. Once the child has been told, the transaction now also
//! settles [`RESIZE_SETTLE_BOUND`] after the request, or once
//! [`RESIZE_STAGING_BUDGET_CELLS`] have been staged since it. Every clock here is the test's own,
//! so nothing waits.

#![allow(clippy::disallowed_methods)]

use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

use bt_doc::DecorationIntent;
use bt_term::{
    DualPlaneSession, RESIZE_SETTLE_BOUND, RESIZE_STAGING_BUDGET_CELLS, ResizeSettleReason,
    ResizeTraceKind,
};

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("a positive dimension")
}

const NARROW: u32 = 31;
const WIDER: u32 = 40;
const ROWS: u32 = 24;
/// What one drain reads: `bt_pty::TERM_READ_SLICE`.
const SLICE: usize = 8 * 1024;
/// bt-term's `RESIZE_IDLE_AFTER_OUTPUT`.
const QUIET: Duration = Duration::from_millis(200);
/// How far apart a spinner's frames are: ten a second, never a quiet window apart.
const FRAME: Duration = Duration::from_millis(99);

/// Line `number` of the flood: 200 characters, its number first.
fn numbered_line(number: usize) -> String {
    format!("{number:06}{}\r\n", "x".repeat(194))
}

fn spinner_frame(tick: usize) -> String {
    let glyph = ["|", "/", "-", "\\"][tick % 4];
    format!("\r\x1b[2K{glyph} Working... ({}s)", tick / 10)
}

/// One pane and its own clock, fed a turn at a time and asked to settle at every turn's end, as
/// the window's turn asks.
struct PrintingPane {
    session: DualPlaneSession,
    clock: Instant,
    lines: usize,
}

impl PrintingPane {
    /// A pane narrowed to [`NARROW`] columns at its first layout and settled there, quiet.
    fn settled(capacity: usize) -> Self {
        let clock = Instant::now();
        let mut session = DualPlaneSession::with_frozen_quota(
            nz(100),
            nz(ROWS),
            NonZeroUsize::new(capacity).expect("a positive capacity"),
        );
        session.feed_at(b"PS> ", clock).expect("a prompt");
        session
            .resize_at(nz(NARROW), nz(ROWS), clock)
            .expect("the resize reflows");
        session.mark_pty_resize_requested_at(nz(NARROW), nz(ROWS), clock);
        assert!(
            session
                .finish_resize_if_quiescent(clock + QUIET)
                .expect("the transaction settles"),
            "a quiet pane settles at the quiet window"
        );
        Self {
            session,
            clock: clock + QUIET,
            lines: 0,
        }
    }

    /// Feed one drain turn at `step` past the clock. Answers whether the end-of-turn ask closed
    /// the transaction.
    fn turn(&mut self, bytes: &[u8], step: Duration) -> bool {
        self.clock += step;
        self.feed_only(bytes);
        self.session
            .finish_resize_if_quiescent(self.clock)
            .expect("an ask settles or waits")
    }

    fn feed_only(&mut self, bytes: &[u8]) {
        self.session.begin_feed_turn();
        self.session
            .feed_at(bytes, self.clock)
            .expect("the output is plain text");
        self.session.end_feed_turn();
    }

    /// The next numbered lines up to `lines`, as much as one drain slice holds.
    fn next_slice(&mut self, lines: usize) -> Vec<u8> {
        let mut slice = Vec::with_capacity(SLICE + 256);
        while slice.len() < SLICE && self.lines < lines {
            slice.extend_from_slice(numbered_line(self.lines).as_bytes());
            self.lines += 1;
        }
        slice
    }

    /// Flood a slice a turn, 1 ms apart, until `lines`, with no ask in between.
    fn print_quietly_until(&mut self, lines: usize) {
        while self.lines < lines {
            let slice = self.next_slice(lines);
            self.clock += Duration::from_millis(1);
            self.feed_only(&slice);
        }
    }

    /// Resize mid-output and tell the child at once, as the window's release does.
    fn resize_and_tell(&mut self, columns: u32) {
        self.session
            .resize_at(nz(columns), nz(ROWS), self.clock)
            .expect("the resize reflows");
        self.session
            .mark_pty_resize_requested_at(nz(columns), nz(ROWS), self.clock);
        assert!(self.session.resize_transaction_open());
    }

    fn staged_cells(&self) -> usize {
        self.session
            .transcript()
            .resize_staged_rows()
            .iter()
            .map(|row| row.row.cells.len())
            .sum()
    }

    fn end_reason(&self) -> Option<ResizeSettleReason> {
        self.session
            .resize_trace()
            .iter()
            .find_map(|event| match event.kind {
                ResizeTraceKind::TransactionEnd { reason } => Some(reason),
                _ => None,
            })
    }

    /// The numbers of the frozen lines that begin with one, oldest first.
    fn frozen_numbers(&self) -> Vec<usize> {
        self.session
            .transcript()
            .frozen()
            .iter()
            .filter_map(|line| {
                let text = line.text.strip_prefix("PS> ").unwrap_or(&line.text);
                text.get(..6).and_then(|number| number.parse().ok())
            })
            .collect()
    }

    fn typeset_formulas(&self) -> usize {
        self.session
            .document()
            .entries()
            .values()
            .filter(|entry| matches!(entry.decoration, DecorationIntent::Math { .. }))
            .count()
    }
}

/// The cells one drain slice of the flood can add to the tail at [`WIDER`] columns: its lines,
/// one more for a line it starts, five rows each.
const fn cells_one_slice_adds() -> usize {
    (SLICE / 202 + 2) * 5 * WIDER as usize
}

/// T1. A flood that never pauses settles once the tail has grown by the staging budget since
/// the child was told, and the tail never holds more than the budget and one slice beyond what
/// it held then.
#[test]
fn a_flood_that_never_pauses_settles_at_the_staging_budget() {
    let mut pane = PrintingPane::settled(2_000);
    pane.print_quietly_until(500);
    pane.resize_and_tell(WIDER);
    let at_request = pane.staged_cells();
    let mut most = at_request;
    let mut settled = false;
    while pane.lines < 3_500 && !settled {
        let slice = pane.next_slice(3_500);
        settled = pane.turn(&slice, Duration::from_millis(1));
        most = most.max(pane.staged_cells());
    }
    assert!(
        settled,
        "3 000 lines with no pause left the transaction open, holding {} cells of tail",
        pane.staged_cells()
    );
    assert_eq!(pane.end_reason(), Some(ResizeSettleReason::StagingBudget));
    assert!(
        most <= at_request + RESIZE_STAGING_BUDGET_CELLS + cells_one_slice_adds(),
        "the tail reached {most} cells, past the budget and one slice beyond {at_request}"
    );
}

/// T1b. The rows the reflow itself pushes off when the child is told are not counted against
/// the budget: a tall pane narrowed past the budget in one step waits for its child, here the
/// bound, instead of settling on the first turn.
#[test]
fn the_tail_the_reflow_pushed_off_is_not_counted_against_the_budget() {
    let clock = Instant::now();
    let mut session = DualPlaneSession::new(nz(200), nz(1_000));
    let mut screen = Vec::new();
    for number in 0..999 {
        screen.extend_from_slice(format!("{number:06}{}\r\n", "z".repeat(190)).as_bytes());
    }
    session.feed_at(&screen, clock).expect("plain text");
    session
        .resize_at(nz(WIDER), nz(1_000), clock)
        .expect("the resize reflows");
    session.mark_pty_resize_requested_at(nz(WIDER), nz(1_000), clock);
    let pushed_off: usize = session
        .transcript()
        .resize_staged_rows()
        .iter()
        .map(|row| row.row.cells.len())
        .sum();
    assert!(
        pushed_off > RESIZE_STAGING_BUDGET_CELLS,
        "the fixture has to push more than the budget off: {pushed_off} cells"
    );
    let at = clock + FRAME;
    session.begin_feed_turn();
    session
        .feed_at(spinner_frame(0).as_bytes(), at)
        .expect("plain text");
    session.end_feed_turn();
    assert!(
        !session.finish_resize_if_quiescent(at).expect("an ask"),
        "a tail the reflow pushed off settled the transaction on the first turn"
    );
}

/// T2. A settle under the flood moves nothing: the flood's text is in history once and in order,
/// and the screen is what it was. The reflow happened when the child was told, so the settle has
/// nothing to move.
#[test]
fn a_settle_under_the_flood_moves_nothing() {
    let mut pane = PrintingPane::settled(10_000);
    pane.print_quietly_until(500);
    pane.resize_and_tell(WIDER);
    let mut settled_screen = None;
    while pane.lines < 4_000 {
        let slice = pane.next_slice(4_000);
        pane.clock += Duration::from_millis(1);
        pane.feed_only(&slice);
        let before = pane.session.terminal().visible_text();
        if pane
            .session
            .finish_resize_if_quiescent(pane.clock)
            .expect("an ask")
        {
            settled_screen = Some((before, pane.session.terminal().visible_text()));
        }
    }
    let (before, after) = settled_screen.expect("the transaction settled under the flood");
    assert_eq!(before, after, "the settle changed the screen");
    // The text, not the line count: the line that straddled the screen's top when the child was
    // told is frozen in two pieces by that reflow (its head before, its tail after), with a pause
    // as without one, so what is kept once and in order is the characters.
    let kept = pane
        .session
        .transcript()
        .frozen()
        .iter()
        .map(|line| line.text.strip_prefix("PS> ").unwrap_or(&line.text))
        .collect::<String>();
    let last = pane
        .frozen_numbers()
        .last()
        .copied()
        .expect("history holds the flood");
    let printed = (0..=last)
        .map(|number| numbered_line(number).trim_end().to_owned())
        .collect::<String>();
    assert!(
        kept == printed,
        "history is not the flood once and in order up to line {last}: {} characters kept, {} printed",
        kept.len(),
        printed.len()
    );
    assert!(
        last + 30 >= 4_000,
        "history ends where the screen begins: line {last}"
    );
}

/// T3. A spinner ten times a second never gives the quiet window; the transaction settles at
/// the bound after the child was told and not a frame before, and the deadline the window wakes
/// for never lies past the bound while it spins.
#[test]
fn a_spinner_that_never_pauses_settles_at_the_bound() {
    let mut pane = PrintingPane::settled(2_000);
    pane.print_quietly_until(300);
    pane.resize_and_tell(WIDER);
    let told = pane.clock;
    let mut settled_at = None;
    for tick in 0..30 {
        if pane.turn(spinner_frame(tick).as_bytes(), FRAME) {
            settled_at = Some(pane.clock);
            break;
        }
        assert_eq!(
            pane.session.resize_finish_deadline(),
            Some((pane.clock + QUIET).min(told + RESIZE_SETTLE_BOUND)),
            "while it spins, the window wakes for the quiet window or the bound, whichever is first"
        );
    }
    let settled_at = settled_at.expect("the spinner's transaction settled");
    assert!(
        settled_at >= told + RESIZE_SETTLE_BOUND && settled_at < told + RESIZE_SETTLE_BOUND + FRAME,
        "settled {:?} after the child was told",
        settled_at - told
    );
    assert_eq!(pane.end_reason(), Some(ResizeSettleReason::TimeBound));
}

/// T4. A formula printed while the transaction is open is typeset once it settles at the bound,
/// while the program keeps writing.
#[test]
fn a_formula_printed_under_a_spinner_is_typeset_after_the_bound() {
    let mut pane = PrintingPane::settled(2_000);
    pane.print_quietly_until(300);
    pane.session.run_workers(&|_| None);
    pane.resize_and_tell(WIDER);
    let mut answer = String::from("\r\x1b[2K$$\\frac{a}{b}$$\r\n");
    for line in 0..ROWS + 6 {
        answer.push_str(&format!("answer line {line}\r\n"));
    }
    pane.turn(answer.as_bytes(), FRAME);
    for tick in 0..16 {
        pane.turn(spinner_frame(tick).as_bytes(), FRAME);
        pane.session.run_workers(&|_| None);
    }
    assert!(
        pane.typeset_formulas() > 0,
        "the formula the program printed 1.6 s ago, still writing, is not typeset"
    );
}

/// T5. Before the child is told, neither the bound nor the budget settles anything: the grid
/// cannot be harvested against a size the child does not hold.
#[test]
fn nothing_settles_before_the_child_is_told() {
    let mut pane = PrintingPane::settled(2_000);
    pane.print_quietly_until(300);
    pane.session
        .resize_at(nz(WIDER), nz(ROWS), pane.clock)
        .expect("the resize reflows");
    let reflowed_at = pane.clock;
    while pane.lines < 2_000 {
        let slice = pane.next_slice(2_000);
        assert!(
            !pane.turn(&slice, FRAME),
            "a transaction settled at {} lines before its child was told",
            pane.lines
        );
    }
    assert!(
        pane.staged_cells() > RESIZE_STAGING_BUDGET_CELLS
            && pane.clock - reflowed_at > RESIZE_SETTLE_BOUND,
        "the fixture has to run past both the budget and the bound"
    );
    assert!(pane.session.resize_transaction_open());
}
