//! Every pinned test name that `docs/M2-layout-solver-spec.md` and
//! `docs/M2-tiny-window-priority.md` declare for solver logic, plus the
//! determinism properties D1/D2 ask for.
//!
//! These live in `tests/` so they can only reach the public API — the same
//! physical enforcement CONVENTIONS §three asks of the gate tests.

use bt_layout::{
    Axis, AxisSet, COLLAPSED_EXTENT, DIVIDER, Edit, EditError, FILES_W, FILES_W_MIN, LayoutError,
    LayoutMode, LayoutNode, LogicalPx, LogicalRect, LogicalSize, MIN_PANE_H, MIN_PANE_W,
    MIN_PREVIEW_W, Presentation, Ratio, Seat, SeatId, SeatKind, SeatLayout, SeatMetrics, SplitId,
    WorkAreaHint, apply, collapse_order, demand, floor_demand, in_order_index, members,
    necessity_holds, path_to_seat, run_demand, run_root_path_of_seat, run_split_ids, share_ppm,
    solve, tree_distance, window_min_inner_size,
};

// ---------------------------------------------------------------- fixtures --

fn m() -> SeatMetrics {
    SeatMetrics::ruled_at_unit_scale()
}

fn seat(id: u64, kind: SeatKind) -> LayoutNode {
    LayoutNode::seat(Seat::new(SeatId(id), kind))
}

fn term(id: u64) -> LayoutNode {
    seat(id, SeatKind::Terminal)
}

fn files(id: u64) -> LayoutNode {
    seat(id, SeatKind::Files)
}

fn row(id: u64, a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::split(SplitId(id), Axis::Row, a, b)
}

fn col(id: u64, a: LayoutNode, b: LayoutNode) -> LayoutNode {
    LayoutNode::split(SplitId(id), Axis::Col, a, b)
}

fn viewport(w: i64, h: i64) -> LogicalRect {
    LogicalRect::from_px(w, h)
}

fn solved(tree: &LayoutNode, view: LogicalRect, focus: u64) -> SeatLayout {
    solve(tree, view, &m(), SeatId(focus), LayoutMode::Parallel).expect("layout should be solvable")
}

fn extent_px(layout: &SeatLayout, id: u64, axis: Axis) -> i64 {
    layout
        .get(SeatId(id))
        .and_then(|p| p.rect)
        .map(|r| r.extent(axis).floor_px())
        .expect("seat should be presented")
}

fn ratio_of(tree: &LayoutNode, split: u64) -> Ratio {
    tree.ratios()
        .into_iter()
        .find(|(id, _)| *id == SplitId(split))
        .map(|(_, r)| r)
        .expect("split should exist")
}

/// A run of `n` terminals, left-leaning ids, balanced by construction order.
fn row_run(n: u64) -> LayoutNode {
    let mut node = term(1);
    for i in 2..=n {
        node = row(100 + i, node, term(i));
    }
    node
}

// ------------------------------------------------------ D1/D2/D4/D5 purity --

#[test]
fn solve_is_a_pure_function_of_its_inputs() {
    let tree = row(1, term(1), col(2, term(2), files(3)));
    for (w, h) in [(1400, 900), (700, 400), (420, 260), (1, 1)] {
        let first = solve(&tree, viewport(w, h), &m(), SeatId(1), LayoutMode::Parallel);
        let second = solve(&tree, viewport(w, h), &m(), SeatId(1), LayoutMode::Parallel);
        assert_eq!(first, second, "same input must give bit-identical output");
    }
}

#[test]
fn two_structurally_equal_trees_solve_to_identical_rects() {
    // Built in two different orders, node for node the same tree.
    let left_first = row(1, row(2, term(1), term(2)), term(3));
    let inner = row(2, term(1), term(2));
    let assembled_later = {
        let outer_b = term(3);
        row(1, inner, outer_b)
    };
    assert_eq!(left_first, assembled_later);
    assert_eq!(
        solved(&left_first, viewport(1080, 700), 1),
        solved(&assembled_later, viewport(1080, 700), 1),
    );
}

#[test]
fn permuting_child_construction_order_solves_identically() {
    // The same shape assembled bottom-up versus top-down, and with the sibling
    // subtrees built in the opposite order. Nothing about *when* a node was
    // constructed may reach the geometry (D2).
    let bottom_up = {
        let a = col(10, term(1), term(2));
        let b = col(11, term(3), term(4));
        row(1, a, b)
    };
    let reverse_built = {
        let b = col(11, term(3), term(4));
        let a = col(10, term(1), term(2));
        row(1, a, b)
    };
    assert_eq!(
        solved(&bottom_up, viewport(1200, 800), 3),
        solved(&reverse_built, viewport(1200, 800), 3),
    );
}

#[test]
fn the_drop_preview_and_the_drop_agree_rect_for_rect() {
    // The preview is the same `apply` + `solve` the commit runs; there is no
    // second, estimating implementation to drift from.
    let tree = row(1, row(2, term(1), term(2)), term(3));
    let view = viewport(1080, 700);
    let drop = Edit::SplitSeat {
        target: SeatId(3),
        dir: Axis::Row,
        leading: false,
        arriving: LayoutNode::seat(Seat::new(SeatId(4), SeatKind::Terminal)),
        split_id: SplitId(9),
    };

    let previewed = apply(&tree, &m(), &drop).expect("preview plans on the real tree ops");
    let preview_rects = solved(&previewed.tree, view, 1);
    let committed = apply(&tree, &m(), &drop).expect("the commit runs the same ops");
    let commit_rects = solved(&committed.tree, view, 1);
    assert_eq!(preview_rects, commit_rects);

    // D5: feasibility is a fact about the layout that WOULD exist. Halving the
    // target would have judged 359/2 = 179 < 260 and refused a fourth column
    // that in fact fits at 269 apiece.
    for id in 1..=4 {
        assert!(
            extent_px(&commit_rects, id, Axis::Row) >= MIN_PANE_W.floor_px(),
            "column {id} must clear the terminal minimum"
        );
    }
    assert_eq!(extent_px(&commit_rects, 1, Axis::Row), 269);
}

#[test]
fn an_arriving_subtree_is_worth_the_columns_it_actually_has() {
    // A tab dropped onto a pane brings its whole layout, and the run it joins is
    // divided by column count. Two columns arriving beside two columns is a run
    // of four equal columns — count the arrival as one anonymous box and the
    // newcomers come out half the width of the seats they landed beside.
    let tree = row(1, term(1), term(2));
    let out = apply(
        &tree,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(2),
            dir: Axis::Row,
            leading: false,
            arriving: row(50, term(10), term(11)),
            split_id: SplitId(9),
        },
    )
    .expect("splitting a seat is always structurally possible");

    let rects = solved(&out.tree, viewport(1203, 700), 1);
    let widths: Vec<i64> = [1, 2, 10, 11]
        .into_iter()
        .map(|id| extent_px(&rects, id, Axis::Row))
        .collect();
    // 1203 less three dividers is 1200, four ways. The columns are not required
    // to be bit-identical — a run is a chain of nested splits and ppm rounding
    // lands on a pixel boundary differently at each depth — but they are
    // required to be the *same column*, which one pixel of slack says and a
    // subtree counted as one box (which would give 400/200/100/100) does not.
    let (lo, hi) = (*widths.iter().min().unwrap(), *widths.iter().max().unwrap());
    assert!(
        hi - lo <= 1 && (299..=301).contains(&lo),
        "four columns, four shares: {widths:?}"
    );

    // The same drop with one seat arriving divides the same run three ways, which
    // is what makes the assertion above a statement about the *subtree* rather
    // than about the viewport.
    let lone = apply(
        &tree,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(2),
            dir: Axis::Row,
            leading: false,
            arriving: term(10),
            split_id: SplitId(9),
        },
    )
    .expect("splitting a seat is always structurally possible");
    let lone = solved(&lone.tree, viewport(1203, 700), 1);
    assert_eq!(extent_px(&lone, 10, Axis::Row), 400);
}

#[test]
fn replacing_a_seat_rewrites_no_ratio_and_takes_exactly_its_place() {
    // §3.3's `replaceLeafIn`: a slot can receive a whole layout without the run
    // it sits in being re-divided. Taking a seat's place moves no boxes, so `F`
    // is empty and the arrival's footprint is the departed seat's rectangle to
    // the pixel.
    let tree = row(1, term(1), row(2, term(2), term(3)));
    let view = viewport(1200, 800);
    let before = solved(&tree, view, 1);
    let vacated = before.get(SeatId(3)).unwrap().rect.unwrap();

    let out = apply(
        &tree,
        &m(),
        &Edit::ReplaceSeat {
            target: SeatId(3),
            arriving: col(50, term(10), term(11)),
        },
    )
    .expect("the target is in the tree");

    assert!(out.focus_set.is_empty(), "a replace re-divides nothing");
    assert!(necessity_holds(&tree, &out.tree, &out.focus_set));
    assert_eq!(tree.ratios(), {
        let mut kept = out.tree.ratios();
        kept.retain(|(id, _)| *id != SplitId(50));
        kept
    });

    let after = solved(&out.tree, view, 1);
    assert_eq!(
        after.get(SeatId(1)).unwrap().rect,
        before.get(SeatId(1)).unwrap().rect,
        "a replace moves nobody"
    );
    assert_eq!(
        after.get(SeatId(2)).unwrap().rect,
        before.get(SeatId(2)).unwrap().rect,
    );
    let arrived = [SeatId(10), SeatId(11)].map(|id| after.get(id).unwrap().rect.unwrap());
    assert_eq!(arrived[0].left, vacated.left);
    assert_eq!(arrived[0].top, vacated.top);
    assert_eq!(arrived[1].right, vacated.right);
    assert_eq!(arrived[1].bottom, vacated.bottom);
}

// --------------------------------------------------- §3.3 the focus set F(E) --

#[test]
fn opening_a_seat_rebalances_only_its_own_run() {
    // Two independent row runs under one column split. Splitting inside the
    // first must leave the second's ratio, and the column split's ratio, alone.
    let tree = col(1, row(2, term(1), term(2)), row(3, term(3), term(4)));
    let out = apply(
        &tree,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(1),
            dir: Axis::Row,
            leading: false,
            arriving: LayoutNode::seat(Seat::new(SeatId(5), SeatKind::Terminal)),
            split_id: SplitId(9),
        },
    )
    .expect("splitting a seat is always structurally possible");

    assert!(necessity_holds(&tree, &out.tree, &out.focus_set));
    assert_eq!(
        ratio_of(&out.tree, 3),
        ratio_of(&tree, 3),
        "other run untouched"
    );
    assert_eq!(
        ratio_of(&out.tree, 1),
        ratio_of(&tree, 1),
        "the col split is not in F"
    );
    assert!(!out.focus_set.contains(SplitId(3)));
    assert!(!out.focus_set.contains(SplitId(1)));
    // Three columns in that run, divided by column count rather than headcount.
    assert_eq!(ratio_of(&out.tree, 2).ppm(), 666_666);
    assert_eq!(ratio_of(&out.tree, 9).ppm(), 500_000);

    // A same-direction split reachable only *through* a cross-direction split
    // belongs to a different run, and the run root is where the rebalance both
    // starts and stops. Widening either end is the red gate §3.3 names.
    let nested = row(1, term(1), col(2, row(3, term(2), term(3)), term(4)));
    let dragged = apply(
        &nested,
        &m(),
        &Edit::DragDivider {
            split: SplitId(3),
            requested: Ratio::from_ppm(700_000).unwrap(),
            usable: LogicalPx::px(2000),
        },
    )
    .expect("a hand drag inside the inner run")
    .tree;
    let widened = apply(
        &dragged,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(1),
            dir: Axis::Row,
            leading: false,
            arriving: LayoutNode::seat(Seat::new(SeatId(5), SeatKind::Terminal)),
            split_id: SplitId(8),
        },
    )
    .expect("splitting a seat in the root run");
    assert!(!widened.focus_set.contains(SplitId(3)));
    assert_eq!(
        ratio_of(&widened.tree, 3).ppm(),
        700_000,
        "the hand-dragged ratio in another run must survive bit-identically"
    );
    assert_eq!(ratio_of(&widened.tree, 2), ratio_of(&nested, 2));
    assert!(necessity_holds(&dragged, &widened.tree, &widened.focus_set));
}

#[test]
fn closing_a_seat_rebalances_only_the_run_it_left() {
    // Three columns in the first run: 1/3, then an even split of the rest.
    let three = row(2, row(4, term(1), term(2)), term(3));
    let tree = col(1, three, row(3, term(4), term(5)));
    let seeded = apply(
        &tree,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(1),
            dir: Axis::Row,
            leading: false,
            arriving: LayoutNode::seat(Seat::new(SeatId(6), SeatKind::Terminal)),
            split_id: SplitId(9),
        },
    )
    .expect("seed the run")
    .tree;

    let out = apply(&seeded, &m(), &Edit::CloseSeat { target: SeatId(6) })
        .expect("closing a seat with a sibling always works");
    assert!(necessity_holds(&seeded, &out.tree, &out.focus_set));
    assert_eq!(
        ratio_of(&out.tree, 3),
        ratio_of(&seeded, 3),
        "other run untouched"
    );
    // A rule that only balances on the way in exists only half: the survivors
    // must not keep the shares of a set they are no longer in.
    assert_eq!(ratio_of(&out.tree, 4).ppm(), 500_000);
    assert_eq!(ratio_of(&out.tree, 2).ppm(), 666_666);
}

#[test]
fn a_center_swap_moves_no_rectangle() {
    let tree = row(1, row(2, term(1), term(2)), term(3));
    let view = viewport(1200, 700);
    let before = solved(&tree, view, 1);

    let out = apply(
        &tree,
        &m(),
        &Edit::CenterSwap {
            a: SeatId(1),
            b: SeatId(3),
        },
    )
    .expect("both seats exist");
    assert!(out.focus_set.is_empty(), "F(centre swap) = the empty set");
    assert!(necessity_holds(&tree, &out.tree, &out.focus_set));

    let after = solved(&out.tree, view, 1);
    let boxes = |l: &SeatLayout| {
        let mut v: Vec<_> = l.presented().map(|(_, r)| r).collect();
        v.sort_by_key(|r| (r.left.subpixels(), r.top.subpixels()));
        v
    };
    assert_eq!(
        boxes(&before),
        boxes(&after),
        "the same boxes, in the same places"
    );

    // Even when the swap moves a fixed column across the tree, no ratio moves.
    let mixed = row(1, files(1), term(2));
    let swapped = apply(
        &mixed,
        &m(),
        &Edit::CenterSwap {
            a: SeatId(1),
            b: SeatId(2),
        },
    )
    .expect("both seats exist");
    assert_eq!(swapped.tree.ratios(), mixed.ratios());
}

#[test]
fn a_dpi_change_rewrites_no_ratio() {
    let tree = row(1, row(2, term(1), term(2)), files(3));
    let out = apply(&tree, &m(), &Edit::DpiChanged).expect("a DPI change is always applicable");
    assert!(out.focus_set.is_empty());
    assert_eq!(out.tree, tree, "not one node, ratio or fixed extent moved");

    // A scale change is a similarity transform: the same logical rectangles, and
    // device rectangles that simply scale.
    let view = viewport(1200, 700);
    let at_100 = solve(&tree, view, &m(), SeatId(1), LayoutMode::Parallel).unwrap();
    let at_200 = solve(
        &tree,
        view,
        &m().with_scale_ppm(2_000_000),
        SeatId(1),
        LayoutMode::Parallel,
    )
    .unwrap();
    assert_eq!(at_100.logical_rects(), at_200.logical_rects());
    let d100 = at_100.get(SeatId(3)).unwrap().device_rect.unwrap();
    let d200 = at_200.get(SeatId(3)).unwrap().device_rect.unwrap();
    assert_eq!(d200.width(), d100.width() * 2);
}

#[test]
fn opening_the_preview_seat_narrows_the_root_run_and_nothing_else() {
    // A nested column split hangs off the root run; the preview's arrival is
    // allowed to narrow the run's columns and forbidden to touch that nest.
    let tree = row(1, term(1), col(2, term(2), term(3)));
    let view = viewport(1600, 900);
    let before = solved(&tree, view, 1);

    let out = apply(
        &tree,
        &m(),
        &Edit::LandPreview {
            seat: Seat::new(SeatId(4), SeatKind::Preview),
            split_id: SplitId(9),
        },
    )
    .expect("the preview can always be landed");
    assert!(necessity_holds(&tree, &out.tree, &out.focus_set));
    assert_eq!(
        ratio_of(&out.tree, 2),
        ratio_of(&tree, 2),
        "the nested col split is outside F"
    );
    assert!(!out.focus_set.contains(SplitId(2)));
    assert!(out.focus_set.contains(SplitId(1)));

    // The bill of a fixed address, stated in §3.3: the terminal columns narrow.
    let after = solved(&out.tree, view, 1);
    assert!(extent_px(&after, 1, Axis::Row) < extent_px(&before, 1, Axis::Row));
    // The nested pair keeps its own division exactly.
    assert_eq!(
        extent_px(&after, 2, Axis::Col),
        extent_px(&before, 2, Axis::Col)
    );
}

#[test]
fn reusing_the_unpinned_preview_seat_moves_no_rectangle() {
    let tree = row(1, term(1), seat(2, SeatKind::Preview));
    let view = viewport(1400, 800);
    let before = solved(&tree, view, 1);

    let out = apply(
        &tree,
        &m(),
        &Edit::LandPreview {
            seat: Seat::new(SeatId(3), SeatKind::Preview),
            split_id: SplitId(9),
        },
    )
    .expect("an unpinned preview seat is reused rather than multiplied");
    assert!(
        out.focus_set.is_empty(),
        "what changed is content, not geometry"
    );
    assert_eq!(out.tree, tree);
    assert_eq!(solved(&out.tree, view, 1), before);
    assert_eq!(out.tree.seat_count(), 2, "singleton semantics: no new seat");
}

#[test]
fn the_preview_seat_lands_left_of_a_trailing_files_column() {
    let tree = row(1, term(1), files(2));
    let out = apply(
        &tree,
        &m(),
        &Edit::LandPreview {
            seat: Seat::new(SeatId(3), SeatKind::Preview),
            split_id: SplitId(9),
        },
    )
    .expect("the preview can always be landed");

    let order: Vec<SeatId> = out.tree.seats_in_order().iter().map(|s| s.id).collect();
    assert_eq!(order, vec![SeatId(1), SeatId(3), SeatId(2)]);

    let layout = solved(&out.tree, viewport(1600, 800), 1);
    let preview = layout.get(SeatId(3)).unwrap().rect.unwrap();
    let files_col = layout.get(SeatId(2)).unwrap().rect.unwrap();
    assert!(
        preview.right <= files_col.left,
        "files is navigation and preview is content; content does not go outboard"
    );
    assert!(preview.extent(Axis::Row) >= MIN_PREVIEW_W);
}

#[test]
fn entering_and_leaving_focus_mode_rewrites_no_ratio() {
    let tree = row(1, row(2, term(1), term(2)), files(3));
    let view = viewport(1500, 900);
    let parallel = solved(&tree, view, 1);

    let entered = apply(&tree, &m(), &Edit::FocusModeToggled).expect("always applicable");
    assert!(entered.focus_set.is_empty());
    assert_eq!(entered.tree, tree);

    let staged = solve(
        &entered.tree,
        view,
        &m(),
        SeatId(1),
        LayoutMode::Focus { stage: SeatId(1) },
    )
    .expect("the stage fits this viewport");
    assert_eq!(staged.get(SeatId(1)).unwrap().rect, Some(view));
    assert!(staged.get(SeatId(2)).unwrap().rect.is_none());
    assert_eq!(
        staged.rects.len(),
        3,
        "every seat keeps its place in the tree"
    );

    let left = apply(&entered.tree, &m(), &Edit::FocusModeToggled).expect("always applicable");
    assert_eq!(left.tree, tree);
    // Restoring the parallel tree is not a feature that had to be written.
    assert_eq!(solved(&left.tree, view, 1), parallel);
}

#[test]
fn opening_or_moving_a_floating_surface_rewrites_no_ratio() {
    let tree = row(1, term(1), col(2, term(2), files(3)));
    let out = apply(&tree, &m(), &Edit::FloatingSurfaceChanged).expect("always applicable");
    assert!(out.focus_set.is_empty());
    assert_eq!(
        out.tree, tree,
        "a floating surface is not a seat and is not in the tree"
    );
}

// ------------------------------------------------- §2.6 the concession chain --

#[test]
fn a_squeeze_and_release_round_trips_to_the_same_rects() {
    let tree = row(1, row(2, term(1), term(2)), term(3));
    let roomy = viewport(1400, 800);
    let before = solved(&tree, roomy, 1);

    let squeezed = solved(&tree, viewport(600, 300), 1);
    assert_eq!(
        squeezed.rects.len(),
        before.rects.len(),
        "W1: nothing deleted"
    );
    let ids = |l: &SeatLayout| l.rects.iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(ids(&squeezed), ids(&before), "W1: nothing reordered");

    let released = solved(&tree, roomy, 1);
    assert_eq!(
        released, before,
        "bit-identical after losing and regaining space"
    );
}

#[test]
fn a_collapsed_seat_keeps_a_clickable_bar_and_its_place_in_the_tree() {
    let tree = row(1, term(1), row(2, term(2), term(3)));
    let layout = solved(&tree, viewport(600, 400), 1);

    let bar = layout.get(SeatId(2)).expect("still there");
    assert_eq!(bar.presentation, Presentation::Collapsed(AxisSet::ROW));
    let rect = bar.rect.expect("a collapsed seat still has a rectangle");
    assert_eq!(
        rect.extent(Axis::Row),
        COLLAPSED_EXTENT,
        "24 logical pixels of bar"
    );
    assert!(rect.is_non_degenerate(), "red line L4: never zero area");
    assert_eq!(
        rect.extent(Axis::Col).floor_px(),
        400,
        "the other axis still fills the slot"
    );

    let order: Vec<SeatId> = layout.rects.iter().map(|p| p.id).collect();
    assert_eq!(
        order,
        vec![SeatId(1), SeatId(2), SeatId(3)],
        "its place in the tree is kept"
    );
    assert_eq!(
        layout.get(SeatId(1)).unwrap().presentation,
        Presentation::Full,
        "the focus seat is the last to fall"
    );
}

/// A tree with two axes to give on, squeezed on both.
fn dual_axis_tree() -> LayoutNode {
    col(1, term(1), row(2, term(2), term(3)))
}

#[test]
fn a_seat_collapsed_on_both_axes_is_a_twentyfour_square_not_a_bar() {
    let layout = solved(&dual_axis_tree(), viewport(400, 200), 1);
    let square = layout.get(SeatId(2)).unwrap().rect.unwrap();
    assert_eq!(square.extent(Axis::Row), COLLAPSED_EXTENT);
    assert_eq!(square.extent(Axis::Col), COLLAPSED_EXTENT);

    // Its neighbour was judged on one axis only, so it is still a full-width bar
    // — the intersection of two independent verdicts, not a third rule.
    let bar = layout.get(SeatId(3)).unwrap();
    assert_eq!(bar.presentation, Presentation::Collapsed(AxisSet::COL));
    let bar = bar.rect.unwrap();
    assert_eq!(bar.extent(Axis::Col), COLLAPSED_EXTENT);
    assert!(bar.extent(Axis::Row) > COLLAPSED_EXTENT);
}

#[test]
fn a_double_collapsed_seat_is_a_color_block_with_only_a_state_icon() {
    // The crate owns the part a renderer can act on: which axes were squeezed.
    // The colour block's chrome — no border, no name, one state icon — is the
    // renderer's business.
    let layout = solved(&dual_axis_tree(), viewport(400, 200), 1);
    let square = layout.get(SeatId(2)).unwrap().presentation;
    assert_eq!(square, Presentation::Collapsed(AxisSet::BOTH));
    assert!(
        square.is_double_collapsed(),
        "distinguishable from a bar by type"
    );

    let bar = layout.get(SeatId(3)).unwrap().presentation;
    assert!(
        !bar.is_double_collapsed(),
        "a one-axis bar must not answer yes"
    );
    assert!(bar.is_collapsed_along(Axis::Col) && !bar.is_collapsed_along(Axis::Row));
}

#[test]
fn a_fixed_column_gives_its_own_slack_before_a_terminal_goes_below_its_minimum() {
    // L1 before L2 is a ruling, not a convenience: a fixed column declared a
    // 240..170 band of its own, and spending a flex seat's ground first would be
    // filling one's own hole with someone else's land.
    let tree = row(1, files(1), term(2));
    let layout = solved(&tree, viewport(481, 400), 2);
    assert_eq!(
        extent_px(&layout, 1, Axis::Row),
        220,
        "the files column gave the 20px"
    );
    assert_eq!(
        extent_px(&layout, 2, Axis::Row),
        MIN_PANE_W.floor_px(),
        "the terminal is still exactly at its own minimum"
    );

    // Roomy: the column sits at its opening width and gives nothing.
    let roomy = solved(&tree, viewport(1200, 400), 2);
    assert_eq!(extent_px(&roomy, 1, Axis::Row), FILES_W.floor_px());

    // The band is walked gradually, exactly as a hand drag would walk it: the
    // column gives what is asked for and not a pixel more, which is why L1 needs
    // no banner (tiny-window §2).
    assert_eq!(
        extent_px(&solved(&tree, viewport(432, 400), 2), 1, Axis::Row),
        171
    );

    // Squeezed to the end of the band: the column stops at its floor, never below.
    let tight = solved(&tree, viewport(431, 400), 2);
    assert_eq!(extent_px(&tight, 1, Axis::Row), FILES_W_MIN.floor_px());
    assert_eq!(extent_px(&tight, 2, Axis::Row), MIN_PANE_W.floor_px());
}

#[test]
fn the_trailing_side_of_a_double_fixed_row_absorbs_the_surplus() {
    // §2.3: with no flexible member in the row, someone must eat the remainder,
    // or it is a patch of white that can never be filled and never be clicked.
    let tree = row(1, files(1), files(2));
    let view = viewport(1000, 600);
    let layout = solved(&tree, view, 1);
    assert_eq!(extent_px(&layout, 1, Axis::Row), FILES_W.floor_px());
    assert_eq!(extent_px(&layout, 2, Axis::Row), 759);
    assert_eq!(
        layout.get(SeatId(2)).unwrap().rect.unwrap().right,
        view.right,
        "no dead white at the trailing edge"
    );
}

#[test]
fn the_column_axis_skips_the_fixed_stage_and_runs_the_same_chain() {
    // §2.7: vertically there is no fixed class, so L1 is skipped and the chain
    // is L0 -> L2 -> L3 -> L4, word for word otherwise.
    let tree = col(1, files(1), term(2));
    let roomy = solved(&tree, viewport(800, 600), 2);
    assert_eq!(
        extent_px(&roomy, 1, Axis::Row),
        800,
        "a files column in a vertical slot fills the slot's width"
    );
    assert!(extent_px(&roomy, 1, Axis::Col) >= MIN_PANE_H.floor_px());

    // Both kinds share MIN_PANE_H, the one row of §2.1 that does not fork.
    let tight = solved(&tree, viewport(800, 241), 2);
    assert_eq!(extent_px(&tight, 1, Axis::Col), MIN_PANE_H.floor_px());
    assert_eq!(extent_px(&tight, 2, Axis::Col), MIN_PANE_H.floor_px());

    // Below the floor the column chain collapses the non-focus seat, 24 high.
    let collapsed = solved(&tree, viewport(800, 200), 2);
    assert_eq!(
        collapsed.get(SeatId(1)).unwrap().presentation,
        Presentation::Collapsed(AxisSet::COL)
    );
    assert_eq!(
        extent_px(&collapsed, 1, Axis::Col),
        COLLAPSED_EXTENT.floor_px()
    );
}

#[test]
fn the_focus_seat_is_the_last_to_fall_and_then_it_is_an_error() {
    let tree = row(1, term(1), row(2, term(2), term(3)));
    // Narrow enough that both non-focus seats must go, but the focus survives.
    let layout = solved(&tree, viewport(320, 400), 1);
    assert_eq!(
        layout.get(SeatId(1)).unwrap().presentation,
        Presentation::Full
    );
    assert!(matches!(
        layout.get(SeatId(2)).unwrap().presentation,
        Presentation::Collapsed(_)
    ));
    assert!(matches!(
        layout.get(SeatId(3)).unwrap().presentation,
        Presentation::Collapsed(_)
    ));

    // Below even that, the answer is an explicit error rather than a lying
    // rectangle: the caller renders fit-what-fits.
    let err = solve(
        &tree,
        viewport(200, 400),
        &m(),
        SeatId(1),
        LayoutMode::Parallel,
    );
    assert_eq!(err, Err(LayoutError::Unsatisfiable { axis: Axis::Row }));
}

#[test]
fn the_floor_is_the_focus_seats_own_minimum_plus_one_bar_per_sibling() {
    let four = row_run(4);
    let floor = floor_demand(&four, Axis::Row, &m(), SeatId(1));
    let expected = MIN_PANE_W
        + (COLLAPSED_EXTENT + DIVIDER)
        + (COLLAPSED_EXTENT + DIVIDER)
        + (COLLAPSED_EXTENT + DIVIDER);
    assert_eq!(floor, expected);

    // The focus seat's *own* kind, not a shared number.
    let with_preview = row(1, seat(1, SeatKind::Preview), term(2));
    assert_eq!(
        floor_demand(&with_preview, Axis::Row, &m(), SeatId(1)),
        MIN_PREVIEW_W + DIVIDER + COLLAPSED_EXTENT
    );

    // One seat has nothing to collapse: the floor is just its minimum. A
    // degenerate case, not an exception.
    let lone = term(1);
    assert_eq!(floor_demand(&lone, Axis::Row, &m(), SeatId(1)), MIN_PANE_W);
    assert_eq!(floor_demand(&lone, Axis::Col, &m(), SeatId(1)), MIN_PANE_H);
}

#[test]
fn the_window_minimum_follows_the_tree_and_never_exceeds_sixty_percent_of_the_workarea() {
    let tree = row_run(4);
    let chrome = LogicalSize::px(0, 40);
    let metrics = m();

    // Follows the tree when there is room for it to.
    let big = window_min_inner_size(
        &tree,
        &metrics,
        SeatId(1),
        chrome,
        WorkAreaHint::Known(LogicalSize::px(3840, 2160)),
    )
    .unwrap();
    assert_eq!(big.width.floor_px(), 4 * 260 + 3, "demand(root) exactly");

    // Clamped at 60% when the tree outgrows the monitor: a window that suddenly
    // refuses to be dragged smaller reads as a freeze.
    let clamped = window_min_inner_size(
        &tree,
        &metrics,
        SeatId(1),
        chrome,
        WorkAreaHint::Known(LogicalSize::px(1600, 900)),
    )
    .unwrap();
    assert_eq!(clamped.width.floor_px(), 960);
    assert!(clamped.width.floor_px() < 4 * 260 + 3);
    assert_eq!(clamped.height.floor_px(), MIN_PANE_H.floor_px() + 40);

    // tiny-window §4.2: the 60% ceiling must not become a missing floor. On a
    // tiny monitor the answer is the honest "folded all the way down, the tree
    // still needs this much".
    let tiny = window_min_inner_size(
        &tree,
        &metrics,
        SeatId(1),
        chrome,
        WorkAreaHint::Known(LogicalSize::px(400, 400)),
    )
    .unwrap();
    assert_eq!(
        tiny.width,
        floor_demand(&tree, Axis::Row, &metrics, SeatId(1)),
        "never below the floor the concession chain can actually reach"
    );
    assert!(tiny.width.floor_px() > 240, "which is above 60% of 400");

    // §4.4 ruling 2: with no work area ever observed, set no hint at all rather
    // than lock the window with a guess.
    assert_eq!(
        window_min_inner_size(&tree, &metrics, SeatId(1), chrome, WorkAreaHint::NeverKnown),
        None
    );
}

// -------------------------------------------------------- §5 persistence --

#[test]
fn a_saved_layout_reloads_to_the_same_rects() {
    let tree = apply(
        &row(1, term(1), term(2)),
        &m(),
        &Edit::SplitSeat {
            target: SeatId(2),
            dir: Axis::Row,
            leading: false,
            arriving: LayoutNode::seat(Seat::new(SeatId(3), SeatKind::Terminal)),
            split_id: SplitId(2),
        },
    )
    .expect("seed a run whose ratio is not a round number")
    .tree;
    assert_eq!(ratio_of(&tree, 1).ppm(), 333_333);

    let view = viewport(1080, 700);
    let before = solved(&tree, view, 1);

    // Persist the canonical u32 and read it straight back.
    let reloaded = rebuild_with(&tree, |r| Ratio::from_ppm(r.ppm()).expect("in domain"));
    assert_eq!(solved(&reloaded, view, 1), before);

    // The red gate this constraint exists for: three decimal places is lossy,
    // and on a multi-column layout the loss is visible.
    let lossy = rebuild_with(&tree, |r| {
        Ratio::clamped_from_ppm((r.ppm() + 500) / 1_000 * 1_000)
    });
    assert_ne!(
        solved(&lossy, view, 1),
        before,
        "if this ever passes, the round-trip pin has stopped being able to fail"
    );
}

fn rebuild_with(node: &LayoutNode, f: impl Fn(Ratio) -> Ratio + Copy) -> LayoutNode {
    match node {
        LayoutNode::Seat(s) => LayoutNode::seat(s.clone()),
        LayoutNode::Split {
            id,
            dir,
            ratio,
            a,
            b,
        } => LayoutNode::split_at(*id, *dir, f(*ratio), rebuild_with(a, f), rebuild_with(b, f)),
    }
}

#[test]
fn an_unknown_kind_degrades_per_leaf_into_a_visible_placeholder() {
    // §5 constraint 2: one leaf this build cannot name must not cost the tree,
    // and must not be silently turned into a terminal.
    let tree = row(1, term(1), seat(2, SeatKind::Placeholder));
    let layout = solved(&tree, viewport(1000, 600), 1);
    let ghost = layout.get(SeatId(2)).unwrap();
    assert_eq!(ghost.kind, SeatKind::Placeholder);
    assert!(ghost.rect.unwrap().is_non_degenerate());

    // An out-of-domain persisted ratio is clamped, not fatal.
    assert_eq!(Ratio::from_ppm(0), None);
    assert_eq!(Ratio::clamped_from_ppm(0).ppm(), 1);
    assert_eq!(Ratio::clamped_from_ppm(4_000_000).ppm(), 999_999);
}

// ------------------------------------------------------------- §3.4 drags --

#[test]
fn a_drag_tighter_than_the_minimums_is_refused_with_zero_side_effects() {
    let tree = row(1, term(1), term(2));
    // 500 usable leaves 240 for the other side of a 260 minimum: infeasible.
    let refused = apply(
        &tree,
        &m(),
        &Edit::DragDivider {
            split: SplitId(1),
            requested: Ratio::from_ppm(900_000).unwrap(),
            usable: LogicalPx::px(500),
        },
    );
    assert_eq!(refused, Err(EditError::Refused));

    // Feasible: clamped into the band, and only that one split is in F.
    let out = apply(
        &tree,
        &m(),
        &Edit::DragDivider {
            split: SplitId(1),
            requested: Ratio::from_ppm(950_000).unwrap(),
            usable: LogicalPx::px(1000),
        },
    )
    .expect("1000 usable leaves both sides their minimum");
    assert_eq!(out.focus_set.splits(), &[SplitId(1)]);
    assert_eq!(out.tree.ratios().len(), 1);
    assert_eq!(
        ratio_of(&out.tree, 1).ppm(),
        740_000,
        "clamped by the other side's demand"
    );
}

#[test]
fn a_stacked_pair_of_fixed_columns_has_no_single_width_to_drag() {
    // §3.4: refuse honestly rather than write a share onto a fixed slot.
    let stacked = row(1, col(2, files(1), files(2)), term(3));
    assert_eq!(
        apply(
            &stacked,
            &m(),
            &Edit::DragFixedExtent {
                split: SplitId(1),
                requested: LogicalPx::px(200),
                usable: LogicalPx::px(1000),
            }
        ),
        Err(EditError::Refused)
    );
    // A ratio drag on that same split is refused too — the slot is fixed.
    assert_eq!(
        apply(
            &stacked,
            &m(),
            &Edit::DragDivider {
                split: SplitId(1),
                requested: Ratio::HALF,
                usable: LogicalPx::px(1000),
            }
        ),
        Err(EditError::Refused)
    );

    // A bare files leaf does drag, by pixels, and rewrites no ratio.
    let bare = row(1, files(1), term(2));
    let out = apply(
        &bare,
        &m(),
        &Edit::DragFixedExtent {
            split: SplitId(1),
            requested: LogicalPx::px(120),
            usable: LogicalPx::px(1000),
        },
    )
    .expect("a bare fixed leaf has exactly one width");
    assert!(out.focus_set.is_empty());
    assert_eq!(out.tree.ratios(), bare.ratios());
    assert_eq!(
        extent_px(&solved(&out.tree, viewport(1001, 600), 2), 1, Axis::Row),
        FILES_W_MIN.floor_px(),
        "clamped up to the column's floor"
    );
}

// ------------------------------------------------------- red lines L4/L6 --

#[test]
fn adjacent_seats_share_one_boundary_and_a_long_chain_does_not_drift() {
    // Boundary rounding, not width rounding: a fractional scale over a long run
    // is exactly where per-width rounding accumulates a visible fake divider.
    let tree = row_run(6);
    let metrics = m().with_scale_ppm(1_250_000);
    let view = viewport(1601, 900);
    let layout = solve(&tree, view, &metrics, SeatId(1), LayoutMode::Parallel).unwrap();

    let placed: Vec<_> = layout.presented().collect();
    for pair in placed.windows(2) {
        let (left, right) = (pair[0].1, pair[1].1);
        assert_eq!(right.left - left.right, DIVIDER, "one divider, exactly");
        let (dl, dr) = (
            pair[0].0.device_rect.unwrap(),
            pair[1].0.device_rect.unwrap(),
        );
        assert!(dr.left >= dl.right, "no overlap on the device grid");
        assert!(dr.left - dl.right <= 2, "no gap beyond the divider itself");
    }
    let last = placed.last().unwrap().0.device_rect.unwrap();
    assert_eq!(
        last.right, 2001,
        "the chain still ends exactly at the viewport edge"
    );
}

#[test]
fn no_solved_rectangle_is_ever_zero_area() {
    let tree = col(1, row(2, term(1), files(2)), row(3, term(3), term(4)));
    let mut solved_any = false;
    for w in [2000, 1200, 900, 700, 500, 400, 300] {
        for h in [1200, 800, 500, 300, 200, 150] {
            match solve(&tree, viewport(w, h), &m(), SeatId(1), LayoutMode::Parallel) {
                Ok(layout) => {
                    solved_any = true;
                    for (placement, rect) in layout.presented() {
                        assert!(
                            rect.is_non_degenerate(),
                            "{:?} came out degenerate at {w}x{h}",
                            placement.id
                        );
                    }
                }
                // The only other legal answer (tiny-window §4.3).
                Err(LayoutError::Unsatisfiable { .. }) => {}
                Err(other) => panic!("unexpected {other:?}"),
            }
        }
    }
    assert!(
        solved_any,
        "the sweep must actually exercise the success path"
    );
}

// ------------------------------------------------- §3.3 vocabulary on trees --

#[test]
fn the_run_vocabulary_answers_on_the_tree_alone() {
    // No geometry is an input to any of these, so none of them can drift with
    // the window size — which is why the two concession chains can share one
    // collapse order (tiny-window §1.3).
    let tree = row(1, term(1), col(2, row(3, term(2), term(3)), term(4)));

    // A run stops at the first cross-direction split, so seats 2 and 3 are in a
    // run of their own rather than in the root's.
    assert_eq!(
        members(&tree, SeatId(2), Axis::Row),
        vec![SeatId(2), SeatId(3)]
    );
    assert_eq!(
        run_split_ids(
            &tree,
            &run_root_path_of_seat(&tree, SeatId(2), Axis::Row).unwrap(),
            Axis::Row
        ),
        vec![SplitId(3)]
    );
    assert_eq!(
        run_root_path_of_seat(&tree, SeatId(1), Axis::Row).unwrap(),
        Vec::new(),
        "seat 1 sits in the root run"
    );
    assert!(path_to_seat(&tree, SeatId(9)).is_none());

    // A column split occupies one slot but needs as many columns as its widest
    // member — "equal" means every column, not every node.
    assert_eq!(run_demand(&tree, Axis::Row), 3);
    assert_eq!(run_demand(&tree, Axis::Col), 2);

    // Distance is measured outside the common prefix.
    assert_eq!(tree_distance(&tree, SeatId(2), SeatId(3)), Some(2));
    assert_eq!(tree_distance(&tree, SeatId(1), SeatId(4)), Some(3));
    assert_eq!(in_order_index(&tree, SeatId(3)), Some(2));
    assert_eq!(
        collapse_order(&tree, SeatId(1)),
        vec![SeatId(2), SeatId(3), SeatId(4)],
        "farthest first, ties broken by in-order position"
    );

    // Demand sums along its own axis and maxes across it (§2.2).
    assert_eq!(demand_at_min(&tree, Axis::Row).floor_px(), 782);
    assert_eq!(demand(&tree, Axis::Col, &m()).floor_px(), 241);

    // Share is the product of the side shares down the path.
    assert_eq!(share_ppm(&tree, SeatId(1)), Some(500_000));
    assert_eq!(share_ppm(&tree, SeatId(4)), Some(250_000));
}

fn demand_at_min(tree: &LayoutNode, axis: Axis) -> LogicalPx {
    bt_layout::demand_at_min(tree, axis, &m())
}
