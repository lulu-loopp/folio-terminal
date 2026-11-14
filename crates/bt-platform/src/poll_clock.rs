//! **The clock a polled wait reads its deadline on, and the rest between two looks**
//! (0.4.10 plan row 76, T-ONE-POLL-CLOCK-SHAPE): [`PollClock`] and the product's
//! [`WallPollClock`]. One shape for the waits listed under [`PollClock`], each of which asks,
//! rests and asks again until a deadline read on a clock of the caller's; it has no platform gate,
//! so a door that builds on every target takes it from here. Not `launch_clock::LaunchClock`,
//! whose wait is the transport's own wait for the other end, handed to the clock: a poll has no
//! such wait, only a rest. Not the clocks whose rest takes the worker's `WorkerCtx` either
//! (`ProbeClock`, and bt-app's `update_apply::WaitClock` and `TrialClock`): their rest goes
//! through the worker's wait door, another shape (0.4.10 plan row 76, ruling R4).

use std::time::{Duration, Instant};

/// **The clock a polled wait's deadline is set and read on, and the rest between two looks**:
/// the waiter sets its deadline on [`PollClock::now`], reads it there after every look, and rests
/// through [`PollClock::pause`]. [`WallPollClock`] in the product; a test hands a counted clock,
/// whose reading moves only by the pauses it is asked for, so "the deadline came" is a count of
/// the test's, or a pinned clock (one whose deadline never comes) where the awaited thing's own
/// event is the subject — never a race between a real reading and a real child or holder.
///
/// Its users:
///
/// * **A lock wait** (`install_txn::hold_by`, T-STALE-MARK-RACE-ON-A-COUNTED-CLOCK): the bounded
///   wait for a lock somebody else holds asks, then pauses `install_txn::LOCK_POLL` (the last
///   pause cut to what is left), until the deadline. `install_txn::hold_within` is it on
///   [`WallPollClock`]; a caller whose road already reads its waits on a clock of its own (bt-app's
///   `update_apply::WaitClock`, through a small adapter that carries the worker its pause needs)
///   passes that clock instead of writing a lock loop beside this one.
/// * **The macOS identity door's child poll** (`macos_identity`, T-CODESIGN-CHILD-BOUND-ON-A-CLOCK,
///   0.4.9 plan row 151): every `codesign` and `spctl` run's bound is set on [`PollClock::now`]
///   when the child has started and read on it after every look, and the rest between two looks
///   is [`PollClock::pause`]; [`WallPollClock`] through that module's public doors.
/// * **A git child's deadline** (bt-app's `git::run_git_with_input`,
///   T-GIT-CHILD-DEADLINE-ON-A-CLOCK-SEAM): set on [`PollClock::now`] when the child has started,
///   read after every look, with a pause between two looks; [`WallPollClock`] through `ask_git`.
///
/// No `Send` or `Sync` bound: every user reads its clock on the thread that waits, and an
/// implementor may hold what cannot cross a thread: the lock wait's adapter holds a `&WorkerCtx`,
/// and that borrow of a `!Sync` type is why the adapter is neither `Send` nor `Sync`. A user that
/// shares a clock across threads says so where it does.
pub trait PollClock {
    /// The monotonic reading the deadline is set and read on.
    fn now(&self) -> Instant;
    /// One rest between two looks: two asks of a lock somebody else holds, or two looks at a
    /// running child.
    fn pause(&self, pause: Duration);
}

/// **The product's [`PollClock`]**: `Instant::now`, and each pause slept whole
/// (`std::thread::sleep`).
pub struct WallPollClock;

impl PollClock for WallPollClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn pause(&self, pause: Duration) {
        std::thread::sleep(pause);
    }
}
