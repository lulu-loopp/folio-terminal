//! **The process's media-engine ledger**: how many playback engines this
//! process has made and how many it has given back, on every platform, with
//! one owner for both arms (`video::engine` on Windows, the AVFoundation player
//! on a Mac, and the arm with neither, where both counts stay zero).
//!
//! **The ledger is a signal as well as a number.** Every movement is made
//! under the ledger's lock and announced on its condition variable, so a test
//! that needs "three engines exist" waits for the engine threads to say so
//! rather than for a clock to run out: an engine is counted on its own thread,
//! after the open that asked for it has returned, and a machine under load
//! only moves that moment later. See [`outstanding_reaching`].

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
#[cfg(any(test, feature = "trust-harness"))]
use std::time::Duration;

/// The two halves of the count, read and written together under one lock.
#[derive(Clone, Copy)]
struct Counts {
    started: u64,
    shut_down: u64,
}

impl Counts {
    fn outstanding(self) -> u64 {
        self.started.saturating_sub(self.shut_down)
    }
}

/// The counts, and the condition variable every movement of them is
/// announced on.
struct Ledger {
    counts: Mutex<Counts>,
    moved: Condvar,
}

static LEDGER: Ledger = Ledger {
    counts: Mutex::new(Counts {
        started: 0,
        shut_down: 0,
    }),
    moved: Condvar::new(),
};

/// The counts. A poisoned lock means nothing here: what it guards is two
/// integers that are only ever added to, so the value inside is the truth.
fn counts() -> MutexGuard<'static, Counts> {
    LEDGER.counts.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Move the counts and tell every waiter.
#[cfg(any(windows, target_os = "macos"))]
fn move_counts(change: impl FnOnce(&mut Counts)) {
    change(&mut counts());
    LEDGER.moved.notify_all();
}

/// How many engines this process has created.
pub(crate) fn started() -> u64 {
    counts().started
}

/// How many engines this process has shut down.
pub(crate) fn shut_down() -> u64 {
    counts().shut_down
}

/// How many engines are alive right now.
pub(crate) fn outstanding() -> u64 {
    counts().outstanding()
}

/// One engine has been given back — called by the engine's own thread, after
/// the platform's `Shutdown`, by whatever owned the [`LedgerEntry`] it was
/// handed.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn note_engine_shut_down() {
    move_counts(|counts| counts.shut_down += 1);
}

/// **Wait until [`outstanding`] is `target`, woken by the ledger's own
/// movements**, and answer the count the wait ended on.
///
/// The wait ends the moment an engine thread moves the count onto `target`,
/// so a machine that is quick pays nothing and a machine that is slow is
/// waited for; `patience` is how long the caller waits for something that must
/// happen, and running out of it is a red, never a pass — the answer is then
/// the count that stands, which the caller compares with its `target`.
///
/// Tests only: this crate's, and those of a crate that names the
/// `trust-harness` feature on its dev-dependency (`bt-app`'s). The product
/// never waits on the ledger.
#[cfg(any(test, feature = "trust-harness"))]
pub(crate) fn outstanding_reaching(target: u64, patience: Duration) -> u64 {
    let (counts, _) = LEDGER
        .moved
        .wait_timeout_while(counts(), patience, |counts| counts.outstanding() != target)
        .unwrap_or_else(PoisonError::into_inner);
    counts.outstanding()
}

/// **One engine's place on the ledger, opened where the engine comes into
/// being and closed by whoever ends up owning it** (review row R2-19).
///
/// The ledger's whole promise is that [`outstanding`] is zero at every moment
/// no engine is alive, and a bare increment cannot keep it: everything between
/// the call that makes an engine and the machinery that will one day stop it
/// is fallible, and a failure there would add a count nothing would ever take
/// off. So the entry is a value. [`Self::kept`] hands it to the machinery —
/// from there the machinery's stop closes it, through
/// [`note_engine_shut_down`] — and dropping it any other way closes it here,
/// including on an unwind.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) struct LedgerEntry {
    kept: bool,
}

#[cfg(any(windows, target_os = "macos"))]
impl LedgerEntry {
    /// An engine exists. Counted from here.
    pub(crate) fn opened() -> Self {
        move_counts(|counts| counts.started += 1);
        Self { kept: false }
    }

    /// The engine reached the machinery, which is what will shut it down.
    pub(crate) fn kept(mut self) {
        self.kept = true;
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl Drop for LedgerEntry {
    fn drop(&mut self) {
        if !self.kept {
            note_engine_shut_down();
        }
    }
}
