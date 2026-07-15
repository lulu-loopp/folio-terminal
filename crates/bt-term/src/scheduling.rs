use std::collections::{BTreeSet, VecDeque};

use bt_detect::DetectionTask;
use bt_transcript::TranscriptId;

/// DESIGN.md §1.3 per-session parser quantum.
pub const PARSE_QUANTUM: usize = 256 * 1024;
/// DESIGN.md §1.3 per-session worker queue capacity.
pub const WORKER_QUEUE_CAP: usize = 64;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResizeEpoch {
    current: u64,
    quiescent: u64,
}

impl ResizeEpoch {
    pub(crate) fn changed(&mut self) {
        self.current += 1;
    }

    pub(crate) fn mark_quiescent(&mut self) {
        self.quiescent = self.current;
    }

    pub(crate) fn decorations_allowed(self) -> bool {
        self.current == self.quiescent
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

    pub(crate) fn take(&mut self) -> Option<DetectionTask> {
        self.pending.pop_front()
    }

    pub(crate) fn enqueue(&mut self, task: DetectionTask) -> EnqueueOutcome {
        if let Some(index) = self
            .pending
            .iter()
            .position(|queued| queued.transcript_id == task.transcript_id)
        {
            self.pending.remove(index);
        }
        if self.pending.len() == WORKER_QUEUE_CAP {
            self.retry_on_idle.insert(task.transcript_id);
            EnqueueOutcome::RetryOnIdle
        } else {
            self.retry_on_idle.remove(&task.transcript_id);
            self.pending.push_back(task);
            EnqueueOutcome::Queued
        }
    }

    pub(crate) fn remove_sources(&mut self, removed: &BTreeSet<TranscriptId>) {
        self.pending
            .retain(|task| !removed.contains(&task.transcript_id));
        self.retry_on_idle.retain(|id| !removed.contains(id));
    }
}
