//! What a pane keeps and what it does per byte, measured rather than argued.
//!
//! Both tests here weigh the heap, because both defects they stand on are invisible to any
//! assertion about what the screen says: the row looks right while it costs rows times columns
//! copies of one target, and an ignored clipboard store looks like nothing at all while the
//! payload behind it is decoded in full first.
//!
//! The one `unsafe` in the tree outside `bt-platform`: a [`GlobalAlloc`] implementation is unsafe
//! by signature, and there is no safe way to ask the process how many bytes a piece of work asked
//! for. Every operation below forwards straight to [`System`].
#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeSet,
    num::NonZeroU32,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use bt_term::TerminalAdapter;

/// The system allocator, counting the bytes it is asked for while a measurement is open.
struct Weighing;

static MEASURING: AtomicBool = AtomicBool::new(false);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Weighing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if MEASURING.load(Ordering::Relaxed) {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if MEASURING.load(Ordering::Relaxed) && new_size > layout.size() {
            ALLOCATED.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static WEIGHING: Weighing = Weighing;

/// One measurement at a time, because the counter is process-wide.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn bytes_allocated_by(work: impl FnOnce()) -> usize {
    ALLOCATED.store(0, Ordering::Relaxed);
    MEASURING.store(true, Ordering::Relaxed);
    work();
    MEASURING.store(false, Ordering::Relaxed);
    ALLOCATED.load(Ordering::Relaxed)
}

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("a positive dimension")
}

/// R1-15. One `OSC 8` target covering a whole row is one string, and the capture keeps it one.
///
/// The target is sized just under the ceiling `MAX_UNOWNED_OSC_BYTES` puts on a sequence this
/// terminal passes through, because after that ceiling a longer one does not reach the grid at
/// all: the two bounds compose, and this is the largest target a row can still wear.
#[test]
fn a_row_under_one_long_hyperlink_captures_one_shared_target() {
    let _one_at_a_time = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|held| held.into_inner());

    const TARGET_BYTES: usize = 48 * 1024;
    const COLUMNS: u32 = 200;

    let uri = format!("https://example.test/{}", "a".repeat(TARGET_BYTES - 21));
    assert_eq!(uri.len(), TARGET_BYTES);
    let mut terminal = TerminalAdapter::new(nz(COLUMNS), nz(4));
    let text = "x".repeat(COLUMNS as usize);
    terminal.feed(format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\").as_bytes());

    let mut captured = None;
    let bytes = bytes_allocated_by(|| captured = terminal.visible_row(0));
    let captured = captured.expect("the first row");

    let wearing = captured
        .cells
        .iter()
        .filter(|cell| cell.hyperlink.is_some())
        .count();
    assert_eq!(wearing, COLUMNS as usize, "every cell wears the link");

    let targets = captured
        .cells
        .iter()
        .filter_map(|cell| cell.hyperlink.as_ref())
        .map(|link| link.uri.as_ptr())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        targets.len(),
        1,
        "the {COLUMNS} cells share one allocation of the target"
    );

    assert!(
        bytes < TARGET_BYTES * 2,
        "capturing the row cost {bytes} bytes for a {TARGET_BYTES}-byte target"
    );
}

/// R1-27. This terminal does not act on an `OSC 52` store, so it must not pay for one either.
///
/// Measured against a store this terminal refuses for a different reason — an unknown clipboard
/// selector, which the vendored handler rejects before it decodes anything either way. The two
/// feeds are the same length and travel the same path; the only thing that can separate them is
/// whether the payload was decoded.
#[test]
fn an_osc_52_store_is_refused_before_it_is_decoded() {
    let _one_at_a_time = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|held| held.into_inner());

    // Base64, so the decoded form is three quarters of this.
    let payload = "A".repeat(48 * 1024);
    let store = format!("\x1b]52;c;{payload}\x07");
    let unknown_selector = format!("\x1b]52;X;{payload}\x07");

    let mut asked = TerminalAdapter::new(nz(80), nz(24));
    let mut refused = TerminalAdapter::new(nz(80), nz(24));
    let stored = bytes_allocated_by(|| {
        asked.feed(store.as_bytes());
    });
    let control = bytes_allocated_by(|| {
        refused.feed(unknown_selector.as_bytes());
    });

    assert!(
        stored <= control + 4096,
        "the store cost {stored} bytes where the refusal cost {control}, so the payload was \
         decoded before it was dropped"
    );
}
