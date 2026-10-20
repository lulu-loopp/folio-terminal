//! **A value dropped without its retire door** (T-DROP-DOORS, the closed `Drop` inventory's
//! ticket; `docs/plans/structural-debt.md` D-40, D-78…D-82).
//!
//! Every value that owns a thread, a handle or a child whose ending is a wait is taken apart
//! through an explicit door its owner calls — `retire_within`, `close_within`, a flush — and the
//! door is the only road that waits. Its `Drop` keeps only what needs no wait. A drop that finds
//! the door was never called does **not** wait in its place: it does the part that needs no wait,
//! leaves the rest to the end of the process, and says so here, once per value.
//!
//! The line goes to the application's diagnostics once the application has installed its road
//! ([`install`]), and to standard error before that (a test, a door process). Every saying is
//! also counted on the thread that dropped the value ([`skipped_on_this_thread`]), so a test that
//! drops a value on a thread of its own can read, on that thread, that the drop said its line
//! exactly once — and, by the drop having returned at all, that it did not wait.

use std::cell::Cell;
use std::sync::OnceLock;

/// Where a skipped door is said: `bt-app`'s `diagnostics::note`, once `main` installs it.
static SAY: OnceLock<fn(&str)> = OnceLock::new();

thread_local! {
    /// The drops on this thread that found their door uncalled.
    static SKIPPED: Cell<u64> = const { Cell::new(0) };
}

/// **Where the line goes from here on**: the application's diagnostics. Installed once, from
/// `main`, before anything that owns one of these values exists; a second install is ignored.
pub fn install(say: fn(&str)) {
    let _ = SAY.set(say);
}

/// **Say once that `what` was dropped without `door`, and what that leaves behind.** Never a
/// wait: one formatted line handed to the installed road (or standard error), and a count.
pub fn say(what: &str, door: &str, left: &str) {
    // `try_with`: a value dropped while its thread's locals are being torn down is still said.
    let _ = SKIPPED.try_with(|skipped| skipped.set(skipped.get() + 1));
    let line = format!("{what} was dropped without {door}, and did not wait: {left}");
    match SAY.get() {
        Some(say) => say(&line),
        None => eprintln!("{line}"),
    }
}

/// How many values dropped on this thread found their door uncalled, since the thread began.
#[must_use]
pub fn skipped_on_this_thread() -> u64 {
    SKIPPED.try_with(Cell::get).unwrap_or(0)
}

/// **What a door test waits on, and for how long** — the suites' patience for a movement that must
/// come: a drop's thread ending, a door returning. A wait that ends on its message costs nothing
/// when the machine is quick; running out is the test's red, by name.
#[cfg(test)]
pub(crate) const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);

/// One message, within [`PATIENCE`], or the test that awaited `what` fails by name.
#[cfg(test)]
pub(crate) fn received<T>(messages: &std::sync::mpsc::Receiver<T>, what: &str) -> T {
    match messages.recv_timeout(PATIENCE) {
        Ok(message) => message,
        Err(_) => panic!("nothing within the door suites' patience while awaiting {what}"),
    }
}

/// **Drop `value` on a thread of its own and answer how many skipped doors that thread said** —
/// within [`PATIENCE`], so a drop that waits for what the test is holding fails the test by name
/// rather than hanging it.
#[cfg(test)]
pub(crate) fn dropped_alone<V: Send + 'static>(value: V, what: &str) -> u64 {
    let (said, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(value);
        let _ = said.send(skipped_on_this_thread());
    });
    received(&heard, what)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **a saying is counted on the thread that said it, and only there.**
    ///
    /// MUTATION: drop the count from `say` and the first assertion fails; count into a process-wide
    /// atomic instead and the second thread's zero is not zero whenever another test says a line.
    #[test]
    fn a_saying_is_counted_on_its_own_thread() {
        std::thread::spawn(|| {
            assert_eq!(skipped_on_this_thread(), 0);
            say("a test's value", "its door", "nothing");
            assert_eq!(skipped_on_this_thread(), 1, "one saying, one count");
            std::thread::spawn(|| assert_eq!(skipped_on_this_thread(), 0))
                .join()
                .expect("the other thread's count");
        })
        .join()
        .expect("the saying thread");
    }
}
