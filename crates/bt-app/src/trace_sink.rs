//! One bounded queue and writer for performance, resize, and named file traces.
//!
//! A symbolized hang report found the window thread blocked for 5.9 seconds in
//! `ZwWriteFile` under `_eprint` while reporting a renderer frame. Producers now
//! format and timestamp their lines, then try to enqueue them without waiting.
//! Overflow and lock contention drop lines; the writer reports their count.
//!
//! Only a run with a nonempty trace or dump variable starts the writer. File
//! opens, headers, and writes happen on that thread. Shutdown drains under a
//! one-second deadline, because a writer stalled in the kernel cannot be joined
//! indefinitely. Tests without a sink retain synchronous output.
//!
//! Ordinary diagnostics and the hold logger are outside this queue: they go to
//! `diagnostics.log` by a path of their own — see [`crate::diagnostics::note`] —
//! so that neither the watchdog nor a resident UI diagnostic ever waits for
//! whoever is reading a trace. The exit footer shares the queue when active so
//! stderr contention cannot prevent shutdown from reaching the bounded flush.
//!
//! **Nothing but the writer thread ever waits on this sink** (X-7). Producers
//! `try_lock` and `try_send`, and the writer puts its batch on this process's
//! standard error *without* Rust's shared `Stderr` lock
//! ([`bt_platform::write_std_error`]): a thread stuck for seconds inside one
//! `WriteFile` must not also be holding the mutex every `eprintln!` in the
//! workspace goes through, which would reinstate the very hang one layer out.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bt_platform::admission::{WaitToken, admitted, doors};

use crate::trace::TraceFile;

/// **How many lines may be waiting for the writer before one is dropped.**
///
/// Four thousand is about eleven seconds of the heaviest trace this program
/// writes (two and a half lines a frame at 144 Hz) and about 4 MB of `String`
/// if every one of them were the renderer's kilobyte, which is the number that
/// actually sets it: the queue is a shock absorber for a reader that stopped for
/// a second, not a place to keep a recording. A stall longer than the absorber
/// is meant to lose lines and say how many.
pub const QUEUE_DEPTH: usize = 4096;

/// **How long an orderly exit waits for the writer to finish.**
///
/// A bound and not a `join`, because the failure this module exists for is
/// *precisely* a writer stuck in a kernel write that may never return: a
/// `join()` here would move the hang from the middle of the run to the end of
/// it. One second is long enough for a queue this size to reach a working
/// handle and short enough that nobody watching a window close would call it a
/// pause. Whatever is still queued when it expires is lost, which is the same
/// bargain every other line in this module is written under.
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(1);

/// Where one queued line is going.
pub enum Destination {
    /// This run's `stderr` — the console when a trace variable named one, and
    /// `diagnostics.log` otherwise. See `crate::diagnostics`.
    Stderr,
    /// A named trace destination. Held as an `Arc` and not a borrow because
    /// the line outlives the call that made it: it is written by the sink's
    /// thread, some microseconds after the window thread let go of it.
    File(Arc<TraceFile>),
}

/// One line, with the timestamp its producer gave it already in the text.
///
/// **Formatted on the producing thread and never on the writer's.** A trace
/// file's first column is a monotonic millisecond and the whole reading these
/// files support is merging two of them on that column; a stamp taken after a
/// queue would be the time the line was *written*, which is a different fact
/// about a different thread.
struct Line {
    destination: Destination,
    text: String,
}

/// The producer's half: a bounded sender and the tally of what would not fit.
struct Queue {
    /// `None` once [`flush`] has let the sender go.
    ///
    /// Producers only try this lock; contention drops a line too. The writer
    /// never takes it, so disk and pipe latency cannot reach a producer.
    lines: Mutex<Option<SyncSender<Line>>>,
    dropped: Arc<AtomicU64>,
}

impl Queue {
    fn new(lines: SyncSender<Line>, dropped: Arc<AtomicU64>) -> Self {
        Self {
            lines: Mutex::new(Some(lines)),
            dropped,
        }
    }

    /// **Hand one line over, or lose it. Never wait.**
    ///
    /// `try_send` and not `send`: the sender is bounded, and `send` on a full
    /// bounded channel blocks until the writer takes one — which is the fault
    /// this module was written to remove, reintroduced one layer up.
    ///
    /// Answers whether the line was taken, for the one caller that has somewhere
    /// else to put it — [`offer_stderr_line`]. Every other caller has already
    /// said everything it can say about a dropped line by dropping it.
    fn offer(&self, line: Line) -> bool {
        let queued = match self.lines.try_lock() {
            Ok(lines) => lines
                .as_ref()
                .is_some_and(|lines| lines.try_send(line).is_ok()),
            Err(TryLockError::Poisoned(poison)) => poison
                .into_inner()
                .as_ref()
                .is_some_and(|lines| lines.try_send(line).is_ok()),
            Err(TryLockError::WouldBlock) => false,
        };
        if !queued {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        queued
    }

    /// Let the sender go, which is what ends the writer's `recv` once the queue
    /// behind it is empty.
    fn close(&self) -> bool {
        match self.lines.try_lock() {
            Ok(mut lines) => {
                lines.take();
                true
            }
            Err(TryLockError::Poisoned(poison)) => {
                poison.into_inner().take();
                true
            }
            Err(TryLockError::WouldBlock) => false,
        }
    }
}

/// This process's sink: the queue, the thread behind it, and the way to tell
/// that the thread has finished.
struct Sink {
    queue: Queue,
    /// Taken by [`flush`] once the writer has said it is done.
    writer: Mutex<Option<JoinHandle<()>>>,
    /// A rendezvous channel whose sender lives in the writer's body, so it
    /// disconnects the moment that body returns. [`Receiver::recv_timeout`] on
    /// it is the bounded wait [`FLUSH_TIMEOUT`] describes.
    finished: Mutex<Receiver<()>>,
}

/// [`open`]'s answer, resolved once.
static SINK: OnceLock<Option<Sink>> = OnceLock::new();

/// Whether startup has resolved the sink, including a run with tracing off.
pub fn started() -> bool {
    SINK.get().is_some()
}

/// **Start the writer thread, if this run asked for a trace.**
///
/// Called once, from `main`, beside `diagnostics::enter_resident_run` — and it
/// is **the only thing that opens [`SINK`]**, which is what makes "is there a
/// queue" a fact about the program rather than about the environment a binary
/// happens to inherit. A test binary and `bt-replay` never call it, so their
/// lines are written where they are made, synchronously, the way they always
/// were; nothing in a test has to wait for a thread it did not start.
pub struct Shutdown;

impl Drop for Shutdown {
    /// Reached only on `main`'s early return from a loop that could not be built, which says
    /// `exiting()` first (the design note's revision (c)5), so the flush is admitted there. A
    /// refusal loses what is still queued, as the flush's own timeout does.
    fn drop(&mut self) {
        let _ = admitted::<doors::TraceFlush, _>(flush);
    }
}

pub fn start() -> Shutdown {
    let _ = SINK.get_or_init(open);
    Shutdown
}

/// One line for `stderr`, queued or written here.
pub fn stderr_line(text: String) {
    write_line(Destination::Stderr, text);
}

/// **One line for `stderr` if this run has a queue, and nothing at all if it
/// does not** (X-7).
///
/// [`stderr_line`]'s pair for the callers that must never write here: a run
/// without a sink writes the line on the calling thread, which is right for the
/// traces that road serves and wrong for [`crate::diagnostics::note`], whose
/// entire reason for existing is that the calling thread must not touch a
/// console. So this one queues or gives up, and never writes.
///
/// Answers whether the line was taken. `false` is a run with no sink, a full
/// queue or a contended sender — all of which mean the same thing to the
/// caller, which has already written the line where it really belongs.
pub fn offer_stderr_line(text: String) -> bool {
    let Some(sink) = sink() else {
        return false;
    };
    sink.queue.offer(Line {
        destination: Destination::Stderr,
        text,
    })
}

/// One line for a named trace's file, queued or written here.
pub fn file_line(file: Arc<TraceFile>, text: String) {
    write_line(Destination::File(file), text);
}

/// **Write everything already said, then stop.**
///
/// Called on the way out of `main`, after the footer and before
/// `bt_platform::leave_process` — so the last thing in a trace is the last thing
/// that happened, and not whatever the queue happened to be holding.
///
/// **An owner-thread door** (`doors::TraceFlush`, §5.3 row 17): a bounded wait, admitted only on
/// the way out, minted in `main` and in [`Shutdown`]'s drop.
pub fn flush(token: WaitToken<'_, doors::TraceFlush>) {
    let _ = token;
    let Some(sink) = SINK.get().and_then(Option::as_ref) else {
        return;
    };
    flush_sink(sink, FLUSH_TIMEOUT);
}

fn flush_sink(sink: &Sink, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !sink.queue.close() {
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let Ok(finished) = sink.finished.try_lock() else {
        return;
    };
    let _ = finished.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    // Completion of the body can precede thread-local cleanup. Only join a
    // thread that has actually finished, and include that wait in the bound.
    let Ok(mut writer) = sink.writer.try_lock() else {
        return;
    };
    if let Some(handle) = writer.take() {
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
        // Otherwise dropping the handle detaches the stalled writer.
    }
}

fn write_line(destination: Destination, text: String) {
    let Some(sink) = sink() else {
        // No sink: a test binary, `bt-replay`, or a run that named no trace at
        // all. Straight to the destination, the way every one of these lines was
        // written before there was a queue.
        write_here(&destination, &text);
        return;
    };
    sink.queue.offer(Line { destination, text });
}

fn write_here(destination: &Destination, text: &str) {
    match destination {
        Destination::Stderr => {
            let _ = writeln!(std::io::stderr(), "{text}");
        }
        Destination::File(file) => file.append(text),
    }
}

/// This run's sink, or `None` in a run that has not [`start`]ed one.
fn sink() -> Option<&'static Sink> {
    SINK.get().and_then(Option::as_ref)
}

fn open() -> Option<Sink> {
    if !a_trace_was_asked_for(
        std::env::vars_os()
            .filter(|(_, value)| !value.is_empty())
            .map(|(name, _)| name),
    ) {
        return None;
    }
    let (lines, waiting) = sync_channel(QUEUE_DEPTH);
    let (done, finished) = sync_channel::<()>(0);
    let dropped = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&dropped);
    // Below normal, like every other worker this process starts: the window
    // thread is one step above it, and a writer that cannot keep up is supposed
    // to lose lines rather than take a slice from the frame.
    let writer = bt_platform::spawn_at_priority(
        "bt-trace-sink",
        bt_platform::ThreadPriority::BelowNormal,
        move |_ctx| {
            run(&waiting, &counted);
            // The one thing that makes `flush`'s wait a wait on this body.
            drop(done);
        },
    )
    // Failed spawn leaves a disconnected queue: lose traces, never fall back
    // to blocking the producer in the failure mode this sink exists to avoid.
    .ok();
    Some(Sink {
        queue: Queue::new(lines, dropped),
        writer: Mutex::new(writer),
        finished: Mutex::new(finished),
    })
}

/// **The writer thread's whole body.**
///
/// One `recv` to park on, then everything else that is already waiting taken
/// without parking again — so a burst of frames costs one write to `stderr`
/// rather than one per line. A file line flushes whatever `stderr` batch is
/// pending before it goes, because the reading two traces of one run support is
/// a merge on their first column, and that merge is only sound while the order
/// the lines were *offered* in is the order they come out in.
fn run(lines: &Receiver<Line>, dropped: &AtomicU64) {
    run_to(lines, dropped, &mut ProcessStderr);
}

/// **This run's standard error, written without the lock every other writer of
/// it shares** (X-7).
///
/// [`std::io::Stderr`] is one process-wide mutex, taken for the length of the
/// write. This writer is the one that can be inside a write for seconds — a
/// console whose reader stopped reading is the fault this whole module answers
/// — so holding that mutex here would make every `eprintln!` in the process
/// wait for the same stalled console, which is the hang moved rather than
/// removed. The slot is re-read on each write, so the destination
/// [`crate::diagnostics`] chose is still the destination.
///
/// Unbuffered, so [`std::io::Write::flush`] has nothing to do: the batching
/// this module wants is [`run_to`]'s `String`, which is bounded on purpose.
struct ProcessStderr;

impl std::io::Write for ProcessStderr {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bt_platform::write_std_error(bytes) {
            Ok(bytes.len())
        } else {
            Err(std::io::Error::other("standard error refused the batch"))
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn run_to(lines: &Receiver<Line>, dropped: &AtomicU64, stderr: &mut impl std::io::Write) {
    let mut said = 0_u64;
    let mut batch = String::new();
    loop {
        let first = match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(first) => first,
            Err(error) => {
                report_drops(&mut batch, dropped, &mut said);
                put(&mut batch, stderr);
                if error == RecvTimeoutError::Disconnected {
                    break;
                }
                continue;
            }
        };
        let mut line = first;
        for index in 0..256 {
            let Line { destination, text } = line;
            match destination {
                Destination::Stderr => {
                    batch.push_str(&text);
                    batch.push('\n');
                }
                Destination::File(file) => {
                    put(&mut batch, stderr);
                    file.append(&text);
                }
            }
            // Bound the batch too: a continuously replenished queue must not
            // turn into an unbounded String or postpone its write forever.
            if index == 255 || batch.len() >= 64 * 1024 {
                break;
            }
            match lines.try_recv() {
                Ok(next) => line = next,
                Err(_) => break,
            }
        }
        // **Said once per change and not once per drop.** A reader needs to know
        // that the recording has a hole in it and how big; a line per lost line
        // would be the queue overflowing into the thing that overflowed.
        report_drops(&mut batch, dropped, &mut said);
        put(&mut batch, stderr);
    }
    let _ = stderr.flush();
}

fn report_drops(batch: &mut String, dropped: &AtomicU64, said: &mut u64) {
    let lost = dropped.load(Ordering::Relaxed);
    if lost != *said {
        *said = lost;
        let _ = writeln!(batch, "BT_PERF_TRACE dropped={lost}");
    }
}

/// The batch, in one write, to wherever this run's `stderr` points.
///
/// The standard stderr handle follows the destination `diagnostics` moves the
/// process to with `SetStdHandle`; the whole of that design is that
/// no call site knows it moved.
fn put(batch: &mut String, stderr: &mut impl std::io::Write) {
    if batch.is_empty() {
        return;
    }
    let _ = stderr.write_all(batch.as_bytes());
    batch.clear();
}

/// Trace switches plus the focus-thumbnail file dump use this sink. Other
/// dumps, such as the PTY byte capture, have their own writers and need no idle
/// sink thread. Names are supplied separately so tests never mutate the process
/// environment; `open` removes empty values before calling this function.
fn a_trace_was_asked_for<I: IntoIterator<Item = std::ffi::OsString>>(names: I) -> bool {
    names.into_iter().any(|name| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        name.starts_with("BT_") && (name.contains("TRACE") || name == "BT_FOCUS_THUMB_DUMP")
    })
}

/// **A sink whose writer is parked inside its write**, for the tests — here and
/// in [`crate::hang_watch`] — that have to show that nothing else in the process
/// waits for one (X-7).
///
/// The real fault in one object: a trace destination whose reader has stopped
/// reading, so the writer thread is inside `write` and stays there. Everything
/// offered to it afterwards meets a full queue, which is the state every
/// producer's promise is about.
#[cfg(test)]
pub(crate) struct StalledWriter {
    queue: Queue,
    /// Dropping this is what lets the writer out of its write.
    release: Option<SyncSender<()>>,
    writer: Option<JoinHandle<()>>,
}

#[cfg(test)]
impl StalledWriter {
    /// Start one, and answer only once the writer is actually inside a write.
    ///
    /// The wait is the whole value of the helper: a test that began offering
    /// before the stall would be timing a working sink for its first lines.
    pub(crate) fn start() -> Self {
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let (release, held) = sync_channel::<()>(0);
        let (entered, arrived) = sync_channel::<()>(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        let writer = std::thread::spawn(move || {
            run_to(&waiting, &counted, &mut BlockedStderr { entered, held });
        });
        let queue = Queue::new(lines, dropped);
        queue.offer(Line {
            destination: Destination::Stderr,
            text: "the line that parks the writer".to_owned(),
        });
        arrived
            .recv_timeout(Duration::from_secs(5))
            .expect("the writer reached its write");
        Self {
            queue,
            release: Some(release),
            writer: Some(writer),
        }
    }

    /// Offer one line to the stalled sink. Answers whether it was taken.
    pub(crate) fn offer(&self, text: &str) -> bool {
        self.queue.offer(Line {
            destination: Destination::Stderr,
            text: text.to_owned(),
        })
    }

    /// How many lines this sink has lost.
    pub(crate) fn dropped(&self) -> u64 {
        self.queue.dropped.load(Ordering::Relaxed)
    }

    /// Fill the queue, so that the next offer meets a full one.
    pub(crate) fn fill(&self) {
        for index in 0..QUEUE_DEPTH {
            self.offer(&format!("filler={index}"));
        }
    }
}

#[cfg(test)]
impl Drop for StalledWriter {
    fn drop(&mut self) {
        // Released first and closed second: a queue closed while the writer is
        // still in its write would leave this thread waiting for a `join` on a
        // thread that cannot return, which is the hang the tests are about.
        drop(self.release.take());
        while !self.queue.close() {
            std::thread::yield_now();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// [`StalledWriter`]'s destination: a write that does not return until the
/// helper is dropped.
#[cfg(test)]
struct BlockedStderr {
    entered: SyncSender<()>,
    held: Receiver<()>,
}

#[cfg(test)]
impl std::io::Write for BlockedStderr {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        // `try_send` because only the first arrival is being waited for, and a
        // second write must not wait for a reader of this channel.
        let _ = self.entered.try_send(());
        // `Err` — the sender dropped — is the release, and every write after it
        // returns at once.
        let _ = self.held.recv();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A mutex this module never poisons on purpose, unwrapped without a panic path.
#[cfg(test)]
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("bt-sink-{}-{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn stderr_line(text: &str) -> Line {
        Line {
            destination: Destination::Stderr,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_full_queue_refuses_at_once_and_counts_what_it_dropped() {
        let (lines, held) = sync_channel(2);
        let queue = Arc::new(Queue::new(lines, Arc::new(AtomicU64::new(0))));
        queue.offer(stderr_line("first"));
        queue.offer(stderr_line("second"));
        let producer_queue = Arc::clone(&queue);
        let (done, finished) = sync_channel(1);
        let producer = std::thread::spawn(move || {
            for _ in 0..1000 {
                producer_queue.offer(stderr_line("overflow"));
            }
            let _ = done.send(());
        });
        let completed = finished.recv_timeout(Duration::from_secs(1));
        if completed.is_ok() {
            assert_eq!(queue.dropped.load(Ordering::Relaxed), 1000);
            assert_eq!(held.try_recv().unwrap().text, "first");
            assert_eq!(held.try_recv().unwrap().text, "second");
            assert!(held.try_recv().is_err());
        }
        drop(held);
        producer.join().unwrap();
        assert!(completed.is_ok(), "a producer blocked on a full queue");
    }

    #[test]
    fn a_busy_producer_lock_drops_instead_of_waiting() {
        let (lines, _waiting) = sync_channel(2);
        let queue = Arc::new(Queue::new(lines, Arc::new(AtomicU64::new(0))));
        let guard = lock(&queue.lines);
        let offered = Arc::clone(&queue);
        let (done, finished) = sync_channel(1);
        let producer = std::thread::spawn(move || {
            offered.offer(stderr_line("contended"));
            let _ = done.send(());
        });
        let completed = finished.recv_timeout(Duration::from_secs(1));
        drop(guard);
        producer.join().unwrap();
        assert!(completed.is_ok(), "a producer waited for the sender lock");
        assert_eq!(queue.dropped.load(Ordering::Relaxed), 1);
    }

    /// **And what does come out comes out in the order it was offered.**
    ///
    /// A file rather than `stderr` so the test says nothing on the harness's own
    /// channel, and the real writer body rather than a stand-in: the batching
    /// loop inside [`run`] is the part that could reorder, and it is the part
    /// under test.
    ///
    /// MUTATION: drain into a `Vec` and write it reversed; the sequence breaks.
    #[test]
    fn the_writer_puts_the_lines_out_in_the_order_they_were_offered() {
        let path = scratch("order");
        let file = Arc::new(TraceFile::new(&path, None));
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        let writer = std::thread::spawn(move || run(&waiting, &counted));
        let queue = Queue::new(lines, dropped);
        for index in 0..500 {
            queue.offer(Line {
                destination: Destination::File(Arc::clone(&file)),
                text: format!("line={index}"),
            });
        }
        assert!(queue.close());
        writer.join().expect("the writer thread returns");
        let body = std::fs::read_to_string(&path).expect("the trace file was written");
        let written: Vec<&str> = body.lines().collect();
        assert_eq!(written.len(), 500, "one line out per line in");
        for (index, line) in written.iter().enumerate() {
            assert_eq!(
                *line,
                format!("line={index}"),
                "the lines came out shuffled"
            );
        }
        assert_eq!(
            queue.dropped.load(Ordering::Relaxed),
            0,
            "nothing was dropped: five hundred lines fit in a queue of {QUEUE_DEPTH}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **The fault itself: a writer stuck in a write nobody is reading.**
    ///
    /// Not a full queue arranged by hand but the real reason a queue fills —
    /// and what is asserted is the promise the whole module is for: the thread
    /// that offers a line gets on with its frame, and the recording says how
    /// much of itself is missing.
    ///
    /// MUTATION: give `Queue::offer` a `send` in place of its `try_send`; the
    /// producer never returns and the timeout fails the test.
    #[test]
    fn a_stalled_writer_never_delays_a_producer_and_the_count_grows() {
        let sink = StalledWriter::start();
        sink.fill();
        let dropped_before = sink.dropped();
        let started = Instant::now();
        for index in 0..1000 {
            sink.offer(&format!("frame={index}"));
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "a thousand offers to a stalled sink took {elapsed:?}"
        );
        assert_eq!(
            sink.dropped(),
            dropped_before + 1000,
            "every line offered to a full queue should have been counted"
        );
        assert!(
            sink.dropped() > 0,
            "a stalled sink that lost nothing did not stall"
        );
    }

    /// **And the trace's own writer does not wait for the lock every other
    /// writer of `stderr` shares** (X-7).
    ///
    /// The production body — [`run`], with the real [`ProcessStderr`] in its
    /// hand — against the state that lock is in whenever somebody else is
    /// mid-`eprintln!`. Held from **another** thread, because
    /// [`std::io::Stderr`]'s lock is reentrant and taking it on this one would
    /// say nothing about a second. What is asserted is that the writer reached
    /// the end of its body while the lock was still held; the one line it puts
    /// on the real `stderr` on its way there says why it is in the output.
    ///
    /// MUTATION: put `std::io::stderr()` back in `run`'s hand; the writer waits
    /// for the holder and the body does not finish inside the deadline.
    #[test]
    fn the_writer_does_not_wait_for_the_process_stderr_lock() {
        let (release, released) = sync_channel::<()>(0);
        let (locked, holding) = sync_channel::<()>(1);
        let holder = std::thread::spawn(move || {
            let _guard = std::io::stderr().lock();
            let _ = locked.send(());
            let _ = released.recv();
        });
        holding
            .recv_timeout(Duration::from_secs(5))
            .expect("the holder took the process lock");
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        let (done, finished) = sync_channel::<()>(1);
        let writer = std::thread::spawn(move || {
            run(&waiting, &counted);
            let _ = done.send(());
        });
        let queue = Queue::new(lines, dropped);
        queue.offer(stderr_line(
            "bt-app trace_sink test: one line written past a held stderr lock, on purpose",
        ));
        assert!(queue.close());
        let ended = finished.recv_timeout(Duration::from_secs(5));
        drop(release);
        holder.join().unwrap();
        writer.join().unwrap();
        assert!(
            ended.is_ok(),
            "the writer waited for a lock another thread was holding"
        );
    }

    #[test]
    fn the_writer_reports_drops_and_flushes_on_disconnect() {
        let (lines, waiting) = sync_channel(2);
        let queue = Queue::new(lines, Arc::new(AtomicU64::new(0)));
        queue.offer(stderr_line("first"));
        queue.offer(stderr_line("second"));
        queue.offer(stderr_line("lost"));
        assert!(queue.close());
        let mut output = Vec::new();
        run_to(&waiting, &queue.dropped, &mut output);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "first\nsecond\nBT_PERF_TRACE dropped=1\n"
        );
    }

    #[test]
    fn shutdown_does_not_wait_forever_for_a_stalled_writer() {
        let (lines, _waiting) = sync_channel(2);
        let (done, finished) = sync_channel::<()>(0);
        let (release, held) = sync_channel::<()>(1);
        let (returned, observed) = sync_channel::<()>(1);
        let writer = std::thread::spawn(move || {
            let _ = held.recv();
            drop(done);
            let _ = returned.send(());
        });
        let sink = Sink {
            queue: Queue::new(lines, Arc::new(AtomicU64::new(0))),
            writer: Mutex::new(Some(writer)),
            finished: Mutex::new(finished),
        };
        let started = Instant::now();
        flush_sink(&sink, Duration::from_millis(20));
        let elapsed = started.elapsed();
        release.send(()).unwrap();
        observed.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            elapsed < Duration::from_secs(1),
            "shutdown took {elapsed:?}"
        );
        assert!(
            lock(&sink.writer).is_none(),
            "stalled writer was not detached"
        );
    }

    #[test]
    fn file_open_and_header_wait_for_the_writer() {
        let path = scratch("lazy-header");
        let file = Arc::new(TraceFile::new(&path, Some("# header")));
        let (lines, waiting) = sync_channel(2);
        let queue = Queue::new(lines, Arc::new(AtomicU64::new(0)));
        for text in ["first", "second"] {
            queue.offer(Line {
                destination: Destination::File(Arc::clone(&file)),
                text: text.into(),
            });
        }
        assert!(!path.exists(), "the producer opened the file");
        assert!(queue.close());
        run_to(&waiting, &queue.dropped, &mut Vec::new());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# header\nfirst\nsecond\n"
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_gate_is_the_shape_of_a_trace_variable() {
        let names = |names: &[&str]| -> Vec<std::ffi::OsString> {
            names.iter().map(std::ffi::OsString::from).collect()
        };
        assert!(a_trace_was_asked_for(names(&["PATH", "BT_PERF_TRACE"])));
        assert!(a_trace_was_asked_for(names(&["BT_CARD_TRACE"])));
        assert!(a_trace_was_asked_for(names(&["BT_FOCUS_THUMB_DUMP"])));
        assert!(!a_trace_was_asked_for(names(&["BT_PTY_DUMP"])));
        // A made-up name tests the shape without declaring a real switch to
        // the environment-document check's whole-literal scan.
        assert!(a_trace_was_asked_for(names(&[concat!(
            "BT_",
            "SOMETHING_TRACE_V9"
        )])));
        assert!(!a_trace_was_asked_for(names(&["PATH", "APPDATA", "BT_BG"])));
    }

    /// RED (A1d, row 17) — **the trace's flush is one admitted `TraceFlush` on the way out, and
    /// is not waited for anywhere else.**
    ///
    /// Through the real road `main`'s early return takes: `Shutdown`'s drop. On a test thread
    /// entered as the window thread and on its way out it is admitted once; on the same kind of
    /// thread still running it is refused, and nothing is admitted.
    ///
    /// MUTATION: call `flush` from the drop outside its admission (a token cannot be had there,
    /// so: skip the admission and do nothing) and the first list is empty; give the door
    /// `Running` and the second is not.
    #[test]
    fn the_trace_is_flushed_through_its_door_only_on_the_way_out() {
        std::thread::spawn(|| {
            crate::tests::on_the_window_thread();
            drop(Shutdown);
            assert!(
                crate::hang_watch::admissions_on_this_thread().is_empty(),
                "a running window thread does not wait for the trace"
            );
            assert!(bt_platform::admission::exiting());
            drop(Shutdown);
            assert_eq!(
                crate::hang_watch::admissions_on_this_thread(),
                ["TraceFlush"],
                "on the way out, the drop's flush is one admission"
            );
        })
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    }
}
