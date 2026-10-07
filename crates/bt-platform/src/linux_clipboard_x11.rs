//! Controlled X11 ownership using Arboard's ICCCM request-serving logic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;
use std::time::Instant;

use arboard::X11OwnerCandidate as ArboardX11OwnerCandidate;

use crate::admission::WorkerCtx;
use crate::linux_clipboard_x11_transport::{
    CANCEL_POLL as X11_CANCEL_POLL, X11TransportControl, connect_display_controlled,
};

const X11_OWNER_THREAD: &str = "folio-x11-clipboard-owner";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimOutcome {
    /// Connection, atoms, or owner window setup failed before a claim request.
    FailedBeforeClaim(String),
    /// The same-connection owner reply named the candidate window.
    Confirmed,
    /// The claim may have taken effect, but no owner reply resolved it.
    Unconfirmed(String),
    /// The ordered owner reply named another window or no owner.
    ConclusiveNoLongerOwner(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileOutcome {
    CandidateOwns,
    OtherOwner(u32),
    Unconfirmed(String),
}

#[derive(Debug)]
pub enum StartError {
    FailedBeforeClaim(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FailedBeforeClaim(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for StartError {}

enum OwnerCommand {
    Reconcile {
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        response: SyncSender<ReconcileOutcome>,
    },
    Retire {
        cutoff: Instant,
        cancelled: Option<Arc<AtomicBool>>,
        response: SyncSender<RetireOutcome>,
    },
}

struct RetireOutcome {
    finished: bool,
    result: Result<(), String>,
}

/// One independent X11 owner candidate and its joinable serving worker.
///
/// The candidate retains its own Arboard `Inner`; no bytes or server thread
/// are shared with Arboard's process-global owner cache.
#[must_use = "retain the X11 owner until it is reconciled or retired"]
pub struct X11OwnerCandidate {
    commands: mpsc::SyncSender<OwnerCommand>,
    claim_result: Receiver<ClaimOutcome>,
    claim_outcome: Option<ClaimOutcome>,
    completed: Receiver<Result<(), String>>,
    lifecycle_cancelled: Arc<AtomicBool>,
    serving_thread: Option<JoinHandle<()>>,
}

impl X11OwnerCandidate {
    /// Spawn the serving worker. The borrowed caller capability is never
    /// carried into the new thread; that worker receives its own `WorkerCtx`.
    pub fn start(
        _worker: &WorkerCtx,
        text: String,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, StartError> {
        let lifecycle_cancelled = Arc::new(AtomicBool::new(false));
        let (command_tx, command_rx) = mpsc::sync_channel(1);
        let (claim_tx, claim_result) = mpsc::channel();
        let (completed_tx, completed) = mpsc::channel();
        let thread_lifecycle = Arc::clone(&lifecycle_cancelled);
        let thread_cancelled = Arc::clone(&cancelled);

        let serving_thread = crate::spawn_at_priority(
            X11_OWNER_THREAD,
            crate::ThreadPriority::Normal,
            move |worker| {
                let result = serve_candidate(
                    worker,
                    text,
                    deadline,
                    thread_cancelled,
                    thread_lifecycle,
                    command_rx,
                    claim_tx,
                );
                let _ = completed_tx.send(result);
            },
        )
        .map_err(|error| StartError::FailedBeforeClaim(error.to_string()))?;

        Ok(Self {
            commands: command_tx,
            claim_result,
            claim_outcome: None,
            completed,
            lifecycle_cancelled,
            serving_thread: Some(serving_thread),
        })
    }

    /// Wait through the admission deadline for the same-connection owner
    /// reply. An empty wait is reported conservatively and leaves the
    /// candidate available for reconciliation.
    pub fn await_claim_until(
        &mut self,
        _worker: &WorkerCtx,
        deadline: Instant,
        cancelled: &AtomicBool,
    ) -> ClaimOutcome {
        if let Some(outcome) = &self.claim_outcome {
            return outcome.clone();
        }
        loop {
            if cancelled.load(Ordering::Acquire) {
                return ClaimOutcome::Unconfirmed(
                    "X11 clipboard owner operation was cancelled".to_owned(),
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return ClaimOutcome::Unconfirmed(
                    "X11 clipboard owner operation exceeded its deadline".to_owned(),
                );
            }
            match self
                .claim_result
                .recv_timeout(remaining.min(X11_CANCEL_POLL))
            {
                Ok(outcome) => {
                    self.claim_outcome = Some(outcome.clone());
                    return outcome;
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return ClaimOutcome::Unconfirmed(
                        "X11 clipboard owner worker ended before reporting its claim".to_owned(),
                    );
                }
            }
        }
    }

    /// Reconcile an earlier candidate on its original connection. This sends
    /// only `GetSelectionOwner`; it never claims or reclaims the selection.
    pub fn reconcile_until(
        &mut self,
        worker: &WorkerCtx,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
    ) -> ReconcileOutcome {
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        if self
            .commands
            .try_send(OwnerCommand::Reconcile {
                deadline,
                cancelled: Arc::clone(&cancelled),
                response: response_tx,
            })
            .is_err()
        {
            return ReconcileOutcome::Unconfirmed(
                "X11 clipboard owner worker is unavailable for reconciliation".to_owned(),
            );
        }
        wait_for_response(worker, response_rx, deadline, &cancelled).unwrap_or_else(|| {
            ReconcileOutcome::Unconfirmed(if cancelled.load(Ordering::Acquire) {
                "X11 clipboard owner reconciliation was cancelled".to_owned()
            } else {
                "X11 clipboard owner reconciliation exceeded its deadline".to_owned()
            })
        })
    }

    /// Request this owner's serving worker to stop without waiting for its
    /// connection to process another event.
    pub fn cancel(&self) {
        self.lifecycle_cancelled.store(true, Ordering::Release);
    }

    /// Bounded manager handover, owner-window destruction, cancellation, and
    /// serving-thread join. On error, this value still owns the join handle.
    pub fn retire_until(&mut self, worker: &WorkerCtx, cutoff: Instant) -> Result<(), String> {
        self.retire_until_cancellable(worker, cutoff, None)
    }

    /// The same bounded retirement while allowing the current admitted
    /// operation to abandon its wait and leave this candidate joinable.
    pub fn retire_until_cancellable(
        &mut self,
        worker: &WorkerCtx,
        cutoff: Instant,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> Result<(), String> {
        if self.serving_thread.is_none() {
            return Ok(());
        }
        if cancelled
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
        {
            return Err("X11 clipboard owner retirement was canceled by its caller".to_owned());
        }
        if self.lifecycle_cancelled.load(Ordering::Acquire) {
            return self.join_until(worker, cutoff);
        }
        loop {
            if cancelled
                .as_ref()
                .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
            {
                return Err("X11 clipboard owner retirement was canceled by its caller".to_owned());
            }
            if self.lifecycle_cancelled.load(Ordering::Acquire) {
                return self.join_until(worker, cutoff);
            }
            if self
                .serving_thread
                .as_ref()
                .is_some_and(|owner| owner.is_finished())
            {
                return self.join_completed_until(worker, cutoff, cancelled.as_deref())?;
            }
            let (response_tx, response_rx) = mpsc::sync_channel(1);
            queue_retire_until_space(
                worker,
                OwnerCommand::Retire {
                    cutoff,
                    cancelled: cancelled.clone(),
                    response: response_tx,
                },
                cutoff,
                cancelled.as_deref(),
                &self.lifecycle_cancelled,
                |command| self.commands.try_send(command),
            )?;

            let mut final_response = None;
            loop {
                if cancelled
                    .as_ref()
                    .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
                {
                    return Err(
                        "X11 clipboard owner retirement was canceled by its caller".to_owned()
                    );
                }
                let remaining = cutoff.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    self.lifecycle_cancelled.store(true, Ordering::Release);
                    return Err("X11 clipboard owner retirement exceeded its cutoff".to_owned());
                }
                if remaining <= X11_CANCEL_POLL {
                    self.lifecycle_cancelled.store(true, Ordering::Release);
                }
                match response_rx.recv_timeout(remaining.min(X11_CANCEL_POLL)) {
                    Ok(outcome) if !outcome.finished => {
                        if cancelled
                            .as_ref()
                            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
                        {
                            return outcome.result.and_then(|()| {
                                Err("X11 clipboard owner retirement was canceled by its caller"
                                    .to_owned())
                            });
                        }
                        if outcome.result.is_ok() {
                            break;
                        }
                        return outcome.result;
                    }
                    Ok(outcome) => final_response = Some(outcome),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        self.lifecycle_cancelled.store(true, Ordering::Release);
                    }
                }
                if self
                    .serving_thread
                    .as_ref()
                    .is_some_and(|owner| owner.is_finished())
                {
                    let completion =
                        self.join_completed_until(worker, cutoff, cancelled.as_deref())?;
                    return final_response
                        .map_or(Ok(()), |outcome| outcome.result)
                        .and(completion);
                }
            }
        }
    }

    fn join_until(&mut self, worker: &WorkerCtx, cutoff: Instant) -> Result<(), String> {
        self.join_completed_until(worker, cutoff, None)?
    }

    fn join_completed_until(
        &mut self,
        worker: &WorkerCtx,
        cutoff: Instant,
        cancelled: Option<&AtomicBool>,
    ) -> Result<Result<(), String>, String> {
        loop {
            if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
                return Err("X11 clipboard owner retirement was canceled by its caller".to_owned());
            }
            if Instant::now() >= cutoff {
                return Err("X11 clipboard owner retirement exceeded its cutoff".to_owned());
            }
            let Some(owner_thread) = self.serving_thread.as_ref() else {
                return Ok(Ok(()));
            };
            if owner_thread.is_finished() {
                let result = self.completed.try_recv().unwrap_or_else(|error| {
                    Err(match error {
                        TryRecvError::Empty | TryRecvError::Disconnected => {
                            "X11 clipboard owner worker ended without a result".to_owned()
                        }
                    })
                });
                self.join_completed(worker)?;
                return Ok(result);
            }
            std::thread::sleep(
                cutoff
                    .saturating_duration_since(Instant::now())
                    .min(X11_CANCEL_POLL),
            );
        }
    }

    fn join_completed(&mut self, _worker: &WorkerCtx) -> Result<(), String> {
        if self
            .serving_thread
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let thread = self
                .serving_thread
                .take()
                .expect("finished owner thread exists");
            thread
                .join()
                .map_err(|_| "X11 clipboard owner worker panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for X11OwnerCandidate {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn queue_retire_until_space(
    _worker: &WorkerCtx,
    mut command: OwnerCommand,
    cutoff: Instant,
    cancelled: Option<&AtomicBool>,
    lifecycle_cancelled: &AtomicBool,
    mut try_send: impl FnMut(OwnerCommand) -> Result<(), TrySendError<OwnerCommand>>,
) -> Result<(), String> {
    loop {
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire)) {
            return Err("X11 clipboard owner retirement was canceled by its caller".to_owned());
        }
        let remaining = cutoff.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            lifecycle_cancelled.store(true, Ordering::Release);
            return Err("X11 clipboard owner retirement exceeded its cutoff".to_owned());
        }
        match try_send(command) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => {
                return Err("X11 clipboard owner worker disconnected during retirement".to_owned());
            }
            Err(TrySendError::Full(returned)) => {
                command = returned;
                std::thread::sleep(remaining.min(X11_CANCEL_POLL));
            }
        }
    }
}

fn wait_for_response<T>(
    _worker: &WorkerCtx,
    response: Receiver<T>,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Option<T> {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return None;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match response.recv_timeout(remaining.min(X11_CANCEL_POLL)) {
            Ok(value) => return Some(value),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

fn serve_candidate(
    worker: &WorkerCtx,
    text: String,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    lifecycle_cancelled: Arc<AtomicBool>,
    commands: Receiver<OwnerCommand>,
    claim_result: mpsc::Sender<ClaimOutcome>,
) -> Result<(), String> {
    let control = Arc::new(X11TransportControl::owner_operation(
        deadline,
        Arc::clone(&cancelled),
        Arc::clone(&lifecycle_cancelled),
    ));
    let (connection, screen) = match connect_display_controlled(worker, Arc::clone(&control)) {
        Ok(connection) => connection,
        Err(error) => {
            let _ = claim_result.send(ClaimOutcome::FailedBeforeClaim(error.clone()));
            return Err(error);
        }
    };
    let mut owner = match ArboardX11OwnerCandidate::new(connection, screen, text) {
        Ok(owner) => owner,
        Err(error) => {
            let error = format!("X11 clipboard owner setup failed: {error}");
            let _ = claim_result.send(ClaimOutcome::FailedBeforeClaim(error.clone()));
            return Err(error);
        }
    };
    let candidate_window = owner.window();

    let claim_error = owner
        .claim()
        .err()
        .map(|error| format!("X11 clipboard owner claim request failed: {error}"));
    let claim_outcome = match owner.selection_owner() {
        Ok(owner_window) if owner_window == candidate_window => ClaimOutcome::Confirmed,
        Ok(owner_window) => ClaimOutcome::ConclusiveNoLongerOwner(owner_window),
        Err(error) => ClaimOutcome::Unconfirmed(
            claim_error
                .unwrap_or_else(|| format!("X11 clipboard owner confirmation failed: {error}")),
        ),
    };
    control.serving(Arc::clone(&lifecycle_cancelled));
    let _ = claim_result.send(claim_outcome);

    loop {
        if lifecycle_cancelled.load(Ordering::Acquire) {
            break;
        }
        match commands.try_recv() {
            Ok(OwnerCommand::Reconcile {
                deadline,
                cancelled: operation_cancelled,
                response,
            }) => {
                control.begin_owner_operation(
                    deadline,
                    Arc::clone(&operation_cancelled),
                    Arc::clone(&lifecycle_cancelled),
                );
                let result = match owner.selection_owner() {
                    Ok(owner_window) if owner_window == candidate_window => {
                        ReconcileOutcome::CandidateOwns
                    }
                    Ok(owner_window) => ReconcileOutcome::OtherOwner(owner_window),
                    Err(error) => ReconcileOutcome::Unconfirmed(format!(
                        "X11 clipboard owner reconciliation failed: {error}"
                    )),
                };
                control.serving(Arc::clone(&lifecycle_cancelled));
                let _ = response.send(result);
            }
            Ok(OwnerCommand::Retire {
                cutoff,
                cancelled,
                response,
            }) => {
                let (finished, result) = retire_candidate(
                    worker,
                    &mut owner,
                    &control,
                    &lifecycle_cancelled,
                    &commands,
                    cutoff,
                    cancelled.as_ref(),
                );
                let _ = response.send(RetireOutcome { finished, result });
                if finished {
                    break;
                }
            }
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }

        match owner.poll_event() {
            Ok(Some(event)) => match owner.handle_event(event) {
                Ok(true) => break,
                Ok(false) => {}
                Err(_) if lifecycle_cancelled.load(Ordering::Acquire) => break,
                Err(error) => {
                    return Err(format!("X11 clipboard owner request failed: {error}"));
                }
            },
            Ok(None) => {}
            Err(_) if lifecycle_cancelled.load(Ordering::Acquire) => break,
            Err(error) => return Err(format!("X11 clipboard owner event poll failed: {error}")),
        }
        match commands.recv_timeout(X11_CANCEL_POLL) {
            Ok(command) => match command {
                OwnerCommand::Reconcile {
                    deadline,
                    cancelled: operation_cancelled,
                    response,
                } => {
                    control.begin_owner_operation(
                        deadline,
                        Arc::clone(&operation_cancelled),
                        Arc::clone(&lifecycle_cancelled),
                    );
                    let result = match owner.selection_owner() {
                        Ok(owner_window) if owner_window == candidate_window => {
                            ReconcileOutcome::CandidateOwns
                        }
                        Ok(owner_window) => ReconcileOutcome::OtherOwner(owner_window),
                        Err(error) => ReconcileOutcome::Unconfirmed(format!(
                            "X11 clipboard owner reconciliation failed: {error}"
                        )),
                    };
                    control.serving(Arc::clone(&lifecycle_cancelled));
                    let _ = response.send(result);
                }
                OwnerCommand::Retire {
                    cutoff,
                    cancelled,
                    response,
                } => {
                    let (finished, result) = retire_candidate(
                        worker,
                        &mut owner,
                        &control,
                        &lifecycle_cancelled,
                        &commands,
                        cutoff,
                        cancelled.as_ref(),
                    );
                    let _ = response.send(RetireOutcome { finished, result });
                    if finished {
                        break;
                    }
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn retire_candidate<C: x11rb::connection::Connection>(
    _worker: &WorkerCtx,
    owner: &mut ArboardX11OwnerCandidate<C>,
    control: &X11TransportControl<'_>,
    lifecycle_cancelled: &Arc<AtomicBool>,
    commands: &Receiver<OwnerCommand>,
    cutoff: Instant,
    operation_cancelled: Option<&Arc<AtomicBool>>,
) -> (bool, Result<(), String>) {
    let cancelled_by_caller =
        || operation_cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Acquire));
    if cancelled_by_caller() {
        control.serving(Arc::clone(lifecycle_cancelled));
        return (
            false,
            Err("X11 clipboard owner retirement was canceled by its caller".to_owned()),
        );
    }
    let handover_cutoff = (Instant::now() + owner.manager_handover_budget()).min(
        cutoff
            .checked_sub(X11_CANCEL_POLL)
            .unwrap_or_else(Instant::now),
    );
    control.retiring(
        handover_cutoff,
        operation_cancelled.cloned(),
        Arc::clone(lifecycle_cancelled),
    );
    let handover = owner.begin_manager_handover();
    if handover.as_ref().is_ok_and(|started| *started) {
        loop {
            if cancelled_by_caller() {
                control.serving(Arc::clone(lifecycle_cancelled));
                return (
                    false,
                    Err("X11 clipboard owner retirement was canceled by its caller".to_owned()),
                );
            }
            if Instant::now() >= handover_cutoff || lifecycle_cancelled.load(Ordering::Acquire) {
                break;
            }
            match owner.poll_event() {
                Ok(Some(event)) => match owner.handle_event(event) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(error) if cancelled_by_caller() => {
                        control.serving(Arc::clone(lifecycle_cancelled));
                        return (
                            false,
                            Err(format!(
                                "X11 clipboard owner retirement was canceled by its caller: {error}"
                            )),
                        );
                    }
                    Err(error) => {
                        control.serving(Arc::clone(lifecycle_cancelled));
                        return (
                            false,
                            Err(format!("X11 clipboard handover failed: {error}")),
                        );
                    }
                },
                Ok(None) => {}
                Err(error) => {
                    if cancelled_by_caller() {
                        control.serving(Arc::clone(lifecycle_cancelled));
                        return (
                            false,
                            Err(format!(
                                "X11 clipboard owner retirement was canceled by its caller: {error}"
                            )),
                        );
                    }
                    break;
                }
            }
            match commands.recv_timeout(
                handover_cutoff
                    .saturating_duration_since(Instant::now())
                    .min(X11_CANCEL_POLL),
            ) {
                Ok(OwnerCommand::Reconcile { response, .. }) => {
                    let _ = response.send(ReconcileOutcome::Unconfirmed(
                        "X11 clipboard owner is retiring".to_owned(),
                    ));
                }
                Ok(OwnerCommand::Retire { response, .. }) => {
                    let _ = response.send(RetireOutcome {
                        finished: false,
                        result: Ok(()),
                    });
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    if cancelled_by_caller() {
        control.serving(Arc::clone(lifecycle_cancelled));
        return (
            false,
            Err("X11 clipboard owner retirement was canceled by its caller".to_owned()),
        );
    }
    control.retiring(
        cutoff,
        operation_cancelled.cloned(),
        Arc::clone(lifecycle_cancelled),
    );
    let result = owner
        .destroy_window()
        .map_err(|error| format!("X11 clipboard owner window destruction failed: {error}"));
    if result.is_err() && cancelled_by_caller() {
        lifecycle_cancelled.store(true, Ordering::Release);
        return (
            true,
            Err("X11 clipboard owner retirement was canceled during destruction".to_owned()),
        );
    }
    lifecycle_cancelled.store(true, Ordering::Release);
    (true, result)
}

#[cfg(test)]
mod tests {
    use super::{OwnerCommand, RetireOutcome, queue_retire_until_space};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, TrySendError};
    use std::time::{Duration, Instant};

    #[test]
    fn desktop_retire_retries_a_full_mailbox_after_a_canceled_retire_and_joins() {
        let cutoff = Instant::now() + Duration::from_secs(3);
        let (commands, command_rx) = mpsc::sync_channel(1);
        let canceled_retire = Arc::new(AtomicBool::new(true));
        let (canceled_response, canceled_response_rx) = mpsc::sync_channel(1);
        commands
            .try_send(OwnerCommand::Retire {
                cutoff,
                cancelled: Some(Arc::clone(&canceled_retire)),
                response: canceled_response,
            })
            .expect("the earlier canceled retire occupies the one-slot mailbox");
        // This is the abandoned admission's response receiver.
        drop(canceled_response_rx);

        let (mailbox_full_tx, mailbox_full_rx) = mpsc::channel();
        let (worker_joined_tx, worker_joined_rx) = mpsc::channel();
        let serving_thread = std::thread::spawn(move || {
            mailbox_full_rx
                .recv()
                .expect("the desktop retry must observe the full mailbox");
            let mut completed_canceled_retire = false;
            if let Ok(OwnerCommand::Retire {
                cancelled: Some(cancelled),
                response,
                ..
            }) = command_rx.recv()
            {
                assert!(cancelled.load(Ordering::Acquire));
                let _ = response.send(RetireOutcome {
                    finished: false,
                    result: Err("the earlier copy was canceled".to_owned()),
                });
                completed_canceled_retire = true;
            }

            let mut completed_desktop_retire = false;
            if let Ok(OwnerCommand::Retire {
                cancelled: None,
                response,
                ..
            }) = command_rx.recv()
            {
                completed_desktop_retire = response
                    .send(RetireOutcome {
                        finished: true,
                        result: Ok(()),
                    })
                    .is_ok();
            }
            let _ = worker_joined_tx.send(completed_canceled_retire && completed_desktop_retire);
        });

        let (desktop_response, desktop_response_rx) = mpsc::sync_channel(1);
        let lifecycle_cancelled = AtomicBool::new(false);
        let sender = commands.clone();
        let queue_worker = crate::spawn_at_priority(
            "x11-retire-retry-test",
            crate::ThreadPriority::Normal,
            move |worker| {
                let mut reported_full = false;
                queue_retire_until_space(
                    worker,
                    OwnerCommand::Retire {
                        cutoff,
                        cancelled: None,
                        response: desktop_response,
                    },
                    cutoff,
                    None,
                    &lifecycle_cancelled,
                    |command| {
                        let result = sender.try_send(command);
                        if matches!(result, Err(TrySendError::Full(_))) && !reported_full {
                            reported_full = true;
                            mailbox_full_tx
                                .send(())
                                .expect("the serving worker is waiting for the full signal");
                        }
                        result
                    },
                )
            },
        )
        .expect("spawn the desktop retirement worker");
        let queued = queue_worker
            .join()
            .expect("the desktop retirement worker exits");
        drop(commands);

        let worker_completed_both = worker_joined_rx.recv().unwrap_or(false);
        let serving_joined = serving_thread.join().is_ok();
        let desktop_outcome = desktop_response_rx.try_recv().ok();

        assert!(
            queued.is_ok(),
            "a full mailbox must be retried under the desktop cutoff: {queued:?}"
        );
        assert!(
            worker_completed_both,
            "both retirement commands were served"
        );
        assert!(serving_joined, "the serving worker was joined");
        assert!(desktop_outcome.is_some_and(|outcome| outcome.finished && outcome.result.is_ok()));
    }

    #[test]
    fn disconnected_retirement_mailbox_is_reported_separately_from_full() {
        let cutoff = Instant::now() + Duration::from_secs(3);
        let (commands, command_rx) = mpsc::sync_channel(1);
        drop(command_rx);
        let lifecycle_cancelled = Arc::new(AtomicBool::new(false));
        let worker_lifecycle = Arc::clone(&lifecycle_cancelled);
        let (response, _response_rx) = mpsc::sync_channel(1);
        let result = crate::spawn_at_priority(
            "x11-retire-disconnected-test",
            crate::ThreadPriority::Normal,
            move |worker| {
                queue_retire_until_space(
                    worker,
                    OwnerCommand::Retire {
                        cutoff,
                        cancelled: None,
                        response,
                    },
                    cutoff,
                    None,
                    &worker_lifecycle,
                    |command| commands.try_send(command),
                )
            },
        )
        .expect("spawn the retirement worker")
        .join()
        .expect("the retirement worker exits");

        assert!(result.is_err_and(|error| error.contains("disconnected")));
        assert!(!lifecycle_cancelled.load(Ordering::Acquire));
    }
}
