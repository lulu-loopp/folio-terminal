//! **Step 3 — bounded work** (design T-COMPOSE-CRATE §3.2): the session's decoration tasks, taken a
//! budget at a time and answered by the host's [`Executor`] on the calling thread.
//!
//! Every task taken gets exactly one terminal completion: the executor's answer, or the declined
//! completion for its kind. A declined task is final; the session shows the source text, as it
//! does for a formula that could not be rendered. `pump` never blocks and never starts a thread:
//! what an answer costs is the executor's, and the budget bounds how many are asked per call.

use std::path::Path;

use bt_doc::{
    BlockKind, SUBPIXELS_PER_PX,
    math::{MathRaster, MathRenderError},
};
use bt_term::{
    DecodedInlineImage, DualPlaneSession, InlineImageDecodeError, InlineImageScaleTask,
    InlineImageTask, PathVerdict, ScaledInlineImage, SessionDecorationTask, SessionMathTask,
};

/// An executor's answer to one task: the work done, or a refusal to do that kind of work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome<T> {
    Done(T),
    Declined,
}

/// **What a host can do with the session's decoration work.** One method per kind of task, each
/// answering on the calling thread. A host that cannot do a kind declines it; the pump gives the
/// session the declined completion for it.
pub trait Executor {
    /// Typeset one formula, in the host's own ink ([`crate::typeset`] with its engine). A proven
    /// table comes back as the empty raster `typeset` makes for one and is then measured by
    /// [`Self::table`].
    fn math(&mut self, task: &mut SessionMathTask) -> Outcome<Result<MathRaster, MathRenderError>>;
    /// The extent of a proven table at `font_size_px`: the raster the session records for it,
    /// measured by the host's own shaper (the picture is drawn by the host's renderer).
    fn table(
        &mut self,
        source: &str,
        font_size_px: f32,
    ) -> Outcome<Result<MathRaster, MathRenderError>>;
    /// Decode one picture.
    fn image(
        &mut self,
        task: &InlineImageTask,
    ) -> Outcome<Result<DecodedInlineImage, InlineImageDecodeError>>;
    /// Resample one decoded picture to the display size the layout shows it at.
    fn scale(&mut self, task: &InlineImageScaleTask) -> Outcome<ScaledInlineImage>;
    /// Ask the disk about one printed path.
    fn verify(&mut self, path: &Path) -> Outcome<PathVerdict>;
}

/// How much one [`pump`] call may take on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budget {
    /// The most tasks taken in this call.
    pub tasks: usize,
}

impl Default for Budget {
    /// One task per call.
    fn default() -> Self {
        Self { tasks: 1 }
    }
}

/// What one [`pump`] call did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PumpReport {
    /// Tasks answered with the executor's result.
    pub completed: usize,
    /// Tasks the executor declined, each given its declined completion.
    pub declined: usize,
    /// The session is still owed answers ([`DualPlaneSession::outstanding_decoration_work`]).
    ///
    /// That count includes frozen formulas a full queue turned away, which wait for the next
    /// [`crate::schedule`] to be filed again and which no pump can take. So it is not a loop
    /// condition on its own: a host keeps running the per-frame sequence (advance, project,
    /// schedule, pump) and pumps within it, or a queue overflow reads as pending for ever.
    pub more_pending: bool,
}

/// **Take at most `budget.tasks` decoration tasks and answer each one**, through `executor`, in
/// the order the session hands them out. Every task taken is completed — with the executor's
/// result or, when it declines, with the declined completion for its kind — before the next is
/// taken, so no task is held across calls and none is taken twice.
///
/// It drains what is queued, not what the session owes: work a full queue turned away comes back
/// through [`crate::schedule`], so a host pumps inside its per-frame sequence (the crate header's
/// steps 1–6) rather than only while [`PumpReport::more_pending`] holds.
pub fn pump(
    session: &mut DualPlaneSession,
    executor: &mut dyn Executor,
    budget: Budget,
) -> PumpReport {
    let mut report = PumpReport::default();
    for _ in 0..budget.tasks {
        let Some(task) = session.take_decoration_worker_task() else {
            break;
        };
        let answered = match task {
            SessionDecorationTask::Math(task) => answer_math(session, executor, *task),
            SessionDecorationTask::InlineImage(task) => match executor.image(&task) {
                Outcome::Done(result) => {
                    session.complete_inline_image_result(task, result);
                    Answered::Completed
                }
                Outcome::Declined => {
                    session.complete_inline_image_result(
                        task,
                        Err(InlineImageDecodeError::HostDeclined),
                    );
                    Answered::Declined
                }
            },
            SessionDecorationTask::ScaleInlineImage(task) => match executor.scale(&task) {
                Outcome::Done(scaled) => {
                    session.complete_inline_image_scale(scaled);
                    Answered::Completed
                }
                Outcome::Declined => {
                    session.decline_inline_image_scale(&task);
                    Answered::Declined
                }
            },
            SessionDecorationTask::VerifyPath(path) => match executor.verify(&path) {
                Outcome::Done(verdict) => {
                    session.complete_path_verification(path, verdict);
                    Answered::Completed
                }
                Outcome::Declined => {
                    session.decline_path_verification(path);
                    Answered::Declined
                }
            },
        };
        match answered {
            Answered::Completed => report.completed += 1,
            Answered::Declined => report.declined += 1,
        }
    }
    report.more_pending = session.outstanding_decoration_work() > 0;
    report
}

/// Which completion a task taken by [`pump`] was given.
enum Answered {
    Completed,
    Declined,
}

/// A formula's answer, and a table's extent after it: the worker proves a table and returns no
/// picture, and the host measures the block at the size the task was laid out at, as `bt-app`'s
/// lane does before it hands the session the result.
fn answer_math(
    session: &mut DualPlaneSession,
    executor: &mut dyn Executor,
    mut task: SessionMathTask,
) -> Answered {
    let (result, answered) = match executor.math(&mut task) {
        Outcome::Done(result) => match table_of(&task) {
            Some((source, font_size_px)) => match executor.table(&source, font_size_px) {
                Outcome::Done(result) => (result, Answered::Completed),
                Outcome::Declined => (Err(MathRenderError::HostDeclined), Answered::Declined),
            },
            None => (result, Answered::Completed),
        },
        Outcome::Declined => (Err(MathRenderError::HostDeclined), Answered::Declined),
    };
    match task {
        SessionMathTask::Frozen(task) => session.complete_worker_result(task, result),
        SessionMathTask::Live(task) => session.complete_live_worker_result(task, result),
    };
    answered
}

/// A proven table's source and the em it was laid out at, or `None` for a formula.
fn table_of(task: &SessionMathTask) -> Option<(String, f32)> {
    let (span, layout) = match task {
        SessionMathTask::Frozen(task) => (&task.span, task.versions.layout),
        SessionMathTask::Live(task) => (&task.span, task.layout),
    };
    (span.kind == BlockKind::Table).then(|| {
        (
            span.render_source.clone(),
            layout.font_size_subpixels as f32 / SUBPIXELS_PER_PX as f32,
        )
    })
}

#[cfg(test)]
#[path = "pump_tests.rs"]
mod tests;
