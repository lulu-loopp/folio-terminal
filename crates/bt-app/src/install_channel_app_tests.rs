//! **`install_channel`, as the application drives it.** Tests whose first assertion is about
//! `install_channel`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{pane_box_of, pane_rects_of, probe_leaf, scale_task, split_window};

/// A formula worker that has ended turns the lane off once, and the pane goes on. The worker ends
/// with a glance owed (its share's ledger ends it `WorkerGone`); the pane's next hand-out is
/// refused at the share's door, its task given back to the pane, and the lane is turned off with
/// one notice — the terminal itself keeps running.
#[test]
fn disconnected_math_dispatch_downgrades_once_and_leaves_the_real_session_usable() {
    let start = Instant::now();
    let mut session =
        DualPlaneSession::new(NonZeroU32::new(40).unwrap(), NonZeroU32::new(2).unwrap());
    session.feed_at(b"$$x$$\x1b[?25l", start).unwrap();
    assert_eq!(
        session.advance_live_stability(start + bt_term::LIVE_MATH_STABLE_INTERVAL),
        1
    );
    let gate = Arc::new(lane::Gate::default());
    let mut watched =
        computation_lane::fixtures::watched(lane::Executor::scripted(Arc::clone(&gate)));
    gate.kill();
    let owed = watched
        .lane
        .decorate(MathWorkerRequest::PeekImage {
            leaf: probe_leaf(),
            path: PathBuf::from("不在这里/截图.png"),
        })
        .ok()
        .expect("admitted");
    assert!(matches!(
        watched.terminal(
            computation_lane::Share::Decoration,
            owed,
            computation_lane::fixtures::nobody
        ),
        lane::Terminal::WorkerGone
    ));
    let outstanding = session.outstanding_decoration_work();
    let mut running = true;
    let mut notice_pending = false;

    assert!(dispatch_pending_math_tasks(
        probe_leaf(),
        &mut session,
        &watched.lane,
        &mut running,
        &mut notice_pending,
    ));
    assert!(!running);
    assert!(notice_pending);
    assert_eq!(
        session.outstanding_decoration_work(),
        outstanding,
        "the refused formula is given back to the pane, not lost"
    );
    session.feed(b"\r\nterminal-still-running").unwrap();
    assert!(
        session
            .terminal()
            .visible_text()
            .iter()
            .any(|row| row.contains("terminal-still-running"))
    );

    assert_eq!(
        take_math_worker_notice(&mut notice_pending),
        Some(math_worker_stopped_notice())
    );
    assert!(!notice_pending);
    assert_eq!(take_math_worker_notice(&mut notice_pending), None);
    assert!(!dispatch_pending_math_tasks(
        probe_leaf(),
        &mut session,
        &watched.lane,
        &mut running,
        &mut notice_pending,
    ));
    assert!(
        !notice_pending,
        "the user-visible downgrade notice is one-shot"
    );
}

/// A scale worker may spend arbitrarily long inside Lanczos3; validation still reaches the
/// independent decoration share instead of sitting behind that raster in one FIFO.
#[test]
fn local_path_validation_and_resampling_are_dispatched_to_independent_lanes() {
    let gate = Arc::new(lane::Gate::default());
    let watched = computation_lane::fixtures::watched(lane::Executor::scripted(Arc::clone(&gate)));
    gate.hold();
    let lane = &watched.lane;
    assert!(
        dispatch_decoration_task(
            probe_leaf(),
            SessionDecorationTask::ScaleInlineImage(scale_task("same-path", 128)),
            lane,
        )
        .is_ok()
    );
    assert!(
        dispatch_decoration_task(
            probe_leaf(),
            SessionDecorationTask::InlineImage(bt_term::InlineImageTask {
                occurrence_id: 7,
                source: bt_term::InlineImageSource::LocalPath(PathBuf::from("same-path.png")),
            }),
            lane,
        )
        .is_ok()
    );
    assert_eq!(lane.held(computation_lane::Share::Scale), 1);
    assert_eq!(lane.held(computation_lane::Share::Decoration), 1);

    // **And a path question goes down a third share** (audit 3 C-2). It rode the decoration
    // queue until a printed name turned out to be able to name a mapped drive whose server is
    // gone: a `GetFileAttributesW` inside the SMB redirector takes about twenty-one seconds to
    // give up, and twenty-one seconds at the head of *this* queue is every formula and every
    // picture in the window waiting behind one hostile line of output. Nobody waits on the path
    // share — an unanswered name is simply not a link yet — so it is the one that may be slow.
    assert!(
        dispatch_decoration_task(
            probe_leaf(),
            SessionDecorationTask::VerifyPath(PathBuf::from(r"Z:\work\notes 笔记.md")),
            lane,
        )
        .is_ok()
    );
    assert_eq!(lane.held(computation_lane::Share::Path), 1);
    assert_eq!(
        lane.held(computation_lane::Share::Decoration),
        1,
        "a path question must not be able to stand in front of a formula"
    );
    gate.release();
}

/// PIN — U8, R3. The counter-scale, stated as the three things it composes
/// to, in one frame.
///
/// The mock-up writes the animation as `scale(s)` on the pane and
/// `scale(1/s)` on an inner wrapper (6584-6586), and it needs both because
/// in CSS a transform is the only way to move a box that is already laid
/// out: the outer scale is the price of the movement and the inner one buys
/// back the text it stretched. Multiply the pair out and three separate
/// facts fall out, which are the three clauses below —
///
/// 1. the pane's **box** on the first frame is the box it left;
/// 2. the pane's **content origin** on that frame is the corner it left;
/// 3. the pane's **content extent** is already the one the solver just gave
///    it, on that same first frame.
///
/// The third is the one that fails a literal transcription of the CSS. Scale
/// this crate's viewport by `s` and clauses 1 and 2 still pass — the box and
/// the corner are right — while the grid inside it is drawn at the *old*
/// width and has to reflow toward the new one over 200ms, which is a resize
/// per frame handed to ConPTY (R2) and glyphs that visibly squeeze.
#[test]
fn the_first_frame_of_a_split_draws_the_old_box_around_contents_already_at_their_new_size() {
    let now = Instant::now();
    let (seats, before, after, survivor, _) = split_window(true);
    let mut motion = PaneMotion::default();
    motion.begin(
        &pane_rects_of(&before),
        &pane_rects_of(&after),
        now,
        Motion::Full,
    );

    let was = pane_box_of(&before, survivor);
    let is = pane_box_of(&after, survivor);
    assert!(
        (was[0] - is[0]).abs() > 1.0 && (was[2] - was[0]) > (is[2] - is[0]),
        "the survivor has to both move and narrow, or this pin proves \
             nothing: {was:?} -> {is:?}"
    );
    let transform = motion.transform_of(survivor, now, Motion::Full);

    // Clause 1 — the box.
    let box_now = transform.applied_to(is);
    for channel in 0..4 {
        assert!(
            (box_now[channel] - was[channel]).abs() < 1e-3,
            "the first frame is drawn through the box the pane left: \
                 {box_now:?} against {was:?}"
        );
    }

    let body =
        seats::pane_body_viewport(&seats, &after, survivor, 1.0).expect("the survivor has a body");
    let (viewport, _) = animated_pane_viewports(body, is, transform);

    // Clause 2 — the content's corner.
    assert_eq!(
        viewport.x as f32, was[0],
        "the contents are drawn from the corner the pane left, not from the \
             one the solver just gave it ({} against {})",
        viewport.x, is[0]
    );

    // Clause 3 — the content's extent, and the one a scaled viewport fails.
    assert_eq!(
        (viewport.width, viewport.height),
        (body.width, body.height),
        "the contents are already at the size the solver gave them on the \
             very first frame — nothing is ever scaled"
    );
    assert!(
        f32::from(u16::try_from(viewport.width).unwrap_or(u16::MAX)) < was[2] - was[0],
        "and that size really is different from the box's, so clause 3 is \
             not restating clause 1"
    );
}
