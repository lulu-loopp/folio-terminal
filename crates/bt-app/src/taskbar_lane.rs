//! **Whether the taskbar hides itself, asked on a lane of its own** (0.4.5 ticket 62;
//! `docs/ARCHITECTURE.md` §5.1's observation lane, §4.2's "observations of external state").
//!
//! `bt_platform::taskbar_is_auto_hidden` is `SHAppBarMessage(ABM_GETSTATE)` on Windows — a message
//! to Explorer's taskbar that waits for Explorer to answer. Until this lane it was asked by
//! `sample_window_place` on the window thread, at the head of every turn and again by an attention
//! delivery between turns, and the owner's stall report of 2026-09-25 caught two of those asks at
//! 99 ms and 92 ms inside one 535 ms hold while Explorer was busy. The answer changes when somebody
//! changes one setting, so there is no reason for the window thread to wait for it.
//!
//! **One owner, one writer.** The answer lives in a [`bt_platform::TaskbarState`], numbered by the
//! request it answers; the lane's worker is the only writer, and the slot takes an answer only if
//! it is newer than the one held. The window thread reads the latest answer with one atomic load
//! ([`observe`]) and never waits.
//!
//! **When it is asked**:
//! * **at launch** — [`request`] beside the other probes' wakes in `FolioApp`'s start;
//! * **every [`REFRESH_INTERVAL`] while a window is on a screen** — a window's own reading of
//!   where it is ([`observe`], from `sample_window_place`) asks again when the last request is
//!   that old and the window is not hidden. A window nobody can see decides no flash;
//! * **when Windows says a system setting moved** — the `WM_SETTINGCHANGE` subclass
//!   (`bt_platform::SystemSettingsWatch`) already wakes the loop with
//!   `AppEvent::SystemPreferencesChanged`, and that arm asks. Turning auto-hide on or off changes
//!   the desktop's work area, which Windows announces with that broadcast. `ABN_STATECHANGE` is not
//!   used: the shell sends it only to windows registered as application bars (`ABM_NEW`), and a
//!   Folio window is not one.
//!
//! **Until the first answer lands the reading is "not hidden"** ([`bt_platform::TaskbarReading`]'s
//! default). That is the direction whose mistake can be taken back: under it a covered window's
//! delivery flashes the taskbar button, and a flash can be stopped and re-placed when the answer
//! says the bar hides (`TaskbarFlash`, in `main.rs`); the other direction would put a message on
//! the desktop, which cannot be unsent.

use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use bt_platform::{TaskbarReading, TaskbarState};

/// **How old a request may get while a window is on a screen** before the next reading of where
/// the window is asks again. "Every few seconds" (ticket 62): the setting it watches is changed by
/// hand, and the `WM_SETTINGCHANGE` road usually brings the change sooner than this.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// The lane: the slot, the numbered requests, and the one worker that serves them.
///
/// A type rather than three statics so a test can run a lane of its own beside the product's —
/// with the real shell probe, or with one it holds back — without the two sharing a number.
pub struct TaskbarLane {
    slot: TaskbarState,
    asks: Mutex<Asks>,
    asked: Condvar,
    /// The question. The product's is [`ask_the_shell`].
    ask: Box<dyn Fn() -> bool + Send + Sync>,
    /// How the worker brings the event loop round: after a first answer, and after an answer that
    /// differs from the one held. The same shape as `settings::install_font_scan_wake`.
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

/// The requests, and whether a worker is out to serve them.
#[derive(Debug, Default)]
struct Asks {
    /// The number of the newest request. Requests are counted from `1`, so `0` is "none yet" —
    /// the same number [`bt_platform::TaskbarReading::generation`] holds before the first answer.
    requested: u64,
    /// The number of the newest request the worker has answered.
    served: u64,
    /// A worker thread is running. It is started by the first request and then waits on
    /// [`TaskbarLane::asked`] for the next one; it is started again only if starting it failed.
    worker: bool,
    /// When the newest request was made — the clock [`TaskbarLane::refresh_if_due`] reads.
    last_request: Option<Instant>,
}

impl TaskbarLane {
    /// A lane that asks `ask`, with nothing asked yet and no worker running.
    pub fn new(ask: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            slot: TaskbarState::new(),
            asks: Mutex::new(Asks::default()),
            asked: Condvar::new(),
            ask: Box::new(ask),
            wake: OnceLock::new(),
        }
    }

    /// Teach the worker how to bring the event loop round. Once; a second call is ignored.
    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// The latest answer. One atomic load; never waits for the worker.
    pub fn reading(&self) -> TaskbarReading {
        self.slot.latest()
    }

    /// **The whole of what a window's reading of where it is does about the taskbar**: ask for a
    /// fresh answer when the clock says and the window is on a screen, and read the latest one.
    ///
    /// Never asks the shell. A request is a lock and a condition-variable signal — or, the first
    /// time, starting the worker — and the answer comes back on a later reading.
    pub fn observe(&'static self, now: Instant, on_screen: bool) -> TaskbarReading {
        if on_screen {
            self.refresh_if_due(now);
        }
        self.reading()
    }

    /// Ask again if no request has been made for [`REFRESH_INTERVAL`].
    pub fn refresh_if_due(&'static self, now: Instant) {
        let asks = self.lock();
        let due = asks
            .last_request
            .is_none_or(|last| now.saturating_duration_since(last) >= REFRESH_INTERVAL);
        if due {
            self.ask_locked(asks, now);
        }
    }

    /// **Number a request and see that the worker serves it.**
    pub fn request(&'static self, now: Instant) {
        let asks = self.lock();
        self.ask_locked(asks, now);
    }

    fn ask_locked(&'static self, mut asks: MutexGuard<'_, Asks>, now: Instant) {
        asks.requested += 1;
        asks.last_request = Some(now);
        if asks.worker {
            self.asked.notify_one();
            return;
        }
        asks.worker = true;
        drop(asks);
        let started = bt_platform::spawn_at_priority(
            "taskbar-state",
            bt_platform::ThreadPriority::BelowNormal,
            move |_ctx| self.serve(),
        );
        if started.is_err() {
            // A machine that will not give this process a thread keeps the answer it has, and the
            // next request tries again. Nothing on the window thread waits for an answer.
            self.lock().worker = false;
        }
    }

    /// **The worker's whole body**: wait for a request newer than the last one served, ask, hold
    /// the answer out, and wake the loop when the answer is news.
    fn serve(&self) {
        loop {
            // The number is read before the question is put, so a request made while the shell is
            // being asked has a larger one and is served by the next round.
            let generation = {
                let mut asks = self.lock();
                while asks.served == asks.requested {
                    asks = self
                        .asked
                        .wait(asks)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                asks.requested
            };
            let start = Instant::now();
            let auto_hidden = (self.ask)();
            if std::env::var_os("BT_PERF_TRACE").is_some_and(|v| !v.is_empty()) {
                crate::trace_sink::stderr_line(format!(
                    "BT_PERF_TRACE taskbar_state_us={} auto_hidden={} generation={generation}",
                    start.elapsed().as_micros(),
                    u8::from(auto_hidden)
                ));
            }
            let before = self.slot.latest();
            let taken = self.slot.offer(generation, auto_hidden);
            self.lock().served = generation;
            // After the answer is in the slot and never before: a wake that raced the offer would
            // send the loop to read the old answer.
            let news = !before.answered() || before.auto_hidden != auto_hidden;
            if taken
                && news
                && let Some(wake) = self.wake.get()
            {
                wake();
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, Asks> {
        self.asks.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The product's lane, asking the shell.
static LANE: LazyLock<TaskbarLane> = LazyLock::new(product_lane);

/// The lane the product runs: the one that is handed [`ask_the_shell`].
fn product_lane() -> TaskbarLane {
    TaskbarLane::new(ask_the_shell)
}

/// [`TaskbarLane::install_wake`] on the product's lane. Called once, at startup.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    LANE.install_wake(wake);
}

/// [`TaskbarLane::request`] on the product's lane: at launch, and when a system setting moved.
pub fn request() {
    LANE.request(Instant::now());
}

/// [`TaskbarLane::observe`] on the product's lane — `sample_window_place`'s one taskbar call.
pub fn observe(now: Instant, on_screen: bool) -> TaskbarReading {
    LANE.observe(now, on_screen)
}

/// [`TaskbarLane::reading`] on the product's lane.
pub fn reading() -> TaskbarReading {
    LANE.reading()
}

thread_local! {
    /// How many times the calling thread has asked the shell about the taskbar — the door the
    /// pins read, per thread for `settings::MONOSPACE_SCANS`' reason (ticket 50): the lane asks
    /// on its own thread while a test's thread reads, and only a per-thread count says anything
    /// about the thread that reads.
    static SHELL_ASKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// **The one place in `bt-app` that asks the shell whether its taskbar hides itself**, and it runs
/// only on the lane's worker.
pub fn ask_the_shell() -> bool {
    SHELL_ASKS.with(|asks| asks.set(asks.get() + 1));
    bt_platform::taskbar_is_auto_hidden()
}

/// How many times the calling thread has asked the shell. A door for the pins and nothing else.
#[cfg(test)]
pub fn shell_asks() -> u64 {
    SHELL_ASKS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{REFRESH_INTERVAL, TaskbarLane};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// A lane of the test's own, leaked for the `'static` its worker needs, with a wake that
    /// reports every answer the worker calls news. `pub(crate)` for the flat module's pins.
    pub(crate) fn lane(
        ask: impl Fn() -> bool + Send + Sync + 'static,
    ) -> (&'static TaskbarLane, mpsc::Receiver<()>) {
        let lane: &'static TaskbarLane = Box::leak(Box::new(TaskbarLane::new(ask)));
        let (woke, wakes) = mpsc::channel();
        let woke = std::sync::Mutex::new(woke);
        lane.install_wake(move || {
            let _ = woke.lock().unwrap().send(());
        });
        (lane, wakes)
    }

    /// RED (62) — **The lane's answer is read without waiting, and an older answer never
    /// overwrites a newer one.**
    ///
    /// The shell here is held: it answers only when the test lets it. While it is held, a reading
    /// comes back at once with the answer already in the slot — first the default, then the last
    /// one that landed — and a request made during the held ask is served by the next round with a
    /// larger number. The slot is the one place numbers are compared, so an answer that arrives
    /// out of order (a second asker, a retried one) cannot put back what the reader has moved past.
    ///
    /// MUTATION: drop the generation comparison in `bt_platform::TaskbarState::offer` — red (the
    /// older answer replaces the newer one).
    #[test]
    fn the_lanes_answer_is_read_without_waiting_and_an_older_answer_never_overwrites_a_newer_one() {
        let (release, released) = mpsc::channel::<bool>();
        let released = std::sync::Mutex::new(released);
        let (lane, wakes) = lane(move || released.lock().unwrap().recv().unwrap_or(false));
        let start = Instant::now();

        lane.request(start);
        let held = lane.observe(start, true);
        assert!(
            !held.answered(),
            "the shell is held, and the reading did not wait for it"
        );
        assert!(
            !held.auto_hidden,
            "until an answer lands the bar is read as on screen"
        );

        release.send(true).unwrap();
        wakes
            .recv_timeout(Duration::from_secs(30))
            .expect("the answer wakes the loop");
        let first = lane.reading();
        assert_eq!((first.generation, first.auto_hidden), (1, true));

        // A second question, held; the reading is the last answer, at once.
        lane.request(start + REFRESH_INTERVAL);
        assert_eq!(lane.observe(start + REFRESH_INTERVAL, true), first);
        release.send(false).unwrap();
        wakes
            .recv_timeout(Duration::from_secs(30))
            .expect("a changed answer wakes the loop");
        let second = lane.reading();
        assert_eq!((second.generation, second.auto_hidden), (2, false));

        // An answer to the first request arriving now changes nothing.
        assert!(!lane.slot.offer(first.generation, true));
        assert_eq!(
            lane.reading(),
            second,
            "an older answer never overwrites a newer one"
        );
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): a [`TaskbarLane`] of its
/// own — the real numbered requests, worker, slot and wake — asking a question the suite's
/// [`crate::lane::Gate`] can hold, whose answer changes every time so that every answer is news.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    use super::TaskbarLane;
    use crate::lane::{
        Admission, Contract, Delivered, Gate, LaneUnderTest, Outcome, TASKBAR, WakeProbe,
    };

    struct TaskbarAdapter {
        lane: &'static TaskbarLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// The generation the consumer last read — what `attention.rs` compares a reading with.
        seen: u64,
    }

    /// A fresh lane, leaked for the `'static` its worker needs.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let door = Arc::clone(&gate);
        let asked = AtomicU64::new(0);
        let lane: &'static TaskbarLane = Box::leak(Box::new(TaskbarLane::new(move || {
            door.pass(None);
            asked.fetch_add(1, Ordering::Relaxed).is_multiple_of(2)
        })));
        let wake = Arc::clone(&probe);
        lane.install_wake(move || wake.woke());
        Box::new(TaskbarAdapter {
            lane,
            gate,
            probe,
            seen: 0,
        })
    }

    impl LaneUnderTest for TaskbarAdapter {
        fn contract(&self) -> &'static Contract {
            &TASKBAR
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        fn submit(&mut self, _target: u32, question: u64) -> Admission {
            self.lane.request(Instant::now());
            Admission {
                ticket: Some(self.lane.lock().requested),
                question,
                refused: None,
            }
        }

        fn close_target(&mut self, _target: u32) {
            unreachable!("the taskbar lane's target is the application, which does not close");
        }

        /// The window's reading: one load of the slot. A generation other than the one last read
        /// is an answer raised.
        fn drain(&mut self) -> Vec<Delivered> {
            let reading = self.lane.reading();
            if reading.generation == self.seen {
                return Vec::new();
            }
            self.seen = reading.generation;
            vec![Delivered {
                target: Some(0),
                ticket: Some(reading.generation),
                question: None,
                outcome: Outcome::Answered,
            }]
        }

        /// A second asker's late answer, through the slot's own `offer`.
        fn offer_stale(&mut self, ticket: u64) {
            let _ = self.lane.slot.offer(ticket, true);
        }
    }
}
