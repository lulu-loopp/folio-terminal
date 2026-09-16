//! **The apparatus behind every `BT_*_TRACE` file: one named file, one line per
//! station, and nothing at all when the variable is unset.**
//!
//! [`mouse_trace`](crate::mouse_trace) was the first of these and wrote the rules
//! down: a named *file* rather than a folder, appended rather than truncated,
//! flushed per line, and a closure at every call site so an unset gate never
//! formats a field. The second trace ([`attention_trace`](crate::attention_trace))
//! wants all five of those properties and none of the mouse's stations, so the
//! machinery moved here and the two modules above it are now what they always
//! should have been: **a variable name, a header, and a list of stations.**
//!
//! Nothing in this file knows what it is tracing. It is handed a path and hands
//! back timestamped lines.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use crate::trace_sink;

/// **The process's own zero, shared by every named trace.**
///
/// A clock taken at each file's own opening would be a different zero per
/// variable — a gate opens at the first station any code path reaches, and two
/// variables' first stations can be seconds apart — so the same event would
/// carry two different numbers in two files of the same run. That is precisely
/// the reading these files exist to support: the wheel's road is
/// [`BT_MOUSE_TRACE`](crate::mouse_trace) and what it did to a card is
/// [`BT_CARD_TRACE`](crate::card_trace), and a reader answers "which came
/// first" by merging them on the first column. One origin makes that column
/// mean the same thing in every file this process writes.
///
/// It is still per *process*, and that is what the header line is for: a file
/// collecting several runs restarts at zero under each header.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// [`ORIGIN`], set by whichever trace opens first.
fn origin() -> Instant {
    *ORIGIN.get_or_init(Instant::now)
}

/// A destination resolved by the producer, opened and written only by the sink.
/// The header is written with the first accepted line, so dropping a queued
/// line can never leave the file without its header.
pub struct TraceFile {
    path: PathBuf,
    header: Option<String>,
    file: OnceLock<Option<Mutex<File>>>,
}

impl TraceFile {
    pub fn new(path: &Path, header: Option<&str>) -> Self {
        Self {
            path: path.to_owned(),
            header: header.map(str::to_owned),
            file: OnceLock::new(),
        }
    }

    /// Only the writer calls this in a resident run. Tests without a sink keep
    /// their synchronous behavior. Failed opens are remembered, not retried on
    /// every frame, and their diagnostic is emitted on this same writer thread.
    fn open(&self) -> Option<&Mutex<File>> {
        self.file
            .get_or_init(|| {
                let opened = (|| -> std::io::Result<File> {
                    let mut file = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&self.path)?;
                    if let Some(header) = &self.header {
                        writeln!(file, "{header}")?;
                    }
                    Ok(file)
                })();
                match opened {
                    Ok(file) => Some(Mutex::new(file)),
                    Err(error) => {
                        // Ignore stderr failures too: a trace must not panic.
                        let _ = writeln!(
                            std::io::stderr(),
                            "{} could not be opened for the trace: {error}",
                            self.path.display()
                        );
                        None
                    }
                }
            })
            .as_ref()
    }

    pub fn append(&self, line: &str) {
        if let Some(file) = self.open() {
            let mut file = file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

/// One trace destination, and the clock its timestamps are measured from.
///
/// The clock is [`Instant`] rather than a wall time: what a reader of this file
/// needs is the *distance* between two stations of one gesture, and a monotonic
/// millisecond is the only number that means the same thing on both sides of a
/// clock adjustment.
pub struct Trace {
    file: Arc<TraceFile>,
    started: Instant,
}

impl Trace {
    /// Prepare an append-only trace; the writer opens it with the first line.
    ///
    /// Appending rather than truncating because a reproduction is several runs —
    /// "main monitor, second monitor, back to main" is one story the user tells
    /// across however many launches it takes — and a second launch that erased
    /// the first would take the comparison away. The header line is what keeps
    /// the runs separable, and naming the format in it is what keeps a file
    /// from two different traces readable by whoever opens it.
    pub fn create(path: &Path, header: &str) -> Self {
        let file = Arc::new(TraceFile::new(path, Some(header)));
        // Standalone users keep the immediate header, even when no events
        // follow. In a resident run, all file I/O belongs to the sink instead.
        if !trace_sink::started() {
            let _ = file.open();
        }
        Self {
            file,
            started: origin(),
        }
    }

    /// Resolve the path without opening it on the calling thread.
    fn from_environment(env: &str, header: &str) -> Option<Self> {
        let path = std::env::var_os(env).filter(|value| !value.is_empty())?;
        Some(Self::create(&PathBuf::from(path), header))
    }

    /// One line, timestamped here and written by [`trace_sink`]'s thread.
    ///
    /// **The stamp is taken on this thread and not on the writer's**, which is
    /// the whole reason the queue carries text rather than fields: the first
    /// column of these files is what a reader merges two of them on, and a
    /// number taken after a queue would be the time the line was *written* —
    /// a fact about a different thread at a different moment.
    fn write(&self, message: &str) {
        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        trace_sink::file_line(Arc::clone(&self.file), format!("{elapsed:9.3} {message}"));
    }
}

/// **One environment variable, opened at most once.**
///
/// A `static` of this type is the whole of what a named trace is: the variable
/// is read at the first station any code path reaches, and the answer is an
/// `Option` from then on. Off therefore costs one atomic load, which is what
/// makes it honest to leave these calls on a per-frame path.
pub struct Gate {
    env: &'static str,
    header: &'static str,
    trace: OnceLock<Option<Trace>>,
}

impl Gate {
    pub const fn new(env: &'static str, header: &'static str) -> Self {
        Self {
            env,
            header,
            trace: OnceLock::new(),
        }
    }

    /// This process's trace for this variable, resolving its path on first ask.
    pub fn get(&'static self) -> Option<&'static Trace> {
        self.trace
            .get_or_init(|| Trace::from_environment(self.env, self.header))
            .as_ref()
    }
}

/// **A file a diagnostic appends to in its own words** — no header of ours, no
/// timestamp of ours, one line per call.
///
/// [`Gate`]'s sibling, for the two writers that were opening a file per line on
/// the window thread and whose *format* is not this module's to change:
/// `BT_IME_TRACE`, which was doing a whole `CreateFile`/`WriteFile`/`CloseHandle`
/// on the first statement of `ime_input` (1370 of them in the `next68` run), and
/// `BT_FOCUS_THUMB_DUMP`, which was doing the same once per card frame. What
/// they get from this is what [`Gate`] already had — the handle opened once —
/// and what [`trace_sink`] adds to both: the write happens on the sink's thread.
///
/// The bytes are unchanged. Whatever the caller formats is the whole of the
/// line, so a reader's existing tooling reads exactly what it read before.
pub struct Dump {
    env: &'static str,
    file: OnceLock<Option<Arc<TraceFile>>>,
}

impl Dump {
    pub const fn new(env: &'static str) -> Self {
        Self {
            env,
            file: OnceLock::new(),
        }
    }

    /// One line, formatting nothing at all when the variable names no file.
    pub fn line(&self, message: impl FnOnce() -> String) {
        let Some(file) = self.file.get_or_init(|| open_dump(self.env)).as_ref() else {
            return;
        };
        trace_sink::file_line(Arc::clone(file), message());
    }
}

/// The file `env` names, or `None` when it names nothing.
///
/// Set-but-empty is off, on [`Trace::from_environment`]'s rule and for its
/// reason.
fn open_dump(env: &'static str) -> Option<Arc<TraceFile>> {
    let path = std::env::var_os(env).filter(|value| !value.is_empty())?;
    let path = PathBuf::from(path);
    Some(Arc::new(TraceFile::new(&path, None)))
}

/// Write one line to a named trace, formatting nothing when there is none.
///
/// The gate takes the trace rather than reaching for a global so that it is
/// testable without touching the environment of a running test binary — setting
/// a process-wide variable is `unsafe` in this edition and would race every other
/// test in the same process besides.
pub fn emit(trace: Option<&Trace>, message: impl FnOnce() -> String) {
    if let Some(trace) = trace {
        trace.write(&message());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    const HEADER: &str = "# BT_TRACE_TEST_V1 elapsed_ms event field=value…";

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("bt-trace-{}-{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn body(path: &Path) -> String {
        std::fs::read_to_string(path).expect("the trace file was created")
    }

    /// **A [`Dump`]'s bytes are the caller's bytes** — no header, no timestamp,
    /// one newline.
    ///
    /// The property that let `BT_IME_TRACE` and `BT_FOCUS_THUMB_DUMP` move onto
    /// this machinery without anybody's tooling changing: what those two writers
    /// gained was a handle opened once and a write that happens on
    /// [`trace_sink`]'s thread, and what they were not allowed to lose was the
    /// shape of the line they had been writing since the day they were added.
    ///
    /// MUTATION: give [`TraceFile::append`] a timestamp of its own and both
    /// comparisons go red.
    #[test]
    fn a_trace_file_writes_the_line_it_was_given_and_one_newline() {
        let path = scratch("verbatim");
        let file = TraceFile::new(&path, None);
        file.append("Instant { t: 1 } Preedit(\"ni\")");
        file.append("focus-thumb visible=3 projections=1");
        assert_eq!(
            body(&path),
            "Instant { t: 1 } Preedit(\"ni\")\nfocus-thumb visible=3 projections=1\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **The whole of what "zero overhead when off" means**: not that the string
    /// is thrown away, that it is never built.
    #[test]
    fn a_closed_gate_never_calls_its_closure() {
        let called = AtomicBool::new(false);
        emit(None, || {
            called.store(true, Ordering::SeqCst);
            String::from("this must never be formatted")
        });
        assert!(
            !called.load(Ordering::SeqCst),
            "an unset trace variable must not format its line"
        );
    }

    /// One call, one line — and the timestamp and the fields on the same one.
    #[test]
    fn an_open_gate_writes_one_line_per_call() {
        let path = scratch("one-line");
        let trace = Trace::create(&path, HEADER);
        emit(Some(&trace), || String::from("mouse_input state=Pressed"));
        emit(Some(&trace), || {
            String::from("finish_local_selection single_click=1")
        });
        let written = body(&path);
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(
            lines.len(),
            3,
            "a header and one line per call, got {written:?}"
        );
        assert_eq!(lines[0], HEADER);
        assert!(
            lines[1].ends_with("mouse_input state=Pressed"),
            "the event text is written after its timestamp: {:?}",
            lines[1]
        );
        assert!(
            lines[1]
                .split_whitespace()
                .next()
                .expect("a timestamp leads the line")
                .parse::<f64>()
                .is_ok(),
            "the line leads with a monotonic millisecond: {:?}",
            lines[1]
        );
        assert!(lines[2].ends_with("finish_local_selection single_click=1"));
        let _ = std::fs::remove_file(&path);
    }

    /// A second run keeps the first one's evidence. The reproduction this file
    /// exists for is "main monitor, second monitor, back to main", which is more
    /// than one launch of the program.
    #[test]
    fn a_second_run_appends_rather_than_erasing_the_first() {
        let path = scratch("append");
        {
            let first = Trace::create(&path, HEADER);
            emit(Some(&first), || String::from("run=1"));
        }
        {
            let second = Trace::create(&path, HEADER);
            emit(Some(&second), || String::from("run=2"));
        }
        let written = body(&path);
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 4, "two headers and two lines, got {written:?}");
        assert!(lines[1].ends_with("run=1"));
        assert_eq!(lines[2], HEADER);
        assert!(lines[3].ends_with("run=2"));
        let _ = std::fs::remove_file(&path);
    }

    /// **Two variables are two files and ONE clock** (T-CARD-TRACE).
    ///
    /// The merge is the reading these files exist for — the wheel's road is
    /// `BT_MOUSE_TRACE` and what it did to a card is `BT_CARD_TRACE` — and a
    /// merge on the first column is only sound while that column counts from
    /// the same instant in both. Two traces opened a measurable time apart are
    /// therefore stamped from the same origin, so the second one's first line
    /// is *later* than the first one's rather than starting again at zero.
    ///
    /// Mutation: give [`Trace::create`] its own `Instant::now()` back and the
    /// second file's clock restarts, which silently shifts every one of its
    /// lines against the other file's.
    #[test]
    fn two_gates_share_one_clock_so_their_files_merge() {
        let first = scratch("clock-first");
        let second = scratch("clock-second");
        let early = Trace::create(&first, HEADER);
        emit(Some(&early), || String::from("first"));
        let stamp_of = |line: &str| -> f64 {
            line.split_whitespace()
                .next()
                .expect("a timestamp leads the line")
                .parse()
                .expect("and it is a number of milliseconds")
        };
        let opened_at = stamp_of(body(&first).lines().nth(1).expect("the first line"));
        let late = Trace::create(&second, HEADER);
        emit(Some(&late), || String::from("second"));
        let later = stamp_of(
            body(&second)
                .lines()
                .nth(1)
                .expect("the second file's line"),
        );
        assert!(
            later >= opened_at,
            "the second trace restarted the clock: {later} is before {opened_at}"
        );
        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }

    /// **Two variables are two files**, which is the property that makes a
    /// second trace a binding rather than a fork of the machinery.
    #[test]
    fn two_gates_write_to_two_files() {
        let mouse = scratch("two-mouse");
        let attention = scratch("two-attention");
        let first = Trace::create(&mouse, HEADER);
        let second = Trace::create(&attention, HEADER);
        emit(Some(&first), || String::from("pane_press"));
        emit(Some(&second), || String::from("bell"));
        assert!(body(&mouse).contains("pane_press"));
        assert!(!body(&mouse).contains("bell"));
        assert!(body(&attention).contains("bell"));
        assert!(!body(&attention).contains("pane_press"));
        let _ = std::fs::remove_file(&mouse);
        let _ = std::fs::remove_file(&attention);
    }
}
