//! One bounded queue and writer for performance, resize, and named file traces.
//!
//! A symbolized hang report found the window thread blocked for 5.9 seconds in
//! `ZwWriteFile` under `_eprint` while reporting a renderer frame. Producers now
//! format and timestamp their lines, then try to enqueue them without waiting.
//! Overflow and lock contention drop lines; the writer reports their count.
//!
//! **Its destinations are the ones a reader can stall: the console a trace run
//! kept, and the named trace files** (T-TRACE-SINK-OFF-THE-CALLING-THREAD). Only
//! a run that kept its console, or whose entry named a trace file, starts the
//! writer; every file open, header and write happens on that thread, and no
//! other thread writes a trace file at all — a line offered where there is no
//! sink is not written. Shutdown drains under a one-second deadline, because a
//! writer stalled in the kernel cannot be joined indefinitely.
//!
//! `diagnostics.log` is outside this queue: it has a writer of its own —
//! [`crate::diagnostics::note`], and in a run whose `stderr` is that file every
//! `stderr` line ([`crate::diagnostics::stderr_line`]) — so that neither the
//! watchdog nor a resident UI diagnostic ever waits for whoever is reading a
//! trace. A console run's `stderr` lines, the exit footer among them, share this
//! queue, so stderr contention cannot prevent shutdown from reaching the bounded
//! flush.
//!
//! **Nothing but the writer thread ever waits on this sink** (X-7). Producers
//! `try_lock` and `try_send`, and the writer puts its batch on this process's
//! standard error *without* Rust's shared `Stderr` lock
//! ([`bt_platform::write_std_error`]): a thread stuck for seconds inside one
//! `WriteFile` must not also be holding the mutex every `eprintln!` in the
//! workspace goes through, which would reinstate the very hang one layer out.
//! That includes the error path: a trace file that cannot be opened, or whose
//! header cannot be written, is reported through the same writer
//! ([`TraceFile`]'s open takes it as an argument), once, and never re-queued.

use std::fmt::Write as _;
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
    /// The console a trace run kept (`crate::diagnostics::Channel::Console`).
    /// A run whose `stderr` is `diagnostics.log` hands its lines to that file's
    /// own writer instead (`crate::diagnostics::stderr_line`).
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
pub(crate) struct Queue {
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

    /// One line for a named trace's file. Never waits; a line that does not fit
    /// is counted.
    /// One line for the console a trace run kept, on this queue (see [`offer_stderr_line`]).
    pub(crate) fn offer_stderr_line(&self, text: String) -> bool {
        self.offer(Line {
            destination: Destination::Stderr,
            text,
        })
    }

    pub(crate) fn offer_file_line(&self, file: Arc<TraceFile>, text: String) {
        self.offer(Line {
            destination: Destination::File(file),
            text,
        });
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

/// **Start the writer thread, if this run has a destination for it.**
///
/// Called once, from `main`, after `diagnostics::enter_resident_run` has said
/// whether the run kept its console — and it is **the only thing that opens
/// [`SINK`]**, which is what makes "is there a queue" a fact about the program
/// rather than about the environment a binary happens to inherit. A test binary
/// never calls it: a test that writes a trace drives a queue of its own
/// ([`Capture`]), and nothing in a test has to wait for a thread it did not start.
///
/// **The writer is retired through its admitted flush door, never by a drop**
/// (T-DROP-DOORS, D-78): `main` calls [`Shutdown::flush_through_its_door`] on
/// each of its two ways out — the tail, and the early return from a loop that
/// could not be built — and that is the only road on which the trace is waited
/// for. The value's `Drop` waits for nothing: one that finds the door was never
/// called says so once and leaves what is still queued to the process's end.
#[must_use = "the trace writer is retired through `Shutdown::flush_through_its_door`, never by a drop"]
pub struct Shutdown {
    /// Whether [`Shutdown::flush_through_its_door`] has been asked — admitted or refused.
    retired: bool,
}

impl Shutdown {
    /// **The door: the trace's one bounded flush, admitted on the way out** (`doors::TraceFlush`,
    /// §5.3 row 17). Consumes the writer's token, so no road can flush twice. A refusal — a
    /// thread that is not the window thread, or one not yet `Exiting` — loses what is still
    /// queued, as the flush's own timeout does, and is counted by the admission, which is where
    /// it is said.
    pub fn flush_through_its_door(mut self) -> Result<(), bt_platform::admission::Refused> {
        self.retired = true;
        admitted::<doors::TraceFlush, _>(flush)
    }
}

impl Drop for Shutdown {
    /// Waits for nothing. A writer whose door was never asked says so once; what is still queued
    /// goes with the process.
    fn drop(&mut self) {
        if !self.retired {
            bt_platform::door_skipped::say(
                "the trace writer (`trace_sink::Shutdown`)",
                "its flush door",
                "what is still queued is left to the end of the process",
            );
        }
    }
}

pub fn start(console: bool) -> Shutdown {
    let _ = SINK.get_or_init(|| open(console));
    Shutdown { retired: false }
}

/// **One line for the console a trace run kept, if this run has a queue, and
/// nothing at all if it does not** (X-7).
///
/// The console's half of [`crate::diagnostics::stderr_line`] and of
/// [`crate::diagnostics::note`]: neither the calling thread nor anybody else
/// writes the line here, so this queues or gives up.
///
/// Answers whether the line was taken. `false` is a run with no sink, a full
/// queue or a contended sender — all of which mean the same thing to the
/// caller, which has already said the line where it really belongs.
pub fn offer_stderr_line(text: String) -> bool {
    let Some(sink) = sink() else {
        return false;
    };
    sink.queue.offer_stderr_line(text)
}

/// One line for a named trace's file, queued for the sink's writer — or, in a
/// run with no sink, not written at all: a trace file is written by that thread
/// and by no other.
pub fn file_line(file: Arc<TraceFile>, text: String) {
    if let Some(queue) = queue() {
        queue.offer_file_line(file, text);
    }
}

/// This run's queue, once [`start`] has opened one.
pub(crate) fn queue() -> Option<&'static Queue> {
    sink().map(|sink| &sink.queue)
}

/// **Write everything already said, then stop.**
///
/// Called on the way out of `main`, after the footer and before
/// `bt_platform::leave_process` — so the last thing in a trace is the last thing
/// that happened, and not whatever the queue happened to be holding.
///
/// **An owner-thread door** (`doors::TraceFlush`, §5.3 row 17): a bounded wait, admitted only on
/// the way out, minted in [`Shutdown::flush_through_its_door`] — `main`'s two ways out.
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

/// This run's sink, or `None` in a run that has not [`start`]ed one.
fn sink() -> Option<&'static Sink> {
    SINK.get().and_then(Option::as_ref)
}

fn open(console: bool) -> Option<Sink> {
    if !a_destination_is_kept(console, crate::trace::installed_files()) {
        return None;
    }
    let reports = the_reports_road(console);
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
            run(&waiting, &counted, reports);
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
fn run(lines: &Receiver<Line>, dropped: &AtomicU64, reports: Reports) {
    match reports {
        Reports::Console => run_to(lines, dropped, &mut ProcessStderr),
        Reports::TheLogsRoad => run_to(
            lines,
            dropped,
            &mut LineByLine::to(crate::diagnostics::stderr_line),
        ),
    }
}

/// **Where the writer's own reports go** — the count of the lines it lost, a trace file it could
/// not open (T-TRACE-SINK-OFF-THE-CALLING-THREAD, review F1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reports {
    /// A console run: its `stderr` is the console, which the writer already writes.
    Console,
    /// Any other run: `stderr` is `diagnostics.log` (or nothing), whose one writer is
    /// `diagnostics`' — so each report is a line handed to [`crate::diagnostics::stderr_line`],
    /// queued with the run's notes and covered by the way out's fence, never a second writer of
    /// that file.
    TheLogsRoad,
}

fn the_reports_road(console: bool) -> Reports {
    if console {
        Reports::Console
    } else {
        Reports::TheLogsRoad
    }
}

/// **The writer's reports, one whole line at a time, handed to a road** — for a run whose
/// `stderr` is not the sink's to write. `writeln!` reaches a writer in pieces, so a line is handed
/// over at its end and not before.
struct LineByLine {
    say: fn(String),
    pending: Vec<u8>,
}

impl LineByLine {
    fn to(say: fn(String)) -> Self {
        Self {
            say,
            pending: Vec::new(),
        }
    }
}

impl std::io::Write for LineByLine {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            (self.say)(String::from_utf8_lossy(&line[..end]).into_owned());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
pub(crate) struct ProcessStderr;

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
            take(line, &mut batch, stderr);
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

/// One line, where it is going: a console line into the batch, a file line
/// after the batch before it has gone out — so the order the lines were offered
/// in is the order they come out in.
fn take(line: Line, batch: &mut String, stderr: &mut impl std::io::Write) {
    let Line { destination, text } = line;
    match destination {
        Destination::Stderr => {
            batch.push_str(&text);
            batch.push('\n');
        }
        Destination::File(file) => {
            put(batch, stderr);
            file.append(&text, stderr);
        }
    }
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

/// **Whether this run has a destination for the sink**: the console a trace run
/// kept, or any trace file the entry named ([`crate::trace::TraceFiles`]).
///
/// Read from the two facts the destinations are, and not from the shape of a
/// variable's name: `BT_GLYPH_CENSUS` names a file and mentions no `TRACE`, and
/// a gate on the name left that file opened and appended on the window thread,
/// every frame. Other dumps, such as the PTY byte capture, have their own
/// writers and need no idle sink thread.
fn a_destination_is_kept(console: bool, files: Option<&crate::trace::TraceFiles>) -> bool {
    console || files.is_some_and(crate::trace::TraceFiles::names_any)
}

/// **A queue a test writes through, and the writer's own body to put it out
/// with** — the way a test reads a trace now that no thread but the writer
/// writes one: the lines a station offers wait in the queue until
/// [`Capture::write_out`] hands them, in order, to [`take`], exactly as the
/// sink's thread does.
#[cfg(test)]
pub(crate) struct Capture {
    queue: &'static Queue,
    waiting: Receiver<Line>,
}

#[cfg(test)]
impl Capture {
    pub(crate) fn new() -> Self {
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        // Leaked: a trace holds its queue for as long as the process, as the sink's does.
        let queue = Box::leak(Box::new(Queue::new(lines, Arc::new(AtomicU64::new(0)))));
        Self { queue, waiting }
    }

    /// The queue a test's [`crate::trace::Trace`] is created on.
    pub(crate) fn queue(&self) -> &'static Queue {
        self.queue
    }

    /// The console lines offered so far, taken off the queue in order (a file line is put out).
    pub(crate) fn stderr_lines(&self) -> Vec<String> {
        let mut said = Vec::new();
        let mut batch = String::new();
        while let Ok(line) = self.waiting.try_recv() {
            match line.destination {
                Destination::Stderr => said.push(line.text),
                Destination::File(_) => take(line, &mut batch, &mut ProcessStderr),
            }
        }
        said
    }

    /// Everything offered so far, written where it was going.
    pub(crate) fn write_out(&self) {
        let mut batch = String::new();
        while let Ok(line) = self.waiting.try_recv() {
            take(line, &mut batch, &mut ProcessStderr);
        }
        put(&mut batch, &mut ProcessStderr);
    }
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
        // A writer that never leaves its write is this helper's red, said by name; while a test
        // is already failing, its own panic is the red and the writer is left where it stands.
        if !crate::lane::within_patience(|| self.queue.close()) {
            assert!(
                std::thread::panicking(),
                "the stalled writer was released and never left its write within the lane suite's patience"
            );
            return;
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
        let path = std::env::temp_dir().join(format!(
            "{}.log",
            bt_testpath::unique_name(&format!("bt-sink-{name}"))
        ));
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
        let writer = std::thread::spawn(move || run(&waiting, &counted, Reports::Console));
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
            run(&waiting, &counted, Reports::Console);
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

    /// RED (B-TRACE-STDERR) — **a trace file that cannot be opened is reported
    /// by the sink's own writer, once, without waiting for the lock every other
    /// writer of `stderr` shares.**
    ///
    /// The error road of the test above. A directory given as the trace file
    /// fails to open on every platform. First the production body — [`run`],
    /// with the real [`ProcessStderr`] — while another thread holds Rust's
    /// `Stderr` lock: the writer has to reach the end of its body inside the
    /// shutdown bound, which it cannot if the report takes that lock. Then the
    /// same body with a writer the test can read: the report arrives there,
    /// exactly once for two lines (the failure is remembered), and nothing is
    /// written to the directory's name.
    ///
    /// MUTATION: in `TraceFile::open`, write the report to `std::io::stderr()`
    /// instead of `report`; the writer waits for the holder past
    /// [`FLUSH_TIMEOUT`], and the captured output holds no report.
    #[test]
    fn a_failed_trace_open_reports_without_the_process_stderr_lock() {
        let folder = bt_testpath::temp_path("bt-sink-a-folder-not-a-file");
        std::fs::create_dir_all(&folder).expect("the scratch folder");
        let offer_two = |queue: &Queue, file: &Arc<TraceFile>| {
            for text in ["first", "second"] {
                assert!(queue.offer(Line {
                    destination: Destination::File(Arc::clone(file)),
                    text: text.into(),
                }));
            }
        };

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
        let failing = Arc::new(TraceFile::new(&folder, Some("# header")));
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        let (done, finished) = sync_channel::<()>(1);
        let writer = std::thread::spawn(move || {
            run(&waiting, &counted, Reports::Console);
            let _ = done.send(());
        });
        let queue = Queue::new(lines, dropped);
        offer_two(&queue, &failing);
        assert!(queue.close());
        let ended = finished.recv_timeout(FLUSH_TIMEOUT);
        drop(release);
        holder.join().unwrap();
        writer.join().unwrap();
        assert!(
            ended.is_ok(),
            "the failed open's report waited for a lock another thread was holding"
        );

        let failing = Arc::new(TraceFile::new(&folder, Some("# header")));
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let queue = Queue::new(lines, Arc::new(AtomicU64::new(0)));
        offer_two(&queue, &failing);
        assert!(queue.close());
        let mut output = Vec::new();
        run_to(&waiting, &queue.dropped, &mut output);
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output.matches("could not be opened for the trace").count(),
            1,
            "one report on the sink's writer for a failure that is remembered: {output:?}"
        );
        assert!(
            output.contains(&folder.display().to_string()),
            "the report names the trace: {output:?}"
        );
        assert!(folder.is_dir(), "the folder is still a folder");
        let _ = std::fs::remove_dir(&folder);
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

    /// RED (T-TRACE-SINK-OFF-THE-CALLING-THREAD; mutation: the old gate on a
    /// variable's name, `BT_` + `TRACE` or `BT_FOCUS_THUMB_DUMP`) — **a run whose
    /// only trace is a file starts the sink**, whatever the file's variable is
    /// called: `BT_GLYPH_CENSUS` alone left its file written on the window
    /// thread. A console run starts it with no file; a run with neither does not.
    #[test]
    fn a_run_with_a_trace_file_or_a_console_starts_the_sink() {
        let census = crate::trace::TraceFiles {
            glyph_census: Some(PathBuf::from("census.log")),
            ..crate::trace::TraceFiles::default()
        };
        assert!(a_destination_is_kept(false, Some(&census)));
        assert!(a_destination_is_kept(true, None));
        assert!(a_destination_is_kept(
            true,
            Some(&crate::trace::TraceFiles::default())
        ));
        assert!(!a_destination_is_kept(false, None));
        assert!(!a_destination_is_kept(
            false,
            Some(&crate::trace::TraceFiles::default())
        ));
    }

    /// RED (review F1; mutation: [`the_reports_road`] answering `Console` for a run without one,
    /// which puts the reports through `write_std_error` into `diagnostics.log` beside its writer,
    /// or [`LineByLine`] handing over a piece of a line) — **in a run without a console the sink's
    /// own reports are lines on the log's road**: a trace file it could not open and the count of
    /// the lines it lost each reach the road whole, as one line, from the writer's body — so in a
    /// Log run (`BT_GLYPH_CENSUS` or `BT_FOCUS_THUMB_DUMP` alone) they are queued with the run's
    /// notes, under the way out's fence, and the file keeps one writer.
    #[test]
    fn a_run_without_a_console_hands_the_writers_reports_to_the_logs_road() {
        static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
        fn road(line: String) {
            SAID.lock().unwrap().push(line);
        }
        assert_eq!(the_reports_road(false), Reports::TheLogsRoad);
        assert_eq!(the_reports_road(true), Reports::Console);
        let folder = bt_testpath::temp_path("bt-sink-reports-a-folder-not-a-file");
        std::fs::create_dir_all(&folder).unwrap();
        let failing = Arc::new(TraceFile::new(&folder, Some("# header")));
        let (lines, waiting) = sync_channel(QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(3));
        let queue = Queue::new(lines, Arc::clone(&dropped));
        queue.offer_file_line(failing, "a census line".to_owned());
        drop(queue);
        run_to(&waiting, &dropped, &mut LineByLine::to(road));
        let said = SAID.lock().unwrap().clone();
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said[0].starts_with(&folder.display().to_string())
                && said[0].contains("could not be opened for the trace"),
            "{said:?}"
        );
        assert_eq!(said[1], "BT_PERF_TRACE dropped=3");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// RED (mutation: [`file_line`] writing the line where it is made when no
    /// sink runs, the arm it had) — **a trace file is written by the sink's
    /// writer and by nobody else**: a line offered on a queue is not in the file
    /// until the writer's body takes it, and in a process with no sink a line is
    /// not written at all.
    #[test]
    fn a_trace_line_is_queued_and_written_only_by_the_writer() {
        let path = scratch("only-the-writer");
        let file = Arc::new(TraceFile::new(&path, Some("# header")));
        let capture = Capture::new();
        capture
            .queue()
            .offer_file_line(Arc::clone(&file), "queued".to_owned());
        assert!(!path.exists(), "the producer wrote the trace file");
        capture.write_out();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# header\nqueued\n"
        );
        // This test binary started no sink: the global road writes nothing.
        file_line(Arc::clone(&file), "unqueued".to_owned());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# header\nqueued\n"
        );
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    /// RED (A1d, row 17; T-DROP-DOORS, D-78) — **the trace's flush is one admitted `TraceFlush`
    /// on the way out, through the writer's own door, and is not waited for anywhere else.**
    ///
    /// On a test thread entered as the window thread and still running, the door is refused and
    /// nothing is admitted; on the same kind of thread on its way out, it is one admission. Neither
    /// is a drop without the door: the refusal is the admission's to count, and said nowhere else.
    ///
    /// MUTATION: skip the admission in `flush_through_its_door` and the second list is empty;
    /// give the door `Running` and the first is not; leave `retired` unset there and the refused
    /// door's drop says a skipped line.
    #[test]
    fn the_trace_is_flushed_through_its_door_only_on_the_way_out() {
        let (said, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            crate::test_support::on_the_window_thread();
            let refused = Shutdown { retired: false }.flush_through_its_door();
            let running = crate::hang_watch::admissions_on_this_thread();
            assert!(bt_platform::admission::exiting());
            let admitted = Shutdown { retired: false }.flush_through_its_door();
            let _ = said.send((
                refused.is_err(),
                running,
                admitted.is_ok(),
                crate::hang_watch::admissions_on_this_thread(),
                bt_platform::door_skipped::skipped_on_this_thread(),
            ));
        });
        let (refused, running, admitted, exiting, skipped) =
            crate::lane::receive(&heard, "the writer's door on the window thread");
        assert!(refused, "a running window thread's door is refused");
        assert!(
            running.is_empty(),
            "a running window thread does not wait for the trace"
        );
        assert!(admitted, "on the way out, the door is admitted");
        assert_eq!(
            exiting,
            ["TraceFlush"],
            "on the way out, the door's flush is one admission"
        );
        assert_eq!(
            skipped, 0,
            "a door asked, admitted or refused, is no skipped door"
        );
    }

    /// RED (T-DROP-DOORS, D-78) — **a writer dropped without its door waits for nothing and says
    /// so once.**
    ///
    /// On the window thread on its way out — the one place the flush could be admitted — a drop
    /// admits no `TraceFlush` (so nothing on it waited) and says its line once; the thread that
    /// dropped it ends within the lane suite's patience.
    ///
    /// MUTATION: put the old body back in `Drop` (`let _ = admitted::<doors::TraceFlush, _>(flush)`)
    /// and the admissions read `["TraceFlush"]`; drop the saying and the count is zero.
    #[test]
    fn a_writer_dropped_without_its_door_waits_for_nothing_and_says_so_once() {
        let (said, heard) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            crate::test_support::on_the_window_thread();
            assert!(bt_platform::admission::exiting());
            drop(Shutdown { retired: false });
            let _ = said.send((
                crate::hang_watch::admissions_on_this_thread(),
                bt_platform::door_skipped::skipped_on_this_thread(),
            ));
        });
        let (admissions, skipped) = crate::lane::receive(&heard, "the writer's drop");
        assert!(
            admissions.is_empty(),
            "a drop admits no flush, so it waited for nothing: {admissions:?}"
        );
        assert_eq!(skipped, 1, "the drop says its skipped door once");
    }
}
