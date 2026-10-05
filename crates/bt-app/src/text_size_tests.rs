//! **Ticket 37 — each terminal pane owns its text size.**
//!
//! `Runtime` cannot be built without a window, so every claim here is made the way the paste
//! card's tests make theirs: the decision the runtime takes is a free function over real leaves
//! and real tabs — the real `step_leaf_text_scale` (the one mutation door's core), the real
//! `apply_leaf_metrics`, the real `pane_cell_metrics`-shaped derivation through the production
//! measuring service (`bt_render::CellMetrics::measure_at` behind `bt_render::CellMetricsMemo`),
//! the real `schedule_leaf_grid_change` / `release_due_leaf_resize` transaction, and frames the
//! leaves' own sessions compose. Where the claim is about the window's own wiring — which rung
//! the wheel reaches first, which metrics a hit test reads, who may write the rung — it is
//! pinned on the item bodies through `bt_source`.

use super::*;
use crate::tests::{
    cross_metrics, cross_solve, cross_tab, focused_frame, leaf_saying, tab_holding,
};
use bt_source::{Index, ItemQuery, Pattern, Search, View, needle};

/// The Settings size these tests stand on — the renderer's default face.
const BASE: f32 = 16.0;

/// The derivation of `pane_cell_metrics`, at a chosen base and display scale, through the
/// production measuring service.
fn derive_at(scale: f64, base: f32) -> impl Fn(TextScale) -> Result<bt_render::CellMetrics> {
    move |rung| Ok(fixture_cell_metrics(scale, rung.effective_logical_px(base)))
}

/// A pane's body, big enough for a real grid at every rung.
fn body() -> SeatViewport {
    SeatViewport {
        x: 0,
        y: 0,
        width: 900,
        height: 600,
    }
}

fn physical() -> PhysicalSize<u32> {
    PhysicalSize::new(body().width, body().height)
}

/// A shell-less leaf at 100 % whose session really is at its metrics, with its grid solved for
/// [`body`] and its child told — a pane at rest.
///
/// `leaf_saying` is born with its own 22-pixel rows rather than with its metrics, so the leaf is
/// first given a different scale and then put at 100 % through the production apply operation,
/// which is the one writer of the session's cell geometry.
fn pane_at_rest(text: &str) -> LeafSession {
    let mut leaf = leaf_saying(text);
    leaf.metrics = fixture_cell_metrics(2.0, BASE);
    assert!(apply_leaf_metrics(
        &mut leaf,
        fixture_cell_metrics(1.0, BASE)
    ));
    leaf.presented_metrics = leaf.metrics;
    resolve(&mut leaf, Instant::now());
    settle(&mut leaf, Instant::now() + Duration::from_secs(1));
    leaf
}

/// Re-solve a leaf into [`body`], as every road that re-solves the panes does.
fn resolve(leaf: &mut LeafSession, at: Instant) -> bool {
    let next = leaf.grid_for(body());
    schedule_leaf_grid_change(
        leaf,
        next,
        physical(),
        at,
        LeafOnStage::Shown,
        "ticket 37",
        card_trace::Pane::untraced(),
    )
    .expect("a shell-less leaf reflows")
}

/// Release whatever the leaf owes its child, as the loop does past the quiet boundary.
fn settle(leaf: &mut LeafSession, at: Instant) -> Option<LeafResizeCommit> {
    release_due_leaf_resize(leaf, at, false)
        .expect("a shell-less leaf commits")
        .0
}

/// Everything a neighbour's step could disturb, as one comparable picture.
fn snapshot(leaf: &LeafSession) -> String {
    format!(
        "{:?}",
        (
            leaf.text_scale,
            leaf.metrics,
            leaf.presented_metrics,
            leaf.grid,
            leaf.conpty_grid,
            leaf.pending_pty_resize.map(|pending| pending.grid),
            leaf.session.layout_key(),
            leaf.projection.cell_height_subpixels(),
        )
    )
}

// ── ownership and lifecycle ──────────────────────────────────────────────────

/// NEW (37) — **stepping one pane changes that pane's metrics, grid, projection, layout key and
/// resize debt, and leaves the pane beside it byte-identical.**
///
/// Non-vacuous on both halves: the target is asserted to have moved in every one of the five
/// places a size lives, and the sibling — re-solved through the same road, as the door re-solves
/// every pane — is asserted to have moved in none, with nothing owed to its child.
///
/// MUTATION: make `step_leaf_text_scale` apply the derived metrics without moving the rung (drop
/// `leaf.text_scale = requested`) — the target's rung reads 100 and the percent assertion goes
/// red; make `LeafSession::grid_for` read the window's base metrics — the target's grid does not
/// shrink.
#[test]
fn stepping_one_pane_changes_its_metrics_and_leaves_its_neighbour_byte_identical() {
    let mut target = pane_at_rest("left");
    let mut sibling = pane_at_rest("right");
    let target_before = (
        target.metrics,
        target.grid,
        target.session.layout_key(),
        target.projection.cell_height_subpixels(),
    );
    let sibling_before = snapshot(&sibling);

    let moved = step_leaf_text_scale(&mut target, TextStep::Larger, 3, derive_at(1.0, BASE))
        .expect("the derivation measures");
    assert!(moved, "three rungs up is a new size");
    let now = Instant::now();
    assert!(resolve(&mut target, now), "the stepped pane reflows");
    assert!(!resolve(&mut sibling, now), "its neighbour does not");

    assert_eq!(target.text_scale.percent(), 150);
    assert_eq!(target.metrics, fixture_cell_metrics(1.0, 24.0));
    assert_ne!(target.metrics, target_before.0);
    assert!(target.grid.columns < target_before.1.columns);
    assert!(target.grid.rows < target_before.1.rows);
    assert_ne!(target.session.layout_key(), target_before.2);
    assert_eq!(
        target.session.layout_key().font_size_subpixels,
        target.metrics.font_size_subpixels().get()
    );
    assert_ne!(target.projection.cell_height_subpixels(), target_before.3);
    assert_eq!(
        target.pending_pty_resize.map(|pending| pending.grid),
        Some(target.grid),
        "the child is owed the new grid"
    );
    assert_eq!(
        snapshot(&sibling),
        sibling_before,
        "nothing about the pane beside it moved"
    );
}

/// NEW (37) — **a burst of text-size notches tells the child at most once, with the final grid;
/// a step the ladder refuses and a round trip back to the child's grid tell it nothing.**
///
/// Driven the way the wheel drives it: reports turned into rungs by the rung's own carry
/// ([`TextSizeAim::notch`]), each spend applied through the door's core and re-solved through
/// `schedule_leaf_grid_change`, and the child released by `release_due_leaf_resize` exactly as
/// the loop releases it. `told_the_child` is the commit's own record of whether
/// `PtySession::resize` — `ResizePseudoConsole` — is called; the fixture has no ConPTY, so this
/// counts the decision and not the syscall.
///
/// MUTATION: release each step's grid at once (call `settle` inside the loop) — three commits
/// that each tell the child, and the count goes red.
#[test]
fn a_burst_of_notches_tells_the_child_at_most_once_with_the_final_grid() {
    let at = LeafId {
        tab: TabId(1),
        seat: SeatId(1),
    };
    let started = Instant::now();
    let turn = |leaf: &mut LeafSession, slot: &mut Option<TextSizeAim>, delta, when| {
        let carry = slot.take();
        let (step, count) = TextSizeAim::notch(carry, slot, at, delta, leaf.text_scale);
        if count != 0
            && step_leaf_text_scale(leaf, step, count, derive_at(1.0, BASE)).expect("measures")
        {
            resolve(leaf, when);
        }
    };

    // Six 20-pixel reports are one notch; then three whole notches — all inside the quiet window.
    let mut leaf = pane_at_rest("burst");
    let told_before = leaf.conpty_grid;
    let mut slot = None;
    for index in 0..6 {
        turn(
            &mut leaf,
            &mut slot,
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 20.0)),
            started + Duration::from_millis(index),
        );
    }
    assert_eq!(
        leaf.text_scale.percent(),
        110,
        "six 20-pixel reports are one rung"
    );
    for index in 0..3 {
        turn(
            &mut leaf,
            &mut slot,
            MouseScrollDelta::LineDelta(0.0, 1.0),
            started + Duration::from_millis(10 + index),
        );
    }
    assert_eq!(leaf.text_scale.percent(), 175);
    assert_eq!(
        leaf.conpty_grid, told_before,
        "nothing told inside the window"
    );
    assert!(
        settle(&mut leaf, started + Duration::from_millis(20)).is_none(),
        "nothing released before the quiet boundary"
    );
    let commit = settle(&mut leaf, started + Duration::from_secs(2)).expect("one release");
    assert!(commit.told_the_child, "the child is told once");
    assert_eq!(leaf.conpty_grid, leaf.grid, "and told the final grid");
    assert_ne!(leaf.conpty_grid, told_before);
    assert!(
        settle(&mut leaf, started + Duration::from_secs(4)).is_none(),
        "and never again"
    );

    // At the top of the ladder a further notch is spent on nothing.
    let mut top = pane_at_rest("top");
    step_leaf_text_scale(&mut top, TextStep::Larger, 20, derive_at(1.0, BASE)).unwrap();
    resolve(&mut top, started);
    settle(&mut top, started + Duration::from_secs(1));
    let at_the_top = snapshot(&top);
    let mut slot = None;
    turn(
        &mut top,
        &mut slot,
        MouseScrollDelta::LineDelta(0.0, 1.0),
        started + Duration::from_secs(2),
    );
    assert_eq!(snapshot(&top), at_the_top, "a refused step touches nothing");
    assert!(settle(&mut top, started + Duration::from_secs(5)).is_none());

    // Up and back down inside the quiet window ends where the child already is.
    let mut round = pane_at_rest("round");
    let grid = round.conpty_grid;
    let mut slot = None;
    turn(
        &mut round,
        &mut slot,
        MouseScrollDelta::LineDelta(0.0, 1.0),
        started,
    );
    let mut slot = None;
    turn(
        &mut round,
        &mut slot,
        MouseScrollDelta::LineDelta(0.0, -1.0),
        started + Duration::from_millis(5),
    );
    assert!(round.text_scale.is_actual());
    let commit = settle(&mut round, started + Duration::from_secs(2));
    assert!(
        commit.is_none_or(|commit| !commit.told_the_child),
        "a round trip back to the child's grid tells it nothing"
    );
    assert_eq!(round.conpty_grid, grid);
}

/// NEW (37) — **a Settings change then a display change, the display change then the Settings
/// change, and a direct measurement are one size; and a tab moved to a window at another scale is
/// measured there.**
///
/// Through the stored rung, the production memo (keyed by the font environment's epoch, the size
/// and the scale), the effective-size derivation and the apply operation, in both orders — and
/// then through `apply_tabs_leaf_metrics`, the walk a transfer to another window takes.
///
/// MUTATION: store the product — derive from `leaf.metrics.font_size_px` scaled instead of from
/// the rung — and the two orders part; key the memo by size alone — and the move to 2.0 keeps
/// the 1.5 cell.
#[test]
fn settings_then_dpi_equals_dpi_then_settings_equals_a_direct_measure() {
    let mut fonts = bt_render::preview_measure_font_system();
    let mut memo = bt_render::CellMetricsMemo::default();
    let rung = TextScale::ACTUAL
        .stepped(TextStep::Larger)
        .stepped(TextStep::Larger)
        .stepped(TextStep::Larger);
    assert_eq!(rung.percent(), 150);
    let mut measure = |epoch: u64, scale: f64, base: f32| {
        memo.measure(&mut fonts, epoch, scale, rung.effective_logical_px(base))
            .expect("measures")
    };
    // Settings (a new font environment, 14 → 18) first, then the display (1.0 → 1.5).
    let mut first = pane_at_rest("first");
    first.text_scale = rung;
    apply_leaf_metrics(&mut first, measure(1, 1.0, 14.0));
    apply_leaf_metrics(&mut first, measure(2, 1.0, 18.0));
    apply_leaf_metrics(&mut first, measure(2, 1.5, 18.0));
    // The display first, then Settings.
    let mut second = pane_at_rest("second");
    second.text_scale = rung;
    apply_leaf_metrics(&mut second, measure(1, 1.0, 14.0));
    apply_leaf_metrics(&mut second, measure(1, 1.5, 14.0));
    apply_leaf_metrics(&mut second, measure(2, 1.5, 18.0));
    let direct = bt_render::CellMetrics::measure_at(
        &mut bt_render::preview_measure_font_system(),
        1.5,
        27.0,
    )
    .expect("measures");
    assert_eq!(first.metrics, direct);
    assert_eq!(second.metrics, direct);
    assert_eq!(first.session.layout_key(), second.session.layout_key());
    assert_eq!(first.text_scale, rung, "neither change moved the rung");

    // Carried to a window at 2.0: the same rung, measured there.
    let mut tabs = vec![tab_holding(first)];
    apply_tabs_leaf_metrics(&mut tabs, derive_at(2.0, 18.0)).expect("measures");
    let moved = tabs[0].focused().expect("the tab holds its shell");
    assert_eq!(moved.text_scale, rung);
    assert_eq!(moved.metrics, fixture_cell_metrics(2.0, 27.0));
    assert_eq!(
        moved.session.layout_key().font_size_subpixels,
        moved.metrics.font_size_subpixels().get()
    );
}

/// NEW (37) — **a tab moved to a window at another scale is measured there**, and the transfer
/// asks for it before the re-solve hands the panes their grids.
///
/// The measuring half runs the real walk (`apply_tabs_leaf_metrics`) on a real tab; the wiring
/// half is the order inside `FolioApp::transfer_tab`, pinned because `FolioApp` cannot be built
/// without an event loop.
///
/// MUTATION: delete `runtime.apply_every_leaf_metrics()?` from `transfer_tab` — the pin goes red,
/// and on the machine a pane carried from a 1.0 monitor to a 2.0 one draws at half size until
/// the next display change.
#[test]
fn a_tab_moved_to_a_window_at_another_scale_is_measured_there() {
    let mut leaf = pane_at_rest("carried");
    step_leaf_text_scale(&mut leaf, TextStep::Larger, 2, derive_at(1.0, BASE)).unwrap();
    let mut tabs = vec![tab_holding(leaf)];
    apply_tabs_leaf_metrics(&mut tabs, derive_at(2.0, BASE)).unwrap();
    let there = tabs[0].focused().unwrap();
    assert_eq!(there.text_scale.percent(), 125);
    assert_eq!(there.metrics, fixture_cell_metrics(2.0, 20.0));
    assert_eq!(
        there.projection.cell_height_subpixels(),
        there.metrics.cell_height_subpixels()
    );

    let transfer = method_body("FolioApp", "transfer_tab");
    let applied = transfer
        .find("runtime.apply_every_leaf_metrics()?;")
        .expect("the destination re-derives its panes");
    let settled = transfer
        .find("runtime.settle_seat_set_change()?;")
        .expect("and re-solves them");
    assert!(
        applied < settled,
        "measured first, then solved:\n{transfer}"
    );
}

/// PIN (37) — ***Restart shell* keeps the rung, and a restart that fails keeps the old leaf.**
///
/// The replacement is born at the rung the constructor is handed — read off the leaf it replaces
/// before anything is spawned — and the old leaf leaves the map only by the insert of a
/// replacement that exists. `Runtime::restart_shell` spawns a ConPTY and cannot run here, so this
/// is the body, read through `bt_source`.
///
/// MUTATION: build the view with `TextScale::ACTUAL` — a restarted pane at 150 % comes back at
/// 100 %; remove the old leaf before the spawn — a failed spawn leaves the seat empty.
#[test]
fn restart_shell_keeps_the_rung_and_a_failed_restart_keeps_the_old_leaf() {
    let restart = method_body("Runtime", "restart_shell");
    let read = restart
        .find("let text_scale = leaf.text_scale;")
        .expect("the rung is read off the leaf being replaced");
    let built = restart
        .find("LeafView::at(&mut self.app.gpu, &self.window.renderer, text_scale)?")
        .expect("and handed to the constructor");
    let spawned = restart.find("create_leaf_session(").expect("spawns");
    let replaced = restart
        .find("self.sessions.insert(seat, spawned?);")
        .expect("the old leaf goes only with a replacement in hand");
    assert!(
        read < built && built < spawned && spawned < replaced,
        "{restart}"
    );
    assert!(
        !restart.contains("self.sessions.remove(&seat)"),
        "nothing takes the old leaf out before the spawn has answered"
    );
}

/// NEW (37) — **a split and a duplicate start at 100 %; tear-out and merge keep the rung.**
///
/// The moving half runs the production moves on real tabs: `tear_pane_into_tab`, and a merge
/// through `absorb_tab_into_layout` whose arriving seats are renumbered — the rung travels inside
/// the `LeafSession` that is moved, with no copy in any transfer record. The birth half is every
/// constructor's argument, pinned: `Runtime::split_seat` and every caller of `create_tab_state`
/// (the `+`, a duplicate, a restore, *Reopen closed*) hand new views `TextScale::ACTUAL`.
///
/// MUTATION: rebuild the moved leaf's view on arrival (reset `text_scale`) — both moves read 100.
#[test]
fn split_and_duplicate_start_at_100_and_tear_out_and_merge_keep_the_rung() {
    // Tear-out.
    let mut staying = cross_tab(1, &["STAYS", "LEAVES"]);
    let leaving = SeatId(2);
    let leaf = staying.sessions.get_mut(&leaving).expect("the second pane");
    step_leaf_text_scale(leaf, TextStep::Larger, 2, derive_at(1.0, BASE)).unwrap();
    let torn = tear_pane_into_tab(
        &mut staying,
        &cross_metrics(),
        leaving,
        TabId(9),
        Instant::now(),
        Motion::Full,
        cross_solve,
    )
    .expect("a terminal pane may become a tab");
    let carried = torn.focused().expect("the torn tab holds the shell");
    assert_eq!(carried.text_scale.percent(), 125, "tear-out keeps the rung");
    assert_eq!(carried.metrics, fixture_cell_metrics(1.0, 20.0));
    assert!(
        staying
            .sessions
            .values()
            .all(|leaf| leaf.text_scale.is_actual()),
        "and the pane that stayed is at its own size"
    );

    // Merge, with the arriving seats renumbered.
    let mut arriving = cross_tab(2, &["ONE", "TWO"]);
    let leaf = arriving.sessions.get_mut(&SeatId(2)).unwrap();
    step_leaf_text_scale(leaf, TextStep::Smaller, 1, derive_at(1.0, BASE)).unwrap();
    let mut target = cross_tab(3, &["HOST"]);
    let arrived = crate::tests::cross_merge(
        &arriving.seats,
        &mut target,
        seats::LayoutAim::SeatEdge(SeatId(1), seats::DropEdge::Right),
    );
    let renamed = arrived
        .iter()
        .find(|(was, _)| *was == SeatId(2))
        .expect("the stepped pane arrived")
        .1;
    absorb_tab_into_layout(
        &mut arriving,
        &mut target,
        &arrived,
        None,
        TabId(9),
        cross_solve,
    );
    assert_ne!(renamed, SeatId(2), "the merge renumbered it");
    assert_eq!(
        target.sessions.get(&renamed).unwrap().text_scale.percent(),
        90,
        "merge keeps the rung under its new number"
    );

    // Birth: every new view is handed 100 %.
    assert!(
        method_body("Runtime", "split_seat")
            .contains("LeafView::at(&mut self.app.gpu, &self.window.renderer, TextScale::ACTUAL)?"),
        "a split starts at 100 %"
    );
    let callers = found(
        needle!(Pattern::call("create_tab_state")),
        View::CodeKeepingLiterals,
    )
    .in_the_product(source());
    assert!(callers.len() >= 6, "{}", callers.report(source()));
    for (identity, _) in callers.owners(source()) {
        // The definition's own line is read as a call of itself.
        if identity.name == "create_tab_state" {
            continue;
        }
        let body = if let Some(owner) = identity.type_owner.as_deref() {
            method_body(owner, &identity.name)
        } else {
            free_fn_body(&identity.name)
        };
        assert!(
            body.contains("TextScale::ACTUAL"),
            "`{}` builds a tab's views at 100 %:\n{body}",
            identity.name
        );
    }
}

/// GREEN-INVARIANT (37) — **the rung is never written, and every rebuilt view starts at 100 %.**
///
/// Green on base, vacuously — there was no rung to write. From here on it is the invariant: the
/// persisted layout a tab writes is byte-identical before and after a pane's size is stepped,
/// and its terminal leaves carry nothing a size could be read back from. Every view those
/// documents rebuild is born through `create_tab_state`, whose views are `TextScale::ACTUAL`
/// (pinned by the test above).
///
/// MUTATION: add the rung to `TermLeafV1` and write it from `TabState::term_leaf` — the two
/// documents differ.
#[test]
fn the_rung_is_never_written_and_every_rebuilt_view_starts_at_100() {
    let mut tab = tab_holding(pane_at_rest("saved"));
    let persisted = |tab: &TabState| {
        serde_json::to_string(
            &tab.seats
                .to_persisted(&|seat| tab.term_leaf(seat, false), &|seat| {
                    tab.files_state(seat)
                }),
        )
        .expect("the layout serializes")
    };
    let before = persisted(&tab);
    let leaf = tab.focused_mut().unwrap();
    step_leaf_text_scale(leaf, TextStep::Larger, 4, derive_at(1.0, BASE)).unwrap();
    assert_eq!(tab.focused().unwrap().text_scale.percent(), 175);
    let after = persisted(&tab);
    assert_eq!(before, after, "a size step writes nothing");
    assert!(!after.contains("text") && !after.contains("175"), "{after}");

    // The document this tab writes is the whole of what a relaunch, a startup restore, a recent
    // row and *Reopen closed* rebuild it from, so there is nowhere a size could come back from.
    let leaves = persisted_term_leaves(
        &tab.seats
            .to_persisted(&|seat| tab.term_leaf(seat, false), &|seat| {
                tab.files_state(seat)
            }),
    )
    .into_iter()
    .cloned()
    .collect::<Vec<_>>();
    assert_eq!(leaves.len(), 1);
    assert!(!format!("{leaves:?}").contains("175"));
}

/// GUARD (37) — **the rung has one owner, and no pane is sized from the window's metrics.**
///
/// The rule, in prose (also `docs/DESIGN.md`, 2026-09-24): *a pane's requested text size lives in
/// `LeafSession::text_scale` and nowhere else; it is written by `step_leaf_text_scale` (the one
/// door's core) and by construction; a size is derived from it by `pane_cell_metrics` and put on
/// the leaf by `apply_leaf_metrics`; and the window's base metrics are read only through
/// `WindowRenderer::base_metrics`, by chrome that follows the Settings size at window scale.*
///
/// Four readings, over the recursive production universe of both crates:
/// 1. the rung is assigned in exactly one item;
/// 2. the name `text_scale` stands only in the items listed here — its owner's field, the
///    constructor's view, the door, the derivation, the walks that re-derive, and the two
///    readers of the requested rung (the pane head's mark and the wheel's end-of-ladder test);
/// 3. no other crate names it — not `bt-persist`, not the renderer;
/// 4. the window's base metrics are read by the named chrome readers and nobody else, on both
///    sides of the crate boundary.
///
/// Red on base as a structural guard: base had no rung and eighty-odd readers of the window's
/// single `metrics()`.
///
/// MUTATION: add `let size = leaf.text_scale;` to any other method (an alias read) — reading 2
/// names it; add `leaf.text_scale = TextScale::ACTUAL;` in `Runtime::activate_tab` (an extra
/// writer) — readings 1 and 2; call `self.window.renderer.base_metrics().cell_width_px` from a
/// hit test — reading 4.
#[test]
fn the_rung_has_one_owner() {
    // 1. One writer.
    let writes = found(
        needle!(Pattern::text(".text_scale =")),
        View::CodeKeepingLiterals,
    )
    .in_the_product(source());
    assert_eq!(
        owner_names(&writes, source()),
        vec!["step_leaf_text_scale".to_owned()],
        "{}",
        writes.report(source())
    );
    // 2. The name stands in these items and no others.
    let named = found(
        needle!(Pattern::identifier("text_scale")),
        View::Identifiers,
    )
    .in_the_product(source());
    let allowed = [
        // the module that names the fact, and its owner's field
        "text_scale",
        "LeafSession::text_scale",
        // the view a constructor is handed, and the constructor
        "LeafView::text_scale",
        "LeafView::at",
        "create_leaf_session",
        // the door's core, the derivation and the walk that re-derives
        "step_leaf_text_scale",
        "pane_cell_metrics",
        "apply_tabs_leaf_metrics",
        // the restart reads the rung it hands the constructor
        "Runtime::restart_shell",
        // the pane head's mark and its tip read the requested rung
        "Runtime::pane_text_sizes",
        "Runtime::rebuild_tooltip_anchors",
        // the wheel's end-of-ladder test reads it
        "Runtime::mouse_wheel",
    ];
    for name in owner_names(&named, source()) {
        assert!(
            allowed.contains(&name.as_str()),
            "`{name}` names a pane's text size — only its owner, the door, the derivation and \
             the listed readers may:\n{}",
            named.report(source())
        );
    }
    // Outside every item: the crate root's `mod text_scale;` and its `use text_scale::{..}`, and nothing else.
    assert_eq!(
        named.outside_items(source()),
        2,
        "{}",
        named.report(source())
    );
    // 3. No other crate.
    for package in ["bt-persist", "bt-render", "bt-term", "bt-layout"] {
        let index = Index::of_package(package);
        let elsewhere = index
            .search(&Search::new(
                needle!(Pattern::identifier("text_scale")),
                View::Identifiers,
            ))
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(index);
        assert!(
            elsewhere.is_empty(),
            "{package}: {}",
            elsewhere.report(index)
        );
    }
    // 4. The window's base metrics, read by chrome only.
    let base =
        found(needle!(Pattern::call("base_metrics")), View::Identifiers).in_the_product(source());
    let chrome = [
        // a flash band's padding, which is window chrome at every text size
        "Runtime::command_flash_layer",
        // the chrome scroll distance — a strip and a list scroll by the Settings row
        "Runtime::line_height_subpixels",
        // a tab with no shell has no pane metrics; its grid is only traced
        "Runtime::resize_leaves_to_layout",
        // a redraw with no shell composes no pane at its own size
        "Runtime::redraw",
        // a table answer for a pane that has since closed goes nowhere
        "Runtime::apply_math_results",
    ];
    for name in owner_names(&base, source()) {
        assert!(
            chrome.contains(&name.as_str()),
            "`{name}` reads the window's base metrics — a pane is sized from its own:\n{}",
            base.report(source())
        );
    }
    let render = Index::of_package("bt-render");
    let inside = render
        .search(&Search::new(
            needle!(Pattern::text("self.base_metrics")),
            View::CodeKeepingLiterals,
        ))
        .unwrap_or_else(|failure| panic!("{failure}"))
        .in_the_product(render);
    let renderer_chrome = [
        // the named interface itself
        "WindowRenderer::base_metrics",
        "WindowRenderer::scale_factor",
        "WindowRenderer::dpi_milli",
        "WindowRenderer::pane_cell_metrics",
        // what re-measures the base
        "WindowRenderer::update_scale_factor",
        "WindowRenderer::apply_font_change",
        "WindowRenderer::adopt_metrics",
        // window-level chrome and presentation at window scale
        "WindowRenderer::present_state",
        "WindowRenderer::presentation_geometry",
        "WindowRenderer::peek_thumbnail_extent",
        "WindowRenderer::prepare_peek_draws",
        "WindowRenderer::peek_box_rects",
        "WindowRenderer::float_tag_rects",
        // the single-seat door and the replay probe, whose one pane has no rung
        "WindowRenderer::present",
        "WindowRenderer::probe_frame",
    ];
    for name in owner_names(&inside, render) {
        assert!(
            renderer_chrome.contains(&name.as_str()),
            "`{name}` reads the window's base metrics — a seat is drawn at \
             its `SeatFrame::metrics`:\n{}",
            inside.report(render)
        );
    }
}

// ── two live panes at two sizes ──────────────────────────────────────────────

/// NEW (37) — **two panes at two sizes each map the pointer with their own cells.**
///
/// Two real leaves at 100 % and 150 % with equal bodies, each projected by its own session:
/// one pointer position names a different cell in each, and each answer is the one that pane's
/// own frame and metrics give — row from the frame's row map, column from its cell width — so the
/// SGR report sent to each program names that program's cell. The size is then stepped under a
/// stationary pointer: until the new picture is presented, the pointer is measured with the
/// metrics the picture on the glass was drawn at (`LeafSession::presented_metrics`). The window's
/// own wiring — every hit site reading `pane_frame_metrics`, the IME caret reading the focused
/// pane's metrics — is pinned on the bodies.
///
/// MUTATION: make `Runtime::pane_frame_metrics` answer `leaf.metrics` — the stationary-pointer
/// half goes red; make any hit site read the window's base metrics — the pin names it.
#[test]
fn two_panes_at_two_sizes_each_map_the_pointer_with_their_own_cells() {
    let mut small = tab_holding(pane_at_rest("small"));
    let mut large = tab_holding(pane_at_rest("large"));
    let leaf = large.focused_mut().unwrap();
    step_leaf_text_scale(leaf, TextStep::Larger, 3, derive_at(1.0, BASE)).unwrap();
    resolve(leaf, Instant::now());
    let small_frame = focused_frame(&mut small);
    let large_frame = focused_frame(&mut large);
    let small_metrics = small.focused().unwrap().metrics;
    let large_metrics = large.focused().unwrap().metrics;
    let (x, y) = (220.0, 90.0);
    let small_hit = small_metrics.hit_test_frame(&small_frame, x, y).unwrap();
    let large_hit = large_metrics.hit_test_frame(&large_frame, x, y).unwrap();
    assert_ne!(small_hit, large_hit, "one point, two cells");
    for (metrics, hit) in [(small_metrics, small_hit), (large_metrics, large_hit)] {
        let column = ((x as f32 - metrics.padding_px) / metrics.cell_width_px).floor() as u32;
        let row = ((y as f32 - metrics.padding_px) / metrics.cell_height_px).floor() as u32;
        assert_eq!(hit.column, column);
        assert_eq!(hit.row, row);
    }
    let report = |hit: bt_render::GridHit| {
        input::mouse_bytes(
            true,
            input::MouseProtocolButton::Left,
            input::MouseProtocolEvent::Press,
            hit.row,
            hit.column,
            ModifiersState::empty(),
        )
    };
    assert_ne!(
        report(small_hit),
        report(large_hit),
        "each program is told its own cell"
    );

    // A stationary pointer while the size changes: the glass still shows the old picture.
    let presented = large.focused().unwrap().presented_metrics;
    let leaf = large.focused_mut().unwrap();
    step_leaf_text_scale(leaf, TextStep::Larger, 1, derive_at(1.0, BASE)).unwrap();
    assert_eq!(
        large.focused().unwrap().presented_metrics,
        presented,
        "a step is not a presentation"
    );

    for method in [
        "pane_frame_hit",
        "drag_hit_in_pane",
        "hovered_leaf",
        "forwarded_mouse_hit",
        "forwarded_mouse_hit_in",
        "math_hit",
        "terminal_reference_at",
    ] {
        let body = method_body("Runtime", method);
        assert!(
            body.contains("pane_frame_metrics("),
            "`{method}` measures the pointer with the presented picture's metrics:\n{body}"
        );
    }
    assert!(method_body("Runtime", "pane_frame_metrics").contains("presented_metrics"));
    assert!(
        method_body("Runtime", "offer_ime_caret")
            .contains("self.focused().map(|leaf| leaf.metrics)"),
        "the IME caret is measured in the focused pane's cells"
    );
    for (method, needle) in [
        (
            "take_forward_wheel_lines",
            "self.leaf(seat).metrics.cell_height_px",
        ),
        (
            "take_forward_wheel_notches",
            "self.leaf(seat).metrics.cell_height_px",
        ),
        ("wheel_columns", "self.leaf(seat).metrics"),
    ] {
        assert!(
            method_body("Runtime", method).contains(needle),
            "`{method}` converts pixels in the addressed pane's own cells"
        );
    }
}

/// NEW (37) — **a size change that leaves the integer grid unchanged is still a new picture,
/// the gate does not suppress it, and input stays on the displayed picture until it lands.**
///
/// A real leaf is projected, its picture is signed and presented, and then only its metrics
/// move (the apply operation touches no grid). The new frame is a different picture
/// (`pictures_match`), its seat signature differs by the metrics it carries, and the gate asks
/// for a present; the leaf's presented metrics stay the old ones until a present records the new.
///
/// MUTATION: drop `layout_key` from `presentation_equivalent` — with one cell box on both
/// sides, the new picture is called the old one.
#[test]
fn a_size_change_with_an_unchanged_grid_still_presents_and_input_uses_the_displayed_picture() {
    let mut tab = tab_holding(pane_at_rest("same grid"));
    let before = focused_frame(&mut tab);
    let old_metrics = tab.focused().unwrap().metrics;
    let signature = |metrics| present_gate::SeatSignature {
        owner: (1, 1),
        picture_revision: 1,
        viewport: body(),
        clip: body(),
        focused: true,
        metrics,
    };
    let leaf = tab.focused_mut().unwrap();
    let grid = leaf.grid;
    // A face a tenth of a pixel smaller measures to the same cell box: the one case where
    // nothing but the face size tells the two pictures apart.
    let smaller = fixture_cell_metrics(1.0, 15.9);
    assert_eq!(
        (smaller.cell_width_px, smaller.cell_height_px),
        (old_metrics.cell_width_px, old_metrics.cell_height_px),
        "the fixture's two sizes share one cell"
    );
    assert!(apply_leaf_metrics(leaf, smaller));
    assert_eq!(leaf.grid, grid, "the apply operation moved no grid");
    let after = focused_frame(&mut tab);
    assert!(frame_matches_grid(&after, grid), "the grid is unchanged");
    assert!(
        !present_gate::pictures_match(&before, &after),
        "and the picture is not"
    );
    assert_ne!(
        signature(old_metrics),
        signature(tab.focused().unwrap().metrics),
        "the seat's signature carries its metrics"
    );
    assert_eq!(
        tab.focused().unwrap().presented_metrics,
        old_metrics,
        "input is measured on the picture on the glass until the new one is presented"
    );
}

// ── the clamp and the derivation's edges ────────────────────────────────────

/// NEW (37) — **at a base of 10 the three lowest rungs all draw at 8 px, and the head still shows
/// the rung that was asked for**; its tip says the size the clamp drew.
///
/// MUTATION: clamp at 6 instead of 8 — the three sizes part.
#[test]
fn at_base_10_the_three_lowest_rungs_all_draw_at_8_px_and_the_head_shows_the_rung() {
    let mut rung = TextScale::ACTUAL;
    for _ in 0..4 {
        rung = rung.stepped(TextStep::Smaller);
    }
    let lowest: Vec<TextScale> = (0..3)
        .map(|up| (0..up).fold(rung, |scale, _| scale.stepped(TextStep::Larger)))
        .collect();
    assert_eq!(
        lowest
            .iter()
            .map(|scale| scale.percent())
            .collect::<Vec<_>>(),
        [50, 67, 80]
    );
    for scale in &lowest {
        assert_eq!(scale.effective_logical_px(10.0), 8.0);
        assert!(!scale.is_actual(), "the head shows {}", scale.percent());
        assert_eq!(
            i18n::text_size_tip("Actual text size", 10.0, scale.percent(), 8.0),
            if scale.percent() == 80 {
                "Actual text size".to_owned()
            } else {
                "Actual text size (8 px)".to_owned()
            }
        );
    }
    assert_eq!(
        i18n::zoom_percent(f64::from(lowest[1].percent()) / 100.0),
        "67%"
    );
}

/// NEW (37) — **at a base of 24 the top rung is exactly 72.**
///
/// MUTATION: clamp at 64 — 300 % of 24 is cut.
#[test]
fn at_base_24_the_top_rung_is_exactly_72() {
    let top = (0..20).fold(TextScale::ACTUAL, |scale, _| {
        scale.stepped(TextStep::Larger)
    });
    assert_eq!(top.percent(), 300);
    assert_eq!(top.effective_logical_px(24.0), 72.0);
    assert_eq!(top.stepped(TextStep::Larger), top, "the ladder ends there");
}

/// NEW (37) — **the derivation's other edges**: a corrupt stored Settings size is validated
/// where it always was and the rung multiplies the validated base; a fractional display scale is
/// measured once, inside; a new font environment at an unchanged nominal size is re-measured
/// rather than answered from the memo; `Actual` returns to exactly the Settings size; and a pane
/// behind another tab is given its new grid now and reflows only at the quiet boundary.
///
/// MUTATION: key the memo without the epoch — the font change is answered with the old face's
/// cell; reflow a `Behind` leaf at once — the hidden-tab half goes red.
#[test]
fn the_text_size_derivation_holds_at_its_edges() {
    // A corrupt stored size is validated to 10–24 by the one validator, untouched here.
    assert_eq!(settings::drawable_font_size(0), 10);
    assert_eq!(settings::drawable_font_size(255), 24);
    let up = TextScale::ACTUAL.stepped(TextStep::Larger);
    assert_eq!(
        up.effective_logical_px(f32::from(settings::drawable_font_size(255))),
        24.0 * 110.0 / 100.0
    );
    // Fractional scale: one measurement at 1.25, the same as measuring there directly.
    let mut fonts = bt_render::preview_measure_font_system();
    let mut memo = bt_render::CellMetricsMemo::default();
    let at = memo.measure(&mut fonts, 1, 1.25, 16.0).unwrap();
    assert_eq!(
        at,
        bt_render::CellMetrics::measure_at(
            &mut bt_render::preview_measure_font_system(),
            1.25,
            16.0
        )
        .unwrap()
    );
    assert_eq!(at.font_size_px, 20.0, "the scale is applied once");
    // A new font environment at the same nominal size is measured again.
    assert_eq!(memo.len(), 1);
    memo.measure(&mut fonts, 2, 1.25, 16.0).unwrap();
    assert_eq!(
        memo.len(),
        1,
        "the old environment's entry is gone, the new one measured"
    );
    // Back to 100.
    let mut leaf = pane_at_rest("return");
    let home = leaf.metrics;
    step_leaf_text_scale(&mut leaf, TextStep::Larger, 5, derive_at(1.0, BASE)).unwrap();
    step_leaf_text_scale(&mut leaf, TextStep::Actual, 1, derive_at(1.0, BASE)).unwrap();
    assert!(leaf.text_scale.is_actual());
    assert_eq!(leaf.metrics, home);
    // Behind another tab: given the grid, reflowed at release.
    let mut behind = pane_at_rest("behind");
    let grid = behind.grid;
    step_leaf_text_scale(&mut behind, TextStep::Larger, 3, derive_at(1.0, BASE)).unwrap();
    let next = behind.grid_for(body());
    let now = Instant::now();
    assert!(
        !schedule_leaf_grid_change(
            &mut behind,
            next,
            physical(),
            now,
            LeafOnStage::Behind,
            "ticket 37",
            card_trace::Pane::untraced(),
        )
        .unwrap(),
        "a hidden pane does not reflow now"
    );
    assert_eq!(behind.grid, grid);
    let commit = settle(&mut behind, now + Duration::from_secs(1)).expect("released");
    assert!(commit.reflowed && commit.told_the_child);
    assert_eq!(behind.grid, next);
}

// ── input ────────────────────────────────────────────────────────────────────

/// NEW (37) — **six 20-pixel reports are one step, and a reversal at the top of the ladder steps
/// down once.**
///
/// [`TextSizeAim::notch`] is the whole of the rung's arithmetic: the carry in the driver's own
/// currency, dropped on a change of direction or of pane, cleared at the ends of the ladder. A
/// report with no vertical component is not the gesture at all (`wheel_steps_text_size`), and a
/// touch pan arrives as pixels and takes the same carry.
///
/// MUTATION: `round` instead of `trunc` in `TextSizeAim::spend` — a 60-pixel nudge steps; drop the
/// end-of-ladder clear — the reversal at the top is spent on the carry and does not step.
#[test]
fn six_twenty_pixel_reports_are_one_step_and_a_reversal_at_the_top_steps_down_once() {
    let at = LeafId {
        tab: TabId(1),
        seat: SeatId(1),
    };
    let elsewhere = LeafId {
        tab: TabId(1),
        seat: SeatId(2),
    };
    let pixels = |y: f64| MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, y));
    let notch = |slot: &mut Option<TextSizeAim>, at, delta, rung| {
        let carry = slot.take();
        TextSizeAim::notch(carry, slot, at, delta, rung)
    };
    let mut slot = None;
    let spent: Vec<u32> = (0..6)
        .map(|_| notch(&mut slot, at, pixels(20.0), TextScale::ACTUAL).1)
        .collect();
    assert_eq!(
        spent,
        [0, 0, 0, 0, 0, 1],
        "six 20-pixel reports are one rung"
    );
    // A change of pane starts again.
    let mut slot = None;
    notch(&mut slot, at, pixels(100.0), TextScale::ACTUAL);
    assert_eq!(
        notch(&mut slot, elsewhere, pixels(40.0), TextScale::ACTUAL).1,
        0
    );
    // A change of direction starts again.
    let mut slot = None;
    notch(&mut slot, at, pixels(100.0), TextScale::ACTUAL);
    assert_eq!(notch(&mut slot, at, pixels(-40.0), TextScale::ACTUAL).1, 0);
    // At the top: up is spent on nothing and clears the carry; down steps once.
    let top = (0..20).fold(TextScale::ACTUAL, |scale, _| {
        scale.stepped(TextStep::Larger)
    });
    let mut slot = None;
    let (step, count) = notch(&mut slot, at, MouseScrollDelta::LineDelta(0.0, 1.0), top);
    assert_eq!((step, count), (TextStep::Larger, 1));
    assert!(
        slot.is_none(),
        "a notch past the end takes the carry with it"
    );
    let (step, count) = notch(&mut slot, at, MouseScrollDelta::LineDelta(0.0, -1.0), top);
    assert_eq!((step, count), (TextStep::Smaller, 1));
    // Horizontal only: not the gesture. A touch pan with the modifier: pixels, as above.
    let exact = ModifiersState::CONTROL;
    assert!(!wheel_steps_text_size(
        exact,
        bt_platform::HostPlatform::Windows,
        MouseScrollDelta::LineDelta(1.0, 0.0)
    ));
    assert!(wheel_steps_text_size(
        exact,
        bt_platform::HostPlatform::Windows,
        pan_on_the_wheel_road(bt_platform::PanStep {
            began_at: None,
            travel: (0, 30),
        })
        .1
        .expect("a pan that moved turns the wheel")
    ));
}

/// NEW (37) — **`Ctrl+Alt` and `Ctrl+Shift` keep the wheel's existing routes, and on a Mac the
/// gesture is `⌘`, never Control.**
///
/// MUTATION: drop the Shift and Alt exclusions from `input::text_size_wheel_held_on` — the
/// Ctrl+Alt and Ctrl+Shift rows go red (and, with it, the extended sweep).
#[test]
fn ctrl_alt_and_ctrl_shift_wheel_keep_their_existing_routes() {
    use bt_platform::HostPlatform::Windows;
    let notch = MouseScrollDelta::LineDelta(0.0, 1.0);
    assert!(wheel_steps_text_size(
        ModifiersState::CONTROL,
        Windows,
        notch
    ));
    for held in [
        ModifiersState::CONTROL | ModifiersState::ALT,
        ModifiersState::CONTROL | ModifiersState::SHIFT,
        ModifiersState::CONTROL | ModifiersState::SUPER,
        ModifiersState::SUPER,
        ModifiersState::empty(),
    ] {
        assert!(!wheel_steps_text_size(held, Windows, notch), "{held:?}");
    }
}

/// NEW (37) — the macOS dialect of the same ruling: **Control+wheel is not the size gesture on a
/// Mac**; `⌘` alone is.
#[test]
fn control_wheel_is_not_the_size_gesture_on_a_mac() {
    use bt_platform::HostPlatform::MacOs;
    let notch = MouseScrollDelta::LineDelta(0.0, -1.0);
    assert!(wheel_steps_text_size(ModifiersState::SUPER, MacOs, notch));
    for held in [
        ModifiersState::CONTROL,
        ModifiersState::SUPER | ModifiersState::CONTROL,
        ModifiersState::SUPER | ModifiersState::ALT,
        ModifiersState::SUPER | ModifiersState::SHIFT,
    ] {
        assert!(!wheel_steps_text_size(held, MacOs, notch), "{held:?}");
    }
}

/// PIN (37) — **`Ctrl`+wheel over a formula steps the pane and does not pan the formula.**
///
/// The text-size rung stands after the terminal is chosen and before `self.math_hit()` is asked,
/// which is before `wheel_route`; the decision it takes asks nothing about a formula.
/// `Runtime::mouse_wheel` cannot run here, so the order is read off its body.
///
/// MUTATION: move the rung below the math-block pan — a formula under the pointer swallows the
/// gesture, and the order pin goes red.
#[test]
fn ctrl_wheel_over_a_formula_steps_the_pane_and_does_not_pan_the_formula() {
    let wheel = method_body("Runtime", "mouse_wheel");
    let targeted = wheel
        .find("let Some(target_leaf) = self.sessions.get(&target_seat) else {")
        .expect("the terminal is chosen");
    let rung = wheel
        .find("if wheel_steps_text_size(")
        .expect("the text-size rung");
    let pan = wheel.find("self.math_hit()").expect("the math-block pan");
    let route = wheel.find("wheel_route(").expect("the terminal's routes");
    assert!(targeted < rung && rung < pan && pan < route, "{wheel}");
    assert!(
        wheel.contains("let text_size_carry = self.window.text_size_aim.take();"),
        "the carry leaves the window on every notch"
    );
}

/// NEW (37) — **the text-size rows are in force only while a shell holds the keyboard**, on
/// either screen; a tree, a search field, a menu or a modal (no focus bit set), a preview and a
/// page each keep the chord.
///
/// MUTATION: make `Scope::Terminal` hold on `!focus.preview` — the empty focus (a tree, a field,
/// a menu) claims `Ctrl+=`.
#[test]
fn the_text_size_rows_are_in_force_only_while_a_shell_holds_the_keyboard() {
    use winit::keyboard::Key;
    let table = shortcuts::Shortcuts::defaults_for(bt_platform::HostPlatform::Windows);
    let press = |key: &str, focus: shortcuts::Focus| {
        let key = Key::Character(key.into());
        table.lookup(&key, &key, ModifiersState::CONTROL, focus)
    };
    let shell = shortcuts::Focus {
        terminal: true,
        terminal_primary: true,
        ..shortcuts::Focus::default()
    };
    let full_screen = shortcuts::Focus {
        terminal: true,
        ..shortcuts::Focus::default()
    };
    assert_eq!(press("=", shell), Some(shortcuts::Action::TextLarger));
    assert_eq!(press("-", shell), Some(shortcuts::Action::TextSmaller));
    assert_eq!(press("0", shell), Some(shortcuts::Action::TextActualSize));
    assert_eq!(
        press("=", full_screen),
        Some(shortcuts::Action::TextLarger),
        "the alternate screen is still a terminal at its size"
    );
    let elsewhere = [
        shortcuts::Focus::default(),
        shortcuts::Focus {
            preview: true,
            ..shortcuts::Focus::default()
        },
        shortcuts::Focus {
            preview: true,
            web_page: true,
            ..shortcuts::Focus::default()
        },
        shortcuts::Focus {
            search_open: true,
            ..shortcuts::Focus::default()
        },
    ];
    for focus in elsewhere {
        for key in ["=", "-", "0"] {
            assert_eq!(press(key, focus), None, "{key} over {focus:?}");
        }
    }
    let over_a_page = webhost::claimable_chords(
        &table,
        shortcuts::Focus {
            preview: true,
            web_page: true,
            ..shortcuts::Focus::default()
        },
    );
    assert!(
        over_a_page.iter().all(|claim| !matches!(
            claim.action,
            shortcuts::Action::TextLarger
                | shortcuts::Action::TextSmaller
                | shortcuts::Action::TextActualSize
        )),
        "a page keeps its own zoom keys"
    );
    assert!(
        method_body("Runtime", "shortcut_focus")
            .contains("terminal: self.keyboard_owner_is_a_shell()"),
        "the scope is the keyboard owner being a shell, and nothing looser"
    );
}

/// NEW (37; owner ruling 2026-09-24) — **a head wears the percentage beside its top-right
/// controls, to the left of the `⌄`, while a terminal pane is not at 100 %; the title stops
/// before it.**
///
/// One placement rule for the titled head and the headless corner (the next test), so this
/// asserts the rule's own terms: the mark ends where the `⌄` begins, shares its top and height,
/// and neither the name nor a leading control runs into it.
///
/// MUTATION: place the mark after the leading run again (the 0.4.5 draft's slot) — it no longer
/// ends at the `⌄`.
#[test]
fn the_pane_head_wears_the_text_size_while_it_is_not_100() {
    let rect = [0.0, 0.0, 900.0, 600.0];
    for scale in [1.0_f32, 1.5, 2.0] {
        for zoomed in [false, true] {
            let bare = seats::pane_head_geometry(rect, SeatKind::Terminal, zoomed, false, scale);
            let marked = seats::pane_head_geometry(rect, SeatKind::Terminal, zoomed, true, scale);
            assert_eq!(bare.text_size, None, "nothing at 100 %");
            let mark = marked.text_size.expect("a 900px head seats the mark");
            let chevron = marked.chevron.expect("and its `⌄`");
            assert_eq!(mark[2], chevron[0], "it ends where the `⌄` begins");
            assert_eq!(
                (mark[1], mark[3]),
                (chevron[1], chevron[3]),
                "in the run's rhythm"
            );
            assert!(marked.title[2] < mark[0], "the name stops before it");
            assert!(
                marked.control_limit < mark[0],
                "and so do the leading controls"
            );
            let lead = marked.zoom_mark.map_or(marked.mark[2], |zoom| zoom[2]);
            assert!(mark[0] > lead, "clear of the leading run");
            assert_eq!(
                bare.chevron, marked.chevron,
                "the trailing run does not move"
            );
            assert_eq!(bare.close, marked.close);
        }
    }
    assert_eq!(
        seats::pane_head_geometry(rect, SeatKind::Preview, false, true, 1.0).text_size,
        None,
        "a preview head has no text size"
    );
    let narrow = [0.0, 0.0, 60.0, 600.0];
    assert_eq!(
        seats::pane_head_geometry(narrow, SeatKind::Terminal, false, true, 1.0).text_size,
        None,
        "no room, no mark"
    );
}

/// RED (37b) — **an untitled pane at 125 % lays the indicator left of its `⌄` box, and at
/// 100 % lays nothing** (owner ruling 2026-09-24).
///
/// A lone terminal wears no head, so it has no title to stand beside; it has its corner's
/// `⌄ 🗀` on every frame, and the mark stands left of that `⌄` by the same rule a head uses
/// (`seats::text_size_mark_beside`). Asked through `seats::pane_text_size_box`, the one box the
/// painter, the hit test and the tip all read, on a real lone-terminal layout; the hit test is
/// asked at the mark's centre and answers the reset target.
///
/// MUTATION: drop the untitled arm of `pane_text_size_box` (answer `None` for a pane that wears
/// no head) — the 125 % pane lays nothing.
#[test]
fn an_untitled_pane_lays_its_text_size_left_of_its_chevron() {
    let lone = seats::Seats::lone_terminal();
    let seat = lone.identity();
    let (layout, _) = cross_solve(&lone);
    assert!(
        !lone.seat_wears_head(SeatKind::Terminal),
        "a lone terminal has no head"
    );
    let at_125 = std::collections::BTreeMap::from([(seat, 125_u16)]);
    let at_100 = std::collections::BTreeMap::new();
    for scale in [1.0_f32, 2.0] {
        let mark = seats::pane_text_size_box(&lone, &layout, seat, &at_125, scale, None)
            .expect("a 125 % lone pane lays the indicator");
        let rect = seats::full_pane_rect(&layout, seat).expect("the pane is on the stage");
        let chevron = seats::pane_ghost_geometry(rect, scale).expect("the corner's `⌄`");
        assert_eq!(mark[2], chevron[0], "left of the `⌄`, touching it");
        assert_eq!((mark[1], mark[3]), (chevron[1], chevron[3]));
        assert_eq!(
            seats::hit_text_size(
                &lone,
                &layout,
                &at_125,
                scale,
                None,
                f64::from((mark[0] + mark[2]) / 2.0),
                f64::from((mark[1] + mark[3]) / 2.0),
            ),
            Some(seats::ChromeTarget::PaneTextSize(seat)),
            "and a click there is the reset"
        );
        assert_eq!(
            seats::pane_text_size_box(&lone, &layout, seat, &at_100, scale, None),
            None,
            "at 100 % nothing is laid"
        );
    }
    assert_eq!(
        seats::pane_text_size_box(&lone, &layout, seat, &at_125, 1.0, Some(seat)),
        None,
        "a corner the search capsule has taken carries no mark"
    );
}

// ── the reader over this crate's own source ────────────────────────────────

fn source() -> &'static Index {
    Index::of_package("bt-app")
}

fn found(needle: bt_source::Needle, view: View) -> bt_source::Found {
    source()
        .search(&Search::new(needle, view))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

fn method_body(owner: &str, name: &str) -> &'static str {
    source()
        .body_of(&ItemQuery::method(owner, name))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

fn free_fn_body(name: &str) -> &'static str {
    source()
        .body_of(&ItemQuery::function(name))
        .unwrap_or_else(|failure| panic!("{failure}"))
}

/// The items these occurrences stand in, as `Type::name` for a method, a field or a variant and
/// as the bare name for a free function, a module or a type.
fn owner_names(found: &bt_source::Found, index: &Index) -> Vec<String> {
    let mut names: Vec<String> = found
        .owners(index)
        .into_keys()
        .map(|identity| match identity.type_owner {
            Some(owner) => format!("{owner}::{}", identity.name),
            None => identity.name,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
