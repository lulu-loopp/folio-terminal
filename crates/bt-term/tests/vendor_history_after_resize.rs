//! **A settled resize leaves the vendor grid no history** (0.4.10 plan row 107,
//! T-SIX-KB-PER-LINE-PAST-THE-CAP).
//!
//! The vendored grid keeps no scrollback of its own (`bt_term::SCROLLBACK_LINES` is 0): a row
//! that leaves the screen is handed to the transcript and frozen there. Only a resize
//! transaction lets the grid hold history, and only until the transaction settles. The clean-VM
//! flood of 2026-11-18 grew the process by about 6 KB a line, one alacritty row per wrapped
//! row: a resize that had staged nothing was closed only inside a `debug_assert!`, so in a
//! release build the grid went on keeping every row the pane printed, beside the transcript.
//! These claims are about the grid itself, read through `alacritty_history_size`, so they hold
//! whether or not the build evaluates its assertions.

use std::{
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

use bt_term::DualPlaneSession;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("a positive dimension")
}

/// The width the VM run's focused pane wore after its two splits.
const COLUMNS: u32 = 31;
const ROWS: u32 = 24;
/// What one drain reads: `bt_pty::TERM_READ_SLICE`.
const SLICE: usize = 8 * 1024;
/// How long a settled resize waits for the child's output to go quiet (bt-term's
/// `RESIZE_IDLE_AFTER_OUTPUT`).
const QUIET: Duration = Duration::from_millis(200);

/// Line `number` of the flood: 200 characters, its number first, so a line that came back
/// twice is visible as a number seen twice.
fn numbered_flood_line(number: usize) -> String {
    format!("{number:06}{}\r\n", "x".repeat(194))
}

/// One pane, fed the way the drain feeds it: a slice at a time inside a feed turn.
struct NarrowedPane {
    session: DualPlaneSession,
    pending: Vec<u8>,
    lines: usize,
}

impl NarrowedPane {
    /// A pane opened wide with only its prompt on screen, then narrowed to [`COLUMNS`] — the
    /// first layout resize a split gives a fresh pane, which pushes nothing off the top — and
    /// settled as the window settles it: the child is told the size, and the pane goes quiet.
    fn narrowed_and_settled(capacity: usize) -> Self {
        let mut session = DualPlaneSession::with_frozen_quota(
            nz(100),
            nz(ROWS),
            NonZeroUsize::new(capacity).expect("a positive capacity"),
        );
        session.feed(b"PS> ").expect("a prompt is plain text");
        let mut pane = Self {
            session,
            pending: Vec::new(),
            lines: 0,
        };
        pane.resize_and_settle(COLUMNS);
        assert_eq!(
            pane.session.terminal().alacritty_history_size(),
            0,
            "a resize of a pane holding only its prompt moves nothing into history"
        );
        pane
    }

    fn resize_and_settle(&mut self, columns: u32) {
        let at = Instant::now();
        self.session
            .resize_at(nz(columns), nz(ROWS), at)
            .expect("the resize reflows");
        assert!(
            self.session.resize_transaction_open(),
            "a width change opens a resize transaction"
        );
        self.session
            .mark_pty_resize_requested_at(nz(columns), nz(ROWS), at);
        assert!(
            self.session
                .finish_resize_if_quiescent(at + QUIET)
                .expect("the transaction settles"),
            "a quiet pane's resize settles once the child has been told"
        );
        assert!(!self.session.resize_transaction_open());
    }

    /// Print until `lines` lines have been written, a drain's slice at a time.
    fn print_numbered_until(&mut self, lines: usize) {
        while self.lines < lines {
            while self.pending.len() < SLICE && self.lines < lines {
                self.pending
                    .extend_from_slice(numbered_flood_line(self.lines).as_bytes());
                self.lines += 1;
            }
            let take = self.pending.len().min(SLICE);
            let slice = self.pending.drain(..take).collect::<Vec<u8>>();
            self.feed_drain_slice(&slice);
        }
        let slice = std::mem::take(&mut self.pending);
        if !slice.is_empty() {
            self.feed_drain_slice(&slice);
        }
    }

    fn feed_drain_slice(&mut self, slice: &[u8]) {
        self.session.begin_feed_turn();
        self.session.feed(slice).expect("the flood is plain text");
        self.session.end_feed_turn();
    }

    fn vendor_history_rows(&self) -> usize {
        self.session.terminal().alacritty_history_size()
    }

    /// The numbers of the frozen lines, oldest first.
    fn frozen_numbers(&self) -> Vec<usize> {
        self.session
            .transcript()
            .frozen()
            .iter()
            .map(|line| {
                // The first line of the flood was typed at the prompt, so it is frozen behind it.
                let text = line.text.strip_prefix("PS> ").unwrap_or(&line.text);
                text.get(..6)
                    .and_then(|number| number.parse().ok())
                    .unwrap_or_else(|| panic!("a frozen line of the flood: {:?}", line.text))
            })
            .collect()
    }
}

/// T-a. After a resize that staged nothing has settled, eight thousand lines past a full
/// two-thousand-line history leave the vendor grid holding no row of history.
#[test]
fn a_settled_resize_leaves_the_vendor_no_history() {
    const CAPACITY: usize = 2_000;
    let mut pane = NarrowedPane::narrowed_and_settled(CAPACITY);
    pane.print_numbered_until(2 * CAPACITY);
    let mut trail = vec![(pane.lines, pane.vendor_history_rows())];
    for step in 1..=4 {
        pane.print_numbered_until(2 * CAPACITY + step * CAPACITY);
        trail.push((pane.lines, pane.vendor_history_rows()));
    }
    eprintln!("lines fed, rows of vendor history: {trail:?}");
    assert!(
        trail.iter().all(|(_, rows)| *rows == 0),
        "the vendor grid kept history after its resize settled; lines fed, vendor rows: {trail:?}"
    );
}

/// T-d. A second resize after a long flood keeps every line in history once: the frozen lines
/// are the same sequence before and after it, and no number is frozen twice.
#[test]
fn a_resize_after_a_long_flood_keeps_history_once() {
    let mut pane = NarrowedPane::narrowed_and_settled(10_000);
    pane.print_numbered_until(3_000);
    let before = pane.frozen_numbers();
    assert!(
        before.len() > 2_900,
        "the flood is in history: {} lines",
        before.len()
    );
    pane.resize_and_settle(40);
    let after = pane.frozen_numbers();
    let first_repeat = after
        .windows(2)
        .find(|pair| pair[1] <= pair[0])
        .map(|pair| (pair[0], pair[1]));
    assert_eq!(
        first_repeat,
        None,
        "a line came back into history: {} lines before the resize, {} after",
        before.len(),
        after.len()
    );
    assert_eq!(
        after.get(..before.len()),
        Some(before.as_slice()),
        "the resize kept the history it found ({} lines before, {} after)",
        before.len(),
        after.len()
    );
}
