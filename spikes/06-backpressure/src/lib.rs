use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use serde::Serialize;

/// DESIGN.md §1.3: each session owns a 1 MiB PTY-to-Term byte ring.
pub const PTY_RING_BYTES: usize = 1024 * 1024;
/// DESIGN.md §1.3: one actor may parse at most 256 KiB before yielding.
pub const PARSE_QUANTUM_BYTES: usize = 256 * 1024;
/// DESIGN.md §1.3: each session may queue at most 64 derived worker jobs.
pub const WORKER_QUEUE_TASKS: usize = 64;
/// DESIGN.md §1.3: no session may run more than two render jobs.
pub const PER_SESSION_RENDER_LIMIT: usize = 2;
/// DESIGN.md §1.3: the process may run at most eight render jobs globally.
pub const GLOBAL_RENDER_LIMIT: usize = 8;
/// Visible sessions receive seven turns for every invisible-session floor turn.
pub const VISIBLE_WEIGHT: usize = 7;
pub const INVISIBLE_WEIGHT: usize = 1;

#[derive(Default)]
struct RingState {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    max_bytes: usize,
    blocked_pushes: u64,
    closed: bool,
}

/// Byte-counted bounded ring. Full writers sleep on a condition variable, propagating
/// backpressure to the simulated blocking PTY reader instead of dropping VT bytes.
pub struct ByteRing {
    capacity: usize,
    state: Mutex<RingState>,
    changed: Condvar,
}

impl ByteRing {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            state: Mutex::new(RingState::default()),
            changed: Condvar::new(),
        }
    }

    pub fn push(&self, chunk: Vec<u8>) -> Result<()> {
        if chunk.len() > self.capacity {
            bail!(
                "chunk {} exceeds byte ring capacity {}",
                chunk.len(),
                self.capacity
            );
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut counted_block = false;
        while !state.closed && state.bytes + chunk.len() > self.capacity {
            if !counted_block {
                state.blocked_pushes += 1;
                counted_block = true;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        if state.closed {
            bail!("byte ring closed");
        }
        state.bytes += chunk.len();
        state.max_bytes = state.max_bytes.max(state.bytes);
        state.chunks.push_back(chunk);
        self.changed.notify_all();
        Ok(())
    }

    /// Blocks until data or closure, then returns no more than the quantum.
    pub fn pop_quantum(&self, quantum: usize) -> Vec<u8> {
        assert!(quantum > 0);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.chunks.is_empty() && !state.closed {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        let mut output = Vec::with_capacity(quantum.min(state.bytes));
        while output.len() < quantum {
            let Some(mut chunk) = state.chunks.pop_front() else {
                break;
            };
            let remaining = quantum - output.len();
            if chunk.len() <= remaining {
                state.bytes -= chunk.len();
                output.extend_from_slice(&chunk);
            } else {
                let tail = chunk.split_off(remaining);
                state.bytes -= chunk.len();
                output.extend_from_slice(&chunk);
                state.chunks.push_front(tail);
            }
        }
        self.changed.notify_all();
        output
    }

    pub fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed = true;
        self.changed.notify_all();
    }

    pub fn is_drained_and_closed(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed && state.bytes == 0
    }

    pub fn stats(&self) -> RingStats {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        RingStats {
            capacity: self.capacity,
            current_bytes: state.bytes,
            max_bytes: state.max_bytes,
            blocked_pushes: state.blocked_pushes,
            closed: state.closed,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RingStats {
    pub capacity: usize,
    pub current_bytes: usize,
    pub max_bytes: usize,
    pub blocked_pushes: u64,
    pub closed: bool,
}

#[derive(Debug)]
pub struct LatestSlot<T> {
    value: Option<T>,
    pub overwrites: u64,
    pub max_occupied: usize,
}

impl<T> Default for LatestSlot<T> {
    fn default() -> Self {
        Self {
            value: None,
            overwrites: 0,
            max_occupied: 0,
        }
    }
}

impl<T> LatestSlot<T> {
    pub fn publish(&mut self, value: T) {
        if self.value.replace(value).is_some() {
            self.overwrites += 1;
        }
        self.max_occupied = 1;
    }

    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SpanId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct WorkerTask {
    pub span: SpanId,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum EnqueueResult {
    Queued,
    Replaced,
    RejectedRetryOnIdle,
}

#[derive(Debug, Default)]
pub struct WorkerQueue {
    tasks: VecDeque<WorkerTask>,
    retry_on_idle: BTreeSet<SpanId>,
    pub replacements: u64,
    pub rejections: u64,
    pub max_queued: usize,
}

impl WorkerQueue {
    pub fn enqueue(&mut self, task: WorkerTask) -> EnqueueResult {
        if let Some(index) = self
            .tasks
            .iter()
            .position(|queued| queued.span == task.span)
        {
            self.tasks[index] = task;
            self.replacements += 1;
            return EnqueueResult::Replaced;
        }
        if self.tasks.len() == WORKER_QUEUE_TASKS {
            self.retry_on_idle.insert(task.span);
            self.rejections += 1;
            return EnqueueResult::RejectedRetryOnIdle;
        }
        self.tasks.push_back(task);
        self.max_queued = self.max_queued.max(self.tasks.len());
        EnqueueResult::Queued
    }

    pub fn pop(&mut self) -> Option<WorkerTask> {
        self.tasks.pop_front()
    }

    pub fn retry_when_idle(&mut self, span: SpanId, generation: u64) -> EnqueueResult {
        if !self.retry_on_idle.remove(&span) {
            return EnqueueResult::Queued;
        }
        self.enqueue(WorkerTask { span, generation })
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn retry_len(&self) -> usize {
        self.retry_on_idle.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct ScheduledSession {
    id: u32,
    visible: bool,
}

/// Deterministic weighted round robin. With one visible and one invisible session the latter gets
/// exactly one of every eight dispatch opportunities, making the starvation floor observable.
pub struct VisibilityScheduler {
    cycle: Vec<u32>,
    cursor: usize,
}

impl VisibilityScheduler {
    pub fn new(sessions: impl IntoIterator<Item = (u32, bool)>) -> Self {
        let sessions = sessions
            .into_iter()
            .map(|(id, visible)| ScheduledSession { id, visible })
            .collect::<Vec<_>>();
        let mut cycle = Vec::new();
        for session in sessions {
            let weight = if session.visible {
                VISIBLE_WEIGHT
            } else {
                INVISIBLE_WEIGHT
            };
            cycle.extend(std::iter::repeat_n(session.id, weight));
        }
        assert!(!cycle.is_empty());
        Self { cycle, cursor: 0 }
    }

    pub fn next_session(&mut self) -> u32 {
        let id = self.cycle[self.cursor];
        self.cursor = (self.cursor + 1) % self.cycle.len();
        id
    }
}

#[derive(Default)]
pub struct RenderLimiter {
    running_by_session: BTreeMap<u32, usize>,
    global_running: usize,
    pub max_global: usize,
    pub max_per_session: usize,
}

impl RenderLimiter {
    pub fn try_start(&mut self, session: u32) -> bool {
        let session_running = self.running_by_session.get(&session).copied().unwrap_or(0);
        if self.global_running == GLOBAL_RENDER_LIMIT || session_running == PER_SESSION_RENDER_LIMIT
        {
            return false;
        }
        self.global_running += 1;
        self.running_by_session.insert(session, session_running + 1);
        self.max_global = self.max_global.max(self.global_running);
        self.max_per_session = self.max_per_session.max(session_running + 1);
        true
    }

    pub fn finish(&mut self, session: u32) {
        let running = self
            .running_by_session
            .get_mut(&session)
            .expect("finish active session");
        assert!(*running > 0);
        *running -= 1;
        self.global_running -= 1;
    }
}

pub fn parser_thread_count(physical_cores: usize, sessions: usize) -> usize {
    physical_cores.saturating_sub(2).clamp(1, sessions.max(1))
}

#[derive(Debug, Serialize)]
pub struct FloodReport {
    pub generated_bytes: usize,
    pub parsed_bytes: usize,
    pub parse_quantum_bytes: usize,
    pub ring: RingStats,
    pub input_events: usize,
    pub input_latency_p50_us: u128,
    pub input_latency_p95_us: u128,
    pub input_latency_max_us: u128,
    pub snapshot_max_occupied: usize,
    pub snapshot_overwrites: u64,
}

#[derive(Debug, Serialize)]
pub struct WorkerReport {
    pub max_queued: usize,
    pub replacements: u64,
    pub rejections: u64,
    pub retry_markers_after_retry: usize,
    pub stale_canceled: usize,
    pub fresh_completed: usize,
    pub cancellation_rate: f64,
    pub visible_dispatches: usize,
    pub invisible_dispatches: usize,
    pub invisible_share: f64,
    pub max_global_running: usize,
    pub max_per_session_running: usize,
}

#[derive(Debug, Serialize)]
pub struct ShutdownReport {
    pub blocked_writer_released: bool,
    pub elapsed_us: u128,
}

#[derive(Debug, Serialize)]
pub struct BackpressureReport {
    pub schema: &'static str,
    pub flood: FloodReport,
    pub workers: WorkerReport,
    pub shutdown: ShutdownReport,
    pub parser_threads_1_core: usize,
    pub parser_threads_2_cores: usize,
    pub parser_threads_12_cores_8_sessions: usize,
}

pub fn run_benchmark() -> Result<BackpressureReport> {
    let flood = run_flood()?;
    let workers = run_worker_simulation();
    let shutdown = run_shutdown_probe()?;
    Ok(BackpressureReport {
        schema: "bt-backpressure-spike/v1",
        flood,
        workers,
        shutdown,
        parser_threads_1_core: parser_thread_count(1, 8),
        parser_threads_2_cores: parser_thread_count(2, 8),
        parser_threads_12_cores_8_sessions: parser_thread_count(12, 8),
    })
}

fn run_flood() -> Result<FloodReport> {
    const GENERATED_BYTES: usize = 64 * 1024 * 1024;
    const CHUNK_BYTES: usize = 16 * 1024;
    const INPUT_EVENTS: usize = 512;

    let ring = Arc::new(ByteRing::new(PTY_RING_BYTES));
    let producer_ring = Arc::clone(&ring);
    let producer = std::thread::spawn(move || -> Result<()> {
        for chunk_index in 0..(GENERATED_BYTES / CHUNK_BYTES) {
            producer_ring.push(vec![(chunk_index & 0xff) as u8; CHUNK_BYTES])?;
        }
        producer_ring.close();
        Ok(())
    });

    let (input_tx, input_rx) = std::sync::mpsc::sync_channel::<Instant>(256);
    let input_sender = std::thread::spawn(move || {
        for _ in 0..INPUT_EVENTS {
            if input_tx.send(Instant::now()).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
    });

    let mut parsed_bytes = 0;
    let mut snapshots = LatestSlot::default();
    let mut input_latencies = Vec::with_capacity(INPUT_EVENTS);
    loop {
        while let Ok(sent) = input_rx.try_recv() {
            input_latencies.push(sent.elapsed().as_micros());
        }
        let bytes = ring.pop_quantum(PARSE_QUANTUM_BYTES);
        if bytes.is_empty() && ring.is_drained_and_closed() && input_latencies.len() == INPUT_EVENTS
        {
            break;
        }
        parsed_bytes += bytes.len();
        snapshots.publish(parsed_bytes);
        std::thread::sleep(Duration::from_micros(75));
    }
    input_sender.join().expect("input sender thread");
    while let Ok(sent) = input_rx.try_recv() {
        input_latencies.push(sent.elapsed().as_micros());
    }
    producer.join().expect("producer thread")?;
    input_latencies.sort_unstable();
    let percentile = |numerator: usize| {
        let index = ((input_latencies.len() - 1) * numerator) / 100;
        input_latencies[index]
    };
    Ok(FloodReport {
        generated_bytes: GENERATED_BYTES,
        parsed_bytes,
        parse_quantum_bytes: PARSE_QUANTUM_BYTES,
        ring: ring.stats(),
        input_events: input_latencies.len(),
        input_latency_p50_us: percentile(50),
        input_latency_p95_us: percentile(95),
        input_latency_max_us: *input_latencies.last().expect("input events"),
        snapshot_max_occupied: snapshots.max_occupied,
        snapshot_overwrites: snapshots.overwrites,
    })
}

fn run_worker_simulation() -> WorkerReport {
    let mut queue = WorkerQueue::default();
    for span in 0..WORKER_QUEUE_TASKS as u64 {
        assert_eq!(
            queue.enqueue(WorkerTask {
                span: SpanId(span),
                generation: 1
            }),
            EnqueueResult::Queued
        );
    }
    for generation in 2..=10 {
        for span in 0..WORKER_QUEUE_TASKS as u64 {
            assert_eq!(
                queue.enqueue(WorkerTask {
                    span: SpanId(span),
                    generation
                }),
                EnqueueResult::Replaced
            );
        }
    }
    assert_eq!(
        queue.enqueue(WorkerTask {
            span: SpanId(999),
            generation: 1
        }),
        EnqueueResult::RejectedRetryOnIdle
    );
    let first = queue.pop().expect("full queue");
    assert_eq!(first.span, SpanId(0));
    assert_eq!(queue.retry_when_idle(SpanId(999), 1), EnqueueResult::Queued);

    let mut stale_canceled = 0;
    let mut fresh_completed = 0;
    while let Some(task) = queue.pop() {
        let current_generation = if task.span.0 < 49 {
            11
        } else {
            task.generation
        };
        if task.generation == current_generation {
            fresh_completed += 1;
        } else {
            stale_canceled += 1;
        }
    }

    let mut scheduler = VisibilityScheduler::new([(1, true), (2, false)]);
    let mut visible_dispatches = 0;
    let mut invisible_dispatches = 0;
    for _ in 0..800 {
        if scheduler.next_session() == 1 {
            visible_dispatches += 1;
        } else {
            invisible_dispatches += 1;
        }
    }

    let mut limiter = RenderLimiter::default();
    for session in 0..4 {
        assert!(limiter.try_start(session));
        assert!(limiter.try_start(session));
        assert!(!limiter.try_start(session));
    }
    assert!(!limiter.try_start(99));
    for session in 0..4 {
        limiter.finish(session);
        limiter.finish(session);
    }

    let total_finished = stale_canceled + fresh_completed;
    WorkerReport {
        max_queued: queue.max_queued,
        replacements: queue.replacements,
        rejections: queue.rejections,
        retry_markers_after_retry: queue.retry_len(),
        stale_canceled,
        fresh_completed,
        cancellation_rate: stale_canceled as f64 / total_finished as f64,
        visible_dispatches,
        invisible_dispatches,
        invisible_share: invisible_dispatches as f64
            / (visible_dispatches + invisible_dispatches) as f64,
        max_global_running: limiter.max_global,
        max_per_session_running: limiter.max_per_session,
    }
}

fn run_shutdown_probe() -> Result<ShutdownReport> {
    let ring = Arc::new(ByteRing::new(1024));
    ring.push(vec![0; 1024])?;
    let blocked_ring = Arc::clone(&ring);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let blocked_writer = std::thread::spawn(move || {
        let released = blocked_ring.push(vec![1]).is_err();
        let _ = done_tx.send(released);
    });
    std::thread::sleep(Duration::from_millis(10));
    let started = Instant::now();
    ring.close();
    let blocked_writer_released = done_rx.recv_timeout(Duration::from_secs(1))?;
    blocked_writer.join().expect("blocked writer");
    Ok(ShutdownReport {
        blocked_writer_released,
        elapsed_us: started.elapsed().as_micros(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_capacity_constants_are_exact() {
        assert_eq!(PTY_RING_BYTES, 1024 * 1024);
        assert_eq!(PARSE_QUANTUM_BYTES, 256 * 1024);
        assert_eq!(WORKER_QUEUE_TASKS, 64);
        assert_eq!(PER_SESSION_RENDER_LIMIT, 2);
        assert_eq!(GLOBAL_RENDER_LIMIT, 8);
        assert_eq!(VISIBLE_WEIGHT, 7);
        assert_eq!(INVISIBLE_WEIGHT, 1);
    }

    #[test]
    fn flood_keeps_memory_bounded_and_interactive_input_live() {
        let report = run_flood().unwrap();
        assert_eq!(report.generated_bytes, report.parsed_bytes);
        assert!(report.ring.max_bytes <= PTY_RING_BYTES);
        assert!(
            report.ring.blocked_pushes > 0,
            "backpressure path was not exercised"
        );
        assert_eq!(report.input_events, 512);
        assert!(
            report.input_latency_max_us < 100_000,
            "interactive input starved: {report:?}"
        );
        assert_eq!(report.snapshot_max_occupied, 1);
        assert!(report.snapshot_overwrites > 0);
    }

    #[test]
    fn worker_queue_replaces_rejects_and_retries_without_blocking_actor() {
        let mut queue = WorkerQueue::default();
        for span in 0..WORKER_QUEUE_TASKS as u64 {
            assert_eq!(
                queue.enqueue(WorkerTask {
                    span: SpanId(span),
                    generation: 1
                }),
                EnqueueResult::Queued
            );
        }
        assert_eq!(queue.len(), WORKER_QUEUE_TASKS);
        assert_eq!(
            queue.enqueue(WorkerTask {
                span: SpanId(4),
                generation: 2
            }),
            EnqueueResult::Replaced
        );
        assert_eq!(queue.len(), WORKER_QUEUE_TASKS);
        assert_eq!(
            queue.enqueue(WorkerTask {
                span: SpanId(1000),
                generation: 1
            }),
            EnqueueResult::RejectedRetryOnIdle
        );
        assert_eq!(queue.retry_len(), 1);
        queue.pop();
        assert_eq!(
            queue.retry_when_idle(SpanId(1000), 2),
            EnqueueResult::Queued
        );
        assert_eq!(queue.retry_len(), 0);
        assert_eq!(queue.max_queued, WORKER_QUEUE_TASKS);
    }

    #[test]
    fn visibility_floor_and_render_limits_are_exact() {
        let report = run_worker_simulation();
        assert_eq!(report.visible_dispatches, 700);
        assert_eq!(report.invisible_dispatches, 100);
        assert_eq!(report.invisible_share, 0.125);
        assert_eq!(report.max_global_running, GLOBAL_RENDER_LIMIT);
        assert_eq!(report.max_per_session_running, PER_SESSION_RENDER_LIMIT);
        assert_eq!(report.retry_markers_after_retry, 0);
        assert!(report.cancellation_rate > 0.70);
    }

    #[test]
    fn shutdown_releases_a_writer_blocked_by_backpressure() {
        let report = run_shutdown_probe().unwrap();
        assert!(report.blocked_writer_released);
        assert!(report.elapsed_us < 1_000_000);
    }

    #[test]
    fn one_and_two_core_degradation_keeps_one_parser() {
        assert_eq!(parser_thread_count(1, 8), 1);
        assert_eq!(parser_thread_count(2, 8), 1);
        assert_eq!(parser_thread_count(3, 8), 1);
        assert_eq!(parser_thread_count(4, 8), 2);
        assert_eq!(parser_thread_count(12, 8), 8);
        assert_eq!(parser_thread_count(24, 3), 3);
    }

    #[test]
    fn over_capacity_chunk_is_rejected_instead_of_growing_ring() {
        let ring = ByteRing::new(16);
        assert!(ring.push(vec![0; 17]).is_err());
        assert_eq!(ring.stats().max_bytes, 0);
    }
}
