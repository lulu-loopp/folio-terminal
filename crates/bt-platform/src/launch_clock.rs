//! **The clock a wait for the other end is read on** (0.4.10 plan row 66,
//! T-LAUNCHCLOCK-OUT-OF-THE-PLATFORM-GATED-MODULE): [`LaunchClock`], the product's
//! [`RealClock`] and a test's `CountedClock`. They lived in `launch_conversation`, which is
//! compiled only where the launch wire has a channel (Windows and Unix); the clock has no such
//! gate, and a door that builds on every target — the deferred remover's readiness wait — takes
//! it from here.

use std::time::{Duration, Instant};

/// **The clock the launch wire's waits are read on, and the waits themselves**
/// (T-LAUNCH-HANDOVER-BUDGET-ON-A-CLOCK). A wait of the conversation is a wait for the other end
/// to act, at either end: the second launch's budget ([`crate::launch_pipe::HANDOVER_BUDGET`]:
/// the reply on Windows, every frame on Unix), the server's step deadline, and the Windows
/// listener's first word. Each arm reads its deadline on [`LaunchClock::now`] and hands each of
/// those waits to [`LaunchClock::wait`]; the Windows busy retry's own waits, made inside the
/// client's open (`attention_pipe::open_client`), are each one [`LaunchClock::wait`] there too
/// (T-ATTENTION-ENDPOINT-WAITS-ON-A-CLOCK). The Windows writes are not here:
/// each is `attention_pipe::write_bounded`'s own real 250 ms, which buffering decides, not the
/// peer (noted in `docs/plans/TIMING-BOUND-TESTS.tsv`). [`RealClock`]
/// in the product, through `hand_over` and `LaunchPipe::start`; a test hands a counted clock, so
/// "the other end acted in time" and "the budget ran out" are facts of the test, never a race
/// between a real window and a thread of the same process. WinHTTP's close door
/// (`http::Request::close_within`, Windows) is its second user: one wait, up to `http::CLOSE_WAIT`,
/// for the handle's closing callback.
///
/// The elevated pane's handshake (`crate::elevated_pipe`, Windows) is its third user
/// (T-ELEVATED-HANDSHAKE-FINISH-DEADLINE-ON-A-CLOCK): its attempt's deadline is read on
/// [`LaunchClock::now`] and each overlapped operation's one wait is a [`LaunchClock::wait`].
///
/// The macOS update check (`http::Exchange::wait`, macOS) is its fourth user (0.4.9 plan row 150):
/// one wait, up to the request's `budget + phase_timeout`, for the session delegate's completion.
///
/// The attention endpoint (`crate::attention_pipe`, both platforms) is its fifth user
/// (T-ATTENTION-ENDPOINT-WAITS-ON-A-CLOCK): the Windows listener's first word, each attached
/// caller's read deadline, and the client's one wait past a busy endpoint inside
/// `attention_pipe::open_client`, which the launch wire and the elevated pane's client share. Its
/// writes stay real, for the launch wire's reason above.
///
/// The deferred remover's readiness (`crate::deferred_removal::schedule`, every target) is its
/// sixth user (0.4.10 plan row 66): one wait, up to the caller's `ready_within`, for the copied
/// remover's ready line.
pub(crate) trait LaunchClock: Send + Sync {
    /// The monotonic reading a deadline is set and read on.
    fn now(&self) -> Instant;
    /// Wait up to `bound` for the other end. `ready(d)` makes the transport's own wait of `d` and
    /// answers whether the other end acted inside it; this answers the same.
    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool;
}

/// **The product's [`LaunchClock`]**: the monotonic clock, and each wait made once, as long as it
/// was asked to be.
pub(crate) struct RealClock;

impl LaunchClock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool {
        ready(bound)
    }
}

/// What a [`CountedClock`] does with one wait it is asked for.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Turn {
    /// Wait for the other end within the door suites' patience
    /// ([`crate::door_skipped::PATIENCE`]); a wait the other end does not end inside it is spent
    /// whole, on the count.
    Await,
    /// Do not wait: the whole bound is spent, on the count.
    Spend,
}

/// **A test's [`LaunchClock`]** (T-LAUNCH-HANDOVER-BUDGET-ON-A-CLOCK): its reading moves only by
/// the waits it spends, and `turn` decides, for each wait by its number from one and its bound,
/// whether the other end is awaited — within the suite's patience — or the bound is spent at once.
/// "Answered in time" is the other end's event; "the budget ran out" is the count; a wait of no
/// time at all is spent without being made. It writes down every bound it was asked for.
#[cfg(test)]
pub(crate) struct CountedClock {
    began: Instant,
    /// How far the spent waits moved the clock, and every bound asked for, in order.
    counted: std::sync::Mutex<(Duration, Vec<Duration>)>,
    turn: std::sync::Mutex<Box<dyn FnMut(usize, Duration) -> Turn + Send>>,
}

#[cfg(test)]
impl CountedClock {
    pub(crate) fn new(turn: impl FnMut(usize, Duration) -> Turn + Send + 'static) -> Self {
        Self {
            began: Instant::now(),
            counted: std::sync::Mutex::new((Duration::ZERO, Vec::new())),
            turn: std::sync::Mutex::new(Box::new(turn)),
        }
    }

    /// A clock whose every wait awaits the other end.
    pub(crate) fn answering() -> Self {
        Self::new(|_, _| Turn::Await)
    }

    /// How far the spent waits moved the clock.
    pub(crate) fn moved(&self) -> Duration {
        self.counted.lock().unwrap().0
    }

    /// Every bound a wait was asked for, in order.
    pub(crate) fn asked(&self) -> Vec<Duration> {
        self.counted.lock().unwrap().1.clone()
    }
}

#[cfg(test)]
impl LaunchClock for CountedClock {
    fn now(&self) -> Instant {
        self.began + self.moved()
    }

    fn wait(&self, bound: Duration, ready: &mut dyn FnMut(Duration) -> bool) -> bool {
        if self.turn_for(bound) == Turn::Await
            && !bound.is_zero()
            && ready(crate::door_skipped::PATIENCE)
        {
            return true;
        }
        self.moves_by(bound);
        false
    }
}

#[cfg(test)]
impl CountedClock {
    /// Write `bound` down, and ask `turn` for this wait.
    fn turn_for(&self, bound: Duration) -> Turn {
        let number = {
            let mut counted = self.counted.lock().unwrap();
            counted.1.push(bound);
            counted.1.len()
        };
        (self.turn.lock().unwrap())(number, bound)
    }

    fn moves_by(&self, bound: Duration) {
        let mut counted = self.counted.lock().unwrap();
        counted.0 = counted.0.saturating_add(bound);
    }
}
