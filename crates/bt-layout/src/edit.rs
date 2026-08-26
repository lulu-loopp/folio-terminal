//! Structural edits and the focus set `F(E)` that bounds each one.
//!
//! §3.3 is the heart of this module: "do not move unrelated seats" is only a
//! wish until it is formalised. `F(E)` is the *only* range of ratios an edit may
//! rewrite, and theorem N — every split outside `F(E)` keeps a bit-identical
//! ratio — is what turns the wish into an assertion, and an assertion into a gate.

use crate::LogicalPx;
use crate::demand::{
    Path, Side, balanced_ratio, column_key, column_shares, fixed_width, flex_run_demand, node_at,
    node_at_mut, path_to_seat, run_columns, run_root_path, run_split_ids, share_of, weight_of,
};
use crate::geom::Axis;
use crate::metrics::SeatMetrics;
use crate::tree::{LayoutNode, Ratio, Seat, SeatId, SeatKind, SplitId};

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

/// Where a pane already in the tree lets go (§3.3, the move row).
///
/// The two shapes a drag can aim at along one axis: beside one pane, or beside
/// the whole tree. A centre is not among them — taking another pane's place is
/// [`Edit::CenterSwap`], which moves no boxes and settles no shares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Landing {
    /// On the named side of one pane.
    Edge {
        target: SeatId,
        dir: Axis,
        leading: bool,
    },
    /// On the named side of the whole tree.
    Rim { dir: Axis, leading: bool },
}

impl Landing {
    /// The axis the new split runs along.
    #[must_use]
    pub fn dir(self) -> Axis {
        match self {
            Landing::Edge { dir, .. } | Landing::Rim { dir, .. } => dir,
        }
    }

    /// Whether the arriving pane goes on the leading side of the new split.
    #[must_use]
    pub fn leading(self) -> bool {
        match self {
            Landing::Edge { leading, .. } | Landing::Rim { leading, .. } => leading,
        }
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
    /// Split a seat at one of its edges, putting `arriving` on the leading or
    /// trailing side. `F` = the run the target seat is in, along `dir`.
    ///
    /// `arriving` is a whole subtree rather than one seat, and it has to be:
    /// a tab dropped onto a pane brings its *entire* layout, and previewing or
    /// committing that as a single anonymous leaf gets the geometry wrong twice
    /// over. [`rebalance_run`] divides a run by column count — `run_demand` of
    /// the arriving subtree, not `1` — so a three-column tab joining a
    /// two-column run must be worth three columns at the moment the ratio is
    /// decided, and the fit judgement downstream has to see the real leaves
    /// rather than one box standing in for them.
    SplitSeat {
        target: SeatId,
        dir: Axis,
        leading: bool,
        arriving: LayoutNode,
        split_id: SplitId,
    },
    /// **Move a pane that is already in this tree to another place in it**, at
    /// the shares it and everybody else already had (用户裁决 2026-08-25).
    /// `F` = the run it leaves and the run it lands in.
    ///
    /// Emphatically **not** [`Edit::CloseSeat`] followed by [`Edit::SplitSeat`],
    /// which is what a drag used to be. Those two are the right edits for a pane
    /// that is *going away* and a pane that is *arriving*, and each of them
    /// re-divides its run by column count — the rule that says "these members
    /// changed". A re-place changes no members: the same panes are in the same
    /// run afterwards, in a different order. Spelling it as a leave and a join
    /// therefore ran the members-changed rule twice over a set that never
    /// changed, and the bill landed on panes nobody had touched — a hand-set
    /// 65:35 between two siblings came back 50:50, and a files column moved
    /// along its own row left the middle pane 145px wider than it found it.
    ///
    /// So the shares are carried instead of re-decided:
    ///
    /// * every column that did not move keeps its share of its run, which is
    ///   theorem N's promise stated in widths rather than in ratios;
    /// * the traveller keeps its **own** share when it comes back to the run it
    ///   left — a pane put back where it was is then a no-op to the pixel,
    ///   whichever of the shapes that place can be written as it lands in;
    /// * a traveller arriving in a *different* run has no share there yet and
    ///   buys one off whoever the landing rule says pays — the pane it landed
    ///   beside, by column count, or the whole run when it landed on a rim or
    ///   beside a fixed band that has no share to sell.
    MoveSeat {
        seat: SeatId,
        landing: Landing,
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
    ///
    /// `arriving` is a subtree for the same reason [`Edit::SplitSeat`]'s is.
    RootRimDrop {
        dir: Axis,
        leading: bool,
        arriving: LayoutNode,
        split_id: SplitId,
    },
    /// Put a subtree in a seat's place, leaving everything around it alone.
    /// `F` = `∅`.
    ///
    /// §3.3's `replaceLeafIn`: the row that says a leaf's slot can receive a
    /// whole layout without the run it sits in being re-divided. A centre drop
    /// from the tab strip is its one caller — the target leaves for the strip
    /// and the arriving tab takes the slot it vacated — and taking a slot moves
    /// no boxes, which is exactly what an empty focus set says.
    ///
    /// It is emphatically *not* [`Edit::CenterSwap`] with one side thrown away.
    /// A swap trades two payloads and both seats keep their places; this one
    /// substitutes, so the arriving side may be a whole tree and the leaving
    /// side goes somewhere this crate does not model.
    ReplaceSeat {
        target: SeatId,
        arriving: LayoutNode,
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
    /// The gesture asked for something that has no answer.
    ///
    /// Refused with zero side effects, never with a written-then-corrected
    /// value: writing something unsatisfiable and letting the next solve tidy it
    /// up dresses a refusal as a jitter (§2.4).
    ///
    /// What survives here after the 2026-08-08 ruling is only the questions with
    /// no answer at all — a ratio aimed at a fixed slot, a width aimed at a
    /// subtree that has no single width, the last seat in the tree closing.
    /// "That would be smaller than the minimum" is no longer among them: it is a
    /// question with a perfectly good answer, which is the smaller pane the user
    /// asked for.
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
            arriving,
            split_id,
        } => {
            let path = path_to_seat(tree, *target).ok_or(EditError::UnknownSeat(*target))?;
            let mut next = tree.clone();
            insert_split_at(
                &mut next,
                &path,
                *dir,
                *leading,
                arriving.clone(),
                *split_id,
            );
            Ok(balance_run_containing(next, metrics, &path, *dir))
        }

        Edit::RootRimDrop {
            dir,
            leading,
            arriving,
            split_id,
        } => {
            let mut next = tree.clone();
            insert_split_at(&mut next, &[], *dir, *leading, arriving.clone(), *split_id);
            Ok(balance_run_containing(next, metrics, &[], *dir))
        }

        Edit::ReplaceSeat { target, arriving } => {
            let path = path_to_seat(tree, *target).ok_or(EditError::UnknownSeat(*target))?;
            let mut next = tree.clone();
            // The path came from this very tree, so the slot is there.
            if let Some(slot) = node_at_mut(&mut next, &path) {
                *slot = arriving.clone();
            }
            Ok(EditOutcome {
                tree: next,
                focus_set: FocusSet::empty(),
            })
        }

        Edit::MoveSeat {
            seat,
            landing,
            split_id,
        } => move_seat(tree, metrics, *seat, *landing, *split_id),

        Edit::CloseSeat { target } => close_seat(tree, metrics, *target),

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

/// Replace the subtree at `path` with a split holding it and `arriving`.
fn insert_split_at(
    tree: &mut LayoutNode,
    path: &[Side],
    dir: Axis,
    leading: bool,
    arriving: LayoutNode,
    split_id: SplitId,
) {
    // The path came from this very tree, so the slot is there.
    if let Some(slot) = node_at_mut(tree, path) {
        let existing = slot.clone();
        *slot = if leading {
            LayoutNode::split(split_id, dir, arriving, existing)
        } else {
            LayoutNode::split(split_id, dir, existing, arriving)
        };
    }
}

/// Re-divide the run that the node at `path` now belongs to.
fn balance_run_containing(
    mut tree: LayoutNode,
    metrics: &SeatMetrics,
    path: &[Side],
    dir: Axis,
) -> EditOutcome {
    let run_root = run_root_path(&tree, path, dir);
    let focus_set = FocusSet::of(run_split_ids(&tree, &run_root, dir));
    if let Some(node) = node_at_mut(&mut tree, &run_root) {
        rebalance_run(node, metrics, dir);
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
fn rebalance_run(node: &mut LayoutNode, metrics: &SeatMetrics, dir: Axis) {
    if let LayoutNode::Split {
        dir: node_dir,
        ratio,
        a,
        b,
        ..
    } = node
        && *node_dir == dir
    {
        *ratio = balanced_ratio(a, b, dir, metrics);
        rebalance_run(a, metrics, dir);
        rebalance_run(b, metrics, dir);
    }
}

/// Write a run's ratios from a table of shares its columns are owed.
///
/// The counterpart of [`rebalance_run`] and the whole of what a move does with
/// arithmetic: equal columns is what "these members changed" means, and *these
/// shares* is what "the same members, re-arranged" means. Only the splits of
/// this run are written, so `F` says exactly what it did.
///
/// A side owed nothing takes no share, and the ratio there is not a number the
/// allocator will read — [`balanced_ratio`] leaves a structurally sane one on
/// the books rather than the degenerate `0:n` this would otherwise compute.
fn settle_run(node: &mut LayoutNode, metrics: &SeatMetrics, dir: Axis, shares: &[(SeatId, u128)]) {
    if let LayoutNode::Split {
        dir: node_dir,
        ratio,
        a,
        b,
        ..
    } = node
        && *node_dir == dir
    {
        let (wa, wb) = (weight_of(a, dir, shares), weight_of(b, dir, shares));
        *ratio = if wa == 0 || wb == 0 {
            balanced_ratio(a, b, dir, metrics)
        } else {
            Ratio::from_parts(wa, wb)
        };
        settle_run(a, metrics, dir, shares);
        settle_run(b, metrics, dir, shares);
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

/// A pane already in this tree, moving house (用户裁决 2026-08-25).
///
/// Two shapes, and which one it is turns on a single question: **is the pane
/// coming back to the run it is already in?**
///
/// * **Yes** — then nothing joins the run and nothing leaves it. The run's
///   columns are re-ordered in place: the skeleton keeps its splits, its ids
///   and its nesting, and only which column sits in which slot changes. The
///   shares are then written back exactly as they were, so every pane in the run
///   comes out at the width it went in at, the traveller included. Letting one
///   go in the gap it came out of is not even an edit — the same order is the
///   same layout, and it is answered with the tree untouched and `F` = `∅`.
/// * **No** — the pane is plucked out of one run and cut into another, and each
///   run settles on the shares it can account for: the one it left divides what
///   it had between the columns still in it, and the one it joins hands the
///   newcomer a share off whoever the landing rule says pays.
///
/// Neither shape is a *rebalance*, and that is the whole of the ruling. A run
/// re-divides itself by column count when its members change (§3.3); a move
/// changes no members, and spelling it as a close plus a split ran the
/// members-changed rule twice over a set that never changed. What that cost was
/// paid by panes nobody had touched: a hand-set 65:35 between two siblings came
/// back 50:50, and a files column re-placed along its own row left the middle
/// pane 145px wider than it found it.
fn move_seat(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    seat: SeatId,
    landing: Landing,
    split_id: SplitId,
) -> Result<EditOutcome, EditError> {
    let dir = landing.dir();
    let path = path_to_seat(tree, seat).ok_or(EditError::UnknownSeat(seat))?;
    let Some((&last, parent_path)) = path.split_last().map(|(l, p)| (l, p.to_vec())) else {
        // The only pane in the tree has no other place in it to be. Refused for
        // the reason G84 refuses the last close: there is no tree on the far
        // side of the edit.
        return Err(EditError::Refused);
    };
    let Some(LayoutNode::Split {
        dir: src_dir, a, b, ..
    }) = node_at(tree, &parent_path)
    else {
        return Err(EditError::UnknownSeat(seat));
    };
    let src_dir = *src_dir;
    let sibling = match last {
        Side::A => (**b).clone(),
        Side::B => (**a).clone(),
    };
    // The run it is in now, and the shares that run's columns hold today.
    let src_run_path = run_root_path(tree, &path, src_dir);
    let src_run = node_at(tree, &src_run_path).ok_or(EditError::UnknownSeat(seat))?;
    let src_shares = column_shares(src_run, src_dir, metrics);

    if dir == src_dir && stays_in_run(tree, &src_run_path, dir, seat, landing) {
        return reorder_run(
            tree,
            metrics,
            seat,
            landing,
            &src_run_path,
            dir,
            &src_shares,
        );
    }

    // Cloned before the pluck: everything durable about the pane — its kind, its
    // fixed extent, its pin — lives on the `Seat` (§5).
    let travelling = LayoutNode::seat(
        tree.find_seat(seat)
            .ok_or(EditError::UnknownSeat(seat))?
            .clone(),
    );

    // ① The pluck, and the run it leaves settled onto what its remaining columns
    //   already held. Promoting the sibling on its own is not enough: the space
    //   the traveller frees would go to whatever subtree happened to be beside
    //   it, so a run written `((a b) c)` would give `b` the whole of `a`'s width
    //   while `c`, which nobody touched, kept its own.
    let mut next = tree.clone();
    // The path came from this very tree, so the slot is there.
    if let Some(slot) = node_at_mut(&mut next, &parent_path) {
        *slot = sibling;
    }
    // Every path at or above the run's root is untouched by a pluck below it, so
    // the run is still rooted where it was.
    let survivors: Vec<(SeatId, u128)> = src_shares
        .iter()
        .copied()
        .filter(|(id, _)| *id != seat)
        .collect();
    if let Some(node) = node_at_mut(&mut next, &src_run_path) {
        settle_run(node, metrics, src_dir, &survivors);
    }
    let mut splits = run_split_ids(&next, &src_run_path, src_dir);

    // ② Where it lands, cut exactly where `SplitSeat` and `RootRimDrop` cut
    //   theirs, and what that run is worth without it.
    let anchor_path = match landing {
        Landing::Edge { target, .. } => {
            path_to_seat(&next, target).ok_or(EditError::UnknownSeat(target))?
        }
        Landing::Rim { .. } => Path::new(),
    };
    let dst_run_path = run_root_path(&next, &anchor_path, dir);
    let dst_run = node_at(&next, &dst_run_path).ok_or(EditError::Refused)?;
    let dst_shares = column_shares(dst_run, dir, metrics);
    let arriving = arriving_share(&travelling, landing, dir, metrics, dst_run, &dst_shares);

    let mut settled: Vec<(SeatId, u128)> = dst_shares
        .iter()
        .map(|&(id, share)| match arriving.paid_by {
            // The buyer's share comes out of the seller's and out of nobody
            // else's, which is what "settles with the pane it landed beside"
            // means in numbers.
            Some(payer) if payer == id => (id, share - arriving.share),
            _ => (id, share),
        })
        .collect();
    settled.push((column_key(&travelling), arriving.share));

    insert_split_at(
        &mut next,
        &anchor_path,
        dir,
        landing.leading(),
        travelling,
        split_id,
    );

    // ③ The settlement. The run's root is read back off the tree it is now in:
    //   an insert at the run's own root — every rim drop, and an edge drop on a
    //   lone column — pushes it down a level, and a path taken before the cut
    //   would name the wrong node.
    let arrived_path = path_to_seat(&next, seat).ok_or(EditError::UnknownSeat(seat))?;
    let dst_run_path = run_root_path(&next, &arrived_path, dir);
    if let Some(node) = node_at_mut(&mut next, &dst_run_path) {
        settle_run(node, metrics, dir, &settled);
    }
    splits.extend(run_split_ids(&next, &dst_run_path, dir));
    Ok(EditOutcome {
        tree: next,
        focus_set: FocusSet::of(splits),
    })
}

/// Whether this landing puts the pane back into the run it is already in.
///
/// A rim along the run's own axis is that run's own end exactly when the run is
/// the root one; an edge is, when the pane it names is a column of the same run.
/// Aiming at the traveller itself is neither: it names a gap that will not exist
/// once the pane is out of it.
fn stays_in_run(
    tree: &LayoutNode,
    run_path: &[Side],
    dir: Axis,
    seat: SeatId,
    landing: Landing,
) -> bool {
    match landing {
        Landing::Rim { .. } => run_path.is_empty(),
        Landing::Edge { target, .. } => {
            target != seat
                && path_to_seat(tree, target)
                    .is_some_and(|at| run_root_path(tree, &at, dir) == run_path)
        }
    }
}

/// A move that begins and ends in the same run: the columns re-ordered, the
/// skeleton and the shares left exactly as they were.
///
/// The run's shape carries no geometry of its own — the allocator takes the
/// fixed bands and the dividers out and then divides by share, so which way a
/// chain of same-direction splits leans is not something anybody can see. What
/// *is* visible is which column stands where and how wide each one is, and this
/// changes the first and preserves the second. Re-cutting the skeleton instead
/// would renumber the dividers under the user's hand, and would move a floor
/// that one neighbour was paying for quietly onto another.
fn reorder_run(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    seat: SeatId,
    landing: Landing,
    run_path: &[Side],
    dir: Axis,
    shares: &[(SeatId, u128)],
) -> Result<EditOutcome, EditError> {
    let run = node_at(tree, run_path).ok_or(EditError::UnknownSeat(seat))?;
    let mut order: Vec<LayoutNode> = run_columns(run, dir).into_iter().cloned().collect();
    let from = order
        .iter()
        .position(|column: &LayoutNode| column.contains(seat))
        .ok_or(EditError::UnknownSeat(seat))?;
    let travelling = order.remove(from);
    let to = match landing {
        Landing::Rim { leading, .. } => {
            if leading {
                0
            } else {
                order.len()
            }
        }
        Landing::Edge {
            target, leading, ..
        } => {
            let beside = order
                .iter()
                .position(|column: &LayoutNode| column.contains(target))
                .ok_or(EditError::UnknownSeat(target))?;
            if leading { beside } else { beside + 1 }
        }
    };
    if to == from {
        // It is already there, and "already there" is the second half of the
        // ruling entire: a pane let go in the gap it came out of is not an edit
        // at all, so not one number moves and `F` is empty.
        return Ok(EditOutcome {
            tree: tree.clone(),
            focus_set: FocusSet::empty(),
        });
    }
    order.insert(to, travelling);
    let mut next = tree.clone();
    // The path came from this very tree, so the slot is there.
    if let Some(node) = node_at_mut(&mut next, run_path) {
        reseat_columns(node, dir, &mut order.into_iter());
        settle_run(node, metrics, dir, shares);
    }
    let focus_set = FocusSet::of(run_split_ids(&next, run_path, dir));
    Ok(EditOutcome {
        tree: next,
        focus_set,
    })
}

/// Put a run's columns back into its own skeleton, in the order given.
///
/// The skeleton has exactly one slot per column and the caller hands back
/// exactly the columns it took out of it, so the two run out together.
fn reseat_columns(node: &mut LayoutNode, dir: Axis, order: &mut impl Iterator<Item = LayoutNode>) {
    match node {
        LayoutNode::Split {
            dir: node_dir,
            a,
            b,
            ..
        } if *node_dir == dir => {
            reseat_columns(a, dir, order);
            reseat_columns(b, dir, order);
        }
        slot => {
            *slot = order
                .next()
                .expect("a run has exactly as many columns as its skeleton has slots");
        }
    }
}

/// The share a moving pane is owed in the run it has just landed in, and whose
/// share it came out of.
struct ArrivingShare {
    share: u128,
    /// The column that gives the share up, or `None` when the whole run does —
    /// which needs no bookkeeping, because settling normalises what is left.
    paid_by: Option<SeatId>,
}

/// What a pane arriving from *another* run is owed where it lands.
///
/// It buys a share the way a split has always cut one: by column count, off the
/// pane it landed beside. Two things can leave that pane with nothing to sell —
/// a rim has no neighbour at all, and a fixed band holds pixels rather than a
/// share — and in both the run pays instead, every column giving up the same
/// proportion of what it holds, which is the bill §3.3 already states for a
/// column joining a row.
fn arriving_share(
    travelling: &LayoutNode,
    landing: Landing,
    dir: Axis,
    metrics: &SeatMetrics,
    dst_run: &LayoutNode,
    dst_shares: &[(SeatId, u128)],
) -> ArrivingShare {
    let mine = u128::from(flex_run_demand(travelling, dir, metrics));
    if let Landing::Edge { target, .. } = landing {
        // A landing names a *seat*, and the run's root was found by walking up
        // from it through same-direction splits alone, so that seat is one whole
        // column of this run — worth one column exactly when it holds a share.
        let theirs = share_of(dst_shares, target);
        if theirs > 0 {
            return ArrivingShare {
                share: theirs * mine / (mine + 1),
                paid_by: Some(target),
            };
        }
    }
    let held: u128 = dst_shares.iter().map(|(_, share)| *share).sum();
    let columns = u128::from(flex_run_demand(dst_run, dir, metrics));
    ArrivingShare {
        // A fixed band landing among fixed bands asks for nothing and is owed
        // nothing: there is no flexible extent here for anyone to have a share
        // of, and the ratio the settlement writes will not be read.
        share: if mine + columns == 0 {
            0
        } else {
            held * mine / (mine + columns)
        },
        paid_by: None,
    }
}

fn close_seat(
    tree: &LayoutNode,
    metrics: &SeatMetrics,
    target: SeatId,
) -> Result<EditOutcome, EditError> {
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
    Ok(balance_run_containing(next, metrics, &parent_path, dir))
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
        LayoutNode::seat(seat.clone()),
        split_id,
    );
    Ok(balance_run_containing(next, metrics, &tail, Axis::Row))
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
    // No floor from `demand`, and therefore no refusal from it either (user
    // ruling 2026-08-08). A hand on a divider is the user saying "I want this
    // proportion"; answering "that pane would be under its minimum" is answering
    // a question nobody asked, and the old refusal made the divider go dead
    // under the hand well before the pane got small. The minima have not stopped
    // meaning anything — they still refuse a *drop* (`plan_fits`) and still run
    // the concession chain for layouts the program itself chose. They have
    // stopped overruling a gesture.
    //
    // What is left is the ratio's own domain: `(0, 1)` open at both ends, which
    // the type has always enforced, because a side of exactly zero is something
    // a close says and never something a proportion says (红线 L4).
    let chosen = Ratio::clamped_from_ppm(requested.ppm());
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
    let side = match (a.as_ref(), b.as_ref()) {
        (LayoutNode::Seat(s), _) if is_fixed_leaf(s, metrics) => Side::A,
        (_, LayoutNode::Seat(s)) if is_fixed_leaf(s, metrics) => Side::B,
        _ => return Err(EditError::Refused),
    };
    // The slot, and nothing narrower. Same ruling as [`drag_divider`]: the
    // column's declared floor of 170 is what it would *like* to be, and a hand
    // that keeps pulling past it is entitled to a narrower column and the
    // clipped filenames that come with it. What a width may not do is leave the
    // slot it lives in — that is not a preference, it is arithmetic.
    //
    // **KNOWN DIVERGENCE, awaiting a ruling** (Files block P7). What is written
    // here below 170 is not what gets drawn: [`Seat::fixed_width`] raises every
    // declared width to `min_size` (§2.3) before a `Lawful` solve sees it, so a
    // column dragged to 90 is stored at 90, persisted at 90, and drawn at 170.
    // Two pins describe the two halves and neither is wrong on its own —
    // `a_fixed_column_drag_goes_where_the_hand_goes_but_never_leaves_its_slot`
    // holds this line, `a_stacked_pair_of_fixed_columns_has_no_single_width_to_drag`
    // holds the other. Converging them means overturning one, which is a
    // decision about whose minimum wins and not a defect to be patched.
    // See `scratchpad/files-block-final-audit.md`, ruling request R2.
    let width = requested.clamp_to(LogicalPx::ZERO, usable.max(LogicalPx::ZERO));
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
