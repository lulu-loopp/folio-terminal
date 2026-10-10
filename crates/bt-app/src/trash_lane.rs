//! Linux recycle transactions, executed off the event-loop thread.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use bt_platform::admission::WorkerCtx;

pub(crate) const LANE_FULL: &str = "the Linux trash lane is full";
pub(crate) const LANE_GONE: &str = "the Linux trash lane has stopped";

/// One submitted trash operation, unique for this lane's lifetime.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TrashId(u64);

/// The desktop's answer to one submitted operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrashCompletion {
    pub(crate) id: TrashId,
    pub(crate) outcome: std::result::Result<bool, String>,
}

struct Request {
    id: TrashId,
    path: PathBuf,
}

/// The application-owned request queue and its answers.
pub(crate) struct TrashLane {
    requests: Option<mpsc::SyncSender<Request>>,
    answers: mpsc::Receiver<TrashCompletion>,
    worker: Option<JoinHandle<()>>,
    next: u64,
    pending: VecDeque<TrashId>,
    worker_stopped: bool,
}

impl TrashLane {
    /// Start the one FIFO worker and return before it performs any request.
    pub(crate) fn spawn(wake: impl Fn() + Clone + Send + 'static) -> Result<Self> {
        Self::start(
            |_worker| {
                |worker: &WorkerCtx, path: &Path| bt_platform::recycle_on_worker(worker, path)
            },
            wake,
        )
    }

    fn start<M, E, W>(make_executor: M, wake: W) -> Result<Self>
    where
        M: FnOnce(&WorkerCtx) -> E + Send + 'static,
        E: FnMut(&WorkerCtx, &Path) -> std::result::Result<bool, String> + Send + 'static,
        W: Fn() + Clone + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::sync_channel::<Request>(crate::handoff_lane::CAPACITY);
        let (answer_tx, answer_rx) = mpsc::channel::<TrashCompletion>();
        let lane_wake = wake.clone();
        let worker = bt_platform::spawn_at_priority(
            "folio-linux-trash",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| run_trash_lane(ctx, request_rx, answer_tx, make_executor(ctx), lane_wake),
        )
        .context("spawn the Linux trash lane")?;
        Ok(Self {
            requests: Some(request_tx),
            answers: answer_rx,
            worker: Some(worker),
            next: 0,
            pending: VecDeque::new(),
            worker_stopped: false,
        })
    }

    /// Submit one path without waiting for the worker.
    pub(crate) fn submit(&mut self, path: PathBuf) -> std::result::Result<TrashId, String> {
        if self.worker_stopped {
            return Err(LANE_GONE.to_owned());
        }
        let Some(requests) = self.requests.as_ref() else {
            return Err(LANE_GONE.to_owned());
        };
        let id = TrashId(self.next);
        let request = Request { id, path };
        // Register before sending: the worker can fail immediately after it
        // accepts this request, before the sender returns from `try_send`.
        self.pending.push_back(id);
        match requests.try_send(request) {
            Ok(()) => {
                self.next += 1;
                Ok(id)
            }
            Err(mpsc::TrySendError::Full(_)) => {
                self.pending.pop_back();
                Err(LANE_FULL.to_owned())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.worker_stopped = true;
                self.pending.pop_back();
                Err(LANE_GONE.to_owned())
            }
        }
    }

    /// Drain answers already published by the worker; never wait for one.
    pub(crate) fn answers(&mut self) -> Vec<TrashCompletion> {
        let mut completions = Vec::new();
        loop {
            match self.answers.try_recv() {
                Ok(completion) => {
                    self.remove_pending(completion.id);
                    completions.push(completion);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker_stopped = true;
                    completions.extend(self.pending.drain(..).map(|id| TrashCompletion {
                        id,
                        outcome: Err(LANE_GONE.to_owned()),
                    }));
                    break;
                }
            }
        }
        completions
    }

    /// Accepted requests whose answers have not yet been drained by the App.
    #[must_use]
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Close admission and join the idle worker from an exit worker.
    pub(crate) fn shutdown(mut self, _worker: &WorkerCtx) -> Result<()> {
        drop(self.requests.take());
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("the Linux trash lane panicked during shutdown"))
    }

    fn remove_pending(&mut self, id: TrashId) {
        if let Some(index) = self.pending.iter().position(|pending| *pending == id) {
            self.pending.remove(index);
        }
    }
}

fn run_trash_lane(
    worker: &WorkerCtx,
    requests: mpsc::Receiver<Request>,
    answer_sender: mpsc::Sender<TrashCompletion>,
    mut execute: impl FnMut(&WorkerCtx, &Path) -> std::result::Result<bool, String>,
    wake: impl Fn(),
) {
    let _wake_on_exit = WakeOnExit(&wake);
    // Locals drop in reverse declaration order: close the answer channel
    // before the exit guard wakes its receiver, on both return and unwind.
    let answers = answer_sender;
    while let Ok(Request { id, path }) = requests.recv() {
        let outcome = execute(worker, &path);
        let _ = answers.send(TrashCompletion { id, outcome });
        wake();
    }
}

struct WakeOnExit<'a>(&'a dyn Fn());

impl Drop for WakeOnExit<'_> {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[cfg(test)]
mod tests {
    use super::{LANE_FULL, TrashCompletion, TrashLane};
    use bt_platform::admission::WorkerCtx;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    fn shutdown(lane: TrashLane) {
        shutdown_result(lane).expect("the trash worker stopped cleanly");
    }

    fn shutdown_result(lane: TrashLane) -> anyhow::Result<()> {
        let (sent, received) = mpsc::channel();
        bt_platform::spawn_at_priority(
            "linux-trash-retire-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = sent.send(lane.shutdown(worker));
            },
        )
        .expect("start the controlled lane-retirement worker")
        .join()
        .expect("lane retirement worker returns");
        received
            .recv()
            .expect("lane retirement returned its answer")
    }

    #[test]
    fn completions_keep_fifo_order_and_cancellation_and_error_distinct() {
        let (woke, wakes) = mpsc::channel();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut first = true;
        let mut lane = TrashLane::start(
            move |_worker| {
                move |_ctx: &WorkerCtx, path: &Path| {
                    if first {
                        first = false;
                        started
                            .send(())
                            .expect("test still waits for the first request");
                        release_rx.recv().expect("test releases the first request");
                    }
                    match path.file_name().and_then(|name| name.to_str()) {
                        Some("accepted") => Ok(true),
                        Some("cancelled") => Ok(false),
                        _ => Err("controlled refusal".to_owned()),
                    }
                }
            },
            move || {
                let _ = woke.send(());
            },
        )
        .expect("start the controlled trash lane");

        let accepted = lane
            .submit(PathBuf::from("accepted"))
            .expect("admit the first transaction");
        started_rx.recv().expect("the first request is running");
        let cancelled = lane
            .submit(PathBuf::from("cancelled"))
            .expect("admit the cancellation result");
        let refused = lane
            .submit(PathBuf::from("refused"))
            .expect("admit the error result");
        assert_eq!(lane.pending_count(), 3);
        release.send(()).expect("let the worker finish its FIFO");
        for _ in 0..3 {
            wakes.recv().expect("each published answer wakes the owner");
        }

        assert_eq!(
            lane.answers(),
            [
                TrashCompletion {
                    id: accepted,
                    outcome: Ok(true),
                },
                TrashCompletion {
                    id: cancelled,
                    outcome: Ok(false),
                },
                TrashCompletion {
                    id: refused,
                    outcome: Err("controlled refusal".to_owned()),
                },
            ]
        );
        assert_eq!(lane.pending_count(), 0);
        shutdown(lane);
    }

    #[test]
    fn queue_uses_the_handoff_capacity_and_shutdown_joins_after_drain() {
        let (woke, wakes) = mpsc::channel();
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let mut first = true;
        let mut lane = TrashLane::start(
            move |_worker| {
                move |_ctx: &WorkerCtx, _path: &Path| {
                    if first {
                        first = false;
                        started
                            .send(())
                            .expect("test still waits for the first request");
                        release_rx.recv().expect("test releases the first request");
                    }
                    Ok(true)
                }
            },
            move || {
                let _ = woke.send(());
            },
        )
        .expect("start the controlled trash lane");

        lane.submit(PathBuf::from("running"))
            .expect("admit the running request");
        started_rx.recv().expect("the first request is running");
        for index in 0..crate::handoff_lane::CAPACITY {
            lane.submit(PathBuf::from(format!("queued-{index}")))
                .expect("the shared queue capacity remains available");
        }
        assert_eq!(
            lane.submit(PathBuf::from("full")),
            Err(LANE_FULL.to_owned())
        );
        assert_eq!(lane.pending_count(), crate::handoff_lane::CAPACITY + 1);

        release.send(()).expect("let the worker drain its FIFO");
        for _ in 0..crate::handoff_lane::CAPACITY + 1 {
            wakes
                .recv()
                .expect("each answer is published before its wake");
        }
        assert_eq!(lane.answers().len(), crate::handoff_lane::CAPACITY + 1);
        assert_eq!(lane.pending_count(), 0);
        shutdown(lane);
    }

    #[test]
    fn worker_exit_answers_every_accepted_request_once() {
        let (woke, wakes) = mpsc::channel();
        let mut lane = TrashLane::start(
            |_worker| {
                |_ctx: &WorkerCtx, _path: &Path| -> Result<bool, String> {
                    panic!("controlled worker failure")
                }
            },
            move || {
                let _ = woke.send(());
            },
        )
        .expect("start the controlled trash lane");
        let id = lane
            .submit(PathBuf::from("accepted"))
            .expect("admit the controlled request");
        wakes.recv().expect("worker exit wakes the owner");
        assert_eq!(
            lane.answers(),
            [TrashCompletion {
                id,
                outcome: Err(super::LANE_GONE.to_owned()),
            }]
        );
        assert_eq!(lane.pending_count(), 0);
        assert_eq!(
            lane.submit(PathBuf::from("after-stop")),
            Err(super::LANE_GONE.to_owned())
        );
        assert!(shutdown_result(lane).is_err());
    }

    #[test]
    fn request_ids_are_lane_local_and_never_reused_after_completion() {
        let (woke, wakes) = mpsc::channel();
        let mut lane = TrashLane::start(
            |_worker| |_ctx: &WorkerCtx, _path: &Path| Ok(true),
            move || {
                let _ = woke.send(());
            },
        )
        .expect("start the controlled trash lane");
        let first = lane
            .submit(PathBuf::from("first"))
            .expect("admit first request");
        wakes.recv().expect("first request is complete");
        assert_eq!(lane.answers()[0].id, first);
        let second = lane
            .submit(PathBuf::from("second"))
            .expect("admit second request");
        wakes.recv().expect("second request is complete");
        assert_eq!(lane.answers()[0].id, second);
        assert_ne!(first, second);
        shutdown(lane);
    }
}
