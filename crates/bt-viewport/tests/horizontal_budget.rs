//! The horizontal axis's performance gate (`docs/plans/horizontal-scroll/plan.md` §1b, §5.7).
//!
//! One continuous horizontal scroll across the pathological lines §1b names — a hundred thousand
//! graphemes, dense styling and OSC 8, CJK and combining marks — measuring what a frame of it
//! costs: locate the window's first cell, then materialize `viewport_columns + overscan` of them.
//!
//! # What is asserted, and in what order it is trusted
//!
//! Mirrors the resize nail (`crates/bt-term/tests/lifecycle_matrix.rs`), for the same reason it
//! was built that way: a summed wall clock on a machine sharing itself with nineteen `rustc`
//! processes measures the machine.
//!
//! 1. **Complexity, exactly.** A scroll frame must touch a number of cells that depends on the
//!    window and not on the line. This is measured as heap draw per frame and is the same in
//!    every run, on any machine, in any build — a hundred-thousand-column line and an
//!    eighty-column one must cost a frame the same, which is the whole claim §1b makes.
//! 2. **The first index build**, which §1b allows to be O(line length) and requires to have a
//!    stated ceiling.
//! 3. **The median frame's wall clock**, against §1b's 3 ms — the loosest signal, kept because
//!    the budget is stated in milliseconds and somebody has to check.
//!
//! Every number is printed under `BT_HSCROLL_BENCH` whether or not the assertions pass, so a run
//! that only widens the margin still leaves the measurement behind.

use std::time::{Duration, Instant};

use bt_transcript::{
    CellFlags, CellHyperlink, CellStyle, FrozenLine, HyperlinkRange, PhysicalFragment,
    SourceGeneration, StyleSpan, TerminalColor, TranscriptId,
};
use bt_viewport::{
    InferredLink,
    horizontal::{
        CHECKPOINT_STRIDE_COLUMNS, ContentColumn, HorizontalIndexStore, HorizontalProjection,
        INDEX_STORE_BUDGET_BYTES, LINE_INDEX_BUDGET_BYTES, LineColumnIndex, LineKey,
        seek_from_start, window_flattened_line,
    },
};

/// Thread-local on purpose: the other tests of this binary run on their own threads and must not
/// land in a frame's total.
struct ScrollHeapCounter;

thread_local! {
    static SCROLL_HEAP_BYTES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static SCROLL_HEAP_ALLOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[expect(
    unsafe_code,
    reason = "a global allocator has no safe form. Every method here forwards to \
              `std::alloc::System` with its arguments unchanged and adds nothing but two \
              thread-local counter bumps, so the safety contract is exactly System's."
)]
unsafe impl std::alloc::GlobalAlloc for ScrollHeapCounter {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        charge(layout.size() as u64);
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        charge(layout.size() as u64);
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        // A grow is charged for the growth only; a shrink returns memory and is charged nothing.
        charge(new_size.saturating_sub(layout.size()) as u64);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

fn charge(bytes: u64) {
    SCROLL_HEAP_BYTES.with(|counter| counter.set(counter.get().wrapping_add(bytes)));
    SCROLL_HEAP_ALLOCATIONS.with(|counter| counter.set(counter.get().wrapping_add(1)));
}

#[global_allocator]
static SCROLL_HEAP_COUNTER: ScrollHeapCounter = ScrollHeapCounter;

/// A pane's width, plus §1a's overscan: what one frame of a horizontal scroll materializes.
const VIEWPORT_COLUMNS: u32 = 120;
const OVERSCAN_COLUMNS: u32 = 8;
const WINDOW_COLUMNS: u32 = VIEWPORT_COLUMNS + OVERSCAN_COLUMNS;

/// Frames in one measured scroll — a flick across the line at a few columns a frame.
const SCROLL_FRAMES: usize = 200;

/// §1b's budget for a continuous horizontal scroll frame.
const FRAME_CEILING: Duration = Duration::from_millis(3);

/// §1b's "first index build has a stated ceiling". Generous by design: it is allowed to be
/// O(line length) and runs once per line, so the number here is a bound on the pathological case
/// and not a target.
const INDEX_BUILD_CEILING: Duration = Duration::from_millis(50);

fn frozen_line(text: String, styles: Vec<StyleSpan>) -> FrozenLine {
    let mut grapheme_boundaries: Vec<u32> = text
        .char_indices()
        .map(|(index, _)| index as u32)
        .collect::<Vec<_>>();
    // Grapheme boundaries are only read for the tail anchor here; the flatten walks clusters
    // itself, so a per-scalar list is a sound over-approximation for a fixture.
    grapheme_boundaries.push(text.len() as u32);
    grapheme_boundaries.dedup();
    FrozenLine {
        id: TranscriptId(1),
        source_generation: SourceGeneration(1),
        fragments: vec![PhysicalFragment {
            byte_start: 0,
            byte_end: text.len() as u32,
            soft_wrapped: false,
            captured_columns: 80,
        }],
        grapheme_boundaries,
        text,
        styles,
        shell_marks: Vec::new(),
        wrap_split: false,
    }
}

/// The three shapes §1b names, plus the eighty-column line they have to cost the same as.
fn corpus() -> Vec<(&'static str, FrozenLine, Vec<InferredLink>)> {
    let plain = "abcdefghij".repeat(10_000);
    let cjk = "漢字仮名の混じった文章".repeat(9_000);
    let combining = "e\u{301}o\u{308}u\u{30a}".repeat(33_000);

    // A colour every fourth column and an OSC 8 span every two hundred: about as much styling as
    // a terminal ever paints, over a line long enough that a per-cell linear scan of the span
    // list would be quadratic.
    let mut styles = Vec::new();
    let mut byte = 0u32;
    let mut run = 0u32;
    while (byte as usize) < plain.len() {
        let end = (byte + 4).min(plain.len() as u32);
        styles.push(StyleSpan {
            byte_start: byte,
            byte_end: end,
            style: CellStyle {
                flags: if run.is_multiple_of(7) {
                    CellFlags::BOLD
                } else {
                    CellFlags::empty()
                },
                foreground: TerminalColor::Indexed((16 + run % 200) as u8),
                background: TerminalColor::Named(17),
            },
            hyperlink: run.is_multiple_of(50).then(|| CellHyperlink {
                id: Some(format!("run-{run}")),
                uri: format!("https://example.test/{run}"),
            }),
        });
        byte = end;
        run += 1;
    }

    let inferred = vec![InferredLink {
        range: HyperlinkRange {
            byte_start: 0,
            byte_end: 40,
        },
        uri: "https://example.test/head".to_owned(),
        resting_dotted: true,
    }];

    vec![
        (
            "ascii-100k",
            frozen_line(plain.clone(), Vec::new()),
            Vec::new(),
        ),
        ("styled-100k", frozen_line(plain, styles), inferred),
        ("cjk-99k", frozen_line(cjk, Vec::new()), Vec::new()),
        (
            "combining-99k",
            frozen_line(combining, Vec::new()),
            Vec::new(),
        ),
        (
            "eighty-columns",
            frozen_line("x".repeat(80), Vec::new()),
            Vec::new(),
        ),
    ]
}

struct Measured {
    frame_nanos: Vec<u64>,
    heap_bytes: u64,
    heap_allocations: u64,
    index_build_nanos: u64,
    checkpoints: usize,
    columns: u32,
}

impl Measured {
    fn median_frame_nanos(&self) -> u64 {
        let mut sorted = self.frame_nanos.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    fn report(&self, name: &str) {
        let mut sorted = self.frame_nanos.clone();
        sorted.sort_unstable();
        eprintln!(
            "BT_HSCROLL_BENCH {name} columns={} frames={SCROLL_FRAMES} checkpoints={} \
             index_build_us={} frame_p50_ns={} frame_p90_ns={} frame_max_ns={} \
             heap_bytes_per_frame={} heap_allocations_per_frame={}",
            self.columns,
            self.checkpoints,
            self.index_build_nanos / 1_000,
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 9 / 10],
            sorted[sorted.len() - 1],
            self.heap_bytes / SCROLL_FRAMES as u64,
            self.heap_allocations / SCROLL_FRAMES as u64,
        );
    }
}

fn scroll(line: &FrozenLine, links: &[InferredLink]) -> Measured {
    let mut store = HorizontalIndexStore::new(
        CHECKPOINT_STRIDE_COLUMNS,
        LINE_INDEX_BUDGET_BYTES,
        INDEX_STORE_BUDGET_BYTES,
    );
    let key = LineKey::History(line.id, line.source_generation);

    let started = Instant::now();
    let index = LineColumnIndex::build(
        &line.text,
        CHECKPOINT_STRIDE_COLUMNS,
        LINE_INDEX_BUDGET_BYTES,
    );
    let index_build_nanos = started.elapsed().as_nanos() as u64;
    let columns = index
        .as_ref()
        .map_or(0, |index| index.columns().0)
        .max(bt_unicode::text_width(&line.text) as u32);
    let checkpoints = index.as_ref().map_or(0, LineColumnIndex::checkpoint_count);

    // The store's own lazy build happens on the first non-zero seek and is not part of a frame.
    store.seek(key, &line.text, ContentColumn(1));

    let step = (columns / SCROLL_FRAMES as u32).max(1);
    let mut measured = Measured {
        // Reserved before the first frame: a `push` that grew mid-scroll would charge the scroll
        // for this test's own bookkeeping.
        frame_nanos: Vec::with_capacity(SCROLL_FRAMES),
        heap_bytes: 0,
        heap_allocations: 0,
        index_build_nanos,
        checkpoints,
        columns,
    };
    for frame in 0..SCROLL_FRAMES {
        let requested = ContentColumn(step.saturating_mul(frame as u32));
        let bytes_before = SCROLL_HEAP_BYTES.with(std::cell::Cell::get);
        let allocations_before = SCROLL_HEAP_ALLOCATIONS.with(std::cell::Cell::get);
        let started = Instant::now();
        let axis = HorizontalProjection::new(ContentColumn(columns), WINDOW_COLUMNS, requested);
        let from = store.seek(key, &line.text, axis.x_origin());
        let window = window_flattened_line(line, links, &axis, from);
        std::hint::black_box(window.cells.len());
        let elapsed = started.elapsed();
        measured.heap_bytes += SCROLL_HEAP_BYTES.with(std::cell::Cell::get) - bytes_before;
        measured.heap_allocations +=
            SCROLL_HEAP_ALLOCATIONS.with(std::cell::Cell::get) - allocations_before;
        measured.frame_nanos.push(elapsed.as_nanos() as u64);
    }
    measured
}

/// §1b, all three of its clauses at once: a scroll frame is O(window) and not O(line), the first
/// index build is bounded, and the median frame is inside 3 ms.
///
/// The complexity claim is the one worth the most here and it is checked without a clock: the
/// eighty-column line and the hundred-thousand-column one are asked for the same window, and a
/// frame of the long one may not draw more heap than a small constant over the short one. An
/// implementation that laid the whole line out and then sliced it — which is what
/// `layout_frozen_line` does today, and the reason this axis is being built at all — would fail
/// that comparison by three orders of magnitude while every wall clock still looked fine on a
/// quiet machine.
#[test]
fn a_horizontal_scroll_frame_costs_the_window_and_not_the_line() {
    let mut measurements = Vec::new();
    for (name, line, links) in corpus() {
        let measured = scroll(&line, &links);
        measured.report(name);
        measurements.push((name, measured));
    }

    let short = measurements
        .iter()
        .find(|(name, _)| *name == "eighty-columns")
        .map(|(_, measured)| measured.heap_bytes / SCROLL_FRAMES as u64)
        .expect("the eighty-column line is in the corpus");

    for (name, measured) in &measurements {
        let per_frame = measured.heap_bytes / SCROLL_FRAMES as u64;
        assert!(
            per_frame <= short.max(1) * 4,
            "{name}: a frame draws {per_frame} bytes against the eighty-column line's {short} — \
             a scroll frame must depend on the window, not on the line"
        );
        let median = Duration::from_nanos(measured.median_frame_nanos());
        assert!(
            median <= FRAME_CEILING,
            "{name}: the median scroll frame is {median:?}, ceiling {FRAME_CEILING:?}"
        );
        let build = Duration::from_nanos(measured.index_build_nanos);
        assert!(
            build <= INDEX_BUILD_CEILING,
            "{name}: the first index build is {build:?}, ceiling {INDEX_BUILD_CEILING:?}"
        );
    }
}

/// The other half of §1b's index clause: the sparse index is what makes the seek cheap, and the
/// linear fallback is what makes it correct without one.
///
/// Both numbers are reported. The assertion is only that the indexed seek is not *dearer*, because
/// how much it wins by is a property of the machine and the stride, not of the contract.
#[test]
fn an_indexed_seek_is_never_dearer_than_the_scan_it_replaces() {
    let text = "漢字仮名の混じった文章".repeat(9_000);
    let columns = bt_unicode::text_width(&text) as u32;
    let index = LineColumnIndex::build(&text, CHECKPOINT_STRIDE_COLUMNS, LINE_INDEX_BUDGET_BYTES)
        .expect("a hundred thousand columns at stride 64 fits the per-line budget");

    let targets = (0..SCROLL_FRAMES)
        .map(|frame| ContentColumn((columns / SCROLL_FRAMES as u32).max(1) * frame as u32))
        .collect::<Vec<_>>();

    let mut linear = Vec::with_capacity(targets.len());
    let mut indexed = Vec::with_capacity(targets.len());
    for target in &targets {
        let started = Instant::now();
        let scanned = seek_from_start(&text, *target);
        linear.push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        let sought = index.seek(&text, *target);
        indexed.push(started.elapsed().as_nanos() as u64);
        assert_eq!(scanned, sought, "at column {}", target.0);
    }
    linear.sort_unstable();
    indexed.sort_unstable();
    let linear_p50 = linear[linear.len() / 2];
    let indexed_p50 = indexed[indexed.len() / 2];
    eprintln!(
        "BT_HSCROLL_BENCH seek columns={columns} checkpoints={} index_bytes={} \
         linear_p50_ns={linear_p50} indexed_p50_ns={indexed_p50}",
        index.checkpoint_count(),
        index.resident_bytes(),
    );
    assert!(
        indexed_p50 <= linear_p50.max(1_000),
        "an indexed seek costs {indexed_p50} ns against the scan's {linear_p50} ns"
    );
}
