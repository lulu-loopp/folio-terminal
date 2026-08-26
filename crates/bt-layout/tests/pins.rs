//! Every pinned test name that `docs/M2-layout-solver-spec.md` and
//! `docs/M2-tiny-window-priority.md` declare for solver logic, plus the
//! determinism properties D1/D2 ask for.
//!
//! These live in `tests/` so they can only reach the public API — the same
//! physical enforcement CONVENTIONS §three asks of the gate tests.

use bt_layout::{
    Axis, AxisSet, COLLAPSED_EXTENT, DIVIDER, Edit, EditError, FILES_W, FILES_W_MIN, Landing,
    LayoutError, LayoutMode, LayoutNode, LogicalPx, LogicalRect, LogicalSize, MIN_PANE_H,
    MIN_PANE_W, MIN_PREVIEW_W, Presentation, Ratio, Seat, SeatId, SeatKind, SeatLayout,
    SeatMetrics, SizePolicy, SplitId, WorkAreaHint, apply, collapse_order, demand, floor_demand,
    in_order_index, members, necessity_holds, path_to_seat, run_demand, run_root_path_of_seat,
    run_split_ids, share_ppm, solve, tree_distance, window_min_inner_size,
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

/// The program's own layouts, where every minimum is law and the whole
/// concession chain of §2.6.1 is in force.
fn solved(tree: &LayoutNode, view: LogicalRect, focus: u64) -> SeatLayout {
    solve(
        tree,
        view,
        &m(),
        SeatId(focus),
        LayoutMode::Parallel,
        SizePolicy::Lawful,
    )
    .expect("layout should be solvable")
}

/// A rectangle the user's own hand chose, where the same minima are advice.
fn solved_by_hand(tree: &LayoutNode, view: LogicalRect, focus: u64) -> SeatLayout {
    solve(
        tree,
        view,
        &m(),
        SeatId(focus),
        LayoutMode::Parallel,
        SizePolicy::Sovereign,
    )
    .expect("a rectangle the user chose is never refused")
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
    // Both policies: the ruling added an input to `solve`, not an escape from D1.
    for policy in [SizePolicy::Lawful, SizePolicy::Sovereign] {
        for (w, h) in [(1400, 900), (700, 400), (420, 260), (1, 1)] {
            let once = |_| {
                solve(
                    &tree,
                    viewport(w, h),
                    &m(),
                    SeatId(1),
                    LayoutMode::Parallel,
                    policy,
                )
            };
            assert_eq!(
                once(()),
                once(()),
                "same input must give bit-identical output"
            );
        }
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
    let at_100 = solved(&tree, view, 1);
    let at_200 = solve(
        &tree,
        view,
        &m().with_scale_ppm(2_000_000),
        SeatId(1),
        LayoutMode::Parallel,
        SizePolicy::Lawful,
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
        SizePolicy::Lawful,
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

fn preview(id: u64) -> LayoutNode {
    seat(id, SeatKind::Preview)
}

/// §2.6.1 L3, **用户裁决 2026-08-13**: the class leads the distance.
///
/// The order this asserts is the whole ruling — preview, then files, then the
/// terminal — and the tree is arranged so that *distance alone would answer the
/// other way round*: the focus sits on the preview at the far left, so the
/// terminal is the seat farthest from it and the old order folded exactly the
/// pane holding somebody's work.
///
/// Red gate: drop `collapse_rank` out of `collapse_order`'s sort key and this
/// goes red on the first element.
#[test]
fn a_narrowing_window_folds_the_preview_before_the_files_column_and_the_terminal_last() {
    // preview(1) | ( files(2) | ( preview(3) | terminal(4) ) )
    let tree = row(1, preview(1), row(2, files(2), row(3, preview(3), term(4))));

    assert_eq!(
        collapse_order(&tree, SeatId(1)),
        vec![SeatId(3), SeatId(2), SeatId(4)],
        "previews first (farthest of them leading), then files, then the terminal"
    );

    // And the same order is what the rectangles say. Wide enough for everything,
    // then narrowed a step at a time: each step must take the next seat on the
    // list and no other.
    let folded = |width: i64| -> Vec<SeatId> {
        let layout = solve(
            &tree,
            viewport(width, 700),
            &m(),
            SeatId(1),
            LayoutMode::Parallel,
            SizePolicy::Lawful,
        )
        .expect("a fold is an answer, not a failure");
        layout
            .rects
            .iter()
            .filter(|p| p.presentation.is_collapsed_along(Axis::Row))
            .map(|p| p.id)
            .collect()
    };

    assert_eq!(folded(1400), Vec::<SeatId>::new(), "room for all four");
    assert_eq!(folded(1100), vec![SeatId(3)], "the far preview folds first");
    assert_eq!(
        folded(800),
        vec![SeatId(2), SeatId(3)],
        "then the files column"
    );
    assert_eq!(
        folded(560),
        vec![SeatId(2), SeatId(3), SeatId(4)],
        "the terminal is the last non-focus seat to fall"
    );
}

/// The class order does not replace the old rule, it sits above it: inside one
/// class, farthest-from-focus still decides, which is what keeps L3 a total
/// order and therefore deterministic (§2.6.1's third reading note).
#[test]
fn within_one_content_class_the_farthest_from_the_focus_still_folds_first() {
    let tree = row(1, term(1), row(2, term(2), row(3, term(3), term(4))));
    assert_eq!(
        collapse_order(&tree, SeatId(1)),
        vec![SeatId(3), SeatId(4), SeatId(2)],
        "one class, so distance alone decides — and ties break by in-order position"
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
        SizePolicy::Lawful,
    );
    assert_eq!(err, Err(LayoutError::Unsatisfiable { axis: Axis::Row }));
}

// ------------------------- §2.6.6 the two authorities (ruling 2026-08-08) --

/// Every seat is presented at full size, and the row they make tiles the
/// viewport exactly: one divider between neighbours, first flush left, last
/// flush right. A gap anywhere in that chain is the "中间夹缝" the ruling names.
fn assert_tiles_without_a_seam(layout: &SeatLayout, view: LogicalRect, what: &str) {
    let placed: Vec<_> = layout.presented().collect();
    assert_eq!(
        placed.len(),
        layout.rects.len(),
        "{what}: a seat went missing"
    );
    for (placement, rect) in &placed {
        assert_eq!(
            placement.presentation,
            Presentation::Full,
            "{what}: {:?} folded",
            placement.id
        );
        assert!(
            rect.right >= rect.left,
            "{what}: {:?} inverted",
            placement.id
        );
        assert!(
            rect.bottom >= rect.top,
            "{what}: {:?} inverted",
            placement.id
        );
    }
    assert_eq!(
        placed[0].1.left, view.left,
        "{what}: a gap at the leading edge"
    );
    assert_eq!(
        placed.last().unwrap().1.right,
        view.right,
        "{what}: a gap at the trailing edge"
    );
    for pair in placed.windows(2) {
        assert_eq!(
            pair[1].1.left - pair[0].1.right,
            DIVIDER,
            "{what}: a seam between two seats"
        );
    }
}

#[test]
fn a_hand_narrowing_the_window_scales_the_panes_and_folds_nothing() {
    // Four columns want 1043px. The ruling says the hand may go on past that,
    // all the way down, and see four narrowing panes rather than a row of bars.
    let tree = row_run(4);
    let mut previous = i64::MAX;
    for w in (40..=1400).rev() {
        let view = viewport(w, 600);
        let layout = solved_by_hand(&tree, view, 1);
        assert_tiles_without_a_seam(&layout, view, &format!("{w}px"));
        let first = extent_px(&layout, 1, Axis::Row);
        assert!(
            first <= previous,
            "a narrower window gave seat 1 a wider pane: {first} at {w}px after {previous}"
        );
        previous = first;
    }

    // At the bottom the four equal shares are four equal panes: the floors have
    // relaxed out of the way entirely and the ratios are all that is left.
    let view = viewport(101, 600);
    let bottom = solved_by_hand(&tree, view, 1);
    let widths: Vec<i64> = (1..=4)
        .map(|id| extent_px(&bottom, id, Axis::Row))
        .collect();
    let (lo, hi) = (*widths.iter().min().unwrap(), *widths.iter().max().unwrap());
    assert!(
        hi - lo <= 1,
        "four equal shares should divide a small room equally, got {widths:?}"
    );
    // And account for every subpixel: four panes plus three dividers is the
    // whole room. Summing the floored widths would lose each pane's remainder,
    // which is exactly the accounting error a seam is made of.
    let exact: i64 = bottom
        .presented()
        .map(|(_, rect)| rect.extent(Axis::Row).subpixels())
        .sum();
    assert_eq!(
        exact + 3 * DIVIDER.subpixels(),
        view.extent(Axis::Row).subpixels(),
        "the panes and their dividers are the viewport, exactly"
    );
}

#[test]
fn a_fixed_column_under_the_hand_gives_up_its_floor_too() {
    // 170 is the files column's floor under law. Under the hand it is a
    // preference, and a preference does not get to be the last thing standing
    // while the terminal beside it is crushed.
    let tree = row(1, files(1), term(2));
    let wide = solved_by_hand(&tree, viewport(1200, 600), 2);
    assert_eq!(extent_px(&wide, 1, Axis::Row), FILES_W.floor_px());

    let squeezed = solved_by_hand(&tree, viewport(200, 600), 2);
    let files_w = extent_px(&squeezed, 1, Axis::Row);
    assert!(
        files_w < FILES_W_MIN.floor_px(),
        "the column should be under its floor at 200px, got {files_w}"
    );
    assert!(files_w > 0, "and still a real column, got {files_w}");
}

#[test]
fn a_rectangle_the_user_chose_is_never_refused() {
    let tree = row(1, term(1), row(2, term(2), term(3)));
    for w in [200, 120, 60, 24, 8, 3, 1] {
        for h in [400, 120, 40, 8, 1] {
            let view = viewport(w, h);
            assert!(
                solve(
                    &tree,
                    view,
                    &m(),
                    SeatId(1),
                    LayoutMode::Parallel,
                    SizePolicy::Sovereign,
                )
                .is_ok(),
                "{w}x{h} was refused to the hand that made it"
            );
            // Focus mode has the same two authorities.
            assert!(
                solve(
                    &tree,
                    view,
                    &m(),
                    SeatId(1),
                    LayoutMode::Focus { stage: SeatId(1) },
                    SizePolicy::Sovereign,
                )
                .is_ok(),
                "{w}x{h} was refused a stage"
            );
        }
    }
    // The same rectangle, asked for by the program, still gets the honest no.
    assert_eq!(
        solve(
            &tree,
            viewport(60, 40),
            &m(),
            SeatId(1),
            LayoutMode::Parallel,
            SizePolicy::Lawful,
        ),
        Err(LayoutError::Unsatisfiable { axis: Axis::Row })
    );
}

#[test]
fn the_two_authorities_answer_the_same_rectangle_differently() {
    // One tree, one viewport, two callers. This is the ruling in a single
    // assertion pair: delete the policy branch in `plan_axis` and one of these
    // two goes red whichever way the branch is deleted.
    let tree = row_run(3);
    let view = viewport(400, 600);

    let lawful = solved(&tree, view, 1);
    assert!(
        lawful
            .rects
            .iter()
            .any(|p| matches!(p.presentation, Presentation::Collapsed(_))),
        "the program's own layout still folds"
    );

    let sovereign = solved_by_hand(&tree, view, 1);
    assert!(
        sovereign
            .rects
            .iter()
            .all(|p| p.presentation == Presentation::Full),
        "the user's own window does not"
    );
    assert_tiles_without_a_seam(&sovereign, view, "400px by hand");
}

#[test]
fn a_squeeze_by_hand_and_release_round_trips_to_the_same_rects() {
    // W1 tree conservation is not a Lawful-only promise. Relaxing the floors is
    // a presentation decision like collapsing is, and it has to be as reversible.
    let tree = col(1, row(2, term(1), files(2)), row(3, term(3), term(4)));
    let roomy = viewport(1400, 900);
    let before = solved_by_hand(&tree, roomy, 1);
    let _ = solved_by_hand(&tree, viewport(90, 70), 1);
    let after = solved_by_hand(&tree, roomy, 1);
    assert_eq!(before, after, "the squeeze left a trace");
}

#[test]
fn no_sovereign_rectangle_is_inverted_even_under_its_own_dividers() {
    // Eight columns in 20 logical pixels: the seven dividers alone outweigh the
    // room. The floors relax to nothing and the cuts pile up, which is a legal
    // picture of an absurd window — but every rectangle still has its near edge
    // on the correct side of its far edge.
    let tree = row_run(8);
    for w in [20, 10, 7, 4, 2, 1] {
        let view = viewport(w, 30);
        let layout = solved_by_hand(&tree, view, 1);
        for (placement, rect) in layout.presented() {
            assert!(
                rect.right >= rect.left && rect.bottom >= rect.top,
                "{:?} inverted at {w}px: {rect:?}",
                placement.id
            );
        }
    }
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
fn the_window_minimum_is_one_pane_and_never_the_fleets_demand() {
    let chrome = LogicalSize::px(0, 40);
    let metrics = m();
    let desk = WorkAreaHint::Known(LogicalSize::px(3840, 2160));

    // The technical floor: one terminal leaf plus chrome, and nothing else.
    let min = window_min_inner_size(&metrics, chrome, desk);
    assert_eq!(min.width, MIN_PANE_W);
    assert_eq!(min.height.floor_px(), MIN_PANE_H.floor_px() + 40);

    // **The ruling, as one assertion.** Four columns want 1043px and are welcome
    // to want it; the OS is told 260 all the same, so the hand can keep going and
    // the panes shrink in proportion behind it. Put the fleet's demand back here
    // — `demand(root) + chrome`, or a max over every tab's tree — and this line
    // is the one that goes red.
    let four = row_run(4);
    assert_eq!(
        demand(&four, Axis::Row, &metrics).floor_px(),
        4 * 260 + 3,
        "the demand still exists and is still what Lawful layouts answer to"
    );
    for tree in [term(1), row_run(2), four, col(9, term(1), files(2))] {
        let _ = &tree;
        assert_eq!(
            window_min_inner_size(&metrics, chrome, desk),
            min,
            "the window minimum is not a function of the tree at all"
        );
    }

    // The 60% clamp survives for the case it was written for: a monitor small
    // enough that even one pane is more than a window should demand.
    let clamped = window_min_inner_size(
        &metrics,
        chrome,
        WorkAreaHint::Known(LogicalSize::px(400, 300)),
    );
    assert_eq!(clamped.width.floor_px(), 240, "60% of 400, below one pane");
    assert_eq!(
        clamped.height.floor_px(),
        160,
        "the clamp binds per axis: 60% of 300 is 180, and the floor is under it"
    );

    // No work area ever observed is no longer a reason to set no minimum: the
    // floor is a constant, and it needs nothing to be trustworthy. Leaving the
    // window unbounded is exactly the 157x25 rectangle this line refuses.
    assert_eq!(
        window_min_inner_size(&metrics, chrome, WorkAreaHint::NeverKnown),
        min
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

/// User ruling 2026-08-08. The name is the overturned pin's own, negated: this
/// used to be `a_drag_tighter_than_the_minimums_is_refused_with_zero_side_effects`.
#[test]
fn a_drag_tighter_than_the_minimums_is_the_users_to_make() {
    let tree = row(1, term(1), term(2));
    let drag = |requested: u32, usable: i64| {
        apply(
            &tree,
            &m(),
            &Edit::DragDivider {
                split: SplitId(1),
                requested: Ratio::from_ppm(requested).unwrap(),
                usable: LogicalPx::px(usable),
            },
        )
    };

    // 500 usable leaves 50px for the other side of a 260 minimum. The old rule
    // called that infeasible and made the divider go dead under the hand; the
    // ruling calls it a 50px pane, which is what the user asked for.
    let out = drag(900_000, 500).expect("a hand is not refused for wanting a narrow pane");
    assert_eq!(ratio_of(&out.tree, 1).ppm(), 900_000, "the ratio asked for");
    assert_eq!(
        out.focus_set.splits(),
        &[SplitId(1)],
        "L9: one split, no rebalance"
    );

    // The same on a roomy slot, where the old clamp used to bite at 740_000.
    let wide = drag(950_000, 1000).expect("always applicable");
    assert_eq!(
        ratio_of(&wide.tree, 1).ppm(),
        950_000,
        "no demand-derived ceiling is left to clamp against"
    );

    // What the type still forbids is a side of exactly nothing (红线 L4).
    let extreme = drag(1, 500).expect("still a legal ratio");
    assert_eq!(ratio_of(&extreme.tree, 1).ppm(), 1);
    assert!(ratio_of(&extreme.tree, 1).ppm() > 0);

    // Esc: the restore runs back through the very same edit, so the mechanism
    // the app relies on has to be applicable at a ratio the drag reached.
    let back = apply(
        &out.tree,
        &m(),
        &Edit::DragDivider {
            split: SplitId(1),
            requested: Ratio::HALF,
            usable: LogicalPx::px(500),
        },
    )
    .expect("the origin ratio is restorable from anywhere the drag went");
    assert_eq!(ratio_of(&back.tree, 1), Ratio::HALF);
}

/// The program's side of the same coin: a drop is still judged, and still
/// refused. `plan_fits` lives in `bt-app`, so what this pin holds is the input
/// it judges — a `Lawful` solve that concedes rather than lies.
#[test]
fn a_fixed_column_drag_goes_where_the_hand_goes_but_never_leaves_its_slot() {
    let tree = row(1, files(1), term(2));
    let drag = |requested: i64, usable: i64| {
        apply(
            &tree,
            &m(),
            &Edit::DragFixedExtent {
                split: SplitId(1),
                requested: LogicalPx::px(requested),
                usable: LogicalPx::px(usable),
            },
        )
        .expect("a bare fixed leaf has a width to drag")
        .tree
        .find_seat(SeatId(1))
        .and_then(|s| s.fixed_extent)
        .expect("the drag wrote a width")
    };

    // Under FILES_W_MIN, which the old rule floored at 170.
    assert_eq!(drag(60, 1000), LogicalPx::px(60));
    // Past the other side's demand, which the old rule capped at usable - 260.
    assert_eq!(drag(900, 1000), LogicalPx::px(900));
    // Out of the slot, which is arithmetic rather than preference.
    assert_eq!(drag(4000, 1000), LogicalPx::px(1000));
    assert_eq!(drag(-50, 1000), LogicalPx::ZERO);
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
    let layout = solve(
        &tree,
        view,
        &metrics,
        SeatId(1),
        LayoutMode::Parallel,
        SizePolicy::Lawful,
    )
    .unwrap();

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
            match solve(
                &tree,
                viewport(w, h),
                &m(),
                SeatId(1),
                LayoutMode::Parallel,
                SizePolicy::Lawful,
            ) {
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

// ------------------------------ §3.3 a pane re-placed carries its own share --

/// The three-column layout the 2026-08-25 report was made against: a files
/// column on the left and two terminals beside it.
fn files_and_two_terminals() -> LayoutNode {
    row(1, files(1), row(2, term(2), term(3)))
}

fn dragged(tree: &LayoutNode, split: u64, ppm: u32, usable: i64) -> LayoutNode {
    apply(
        tree,
        &m(),
        &Edit::DragDivider {
            split: SplitId(split),
            requested: Ratio::from_ppm(ppm).expect("a legal ratio"),
            usable: LogicalPx::px(usable),
        },
    )
    .expect("the divider is draggable")
    .tree
}

fn move_pane(tree: &LayoutNode, seat: u64, landing: Landing) -> LayoutNode {
    apply(
        tree,
        &m(),
        &Edit::MoveSeat {
            seat: SeatId(seat),
            landing,
            split_id: SplitId(90),
        },
    )
    .expect("a pane of this tree has somewhere to go")
    .tree
}

fn row_widths(layout: &SeatLayout) -> Vec<i64> {
    layout
        .rects
        .iter()
        .map(|p| {
            p.rect
                .map(|r| r.extent(Axis::Row).floor_px())
                .expect("seat should be presented")
        })
        .collect()
}

fn left_rim() -> Landing {
    Landing::Rim {
        dir: Axis::Row,
        leading: true,
    }
}

fn before(target: u64) -> Landing {
    Landing::Edge {
        target: SeatId(target),
        dir: Axis::Row,
        leading: true,
    }
}

/// **用户裁决 2026-08-25 ①.** Picking a pane up and putting it down again
/// re-divides nothing between the panes it did not land beside.
///
/// The report, to the pixel: a 1600px window holding `[files | mid | right]`
/// draws 240/679/679, and dragging the files column back into its own band came
/// back **240/824/533** — two siblings that were equal, that nobody had aimed
/// at, left in a proportion nobody chose. The cause was the drag being spelled
/// as a close plus a split, each of which re-divides its run *by column count*:
/// the fixed band was counted as one of the columns being divided, while the
/// allocator was charging its 240px to whichever side of the tree it had come
/// to hang on.
///
/// Red gate: put the `CloseSeat` + `SplitSeat` chain back under a pane that is
/// already in the tree, or count fixed bands in `flex_run_demand`, and this goes
/// red on the very numbers the report carried.
#[test]
fn a_re_placed_pane_leaves_its_untouched_siblings_in_proportion() {
    let view = viewport(1600, 900);
    let tree = files_and_two_terminals();
    assert_eq!(row_widths(&solved(&tree, view, 2)), vec![240, 679, 679]);

    // Both landings that mean "back into the band it came from": the window's
    // own rim, and the leading edge of the pane that is now leftmost.
    for landing in [left_rim(), before(2)] {
        let after = move_pane(&tree, 1, landing);
        assert_eq!(
            row_widths(&solved(&after, view, 2)),
            vec![240, 679, 679],
            "{landing:?} redistributed two siblings it never touched"
        );
    }

    // And the same with a proportion the user set by hand, which is where the
    // rebalance used to be most visible: 65:35 came back 50:50.
    let hand = dragged(&tree, 2, 650_000, 1349);
    assert_eq!(row_widths(&solved(&hand, view, 2)), vec![240, 882, 475]);
    for landing in [left_rim(), before(2)] {
        let after = move_pane(&hand, 1, landing);
        assert_eq!(
            row_widths(&solved(&after, view, 2)),
            vec![240, 882, 475],
            "{landing:?} re-divided a hand-set proportion between untouched siblings"
        );
    }

    // A move that really does change the order still costs the untouched
    // siblings nothing: the two terminals keep 882:475 with the band standing
    // between them instead of before them.
    let between = move_pane(&hand, 1, before(3));
    assert_eq!(row_widths(&solved(&between, view, 2)), vec![882, 240, 475]);
}

/// **用户裁决 2026-08-25 ②.** A pane let go in the gap it came out of is
/// not an edit: the tree comes back value for value and `F` is empty.
///
/// It holds for both spellings of that gap — the rim and the near edge of the
/// pane beside it — because a run's *shape* carries no geometry: the columns are
/// re-ordered inside the skeleton the run already had, so an order that did not
/// change leaves the tree bit-identical, split ids and all. That is what makes
/// the promise hold even where a pane is sitting on its own floor, where which
/// neighbour pays for that floor is decided by the slot rather than by the
/// share.
#[test]
fn a_pane_let_go_where_it_was_moves_nothing_at_all() {
    let view = viewport(1600, 900);
    let hand = dragged(&files_and_two_terminals(), 2, 650_000, 1349);
    // A run leaning the other way, hand-dragged until its middle terminal is
    // pinned to its own minimum.
    let leaning = dragged(&row(1, row(2, term(1), term(2)), term(3)), 2, 700_000, 999);
    assert_eq!(row_widths(&solved(&leaning, view, 1)), vec![539, 260, 799]);

    for (tree, travelling, neighbour) in [(hand, 1u64, 2u64), (leaning, 1, 2)] {
        for landing in [left_rim(), before(neighbour)] {
            let outcome = apply(
                &tree,
                &m(),
                &Edit::MoveSeat {
                    seat: SeatId(travelling),
                    landing,
                    split_id: SplitId(90),
                },
            )
            .expect("a pane of this tree has somewhere to go");
            assert_eq!(
                outcome.tree, tree,
                "{landing:?} rewrote a tree it was asked to leave alone"
            );
            assert!(
                outcome.focus_set.is_empty(),
                "{landing:?} claimed the right to rewrite a ratio"
            );
        }
    }
}

/// **§2.3, wherever it sits.** A fixed band spends pixels and the ratios divide
/// what is left — including when the band is nested beside a flex pane rather
/// than hanging straight off a split.
///
/// The rule used to be written only for a subtree that was fixed *entirely*, so
/// the same three columns solved to two different sets of widths depending on
/// which way the tree leaned. Shape is not something the user can see, and it
/// must not be something the widths can.
#[test]
fn a_fixed_column_takes_pixels_wherever_it_sits_in_the_tree() {
    let view = viewport(1600, 900);
    // The same three columns, written both ways round.
    let leaning_right = row(1, files(1), row(2, term(2), term(3)));
    let leaning_left = row(1, row(2, files(1), term(2)), term(3));
    assert_eq!(
        row_widths(&solved(&leaning_right, view, 2)),
        row_widths(&solved(&leaning_left, view, 2)),
        "the same run came out at different widths for leaning the other way"
    );

    // And the ratio a rebalance writes is about the columns that take a share,
    // so the terminals come out equal however the band is nested.
    let rebalanced = apply(
        &leaning_left,
        &m(),
        &Edit::SplitSeat {
            target: SeatId(3),
            dir: Axis::Row,
            leading: false,
            arriving: term(4),
            split_id: SplitId(3),
        },
    )
    .expect("a split at the end of the run")
    .tree;
    let widths = row_widths(&solved(&rebalanced, view, 2));
    assert_eq!(widths[0], FILES_W.floor_px());
    assert_eq!(
        widths[1..],
        [widths[1], widths[1], widths[1]],
        "a rebalanced run must divide equally among the columns that take a share"
    );
}

/// A pane arriving in a run it was *not* in buys its share off the pane it
/// landed beside, and off nobody else — the landing rule §3.3 already had,
/// applied to one column instead of re-divided across the whole run.
#[test]
fn a_pane_arriving_from_another_run_is_paid_for_by_the_pane_it_landed_beside() {
    let view = viewport(1600, 900);
    // Three columns with a stack in the last of them, the first hand-widened.
    let tree = dragged(
        &row(1, term(1), row(2, term(2), col(3, term(3), term(4)))),
        1,
        600_000,
        1599,
    );
    let was = row_widths(&solved(&tree, view, 1));
    assert_eq!(was, vec![958, 319, 319, 319]);

    // Seat 4 comes up out of its stack and lands on seat 1's leading edge.
    let after = move_pane(&tree, 4, before(1));
    // In-order afterwards: the traveller, the pane it landed beside, then the
    // two it did not.
    let widths = row_widths(&solved(&after, view, 1));
    assert_eq!(
        widths[2], was[1],
        "a column nobody landed beside paid for the landing"
    );
    assert_eq!(
        widths[0], widths[1],
        "the buyer and the seller divide what the seller held, by column count"
    );
    assert_eq!(
        widths[3], was[2],
        "the column the traveller stood in keeps the width it had — it was a \
         stack, so the two of them shared it rather than divided it"
    );
}
