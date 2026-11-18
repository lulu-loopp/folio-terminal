//! **A full scrollback holds still in the window too** (0.4.10 plan row 105, T5).
//!
//! `bt-term`'s `full_scrollback_residency` counts what a session and its projection hold once
//! history is full. This is the same claim one layer up: a headless window with one shell pane,
//! fed the flood a drain slice at a time and publishing a frame after each, as the window
//! thread's turn does — the search re-ask, the decoration dispatch, the frame and the glass
//! included. Counted by the test build's allocator on this thread (`held_bytes`), not timed.

use std::num::{NonZeroU32, NonZeroUsize};
use std::time::{Duration, Instant};

use bt_render::{FrameSource, FrameTrigger};

use crate::headless::HeadlessFolio;
use crate::pointer_app_tests::held_bytes;
use crate::test_support::ringing_tab;

/// PowerShell's `'x' * 200`, one line per write — the clean VM's flood.
fn flood_line() -> String {
    format!("{}\r\n", "x".repeat(200))
}

/// What one drain reads: `bt_pty::TERM_READ_SLICE`.
const SLICE: usize = 8 * 1024;
const CAPACITY: usize = 2_000;

struct Window {
    folio: HeadlessFolio,
    lines: usize,
}

impl Window {
    fn new() -> Self {
        let mut tab = ringing_tab(1, 1);
        let seat = tab.focused_leaf;
        tab.sessions
            .get_mut(&seat)
            .expect("the fixture's seat holds a shell")
            .session
            .session
            .set_frozen_quota(NonZeroUsize::new(CAPACITY).expect("a positive capacity"));
        let mut folio = HeadlessFolio::with_tabs("full-scrollback", vec![tab], 0);
        // The window's layout resized the pane, and a resize holds the rows it moves until the
        // child has been asked for the size and gone quiet. This pane has no child, so the ask is
        // said here — as the drain says it of a shell's pseudoconsole — and a turn long after it
        // ends the transaction, as the turn loop does.
        let leaf = &mut folio.window.tabs[0].shell_pane_mut().session;
        let (columns, rows) = (
            NonZeroU32::from(leaf.grid.columns),
            NonZeroU32::from(leaf.grid.rows),
        );
        leaf.session
            .mark_pty_resize_requested_at(columns, rows, Instant::now());
        folio
            .runtime()
            .finish_resize_if_quiescent(Instant::now() + Duration::from_secs(60))
            .expect("the pane's resize ends");
        assert!(
            folio.window.tabs[0]
                .shell_pane()
                .session
                .session
                .resize_finish_deadline()
                .is_none(),
            "no resize is standing when the flood begins"
        );
        Self { folio, lines: 0 }
    }

    /// Print until `lines` lines have been written: each drain slice fed the way
    /// `drain_leaf_pty` feeds it, inside the feed turn, then the frame its output asks for.
    fn print_until(&mut self, lines: usize) {
        let line = flood_line();
        while self.lines < lines {
            let mut slice = Vec::with_capacity(SLICE + line.len());
            while slice.len() < SLICE && self.lines < lines {
                slice.extend_from_slice(line.as_bytes());
                self.lines += 1;
            }
            let leaf = &mut self.folio.window.tabs[0].shell_pane_mut().session;
            leaf.session.begin_feed_turn();
            leaf.session
                .feed_at(&slice, Instant::now())
                .expect("the flood is plain text");
            leaf.session.end_feed_turn();
            leaf.output_revision = leaf.output_revision.wrapping_add(1);
            let mut runtime = self.folio.runtime();
            runtime
                .publish_frame(FrameTrigger {
                    occurred_at: Instant::now(),
                    source: FrameSource::PtyOutput,
                })
                .expect("a headless frame composes");
            runtime.redraw().expect("and reaches the glass");
            // The turn's own last step: a resize the layout made ends once the pane is quiet.
            runtime
                .finish_resize_if_quiescent(Instant::now() + Duration::from_secs(60))
                .expect("the pane's resize ends");
            self.folio.ledger.take();
        }
    }
}

#[test]
fn a_full_scrollback_holds_still_in_the_window() {
    let mut window = Window::new();
    window.print_until(2 * CAPACITY);
    let frozen = window.folio.window.tabs[0]
        .shell_pane()
        .session
        .session
        .transcript()
        .frozen()
        .len();
    assert_eq!(
        frozen, CAPACITY,
        "the history is full before the first measurement"
    );
    let full = held_bytes();
    let mut trail = Vec::new();
    for step in 1..=4 {
        window.print_until(2 * CAPACITY + step * CAPACITY);
        trail.push((window.lines, held_bytes() - full));
    }
    let grown = held_bytes() - full;
    eprintln!("lines, bytes held beyond the full history: {trail:?}");
    // The window's layout resized this pane, and the resize settled before the flood (`new`), so
    // the vendored grid holds no history of its own. A release build once kept every row there,
    // and a count taken in a build that evaluates its assertions could not see it (0.4.10 plan
    // row 107).
    let vendor_rows = window.folio.window.tabs[0]
        .shell_pane()
        .session
        .session
        .terminal()
        .alacritty_history_size();
    assert_eq!(
        vendor_rows, 0,
        "the vendored grid kept {vendor_rows} rows of history after the pane's resize settled"
    );
    assert!(
        grown.abs() < 64 * 1024,
        "{} more lines past a full {CAPACITY}-line history changed what the window holds by \
         {grown} bytes ({:.1} a line); lines, bytes: {trail:?}",
        4 * CAPACITY,
        grown as f64 / (4 * CAPACITY) as f64,
    );
}
