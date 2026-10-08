//! **The crate root: DPI and resize.** Tests of items `main.rs` owns, sorted under
//! this theme by the theme sort of `docs/plans/bt-app-split-inventory-2026-09-15.md`
//! §0.3; written in the crate root's scope (`use super::*`), with their shared fixtures
//! from [`crate::test_support`].

use super::*;
use crate::test_support::{ResizeGateHarness, grid_of, method_body, probe_leaf, scale_task};
use std::time::Duration;

/// Sovereignty taken by a divider drag is not handed back by a rectangle
/// that happens to match the program's last claim — the same asymmetry the
/// window drag already relies on, and the reason the layout does not snap
/// back one frame after the button comes up.
#[test]
fn a_layout_the_hand_chose_stays_the_hands_until_the_program_claims_again() {
    let claimed = PhysicalSize::new(1920, 1200);
    let (policy, held) =
        size_authority_for_rectangle(SizePolicy::Sovereign, Some(claimed), claimed);
    assert_eq!(policy, SizePolicy::Sovereign);
    assert_eq!(held, Some(claimed));
}

/// **Whose size is it** (user ruling 2026-08-08), as the three rules that decide it.
///
/// Windows delivers the same `Resized` event for a hand on the frame, for a window created at
/// a restored size, and for the resize that rides along with a DPI change. Only the first is a
/// user saying "this is my size"; getting that wrong either folds a window somebody dragged
/// narrow or leaves a restored-too-small session showing slivers.
#[test]
fn a_resize_is_the_users_unless_the_program_asked_for_it() {
    let opened = PhysicalSize::new(1600, 900);
    let dragged = PhysicalSize::new(700, 400);

    // ① Startup, exactly as it really arrives. `Runtime::create` leaves a standing claim and
    // the first rectangle answers it — whatever the OS made of the request. Repeat deliveries
    // of that same rectangle change nothing, so a session restored into a window too small for
    // its tree still folds.
    let (mut policy, mut claimed) = (SizePolicy::Lawful, None);
    for _ in 0..3 {
        (policy, claimed) = size_authority_for_rectangle(policy, claimed, opened);
    }
    assert_eq!(
        claimed,
        Some(opened),
        "the pending claim adopted the opening rectangle"
    );
    assert_eq!(
        policy,
        SizePolicy::Lawful,
        "a restored session must still fold; startup is not a gesture"
    );

    // ② A rectangle we did not ask for is a hand on the frame.
    (policy, claimed) = size_authority_for_rectangle(policy, claimed, dragged);
    assert_eq!(policy, SizePolicy::Sovereign);
    assert_eq!(
        claimed,
        Some(opened),
        "the claim is not rewritten by a gesture"
    );

    // ③ Sovereignty is not handed back by coincidence. Dragging back through the opening size
    // is not a change of mind about owning the window.
    let (back, _) = size_authority_for_rectangle(policy, claimed, opened);
    assert_eq!(
        back,
        SizePolicy::Sovereign,
        "only a claim returns the layout to law"
    );

    // ④ ...and the claim is `claim_lawful_layout`, whose whole content is the pair below. A
    // DPI change makes it *before* the new rectangle is known, so the resize Windows sends
    // alongside `WM_DPICHANGED` is adopted rather than read as a drag — no matter what size it
    // turns out to be, and no matter how many times it is announced.
    let (mut policy, mut claimed) = (SizePolicy::Lawful, None);
    let after_dpi = PhysicalSize::new(1050, 590);
    for _ in 0..2 {
        (policy, claimed) = size_authority_for_rectangle(policy, claimed, after_dpi);
    }
    assert_eq!((policy, claimed), (SizePolicy::Lawful, Some(after_dpi)));

    // ⑤ The transient that made this rule necessary: winit announced 826x1271 at startup while
    // the window's own inner size was 800x1200. Judging on the presentation rectangle — the
    // number the solver is actually handed — never sees it.
    let (policy, _) = size_authority_for_rectangle(
        SizePolicy::Lawful,
        Some(PhysicalSize::new(800, 1200)),
        PhysicalSize::new(800, 1200),
    );
    assert_eq!(policy, SizePolicy::Lawful);
}

/// **RED — a seam that changes its mind twenty times is paid for once**
/// (§7.50, user ruling 2026-08-31).
///
/// The reporter's log has fifteen `stage=scale-factor-changed` lines inside
/// one drag, alternating 1.5 and 2. Each of them used to remeasure the
/// terminal font, walk every leaf in every tab, rebuild every grid — and
/// re-state this window's minimum size to the OS, which is a `SetWindowPos`
/// aimed at a window the reader still had hold of.
///
/// Two claims, and the second is the one worth the type: outside a drag
/// nothing is deferred at all, so a scale change on a settled window is
/// answered on the spot exactly as it always was.
///
/// Red gate: return `true` unconditionally from `arrived` and the count goes
/// to twenty; return `true` unconditionally from `due` and the settled-window
/// half fires a payment nobody owes.
#[test]
fn a_seam_that_flips_twenty_times_under_one_hand_is_settled_once() {
    let mut settlement = DpiSettlement::default();

    // Twenty flips with the hand on the frame. None of them buys the
    // expensive half, and asking `due` while the hand is still on does not
    // smuggle it in either. No turn is expected there — Windows pumps its own
    // message loop while the modal move/size loop runs, which delivers events
    // but sends no `AboutToWait` (see `hang_watch`'s
    // `a_thread_that_answers_is_alive_even_when_its_loop_has_stopped_turning`)
    // — and the settlement does not rely on that.
    let mut paid = 0;
    for _ in 0..20 {
        if settlement.arrived(true) {
            paid += 1;
        }
        if settlement.due(true) {
            paid += 1;
        }
    }
    assert_eq!(paid, 0, "nothing is paid for while the window is moving");

    // The hand lets go. One payment, on the first turn after it, and the
    // turn after that is quiet.
    assert!(settlement.due(false), "the deferred change comes due once");
    assert!(!settlement.due(false), "and only once");
    assert_eq!(settlement, DpiSettlement::default());

    // And a window nobody is holding is not deferred at all: two opposite
    // changes, two payments, and no third adjustment invented between them.
    let mut settlement = DpiSettlement::default();
    assert!(settlement.arrived(false), "1.5 is answered on the spot");
    assert!(!settlement.due(false), "and owes nothing afterwards");
    assert!(settlement.arrived(false), "so is 2");
    assert!(!settlement.due(false), "and it owes nothing either");
}

/// **RED — a scale change owes a rectangle, and is paid by the first one to
/// arrive** (T-CARD-ANCHOR-DPI, user report 2026-09-14; §7.1.6b′ ④).
///
/// The report is a focus card scrolled back through a shell's output on a
/// full-screen window carried 4K → 1080p → 4K. It came back showing
/// something else, and the reason is on this road rather than on the card's:
/// winit raises `ScaleFactorChanged` from inside its `WM_DPICHANGED`
/// handler, *before* the `SetWindowPos` that handler ends with, so the whole
/// of `scale_factor_changed` works with the rectangle of the display being
/// left. Cutting a grid out of it counts one display's pixels in the other
/// display's cells — and a grid change is a vendor reflow, which freezes
/// whatever it pushes off the top at the width it pushed it off at, for
/// ever. The panes went 240 → 320 → 160 → 120 → 240 columns, two of those
/// widths belonging to no display, and the card's transcript came back cut
/// differently.
///
/// Red gate: return `true` unconditionally from `may_cut_a_grid` and the
/// announcement buys nothing; return `false` unconditionally from `due` and
/// a maximized window whose own display changed scale — the one case that
/// produces no `Resized` at all — keeps the grid it had for ever.
#[test]
fn a_scale_change_owes_a_rectangle_and_is_paid_by_the_first_one_to_arrive() {
    // A window nobody has moved cuts grids exactly as it always did.
    let mut rectangle = DpiRectangle::default();
    assert!(rectangle.may_cut_a_grid());
    assert!(!rectangle.due(), "nothing is owed before a scale changes");

    // The scale arrives without its rectangle. Nothing is cut until one does.
    rectangle.announced();
    assert!(!rectangle.may_cut_a_grid());
    rectangle.announced();
    assert!(
        !rectangle.may_cut_a_grid(),
        "a seam that changes its mind twice still owes one rectangle"
    );

    // The `Resized` Windows sends alongside the scale change: the debt is
    // paid by the event itself, so the turn after it owes nothing.
    rectangle.arrived();
    assert!(rectangle.may_cut_a_grid());
    assert!(
        !rectangle.due(),
        "the rectangle arrived; nothing is deferred"
    );

    // And the case that sends no `Resized` at all — a maximized window whose
    // own display's scale changed, every physical pixel where it was. One
    // payment, on the first turn, and the turn after it is quiet.
    rectangle.announced();
    assert!(
        rectangle.due(),
        "the grids are owed to the rectangle in hand"
    );
    assert!(rectangle.may_cut_a_grid());
    assert!(!rectangle.due(), "and owed once");
    assert_eq!(rectangle, DpiRectangle::default());

    // A rectangle that arrives when none was owed is not a payment waiting
    // to be spent: it clears nothing and leaves nothing behind.
    rectangle.arrived();
    assert!(!rectangle.due());
    assert!(rectangle.may_cut_a_grid());
}

/// **RED (shape) — the two halves of a DPI change are on either side of the
/// question, and the deferred half has somewhere to be spent** (§7.50).
///
/// Two facts about where a line sits relative to another line, which no
/// value in the program carries — so they are held against the source, the
/// way this file's other structural promises are.
///
/// ① The swapchain and the seat solve happen *before* the drag is asked
/// about, because they are owed on every judgement; the font remeasure and
/// everything downstream of it happen *after*, because they are owed only by
/// a window that has stopped moving. A check that drifted below
/// `apply_scale_factor` would defer nothing at all.
///
/// ② A change written down during a drag is paid for from `Runtime::turn`.
/// There is no event at the end of the OS's modal move/size loop, so a
/// settlement with no turn to be spent on is a window that keeps the font it
/// left the other display with, forever.
#[test]
fn the_deferred_half_of_a_dpi_change_is_asked_about_late_and_spent_on_a_turn() {
    let reconcile = method_body("Runtime", "reconcile_authoritative_dpi");
    let solved = reconcile
        .find("self.resolve_seat_layout(render_physical);")
        .expect("the seat rectangles are re-solved against the new surface");
    let asked = reconcile
        .find(".arrived(self.window.custom_window_frame.in_size_move())")
        .expect("the drag is asked whether the expensive half may run");
    let remeasured = reconcile
        .find("self.apply_scale_factor(snapshot.authoritative_scale)?;")
        .expect("the terminal font is remeasured at the new scale");
    assert!(
        solved < asked && asked < remeasured,
        "the cheap half is owed on every judgement and the expensive half only \
             by a window that has stopped moving"
    );

    assert!(
        method_body("Runtime", "turn").contains("self.settle_deferred_dpi()?;"),
        "a deferred DPI change is spent on the first turn after the hand lets go"
    );
}

/// **RED (shape) — the scale change's own arm says the panel's scroll out
/// loud** (the pin the fix asks for).
///
/// The restatement needs the scale the offsets were measured at, and that
/// number lives in exactly one place for exactly as long as it takes
/// `update_scale_factor` to overwrite it. A reading taken after the
/// remeasure is the new scale twice over and the ratio is 1 — a fix that
/// silently does nothing. So the order is held against the source, the way
/// this file's other structural promises are.
#[test]
fn the_scale_change_arm_restates_the_cards_columns_scroll() {
    let body = method_body("Runtime", "apply_scale_factor");
    let read = body
        .find("let measured_at = self.window.renderer.scale_factor();")
        .expect("the scale the panel's lists were measured at is read");
    let remeasured = body
        .find(".update_scale_factor(&mut self.app.gpu, scale_factor)")
        .expect("the renderer is remeasured at the new scale");
    let restated = body
        .find("self.restate_panel_scroll(measured_at, scale_factor);")
        .expect("the panel's scroll offsets are restated in the new scale's pixels");
    assert!(
        read < remeasured && remeasured < restated,
        "the old scale is read before it is overwritten, and spent after"
    );
}

/// RED: a divider storm used to put every intermediate Lanczos3 request on the one FIFO.
/// The worker must execute only the newest size for one content/purpose, while preserving a
/// different purpose as an independent question.
#[test]
fn scale_worker_drag_storm_discards_superseded_work_and_completes_the_latest() {
    let (sender, receiver) = mpsc::channel();
    for width in 1..=128 {
        sender
            .send(ScaleWorkerRequest::Preview {
                leaf: probe_leaf(),
                task: scale_task("same-path", width),
            })
            .unwrap();
    }
    sender
        .send(ScaleWorkerRequest::Peek {
            leaf: probe_leaf(),
            task: scale_task("same-path", 17),
        })
        .unwrap();
    drop(sender);

    let mut executed = Vec::new();
    run_scale_worker(receiver, |request| {
        executed.push((request.purpose(), request.task().display_width_px));
    });

    assert_eq!(
        executed,
        vec![(ScalePurpose::Preview, 128), (ScalePurpose::Peek, 17)]
    );
}

#[test]
fn startup_metrics_must_match_the_authoritative_win32_scale_factor() {
    assert!(ensure_metrics_match_authoritative_scale(1.5, 1.5).is_ok());
    assert!(ensure_metrics_match_authoritative_scale(1.0, 1.5).is_err());
}

#[test]
fn recorded_swapchain_size_matches_clamped_physical_inner_after_every_reconcile_size() {
    const LIMIT: u32 = 8192;
    for inner_size in [
        PhysicalSize::new(960, 600),
        PhysicalSize::new(1440, 900),
        PhysicalSize::new(1920, 1200),
        PhysicalSize::new(2560, 1440),
    ] {
        assert!(swapchain_size_matches_inner(
            (inner_size.width, inner_size.height),
            inner_size,
            LIMIT,
        ));
    }
    assert!(swapchain_size_matches_inner(
        (534, LIMIT),
        PhysicalSize::new(534, 65_464),
        LIMIT,
    ));
    assert!(!swapchain_size_matches_inner(
        (3840, 2160),
        PhysicalSize::new(1920, 1200),
        LIMIT,
    ));
}

#[test]
fn pty_pixel_size_is_clamped_to_backend_width() {
    let size = pty_size(
        GridSize {
            columns: std::num::NonZeroU16::new(80).unwrap(),
            rows: std::num::NonZeroU16::new(24).unwrap(),
        },
        PhysicalSize::new(100_000, 80_000),
    );
    assert_eq!((size.pixel_width, size.pixel_height), (u16::MAX, u16::MAX));
}

/// RED — **a gesture that comes back to the width the child already has still owes a
/// release** (user report 2026-09-17).
///
/// What is queued here is the end of a gesture, not a size the child is owed. It used to be
/// the second thing, so a solve answering `conpty_grid` cancelled the queue outright — and
/// `plan_grid_change` had already reflowed this pane's own actor on that very solve, opening a
/// resize transaction that only a release can settle. No release, no settlement, and the
/// transaction stayed open for the rest of the pane's life.
///
/// Red gate: put the `*pending = None` back in the `else`. The drag below then ends with an
/// empty queue and nothing to close its transaction with.
#[test]
fn a_gesture_that_returns_to_the_childs_own_width_still_queues_its_release() {
    let start = Instant::now();
    let child = grid_of(86, 31);
    let away = grid_of(85, 31);
    let physical = PhysicalSize::new(688, 620);
    let mut pending = None;

    assert!(
        coalesce_pty_resize_on_grid_change(&mut pending, away, child, child, physical, start),
        "the hand leaves the width the child holds, so the child is owed a word"
    );
    let back_at = start + Duration::from_millis(17);
    assert!(
        !coalesce_pty_resize_on_grid_change(&mut pending, child, child, away, physical, back_at),
        "and comes back to it, so it is owed none"
    );
    let due = take_due_pty_resize(&mut pending, back_at + WINDOW_RESIZE_QUIET)
        .expect("the end of the gesture is still owed, and it is what the queue carries");
    assert_eq!(
        due.grid, child,
        "carrying the last grid solved, never an intermediate one"
    );

    // And a solve that moved neither grid is still not a gesture: it schedules nothing, and it
    // does not forget one that is already waiting to be released.
    let mut pending = None;
    assert!(!coalesce_pty_resize_on_grid_change(
        &mut pending,
        child,
        child,
        child,
        physical,
        start
    ));
    assert!(
        take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET).is_none(),
        "a spurious repeat is not a gesture"
    );
    coalesce_pty_resize_on_grid_change(&mut pending, away, child, child, physical, start);
    coalesce_pty_resize_on_grid_change(&mut pending, away, child, away, physical, start);
    assert!(
        take_due_pty_resize(&mut pending, start + WINDOW_RESIZE_QUIET).is_some(),
        "and a repeat arriving behind a real one must not cancel it"
    );
}

#[test]
fn window_resize_coalescer_keeps_only_the_last_size_and_resets_quiet_deadline() {
    let start = Instant::now();
    let first = GridSize {
        columns: std::num::NonZeroU16::new(80).unwrap(),
        rows: std::num::NonZeroU16::new(24).unwrap(),
    };
    let final_grid = GridSize {
        columns: std::num::NonZeroU16::new(112).unwrap(),
        rows: std::num::NonZeroU16::new(31).unwrap(),
    };
    let mut pending = None;
    coalesce_pty_resize(&mut pending, first, PhysicalSize::new(960, 600), start);
    coalesce_pty_resize(
        &mut pending,
        final_grid,
        PhysicalSize::new(1440, 900),
        start + Duration::from_millis(150),
    );

    assert!(take_due_pty_resize(&mut pending, start + Duration::from_millis(349)).is_none());
    let committed = take_due_pty_resize(&mut pending, start + Duration::from_millis(350)).unwrap();
    assert_eq!(committed.grid, final_grid);
    assert_eq!(committed.physical, PhysicalSize::new(1440, 900));
    assert!(pending.is_none());
}

/// RED-CHECK for the pin above: proves it is not vacuous. The pre-fix `resize()` and
/// `commit_seat_geometry()` called `coalesce_pty_resize` unconditionally — the old
/// spawn-then-resize shape this pin exists to forbid — which schedules a real ConPTY resize
/// even when the grid the PTY was spawned with never moved. Restoring that unconditional call
/// at the two real call sites is exactly what turns the pin above red.
#[test]
fn the_old_unconditional_coalesce_would_have_scheduled_a_resize_for_an_unchanged_grid() {
    let grid = GridSize {
        columns: std::num::NonZeroU16::new(100).unwrap(),
        rows: std::num::NonZeroU16::new(30).unwrap(),
    };
    let mut pending = None;
    let now = Instant::now();
    // The old shape: no `next_grid != current_grid` gate at all.
    coalesce_pty_resize(&mut pending, grid, PhysicalSize::new(1000, 700), now);
    assert!(
        take_due_pty_resize(&mut pending, now + WINDOW_RESIZE_QUIET).is_some(),
        "an unconditional coalesce call schedules a resize even for an unchanged grid"
    );
}

/// The other half of that rule: **letting go is what releases it, not a timer that outlives
/// the gesture.** A hand that comes up after the quiet window has already passed is answered
/// on the very next turn, with no second wait.
///
/// Red gate: return the pending deadline while the hand is down instead of `None` and the
/// wake asked for is one this loop does not need; refuse to release on the turn the hand comes
/// up and the assertion below goes red naming an empty request list.
#[test]
fn letting_go_releases_the_size_on_the_next_turn() {
    let start = Instant::now();
    let mut harness = ResizeGateHarness::new(50, 50);
    harness.hand_down = true;
    harness.window_resized(PhysicalSize::new(960, 1800), false, start);
    // Long past the quiet window, and still nothing, because the hand is still down.
    harness.tick(start + Duration::from_secs(5));
    assert_eq!(harness.requests, Vec::new());
    // The button comes up. No further solve — the rectangle has not moved — and the very next
    // turn pays what is owed.
    harness.hand_down = false;
    harness.tick(start + Duration::from_secs(5) + Duration::from_millis(1));
    assert_eq!(harness.requests, vec![grid_of(16, 50)]);
}

/// A window with no client area is still refused, and by the same gate.
#[test]
fn a_window_with_no_client_area_is_not_a_rectangle_to_solve_for() {
    for empty in [
        PhysicalSize::new(0, 600),
        PhysicalSize::new(960, 0),
        PhysicalSize::new(0, 0),
    ] {
        assert!(!resize_worth_solving(false, empty), "{empty:?}");
    }
    assert!(resize_worth_solving(false, PhysicalSize::new(1, 1)));
}

/// PIN: a PTY deadline handed to `ControlFlow::WaitUntil` must still be in the future.
///
/// A past `WaitUntil` makes winit immediately re-enter `about_to_wait` instead of sleeping, so
/// the loop spins on a deadline it has already served. The pending request's own coalescing
/// deadline is the only one there is, and the instant it comes due it is released and there is
/// nothing left to wait for.
///
/// Red gate: drop the `filter(|deadline| *deadline > now)` in `pty_resize_wake_deadline` and
/// the first assertion names the already-due instant it offered.
#[test]
fn wait_until_is_never_offered_an_already_due_resize_deadline() {
    let start = Instant::now();
    let mut pending = None;
    coalesce_pty_resize(
        &mut pending,
        grid_of(80, 24),
        PhysicalSize::new(800, 600),
        start,
    );
    let due_at = start + WINDOW_RESIZE_QUIET;

    assert_eq!(
        pty_resize_wake_deadline(pending, start),
        Some(due_at),
        "before it is due, the coalescing deadline is exactly what the loop should wait for"
    );
    assert!(
        pty_resize_wake_deadline(pending, due_at).is_none(),
        "ControlFlow::WaitUntil must never receive an already-due PTY deadline"
    );
    let (released, wake_deadline) = service_pending_pty_resize(&mut pending, due_at, false);
    assert_eq!(
        released.map(|released| released.grid),
        Some(grid_of(80, 24)),
        "the same reading that refuses the deadline is the one that released the request"
    );
    assert_eq!(
        wake_deadline, None,
        "nothing is owed, so nothing is awaited"
    );
}

#[test]
fn one_cell_terminal_and_zero_pixel_transition_are_defended() {
    let one = std::num::NonZeroU16::new(1).unwrap();
    let grid = GridSize {
        columns: one,
        rows: one,
    };
    let backend = pty_size(grid, PhysicalSize::new(0, 0));
    assert_eq!((backend.columns.get(), backend.rows.get()), (1, 1));
    assert_eq!((backend.pixel_width, backend.pixel_height), (0, 0));

    let mut session =
        DualPlaneSession::new(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap());
    session.feed(b"A").unwrap();
    session
        .resize(NonZeroU32::new(1).unwrap(), NonZeroU32::new(1).unwrap())
        .unwrap();
    assert_eq!(session.terminal().visible_text(), ["A"]);
}
