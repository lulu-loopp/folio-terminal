//! The window's minimum inner size (§2.6.5, tiny-window §4.2/§4.4).

use crate::demand::{demand, floor_demand};
use crate::geom::{Axis, LogicalSize};
use crate::metrics::SeatMetrics;
use crate::tree::{LayoutNode, SeatId};
use crate::{LogicalPx, MIN_WINDOW_CLAMP_PERCENT};

/// What is known about the monitor work area.
///
/// tiny-window §4.4, user ruling 6: a work-area query can fail briefly while
/// monitors are being switched or hot-plugged. Prefer the last value that
/// succeeded — the work area rarely changes between two queries, and reusing the
/// old number is far more honest than inventing one. Having never succeeded is a
/// different state, and it has a different answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkAreaHint {
    Known(LogicalSize),
    /// No query has ever succeeded.
    NeverKnown,
}

/// The minimum inner size to hand the OS, or `None` to set no hint at all.
///
/// ```text
/// min_inner = max(floor_demand(root) + chrome,
///                 min(demand(root) + chrome, 60% * workarea))
/// ```
///
/// The outer `min` is §2.6.5: a big tree would otherwise lock the window against
/// being dragged smaller, and "the window suddenly refuses to shrink" reads as a
/// freeze — worse than a collapsed bar. The outer `max` is tiny-window §4.2,
/// closing a gap the parent spec left: on a small monitor, 60% of the work area
/// can be *below* what the tree needs with everything collapsed, so the clamp
/// meant as a ceiling would push the tree into `Unsatisfiable` all by itself.
///
/// Red line L12: every input here is `demand`, `floor_demand`, chrome and the
/// work area. The current window size is deliberately not among them — deriving
/// a minimum from the current size builds a ratchet, where a window that grows
/// can never shrink again.
///
/// `None` means "set no minimum" and is returned only when no work area has ever
/// been observed (§4.4 ruling 2). Letting the OS use its own default beats
/// locking the user's window with a guess: with no trustworthy input, produce no
/// output that pretends to be trustworthy.
#[must_use]
pub fn window_min_inner_size(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    focus: SeatId,
    chrome: LogicalSize,
    work_area: WorkAreaHint,
) -> Option<LogicalSize> {
    let WorkAreaHint::Known(work_area) = work_area else {
        return None;
    };
    let axis_min = |axis: Axis| {
        let chrome = chrome.along(axis);
        let full = demand(tree, axis, metrics) + chrome;
        let floor = floor_demand(tree, axis, metrics, focus) + chrome;
        let clamp = percent(work_area.along(axis), MIN_WINDOW_CLAMP_PERCENT);
        full.min(clamp).max(floor)
    };
    Some(LogicalSize::new(axis_min(Axis::Row), axis_min(Axis::Col)))
}

fn percent(value: LogicalPx, percent: u32) -> LogicalPx {
    LogicalPx::from_subpixels((i128::from(value.subpixels()) * i128::from(percent) / 100) as i64)
}
