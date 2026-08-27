//! The solver: a pure function from a tree and a rectangle to seat rectangles.

use crate::demand::{collapse_order, demand, demand_at_min, fold_demand};
use crate::geom::{Axis, AxisSet, DeviceRect, LogicalRect, snap_boundary};
use crate::metrics::SeatMetrics;
use crate::tree::{LayoutNode, Seat, SeatId, SeatKind};
use crate::{COLLAPSED_EXTENT, LogicalPx, RATIO_DENOM_PPM};

/// Whose hand chose the rectangle the seats are being fitted into.
///
/// **User ruling 2026-08-08: a minimum is law to the program and advice to the
/// user.** The numbers in [`SeatMetrics`] do not change; what changes is what
/// they are allowed to *do*. When the program picks a rectangle it must not
/// produce a layout nobody can work in, so the concession chain runs to its end
/// and L4 is an honest refusal. When the user's own hand picks it — a window
/// drag, a divider drag — the same numbers are a recommendation the user is
/// entitled to overrule, and overruling it must look like the window shrinking,
/// not like the app rearranging itself.
///
/// It is a parameter and never a field, on the solver or anywhere else: a policy
/// the solver remembers is a policy two call sites will eventually disagree
/// about, and the disagreement would show up as geometry (discipline ①).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizePolicy {
    /// The user's hand set this rectangle. Minima are advice.
    ///
    /// Nothing collapses, nothing is refused: past the point where every seat
    /// sits on its own floor, the floors themselves give way together and the
    /// seats keep dividing the room in proportion. The content inside clips,
    /// which is what a window getting smaller has always looked like.
    Sovereign,
    /// The program set this rectangle. Minima are law.
    ///
    /// The full §2.6.1 chain: fixed columns spend their declared band, flex
    /// seats fall to their floors, non-focus seats collapse farthest-first, and
    /// if even the focus will not fit the answer is
    /// [`LayoutError::Unsatisfiable`] rather than a rectangle that lies.
    Lawful,
}

/// Which attention shape the same solver is being asked for.
///
/// §3.5, user ruling 6: focus mode is a *constraint*, not a second geometry.
/// One solver, two attentions — a second set of formulas is the thing
/// discipline ① exists to forbid, because two geometries always drift apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    Parallel,
    Focus { stage: SeatId },
}

/// How a seat is presented after the concession chain has had its say.
///
/// `M2-tiny-window-priority.md` §1.3 widened this from the parent spec's plain
/// `Collapsed`: the two chains judge independently, so the result has to be able
/// to name *which* axis was squeezed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presentation {
    Full,
    /// Squeezed to [`COLLAPSED_EXTENT`] along every axis in the set.
    ///
    /// `{Row}` or `{Col}`: a clickable title bar — name plus state icon — still
    /// in the tree and still focusable (§2.6.3). `{Row, Col}`: a 24x24 block
    /// carrying only the state icon, since 24 square cannot hold a name and
    /// forcing one in would be a mosaic pretending to be text (tiny-window §1.3).
    Collapsed(AxisSet),
}

impl Presentation {
    /// Whether this is the double-collapsed degenerate square.
    #[must_use]
    pub fn is_double_collapsed(self) -> bool {
        matches!(self, Presentation::Collapsed(set) if set.is_both())
    }

    /// Whether the seat was squeezed along `axis`.
    #[must_use]
    pub fn is_collapsed_along(self, axis: Axis) -> bool {
        matches!(self, Presentation::Collapsed(set) if set.contains(axis))
    }
}

/// Where one seat ended up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatPlacement {
    pub id: SeatId,
    /// The kind travels *on the placement* (red line L2), so a consumer never
    /// has to look the leaf back up in a tree that a swap may have changed.
    pub kind: SeatKind,
    /// `None` for every non-stage seat in focus mode: the seat keeps its place
    /// in the tree, it is simply not presented (§3.5).
    pub rect: Option<LogicalRect>,
    /// The same rectangle with each boundary snapped to the device pixel grid
    /// (§2.5). Adjacent seats snap the *same* logical number, so they cannot
    /// disagree by a pixel — that is red line L6 stated as an implementation.
    pub device_rect: Option<DeviceRect>,
    pub presentation: Presentation,
}

/// The solver's answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SeatLayout {
    /// In-order, which is a function of the tree and of nothing else (D2).
    pub rects: Vec<SeatPlacement>,
}

impl SeatLayout {
    /// The seats that were actually given a rectangle.
    pub fn presented(&self) -> impl Iterator<Item = (&SeatPlacement, LogicalRect)> {
        self.rects
            .iter()
            .filter_map(|p| p.rect.map(|rect| (p, rect)))
    }

    /// The placement of one seat.
    #[must_use]
    pub fn get(&self, id: SeatId) -> Option<&SeatPlacement> {
        self.rects.iter().find(|p| p.id == id)
    }

    /// Every logical rectangle, in in-order. Convenient for the "these two
    /// layouts are the same picture" assertions the spec pins.
    #[must_use]
    pub fn logical_rects(&self) -> Vec<Option<LogicalRect>> {
        self.rects.iter().map(|p| p.rect).collect()
    }
}

/// Why a layout could not be produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutError {
    /// L4: even the focus seat does not fit.
    ///
    /// The honest error state rather than a lying rectangle (W2). The caller
    /// renders fit-what-fits: as many collapsed bars as go in, with a trailing
    /// "N more do not fit", and no buttons — window size is a gesture the user
    /// makes with the OS, not something the app should reach over and change
    /// (tiny-window §2, §4.3).
    ///
    /// Reachable only under [`SizePolicy::Lawful`]. A rectangle the user chose
    /// is never refused (user ruling 2026-08-08): telling someone their own
    /// window is the wrong size is the one answer this error must never give.
    Unsatisfiable { axis: Axis },
    /// The focus (or stage) seat is not in this tree.
    UnknownSeat(SeatId),
}

/// The single global decision one axis made, before any rectangle is cut.
///
/// W3: the concession is one decision for the whole tree. Deciding it per node
/// would let two symmetric subtrees land on different levels purely by traversal
/// order, which is exactly what D2 exists to forbid.
///
/// There is deliberately no `ConcessionLevel` value here. §2.6.2 keeps the level
/// off disk and tiny-window §1.3 keeps it out of the output; a field nobody may
/// read is a field that will drift from what the code actually did. What the
/// levels *do* — fixed columns give before flex seats, flex seats give before
/// anything collapses, the focus falls last — is visible in the rectangles, and
/// that is what the pins assert.
struct AxisPlan {
    axis: Axis,
    /// Sorted, so membership never depends on insertion order (L8).
    collapsed: Vec<SeatId>,
    /// How far each fixed column was pulled toward its floor, in ppm.
    fixed_shrink_ppm: u32,
    /// How far every seat's floor was pulled toward zero, in ppm.
    ///
    /// Zero in every [`SizePolicy::Lawful`] plan — under law the floors are the
    /// floors, and the chain reaches for a collapse rather than for this. It is
    /// the whole of what [`SizePolicy::Sovereign`] does differently: past L2 the
    /// minima themselves give way, together and in one global step, so the seats
    /// go on dividing the room in proportion instead of rearranging themselves.
    floor_relax_ppm: u32,
}

impl AxisPlan {
    /// A plan that concedes nothing: the L0 answer, and the base every other
    /// level is a modification of.
    fn intact(axis: Axis) -> Self {
        Self {
            axis,
            collapsed: Vec::new(),
            fixed_shrink_ppm: 0,
            floor_relax_ppm: 0,
        }
    }

    fn is_collapsed(&self, id: SeatId) -> bool {
        self.collapsed.binary_search(&id).is_ok()
    }

    /// The floor this kind is held to under the concessions in force.
    ///
    /// Relaxing toward zero is the same arithmetic as pulling a fixed column
    /// toward its floor, with zero for the floor — one linear ramp, not two, so
    /// there is only one rounding rule in the crate to keep honest.
    fn floor(&self, kind: SeatKind, metrics: &SeatMetrics) -> LogicalPx {
        shrink_fixed(
            metrics.min_size(kind, self.axis),
            LogicalPx::ZERO,
            self.floor_relax_ppm,
        )
    }

    /// The extent this seat asks for along the plan's axis.
    fn leaf_demand(&self, seat: &Seat, metrics: &SeatMetrics) -> LogicalPx {
        if self.is_collapsed(seat.id) {
            return COLLAPSED_EXTENT;
        }
        let floor = self.floor(seat.kind, metrics);
        match seat.fixed_width(metrics, self.axis) {
            Some(full) => shrink_fixed(full, floor, self.fixed_shrink_ppm),
            None => floor,
        }
    }

    /// What a subtree needs along the plan's axis, given the concessions in force.
    fn subtree_demand(&self, node: &LayoutNode, metrics: &SeatMetrics) -> LogicalPx {
        fold_demand(node, self.axis, metrics.divider(), &|seat: &Seat| {
            self.leaf_demand(seat, metrics)
        })
    }

    /// The extent of a subtree that is *entirely* fixed along this axis, if it is.
    ///
    /// §2.3 generalised from the row axis to both: a collapsed seat is fixed at
    /// 24 along the axis it was squeezed on, and a stack of only fixed things is
    /// itself fixed at its widest member — without that last clause two stacked
    /// files columns throw away their fixedness and balloon to half the window.
    fn fixed_extent(&self, node: &LayoutNode, metrics: &SeatMetrics) -> Option<LogicalPx> {
        match node {
            LayoutNode::Seat(seat) => {
                if self.is_collapsed(seat.id) {
                    Some(COLLAPSED_EXTENT)
                } else {
                    let floor = self.floor(seat.kind, metrics);
                    seat.fixed_width(metrics, self.axis)
                        .map(|full| shrink_fixed(full, floor, self.fixed_shrink_ppm))
                }
            }
            LayoutNode::Split { dir, a, b, .. } => {
                let fa = self.fixed_extent(a, metrics)?;
                let fb = self.fixed_extent(b, metrics)?;
                Some(if *dir == self.axis {
                    fa + fb + metrics.divider()
                } else {
                    fa.max(fb)
                })
            }
        }
    }

    /// The pixels inside a subtree that no ratio may divide: its fixed columns,
    /// its collapsed bars, and the dividers standing between them and the rest
    /// of it, along the plan's axis.
    ///
    /// §2.3 said "a fixed column takes pixels and the ratios divide what is
    /// left" and [`Self::fixed_extent`] only ever answered for a subtree that
    /// was fixed *entirely* — so the rule held for a files column hanging
    /// directly off a split and quietly lapsed the moment one was nested beside
    /// a flex pane. The same three columns then came out at two different sets
    /// of widths depending on which way the tree happened to lean, which is how
    /// a pane that was merely re-placed made an untouched sibling change size.
    ///
    /// The dividers are in here for the same reason the fixed columns are: a
    /// divider is a pixel nobody has a share of, and leaving it inside the
    /// divided extent made a column's width depend on how many dividers
    /// happened to sit on its side of the tree rather than on its share.
    fn reserved(&self, node: &LayoutNode, metrics: &SeatMetrics) -> LogicalPx {
        match node {
            LayoutNode::Split { dir, a, b, .. } if *dir == self.axis => {
                self.reserved(a, metrics) + self.reserved(b, metrics) + metrics.divider()
            }
            // A leaf, or a cross-direction subtree — one column of this run,
            // and reserved only if the whole of it is a band of pixels.
            _ => self.fixed_extent(node, metrics).unwrap_or(LogicalPx::ZERO),
        }
    }

    /// Whether this subtree holds anything that takes a share at all.
    ///
    /// The exact complement of [`Self::fixed_extent`]: a subtree with no flex
    /// seat in it folds to a fixed extent at every level, and one with a flex
    /// seat cannot.
    fn has_flex(&self, node: &LayoutNode, metrics: &SeatMetrics) -> bool {
        self.fixed_extent(node, metrics).is_none()
    }

    /// Whether every seat in this subtree is collapsed along the plan's axis.
    ///
    /// Such a subtree is a bar, and a bar stays exactly [`COLLAPSED_EXTENT`]: it
    /// is the one thing that must not absorb a trailing surplus.
    fn is_all_collapsed(&self, node: &LayoutNode) -> bool {
        let mut all = true;
        node.walk_in_order(&mut |seat| all &= self.is_collapsed(seat.id));
        all
    }
}

/// Pull a fixed column from its declared width toward its floor by `t` ppm.
///
/// Linear, and gradual on purpose: tiny-window §2 notes that L1 leaves a files
/// column somewhere in the same 240..170 band a hand drag can reach, so a window
/// resize landing inside that band is not a surprise and needs no banner.
fn shrink_fixed(full: LogicalPx, floor: LogicalPx, t_ppm: u32) -> LogicalPx {
    if t_ppm == 0 || full <= floor {
        return full;
    }
    let range = i128::from((full - floor).subpixels());
    let taken = range * i128::from(t_ppm) / i128::from(RATIO_DENOM_PPM);
    LogicalPx::from_subpixels(full.subpixels() - taken as i64)
}

/// The smallest shrink, in ppm, that brings the tree's demand within `avail`.
///
/// Demand is non-increasing in `t`, so a bisection finds the least sufficient
/// value; integer bisection is deterministic, which D1 needs and a float solve
/// would not give.
fn least_sufficient_shrink(
    tree: &LayoutNode,
    axis: Axis,
    metrics: &SeatMetrics,
    avail: LogicalPx,
) -> u32 {
    least_sufficient(|t| AxisPlan {
        fixed_shrink_ppm: t,
        ..AxisPlan::intact(axis)
    })(tree, metrics, avail)
}

/// The smallest relaxation, in ppm, that brings a fully-conceded tree's demand
/// within `avail`.
///
/// The [`SizePolicy::Sovereign`] counterpart of [`least_sufficient_shrink`], and
/// deliberately the same bisection over the same monotone predicate: demand is
/// non-increasing in the relaxation, so the least sufficient value exists and an
/// integer bisection finds it without a float in sight (D3).
///
/// Returning [`RATIO_DENOM_PPM`] means even floors of zero did not fit — the
/// dividers alone are wider than the room. That is not an error under this
/// policy: it is a window the user has dragged past the point where the seats
/// have any room left, and [`allocate`] pins the cuts inside the slot.
fn least_sufficient_relax(
    tree: &LayoutNode,
    axis: Axis,
    metrics: &SeatMetrics,
    avail: LogicalPx,
) -> u32 {
    least_sufficient(|t| AxisPlan {
        fixed_shrink_ppm: RATIO_DENOM_PPM,
        floor_relax_ppm: t,
        ..AxisPlan::intact(axis)
    })(tree, metrics, avail)
}

/// Bisect a family of plans parameterised by one ppm knob for the least value
/// whose demand fits.
///
/// Integer bisection is deterministic, which D1 needs and a float solve would
/// not give. Both concession knobs bisect the same way, so they share the search
/// rather than each carrying a copy of it.
fn least_sufficient(
    plan_at: impl Fn(u32) -> AxisPlan,
) -> impl Fn(&LayoutNode, &SeatMetrics, LogicalPx) -> u32 {
    move |tree, metrics, avail| {
        let (mut lo, mut hi) = (0u32, RATIO_DENOM_PPM);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if plan_at(mid).subtree_demand(tree, metrics) <= avail {
                hi = mid
            } else {
                lo = mid + 1
            }
        }
        lo
    }
}

/// Decide one axis's concessions. The two axes never consult each other.
///
/// tiny-window §1.3: `demand` sums only along its own axis and takes a max
/// across it, so walking the tree for rows and walking it for columns are two
/// independent folds — neither one's output is the other's input. There is
/// therefore no "which axis yields first" question to answer, and both chains
/// collapse seats in the same order, merely stopping at different points on it.
fn plan_axis(
    tree: &LayoutNode,
    axis: Axis,
    avail: LogicalPx,
    metrics: &SeatMetrics,
    focus: SeatId,
    policy: SizePolicy,
) -> Result<AxisPlan, LayoutError> {
    // L0.
    if demand(tree, axis, metrics) <= avail {
        return Ok(AxisPlan::intact(axis));
    }
    // L1 — fixed columns spend the slack they declared for themselves before any
    // flex seat is asked to give. A fixed column carries a stated 240..170 band;
    // taking from a flex seat first would be filling one's own hole with someone
    // else's ground. On the column axis there is no fixed class, so `demand` and
    // `demand_at_min` coincide there and this branch cannot fire — §2.7's "L1 is
    // skipped vertically", falling out of the definitions rather than being
    // special-cased.
    if demand_at_min(tree, axis, metrics) <= avail {
        return Ok(AxisPlan {
            fixed_shrink_ppm: least_sufficient_shrink(tree, axis, metrics, avail),
            ..AxisPlan::intact(axis)
        });
    }
    // L2 is already in force from here on: every flex seat is at its kind's
    // minimum and every fixed column at its floor. This is where the two
    // policies part company, and the only place they do.
    //
    // Under the user's own hand there is no L3 and no L4 (user ruling
    // 2026-08-08). The ladder's remaining rungs are *rearrangements* — a seat
    // that was a pane becomes a bar, and past that the window refuses to be the
    // size it plainly is — and neither is an answer to "make my window
    // narrower". The floors relax together instead, by one global amount, so
    // every seat keeps its share of a smaller room and the content inside does
    // the clipping. W3 is upheld exactly as it is for the other levels: one
    // decision for the whole tree, not a per-node greed.
    if policy == SizePolicy::Sovereign {
        return Ok(AxisPlan {
            fixed_shrink_ppm: RATIO_DENOM_PPM,
            floor_relax_ppm: least_sufficient_relax(tree, axis, metrics, avail),
            ..AxisPlan::intact(axis)
        });
    }
    // L3 collapses non-focus seats, farthest from the focus first, and stops the
    // moment the tree fits.
    let mut plan = AxisPlan {
        fixed_shrink_ppm: RATIO_DENOM_PPM,
        ..AxisPlan::intact(axis)
    };
    for id in collapse_order(tree, focus) {
        let at = plan.collapsed.partition_point(|x| *x < id);
        plan.collapsed.insert(at, id);
        if plan.subtree_demand(tree, metrics) <= avail {
            return Ok(plan);
        }
    }
    // L4: the focus seat is the last to fall, and it has fallen.
    Err(LayoutError::Unsatisfiable { axis })
}

/// Solve the layout.
///
/// Pure: no IO, no clock, no randomness, no global state, no cache (§3.1). It is
/// `O(seats)` over a tree of a few dozen leaves, so solving every frame during a
/// divider drag is the preferred implementation rather than something to
/// optimise away — the drop preview, the live drag and the committed layout all
/// call *this*, because two geometries always drift (D4).
/// `policy` says whose rectangle `viewport` is, and therefore whether the minima
/// in `metrics` are law or advice (user ruling 2026-08-08). It is the caller's to
/// state because only the caller knows what it is doing: a frame drawn into the
/// window the user is dragging is [`SizePolicy::Sovereign`], a plan the program
/// is judging before it commits it is [`SizePolicy::Lawful`].
pub fn solve(
    tree: &LayoutNode,
    viewport: LogicalRect,
    metrics: &SeatMetrics,
    focus: SeatId,
    mode: LayoutMode,
    policy: SizePolicy,
) -> Result<SeatLayout, LayoutError> {
    if tree.find_seat(focus).is_none() {
        return Err(LayoutError::UnknownSeat(focus));
    }
    match mode {
        LayoutMode::Focus { stage } => solve_focused(tree, viewport, metrics, stage, policy),
        LayoutMode::Parallel => solve_parallel(tree, viewport, metrics, focus, policy),
    }
}

/// Focus mode: the stage owns the viewport and the tree is not touched.
///
/// Red line L13. Leaving the parallel tree exactly as it was on exit is not a
/// feature that had to be written — it is what happens when nothing is done.
fn solve_focused(
    tree: &LayoutNode,
    viewport: LogicalRect,
    metrics: &SeatMetrics,
    stage: SeatId,
    policy: SizePolicy,
) -> Result<SeatLayout, LayoutError> {
    let Some(seat) = tree.find_seat(stage) else {
        return Err(LayoutError::UnknownSeat(stage));
    };
    // The stage is the whole viewport, so there is nothing to divide and nothing
    // to concede — the only question the minima can answer here is whether to
    // refuse, and a sovereign rectangle is never refused.
    if policy == SizePolicy::Lawful {
        for axis in Axis::BOTH {
            if viewport.extent(axis) < metrics.min_size(seat.kind, axis) {
                return Err(LayoutError::Unsatisfiable { axis });
            }
        }
    }
    let rects = tree
        .seats_in_order()
        .into_iter()
        .map(|s| {
            let on_stage = s.id == stage;
            SeatPlacement {
                id: s.id,
                kind: s.kind,
                rect: on_stage.then_some(viewport),
                device_rect: on_stage.then(|| to_device(viewport, metrics.scale_ppm())),
                presentation: Presentation::Full,
            }
        })
        .collect();
    Ok(SeatLayout { rects })
}

fn solve_parallel(
    tree: &LayoutNode,
    viewport: LogicalRect,
    metrics: &SeatMetrics,
    focus: SeatId,
    policy: SizePolicy,
) -> Result<SeatLayout, LayoutError> {
    let plan = |axis: Axis| plan_axis(tree, axis, viewport.extent(axis), metrics, focus, policy);
    let (row, col) = (plan(Axis::Row)?, plan(Axis::Col)?);
    let mut rects = Vec::with_capacity(tree.seat_count());
    allocate(tree, viewport, &row, &col, metrics, &mut rects);
    Ok(SeatLayout { rects })
}

fn allocate(
    node: &LayoutNode,
    rect: LogicalRect,
    row: &AxisPlan,
    col: &AxisPlan,
    metrics: &SeatMetrics,
    out: &mut Vec<SeatPlacement>,
) {
    match node {
        LayoutNode::Seat(seat) => {
            let mut collapsed_on: Option<AxisSet> = None;
            for (plan, axis) in [(row, Axis::Row), (col, Axis::Col)] {
                if plan.is_collapsed(seat.id) {
                    let one = AxisSet::of(axis);
                    collapsed_on = Some(collapsed_on.map_or(one, |set| set.union(one)));
                }
            }
            out.push(SeatPlacement {
                id: seat.id,
                kind: seat.kind,
                rect: Some(rect),
                device_rect: Some(to_device(rect, metrics.scale_ppm())),
                presentation: match collapsed_on {
                    None => Presentation::Full,
                    Some(set) => Presentation::Collapsed(set),
                },
            });
        }
        LayoutNode::Split {
            dir, ratio, a, b, ..
        } => {
            let plan = if *dir == Axis::Row { row } else { col };
            let avail = rect.extent(*dir) - metrics.divider();
            // §2.3, stated once and for every shape: the pixels nobody has a
            // share of come off the top, and the ratio divides what is left.
            let (res_a, res_b) = (plan.reserved(a, metrics), plan.reserved(b, metrics));
            let raw = match (plan.has_flex(a, metrics), plan.has_flex(b, metrics)) {
                // Both sides fixed: someone has to eat the surplus, or it is a
                // patch of white that can never be filled and never be clicked
                // (§2.3, "the trailing side absorbs — kill the dead white").
                // The trailing side eats it, unless the trailing side is nothing
                // but collapsed bars, which must stay exactly 24; then the
                // leading side eats it. Both sides cannot be all-collapsed,
                // because the focus seat is never collapsed (W2).
                (false, false) => {
                    if plan.is_all_collapsed(b) {
                        avail - res_b
                    } else {
                        res_a
                    }
                }
                // One side holds every share there is, so it takes the whole of
                // what is left over from the other side's band.
                (false, true) => res_a,
                (true, false) => avail - res_b,
                (true, true) => res_a + ratio.apply(avail - res_a - res_b),
            };
            // The flex squeeze of L2: shares give way proportionally, and the
            // floor is what each *subtree* demands rather than what one seat
            // needs (red line L3 — an outer divider once crushed a two-pane group
            // to 260 total, 130 apiece, because the constraint was per child).
            let lo = plan.subtree_demand(a, metrics);
            let hi = avail - plan.subtree_demand(b, metrics);
            // The cut stays inside the slot. Under law it always was — `lo` is
            // bounded by a demand the plan has already proved fits — but a
            // sovereign window can be dragged narrower than the dividers inside
            // it, and then `lo` is the wider number. A near edge past its own
            // far edge is not a small rectangle, it is a negative one, and no
            // consumer of `SeatPlacement` is owed that.
            let room = avail.max(LogicalPx::ZERO);
            let a_ext = raw.min(hi).max(lo).clamp_to(LogicalPx::ZERO, room);

            let near = rect.near(*dir);
            let far = near + rect.extent(*dir);
            let cut = near + a_ext;
            // The divider takes its full logical pixel out of the room, and what
            // is left over — nothing, in the degenerate case above — is the
            // trailing side's.
            let resume = (cut + metrics.divider()).min(far);
            let (rect_a, rect_b) = match dir {
                Axis::Row => (
                    LogicalRect::new(rect.left, rect.top, cut, rect.bottom),
                    LogicalRect::new(resume, rect.top, rect.right, rect.bottom),
                ),
                Axis::Col => (
                    LogicalRect::new(rect.left, rect.top, rect.right, cut),
                    LogicalRect::new(rect.left, resume, rect.right, rect.bottom),
                ),
            };
            allocate(a, rect_a, row, col, metrics, out);
            allocate(b, rect_b, row, col, metrics, out);
        }
    }
}

/// The pixels of a subtree that no ratio divides, with nothing conceded: its
/// fixed columns and the dividers between them and the rest of it, along `axis`.
///
/// Published because a divider drag has to *invert* what [`allocate`] does with
/// a ratio, and the two must not be two opinions: a cut `leading` px into a slot
/// is asking for `(leading - reserved(a)) / (usable - reserved(a) - reserved(b))`
/// and for nothing else. Working the inverse out at the call site from
/// rectangles is exactly the second, drifting geometry D4 forbids.
///
/// "With nothing conceded" is the honest scope. The concession chain can shrink
/// a fixed column or collapse a seat, and this does not model that — the drag
/// asks where the hand is, and `allocate`'s own `lo`/`hi` clamp is what holds
/// the answer inside the slot when a floor is in force.
#[must_use]
pub fn reserved_extent(node: &LayoutNode, axis: Axis, metrics: &SeatMetrics) -> LogicalPx {
    AxisPlan::intact(axis).reserved(node, metrics)
}

/// Snap a rectangle's four boundaries onto the device grid.
///
/// Boundary rounding, not width rounding (§2.5, red line L6): two adjacent seats
/// hand the *same* logical number to this function, so they cannot come back
/// with device edges a pixel apart. Rounding each width instead accumulates a
/// seam along a long chain, and one seam is a fake divider the user can see.
fn to_device(rect: LogicalRect, scale_ppm: u32) -> DeviceRect {
    DeviceRect {
        left: snap_boundary(rect.left, scale_ppm),
        top: snap_boundary(rect.top, scale_ppm),
        right: snap_boundary(rect.right, scale_ppm),
        bottom: snap_boundary(rect.bottom, scale_ppm),
    }
}
