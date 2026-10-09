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
