//! **`install_channel`, as the application drives it.** Tests whose first assertion is about
//! `install_channel`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use crate::test_support::{pane_box_of, pane_rects_of, probe_leaf, scale_task, split_window};

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
    let (tasks, receiver) = mpsc::channel();
    drop(receiver);
    let (scale_tasks, _scale_receiver) = mpsc::channel();
    let (path_tasks, _path_receiver) = mpsc::channel();
    let mut running = true;
    let mut notice_pending = false;

    assert!(dispatch_pending_math_tasks(
        probe_leaf(),
        &mut session,
        &tasks,
        &scale_tasks,
        &path_tasks,
        &mut running,
        &mut notice_pending,
    ));
    assert!(!running);
    assert!(notice_pending);
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
        &tasks,
        &scale_tasks,
        &path_tasks,
        &mut running,
        &mut notice_pending,
    ));
    assert!(
        !notice_pending,
        "the user-visible downgrade notice is one-shot"
    );
}

/// A scale worker may spend arbitrarily long inside Lanczos3; validation still reaches the
/// independent decoration receiver instead of sitting behind that raster in one FIFO.
#[test]
fn local_path_validation_and_resampling_are_dispatched_to_independent_lanes() {
    let (tasks, task_receiver) = mpsc::channel();
    let (scale_tasks, scale_receiver) = mpsc::channel();
    // The third lane, since audit 3 C-2: a path question is not the decoration queue's.
    let (path_tasks, path_receiver) = mpsc::channel();
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::ScaleInlineImage(scale_task("same-path", 128)),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::InlineImage(bt_term::InlineImageTask {
            occurrence_id: 7,
            source: bt_term::InlineImageSource::LocalPath(PathBuf::from("same-path.png")),
        }),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));

    assert!(matches!(
        scale_receiver.try_recv(),
        Ok(ScaleWorkerRequest::InlineImage { .. })
    ));
    assert!(matches!(
        task_receiver.try_recv(),
        Ok(MathWorkerRequest::InlineImage {
            task: bt_term::InlineImageTask {
                occurrence_id: 7,
                ..
            },
            ..
        })
    ));

    // **And a path question goes down a third lane** (audit 3 C-2). It rode the decoration queue
    // until a printed name turned out to be able to name a mapped drive whose server is gone: a
    // `GetFileAttributesW` inside the SMB redirector takes about twenty-one seconds to give up,
    // and twenty-one seconds at the head of *this* queue is every formula and every picture in
    // the window waiting behind one hostile line of output. Nobody waits on the path lane — an
    // unanswered name is simply not a link yet — so it is the one that may be slow.
    assert!(dispatch_decoration_task(
        probe_leaf(),
        SessionDecorationTask::VerifyPath(PathBuf::from(r"Z:\work\notes.md")),
        &tasks,
        &scale_tasks,
        &path_tasks,
    ));
    assert!(matches!(
        path_receiver.try_recv(),
        Ok(PathWorkerRequest { .. })
    ));
    assert!(
        task_receiver.try_recv().is_err(),
        "a path question must not be able to stand in front of a formula"
    );
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
