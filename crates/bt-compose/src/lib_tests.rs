//! The order a pane's frame is made in, driven through this crate's own functions over real
//! sessions fed real bytes (design T-COMPOSE-CRATE §6.2, tests 1–4 and the hold pin).
//!
//! The host's half — the pending slot, the "a frame is already on the glass" fact, a present that
//! fails — is played by [`Host`], one pane's worth of the per-frame sequence this crate's header
//! writes down. A math task is answered with a synthetic raster: typesetting is not this crate's
//! subject, and an engine would make every case here wait on one.

use std::num::NonZeroU32;
use std::time::Duration;

use bt_doc::math::{MathRaster, MathRenderError};
use bt_render::{CellMetrics, SeatViewport};
use bt_term::{DualPlaneSession, LIVE_MATH_STABLE_INTERVAL, SessionMathTask};
use bt_viewport::{ViewportFrame, ViewportProjection};
use web_time::Instant;

use super::*;

fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

/// A session and its view, the two halves a host owns.
fn pane_of(columns: u32, rows: u32) -> (DualPlaneSession, ViewportProjection) {
    bt_term::install_test_host_names();
    let session = DualPlaneSession::new(nz(columns), nz(rows));
    let view = session.new_projection(session.layout_key());
    (session, view)
}

fn synthetic_raster() -> MathRaster {
    MathRaster {
        rgba: vec![0xff; 40 * 40 * 4],
        width_px: 40,
        height_px: 40,
        content_height_px: 40,
        ascent_px: 36.0,
        descent_px: 4.0,
        baseline_px: 36.0,
        render_time: Duration::from_millis(1),
        inline_runs: Vec::new(),
    }
}

/// Answer every math task the session has filed, as the lane would, and say how many there were.
fn answer_math(session: &mut DualPlaneSession) -> usize {
    let mut answered = 0;
    while let Some(task) = session.take_math_worker_task() {
        answered += 1;
        match task {
            SessionMathTask::Live(mut task) => {
                let result = if bt_detect::resolve_live_detection_task(&mut task) {
                    Ok(synthetic_raster())
                } else {
                    Err(MathRenderError::NotDetected)
                };
                session.complete_live_worker_result(task, result);
            }
            SessionMathTask::Frozen(mut task) => {
                let result = if bt_detect::resolve_detection_task(&mut task) {
                    Ok(synthetic_raster())
                } else {
                    Err(MathRenderError::NotDetected)
                };
                session.complete_worker_result(task, result);
            }
        }
    }
    answered
}

fn frame_text(frame: &ViewportFrame) -> String {
    let columns = frame.columns.get() as usize;
    (0..frame.drawable_rows())
        .map(|row| {
            frame.cells[row * columns..(row + 1) * columns]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A whole-screen rewrite that lands cell by cell, bracketed by the cursor going away and coming
/// back: the shape a full-screen program repaints in, which the session holds presentation for
/// while it is half written.
fn cursor_bracketed_repaint(rows: &[&str]) -> Vec<u8> {
    let mut out = b"\x1b[?25l\x1b[H".to_vec();
    for (row, line) in rows.iter().enumerate() {
        if row != 0 {
            out.extend_from_slice(format!("\x1b[{};1H", row + 1).as_bytes());
        }
        out.extend_from_slice(b"\x1b[K");
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(b"\x1b[?25h");
    out
}

/// A screen with one display formula on it, before and after a one-screen scroll. Mixed script:
/// the filler lines carry Chinese.
const SCROLL_BEFORE: &[&str] = &[
    "filler 0 \u{4e2d}\u{6587}",
    "filler 1",
    "filler 2",
    "filler 3",
    "$$",
    r"\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}",
    "$$",
    "filler 4 \u{6d4b}\u{8bd5}",
    "filler 5",
    "prompt> ",
];

const SCROLL_AFTER: &[&str] = &[
    "$$",
    r"\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}",
    "$$",
    "filler 4 \u{6d4b}\u{8bd5}",
    "filler 5",
    "filler 6",
    "filler 7",
    "filler 8",
    "filler 9 \u{4e2d}\u{6587}",
    "prompt> ",
];

/// **One host, one pane**: the per-frame sequence of this crate's header, steps 3 to 6, with the
/// host's own facts beside it — a one-frame pending slot (newest wins), the picture on the glass,
/// and whether this pane is the one whose hold is read (`bt-app`'s focused leaf) or one that holds
/// nothing (its unfocused panes).
struct Host {
    session: DualPlaneSession,
    view: ViewportProjection,
    reads_the_hold: bool,
    pending: Option<ViewportFrame>,
    on_glass: Option<ViewportFrame>,
    /// What the last [`Self::compose`] scheduled.
    scheduled: usize,
}

/// What one [`Host::compose`] did with its frame.
#[derive(Debug, Eq, PartialEq)]
enum Composed {
    /// Held: not scheduled and not published.
    Held,
    /// The same picture the slot or the glass already has: not published.
    Unchanged,
    /// Acknowledged and entered into the pending slot.
    Published,
}

impl Host {
    fn new(columns: u32, rows: u32, reads_the_hold: bool) -> Self {
        let (session, view) = pane_of(columns, rows);
        Self {
            session,
            view,
            reads_the_hold,
            pending: None,
            on_glass: None,
            scheduled: 0,
        }
    }

    fn compose(&mut self, at: Instant) -> Composed {
        let projected = project(Pane {
            session: &mut self.session,
            view: &mut self.view,
        })
        .unwrap();
        if self.reads_the_hold && projected.hold_requested && self.on_glass.is_some() {
            self.scheduled = 0;
            return Composed::Held;
        }
        self.scheduled = schedule(&mut self.session, &projected.frame);
        let newest = self.pending.as_ref().or(self.on_glass.as_ref());
        if newest == Some(&projected.frame) {
            return Composed::Unchanged;
        }
        acknowledge(&mut self.session, &projected.frame, at);
        self.pending = Some(projected.frame);
        Composed::Published
    }

    /// Drain the slot onto the glass. A present that fails files the frame back in the slot, as
    /// `bt-app`'s retry arm does, and acknowledges nothing.
    fn present(&mut self, succeeds: bool) {
        let Some(frame) = self.pending.take() else {
            return;
        };
        if succeeds {
            self.on_glass = Some(frame);
        } else {
            self.pending = Some(frame);
        }
    }
}

// ── the hold pin ──────────────────────────────────────────────────────────────────────────────

/// RED — **`project` files no decoration work; `schedule` is what files it** (§6.2, planted
/// violations: "hold before schedule").
///
/// A live formula settles while the caret is still on its line, which keeps it from becoming
/// work; the caret then leaves the line without touching it. Work is now due, and only a frame
/// boundary that schedules asks for it — so a projection that scheduled on its own would hand a
/// held frame's work to the lane before the host had decided anything.
///
/// MUTATION: add `session.schedule_visible_artifacts(&frame);` to `project` before it returns —
/// the projection files the formula and the first assertion goes red.
#[test]
fn project_files_no_decoration_work_and_schedule_does() {
    let start = Instant::now();
    let (mut session, mut view) = pane_of(40, 24);
    session
        .feed_at("\u{8bc1}\u{660e} $$x^2$$".as_bytes(), start)
        .unwrap();
    session.advance_live_stability(start + LIVE_MATH_STABLE_INTERVAL);
    assert_eq!(
        answer_math(&mut session),
        0,
        "a formula on the caret's own line is not work yet"
    );
    session
        .feed_at(b"\r\n\r\n", start + LIVE_MATH_STABLE_INTERVAL * 2)
        .unwrap();

    let projected = project(Pane {
        session: &mut session,
        view: &mut view,
    })
    .unwrap();
    assert_eq!(
        answer_math(&mut session),
        0,
        "projecting the frame asked for nothing"
    );
    assert!(
        schedule(&mut session, &projected.frame) > 0,
        "the frame boundary is what schedules"
    );
    assert_eq!(answer_math(&mut session), 1, "and the formula is the work");
}

// ── test 1: focused/unfocused parity, and the hold ─────────────────────────────────────────────

/// RED — **a focused and an unfocused pane fed the same bytes make the same frame and schedule
/// the same work, except while the projection asks for a hold** (§6.2 test 1). Then the pane
/// whose hold is read, with a picture on the glass, schedules nothing and publishes nothing; the
/// pane that holds nothing goes on composing as it always did.
///
/// MUTATION: report `hold_requested: false` from `project` — the held pane publishes the
/// half-written screen and the second half goes red.
#[test]
fn a_focused_and_an_unfocused_pane_agree_except_while_held() {
    let start = Instant::now();
    let mut focused = Host::new(60, 10, true);
    let mut unfocused = Host::new(60, 10, false);
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&cursor_bracketed_repaint(SCROLL_BEFORE));
    let settled = start + LIVE_MATH_STABLE_INTERVAL;
    for host in [&mut focused, &mut unfocused] {
        host.session.feed_at(&first, start).unwrap();
        advance(&mut host.session, settled).unwrap();
        answer_math(&mut host.session);
        assert_eq!(host.compose(settled), Composed::Published);
        host.present(true);
    }
    assert_eq!(focused.on_glass, unfocused.on_glass, "the same frame");
    assert_eq!(focused.scheduled, unfocused.scheduled, "the same work");
    assert_eq!(
        focused.on_glass.as_ref().unwrap().math_blocks.len(),
        1,
        "the formula is a picture on both"
    );

    // Half a repaint: the screen is being rewritten and is not finished.
    let repaint = cursor_bracketed_repaint(SCROLL_AFTER);
    let at = start + Duration::from_millis(400);
    for host in [&mut focused, &mut unfocused] {
        host.session
            .feed_at(&repaint[..repaint.len() / 2], at)
            .unwrap();
    }
    let revision = focused.session.published_revision();
    assert_eq!(focused.compose(at), Composed::Held);
    assert_eq!(focused.scheduled, 0, "a held frame schedules nothing");
    assert!(focused.pending.is_none(), "and publishes nothing");
    assert_eq!(focused.session.published_revision(), revision);
    assert_eq!(
        unfocused.compose(at),
        Composed::Published,
        "a pane that holds nothing composes the half-written screen, as today"
    );
}

// ── test 2: live deadlines ───────────────────────────────────────────────────────────────────

/// RED — **a live formula is scheduled only once its stability deadline has been advanced past,
/// and is then a picture** (§6.2 test 2). `schedule` alone never asks for an unsettled live row;
/// the deadline [`deadlines`] reports is the moment [`advance`] settles it.
///
/// MUTATION: make `advance_live_stability` compare `now > deadline` — advancing exactly at the
/// deadline settles nothing and the formula stays its source text.
#[test]
fn a_live_formula_is_scheduled_once_its_deadline_is_advanced_past() {
    let start = Instant::now();
    let mut host = Host::new(40, 24, true);
    host.session
        .feed_at("\u{516c}\u{5f0f}\r\n$$x^2$$\r\n".as_bytes(), start)
        .unwrap();
    assert_eq!(host.compose(start), Composed::Published);
    assert_eq!(answer_math(&mut host.session), 0, "not scheduled yet");
    let due = deadlines(&host.session)
        .live_stability
        .expect("the formula's row is settling");
    assert_eq!(due, start + LIVE_MATH_STABLE_INTERVAL);

    let early = advance(&mut host.session, due - Duration::from_millis(1)).unwrap();
    assert!(!early.live_stability_advanced, "a millisecond early");
    assert_eq!(answer_math(&mut host.session), 0);

    let advanced = advance(&mut host.session, due).unwrap();
    assert!(advanced.live_stability_advanced);
    assert_eq!(
        answer_math(&mut host.session),
        1,
        "scheduled at its deadline"
    );
    host.present(true);
    assert_eq!(host.compose(due), Composed::Published);
    let frame = host.pending.as_ref().unwrap();
    assert_eq!(frame.math_blocks.len(), 1, "and drawn as a picture");
    assert_eq!(
        deadlines(&host.session).live_stability,
        None,
        "nothing left to wait for"
    );
}

// ── test 3: synchronized update ──────────────────────────────────────────────────────────────

/// RED — **a synchronized update that never ends is committed when its deadline is advanced
/// past** (§6.2 test 3): `?2026h`, then output, then no `?2026l`.
///
/// MUTATION: make `Deadlines::synchronized_update_due` compare `deadline < now` — advancing
/// exactly at the deadline commits nothing and the output stays withheld.
#[test]
fn a_synchronized_update_that_never_ends_is_committed_at_its_deadline() {
    let start = Instant::now();
    let mut host = Host::new(40, 4, true);
    host.session
        .feed_at("\x1b[?2026h\u{4f60}\u{597d} held".as_bytes(), start)
        .unwrap();
    assert_eq!(host.compose(start), Composed::Published);
    assert!(
        !frame_text(host.pending.as_ref().unwrap()).contains("held"),
        "the update's output is withheld while it is open"
    );
    host.present(true);
    let due = deadlines(&host.session)
        .synchronized_update
        .expect("an open update has a deadline");

    let early = advance(&mut host.session, due - Duration::from_millis(1)).unwrap();
    assert!(!early.synchronized_update_finished);
    let advanced = advance(&mut host.session, due).unwrap();
    assert!(
        advanced.synchronized_update_finished,
        "committed at its deadline"
    );
    assert_eq!(deadlines(&host.session).synchronized_update, None);
    assert_eq!(host.compose(due), Composed::Published);
    let shown = frame_text(host.pending.as_ref().unwrap());
    assert!(
        shown.contains('\u{4f60}') && shown.contains('\u{597d}') && shown.contains("held"),
        "and the frame shows what it withheld: {shown:?}"
    );
}

// ── test 4: acknowledgment ───────────────────────────────────────────────────────────────────

/// RED — **the published revision moves exactly once per frame entered into the pending slot**
/// (§6.2 test 4): at entry, before any present; not for a held frame, not for an unchanged one;
/// and it is neither undone nor repeated when the present that follows fails and the frame is
/// filed again.
///
/// MUTATIONS: make `acknowledge` a no-op — the first entry does not move the revision; record the
/// frame twice in `acknowledge` — it moves by two.
#[test]
fn the_revision_moves_once_per_frame_entered_into_the_slot() {
    let start = Instant::now();
    let mut host = Host::new(60, 10, true);
    let mut first = b"\x1b[?1049h".to_vec();
    first.extend_from_slice(&cursor_bracketed_repaint(SCROLL_BEFORE));
    host.session.feed_at(&first, start).unwrap();
    // The formula on it becomes a picture first: a half-written repaint is held only over a
    // screen whose formula is already drawn.
    let settled = start + LIVE_MATH_STABLE_INTERVAL;
    advance(&mut host.session, settled).unwrap();
    answer_math(&mut host.session);
    let before = host.session.published_revision();

    assert_eq!(host.compose(settled), Composed::Published);
    assert_eq!(
        host.session.published_revision(),
        before + 1,
        "at entry, before any present"
    );
    host.present(false);
    assert!(
        host.pending.is_some(),
        "a failed present files the frame again"
    );
    assert_eq!(host.session.published_revision(), before + 1, "not undone");
    assert_eq!(host.compose(settled), Composed::Unchanged);
    host.present(true);
    assert_eq!(
        host.session.published_revision(),
        before + 1,
        "nor repeated by the retry or the unchanged frame"
    );

    let repaint = cursor_bracketed_repaint(SCROLL_AFTER);
    let at = start + Duration::from_millis(400);
    host.session
        .feed_at(&repaint[..repaint.len() / 2], at)
        .unwrap();
    assert_eq!(host.compose(at), Composed::Held);
    assert_eq!(
        host.session.published_revision(),
        before + 1,
        "a held frame"
    );

    host.session
        .feed_at(&repaint[repaint.len() / 2..], at + Duration::from_millis(2))
        .unwrap();
    assert_eq!(
        host.compose(at + Duration::from_millis(2)),
        Composed::Published
    );
    assert_eq!(host.session.published_revision(), before + 2);
}

// ── the draw list ────────────────────────────────────────────────────────────────────────────

/// RED — **the draw list is the focused pane first, carrying the caret answer it was handed, and
/// every other pane after it in order with no caret of its own.**
///
/// MUTATION: push the other panes with `focused: true` — the second entry claims the caret.
#[test]
fn the_draw_list_puts_the_focused_pane_first_and_only_it_may_own_the_caret() {
    let (mut left, mut left_view) = pane_of(20, 3);
    let (mut right, mut right_view) = pane_of(20, 3);
    left.feed("\u{5de6} left".as_bytes()).unwrap();
    right.feed("\u{53f3} right".as_bytes()).unwrap();
    let left_frame = left.viewport_frame(&mut left_view).unwrap();
    let right_frame = right.viewport_frame(&mut right_view).unwrap();
    let metrics = CellMetrics::measure(&mut bt_render::preview_measure_font_system(), 1.0).unwrap();
    let whole = SeatViewport::whole(200, 60);
    let draw = |frame| PaneDrawInput {
        seat: whole,
        clip: whole,
        frame,
        metrics,
    };

    for owns in [true, false] {
        let list = seat_frames(draw(&right_frame), owns, [draw(&left_frame)].into_iter());
        assert_eq!(list.len(), 2);
        assert!(std::ptr::eq(list[0].frame, &right_frame), "focused first");
        assert!(std::ptr::eq(list[1].frame, &left_frame));
        assert_eq!(list[0].focused, owns, "the caret answer the host gave");
        assert!(
            !list[1].focused,
            "no other pane's caret is where typing lands"
        );
    }
}

// ── the differential: the old path against the new (temporary migration evidence) ───────────
//
// Design T-COMPOSE-CRATE §6.1: the old path and the new draw the same input offscreen and the
// readbacks must be byte-equal. The old path is `bt-app`'s sites as `main` 6f09934a wrote them,
// inline; it is deleted from `bt-app` by this ticket, and this test is deleted with it.

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const WIDTH: u32 = 1000;
const HEIGHT: u32 = 360;

fn admit<D: bt_effects::admission::Door, R>(
    work: impl for<'scope> FnOnce(bt_effects::admission::WaitToken<'scope, D>) -> R,
) -> R {
    use bt_effects::admission::{Role, admitted, enter_window_thread, loop_running, role};
    if role() != Role::Window {
        assert!(
            enter_window_thread(),
            "this thread enters as the window thread"
        );
        assert!(loop_running(), "and its loop is running");
    }
    admitted::<D, R>(work).expect("admitted on the window thread")
}

/// What one turn of one road produced, compared field by field.
#[derive(Debug, PartialEq)]
struct Turn {
    frame: Option<ViewportFrame>,
    scheduled: usize,
    paths_asked: bool,
    filed: usize,
    revision: u64,
}

/// `publish_frame_inner`'s projection, hold and schedule, then the acknowledgment, as `main`
/// wrote them.
fn old_focused(
    session: &mut DualPlaneSession,
    view: &mut ViewportProjection,
    on_glass: bool,
    at: Instant,
) -> Turn {
    session.refresh_projection(view);
    let frame = session.viewport_frame(view).unwrap();
    let asked = session.absorb_printed_path_probes(view) != 0;
    if view.presentation_hold() && on_glass {
        return Turn {
            frame: None,
            scheduled: 0,
            paths_asked: asked,
            filed: answer_math(session),
            revision: session.published_revision(),
        };
    }
    let scheduled = session.schedule_visible_artifacts(&frame);
    session.record_published_frame(&frame, at);
    Turn {
        frame: Some(frame),
        scheduled,
        paths_asked: asked,
        filed: answer_math(session),
        revision: session.published_revision(),
    }
}

fn new_focused(
    session: &mut DualPlaneSession,
    view: &mut ViewportProjection,
    on_glass: bool,
    at: Instant,
) -> Turn {
    let projected = project(Pane { session, view }).unwrap();
    if projected.hold_requested && on_glass {
        return Turn {
            frame: None,
            scheduled: 0,
            paths_asked: projected.path_work_filed,
            filed: answer_math(session),
            revision: session.published_revision(),
        };
    }
    let scheduled = schedule(session, &projected.frame);
    acknowledge(session, &projected.frame, at);
    Turn {
        frame: Some(projected.frame),
        scheduled,
        paths_asked: projected.path_work_filed,
        filed: answer_math(session),
        revision: session.published_revision(),
    }
}

/// `redraw`'s unfocused pane, as `main` wrote it: no hold.
fn old_unfocused(session: &mut DualPlaneSession, view: &mut ViewportProjection) -> Turn {
    session.refresh_projection(view);
    let frame = session.viewport_frame(view).unwrap();
    let asked = session.absorb_printed_path_probes(view) != 0;
    let scheduled = session.schedule_visible_artifacts(&frame);
    Turn {
        frame: Some(frame),
        scheduled,
        paths_asked: asked,
        filed: answer_math(session),
        revision: session.published_revision(),
    }
}

fn new_unfocused(session: &mut DualPlaneSession, view: &mut ViewportProjection) -> Turn {
    let projected = project(Pane { session, view }).unwrap();
    let scheduled = schedule(session, &projected.frame);
    Turn {
        frame: Some(projected.frame),
        scheduled,
        paths_asked: projected.path_work_filed,
        filed: answer_math(session),
        revision: session.published_revision(),
    }
}

/// `advance_live_math_if_due`'s per-leaf body and the synchronized-update release, as `main`
/// wrote them.
fn old_advance(session: &mut DualPlaneSession, now: Instant) -> (bool, bool) {
    let finished = session
        .synchronized_update_deadline()
        .is_some_and(|deadline| deadline <= now)
        && session.finish_synchronized_update(now).unwrap();
    let mut settled = false;
    if session
        .live_stability_deadline()
        .is_some_and(|deadline| now >= deadline)
    {
        session.advance_live_stability(now);
        settled = true;
    }
    (finished, settled)
}

/// `redraw`'s seat-frame assembly, as `main` wrote it.
fn old_seat_frames<'a>(
    focused: PaneDrawInput<'a>,
    owner: bool,
    others: &[PaneDrawInput<'a>],
) -> Vec<bt_render::SeatFrame<'a>> {
    let mut seat_frames = Vec::with_capacity(others.len() + 1);
    seat_frames.push(bt_render::SeatFrame {
        seat: focused.seat,
        clip: focused.clip,
        frame: focused.frame,
        metrics: focused.metrics,
        focused: owner,
    });
    for pane in others {
        seat_frames.push(bt_render::SeatFrame {
            seat: pane.seat,
            clip: pane.clip,
            frame: pane.frame,
            metrics: pane.metrics,
            focused: false,
        });
    }
    seat_frames
}

/// **The old path and the new one, over the same bytes, make the same frames, schedule and
/// file the same work, acknowledge the same frames and draw the same pixels.**
#[test]
fn the_old_path_and_the_new_draw_the_same_pixels() {
    let start = Instant::now();
    let make = || {
        let (focused, focused_view) = pane_of(60, 10);
        let (unfocused, unfocused_view) = pane_of(60, 10);
        (focused, focused_view, unfocused, unfocused_view)
    };
    let (mut old_f, mut old_fv, mut old_u, mut old_uv) = make();
    let (mut new_f, mut new_fv, mut new_u, mut new_uv) = make();

    let mut screen = b"\x1b[?1049h".to_vec();
    screen.extend_from_slice(&cursor_bracketed_repaint(SCROLL_BEFORE));
    let repaint = cursor_bracketed_repaint(SCROLL_AFTER);
    let primary = "\u{4e2d}\u{6587} see ./Cargo.toml and src/lib.rs\r\n$$x^2 + y^2$$\r\n\x1b[?2026hheld \u{6d4b}\u{8bd5}"
        .as_bytes()
        .to_vec();
    let steps: Vec<(Duration, Vec<u8>, Vec<u8>)> = vec![
        (Duration::ZERO, screen.clone(), primary),
        (
            Duration::from_millis(400),
            repaint[..repaint.len() / 2].to_vec(),
            b"more\r\n".to_vec(),
        ),
        (
            Duration::from_millis(402),
            repaint[repaint.len() / 2..].to_vec(),
            Vec::new(),
        ),
        (
            Duration::from_millis(1500),
            Vec::new(),
            b"\x1b[?2026l\r\ndone\r\n".to_vec(),
        ),
    ];

    let mut gpu = pollster::block_on(bt_render::GpuContext::headless(FORMAT)).expect("a device");
    let mut old_window =
        bt_render::WindowRenderer::offscreen(&mut gpu, WIDTH, HEIGHT, 1.0, FORMAT).unwrap();
    let mut new_window =
        bt_render::WindowRenderer::offscreen(&mut gpu, WIDTH, HEIGHT, 1.0, FORMAT).unwrap();
    let metrics = old_window.base_metrics();
    let left = SeatViewport {
        x: 0,
        y: 0,
        width: WIDTH / 2,
        height: HEIGHT,
    };
    let right = SeatViewport {
        x: WIDTH / 2,
        y: 0,
        width: WIDTH / 2,
        height: HEIGHT,
    };
    let mut old_glass: Option<ViewportFrame> = None;
    let mut new_glass: Option<ViewportFrame> = None;
    let mut drawn = 0;
    let mut held = 0;

    for (offset, focused_bytes, unfocused_bytes) in steps {
        let at = start + offset;
        for (session, bytes) in [
            (&mut old_f, &focused_bytes),
            (&mut new_f, &focused_bytes),
            (&mut old_u, &unfocused_bytes),
            (&mut new_u, &unfocused_bytes),
        ] {
            session.feed_at(bytes, at).unwrap();
        }
        // The half-written repaint is composed at once, before any deadline could release it;
        // every other step lets each deadline pass first.
        let waits: &[Duration] = if offset == Duration::from_millis(400) {
            &[Duration::ZERO]
        } else {
            &[
                Duration::ZERO,
                LIVE_MATH_STABLE_INTERVAL,
                Duration::from_secs(2),
            ]
        };
        for now in waits.iter().map(|wait| at + *wait) {
            for (old, new) in [(&mut old_f, &mut new_f), (&mut old_u, &mut new_u)] {
                let before = old_advance(old, now);
                let after = advance(new, now).unwrap();
                assert_eq!(
                    before,
                    (
                        after.synchronized_update_finished,
                        after.live_stability_advanced
                    ),
                    "the same release at {now:?}"
                );
                assert_eq!(answer_math(old), answer_math(new), "the same work filed");
            }
        }
        let old_turn = old_focused(&mut old_f, &mut old_fv, old_glass.is_some(), at);
        let new_turn = new_focused(&mut new_f, &mut new_fv, new_glass.is_some(), at);
        assert_eq!(old_turn, new_turn, "the focused pane at {offset:?}");
        let old_side = old_unfocused(&mut old_u, &mut old_uv);
        let new_side = new_unfocused(&mut new_u, &mut new_uv);
        assert_eq!(old_side, new_side, "the unfocused pane at {offset:?}");
        match (old_turn.frame, new_turn.frame) {
            (Some(old_frame), Some(new_frame)) => {
                old_glass = Some(old_frame);
                new_glass = Some(new_frame);
            }
            _ => held += 1,
        }
        let (Some(old_frame), Some(new_frame)) = (&old_glass, &new_glass) else {
            continue;
        };
        let old_other = old_side.frame.unwrap();
        let new_other = new_side.frame.unwrap();
        let input = |frame, seat| PaneDrawInput {
            seat,
            clip: seat,
            frame,
            metrics,
        };
        let old_list = old_seat_frames(input(old_frame, left), true, &[input(&old_other, right)]);
        let new_list = seat_frames(
            input(new_frame, left),
            true,
            [input(&new_other, right)].into_iter(),
        );
        let trigger = bt_render::FrameTrigger {
            occurred_at: at,
            source: bt_render::FrameSource::Expose,
        };
        for (window, list) in [(&mut old_window, &old_list), (&mut new_window, &new_list)] {
            admit::<bt_effects::admission::doors::PresentFrame, _>(|token| {
                window.present_frame(token, &mut gpu, list, trigger)
            })
            .expect("the frame draws");
        }
        let old_pixels = old_window.read_back(&gpu).unwrap();
        let new_pixels = new_window.read_back(&gpu).unwrap();
        assert!(
            old_pixels.iter().any(|pixel| *pixel != old_pixels[0]),
            "something was drawn"
        );
        assert!(
            old_pixels == new_pixels,
            "the readbacks differ at {offset:?}"
        );
        drawn += 1;
    }
    assert_eq!(drawn, 4, "every step drew");
    assert_eq!(
        held, 1,
        "and the half-written repaint was held on both roads"
    );
    println!("CC6A_DIFFERENTIAL steps=4 drawn={drawn} held={held} readbacks=byte-equal");
}
