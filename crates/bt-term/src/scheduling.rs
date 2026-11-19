use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

use web_time::Instant;

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
/// **The longest a resize transaction waits for that quiet once the child has been told**
/// (0.4.10 plan row 108).
///
/// The quiet window is a guess that the child's reaction to its new size is over: neither ConPTY
/// nor `SIGWINCH` says when a child has redrawn, so there is no acknowledgement to wait for
/// instead. A child that writes more often than every [`RESIZE_IDLE_AFTER_OUTPUT`] (an agent's
/// spinner, a progress bar) never gives one, and the transaction stayed open for as long as it
/// kept writing: every formula it printed stayed source, and every row it scrolled was held two
/// more times beside the transcript's copy. A child still writing this long after being told has
/// had five quiet windows to react, so the transaction settles here at the latest.
pub const RESIZE_SETTLE_BOUND: Duration = Duration::from_secs(1);
/// **The most a resize transaction holds beyond what it held when the child was told**, in cells
/// (0.4.10 plan row 108).
///
/// Every row that scrolls off while a transaction is open stays in its tail: once in the
/// transcript's resize staging, once in the displayed grid's escrow and once in the canonical
/// fork's history, about 131 bytes a cell together (measured at 40 columns), and the whole tail
/// is staged again at every feed. Counted from the final request, so the rows the reflow itself
/// pushed off are not counted against it. 131 072 cells is about 16 MB, five screens of a
/// 200 x 120 pane, and a few milliseconds of re-staging a turn at most. It is checked at each
/// turn's end, so the tail can pass it by one drain turn's rows.
pub const RESIZE_STAGING_BUDGET_CELLS: usize = 131_072;

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

    /// When this transaction may settle: once the child has been told, at the first of its output
    /// going quiet and [`RESIZE_SETTLE_BOUND`] after the final request. `None` until it is told.
    pub(crate) fn quiescence_deadline(self) -> Option<Instant> {
        let request = self.final_request?;
        Some(
            self.quiet_deadline(request)
                .min(request + RESIZE_SETTLE_BOUND),
        )
    }

    fn quiet_deadline(self, request: Instant) -> Instant {
        self.last_output
            .map_or(request + RESIZE_IDLE_AFTER_OUTPUT, |output| {
                request.max(output) + RESIZE_IDLE_AFTER_OUTPUT
            })
    }

    /// The child has been told, and its output has been quiet for [`RESIZE_IDLE_AFTER_OUTPUT`].
    pub(crate) fn is_quiescent_at(self, now: Instant) -> bool {
        self.final_request
            .is_some_and(|request| now >= self.quiet_deadline(request))
    }

    /// The child was told at least [`RESIZE_SETTLE_BOUND`] ago, whatever it has written since.
    pub(crate) fn settle_bound_reached_at(self, now: Instant) -> bool {
        self.final_request
            .is_some_and(|request| now >= request + RESIZE_SETTLE_BOUND)
    }

    /// Whether the window has told the child the size this transaction ends at. Until it has,
    /// nothing may settle the transaction: the grid cannot be harvested against a size the
    /// child does not hold.
    pub(crate) fn child_told(self) -> bool {
        self.final_request.is_some()
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

    /// T6 (0.4.10 plan row 108). Output that never goes quiet moves the quiet deadline on for
    /// ever; the deadline the window wakes for stops at [`RESIZE_SETTLE_BOUND`] after the final
    /// request, and only from the request on.
    #[test]
    fn the_deadline_stops_at_the_bound_after_the_final_request() {
        let start = Instant::now();
        let mut epoch = ResizeEpoch::default();
        epoch.changed(start);
        epoch.observe_output(start + Duration::from_secs(5));
        assert_eq!(
            epoch.quiescence_deadline(),
            None,
            "nothing settles before the child is told"
        );
        assert!(!epoch.settle_bound_reached_at(start + Duration::from_secs(60)));

        let request = start + Duration::from_secs(10);
        epoch.final_request_sent(request);
        let mut at = request;
        while at < request + Duration::from_secs(3) {
            epoch.observe_output(at);
            at += Duration::from_millis(100);
        }
        assert_eq!(
            epoch.quiescence_deadline(),
            Some(request + RESIZE_SETTLE_BOUND),
            "output every 100 ms leaves the bound as the deadline"
        );
        assert!(!epoch.is_quiescent_at(at));
        assert!(
            !epoch
                .settle_bound_reached_at(request + RESIZE_SETTLE_BOUND - Duration::from_millis(1))
        );
        assert!(epoch.settle_bound_reached_at(request + RESIZE_SETTLE_BOUND));

        let mut quiet = ResizeEpoch::default();
        quiet.changed(start);
        quiet.final_request_sent(start);
        quiet.observe_output(start + Duration::from_millis(50));
        assert_eq!(
            quiet.quiescence_deadline(),
            Some(start + Duration::from_millis(250)),
            "a child that goes quiet settles on the quiet window as before"
        );
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

    /// A task [`Self::take`] handed out that its host could not send, back at the head.
    pub(crate) fn give_back(&mut self, task: DetectionTask) {
        self.pending.push_front(task);
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
