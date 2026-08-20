//! **`BT_MOUSE_TRACE` — one named file, one line per station on a click's road.**
//!
//! A click on a hyperlink in a terminal pane travels through six surfaces before
//! anything opens: the window router, the chrome's own hit test, the mouse route
//! a press arms, the release that spends what the press promised, the activation
//! table, and the landing rule that has to mint a pane for the file to arrive in.
//! Every one of those can decline, and — until this file existed — every one of
//! them declined **silently**. A user on a second monitor reporting "the click
//! does nothing" was reporting the absence of six different possible sentences,
//! and no build could tell which one was missing.
//!
//! So this is forensic apparatus and nothing else: **it changes no behaviour**.
//! Every station writes what it decided and why, and the file is the transcript
//! of a gesture that can be carried back from a machine we do not have.
//!
//! **Named like [`BT_PTY_DUMP`](bt_pty) — the value is a *file*, not a folder.**
//! Handing a directory to that one reports Access denied dressed up as a ConPTY
//! failure; this one says so plainly on stderr and then stays off, because a
//! diagnostic that takes the program down with it is worse than the silence it
//! was built to end.
//!
//! **Off costs one atomic load.** The environment is read once, at the first
//! station any gesture reaches, and the answer is an `Option` from then on. Every
//! call site hands in a closure rather than a `String`, so an unset gate never
//! formats a single field — which is what makes it honest to leave these calls on
//! the pointer-move path.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// The variable that names the file. Set-but-empty is off, for the reason
/// `BT_PERF_TRACE` reads the same way: `BT_MOUSE_TRACE=` is a shell saying "not
/// this run", and a run that answered it with a file named the empty string
/// would fail in a way that looks like the feature is broken.
const TRACE_ENV: &str = "BT_MOUSE_TRACE";

/// The first line of every opened trace, so a file that has collected several
/// runs can still be told what it is and where each run began.
const TRACE_HEADER: &str = "# BT_MOUSE_TRACE_V1 elapsed_ms event field=value…";

/// One opened trace file, and the clock its timestamps are measured from.
///
/// The clock is [`Instant`] rather than a wall time: what a reader of this file
/// needs is the *distance* between two stations of one gesture, and a monotonic
/// millisecond is the only number that means the same thing on both sides of a
/// clock adjustment.
pub struct Trace {
    file: Mutex<File>,
    started: Instant,
}

impl Trace {
    /// Open (or re-open) a trace at `path`, **appending**.
    ///
    /// Appending rather than truncating because a reproduction is several runs —
    /// "main monitor, second monitor, back to main" is one story the user tells
    /// across however many launches it takes — and a second launch that erased
    /// the first would take the comparison away. The header line is what keeps
    /// the runs separable.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{TRACE_HEADER}")?;
        file.flush()?;
        Ok(Self {
            file: Mutex::new(file),
            started: Instant::now(),
        })
    }

    /// The trace this process was launched with, or `None`.
    fn from_environment() -> Option<Self> {
        let path = std::env::var_os(TRACE_ENV).filter(|value| !value.is_empty())?;
        let path = PathBuf::from(path);
        match Self::create(&path) {
            Ok(trace) => Some(trace),
            // Said out loud and then dropped. A trace file that could not be
            // opened is a diagnostic that will not run, which is a thing the
            // person who asked for it has to be told; it is not a reason for the
            // terminal to refuse to start.
            Err(error) => {
                eprintln!(
                    "{TRACE_ENV} names {} but it could not be opened for the trace: {error}",
                    path.display()
                );
                None
            }
        }
    }

    /// One line, timestamped and flushed.
    ///
    /// Flushed per line on purpose: the failure this apparatus exists for may end
    /// in a crash, and a buffered last line is the one line that would have said
    /// which station it crashed at.
    fn write(&self, message: &str) {
        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        // A poisoned lock means some other thread panicked mid-line. The bytes
        // are still a file and this line is still worth having, so the guard is
        // taken back rather than propagated: a diagnostic must not be the thing
        // that turns one panic into two.
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let _ = writeln!(file, "{elapsed:9.3} {message}");
        let _ = file.flush();
    }
}

static TRACE: OnceLock<Option<Trace>> = OnceLock::new();

/// The process's trace, opening it on first ask.
pub fn global() -> Option<&'static Trace> {
    TRACE.get_or_init(Trace::from_environment).as_ref()
}

/// Whether anything is listening — for the handful of call sites that must
/// *compute* a field (a hit test, say) rather than merely format one.
pub fn is_on() -> bool {
    global().is_some()
}

/// Write one line to a named trace, formatting nothing when there is none.
///
/// The gate takes the trace rather than reaching for the global so that it is
/// testable without touching the environment of a running test binary — setting
/// a process-wide variable is `unsafe` in this edition and would race every other
/// test in the same process besides.
pub fn emit(trace: Option<&Trace>, message: impl FnOnce() -> String) {
    if let Some(trace) = trace {
        trace.write(&message());
    }
}

/// [`emit`] against the process's own trace — what every station calls.
pub fn line(message: impl FnOnce() -> String) {
    emit(global(), message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("bt-mouse-trace-{}-{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn body(path: &Path) -> String {
        std::fs::read_to_string(path).expect("the trace file was created")
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
            "an unset BT_MOUSE_TRACE must not format its line"
        );
    }

    /// One call, one line — and the timestamp and the fields on the same one.
    #[test]
    fn an_open_gate_writes_one_line_per_call() {
        let path = scratch("one-line");
        let trace = Trace::create(&path).expect("open a trace in the scratch directory");
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
        assert_eq!(lines[0], TRACE_HEADER);
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
            let first = Trace::create(&path).expect("open a trace");
            emit(Some(&first), || String::from("run=1"));
        }
        {
            let second = Trace::create(&path).expect("re-open the same trace");
            emit(Some(&second), || String::from("run=2"));
        }
        let written = body(&path);
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 4, "two headers and two lines, got {written:?}");
        assert!(lines[1].ends_with("run=1"));
        assert_eq!(lines[2], TRACE_HEADER);
        assert!(lines[3].ends_with("run=2"));
        let _ = std::fs::remove_file(&path);
    }
}
