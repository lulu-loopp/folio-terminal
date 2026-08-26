//! Subtree demand, runs, shares, and the tree-distance total order.
//!
//! Everything here is a pure function of the tree — no geometry is an input, so
//! none of it can drift with the window size.

use crate::geom::Axis;
use crate::metrics::SeatMetrics;
use crate::tree::{LayoutNode, Ratio, Seat, SeatId, SeatKind, SplitId};
use crate::{COLLAPSED_EXTENT, DIVIDER, LogicalPx, RATIO_DENOM_PPM};

/// Fold a per-leaf valuation up the tree the way the allocator adds lengths:
/// summed along the split's own axis (plus its divider), maxed across it.
///
/// §2.2 (structural). Red line L3: every clamp — divider floors, drop
/// feasibility, window minimum — uses *this*, the subtree's demand, not a single
/// seat's minimum. An outer divider once crushed a two-pane group to 260px total
/// because the constraint was written per child while the content is per subtree.
pub(crate) fn fold_demand(
    node: &LayoutNode,
    axis: Axis,
    leaf: &impl Fn(&Seat) -> LogicalPx,
) -> LogicalPx {
    match node {
        LayoutNode::Seat(seat) => leaf(seat),
        LayoutNode::Split { dir, a, b, .. } => {
            let (da, db) = (fold_demand(a, axis, leaf), fold_demand(b, axis, leaf));
            if *dir == axis {
                da + db + DIVIDER
            } else {
                da.max(db)
            }
        }
    }
}

/// What a subtree needs along `axis` with every seat in its `Full` state.
///
/// A fixed seat asks for its own column width here, not merely its floor — that
/// width is what it is currently costing, and §2.6.5 sizes the window from it.
#[must_use]
pub fn demand(node: &LayoutNode, axis: Axis, metrics: &SeatMetrics) -> LogicalPx {
    fold_demand(node, axis, &|seat: &Seat| {
        seat.fixed_width(metrics, axis)
            .unwrap_or_else(|| metrics.min_size(seat.kind, axis))
    })
}

/// What a subtree needs along `axis` once every seat is at its own kind's
/// minimum — fixed columns squeezed to `FILES_W_MIN`, flex seats at their floor.
///
/// This is the bound the L1 and L2 concessions descend toward.
#[must_use]
pub fn demand_at_min(node: &LayoutNode, axis: Axis, metrics: &SeatMetrics) -> LogicalPx {
    fold_demand(node, axis, &|seat: &Seat| metrics.min_size(seat.kind, axis))
}

/// The absolute floor: the focus seat keeps its own kind's honest minimum and
/// every other seat contributes exactly one collapsed bar.
///
/// `M2-tiny-window-priority.md` §4.1. Same fold as [`demand`], one different
/// leaf valuation — it is not a new algorithm, and that is the point of naming it.
/// A single-seat tree has nothing to collapse, so its floor is just its own
/// minimum: a degenerate case, not an exception.
#[must_use]
pub fn floor_demand(
    node: &LayoutNode,
    axis: Axis,
    metrics: &SeatMetrics,
    focus: SeatId,
) -> LogicalPx {
    fold_demand(node, axis, &|seat: &Seat| {
        if seat.id == focus {
            metrics.min_size(seat.kind, axis)
        } else {
            COLLAPSED_EXTENT
        }
    })
}

/// The intrinsic fixed extent of a subtree along `axis`, if the whole subtree is
/// a fixed unit.
///
/// §2.3. A cross-direction stack of *only* fixed things is itself fixed, at its
/// widest member — drop that clause and two stacked files columns throw away
/// their fixed nature and the stack balloons to half the window, a bug the
/// prototype measured before the rule was written down.
#[must_use]
pub fn fixed_width(node: &LayoutNode, axis: Axis, metrics: &SeatMetrics) -> Option<LogicalPx> {
    match node {
        LayoutNode::Seat(seat) => seat.fixed_width(metrics, axis),
        LayoutNode::Split { dir, a, b, .. } => {
            let fa = fixed_width(a, axis, metrics)?;
            let fb = fixed_width(b, axis, metrics)?;
            Some(if *dir == axis {
                fa + fb + DIVIDER
            } else {
                fa.max(fb)
            })
        }
    }
}

/// How many columns a subtree needs along `axis`: one per seat, summed along the
/// axis and maxed across it.
///
/// §3.3. This is what "equal" was always supposed to mean — not that every node
/// is the same width, but that every *column* is. Balancing by headcount is what
/// gives a three-column row 539/269/269, a ratio nobody chose and the shape of
/// the data structure showing through.
#[must_use]
pub fn run_demand(node: &LayoutNode, axis: Axis) -> u64 {
    match node {
        LayoutNode::Seat(_) => 1,
        LayoutNode::Split { dir, a, b, .. } => {
            let (ra, rb) = (run_demand(a, axis), run_demand(b, axis));
            if *dir == axis { ra + rb } else { ra.max(rb) }
        }
    }
}

/// How many columns of a subtree take a *share* of the run along `axis`.
///
/// §3.3 read against §2.3: a fixed column takes pixels out of the run before
/// anything is divided, so it is not one of the parties the division is between.
/// Counting it as one is what put a files column's 240px into a ratio and then
/// charged that ratio to the pane beside it — three columns that were equal came
/// back 240/824/533 the moment a re-place changed which side of the tree the
/// fixed column hung on.
///
/// A wholly fixed subtree answers `0` whatever it holds, for the same reason
/// [`fixed_width`] folds one: it is a band of pixels, not a set of shares.
#[must_use]
pub fn flex_run_demand(node: &LayoutNode, axis: Axis, metrics: &SeatMetrics) -> u64 {
    if fixed_width(node, axis, metrics).is_some() {
        return 0;
    }
    match node {
        LayoutNode::Seat(_) => 1,
        LayoutNode::Split { dir, a, b, .. } => {
            let (ra, rb) = (
                flex_run_demand(a, axis, metrics),
                flex_run_demand(b, axis, metrics),
            );
            if *dir == axis { ra + rb } else { ra.max(rb) }
        }
    }
}

/// The unit a column's share of its run's flexible extent is measured in.
///
/// A million times finer than a [`Ratio`] on purpose. The share vector is read
/// out of the ratios and written straight back into them by a move, and at ppm
/// that round trip loses a part per million at every split it passes. At `1e18`
/// the trip is exact for the first three levels of a run and never off by more
/// than a part per million below that — 0.004px across a 4K width, which is two
/// orders of magnitude under the device pixel the boundaries snap to anyway.
pub(crate) const SHARE_DENOM: u128 = 1_000_000_000_000_000_000;

/// The columns of the run rooted here, in order.
///
/// One entry per *column*: a cross-direction subtree is a single entry however
/// many seats it holds, which is the same reading of "run" [`run_demand`] and
/// [`run_split_ids`] take.
pub(crate) fn run_columns(node: &LayoutNode, axis: Axis) -> Vec<&LayoutNode> {
    fn go<'a>(node: &'a LayoutNode, axis: Axis, out: &mut Vec<&'a LayoutNode>) {
        match node {
            LayoutNode::Split { dir, a, b, .. } if *dir == axis => {
                go(a, axis, out);
                go(b, axis, out);
            }
            _ => out.push(node),
        }
    }
    let mut out = Vec::new();
    go(node, axis, &mut out);
    out
}

/// The name a column answers to in a share table: its first seat in in-order.
///
/// Total by construction — every subtree has a first leaf — and stable across a
/// move, which re-arranges whole columns and never what is inside one.
pub(crate) fn column_key(node: &LayoutNode) -> SeatId {
    match node {
        LayoutNode::Seat(seat) => seat.id,
        LayoutNode::Split { a, .. } => column_key(a),
    }
}

/// Every column's share of the run's flexible extent, keyed by [`column_key`].
///
/// This is what a run's ratios *mean* once the allocator has taken the fixed
/// columns and the dividers out (§2.3): a column holding no flex seat has no
/// share at all, and the rest sum to [`SHARE_DENOM`] exactly — `b` is given the
/// remainder rather than its own rounded product, so no run leaks a subpixel.
pub(crate) fn column_shares(
    run_root: &LayoutNode,
    axis: Axis,
    metrics: &SeatMetrics,
) -> Vec<(SeatId, u128)> {
    fn go(
        node: &LayoutNode,
        axis: Axis,
        metrics: &SeatMetrics,
        share: u128,
        out: &mut Vec<(SeatId, u128)>,
    ) {
        if let LayoutNode::Split {
            dir, ratio, a, b, ..
        } = node
            && *dir == axis
        {
            let flex_a = fixed_width(a, axis, metrics).is_none();
            let flex_b = fixed_width(b, axis, metrics).is_none();
            let sa = match (flex_a, flex_b) {
                (true, true) => share * u128::from(ratio.ppm()) / u128::from(RATIO_DENOM_PPM),
                // One side holds every share there is to hold. When neither
                // does, `share` is already zero — a split of two fixed sides is
                // a fixed subtree, and its parent handed it nothing.
                (true, false) => share,
                (false, _) => 0,
            };
            go(a, axis, metrics, sa, out);
            go(b, axis, metrics, share - sa, out);
            return;
        }
        out.push((column_key(node), share));
    }
    let total = if fixed_width(run_root, axis, metrics).is_none() {
        SHARE_DENOM
    } else {
        0
    };
    let mut out = Vec::new();
    go(run_root, axis, metrics, total, &mut out);
    out
}

/// One column's entry in a share table, or zero when it holds no share.
pub(crate) fn share_of(shares: &[(SeatId, u128)], key: SeatId) -> u128 {
    shares
        .iter()
        .find(|(id, _)| *id == key)
        .map_or(0, |(_, share)| *share)
}

/// What a subtree is worth in a share table: the sum over the columns it holds.
pub(crate) fn weight_of(node: &LayoutNode, axis: Axis, shares: &[(SeatId, u128)]) -> u128 {
    run_columns(node, axis)
        .into_iter()
        .map(|column| share_of(shares, column_key(column)))
        .sum()
}

/// Which child of a split.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    A,
    B,
}

/// The path from the root to a node, as the sequence of turns taken.
pub type Path = Vec<Side>;

/// The path to the seat with this id.
#[must_use]
pub fn path_to_seat(root: &LayoutNode, id: SeatId) -> Option<Path> {
    fn go(node: &LayoutNode, id: SeatId, acc: &mut Path) -> bool {
        match node {
            LayoutNode::Seat(s) => s.id == id,
            LayoutNode::Split { a, b, .. } => {
                acc.push(Side::A);
                if go(a, id, acc) {
                    return true;
                }
                acc.pop();
                acc.push(Side::B);
                if go(b, id, acc) {
                    return true;
                }
                acc.pop();
                false
            }
        }
    }
    let mut acc = Path::new();
    go(root, id, &mut acc).then_some(acc)
}

/// The node at a path.
#[must_use]
pub fn node_at<'a>(root: &'a LayoutNode, path: &[Side]) -> Option<&'a LayoutNode> {
    let mut cur = root;
    for step in path {
        match cur {
            LayoutNode::Split { a, b, .. } => {
                cur = match step {
                    Side::A => a,
                    Side::B => b,
                };
            }
            LayoutNode::Seat(_) => return None,
        }
    }
    Some(cur)
}

pub(crate) fn node_at_mut<'a>(
    root: &'a mut LayoutNode,
    path: &[Side],
) -> Option<&'a mut LayoutNode> {
    let mut cur = root;
    for step in path {
        match cur {
            LayoutNode::Split { a, b, .. } => {
                cur = match step {
                    Side::A => a,
                    Side::B => b,
                };
            }
            LayoutNode::Seat(_) => return None,
        }
    }
    Some(cur)
}

/// The root of the run that the node at `path` belongs to, along `axis`.
///
/// §3.3 (structural): walk up while the parent split runs the same way, and stop
/// at the first cross-direction split. A run is the maximal transitive closure of
/// same-direction splits — it is not a node, which is exactly why the tree can
/// stay binary while a row still behaves like a row of equals.
#[must_use]
pub fn run_root_path(root: &LayoutNode, path: &[Side], axis: Axis) -> Path {
    let mut i = path.len();
    while i > 0 {
        let parent = match node_at(root, &path[..i - 1]) {
            Some(p) => p,
            None => break,
        };
        match parent {
            LayoutNode::Split { dir, .. } if *dir == axis => i -= 1,
            _ => break,
        }
    }
    path[..i].to_vec()
}

/// The run that `seat` belongs to along `axis`, as the path to its root.
#[must_use]
pub fn run_root_path_of_seat(root: &LayoutNode, seat: SeatId, axis: Axis) -> Option<Path> {
    let path = path_to_seat(root, seat)?;
    Some(run_root_path(root, &path, axis))
}

/// The seats juxtaposed along `axis` inside the run rooted here — one entry per
/// column, so a cross-direction subtree contributes the seats it holds but
/// occupies a single column.
#[must_use]
pub fn members(root: &LayoutNode, seat: SeatId, axis: Axis) -> Vec<SeatId> {
    let Some(run_root) = run_root_path_of_seat(root, seat, axis) else {
        return Vec::new();
    };
    match node_at(root, &run_root) {
        Some(node) => node.seats_in_order().into_iter().map(|s| s.id).collect(),
        None => Vec::new(),
    }
}

/// The ids of the splits whose ratios belong to the run rooted at `path`.
///
/// This is `F(E)` made concrete for every edit whose focus set is "a run": the
/// same-direction splits reachable from the run's root without crossing a
/// cross-direction split.
#[must_use]
pub fn run_split_ids(root: &LayoutNode, path: &[Side], axis: Axis) -> Vec<SplitId> {
    fn go(node: &LayoutNode, axis: Axis, out: &mut Vec<SplitId>) {
        if let LayoutNode::Split { id, dir, a, b, .. } = node
            && *dir == axis
        {
            out.push(*id);
            go(a, axis, out);
            go(b, axis, out);
        }
    }
    let mut out = Vec::new();
    if let Some(node) = node_at(root, path) {
        go(node, axis, &mut out);
    }
    out.sort_unstable();
    out
}

/// A seat's share of the flexible extent: the product of the side shares along
/// the path from the root, in parts per million.
///
/// §3.3 — the single numeric carrier of layout intent. Fixed columns take pixels
/// rather than shares, so this describes a flex seat's slice of what is left.
#[must_use]
pub fn share_ppm(root: &LayoutNode, seat: SeatId) -> Option<u64> {
    let path = path_to_seat(root, seat)?;
    let mut share: u128 = u128::from(RATIO_DENOM_PPM);
    let mut cur = root;
    for step in &path {
        let LayoutNode::Split { ratio, a, b, .. } = cur else {
            return None;
        };
        let side = match step {
            Side::A => u128::from(ratio.ppm()),
            Side::B => u128::from(RATIO_DENOM_PPM - ratio.ppm()),
        };
        share = share * side / u128::from(RATIO_DENOM_PPM);
        cur = match step {
            Side::A => a,
            Side::B => b,
        };
    }
    Some(share as u64)
}

/// In-order position of each seat.
#[must_use]
pub fn in_order_index(root: &LayoutNode, seat: SeatId) -> Option<usize> {
    root.seats_in_order().iter().position(|s| s.id == seat)
}

/// Edges between two seats outside their common prefix.
///
/// §2.6.1: this is what "farthest from the focus" means, and it is a pure tree
/// function — which is why the row chain and the column chain collapse seats in
/// the same order and merely stop at different points along it.
#[must_use]
pub fn tree_distance(root: &LayoutNode, x: SeatId, y: SeatId) -> Option<usize> {
    let (px, py) = (path_to_seat(root, x)?, path_to_seat(root, y)?);
    let common = px.iter().zip(py.iter()).take_while(|(a, b)| a == b).count();
    Some((px.len() - common) + (py.len() - common))
}

/// Which content class gives way first (§2.6.1 L3, **用户裁决 2026-08-13**).
///
/// Distance alone used to decide the whole order, and the real machine showed
/// what that costs: a window too narrow for its tree folded the *terminal* —
/// the one seat in the house running somebody's work — because it happened to
/// be the seat farthest from a preview that held the focus. A collapsed bar is
/// reversible and honest, but "which pane becomes a bar" is a question about
/// what the user would rather stop seeing, and that is not a question about
/// tree geometry.
///
/// So the class leads and the distance follows: **preview first, files next,
/// the terminal last**. A preview is a quick look at a document that is still
/// on disk; a files column is a way back to it; a terminal holds output no
/// other surface can reproduce. Within one class the old rule stands unchanged
/// — farthest from the focus gives way first.
///
/// [`SeatKind::Placeholder`] ranks ahead of all three, and that rank is
/// reasoning rather than ruling: a leaf whose kind this build does not
/// recognise is drawing nothing anybody can work in, so it is the cheapest
/// thing in the tree to turn into a bar. Written as four arms rather than a
/// `_` so a fifth kind has to answer this question on the way in.
fn collapse_rank(kind: SeatKind) -> u8 {
    match kind {
        SeatKind::Placeholder => 0,
        SeatKind::Preview => 1,
        SeatKind::Files => 2,
        SeatKind::Terminal => 3,
    }
}

/// Non-focus seats in the order they give way: by content class first
/// ([`collapse_rank`]), then farthest-from-focus by tree distance, then by
/// in-order position, and no further — seat ids are unique, so this is a total
/// order and the L3 concession is therefore deterministic.
#[must_use]
pub fn collapse_order(root: &LayoutNode, focus: SeatId) -> Vec<SeatId> {
    let mut ranked: Vec<(u8, usize, usize, SeatId)> = root
        .seats_in_order()
        .iter()
        .enumerate()
        .filter(|(_, s)| s.id != focus)
        .map(|(i, s)| {
            (
                collapse_rank(s.kind),
                tree_distance(root, focus, s.id).unwrap_or(0),
                i,
                s.id,
            )
        })
        .collect();
    ranked.sort_unstable_by(|l, r| l.0.cmp(&r.0).then(r.1.cmp(&l.1)).then(l.2.cmp(&r.2)));
    ranked.into_iter().map(|(_, _, _, id)| id).collect()
}

/// The ratio a split in a balanced run takes: its two sides' *share-holding*
/// column counts ([`flex_run_demand`]).
///
/// When one side holds no share the ratio is not consulted by the allocator at
/// all — a fixed band spends pixels — so the structural column count is written
/// there instead of the degenerate `0:n`. It is a number nobody reads today and
/// a sane one for the day a centre swap turns that band into a flex pane.
pub(crate) fn balanced_ratio(
    a: &LayoutNode,
    b: &LayoutNode,
    axis: Axis,
    metrics: &SeatMetrics,
) -> Ratio {
    let (fa, fb) = (
        flex_run_demand(a, axis, metrics),
        flex_run_demand(b, axis, metrics),
    );
    if fa == 0 || fb == 0 {
        return Ratio::from_parts(
            u128::from(run_demand(a, axis)),
            u128::from(run_demand(b, axis)),
        );
    }
    Ratio::from_parts(u128::from(fa), u128::from(fb))
}
