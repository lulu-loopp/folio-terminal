//! One process-wide, ordered owner for native Linux clipboard operations.
//!
//! Read and write requests share admission order, one worker, and one held
//! result. Text is moved into a write request at admission; only the window
//! adopts results or decides whether their visible effect is still current.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use bt_platform::{ClipboardPayload, LinuxClipboardBackend, ThreadPriority, admission::WorkerCtx};

pub(crate) const MAX_WAITING_REQUESTS: usize = 8;
pub(crate) const OPERATION_DEADLINE: Duration = bt_platform::LINUX_CLIPBOARD_OPERATION_BUDGET;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadKind {
    Text,
    Payload,
}

#[derive(Debug)]
pub(crate) enum ReadValue {
    Text(Result<String, String>),
    Payload(Result<ClipboardPayload, String>),
}

pub(crate) struct ReadResult<T> {
    pub(crate) id: u64,
    pub(crate) target: T,
    pub(crate) kind: ReadKind,
    pub(crate) value: ReadValue,
}

pub(crate) struct WriteResult<E> {
    pub(crate) id: u64,
    pub(crate) action: &'static str,
    pub(crate) effect: E,
    pub(crate) result: Result<(), String>,
}

pub(crate) enum ClipboardLaneResult<T, E> {
    Read(ReadResult<T>),
    Write(WriteResult<E>),
}

impl<T, E> ClipboardLaneResult<T, E> {
    pub(crate) fn id(&self) -> u64 {
        match self {
            Self::Read(result) => result.id,
            Self::Write(result) => result.id,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum RequestRefusal {
    QueueFull,
    Closed,
    WorkerStart(String),
}

struct ReadRequest<T> {
    id: u64,
    target: T,
    kind: ReadKind,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

struct WriteRequest<E> {
    id: u64,
    text: String,
    action: &'static str,
    effect: E,
    backend: LinuxClipboardBackend,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}

enum Request<T, E> {
    Read(ReadRequest<T>),
    Write(WriteRequest<E>),
}

impl<T, E> Request<T, E> {
    fn id(&self) -> u64 {
        match self {
            Self::Read(request) => request.id,
            Self::Write(request) => request.id,
        }
    }

    fn cancelled(&self) -> &Arc<AtomicBool> {
        match self {
            Self::Read(request) => &request.cancelled,
            Self::Write(request) => &request.cancelled,
        }
    }

    fn read_target(&self) -> Option<&T> {
        match self {
            Self::Read(request) => Some(&request.target),
            Self::Write(_) => None,
        }
    }
}

struct ActiveRequest {
    id: u64,
    cancelled: Arc<AtomicBool>,
}

struct State<T, E> {
    queue: VecDeque<Request<T, E>>,
    active: Option<ActiveRequest>,
    active_target: Option<T>,
    result: Option<ClipboardLaneResult<T, E>>,
    result_delivery_pending: Option<u64>,
    shutting_down: bool,
    worker_stopped: bool,
    next_id: u64,
}

struct Shared<T, E> {
    state: Mutex<State<T, E>>,
    changed: Condvar,
}

type Reader = dyn Fn(&WorkerCtx, LinuxClipboardBackend, ReadKind, Instant, Arc<AtomicBool>) -> ReadValue
    + Send
    + Sync;
type Writer = dyn Fn(&WorkerCtx, LinuxClipboardBackend, String, Instant, Arc<AtomicBool>) -> Result<(), String>
    + Send
    + Sync;
type Wake = dyn Fn() + Send + Sync;

/// The process's one clipboard operation lane. `T` is the app-owned read
/// destination and `E` is window-owned feedback for a completed write.
pub(crate) struct ClipboardLane<T: Clone + Send + 'static, E: Send + 'static> {
    shared: Arc<Shared<T, E>>,
    wake: Arc<Wake>,
    reader: Arc<Reader>,
    writer: Arc<Writer>,
    worker: Option<JoinHandle<()>>,
}

impl<T, E> ClipboardLane<T, E>
where
    T: Clone + Send + 'static,
    E: Send + 'static,
{
    pub(crate) fn new(
        wake: impl Fn() + Send + Sync + 'static,
        reader: impl Fn(
            &WorkerCtx,
            LinuxClipboardBackend,
            ReadKind,
            Instant,
            Arc<AtomicBool>,
        ) -> ReadValue
        + Send
        + Sync
        + 'static,
        writer: impl Fn(
            &WorkerCtx,
            LinuxClipboardBackend,
            String,
            Instant,
            Arc<AtomicBool>,
        ) -> Result<(), String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    queue: VecDeque::new(),
                    active: None,
                    active_target: None,
                    result: None,
                    result_delivery_pending: None,
                    shutting_down: false,
                    worker_stopped: false,
                    next_id: 1,
                }),
                changed: Condvar::new(),
            }),
            wake: Arc::new(wake),
            reader: Arc::new(reader),
            writer: Arc::new(writer),
            worker: None,
        }
    }

    pub(crate) fn linux(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::new(
            wake,
            |worker, backend, kind, deadline, cancelled| match kind {
                ReadKind::Text => ReadValue::Text(bt_platform::clipboard_text_on_worker(
                    worker, backend, deadline, cancelled,
                )),
                ReadKind::Payload => ReadValue::Payload(bt_platform::clipboard_payload_on_worker(
                    worker, backend, deadline, cancelled,
                )),
            },
            |worker, backend, text, deadline, cancelled| {
                bt_platform::set_clipboard_text_on_worker(
                    worker, backend, text, deadline, cancelled,
                )
            },
        )
    }

    pub(crate) fn request(
        &mut self,
        target: T,
        kind: ReadKind,
        backend: LinuxClipboardBackend,
    ) -> Result<u64, RequestRefusal> {
        self.request_with_deadline(target, kind, backend, Instant::now() + OPERATION_DEADLINE)
    }

    fn request_with_deadline(
        &mut self,
        target: T,
        kind: ReadKind,
        backend: LinuxClipboardBackend,
        deadline: Instant,
    ) -> Result<u64, RequestRefusal> {
        let shared = Arc::clone(&self.shared);
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.admit(&mut state)?;
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.queue.push_back(Request::Read(ReadRequest {
            id,
            target,
            kind,
            backend,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }));
        shared.changed.notify_one();
        Ok(id)
    }

    pub(crate) fn request_write(
        &mut self,
        text: String,
        action: &'static str,
        effect: E,
        backend: LinuxClipboardBackend,
    ) -> Result<u64, RequestRefusal> {
        self.request_write_with_deadline(
            text,
            action,
            effect,
            backend,
            Instant::now() + OPERATION_DEADLINE,
        )
    }

    fn request_write_with_deadline(
        &mut self,
        text: String,
        action: &'static str,
        effect: E,
        backend: LinuxClipboardBackend,
        deadline: Instant,
    ) -> Result<u64, RequestRefusal> {
        let shared = Arc::clone(&self.shared);
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.admit(&mut state)?;
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.queue.push_back(Request::Write(WriteRequest {
            id,
            text,
            action,
            effect,
            backend,
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        }));
        shared.changed.notify_one();
        Ok(id)
    }

    fn admit(&mut self, state: &mut State<T, E>) -> Result<(), RequestRefusal> {
        if state.shutting_down || state.worker_stopped {
            return Err(RequestRefusal::Closed);
        }
        if state.queue.len() >= MAX_WAITING_REQUESTS {
            return Err(RequestRefusal::QueueFull);
        }
        if self.worker.is_none() {
            let shared = Arc::clone(&self.shared);
            let wake = Arc::clone(&self.wake);
            let reader = Arc::clone(&self.reader);
            let writer = Arc::clone(&self.writer);
            self.worker = Some(
                bt_platform::spawn_at_priority(
                    "bt-linux-clipboard-lane",
                    ThreadPriority::Normal,
                    move |worker| run_clipboard_worker(worker, shared, wake, reader, writer),
                )
                .map_err(|error| RequestRefusal::WorkerStart(error.to_string()))?,
            );
        }
        Ok(())
    }

    /// Take the held answer and let the worker begin its next acquisition.
    pub(crate) fn take_result(&self) -> Option<ClipboardLaneResult<T, E>> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let result = state.result.take();
        if let Some(result) = &result {
            state.result_delivery_pending = Some(result.id());
        }
        result
    }

    /// Release worker backpressure after the window has adopted or discarded
    /// the result it took from the held slot.
    pub(crate) fn acknowledge_result(&self, id: u64) -> bool {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.result_delivery_pending == Some(id) {
            state.result_delivery_pending = None;
            self.shared.changed.notify_one();
            true
        } else {
            false
        }
    }

    /// The bounded set of destinations currently owned by this lane.
    pub(crate) fn pending_targets(&self) -> Vec<(u64, T)> {
        let state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut targets = Vec::with_capacity(state.queue.len() + 2);
        targets.extend(state.queue.iter().filter_map(|request| {
            request
                .read_target()
                .map(|target| (request.id(), target.clone()))
        }));
        if let Some(active) = &state.active {
            // Active target is retained in the queue entry only until it is
            // popped, so the worker records it alongside the active id below.
            if let Some(target) = state.active_target.as_ref() {
                targets.push((active.id, target.clone()));
            }
        }
        if let Some(ClipboardLaneResult::Read(result)) = &state.result {
            targets.push((result.id, result.target.clone()));
        }
        targets
    }

    /// Cancel stale destinations without waiting for their transfer to unwind.
    /// A held result is discarded immediately so it cannot block later reads.
    pub(crate) fn cancel_requests(&self, ids: &[u64]) {
        if ids.is_empty() {
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for request in &state.queue {
            if let Request::Read(request) = request
                && ids.contains(&request.id)
            {
                request.cancelled.store(true, Ordering::Release);
            }
        }
        if let Some(active) = &state.active
            && ids.contains(&active.id)
        {
            active.cancelled.store(true, Ordering::Release);
        }
        if matches!(
            state.result.as_ref(),
            Some(ClipboardLaneResult::Read(result)) if ids.contains(&result.id)
        ) {
            state.result = None;
        }
        self.shared.changed.notify_all();
    }

    /// Stop admission, cancel in-flight work, and join the worker from an
    /// application-retirement worker.
    ///
    /// TEMPORARY (2026-10-06, PR4 of the port split): the caller of this —
    /// `retire_linux_desktop`'s bounded retirement worker — arrives with the
    /// Linux trash and shutdown PR (PR5 of the port split). Until then the
    /// retirement has no caller, so the dead-code lint is silenced rather than
    /// the shutdown deferred; the lane's own tests exercise it in the meantime.
    #[allow(dead_code)]
    pub(crate) fn shutdown(mut self, _worker: &WorkerCtx, cutoff: Instant) -> Result<(), String> {
        self.signal_shutdown();
        while self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            if Instant::now() >= cutoff {
                return Err(
                    "Linux clipboard lane exceeded the desktop retirement cutoff; continuing shutdown"
                        .to_owned(),
                );
            }
            std::thread::sleep(crate::persist::SESSION_JOIN_POLL);
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "Linux clipboard lane worker panicked".to_owned())?;
        }
        Ok(())
    }

    fn signal_shutdown(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.shutting_down = true;
        for request in &state.queue {
            request.cancelled().store(true, Ordering::Release);
        }
        if let Some(active) = &state.active {
            active.cancelled.store(true, Ordering::Release);
        }
        state.queue.clear();
        state.result = None;
        state.result_delivery_pending = None;
        self.shared.changed.notify_all();
    }
}

impl<T: Clone + Send + 'static, E: Send + 'static> Drop for ClipboardLane<T, E> {
    fn drop(&mut self) {
        self.signal_shutdown();
    }
}

fn run_clipboard_worker<T, E>(
    worker: &WorkerCtx,
    shared: Arc<Shared<T, E>>,
    wake: Arc<Wake>,
    reader: Arc<Reader>,
    writer: Arc<Writer>,
) where
    T: Clone + Send + 'static,
    E: Send + 'static,
{
    loop {
        let request = {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while !state.shutting_down
                && (state.queue.is_empty()
                    || state.result.is_some()
                    || state.result_delivery_pending.is_some())
            {
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if state.shutting_down {
                state.worker_stopped = true;
                shared.changed.notify_all();
                return;
            }
            let request = state.queue.pop_front().expect("queue checked above");
            state.active = Some(ActiveRequest {
                id: request.id(),
                cancelled: Arc::clone(request.cancelled()),
            });
            state.active_target = request.read_target().cloned();
            request
        };

        let result = match request {
            Request::Read(request) => {
                let mut value = if request.cancelled.load(Ordering::Acquire) {
                    cancelled_value(request.kind)
                } else if Instant::now() >= request.deadline {
                    timed_out_value(request.kind)
                } else {
                    reader(
                        worker,
                        request.backend,
                        request.kind,
                        request.deadline,
                        Arc::clone(&request.cancelled),
                    )
                };
                if request.cancelled.load(Ordering::Acquire) {
                    value = cancelled_value(request.kind);
                } else if Instant::now() >= request.deadline {
                    value = timed_out_value(request.kind);
                }
                ClipboardLaneResult::Read(ReadResult {
                    id: request.id,
                    target: request.target,
                    kind: request.kind,
                    value,
                })
            }
            Request::Write(request) => {
                let mut result = if request.cancelled.load(Ordering::Acquire) {
                    Err("clipboard write canceled".to_owned())
                } else if Instant::now() >= request.deadline {
                    Err("clipboard write exceeded its four-second deadline".to_owned())
                } else {
                    writer(
                        worker,
                        request.backend,
                        request.text,
                        request.deadline,
                        Arc::clone(&request.cancelled),
                    )
                };
                if request.cancelled.load(Ordering::Acquire) {
                    result = Err("clipboard write canceled".to_owned());
                } else if Instant::now() >= request.deadline {
                    result = Err("clipboard write exceeded its four-second deadline".to_owned());
                }
                ClipboardLaneResult::Write(WriteResult {
                    id: request.id,
                    action: request.action,
                    effect: request.effect,
                    result,
                })
            }
        };

        let published = {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.active = None;
            state.active_target = None;
            if state.shutting_down {
                false
            } else {
                state.result = Some(result);
                true
            }
        };
        if published {
            wake();
        }
    }
}

fn cancelled_value(kind: ReadKind) -> ReadValue {
    let error = "clipboard read canceled".to_owned();
    match kind {
        ReadKind::Text => ReadValue::Text(Err(error)),
        ReadKind::Payload => ReadValue::Payload(Err(error)),
    }
}

fn timed_out_value(kind: ReadKind) -> ReadValue {
    let error = "clipboard read exceeded its four-second deadline".to_owned();
    match kind {
        ReadKind::Text => ReadValue::Text(Err(error)),
        ReadKind::Payload => ReadValue::Payload(Err(error)),
    }
}

#[cfg(test)]
impl<T, E> ClipboardLane<T, E>
where
    T: Clone + Send + 'static,
    E: Send + 'static,
{
    fn test_active_cancel(&self) -> Option<Arc<AtomicBool>> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .as_ref()
            .map(|active| Arc::clone(&active.cancelled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Barrier, mpsc},
        thread,
    };

    fn owner(
        wake: impl Fn() + Send + Sync + 'static,
        reader: impl Fn(
            &WorkerCtx,
            LinuxClipboardBackend,
            ReadKind,
            Instant,
            Arc<AtomicBool>,
        ) -> ReadValue
        + Send
        + Sync
        + 'static,
    ) -> ClipboardLane<u64, ()> {
        ClipboardLane::new(wake, reader, |_, _, _, _, _| Ok(()))
    }

    fn backend() -> LinuxClipboardBackend {
        LinuxClipboardBackend::X11
    }

    fn payload(kind: ReadKind, text: &str) -> ReadValue {
        match kind {
            ReadKind::Text => ReadValue::Text(Ok(text.to_owned())),
            ReadKind::Payload => ReadValue::Payload(Ok(ClipboardPayload::Text(text.to_owned()))),
        }
    }

    fn take_read<E: Send + 'static>(lane: &ClipboardLane<u64, E>) -> ReadResult<u64> {
        let result = lane.take_result().expect("clipboard read result is held");
        assert!(lane.acknowledge_result(result.id()));
        match result {
            ClipboardLaneResult::Read(result) => result,
            ClipboardLaneResult::Write(_) => panic!("expected a read result"),
        }
    }

    #[test]
    fn a_mixed_lane_publishes_read_copy_read_in_admission_order() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let release_first = Arc::new(Barrier::new(2));
        let reader_release = Arc::clone(&release_first);
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reader_reads = Arc::clone(&reads);
        let read_started = started_tx.clone();
        let contents = Arc::new(Mutex::new("before copy".to_owned()));
        let read_contents = Arc::clone(&contents);
        let write_contents = Arc::clone(&contents);
        let mut lane = ClipboardLane::new(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, _| {
                let count = reader_reads.fetch_add(1, Ordering::AcqRel) + 1;
                read_started.send(format!("read-{count}")).unwrap();
                if count == 1 {
                    reader_release.wait();
                }
                let text = read_contents.lock().unwrap().clone();
                payload(kind, &text)
            },
            move |_, _, text, _, _| {
                started_tx.send(format!("write-{text}")).unwrap();
                *write_contents.lock().unwrap() = text;
                Ok(())
            },
        );

        let read_before = lane.request(11, ReadKind::Text, backend()).unwrap();
        assert_eq!(started_rx.recv().unwrap(), "read-1");
        let copy = lane
            .request_write("admitted snapshot".to_owned(), "copy", (), backend())
            .unwrap();
        let read_after = lane.request(22, ReadKind::Text, backend()).unwrap();

        release_first.wait();
        wake_rx.recv().unwrap();
        let first = lane.take_result().expect("the first read publishes first");
        assert!(!lane.acknowledge_result(first.id().wrapping_add(1)));
        assert!(
            started_rx.try_recv().is_err(),
            "a mismatched acknowledgement cannot advance the lane"
        );
        assert!(lane.acknowledge_result(first.id()));
        assert!(!lane.acknowledge_result(first.id()));
        assert!(matches!(
            first,
            ClipboardLaneResult::Read(ReadResult { id, target: 11, .. }) if id == read_before
        ));

        assert_eq!(started_rx.recv().unwrap(), "write-admitted snapshot");
        wake_rx.recv().unwrap();
        let second = lane.take_result().expect("the copy publishes second");
        assert!(lane.acknowledge_result(second.id()));
        assert!(matches!(
            second,
            ClipboardLaneResult::Write(WriteResult { id, result: Ok(()), .. }) if id == copy
        ));

        assert_eq!(started_rx.recv().unwrap(), "read-2");
        wake_rx.recv().unwrap();
        let third = lane
            .take_result()
            .expect("the later paste read publishes third");
        assert!(lane.acknowledge_result(third.id()));
        assert!(matches!(
            third,
            ClipboardLaneResult::Read(ReadResult {
                id,
                target: 22,
                value: ReadValue::Text(Ok(text)),
                ..
            }) if id == read_after && text == "admitted snapshot"
        ));

        retire(lane).recv().unwrap().unwrap();
    }

    fn retire<T: Clone + Send + 'static, E: Send + 'static>(
        owner: ClipboardLane<T, E>,
    ) -> mpsc::Receiver<Result<(), String>> {
        let (done_tx, done_rx) = mpsc::channel();
        let _worker = bt_platform::spawn_at_priority(
            "clipboard-reader-test-retire",
            ThreadPriority::BelowNormal,
            move |worker| {
                let _ = done_tx.send(
                    owner.shutdown(worker, Instant::now() + crate::persist::SESSION_SAVE_BUDGET),
                );
            },
        )
        .expect("controlled retirement worker starts");
        done_rx
    }

    #[test]
    fn queue_is_fifo_and_a_held_result_backpressures_the_reader() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let first_read = Arc::new(Barrier::new(2));
        let release_first = Arc::clone(&first_read);
        let reads = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let reader_reads = Arc::clone(&reads);
        let mut owner = owner(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, _| {
                let read = reader_reads.fetch_add(1, Ordering::AcqRel) + 1;
                started_tx.send(read).unwrap();
                if read == 1 {
                    release_first.wait();
                }
                payload(kind, &read.to_string())
            },
        );
        let first = owner.request(11, ReadKind::Text, backend()).unwrap();
        assert_eq!(started_rx.recv().unwrap(), 1);
        let second = owner.request(22, ReadKind::Text, backend()).unwrap();
        assert!(second > first);
        first_read.wait();
        wake_rx.recv().unwrap();
        assert!(
            started_rx.try_recv().is_err(),
            "second read waits for result consumption"
        );
        let ClipboardLaneResult::Read(held) = owner.take_result().expect("first answer is held")
        else {
            panic!("the first result is a read");
        };
        assert_eq!(held.id, first);
        assert_eq!(held.target, 11);
        assert!(
            started_rx.try_recv().is_err(),
            "the next backend operation waits for window adoption"
        );
        assert!(owner.acknowledge_result(held.id));
        assert_eq!(started_rx.recv().unwrap(), 2);
        wake_rx.recv().unwrap();
        let next = take_read(&owner);
        assert_eq!(next.id, second);
        assert_eq!(next.target, 22);
        retire(owner).recv().unwrap().unwrap();
    }

    #[test]
    fn queue_is_bounded_and_expired_waiters_do_not_open_the_clipboard() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let hold = Arc::new(Barrier::new(2));
        let hold_reader = Arc::clone(&hold);
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reader_reads = Arc::clone(&reads);
        let mut owner = owner(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, _| {
                let read = reader_reads.fetch_add(1, Ordering::AcqRel) + 1;
                started_tx.send(read).unwrap();
                if read == 1 {
                    hold_reader.wait();
                }
                payload(kind, "ok")
            },
        );
        let first = owner.request(0, ReadKind::Text, backend()).unwrap();
        assert_eq!(started_rx.recv().unwrap(), 1);
        for index in 0..MAX_WAITING_REQUESTS - 1 {
            owner
                .request(index as u64 + 1, ReadKind::Text, backend())
                .unwrap();
        }
        let expired = owner
            .request_with_deadline(
                100,
                ReadKind::Text,
                backend(),
                Instant::now() - Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            owner.request(99, ReadKind::Text, backend()),
            Err(RequestRefusal::QueueFull),
        );
        hold.wait();
        wake_rx.recv().unwrap();
        let current = take_read(&owner);
        assert_eq!(current.id, first);
        // Drain the seven queued reads ahead of a timed-out request. Each take
        // releases exactly one next read and the worker's wake is its barrier.
        for expected in 2..=MAX_WAITING_REQUESTS {
            assert_eq!(started_rx.recv().unwrap(), expected);
            wake_rx.recv().unwrap();
            let _ = take_read(&owner);
        }
        wake_rx.recv().unwrap();
        let timed_out = take_read(&owner);
        assert_eq!(timed_out.id, expired);
        assert!(matches!(timed_out.value, ReadValue::Text(Err(_))));
        assert!(started_rx.try_recv().is_err());
        retire(owner).recv().unwrap().unwrap();
    }

    #[test]
    fn mixed_waiters_keep_all_writes_and_publish_expired_and_failed_outcomes() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (read_started_tx, read_started_rx) = mpsc::channel();
        let (write_started_tx, write_started_rx) = mpsc::channel();
        let hold = Arc::new(Barrier::new(2));
        let hold_reader = Arc::clone(&hold);
        let mut lane = ClipboardLane::new(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, _| {
                read_started_tx.send(()).unwrap();
                hold_reader.wait();
                payload(kind, "initial")
            },
            move |_, _, text, _, _| {
                write_started_tx.send(text.clone()).unwrap();
                if text == "backend failure" {
                    Err("controlled writer refusal".to_owned())
                } else {
                    Ok(())
                }
            },
        );

        let first_read = lane.request(1, ReadKind::Text, backend()).unwrap();
        read_started_rx.recv().unwrap();
        let mut writes = Vec::new();
        for index in 0..MAX_WAITING_REQUESTS {
            let text = match index {
                2 => "backend failure".to_owned(),
                5 => "expired before service".to_owned(),
                _ => format!("copy-{index}"),
            };
            let id = if index == 5 {
                lane.request_write_with_deadline(
                    text.clone(),
                    "copy test",
                    (),
                    backend(),
                    Instant::now() - Duration::from_secs(1),
                )
                .unwrap()
            } else {
                lane.request_write(text.clone(), "copy test", (), backend())
                    .unwrap()
            };
            writes.push((id, text));
        }
        assert_eq!(
            lane.request(2, ReadKind::Text, backend()),
            Err(RequestRefusal::QueueFull)
        );

        hold.wait();
        wake_rx.recv().unwrap();
        assert_eq!(take_read(&lane).id, first_read);
        for (index, (expected_id, text)) in writes.iter().enumerate() {
            if index == 5 {
                wake_rx.recv().unwrap();
                let ClipboardLaneResult::Write(result) = lane.take_result().unwrap() else {
                    panic!("the expired waiter is a write result");
                };
                assert_eq!(result.id, *expected_id);
                assert!(result.result.unwrap_err().contains("deadline"));
                assert!(lane.acknowledge_result(result.id));
                continue;
            }
            assert_eq!(write_started_rx.recv().unwrap(), *text);
            wake_rx.recv().unwrap();
            let ClipboardLaneResult::Write(result) = lane.take_result().unwrap() else {
                panic!("the waiter is a write result");
            };
            assert_eq!(result.id, *expected_id);
            if index == 2 {
                assert_eq!(result.result, Err("controlled writer refusal".to_owned()));
            } else {
                assert_eq!(result.result, Ok(()));
            }
            assert!(lane.acknowledge_result(result.id));
        }
        assert!(write_started_rx.try_recv().is_err());
        retire(lane).recv().unwrap().unwrap();
    }

    #[test]
    fn a_canceled_queued_target_gets_a_terminal_answer_without_a_read() {
        let (wake_tx, wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let hold = Arc::new(Barrier::new(2));
        let hold_reader = Arc::clone(&hold);
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let reader_reads = Arc::clone(&reads);
        let mut owner = owner(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, _| {
                let read = reader_reads.fetch_add(1, Ordering::AcqRel) + 1;
                started_tx.send(read).unwrap();
                if read == 1 {
                    hold_reader.wait();
                }
                payload(kind, "ok")
            },
        );
        let first = owner.request(1, ReadKind::Text, backend()).unwrap();
        assert_eq!(started_rx.recv().unwrap(), 1);
        let canceled = owner.request(2, ReadKind::Text, backend()).unwrap();
        owner.cancel_requests(&[canceled]);
        hold.wait();
        wake_rx.recv().unwrap();
        assert_eq!(take_read(&owner).id, first);
        wake_rx.recv().unwrap();
        let answer = take_read(&owner);
        assert_eq!(answer.id, canceled);
        assert!(matches!(answer.value, ReadValue::Text(Err(_))));
        assert!(started_rx.try_recv().is_err());
        retire(owner).recv().unwrap().unwrap();
    }

    #[test]
    fn shutdown_cancels_and_joins_the_reader_before_returning() {
        let (wake_tx, _wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let release_reader = Arc::clone(&release);
        let observed_cancel = Arc::new(Mutex::new(None));
        let observed_reader = Arc::clone(&observed_cancel);
        let mut owner = owner(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, cancelled| {
                *observed_reader.lock().unwrap() = Some(cancelled.load(Ordering::Acquire));
                started_tx.send(()).unwrap();
                release_reader.wait();
                let answer = if cancelled.load(Ordering::Acquire) {
                    cancelled_value(kind)
                } else {
                    payload(kind, "done")
                };
                *observed_reader.lock().unwrap() = Some(cancelled.load(Ordering::Acquire));
                answer
            },
        );
        owner.request(1, ReadKind::Text, backend()).unwrap();
        started_rx.recv().unwrap();
        let cancelled = owner.test_active_cancel().unwrap();
        let retirer = retire(owner);
        while !cancelled.load(Ordering::Acquire) {
            thread::yield_now();
        }
        release.wait();
        retirer.recv().unwrap().unwrap();
        assert_eq!(*observed_cancel.lock().unwrap(), Some(true));
    }

    #[test]
    fn shutdown_cancels_before_an_expired_cutoff_and_reports_continue_shutdown() {
        let (wake_tx, _wake_rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let release_reader = Arc::clone(&release);
        let mut lane = owner(
            move || {
                let _ = wake_tx.send(());
            },
            move |_, _, kind, _, cancelled| {
                started_tx.send(()).unwrap();
                release_reader.wait();
                let stopped = cancelled.load(Ordering::Acquire);
                finished_tx.send(stopped).unwrap();
                if stopped {
                    cancelled_value(kind)
                } else {
                    payload(kind, "unexpected completion")
                }
            },
        );
        lane.request(1, ReadKind::Text, backend()).unwrap();
        started_rx.recv().unwrap();
        let active_cancel = lane.test_active_cancel().unwrap();

        let (retired_tx, retired_rx) = mpsc::channel();
        let retire_worker = bt_platform::spawn_at_priority(
            "clipboard-lane-expired-retire-test",
            ThreadPriority::BelowNormal,
            move |worker| {
                let result = lane.shutdown(worker, Instant::now());
                retired_tx.send(result).unwrap();
            },
        )
        .unwrap();
        let result = retired_rx.recv().unwrap();
        retire_worker.join().unwrap();
        assert!(result.unwrap_err().contains("continuing shutdown"));
        assert!(active_cancel.load(Ordering::Acquire));

        release.wait();
        assert!(finished_rx.recv().unwrap());
    }
}
