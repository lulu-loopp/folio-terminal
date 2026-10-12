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
//! **Built on the completion seam** (`crate::lane`; the side-effects contract's C1–C7, S-4): the
//! one `taskbar-state` worker is started by a [`crate::lane::Executor`] (the thread door's in the
//! product, the suite's scripted one under test) when the first request is made, every request's
//! identity is minted by the lane's [`crate::lane::Ledger`], each ask of the shell runs inside
//! [`crate::lane::task`], and every request ends through the ledger's hooks:
//!
//! * **Coalescing**, kept: one ask out, at most one waiting. Every request is the same question —
//!   does the bar hide itself now — so one made while another waits **joins** it
//!   (`Joined { with }`, carrying its answer; revision (b)4), and one ask answers both; nothing is
//!   `Superseded`. A request made while an ask is out waits for the next ask, because the setting
//!   may have moved after the shell answered the one out.
//! * **Completion**: the shell's answer is the request's `Done`. The question has no error form —
//!   a shell that does not answer reads as a visible bar (`bt_platform::taskbar_is_auto_hidden`)
//!   — so `Failed` is a panic in the ask, said once per streak by the ledger, and the worker goes
//!   on; the worker's end ends every request still owed `WorkerGone`, faults the lane (every later
//!   request refused at the door) and wakes the loop.
//! * **Bound**: the ledger owes at most [`BOUND`] requests at once — the ask out, the one waiting
//!   with every request joined to it, and every answer the window thread has not taken; a request
//!   past it is refused at the door, and the ask already standing answers what it would have
//!   asked.
//! * **Wake**: after every terminal the worker publishes, and after the worker's end
//!   (`AppEvent::TaskbarAnswered`).
//!
//! **One owner, one writer.** The answer lives in a [`bt_platform::TaskbarState`], numbered by the
//! request it answers, and its one writer is the window thread's reading ([`TaskbarLane::reading`]),
//! which takes every terminal the lane has ended, in the order the requests were made, and offers
//! each answer to the slot — which takes it only if it is newer than the one held. The reading
//! never waits for the worker: one lock of the requests, then one atomic load.
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

use std::convert::Infallible;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use bt_platform::admission::WorkerCtx;
use bt_platform::{TaskbarReading, TaskbarState};

use crate::lane::{self, Coalescing, Executor, Ledger, Refused, Saying, Terminal, Waiting};

/// **How old a request may get while a window is on a screen** before the next reading of where
/// the window is asks again. "Every few seconds" (ticket 62): the setting it watches is changed by
/// hand, and the `WM_SETTINGCHANGE` road usually brings the change sooner than this.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// **How many requests the lane's ledger owes at once**: the ask out, the one waiting and every
/// request joined to it, and every answer the window thread has not taken yet. The requests come
/// at most one per [`REFRESH_INTERVAL`] and one per system-setting broadcast, and every reading of
/// where a window is takes the answers; the bound makes a shell that never answers an
/// accounted-for set of requests, not an unbounded one.
pub(crate) const BOUND: usize = 16;

/// The lane's name in the lines its ledger says.
const LANE_NAME: &str = "the taskbar lane";

/// **One request about the taskbar**, its identity minted by the lane's ledger and rising with
/// every request — the number the slot's readings carry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct TaskbarId(u64);

impl From<u64> for TaskbarId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

/// What a request about the taskbar is owed: the application asked it and holds it.
type TaskbarOwed = lane::ToTheApplication<bool, Infallible>;

/// How one request about the taskbar ended.
pub(crate) type TaskbarTerminal = Terminal<TaskbarId, bool, Infallible>;

/// The question, for the newest request an ask answers. The product's is [`ask_the_shell`].
type Question = dyn Fn(u64) -> bool + Send + Sync;

/// The lane: the slot, the clock of the last request, the question, the worker's executor, the
/// wake, and the requests.
///
/// A type rather than statics so a test can run a lane of its own beside the product's — with the
/// real shell probe, or with one it holds back — without the two sharing a number.
pub struct TaskbarLane {
    slot: TaskbarState,
    /// When the newest request was made — the clock [`TaskbarLane::refresh_if_due`] reads.
    last_request: Mutex<Option<Instant>>,
    /// The question. The product's is [`ask_the_shell`].
    ask: Box<Question>,
    executor: Executor,
    /// How the lane brings the event loop round — after every terminal it publishes, and after its
    /// worker's end — once [`Self::install_wake`] has taught it; shared with the ledger.
    wake: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>,
    requests: Coalescing<TaskbarId, TaskbarOwed, ()>,
}

impl TaskbarLane {
    /// A lane that asks `ask`, with nothing asked yet and no worker running: its worker is started
    /// through the thread door by the first request, and its ledger says its lines in
    /// `diagnostics.log`.
    pub fn new(ask: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self::start_with(
            Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            move |_| ask(),
            crate::diagnostics::note,
        )
    }

    /// A lane whose worker `executor` starts, asking `ask` for the newest request each ask answers
    /// and saying its ledger's lines through `say`.
    fn start_with(
        executor: Executor,
        ask: impl Fn(u64) -> bool + Send + Sync + 'static,
        say: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let wake: Arc<OnceLock<Box<dyn Fn() + Send + Sync>>> = Arc::new(OnceLock::new());
        let installed = Arc::clone(&wake);
        let ledger = Ledger::new(LANE_NAME, Some(BOUND), Saying::PerStreak, say, move || {
            if let Some(wake) = installed.get() {
                wake();
            }
        });
        Self {
            slot: TaskbarState::new(),
            last_request: Mutex::new(None),
            ask: Box::new(ask),
            executor,
            wake,
            requests: Coalescing::new(ledger),
        }
    }

    /// Teach the lane how to bring the event loop round. Once; a second call is ignored.
    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// **The latest answer** — the window thread's acceptance: every terminal the lane has ended
    /// is taken, in the order the requests were made, and each answer offered to the slot, which
    /// keeps the newest; then one atomic load. Never waits for the worker.
    pub fn reading(&self) -> TaskbarReading {
        for (id, terminal) in self.requests.ended() {
            self.take(id, terminal);
        }
        self.slot.latest()
    }

    /// **One terminal, taken**: its answer — its own, or the ask's it joined — offered to the
    /// slot. A request that failed, or was owed when the worker ended, leaves the reading as it
    /// was (the ledger has said it).
    fn take(&self, id: TaskbarId, terminal: TaskbarTerminal) {
        if let Some(auto_hidden) = lane::answer_of(terminal) {
            self.slot.offer(id.0, auto_hidden);
        }
    }

    /// **The whole of what a window's reading of where it is does about the taskbar**: ask for a
    /// fresh answer when the clock says and the window is on a screen, and read the latest one.
    ///
    /// Never asks the shell. A request is a lock, a ledger entry and a condition-variable signal —
    /// or, the first time, starting the worker — and the answer comes back on a later reading.
    pub fn observe(&'static self, now: Instant, on_screen: bool) -> TaskbarReading {
        if on_screen {
            self.refresh_if_due(now);
        }
        self.reading()
    }

    /// Ask again if no request has been made for [`REFRESH_INTERVAL`].
    pub fn refresh_if_due(&'static self, now: Instant) {
        let due = self
            .last_request()
            .is_none_or(|last| now.saturating_duration_since(last) >= REFRESH_INTERVAL);
        if due {
            self.request(now);
        }
    }

    /// **Make a request and see that the worker serves it.** A request the lane refuses at its
    /// door — its ledger already owes [`BOUND`], or its worker has ended — has that refusal as its
    /// outcome and is waited on by nobody: the ask already standing answers what it would have
    /// asked, and the worker's end was said once, by the ledger.
    pub fn request(&'static self, now: Instant) {
        let _ = self.ask(now);
    }

    /// **The lane's one admission**: the request's identity minted by the ledger — joined to the
    /// waiting ask, or waiting for the next — or the ledger's refusal, at once. The first request
    /// starts the worker; a thread the door refuses refuses that request at the door
    /// (`Refused::NotStarted`, said once) and leaves the lane as it was, so the next request starts
    /// the worker again.
    fn ask(&'static self, now: Instant) -> Result<TaskbarId, Refused> {
        *self
            .last_request
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(now);
        self.requests
            .ask(
                (),
                TaskbarOwed::owed(),
                |(), ()| true,
                |(), ()| (),
                |ran| {
                    self.executor
                        .start_worker("taskbar-state", ran, move |ctx| run_taskbar_lane(ctx, self))
                },
            )
            .map(|admitted| admitted.id)
    }

    fn last_request(&self) -> Option<Instant> {
        *self
            .last_request
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn wake(&self) {
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }
}

/// **The lane's worker body**: the next ask standing, put to the shell inside its task for the
/// newest request it answers, its terminal published, then the wake — and round again, for the
/// life of the lane.
fn run_taskbar_lane(ctx: &WorkerCtx, lane: &TaskbarLane) {
    loop {
        let Waiting {
            operation, newest, ..
        } = lane.requests.next(ctx);
        let start = Instant::now();
        let terminal = match lane::task(ctx, |_| Ok::<_, Infallible>((lane.ask)(newest.0))) {
            Ok(auto_hidden) => Terminal::Done(auto_hidden),
            Err(failure) => Terminal::Failed(failure),
        };
        if std::env::var_os("BT_PERF_TRACE").is_some_and(|v| !v.is_empty())
            && let Terminal::Done(auto_hidden) = terminal
        {
            crate::trace_sink::stderr_line(format!(
                "BT_PERF_TRACE taskbar_state_us={} auto_hidden={} generation={}",
                start.elapsed().as_micros(),
                u8::from(auto_hidden),
                newest.0
            ));
        }
        lane.requests.ledger().publish(operation, terminal);
        // After the terminal is published and never before: a wake that raced the publication
        // would send the loop to read the old answer.
        lane.wake();
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
    use super::{REFRESH_INTERVAL, TaskbarId, TaskbarLane, TaskbarTerminal};
    use crate::lane::{Executor, Gate, Terminal, wait_for_a_wake, wake_channel};
    use bt_platform::TaskbarReading;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A lane of the test's own, leaked for the `'static` its worker needs, with a wake that
    /// reports every answer the worker publishes. `pub(crate)` for the flat module's pins.
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

    /// What a lane of a test's own has said, line by line, through its ledger's `say`.
    type Said = Arc<Mutex<Vec<String>>>;

    /// **A lane of the test's own on the suite's executor** over `gate`, asking `ask`, its wake
    /// heard on a channel and its ledger's lines kept. Leaked, for the `'static` its worker needs.
    fn scripted_lane(
        gate: &Arc<Gate>,
        ask: impl Fn(u64) -> bool + Send + Sync + 'static,
    ) -> (&'static TaskbarLane, mpsc::Receiver<()>, Said) {
        let said: Said = Arc::default();
        let kept = Arc::clone(&said);
        let lane: &'static TaskbarLane = Box::leak(Box::new(TaskbarLane::start_with(
            Executor::scripted(Arc::clone(gate)),
            ask,
            move |line: &str| kept.lock().unwrap().push(line.to_owned()),
        )));
        let (wake, wakes) = wake_channel();
        lane.install_wake(wake);
        (lane, wakes, said)
    }

    /// Every terminal the lane ends, taken through the reading's own take, until `last` has
    /// ended — each wake waited for within the lane suite's patience.
    fn terminals_through(
        lane: &TaskbarLane,
        wakes: &mpsc::Receiver<()>,
        last: TaskbarId,
    ) -> Vec<(TaskbarId, TaskbarTerminal)> {
        let mut ended = Vec::new();
        loop {
            ended.extend(lane.requests.ended());
            if ended.iter().any(|(id, _)| *id == last) {
                return ended;
            }
            wait_for_a_wake(wakes, &format!("taskbar request {last:?}'s terminal"));
        }
    }

    /// RED (S-4; D-73) — **The requests made while an ask is out end `Joined` to the one the next
    /// ask serves, which ends `Done`** (revision (b)4: every request is the same question — does
    /// the bar hide itself now — so the requests standing for the next ask are one operation, and
    /// nothing is `Superseded`).
    ///
    /// An ask is held out; three requests are made behind it. The ask out ends `Done`; the first
    /// request behind it is the next ask's operation and ends `Done` with that ask's answer — put
    /// for the newest request standing when it started; the two after it end
    /// `Joined { with: that request }`, carrying exactly its answer.
    ///
    /// MUTATION: the old silence — `Coalescing::ask` answering a request made while one waits
    /// with the waiting request's identity and minting none — the third and fourth requests have
    /// no identity, and no terminal, of their own.
    #[test]
    fn requests_made_during_a_walk_end_superseded_or_joined_and_the_served_one_done() {
        let gate = Arc::new(Gate::default());
        // The answer says which request the ask was put for: auto-hidden for an even one.
        let (lane, wakes, _) = scripted_lane(&gate, |generation| generation.is_multiple_of(2));
        let start = Instant::now();
        gate.hold();
        let out = lane.ask(start).expect("admitted");
        assert!(gate.wait_entered(1), "the ask is out");
        let asked: Vec<TaskbarId> = (1..=3)
            .map(|turn| {
                lane.ask(start + REFRESH_INTERVAL * turn)
                    .expect("admitted behind the ask out")
            })
            .collect();
        gate.release();
        let ended = terminals_through(lane, &wakes, asked[2]);
        assert_eq!(
            ended[..2],
            [
                (out, Terminal::Done(out.0.is_multiple_of(2))),
                (asked[0], Terminal::Done(asked[2].0.is_multiple_of(2))),
            ],
            "the ask out is answered, and the next ask is put for the newest request standing"
        );
        assert_eq!(
            ended[2..],
            [
                (
                    asked[1],
                    Terminal::Joined {
                        with: asked[0],
                        outcome: Box::new(Terminal::Done(asked[2].0.is_multiple_of(2))),
                    }
                ),
                (
                    asked[2],
                    Terminal::Joined {
                        with: asked[0],
                        outcome: Box::new(Terminal::Done(asked[2].0.is_multiple_of(2))),
                    }
                ),
            ],
            "a request made while one waited joined it and carries its answer"
        );
        assert_eq!(gate.entered(), 2, "two asks of the shell for four requests");
    }

    /// RED (S-4 round 2) — **A start the thread door refuses refuses that request at the door,
    /// says so once, and the next request starts the worker** (revision (b)5, rule 1: a new
    /// request tries again; the lane is not in fault, because its worker never ran).
    ///
    /// MUTATION: fault the lane on a refused start (`lane::Ran::worker_ended` handing
    /// `NotStarted` to the ledger) — the ledger says the worker's end beside the refusal (two
    /// lines, observed red at "said once"), and the next request would be refused `Faulted`.
    #[test]
    fn a_refused_spawn_refuses_this_request_and_the_next_request_starts_the_worker() {
        let gate = Arc::new(Gate::default());
        let (lane, wakes, said) = scripted_lane(&gate, |_| true);
        let start = Instant::now();
        gate.refuse_next_start();
        assert!(
            matches!(lane.ask(start), Err(crate::lane::Refused::NotStarted(_))),
            "the request whose start was refused is refused at the door"
        );
        assert_eq!(
            lane.requests.ledger().earliest_owed(),
            None,
            "minting nothing"
        );
        let lines = said.lock().unwrap().clone();
        assert_eq!(lines.len(), 1, "said once: {lines:?}");
        assert!(lines[0].contains(crate::lane::REFUSED_START), "{lines:?}");
        let next = lane.ask(start).expect("the next request starts the worker");
        assert_eq!(next, TaskbarId(1), "the refused request took no identity");
        let ended = terminals_through(lane, &wakes, next);
        assert_eq!(
            ended,
            [(next, Terminal::Done(true))],
            "and the worker it started asks"
        );
    }

    /// RED (S-4; D-73) — **A worker that dies ends every request it owes `WorkerGone`, wakes the
    /// loop, says so once, and the next request is refused at the door.**
    ///
    /// Until S-4 a worker that died left `Asks::worker` set: every later request was counted and
    /// never served, with no fault and no outcome. The suite's gate ends the worker while it holds
    /// one ask out and two requests stand behind it.
    ///
    /// MUTATION: leave `Asks::worker` set — `Ledger::worker_ended` returning at once (the death
    /// leaves the lane as it was) — no wake comes, and the wait for it runs out of patience.
    #[test]
    fn a_dead_walk_ends_every_owed_request_worker_gone_and_the_next_request_is_refused() {
        let gate = Arc::new(Gate::default());
        let (lane, wakes, said) = scripted_lane(&gate, |_| false);
        let start = Instant::now();
        gate.hold();
        let out = lane.ask(start).expect("admitted");
        assert!(gate.wait_entered(1), "the ask is out");
        let waiting = lane.ask(start).expect("admitted behind it");
        let joined = lane.ask(start).expect("and joined to that");
        gate.kill();
        gate.release();
        wait_for_a_wake(&wakes, "the worker's end");
        assert_eq!(
            lane.requests.ended(),
            [
                (out, Terminal::WorkerGone),
                (waiting, Terminal::WorkerGone),
                (
                    joined,
                    Terminal::Joined {
                        with: waiting,
                        outcome: Box::new(Terminal::WorkerGone),
                    }
                ),
            ]
        );
        assert!(
            matches!(
                lane.ask(start),
                Err(crate::lane::Refused::Faulted(
                    crate::lane::WorkerEnd::Panicked(_)
                ))
            ),
            "a request after the worker's end is refused at the door, minting nothing"
        );
        assert_eq!(
            lane.reading(),
            TaskbarReading::default(),
            "and the reading is the one held before any answer"
        );
        let said = said.lock().unwrap().clone();
        assert_eq!(said.len(), 1, "the end is said once: {said:?}");
        assert!(
            said[0].contains("3 request(s) owed end as WorkerGone"),
            "{said:?}"
        );
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): a [`TaskbarLane`] of its
/// own — the real admission, worker body, ledger and reading — its worker started by the suite's
/// executor, its question executing the suite's question for the newest request it answers so the
/// suite's [`crate::lane::Gate`] can hold it, count it and plant a panic in it, and its ledger's
/// lines counted.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Instant;

    use super::{TaskbarId, TaskbarLane, TaskbarTerminal};
    use crate::lane::{
        Admission, Contract, Delivered, Executor, Gate, LaneUnderTest, Outcome, TASKBAR, Terminal,
        WakeProbe,
    };

    struct TaskbarAdapter {
        lane: &'static TaskbarLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// The suite's own record of which question each identity carried, read by the ask.
        questions: Arc<Mutex<HashMap<u64, u64>>>,
        /// The lines the ledger said, counted through its `say`.
        said: Arc<AtomicUsize>,
        /// A late answer the slot took, raised at the next drain.
        raised: Vec<Delivered>,
    }

    fn outcome(terminal: TaskbarTerminal) -> Outcome {
        match terminal {
            Terminal::Done(_) => Outcome::Answered,
            Terminal::Failed(failure) => Outcome::Failed(failure.to_string()),
            Terminal::Cancelled => Outcome::Cancelled,
            Terminal::Superseded { by } => Outcome::Superseded { by: by.0 },
            Terminal::Joined {
                with,
                outcome: carried,
            } => Outcome::Joined {
                with: with.0,
                carried: Box::new(self::outcome(*carried)),
            },
            Terminal::WorkerGone => Outcome::WorkerGone,
        }
    }

    /// A fresh lane, leaked for the `'static` its worker needs.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let questions: Arc<Mutex<HashMap<u64, u64>>> = Arc::default();
        let said = Arc::new(AtomicUsize::new(0));
        let (door, asked, counted) = (Arc::clone(&gate), Arc::clone(&questions), Arc::clone(&said));
        let lane: &'static TaskbarLane = Box::leak(Box::new(TaskbarLane::start_with(
            Executor::scripted(Arc::clone(&gate)),
            move |generation| {
                let question = asked
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&generation)
                    .copied();
                if let Some(question) = question {
                    door.execute(question);
                }
                generation.is_multiple_of(2)
            },
            move |_line: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
        )));
        let wake = Arc::clone(&probe);
        lane.install_wake(move || wake.woke());
        Box::new(TaskbarAdapter {
            lane,
            gate,
            probe,
            questions,
            said,
            raised: Vec::new(),
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

        /// `taskbar_lane::request`, on this lane: the lane's own admission, or its refusal at the
        /// door as the request's terminal.
        fn submit(&mut self, _target: u32, question: u64) -> Admission {
            // The question is recorded under the identity the next admission mints, before the
            // worker can read it: the lane mints identities in order, one per admission.
            let mut questions = self
                .questions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let next = questions.keys().max().map_or(1, |last| last + 1);
            questions.insert(next, question);
            drop(questions);
            match self.lane.ask(Instant::now()) {
                Ok(id) => {
                    assert_eq!(id.0, next, "the taskbar lane mints its identities in order");
                    Admission {
                        ticket: Some(id.0),
                        question,
                        refused: None,
                    }
                }
                Err(refused) => {
                    self.questions
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&next);
                    Admission {
                        ticket: None,
                        question,
                        refused: Some(format!("{refused:?}")),
                    }
                }
            }
        }

        fn close_target(&mut self, _target: u32) {
            unreachable!("the taskbar lane's target is the application, which does not close");
        }

        /// The window's reading: the terminals taken in the order they were minted, each a
        /// delivery, and each answer offered to the slot as the product's reading offers it.
        fn drain(&mut self) -> Vec<Delivered> {
            let mut delivered = std::mem::take(&mut self.raised);
            let questions = self
                .questions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for (id, terminal) in self.lane.requests.ended() {
                self.lane.take(id, terminal.clone());
                delivered.push(Delivered {
                    target: Some(0),
                    ticket: Some(id.0),
                    question: questions.get(&id.0).copied(),
                    outcome: outcome(terminal),
                });
            }
            delivered
        }

        /// A second asker's late answer, through the lane's own publication — which refuses a
        /// second terminal — and through the slot's own `offer`, which refuses an answer older
        /// than the one held. Either taking it is an answer raised.
        fn offer_stale(&mut self, ticket: u64) {
            self.lane
                .requests
                .ledger()
                .publish(TaskbarId(ticket), Terminal::Done(true));
            if self.lane.slot.offer(ticket, true) {
                self.raised.push(Delivered {
                    target: Some(0),
                    ticket: Some(ticket),
                    question: None,
                    outcome: Outcome::Answered,
                });
            }
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}
