//! The window's minimum inner size (§2.6.5, revised by user ruling 2026-08-08).

use crate::geom::{Axis, LogicalSize};
use crate::metrics::SeatMetrics;
use crate::tree::SeatKind;
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

/// The minimum inner size to hand the OS: the technical floor, and nothing else.
///
/// ```text
/// min_inner = min(one_terminal_leaf + chrome, 60% * workarea)
/// ```
///
/// **User ruling 2026-08-08 — a minimum is law to the program and advice to the
/// user.** This function speaks to the OS on the user's behalf, so it states the
/// *advice* line's floor and not the law's: how small a window can be dragged is
/// the user's business, and a window that stops moving under the hand reads as a
/// freeze no matter how good the reason. What is left is a technical floor —
/// one terminal leaf's minimum, the smallest rectangle in which this program
/// still runs — and its whole job is to keep a 157x25 window from being a state
/// anyone can reach. Below one pane there is no arrangement of anything to show.
///
/// It is deliberately **not** a function of the tree. The layout's own needs
/// used to set this line, which meant a four-column tab locked the window at the
/// width of four columns and the user could not have a narrow window at all.
/// Those needs did not vanish; they moved to where they belong, into the
/// concession chain under [`SizePolicy::Lawful`](crate::SizePolicy) where the
/// *program's* own layouts are still held to them. Same numbers, different
/// authority.
///
/// The 60% clamp survives from §2.6.5 for the reason it was written: on a small
/// enough monitor even one pane plus chrome can be more than a window ought to
/// demand, and "the window refuses to shrink" is the failure this whole line
/// exists to avoid. It binds almost never now, which is the point — the old
/// formula needed it every day.
///
/// The tree-independence is also why there is no `None` any more. The old answer
/// needed a work area before it could trust itself, because it was clamping a
/// number that could otherwise lock the window; a constant needs nothing, and
/// "no minimum at all" was never a state worth reaching — it is exactly the
/// absurd rectangle the floor is here to refuse.
#[must_use]
pub fn window_min_inner_size(
    metrics: &SeatMetrics,
    chrome: LogicalSize,
    work_area: WorkAreaHint,
) -> LogicalSize {
    let axis_min = |axis: Axis| {
        let floor = metrics.min_size(SeatKind::Terminal, axis) + chrome.along(axis);
        match work_area {
            WorkAreaHint::Known(area) => {
                floor.min(percent(area.along(axis), MIN_WINDOW_CLAMP_PERCENT))
            }
            WorkAreaHint::NeverKnown => floor,
        }
    };
    LogicalSize::new(axis_min(Axis::Row), axis_min(Axis::Col))
}

fn percent(value: LogicalPx, percent: u32) -> LogicalPx {
    LogicalPx::from_subpixels((i128::from(value.subpixels()) * i128::from(percent) / 100) as i64)
}
