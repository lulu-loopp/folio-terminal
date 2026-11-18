//! **A full scrollback holds still** (0.4.10 plan row 105, T-FULL-SCROLLBACK-FEEDS-SLOW-AND-GROWS).
//!
//! Once a pane's history has reached its line capacity, every new line evicts the oldest one,
//! so what the pane holds must stop growing. The clean-VM run of 2026-11-18 measured the
//! opposite: a flood of 200-character lines grew the process by about 5.7 KB a line for ten
//! minutes past the cap. This binary counts the heap that is still held — every allocation
//! added, every free taken away — on the one thread that drives a session and the projection
//! a publish draws it through, and asks that the count at two depths past the cap be the same.

#![allow(unsafe_code)]
#![allow(clippy::disallowed_methods)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    num::{NonZeroU32, NonZeroUsize},
};

use bt_term::DualPlaneSession;
use bt_viewport::ViewportProjection;

thread_local! {
    /// Bytes this thread has allocated and not yet freed, since the thread began.
    static HELD: Cell<i64> = const { Cell::new(0) };
}

fn hold(bytes: usize) {
    let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    // `try_with`: the allocator also serves the thread's own teardown, after its locals are gone.
    let _ = HELD.try_with(|held| held.set(held.get().wrapping_add(bytes)));
}

fn release(bytes: usize) {
    let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    let _ = HELD.try_with(|held| held.set(held.get().wrapping_sub(bytes)));
}

/// The heap a thread is still holding, counted at the allocator.
struct HeldBytes;

// SAFETY: every method forwards its arguments unchanged to `System`, which upholds
// `GlobalAlloc`'s contract; the counting touches only a thread-local `Cell`.
unsafe impl GlobalAlloc for HeldBytes {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        hold(layout.size());
        // SAFETY: the caller's contract for `alloc` is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        hold(layout.size());
        // SAFETY: as `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        hold(new_size);
        release(layout.size());
        // SAFETY: as `alloc`; `pointer` came from this allocator, which is `System`.
        unsafe { System.realloc(pointer, layout, new_size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        release(layout.size());
        // SAFETY: `pointer` came from this allocator, which is `System`.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static HELD_BYTES: HeldBytes = HeldBytes;

fn held() -> i64 {
    HELD.with(Cell::get)
}

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("a positive dimension")
}

/// The VM run's line: PowerShell's `'x' * 200`, one line per write.
const LINE: &str = concat!(
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    "\r\n"
);
/// What one drain reads: `bt_pty::TERM_READ_SLICE`.
const SLICE: usize = 8 * 1024;
const COLUMNS: u32 = 40;
const ROWS: u32 = 24;
const CAPACITY: usize = 2_000;

/// One pane: the session, the projection its publish draws it through, and the flood's bytes.
struct Pane {
    session: DualPlaneSession,
    view: ViewportProjection,
    pending: Vec<u8>,
    lines: usize,
}

impl Pane {
    fn new() -> Self {
        let session = DualPlaneSession::with_frozen_quota(
            nz(COLUMNS),
            nz(ROWS),
            NonZeroUsize::new(CAPACITY).expect("a positive capacity"),
        );
        let view = session.new_projection(session.layout_key());
        Self {
            session,
            view,
            pending: Vec::new(),
            lines: 0,
        }
    }

    /// Print until `lines` lines have been written, a drain's slice at a time, each drain
    /// followed by the publish the window thread runs after it (`bt_compose::project`).
    fn print_until(&mut self, lines: usize) {
        while self.lines < lines {
            while self.pending.len() < SLICE && self.lines < lines {
                self.pending.extend_from_slice(LINE.as_bytes());
                self.lines += 1;
            }
            let take = self.pending.len().min(SLICE);
            let slice = self.pending.drain(..take).collect::<Vec<u8>>();
            self.session.begin_feed_turn();
            self.session.feed(&slice).expect("the flood is plain text");
            self.session.end_feed_turn();
            self.publish();
        }
        let slice = std::mem::take(&mut self.pending);
        if !slice.is_empty() {
            self.session.feed(&slice).expect("the flood is plain text");
            self.publish();
        }
    }

    fn publish(&mut self) {
        self.session.refresh_projection(&mut self.view);
        let frame = self
            .session
            .viewport_frame(&mut self.view)
            .expect("the flood projects");
        self.session.absorb_printed_path_probes(&mut self.view);
        self.session.observe_view(&self.view);
        self.session.schedule_visible_artifacts(&frame);
    }
}

#[test]
fn history_holds_still_once_the_scrollback_is_full() {
    let mut pane = Pane::new();
    pane.print_until(2 * CAPACITY);
    assert_eq!(
        pane.session.transcript().frozen().len(),
        CAPACITY,
        "the history is full before the first measurement"
    );
    let full = held();
    let mut trail = Vec::new();
    for step in 1..=4 {
        pane.print_until(2 * CAPACITY + step * CAPACITY);
        trail.push((
            pane.lines,
            held() - full,
            pane.session.document().entries().len(),
            pane.view.cache_len(),
        ));
    }
    let grown = held() - full;
    eprintln!(
        "lines, bytes held beyond the full history, document entries, layout slots: {trail:?}"
    );
    assert!(
        grown.abs() < 16 * 1024,
        "{} more lines past a full {CAPACITY}-line history changed what the pane holds by {grown} \
         bytes ({:.1} a line); lines, bytes, entries, layout slots: {trail:?}",
        4 * CAPACITY,
        grown as f64 / (4 * CAPACITY) as f64,
    );
}
