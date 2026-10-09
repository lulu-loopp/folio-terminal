//! **Terminal composition: the order one pane's frame is made in.**
//!
//! Every operation on the grid-and-formula path is a public library call of
//! `bt-term`, `bt-viewport` and `bt-render`. What this crate owns is the
//! **order** around them, the one contract a host has to keep the same on
//! every platform it draws on (design T-COMPOSE-CRATE §3.2):
//!
//! 1. bytes are fed to the session (the host's call);
//! 2. [`advance`] — a synchronized update or a live formula whose deadline has
//!    passed is released ([`deadlines`] says when to wake for it);
//! 3. [`project`] — the frame is projected; it is **not** scheduled;
//! 4. if the frame is not held: [`schedule`], then the decoration work runs —
//!    on the host's own lane, or a budget at a time through [`pump`] and the
//!    host's [`Executor`];
//! 5. if the frame changed: [`acknowledge`], and the frame enters the host's
//!    pending slot;
//! 6. the pending slot is presented — through [`seat_frames`], under the
//!    host's own admission.
//!
//! **Composition owns math execution** (design §3.4 D-15): [`typeset`] is the
//! one place a terminal formula is turned into a raster, whichever thread or
//! host runs it.
//!
//! **A borrowing coordinator.** The host owns every session (`DualPlaneSession`)
//! and every view (`ViewportProjection`, the cell metrics it was drawn at, the
//! last presented picture) and lends them for one call ([`Pane`]); nothing is
//! kept across calls. The host also owns the GPU context, the window renderer,
//! the surface, present admission and the fonts.
//!
//! **No direct effects.** This crate reads no file, no environment and no clock
//! of its own, waits on nothing and starts no thread; `bt-app`'s window-waits
//! guard holds it to that. The instants it is handed come from the host.

#![cfg_attr(test, allow(clippy::disallowed_methods))]

mod pump;
mod typeset;

/// The engine [`typeset`] and the two functions beneath it are handed, and the render key that
/// engine is asked at a pane's em: a caller of composition names both here, beside the functions
/// that take them, without a dependency on the math crate of its own (`bt-term`'s session tests,
/// whose manifest names no math crate).
pub use bt_math::{MathEngine, key_for_em_px};
pub use pump::{Budget, Executor, Outcome, PumpReport, pump};
pub use typeset::{render_detection_task, render_live_detection_task, typeset};

use bt_render::{CellMetrics, SeatFrame, SeatViewport};
use bt_term::{DualPlaneSession, SessionError};
use bt_viewport::{FrameProjectionError, ViewportFrame, ViewportProjection};
use web_time::Instant;

/// **One pane, lent for one call.** The two halves are owned separately by the
/// host (`docs/ARCHITECTURE.md` §4.1): the session survives the view, and a
/// view is replaced without the session noticing.
pub struct Pane<'a> {
    pub session: &'a mut DualPlaneSession,
    pub view: &'a mut ViewportProjection,
}

/// What [`project`] made of one pane.
pub struct Projected {
    pub frame: ViewportFrame,
    /// `ViewportProjection::presentation_hold`, read after the projection. The
    /// host combines it with its own "a frame is already on the glass" fact; a
    /// held pane is not scheduled and not published this turn.
    pub hold_requested: bool,
    /// The frame named paths nobody has answered for yet, and the session filed
    /// them as path-verification work: the host's lane has work to take.
    pub path_work_filed: bool,
}

/// **Step 3 — project one pane.** The projection is refreshed, the frame is
/// built from it, and the paths that frame printed are filed as questions.
///
/// It does **not** schedule decoration work: whether this frame may be acted on
/// is the hold's answer, and the hold is read here and decided by the host
/// before [`schedule`].
pub fn project(pane: Pane<'_>) -> Result<Projected, FrameProjectionError> {
    let Pane { session, view } = pane;
    session.refresh_projection(view);
    let frame = session.viewport_frame(view)?;
    let path_work_filed = session.absorb_printed_path_probes(view) != 0;
    Ok(Projected {
        frame,
        hold_requested: view.presentation_hold(),
        path_work_filed,
    })
}

/// **Step 4 — schedule the decoration work a frame shows**, only for a frame
/// that is not held. Frozen candidates intersecting the frame, and live rows
/// that have already settled; an unsettled live row waits for [`advance`].
/// Returns how many tasks it filed.
pub fn schedule(session: &mut DualPlaneSession, frame: &ViewportFrame) -> usize {
    session.schedule_visible_artifacts(frame)
}

/// The instants a host must wake for, one pane's worth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deadlines {
    /// A DEC 2026 synchronized update that has not ended: it is committed when
    /// this passes.
    pub synchronized_update: Option<Instant>,
    /// A live row, a pane math row or a repaint transaction still settling: it
    /// is settled when this passes.
    pub live_stability: Option<Instant>,
}

impl Deadlines {
    /// Whether the synchronized update's timeout has run out at `now`.
    pub fn synchronized_update_due(&self, now: Instant) -> bool {
        self.synchronized_update
            .is_some_and(|deadline| deadline <= now)
    }
}

/// When this pane needs seeing to again, with no byte arriving.
pub fn deadlines(session: &DualPlaneSession) -> Deadlines {
    Deadlines {
        synchronized_update: session.synchronized_update_deadline(),
        live_stability: session.live_stability_deadline(),
    }
}

/// **Settle the live rows whose stability window has run out at `now`.**
/// `schedule` alone never schedules a live row that has not settled, so without
/// this a live formula never becomes a picture. Returns whether the pane's
/// deadline had passed and it was advanced.
pub fn advance_live_stability(session: &mut DualPlaneSession, now: Instant) -> bool {
    if session
        .live_stability_deadline()
        .is_some_and(|deadline| now >= deadline)
    {
        session.advance_live_stability(now);
        return true;
    }
    false
}

/// What [`advance`] released.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Advanced {
    /// A synchronized update had run out and its held bytes were committed to
    /// the screen.
    pub synchronized_update_finished: bool,
    /// The live stability deadline had passed and the pane's rows were settled.
    pub live_stability_advanced: bool,
}

/// **Step 2 — release what has run out at `now`**: a synchronized update whose
/// timeout passed is committed first, then the live rows that have settled are
/// advanced — the order a native turn takes them in.
///
/// The web host's road: `bt-app` does not call it. Its turn runs the same two steps
/// apart — `finish_synchronized_update_if_due` (which reads a pane's name evidence
/// between "due" and "finish") and `advance_live_math_if_due` (through
/// [`advance_live_stability`]).
pub fn advance(session: &mut DualPlaneSession, now: Instant) -> Result<Advanced, SessionError> {
    let synchronized_update_finished = deadlines(session).synchronized_update_due(now)
        && session.finish_synchronized_update(now)?;
    let live_stability_advanced = advance_live_stability(session, now);
    Ok(Advanced {
        synchronized_update_finished,
        live_stability_advanced,
    })
}

/// **Step 5 — acknowledge a frame, at the publish boundary.** Exactly once for
/// each frame the host enters into its pending slot (the queue a later present
/// drains), immediately before that entry: never after a GPU present, never for
/// a held frame, never for one skipped as unchanged. A present that then fails
/// or is delayed does not undo it, and a frame the host files again after such
/// a present is not acknowledged a second time.
pub fn acknowledge(session: &mut DualPlaneSession, frame: &ViewportFrame, at: Instant) {
    session.record_published_frame(frame, at);
}

/// One pane's entry in the draw list: where its contents were laid out, the box
/// they may appear in, the frame, and the metrics the frame was projected at.
#[derive(Clone, Copy, Debug)]
pub struct PaneDrawInput<'a> {
    pub seat: SeatViewport,
    pub clip: SeatViewport,
    pub frame: &'a ViewportFrame,
    pub metrics: CellMetrics,
}

/// **The draw list a present is handed**: the focused pane first, then every
/// other pane in the order given.
///
/// `focused_owns_the_caret` is the focused entry's `SeatFrame::focused`, which
/// `seat_caret` alone reads, and what it is asked there is "is this the caret
/// typing would land in" — the keyboard's owner, not the focus, so the host
/// answers it (a shell owns the keyboard, or a preview or a dialog does). Every
/// other pane's caret is not where typing lands.
pub fn seat_frames<'a>(
    focused: PaneDrawInput<'a>,
    focused_owns_the_caret: bool,
    others: impl ExactSizeIterator<Item = PaneDrawInput<'a>>,
) -> Vec<SeatFrame<'a>> {
    let mut seat_frames = Vec::with_capacity(others.len() + 1);
    seat_frames.push(SeatFrame {
        seat: focused.seat,
        clip: focused.clip,
        frame: focused.frame,
        metrics: focused.metrics,
        focused: focused_owns_the_caret,
    });
    for pane in others {
        seat_frames.push(SeatFrame {
            seat: pane.seat,
            clip: pane.clip,
            frame: pane.frame,
            metrics: pane.metrics,
            focused: false,
        });
    }
    seat_frames
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
