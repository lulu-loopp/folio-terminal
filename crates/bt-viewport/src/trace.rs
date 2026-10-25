//! **Where a trace or `stderr` line goes, for the crates that cannot see the
//! program's writers.**
//!
//! `bt-app` owns the writers this process has — `diagnostics.log`'s and the
//! trace sink's — and the crates below it, the renderer above all, which writes a
//! line on every composed frame, cannot name them: `bt-app` depends on them. So
//! the road is *installed* rather than imported. `bt-app`'s `main` hands
//! [`set_writer`] `diagnostics::stderr_line` before anything else it does, and
//! every [`line`] below it goes to whichever writer owns this run's `stderr`.
//! **A process that installs nothing writes nothing**
//! (T-TRACE-SINK-OFF-THE-CALLING-THREAD): no line is ever written on the thread
//! that made it, so a test binary is silent, and a tool that wants its lines
//! (`bt-replay`) installs a road of its own.
//!
//! **Nothing here knows what a viewport is**, and this module is in this crate
//! for one reason: `bt-render`, `bt-term` and `bt-viewport` are the three crates
//! under `bt-app` that write trace lines, and this is the only crate all three
//! already depend on. A fourth copy of these fifteen lines — the shape
//! `switched_on` already has in three crates — is worse than a module in a
//! slightly odd place, because four copies is four chances for one of them to
//! keep writing straight to `stderr` after the fault this exists for was fixed.

use std::sync::OnceLock;

/// **`BT_PERF_TRACE`, as the program's entry read it** (T-DIAGNOSTIC-DOORS-AT-THE-ENTRY).
///
/// The three crates under `bt-app` that time their own work ask [`perf`]; none of them reads the
/// environment. A library that did honoured the switch in every binary that reached it — a test
/// binary as readily as the program — so a shell left with it set traced every test. The entry
/// reads it once and says so here with [`install_perf`]; a process that says nothing, a test
/// binary above all, writes no perf line from these crates.
static PERF: OnceLock<bool> = OnceLock::new();

/// **Say whether `BT_PERF_TRACE` is on for this process.** Called once, by the program's entry,
/// with its answer; by nothing else. The first answer stands: a second call changes nothing.
pub fn install_perf(on: bool) {
    let _ = PERF.set(on);
}

/// Whether this process's entry said `BT_PERF_TRACE` is on; off when it said nothing.
#[must_use]
pub fn perf() -> bool {
    PERF.get().copied().unwrap_or(false)
}

/// What a trace line is handed to, once somebody has said where lines go.
type Writer = Box<dyn Fn(String) + Send + Sync>;

/// [`set_writer`]'s answer, read by [`line`].
///
/// A `OnceLock` and not a lock a caller could take: this is read on the window
/// thread on every composed frame, and the whole point of the exercise is that
/// the frame path acquires nothing it could be made to wait on.
static WRITER: OnceLock<Writer> = OnceLock::new();

/// **Say where this process's trace lines go.** Called once, from `bt-app`'s
/// `main` (or a tool's).
///
/// A second call is ignored rather than refused: there is one sink per process
/// by construction, and a program that tried to install two would be saying the
/// same thing twice, not two different things.
pub fn set_writer(writer: impl Fn(String) + Send + Sync + 'static) {
    let _ = WRITER.set(Box::new(writer));
}

/// One trace or `stderr` line, handed to the installed writer — or, in a process
/// that installed none, to nobody.
///
/// The `String` is already built by the time this is called, which is deliberate
/// and is the same discipline every gate in this workspace keeps: the *caller*
/// decides whether the variable is on, so an off trace formats nothing at all.
pub fn line(line: String) {
    if let Some(writer) = WRITER.get() {
        writer(line);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// **An installed writer is the one that gets the line.**
    ///
    /// The whole of what the door promises, and the only test that may touch
    /// [`WRITER`] — it is process-wide and can be set once, so a second test
    /// that installed one would be racing this one for a slot only one of them
    /// can have.
    ///
    /// Mutation: have [`line`] print unconditionally and the collector stays
    /// empty. (That no line is written before a writer is installed is the
    /// shape of [`line`] itself: it has no other arm.)
    #[test]
    fn an_installed_writer_takes_the_line() {
        static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
        set_writer(|said| {
            SAID.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(said);
        });
        line("BT_PERF_TRACE frame=1".to_owned());
        line("BT_PERF_TRACE frame=2".to_owned());
        let said = SAID
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(
            said,
            ["BT_PERF_TRACE frame=1", "BT_PERF_TRACE frame=2"],
            "the writer this process installed is where its trace lines go"
        );
    }
}
