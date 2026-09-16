//! **Where a trace line goes, for the crates that cannot see the program's
//! trace sink.**
//!
//! `bt_app::trace_sink` owns the one writer thread this process has, and the
//! crates below `bt-app` — the renderer above all, which writes a line on every
//! composed frame — cannot name it: `bt-app` depends on them. So the sink is
//! *installed* rather than imported. `bt-app` hands [`set_writer`] a closure at
//! startup and every [`line`] below it goes through the queue; a run that
//! installs nothing prints on the spot, which is what a test binary and
//! `bt-replay` do and is exactly what these call sites did before.
//!
//! **Nothing here knows what a viewport is**, and this module is in this crate
//! for one reason: `bt-render`, `bt-term` and `bt-viewport` are the three crates
//! under `bt-app` that write trace lines, and this is the only crate all three
//! already depend on. A fourth copy of these fifteen lines — the shape
//! `switched_on` already has in three crates — is worse than a module in a
//! slightly odd place, because four copies is four chances for one of them to
//! keep writing straight to `stderr` after the fault this exists for was fixed.

use std::sync::OnceLock;

/// What a trace line is handed to, once somebody has said where lines go.
type Writer = Box<dyn Fn(String) + Send + Sync>;

/// [`set_writer`]'s answer, read by [`line`].
///
/// A `OnceLock` and not a lock a caller could take: this is read on the window
/// thread on every composed frame, and the whole point of the exercise is that
/// the frame path acquires nothing it could be made to wait on.
static WRITER: OnceLock<Writer> = OnceLock::new();

/// **Say where this process's trace lines go.** Called once, from `bt-app`.
///
/// A second call is ignored rather than refused: there is one sink per process
/// by construction, and a program that tried to install two would be saying the
/// same thing twice, not two different things.
pub fn set_writer(writer: impl Fn(String) + Send + Sync + 'static) {
    let _ = WRITER.set(Box::new(writer));
}

/// One trace line, handed to the installed writer or printed here.
///
/// The `String` is already built by the time this is called, which is deliberate
/// and is the same discipline every gate in this workspace keeps: the *caller*
/// decides whether the variable is on, so an off trace formats nothing at all.
pub fn line(line: String) {
    match WRITER.get() {
        Some(writer) => writer(line),
        // No sink: this is a test, or `bt-replay`, or a build of a crate
        // somebody is running on its own. Straight to `stderr`, the way every
        // one of these lines was written before there was a queue.
        None => eprintln!("{line}"),
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
    /// empty.
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
