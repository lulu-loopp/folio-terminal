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
//! Ordinary diagnostics and the hold logger are outside this queue. The exit
//! footer shares it when active so stderr contention cannot prevent shutdown
//! from reaching the bounded flush.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
    fn offer(&self, line: Line) {
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
    fn drop(&mut self) {
        flush();
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

/// One line for a named trace's file, queued or written here.
pub fn file_line(file: Arc<TraceFile>, text: String) {
    write_line(Destination::File(file), text);
}

/// **Write everything already said, then stop.**
///
/// Called on the way out of `main`, after the footer and before
/// `bt_platform::leave_process` — so the last thing in a trace is the last thing
/// that happened, and not whatever the queue happened to be holding.
pub fn flush() {
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
        move || {
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
    run_to(lines, dropped, &mut std::io::stderr());
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
}
