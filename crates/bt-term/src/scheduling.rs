use std::{
    collections::{BTreeSet, VecDeque},
    time::{Duration, Instant},
};

use bt_detect::DetectionTask;
use bt_transcript::TranscriptId;

/// DESIGN.md §1.3 per-session parser quantum.
pub const PARSE_QUANTUM: usize = 256 * 1024;
/// DESIGN.md §1.3 per-session worker queue capacity.
pub const WORKER_QUEUE_CAP: usize = 64;

/// Winit 0.30 exposes `WindowEvent::Resized` but no Windows enter/exit-size-move boundary. The app
/// therefore sends only the last ConPTY size after this event-silence interval.
pub const RESIZE_REQUEST_QUIET: Duration = Duration::from_millis(200);
/// Keep vendor ownership open after the final ConPTY resize until its output pipe is quiet.
pub const RESIZE_IDLE_AFTER_OUTPUT: Duration = Duration::from_millis(200);

/// The life of one resize transaction: opened by a reflow, closed only by a settlement.
///
/// **The invariant, and it is the whole of it: every gesture that calls [`Self::changed`] must
/// reach [`Self::final_request_sent`].** `changed` is what `DualPlaneSession::resize_at` calls, so
/// the epoch opens the instant this pane's own grid follows the hand; `final_request_sent` has
/// exactly two callers, the two doors a settled gesture comes through
/// (`mark_pty_resize_requested_at` and `mark_resize_settled_unchanged_at`), and without it
/// [`Self::quiescence_deadline`] is `None`, [`Self::is_quiescent_at`] is false at every instant
/// there is, and [`Self::decorations_allowed`] is false for the rest of the pane's life — no
/// formula is ever scanned for again, silently. That is the shape of two shipped defects: an
/// unfocused pane that was never reconciled (2026-08-24), and a drag whose last wobble returned to
/// the width the child already had, which the window had no way to report at all (2026-09-17).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResizeEpoch {
    current: u64,
    quiescent: u64,
    last_geometry: Option<Instant>,
    final_request: Option<Instant>,
    last_output: Option<Instant>,
}

impl ResizeEpoch {
    pub(crate) fn changed(&mut self, now: Instant) {
        self.current += 1;
        self.last_geometry = Some(now);
        self.final_request = None;
    }

    pub(crate) fn mark_quiescent(&mut self) {
        self.quiescent = self.current;
        self.last_geometry = None;
        self.final_request = None;
        self.last_output = None;
    }

    pub(crate) fn observe_output(&mut self, now: Instant) {
        if self.is_active() {
            self.last_output = Some(now);
        }
    }

    pub(crate) fn final_request_sent(&mut self, now: Instant) {
        if self.is_active() {
            self.final_request = Some(now);
        }
    }

    pub(crate) fn is_active(self) -> bool {
        self.current != self.quiescent
    }

    pub(crate) fn request_deadline(self) -> Option<Instant> {
        self.is_active()
            .then(|| self.last_geometry.map(|at| at + RESIZE_REQUEST_QUIET))
            .flatten()
    }

    pub(crate) fn quiescence_deadline(self) -> Option<Instant> {
        let request = self.final_request?;
        Some(
            self.last_output
                .map_or(request + RESIZE_IDLE_AFTER_OUTPUT, |output| {
                    request.max(output) + RESIZE_IDLE_AFTER_OUTPUT
                }),
        )
    }

    pub(crate) fn is_quiescent_at(self, now: Instant) -> bool {
        self.quiescence_deadline()
            .is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn decorations_allowed(self) -> bool {
        self.current == self.quiescent
    }
}

#[cfg(test)]
mod resize_epoch_tests {
    use super::*;

    #[test]
    fn final_request_and_output_silence_bound_transaction_end() {
        let start = Instant::now();
        let mut epoch = ResizeEpoch::default();
        epoch.changed(start);
        assert_eq!(epoch.request_deadline(), Some(start + RESIZE_REQUEST_QUIET));
        assert_eq!(epoch.quiescence_deadline(), None);

        epoch.final_request_sent(start + Duration::from_millis(200));
        epoch.observe_output(start + Duration::from_millis(250));
        assert!(!epoch.is_quiescent_at(start + Duration::from_millis(449)));
        assert!(epoch.is_quiescent_at(start + Duration::from_millis(450)));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnqueueOutcome {
    Queued,
    RetryOnIdle,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkerScheduler {
    pending: VecDeque<DetectionTask>,
    retry_on_idle: BTreeSet<TranscriptId>,
}

impl WorkerScheduler {
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn retry_len(&self) -> usize {
        self.retry_on_idle.len()
    }

    pub(crate) fn has_retry(&self) -> bool {
        !self.retry_on_idle.is_empty()
    }

    pub(crate) fn retry_sources(&self, limit: usize) -> Vec<TranscriptId> {
        self.retry_on_idle.iter().copied().take(limit).collect()
    }

    pub(crate) fn take(&mut self) -> Option<DetectionTask> {
        self.pending.pop_front()
    }

    pub(crate) fn enqueue(&mut self, task: DetectionTask) -> EnqueueOutcome {
        if let Some(index) = self
            .pending
            .iter()
            .position(|queued| queued.candidate_id == task.candidate_id)
        {
            self.pending.remove(index);
        }
        if self.pending.len() == WORKER_QUEUE_CAP {
            self.retry_on_idle.insert(task.candidate_id);
            EnqueueOutcome::RetryOnIdle
        } else {
            self.retry_on_idle.remove(&task.candidate_id);
            self.pending.push_back(task);
            EnqueueOutcome::Queued
        }
    }

    pub(crate) fn remove_sources(&mut self, removed: &BTreeSet<TranscriptId>) {
        self.pending
            .retain(|task| !removed.contains(&task.candidate_id));
        self.retry_on_idle.retain(|id| !removed.contains(id));
    }
}
