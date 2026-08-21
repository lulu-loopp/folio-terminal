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
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

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
    /// Open (or re-open) a trace at `path`, **appending**, and write `header`.
    ///
    /// Appending rather than truncating because a reproduction is several runs —
    /// "main monitor, second monitor, back to main" is one story the user tells
    /// across however many launches it takes — and a second launch that erased
    /// the first would take the comparison away. The header line is what keeps
    /// the runs separable, and naming the format in it is what keeps a file
    /// from two different traces readable by whoever opens it.
    pub fn create(path: &Path, header: &str) -> std::io::Result<Self> {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{header}")?;
        file.flush()?;
        Ok(Self {
            file: Mutex::new(file),
            started: Instant::now(),
        })
    }

    /// The trace `env` names, or `None` when it names nothing.
    ///
    /// Set-but-empty is off, for the reason `BT_PERF_TRACE` reads the same way:
    /// `BT_MOUSE_TRACE=` is a shell saying "not this run", and a run that
    /// answered it with a file named the empty string would fail in a way that
    /// looks like the feature is broken.
    fn from_environment(env: &str, header: &str) -> Option<Self> {
        let path = std::env::var_os(env).filter(|value| !value.is_empty())?;
        let path = PathBuf::from(path);
        match Self::create(&path, header) {
            Ok(trace) => Some(trace),
            // Said out loud and then dropped. A trace file that could not be
            // opened is a diagnostic that will not run, which is a thing the
            // person who asked for it has to be told; it is not a reason for the
            // terminal to refuse to start.
            Err(error) => {
                eprintln!(
                    "{env} names {} but it could not be opened for the trace: {error}",
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

    /// This process's trace for this variable, opening it on first ask.
    pub fn get(&'static self) -> Option<&'static Trace> {
        self.trace
            .get_or_init(|| Trace::from_environment(self.env, self.header))
            .as_ref()
    }
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
        let trace = Trace::create(&path, HEADER).expect("open a trace in the scratch directory");
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
            let first = Trace::create(&path, HEADER).expect("open a trace");
            emit(Some(&first), || String::from("run=1"));
        }
        {
            let second = Trace::create(&path, HEADER).expect("re-open the same trace");
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

    /// **Two variables are two files and two clocks**, which is the property that
    /// makes a second trace a binding rather than a fork of the machinery.
    #[test]
    fn two_gates_write_to_two_files() {
        let mouse = scratch("two-mouse");
        let attention = scratch("two-attention");
        let first = Trace::create(&mouse, HEADER).expect("open the first trace");
        let second = Trace::create(&attention, HEADER).expect("open the second trace");
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
