//! The M2 layout solver: hand it a tree and a rectangle, get seat rectangles.
//!
//! Formal specification: `docs/M2-layout-solver-spec.md` (the constraint system,
//! red lines L1-L13, determinism D1-D5, the edit focus set and theorem N) and
//! `docs/M2-tiny-window-priority.md` (the vertical chain, the dual-axis
//! concession, `floor_demand`, and the §4.2 lower clamp on the window minimum).
//!
//! # What this crate does not do
//!
//! It owns the *outer* geometry only: it divides one available rectangle among
//! seats and reports each seat's rectangle. Rows and columns inside a seat,
//! scroll anchors, height trees and projection invalidation belong to
//! `bt-viewport`. Content identity — which session, which buffer, which files
//! root — belongs to the content (red line L1): moving content house produces no
//! layout event, and a layout change produces no content event.
//!
//! Red line L7 keeps this crate free of `bt-viewport`, `bt-doc`, `bt-term` and
//! `bt-render`, and in fact free of every dependency: the solver answers what
//! shape a tree unfolds into, never what is drawn inside that shape.
//!
//! # The shape of an answer
//!
//! [`solve`] is pure — no IO, no clock, no randomness, no global state, no cache
//! (§3.1) — and every number in it is an integer (D3), because D1 asks for
//! bit-identical output and a float multiply can differ by one ULP between two
//! code paths that are meant to be the same picture.
//!
//! ```
//! use bt_layout::{Axis, LayoutMode, LayoutNode, LogicalRect, Seat, SeatId, SeatKind,
//!                 SeatMetrics, SplitId, solve};
//!
//! let tree = LayoutNode::split(
//!     SplitId(1),
//!     Axis::Row,
//!     LayoutNode::seat(Seat::new(SeatId(1), SeatKind::Files)),
//!     LayoutNode::seat(Seat::new(SeatId(2), SeatKind::Terminal)),
//! );
//! let metrics = SeatMetrics::ruled_at_unit_scale();
//! let layout = solve(
//!     &tree,
//!     LogicalRect::from_px(1200, 800),
//!     &metrics,
//!     SeatId(2),
//!     LayoutMode::Parallel,
//! )
//! .expect("1200x800 fits a files column and a terminal");
//!
//! // The files column takes pixels; the terminal takes what is left.
//! let files = layout.get(SeatId(1)).unwrap().rect.unwrap();
//! assert_eq!(files.extent(Axis::Row).floor_px(), 240);
//! ```

mod demand;
mod edit;
mod geom;
mod metrics;
mod solve;
mod tree;
mod window;

pub use demand::{
    Path, Side, collapse_order, demand, demand_at_min, fixed_width, floor_demand, in_order_index,
    members, node_at, path_to_seat, run_demand, run_root_path, run_root_path_of_seat,
    run_split_ids, share_ppm, tree_distance,
};
pub use edit::{Edit, EditError, EditOutcome, FocusSet, apply, necessity_holds, path_to_split};
pub use geom::{Axis, AxisSet, DeviceRect, LogicalPx, LogicalRect, LogicalSize, SUBPIXELS_PER_PX};
pub use metrics::{KindMetrics, SeatMetrics};
pub use solve::{LayoutError, LayoutMode, Presentation, SeatLayout, SeatPlacement, solve};
pub use tree::{ExtentClass, LayoutNode, Ratio, Seat, SeatId, SeatKind, SplitId};
pub use window::{WorkAreaHint, window_min_inner_size};

// The constants of §2.1 and §1.4. Every one of them is a *policy* position:
// overturning any of these numbers edits this block and nothing else. The
// structural rulings — a binary tree, dividers taking real space in the
// allocation, fixed only on the row axis, purity and determinism — are not
// numbers and cannot be overturned by editing a table.

/// A terminal's minimum width: one screen of readable command output.
pub const MIN_PANE_W: LogicalPx = LogicalPx::px(260);
/// Every seat's minimum height. §2.1's one row that does not fork by kind.
pub const MIN_PANE_H: LogicalPx = LogicalPx::px(120);
/// A files column's opening width.
pub const FILES_W: LogicalPx = LogicalPx::px(240);
/// A files column's floor: one column of filenames.
pub const FILES_W_MIN: LogicalPx = LogicalPx::px(170);
/// A preview's minimum width: one line of code that does not wrap.
///
/// Deliberately *not* the terminal's 260 (user ruling 1 of 2026-08-03): every
/// line of a 260px code preview wraps, and reading is the whole value of a
/// preview. Sizing by what was opened — code or an image — would drag content
/// into the solver and break red line L1.
pub const MIN_PREVIEW_W: LogicalPx = LogicalPx::px(360);
/// The space a divider occupies in the allocation.
///
/// Structural: the divider holds a real place. Its 1px look and 7px hit target
/// are chrome policy and live elsewhere.
pub const DIVIDER: LogicalPx = LogicalPx::px(1);
/// A collapsed seat's extent along the axis it was squeezed on (§2.6.3).
///
/// A real rectangle with real area, so red line L4 stands: zero area was never
/// how a collapse is expressed. A zero-width invisible seat is indistinguishable
/// on screen from a seat that was silently destroyed, and this UI does not have
/// invisible states.
pub const COLLAPSED_EXTENT: LogicalPx = LogicalPx::px(24);
/// The share of the work area the window minimum may not exceed (§2.6.5).
pub const MIN_WINDOW_CLAMP_PERCENT: u32 = 60;

/// The denominator of a [`Ratio`]: parts per million.
pub const RATIO_DENOM_PPM: u32 = 1_000_000;
/// The smallest and, mirrored, the largest legal ratio.
///
/// Both endpoints of `(0, 1)` are excluded by the type: zero extent is something
/// a close or a collapse says, never something a ratio says (red line L4).
pub const MIN_RATIO_PPM: u32 = 1;
