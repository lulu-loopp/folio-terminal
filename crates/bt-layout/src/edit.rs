//! Structural edits and the focus set `F(E)` that bounds each one.
//!
//! §3.3 is the heart of this module: "do not move unrelated seats" is only a
//! wish until it is formalised. `F(E)` is the *only* range of ratios an edit may
//! rewrite, and theorem N — every split outside `F(E)` keeps a bit-identical
//! ratio — is what turns the wish into an assertion, and an assertion into a gate.

use crate::demand::{
    Path, Side, balanced_ratio, demand, fixed_width, node_at, node_at_mut, path_to_seat,
    run_root_path, run_split_ids,
};
use crate::geom::Axis;
use crate::metrics::SeatMetrics;
use crate::tree::{LayoutNode, Ratio, Seat, SeatId, SeatKind, SplitId};
use crate::{LogicalPx, RATIO_DENOM_PPM};

/// The set of splits an edit is allowed to rewrite.
///
/// Empty is a real and common answer — a centre swap, a DPI change, entering
/// focus mode and opening a floating surface all rewrite nothing at all.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FocusSet {
    splits: Vec<SplitId>,
}

impl FocusSet {
    /// `∅`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A focus set of exactly these splits.
    #[must_use]
    pub fn of(mut splits: Vec<SplitId>) -> Self {
        splits.sort_unstable();
        splits.dedup();
        Self { splits }
    }

    /// Whether this split may be rewritten.
    #[must_use]
    pub fn contains(&self, id: SplitId) -> bool {
        self.splits.binary_search(&id).is_ok()
    }

    /// The splits, ascending.
    #[must_use]
    pub fn splits(&self) -> &[SplitId] {
        &self.splits
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.splits.is_empty()
    }
}

/// A structural edit.
///
/// Every variant here is a row of the §3.3 table, including the rows whose focus
/// set is empty. They are represented rather than omitted on purpose: a table
/// with invisible rows is a table someone will forget to read, and the empty
/// rows are exactly the ones a red gate wants to point at.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Edit {
    /// Split a seat at one of its edges, putting `new_seat` on the leading or
    /// trailing side. `F` = the run the target seat is in, along `dir`.
    SplitSeat {
        target: SeatId,
        dir: Axis,
        leading: bool,
        new_seat: Seat,
        split_id: SplitId,
    },
    /// Close a seat; its sibling is promoted. `F` = the run the promoted subtree
    /// is left in.
    ///
    /// Leaving a run re-divides it for the same reason joining one does: a rule
    /// that only balances on the way in exists only half. Delete the middle of
    /// three and the survivors would otherwise come out 1/3 and 2/3, still
    /// holding the shares of a set they are no longer in.
    CloseSeat { target: SeatId },
    /// A drop on the window's own rim: split the whole tree. `F` = the root run.
    RootRimDrop {
        dir: Axis,
        leading: bool,
        new_seat: Seat,
        split_id: SplitId,
    },
    /// Land the preview seat at its fixed address (§1.3, three tiers).
    LandPreview { seat: Seat, split_id: SplitId },
    /// Drag one divider. `F` = exactly that one split (§3.4).
    DragDivider {
        split: SplitId,
        requested: Ratio,
        /// The extent the two sides share — the slot minus its divider.
        usable: LogicalPx,
    },
    /// Drag a fixed column's width in pixels. Rewrites no ratio at all.
    DragFixedExtent {
        split: SplitId,
        requested: LogicalPx,
        usable: LogicalPx,
    },
    /// Take another seat's place. `F` = `∅` — taking a seat moves no boxes.
    CenterSwap { a: SeatId, b: SeatId },
    /// A scale-factor change. `F` = `∅` (red line L5).
    ///
    /// DPI is a similarity transform: ratios are dimensionless and both fixed
    /// extents and minima are logical pixels, so the same tree is simply solved
    /// again on a new rectangle. Satisfiability may change with DPI — that is
    /// what the concession chain is for, and conceding is a downgrade of
    /// *presentation*, never a loss of *intent*.
    DpiChanged,
    /// A window resize. `F` = `∅`.
    WindowResized,
    /// The monitor work area changed. `F` = `∅` (§2.6.5).
    WorkAreaChanged,
    /// Entering or leaving focus mode. `F` = `∅` (red line L13).
    FocusModeToggled,
    /// Opening, moving or closing a floating surface. `F` = `∅`.
    ///
    /// Floating surfaces are not seats and are not in the tree, so they take no
    /// part in any concession level; the arbitration happens outside the solver,
    /// which receives the *result* as its viewport rather than the inputs
    /// (tiny-window §3.1).
    FloatingSurfaceChanged,
}

/// The tree an edit produced, and the ratios it was allowed to touch.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditOutcome {
    pub tree: LayoutNode,
    pub focus_set: FocusSet,
}

/// Why an edit did not happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditError {
    /// The gesture asked for something the constraints already forbid.
    ///
    /// §2.4: feasibility is judged *before* the clamp, and an infeasible drag is
    /// refused with zero side effects. Clamping first and judging afterwards
    /// writes an unsatisfiable value and lets the next solve "correct" it, which
    /// dresses a refusal up as a jitter.
    Refused,
    UnknownSeat(SeatId),
    UnknownSplit(SplitId),
}

/// Apply an edit, returning the new tree and the focus set that bounded it.
///
/// The necessity theorem is enforced by construction: the only writer of ratios
/// below is [`rebalance_run`], and it is only ever handed the run that `F(E)`
/// names. [`necessity_holds`] states the same thing as a checkable predicate.
pub fn apply(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    edit: &Edit,
) -> Result<EditOutcome, EditError> {
    let outcome = apply_inner(tree, metrics, edit)?;
    debug_assert!(
        necessity_holds(tree, &outcome.tree, &outcome.focus_set),
        "theorem N: an edit rewrote a ratio outside its focus set"
    );
    Ok(outcome)
}

fn apply_inner(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    edit: &Edit,
) -> Result<EditOutcome, EditError> {
    match edit {
        Edit::DpiChanged
        | Edit::WindowResized
        | Edit::WorkAreaChanged
        | Edit::FocusModeToggled
        | Edit::FloatingSurfaceChanged => Ok(EditOutcome {
            tree: tree.clone(),
            focus_set: FocusSet::empty(),
        }),

        Edit::CenterSwap { a, b } => center_swap(tree, *a, *b),

        Edit::SplitSeat {
            target,
            dir,
            leading,
            new_seat,
            split_id,
        } => {
            let path = path_to_seat(tree, *target).ok_or(EditError::UnknownSeat(*target))?;
            let mut next = tree.clone();
            insert_split_at(
                &mut next,
                &path,
                *dir,
                *leading,
                new_seat.clone(),
                *split_id,
            );
            Ok(balance_run_containing(next, &path, *dir))
        }

        Edit::RootRimDrop {
            dir,
            leading,
            new_seat,
            split_id,
        } => {
            let mut next = tree.clone();
            insert_split_at(&mut next, &[], *dir, *leading, new_seat.clone(), *split_id);
            Ok(balance_run_containing(next, &[], *dir))
        }

        Edit::CloseSeat { target } => close_seat(tree, *target),

        Edit::LandPreview { seat, split_id } => land_preview(tree, metrics, seat, *split_id),

        Edit::DragDivider {
            split,
            requested,
            usable,
        } => drag_divider(tree, metrics, *split, *requested, *usable),

        Edit::DragFixedExtent {
            split,
            requested,
            usable,
        } => drag_fixed_extent(tree, metrics, *split, *requested, *usable),
    }
}

/// Replace the subtree at `path` with a split holding it and `new_seat`.
fn insert_split_at(
    tree: &mut LayoutNode,
    path: &[Side],
    dir: Axis,
    leading: bool,
    new_seat: Seat,
    split_id: SplitId,
) {
    // The path came from this very tree, so the slot is there.
    if let Some(slot) = node_at_mut(tree, path) {
        let existing = slot.clone();
        let arriving = LayoutNode::seat(new_seat);
        *slot = if leading {
            LayoutNode::split(split_id, dir, arriving, existing)
        } else {
            LayoutNode::split(split_id, dir, existing, arriving)
        };
    }
}

/// Re-divide the run that the node at `path` now belongs to.
fn balance_run_containing(mut tree: LayoutNode, path: &[Side], dir: Axis) -> EditOutcome {
    let run_root = run_root_path(&tree, path, dir);
    let focus_set = FocusSet::of(run_split_ids(&tree, &run_root, dir));
    if let Some(node) = node_at_mut(&mut tree, &run_root) {
        rebalance_run(node, dir);
    }
    EditOutcome { tree, focus_set }
}

/// Divide a run by column count, not by headcount of nodes.
///
/// §3.3 (structural, i3/Zellij semantics). Halving whatever pane you dropped on
/// is what a binary tree does, not what anyone asked for: three columns come out
/// 539/269/269, and a fourth gives 539/269/134/134 — below the minimum, so the
/// geometry runs the layout out of room and refuses a split that would have fit
/// perfectly at four equal columns.
///
/// The cost, stated plainly: a ratio dragged by hand inside this run is
/// re-decided when a seat joins or leaves it, because the run is no longer the
/// set of seats it was.
///
/// Recursion stops at the first cross-direction split — that subtree is one
/// column of this run, and its interior belongs to a different run.
fn rebalance_run(node: &mut LayoutNode, dir: Axis) {
    if let LayoutNode::Split {
        dir: node_dir,
        ratio,
        a,
        b,
        ..
    } = node
        && *node_dir == dir
    {
        *ratio = balanced_ratio(a, b, dir);
        rebalance_run(a, dir);
        rebalance_run(b, dir);
    }
}

fn center_swap(tree: &LayoutNode, a: SeatId, b: SeatId) -> Result<EditOutcome, EditError> {
    let pa = path_to_seat(tree, a).ok_or(EditError::UnknownSeat(a))?;
    let pb = path_to_seat(tree, b).ok_or(EditError::UnknownSeat(b))?;
    let (Some(LayoutNode::Seat(sa)), Some(LayoutNode::Seat(sb))) =
        (node_at(tree, &pa), node_at(tree, &pb))
    else {
        return Err(EditError::UnknownSeat(a));
    };
    let (sa, sb) = (sa.clone(), sb.clone());
    let mut next = tree.clone();
    if let Some(slot) = node_at_mut(&mut next, &pa) {
        *slot = LayoutNode::seat(sb);
    }
    if let Some(slot) = node_at_mut(&mut next, &pb) {
        *slot = LayoutNode::seat(sa);
    }
    Ok(EditOutcome {
        tree: next,
        focus_set: FocusSet::empty(),
    })
}

fn close_seat(tree: &LayoutNode, target: SeatId) -> Result<EditOutcome, EditError> {
    let path = path_to_seat(tree, target).ok_or(EditError::UnknownSeat(target))?;
    let Some((&last, parent_path)) = path.split_last().map(|(l, p)| (l, p.to_vec())) else {
        // The last pane closing is the tab closing (§7.1.4); an empty tree is
        // not a state this crate can represent, so the caller must not ask.
        return Err(EditError::Refused);
    };
    let Some(LayoutNode::Split { dir, a, b, .. }) = node_at(tree, &parent_path) else {
        return Err(EditError::UnknownSeat(target));
    };
    let dir = *dir;
    let promoted = match last {
        Side::A => (**b).clone(),
        Side::B => (**a).clone(),
    };
    let mut next = tree.clone();
    if let Some(slot) = node_at_mut(&mut next, &parent_path) {
        *slot = promoted;
    }
    Ok(balance_run_containing(next, &parent_path, dir))
}

/// Land the preview seat at its fixed address (§1.3, three tiers).
///
/// The address is a *place*, not a size: the preview is flex, and calling it
/// fixed-width would produce a 240px code preview, which is a thumbnail rather
/// than a preview.
///
/// The cost of the fixed address is stated openly in §3.3: `F` is the root run,
/// so opening a file narrows every terminal column proportionally. That is the
/// bill this ruling arrives with, not a defect — it buys "one singleton content
/// at one predictable address".
fn land_preview(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    seat: &Seat,
    split_id: SplitId,
) -> Result<EditOutcome, EditError> {
    // ① Reuse an unpinned preview seat: the content changes, the geometry does
    // not, so `F` is empty and not one rectangle moves.
    let reusable = tree
        .seats_in_order()
        .into_iter()
        .find(|s| s.kind == SeatKind::Preview && !s.pinned)
        .map(|s| s.id);
    if let Some(existing) = reusable {
        let mut next = tree.clone();
        if let Some(slot) = next.find_seat_mut(existing) {
            slot.pinned = seat.pinned;
        }
        return Ok(EditOutcome {
            tree: next,
            focus_set: FocusSet::empty(),
        });
    }

    // ② A new column at the tail of the root run — unless ③ that tail is a files
    // column, in which case the preview goes to its *left*. Files is navigation
    // and preview is content; pushing the content outboard of the navigation
    // would make "rightmost" an address that does not mean what it says.
    // A drag aimed explicitly at an edge still lands where it was aimed (§7.1.1);
    // this rule governs default creation only.
    let mut tail: Path = Vec::new();
    while let Some(LayoutNode::Split { dir: Axis::Row, .. }) = node_at(tree, &tail) {
        tail.push(Side::B);
    }
    let tail_is_files_column = node_at(tree, &tail)
        .and_then(|n| fixed_width(n, Axis::Row, metrics))
        .is_some();

    let mut next = tree.clone();
    insert_split_at(
        &mut next,
        &tail,
        Axis::Row,
        tail_is_files_column,
        seat.clone(),
        split_id,
    );
    Ok(balance_run_containing(next, &tail, Axis::Row))
}

/// Drag one divider (§2.4, §3.4).
///
/// Red line L9: a drag triggers no rebalance. "I want this ratio" is the whole
/// meaning of dragging by hand, and rebalancing means "these members changed" —
/// they did not.
fn drag_divider(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    split: SplitId,
    requested: Ratio,
    usable: LogicalPx,
) -> Result<EditOutcome, EditError> {
    let path = path_to_split(tree, split).ok_or(EditError::UnknownSplit(split))?;
    let Some(LayoutNode::Split { dir, a, b, .. }) = node_at(tree, &path) else {
        return Err(EditError::UnknownSplit(split));
    };
    if !usable.is_positive() {
        return Err(EditError::Refused);
    }
    // A fixed subtree takes pixels, not a share: writing a ratio onto a fixed
    // slot would corrupt the layout, so refuse honestly.
    if fixed_width(a, *dir, metrics).is_some() || fixed_width(b, *dir, metrics).is_some() {
        return Err(EditError::Refused);
    }
    // The clamp order is not commutative (§2.4): find both floors from subtree
    // demand, judge feasibility, and only then clamp.
    let lo = ceil_ppm(demand(a, *dir, metrics), usable).max(1);
    let hi = RATIO_DENOM_PPM
        .saturating_sub(ceil_ppm(demand(b, *dir, metrics), usable))
        .min(RATIO_DENOM_PPM - 1);
    if lo > hi {
        return Err(EditError::Refused);
    }
    let chosen = Ratio::clamped_from_ppm(requested.ppm().clamp(lo, hi));
    let mut next = tree.clone();
    if let Some(LayoutNode::Split { ratio, .. }) = node_at_mut(&mut next, &path) {
        *ratio = chosen;
    }
    Ok(EditOutcome {
        tree: next,
        focus_set: FocusSet::of(vec![split]),
    })
}

/// Drag a fixed column's width in pixels (§3.4).
fn drag_fixed_extent(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    split: SplitId,
    requested: LogicalPx,
    usable: LogicalPx,
) -> Result<EditOutcome, EditError> {
    let path = path_to_split(tree, split).ok_or(EditError::UnknownSplit(split))?;
    let Some(LayoutNode::Split { dir, a, b, .. }) = node_at(tree, &path) else {
        return Err(EditError::UnknownSplit(split));
    };
    if *dir != Axis::Row {
        return Err(EditError::Refused);
    }
    // Only a bare fixed leaf has a single width to drag. A fixed subtree that is
    // not one — two files panes stacked, say — is refused honestly rather than
    // written to (§3.4).
    let (side, other): (Side, &LayoutNode) = match (a.as_ref(), b.as_ref()) {
        (LayoutNode::Seat(s), _) if is_fixed_leaf(s, metrics) => (Side::A, b),
        (_, LayoutNode::Seat(s)) if is_fixed_leaf(s, metrics) => (Side::B, a),
        _ => return Err(EditError::Refused),
    };
    let floor = match node_at(tree, &path) {
        Some(LayoutNode::Split { a, b, .. }) => {
            let leaf = if side == Side::A { a } else { b };
            match leaf.as_ref() {
                LayoutNode::Seat(s) => metrics.min_size(s.kind, Axis::Row),
                LayoutNode::Split { .. } => return Err(EditError::Refused),
            }
        }
        _ => return Err(EditError::Refused),
    };
    let ceiling = usable - demand(other, Axis::Row, metrics);
    if ceiling < floor {
        return Err(EditError::Refused);
    }
    let width = requested.clamp_to(floor, ceiling);
    let mut next = tree.clone();
    let mut leaf_path = path.clone();
    leaf_path.push(side);
    if let Some(LayoutNode::Seat(s)) = node_at_mut(&mut next, &leaf_path) {
        s.fixed_extent = Some(width);
    }
    // A pixel drag rewrites no ratio at all.
    Ok(EditOutcome {
        tree: next,
        focus_set: FocusSet::empty(),
    })
}

fn is_fixed_leaf(seat: &Seat, metrics: &SeatMetrics) -> bool {
    metrics.extent_class(seat.kind).is_fixed_along(Axis::Row)
}

fn ceil_ppm(part: LogicalPx, whole: LogicalPx) -> u32 {
    let (p, w) = (i128::from(part.subpixels()), i128::from(whole.subpixels()));
    if w <= 0 {
        return RATIO_DENOM_PPM;
    }
    let v = (p * i128::from(RATIO_DENOM_PPM) + w - 1) / w;
    v.clamp(0, i128::from(RATIO_DENOM_PPM)) as u32
}

/// The path to a split by id.
#[must_use]
pub fn path_to_split(root: &LayoutNode, id: SplitId) -> Option<Path> {
    fn go(node: &LayoutNode, id: SplitId, acc: &mut Path) -> bool {
        match node {
            LayoutNode::Seat(_) => false,
            LayoutNode::Split { id: this, a, b, .. } => {
                if *this == id {
                    return true;
                }
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

/// Theorem N, as a predicate.
///
/// > For any split `p` not in `F(E)`, `ratio(p)` is bit-identical before and
/// > after `E`.
///
/// Splits that only one of the two trees has are not compared: a close removes a
/// split, and a split adds one. What the theorem constrains is the ratios that
/// *survive* the edit.
///
/// The theorem deliberately does not promise that seats outside the run keep
/// their pixels. Adding a column to the root run narrows every column in it —
/// that is geometry, not the solver meddling. What it promises is that layout
/// *intent* is never quietly rewritten.
#[must_use]
pub fn necessity_holds(before: &LayoutNode, after: &LayoutNode, focus_set: &FocusSet) -> bool {
    let (old, new) = (before.ratios(), after.ratios());
    old.iter().all(|(id, ratio)| {
        focus_set.contains(*id)
            || new
                .iter()
                .find(|(other, _)| other == id)
                .is_none_or(|(_, now)| now == ratio)
    })
}
