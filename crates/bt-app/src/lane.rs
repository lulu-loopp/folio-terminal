//! **The lane contract** — one shape every lane is held to, and the one suite that holds it
//! (D-33; `docs/plans/design/window-thread-budget-2026-09-25.md` §3 and §R-D; `docs/ARCHITECTURE.md`
//! §5.1, *the lane contract*).
//!
//! A lane is a worker the window thread hands requests to and hears answers from without waiting.
//! Each lane in this crate grew its own policy, stated in its own module doc. This module states
//! them as data ([`Contract`], one constant per lane) and checks each against the lane's real
//! behaviour through an adapter ([`LaneUnderTest`]) that drives the lane's own admission,
//! publication and acceptance.
//!
//! **The obligations**, one [`Claim`] each: a full lane answers without making the asker wait, and
//! admits no more than it declares; every request carries its own identity; execution and delivery
//! follow the declared order; an abandoned answer, or one older than the answer adopted, is not
//! raised; every admitted request ends exactly once; the wake follows the publication, and a lost
//! wake loses no answer; the answers held for a consumer that has not drained are bounded; and a
//! worker that dies leaves an observable terminal or fault state.
//!
//! **A lane that fails a claim is not declared conformant to make the suite pass.** Its failure is
//! a row of [`EXPECTED_FAILURES`], with the exact kind of failure and the ledger row that repairs
//! it (`docs/plans/structural-debt.md`). [`judge`] is red on an unexpected pass, on a failure of a
//! different kind, on a lane or claim that was not run, and on a claim that exercised no request.
//!
//! **Compiled with the tests only.** No product code reads a declaration yet: each lane still
//! enforces its own policy in its own module, and the suite is what ties the two together. A lane
//! born on the contract (the storage lane, 0.4.6 ticket B4) is the change that makes this module
//! product code.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

/// The lanes the suite runs. Every one has an adapter; [`judge`] refuses a run that skipped one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum LaneName {
    /// `handoff_lane`: the OS hand-off lane, the reference instance.
    Handoff,
    /// `settings::FontLane`: the machine's font families.
    Font,
    /// `taskbar_lane`: whether the taskbar hides itself.
    Taskbar,
    /// `MathWorker`'s formula and decode thread (`run_decoration_worker`).
    Computation,
}

impl LaneName {
    pub(crate) const ALL: [Self; 4] = [Self::Handoff, Self::Font, Self::Taskbar, Self::Computation];
}

/// How a newer request relates to an older one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Replacement {
    /// Every request is executed and answered.
    Fifo,
    /// A newer request supersedes one not yet started; "latest" is the newest **requested**, and
    /// an answer older than the one **adopted** is dropped.
    LatestValue,
    /// One question, one answer, keyed by the question.
    PerQuestion,
}

/// The order requests are executed in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Order {
    /// The order they were admitted in.
    Submission,
    /// The newest request standing when a round starts; the ones between are not executed.
    NewestRequested,
}

/// The order answers reach the consumer in, stated apart from execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Delivery {
    /// Refusals made at the door first, then the worker's answers in execution order — the hand-off
    /// lane's `answers()`: a later refused press can be heard before an earlier admitted one.
    RefusalsThenCompletions,
    /// In admission order.
    Submission,
    /// Only newer than what the consumer adopted; the adopted number never goes back.
    NewestAdopted,
}

/// A count a lane promises not to exceed, or the declaration that it promises none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Bound {
    At(usize),
    Unbounded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Bounds {
    /// Requests admitted and not started. For a latest-value lane, the pending rounds.
    pub(crate) waiting: Bound,
    /// Requests executing at once.
    pub(crate) executing: usize,
    /// Answers published and not yet drained by the consumer.
    pub(crate) answers_held: Bound,
}

/// What the asker's going does to its request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cancellation {
    /// The call cannot be interrupted; the target incarnation that asked is gone, and its answer
    /// is dropped when it arrives, never raised in another.
    Abandon,
    /// The target is the application, which does not go: there is nothing to abandon, and what
    /// the consumer must refuse is an answer older than the one it has adopted.
    Never,
}

/// **One lane's declared policy.** Each field is checked by the claim that reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Contract {
    pub(crate) lane: LaneName,
    pub(crate) replacement: Replacement,
    pub(crate) execution: Order,
    pub(crate) delivery: Delivery,
    pub(crate) bounds: Bounds,
    pub(crate) cancellation: Cancellation,
}

/// `handoff_lane`: one `bt-os-handoff` thread; ids minted by `HandoffLane::submit`, unique within
/// one lane (a counter `start` resets; the product starts one lane for the application's life);
/// the asking window's `Pending` is the target incarnation; 32 may wait behind the one running,
/// and a press past that is refused with `LANE_FULL` through the same drain.
pub(crate) const HANDOFF: Contract = Contract {
    lane: LaneName::Handoff,
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::RefusalsThenCompletions,
    bounds: Bounds {
        // `handoff_lane::CAPACITY`, written out rather than read, so that a change to either the
        // declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(32),
        executing: 1,
        answers_held: Bound::Unbounded,
    },
    cancellation: Cancellation::Abandon,
};

/// `settings::FontLane`: numbered requests (`ScanState::requested`); one walk out and one `again`
/// round; the answer offered in a one-answer mailbox and adopted only if not older than the adopted
/// generation; the target is the application-wide slot.
pub(crate) const FONT: Contract = Contract {
    lane: LaneName::Font,
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        answers_held: Bound::At(1),
    },
    cancellation: Cancellation::Never,
};

/// `taskbar_lane`: numbered requests (`Asks::requested`); one worker that serves the newest
/// request standing; the answer in a `bt_platform::TaskbarState` slot that takes only newer
/// numbers; the target is the application.
pub(crate) const TASKBAR: Contract = Contract {
    lane: LaneName::Taskbar,
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        answers_held: Bound::At(1),
    },
    cancellation: Cancellation::Never,
};

/// `MathWorker`'s decoration thread (`bt-math-worker`): one question, one answer, keyed by the
/// question; an unbounded `mpsc::channel` each way; the answer's tab is its target
/// (`MathWorkerResult::owner`, `claimed_by`), and a closed tab's answer is dropped. The answer
/// sender is shared with the scaling and path-verification threads.
pub(crate) const COMPUTATION: Contract = Contract {
    lane: LaneName::Computation,
    replacement: Replacement::PerQuestion,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        waiting: Bound::Unbounded,
        executing: 1,
        answers_held: Bound::Unbounded,
    },
    cancellation: Cancellation::Abandon,
};

/// The obligations of §R-D, one each.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum Claim {
    /// With the executor held, a flood of requests never makes the asker wait, and the lane admits
    /// exactly its declared bound: a bounded queue refuses the rest with a terminal, a latest-value
    /// lane runs at most its declared rounds and answers the newest request.
    FullLaneAnswersWithoutWaiting,
    /// The same question asked twice is two requests whose answers can be told apart.
    EveryRequestHasItsOwnIdentity,
    /// Execution and delivery follow the declared order.
    OrderIsAsDeclared,
    /// An answer whose target has gone is raised in no other target; an answer older than the one
    /// adopted is not raised.
    AnAbandonedOrOlderAnswerIsNotRaised,
    /// Every admitted request reaches exactly one terminal outcome the consumer can see.
    EveryRequestEndsExactlyOnce,
    /// A wake finds its answer already published, and an answer whose wake was lost is found by
    /// the next drain.
    TheWakeFollowsThePublication,
    /// Answers published while the consumer does not drain are bounded as declared.
    AnswersHeldAreBounded,
    /// A worker that dies leaves every admitted request a terminal outcome, or the lane a fault the
    /// consumer can see.
    ADeadWorkerIsObservable,
}

impl Claim {
    pub(crate) const ALL: [Self; 8] = [
        Self::FullLaneAnswersWithoutWaiting,
        Self::EveryRequestHasItsOwnIdentity,
        Self::OrderIsAsDeclared,
        Self::AnAbandonedOrOlderAnswerIsNotRaised,
        Self::EveryRequestEndsExactlyOnce,
        Self::TheWakeFollowsThePublication,
        Self::AnswersHeldAreBounded,
        Self::ADeadWorkerIsObservable,
    ];

    fn check(self, run: &mut Run<'_>) -> Result<(), Failure> {
        match self {
            Self::FullLaneAnswersWithoutWaiting => full_lane_answers_without_waiting(run),
            Self::EveryRequestHasItsOwnIdentity => every_request_has_its_own_identity(run),
            Self::OrderIsAsDeclared => order_is_as_declared(run),
            Self::AnAbandonedOrOlderAnswerIsNotRaised => an_abandoned_or_older_answer(run),
            Self::EveryRequestEndsExactlyOnce => every_request_ends_exactly_once(run),
            Self::TheWakeFollowsThePublication => the_wake_follows_the_publication(run),
            Self::AnswersHeldAreBounded => answers_held_are_bounded(run),
            Self::ADeadWorkerIsObservable => a_dead_worker_is_observable(run),
        }
    }
}

/// **How a claim failed**, with what was seen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    /// The executor never started a request the suite put on the lane.
    NeverStarted,
    /// An answer that should have arrived within the suite's patience did not.
    NoAnswer,
    /// A submission blocked the asker.
    SubmissionWaited { longest_ms: u128 },
    /// The lane admitted or ran more (or fewer) than its declared bound.
    BoundBroken { declared: usize, observed: usize },
    /// A lane with no admission bound took every request of the flood and refused none.
    AdmissionUnbounded { admitted: usize },
    /// A lane declared without a bound turned out to keep one.
    DeclaredUnboundedButBounded { observed: usize },
    /// Two requests were given one identity.
    IdentityRepeated { ticket: u64 },
    /// The lane mints no identity: the answers to one question asked twice cannot be told apart.
    IdentityAmbiguous { answers: usize },
    /// Answers or executions arrived out of the declared order.
    OrderBroken {
        expected: Vec<u64>,
        actual: Vec<u64>,
    },
    /// An answer whose target had gone was taken by another target.
    RaisedInAnotherTarget { target: u32 },
    /// An answer older than the adopted one was raised.
    StaleAnswerRaised { key: u64 },
    /// Admitted requests that no terminal outcome ever reached.
    NoTerminal { requests: usize },
    /// A request that reached two terminal outcomes.
    MoreThanOneTerminal { key: u64, terminals: usize },
    /// A published answer never woke the consumer.
    NoWake,
    /// The consumer was woken before the answer it was woken for could be drained.
    WakeBeforePublication,
    /// An answer whose wake was lost was not found by the next drain.
    AnswerLostWithTheWake,
    /// Every answer published while the consumer did not drain was still held.
    AnswersUnbounded { held: usize },
    /// The worker died and neither its admitted requests nor the lane showed it.
    DeathUnobservable { unanswered: usize },
}

/// A [`Failure`]'s kind, which is what [`EXPECTED_FAILURES`] pins: the counts beside it depend on
/// the flood's size and are reported, not matched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailureKind {
    NeverStarted,
    NoAnswer,
    SubmissionWaited,
    BoundBroken,
    AdmissionUnbounded,
    DeclaredUnboundedButBounded,
    IdentityRepeated,
    IdentityAmbiguous,
    OrderBroken,
    RaisedInAnotherTarget,
    StaleAnswerRaised,
    NoTerminal,
    MoreThanOneTerminal,
    NoWake,
    WakeBeforePublication,
    AnswerLostWithTheWake,
    AnswersUnbounded,
    DeathUnobservable,
}

impl Failure {
    pub(crate) fn kind(&self) -> FailureKind {
        match self {
            Self::NeverStarted => FailureKind::NeverStarted,
            Self::NoAnswer => FailureKind::NoAnswer,
            Self::SubmissionWaited { .. } => FailureKind::SubmissionWaited,
            Self::BoundBroken { .. } => FailureKind::BoundBroken,
            Self::AdmissionUnbounded { .. } => FailureKind::AdmissionUnbounded,
            Self::DeclaredUnboundedButBounded { .. } => FailureKind::DeclaredUnboundedButBounded,
            Self::IdentityRepeated { .. } => FailureKind::IdentityRepeated,
            Self::IdentityAmbiguous { .. } => FailureKind::IdentityAmbiguous,
            Self::OrderBroken { .. } => FailureKind::OrderBroken,
            Self::RaisedInAnotherTarget { .. } => FailureKind::RaisedInAnotherTarget,
            Self::StaleAnswerRaised { .. } => FailureKind::StaleAnswerRaised,
            Self::NoTerminal { .. } => FailureKind::NoTerminal,
            Self::MoreThanOneTerminal { .. } => FailureKind::MoreThanOneTerminal,
            Self::NoWake => FailureKind::NoWake,
            Self::WakeBeforePublication => FailureKind::WakeBeforePublication,
            Self::AnswerLostWithTheWake => FailureKind::AnswerLostWithTheWake,
            Self::AnswersUnbounded { .. } => FailureKind::AnswersUnbounded,
            Self::DeathUnobservable { .. } => FailureKind::DeathUnobservable,
        }
    }
}

/// **A claim a lane is known to fail**, the exact kind of the failure, and the ledger row that
/// repairs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExpectedFailure {
    pub(crate) lane: LaneName,
    pub(crate) claim: Claim,
    pub(crate) failure: FailureKind,
    /// The `docs/plans/structural-debt.md` row.
    pub(crate) repair: &'static str,
    pub(crate) why: &'static str,
}

/// **The lanes' declared failures** — data, not `#[ignore]`. Removing a row while its failure
/// stands is red (an undeclared failure); repairing a lane without removing its row is red too (an
/// unexpected pass).
pub(crate) const EXPECTED_FAILURES: &[ExpectedFailure] = &[
    ExpectedFailure {
        lane: LaneName::Handoff,
        claim: Claim::AnswersHeldAreBounded,
        failure: FailureKind::AnswersUnbounded,
        repair: "D-71",
        why: "the answer channel is an unbounded mpsc::channel, and turned_away a Vec",
    },
    ExpectedFailure {
        lane: LaneName::Handoff,
        claim: Claim::ADeadWorkerIsObservable,
        failure: FailureKind::DeathUnobservable,
        repair: "D-70",
        why: "answers() reads a disconnected channel as empty and LANE_GONE answers only a new \
              submission, so the requests a dead worker had accepted stay owed in Pending",
    },
    ExpectedFailure {
        lane: LaneName::Font,
        claim: Claim::EveryRequestEndsExactlyOnce,
        failure: FailureKind::NoTerminal,
        repair: "D-72",
        why: "a request coalesced into the again round, or superseded, gets no outcome of its own",
    },
    ExpectedFailure {
        lane: LaneName::Font,
        claim: Claim::ADeadWorkerIsObservable,
        failure: FailureKind::DeathUnobservable,
        repair: "D-72",
        why: "a walk that dies leaves ScanState running for ever: no fault, and no later walk",
    },
    ExpectedFailure {
        lane: LaneName::Taskbar,
        claim: Claim::EveryRequestEndsExactlyOnce,
        failure: FailureKind::NoTerminal,
        repair: "D-73",
        why: "the worker serves the newest request standing; the ones between get no outcome",
    },
    ExpectedFailure {
        lane: LaneName::Taskbar,
        claim: Claim::ADeadWorkerIsObservable,
        failure: FailureKind::DeathUnobservable,
        repair: "D-73",
        why: "a worker that dies leaves Asks::worker set: requests are counted and never served",
    },
    ExpectedFailure {
        lane: LaneName::Computation,
        claim: Claim::FullLaneAnswersWithoutWaiting,
        failure: FailureKind::AdmissionUnbounded,
        repair: "D-74",
        why: "MathWorker::spawn's request channel is an unbounded mpsc::channel",
    },
    ExpectedFailure {
        lane: LaneName::Computation,
        claim: Claim::AnswersHeldAreBounded,
        failure: FailureKind::AnswersUnbounded,
        repair: "D-74",
        why: "the answer channel the three threads share is an unbounded mpsc::channel",
    },
    ExpectedFailure {
        lane: LaneName::Computation,
        claim: Claim::EveryRequestHasItsOwnIdentity,
        failure: FailureKind::IdentityAmbiguous,
        repair: "D-75",
        why: "the lane mints no request id; an answer carries only its question",
    },
    ExpectedFailure {
        lane: LaneName::Computation,
        claim: Claim::ADeadWorkerIsObservable,
        failure: FailureKind::DeathUnobservable,
        repair: "D-76",
        why: "the scaling and path-verification threads hold clones of the answer sender, so the \
              drain never sees the decoration thread's death as a disconnection",
    },
];

/// **One claim, run on one lane**: what it found and how many requests it put on the lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Verdict {
    pub(crate) lane: LaneName,
    pub(crate) claim: Claim,
    pub(crate) requests: usize,
    pub(crate) result: Result<(), Failure>,
}

/// **What the suite asks of a lane.** One adapter per lane, each driving the lane's own admission,
/// publication and acceptance; only the executor (through [`Gate`]) and the wake (through
/// [`WakeProbe`]) are the suite's, so it can hold the one and watch the other.
pub(crate) trait LaneUnderTest {
    fn contract(&self) -> &'static Contract;
    /// The executor's gate. The adapter calls [`Gate::pass`] where the lane's worker executes a
    /// request (or, for a lane whose executor cannot be stood in for, in the worker's wake).
    fn gate(&self) -> &Gate;
    /// The wake's probe. The adapter's wake calls [`WakeProbe::woke`] where the lane wakes the
    /// event loop.
    fn probe(&self) -> &WakeProbe;
    /// Put request `question` on the lane for `target` through the lane's own admission. Returns
    /// at once — how long it took is the suite's to measure.
    fn submit(&mut self, target: u32, question: u64) -> Admission;
    /// The target incarnation `target` goes (a window closes, a tab closes).
    fn close_target(&mut self, target: u32);
    /// What the consumer finds now, through the lane's own acceptance. Never waits.
    fn drain(&mut self) -> Vec<Delivered>;
    /// Publish, through the lane's own publication, an answer to the older request `ticket` — as
    /// a second asker or a retried one would. Only asked of a lane whose target is the application.
    fn offer_stale(&mut self, ticket: u64);
}

/// **An adapter**, named by the lane it drives; `make` starts a fresh lane for each claim.
pub(crate) struct Adapter {
    pub(crate) lane: LaneName,
    pub(crate) make: fn() -> Box<dyn LaneUnderTest>,
}

/// What admission said.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Admission {
    /// The identity the lane gave the request, if it gives one.
    pub(crate) ticket: Option<u64>,
    pub(crate) question: u64,
    /// A refusal the lane gave at the door, synchronously. (A refusal delivered through the drain
    /// is a [`Delivered`].)
    pub(crate) refused: Option<String>,
}

impl Admission {
    /// The name the suite tallies this request under: the lane's identity if it gives one, else
    /// the question.
    fn key(&self) -> u64 {
        self.ticket.unwrap_or(self.question)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Answered,
    Refused(String),
    /// The lane says it can no longer answer.
    Fault(String),
}

/// One thing the consumer found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Delivered {
    /// The target that took it; `None` when no open target did (it was dropped).
    pub(crate) target: Option<u32>,
    pub(crate) ticket: Option<u64>,
    pub(crate) question: Option<u64>,
    pub(crate) outcome: Outcome,
}

impl Delivered {
    fn key(&self) -> Option<u64> {
        self.ticket.or(self.question)
    }
}

/// How long the suite waits for something that must happen.
const PATIENCE: Duration = Duration::from_secs(10);
/// How long it watches before concluding that something does not happen.
const QUIET: Duration = Duration::from_millis(300);
/// A submission slower than this made the asker wait.
const PROMPT: Duration = Duration::from_secs(1);
/// A held executor lets go after this on its own, so a submission that blocks on it comes back
/// (slow, and caught) instead of hanging the suite.
const HOLD_LIMIT: Duration = Duration::from_secs(5);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// **The executor's gate**: the suite holds it, counts what passed it, and can make the worker die
/// in it.
#[derive(Default)]
pub(crate) struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    held: bool,
    kill: bool,
    killed: bool,
    entered: usize,
    executed: Vec<u64>,
}

impl Gate {
    /// **Called by the worker, where it executes a request** (`question` when the executor can
    /// see which). Waits while the gate is held, up to [`HOLD_LIMIT`]; then, if the suite asked for
    /// the worker's death, panics — which ends the lane's thread as a fault inside its executor
    /// would.
    pub(crate) fn pass(&self, question: Option<u64>) {
        let mut state = lock(&self.state);
        state.entered += 1;
        state.executed.extend(question);
        self.changed.notify_all();
        let since = Instant::now();
        while state.held && since.elapsed() < HOLD_LIMIT {
            state = self
                .changed
                .wait_timeout(state, HOLD_LIMIT)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if state.kill {
            state.killed = true;
            self.changed.notify_all();
            drop(state);
            panic!("the lane contract suite terminates this worker on purpose");
        }
    }

    pub(crate) fn hold(&self) {
        lock(&self.state).held = true;
    }

    pub(crate) fn release(&self) {
        lock(&self.state).held = false;
        self.changed.notify_all();
    }

    /// The next request to pass (or the one held now) ends the worker.
    fn kill(&self) {
        lock(&self.state).kill = true;
    }

    fn entered(&self) -> usize {
        lock(&self.state).entered
    }

    fn executed(&self) -> Vec<u64> {
        lock(&self.state).executed.clone()
    }

    fn wait_for(&self, done: impl Fn(&GateState) -> bool) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while !done(&state) {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// Whether `count` requests have reached the executor within the suite's patience.
    pub(crate) fn wait_entered(&self, count: usize) -> bool {
        self.wait_for(|state| state.entered >= count)
    }

    fn wait_killed(&self) -> bool {
        self.wait_for(|state| state.killed)
    }
}

/// **The wake's probe**: counts wakes, can lose them, and can park one on the worker's thread so
/// the suite drains while the worker is still inside its wake.
#[derive(Default)]
pub(crate) struct WakeProbe {
    state: Mutex<WakeState>,
    changed: Condvar,
}

#[derive(Default)]
struct WakeState {
    wakes: usize,
    lost: bool,
    /// The suite's thread, when a rendezvous is armed: a wake on any other thread parks.
    rendezvous: Option<ThreadId>,
    parked: bool,
}

impl WakeProbe {
    /// **Called by the lane's wake.**
    pub(crate) fn woke(&self) {
        let mut state = lock(&self.state);
        state.wakes += 1;
        self.changed.notify_all();
        if state.lost {
            return;
        }
        let here = std::thread::current().id();
        if state.rendezvous.is_some_and(|suite| suite != here) {
            state.rendezvous = None;
            state.parked = true;
            self.changed.notify_all();
            let since = Instant::now();
            while state.parked && since.elapsed() < PATIENCE {
                state = self
                    .changed
                    .wait_timeout(state, PATIENCE)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    fn wakes(&self) -> usize {
        lock(&self.state).wakes
    }

    fn arm_rendezvous(&self) {
        lock(&self.state).rendezvous = Some(std::thread::current().id());
    }

    fn wait_parked(&self) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while !state.parked {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    fn let_go(&self) {
        let mut state = lock(&self.state);
        state.parked = false;
        state.rendezvous = None;
        self.changed.notify_all();
    }

    fn lose_wakes(&self) {
        lock(&self.state).lost = true;
    }

    fn wait_wakes_past(&self, count: usize) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while state.wakes <= count {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// One claim's run: the lane, and the count of what it was asked.
struct Run<'a> {
    lane: &'a mut dyn LaneUnderTest,
    requests: usize,
}

impl Run<'_> {
    fn contract(&self) -> &'static Contract {
        self.lane.contract()
    }

    fn submit(&mut self, target: u32, question: u64) -> (Admission, Duration) {
        self.requests += 1;
        let started = Instant::now();
        let admission = self.lane.submit(target, question);
        (admission, started.elapsed())
    }

    /// Drain until `done` holds for everything found so far, or [`PATIENCE`] runs out.
    fn settle(&mut self, found: &mut Vec<Delivered>, done: impl Fn(&[Delivered]) -> bool) -> bool {
        let since = Instant::now();
        loop {
            found.extend(self.lane.drain());
            if done(found) {
                return true;
            }
            if since.elapsed() > PATIENCE {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Drain for [`QUIET`], adding whatever arrives.
    fn watch(&mut self, found: &mut Vec<Delivered>) {
        let since = Instant::now();
        while since.elapsed() < QUIET {
            found.extend(self.lane.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        found.extend(self.lane.drain());
    }

    /// Hold the executor and put request `0` on it, waiting until it has started.
    fn hold_one(&mut self) -> Result<Admission, Failure> {
        self.lane.gate().hold();
        let (first, _) = self.submit(0, 0);
        if self.lane.gate().wait_entered(1) {
            Ok(first)
        } else {
            self.lane.gate().release();
            Err(Failure::NeverStarted)
        }
    }
}

/// Everything found that answers `key`.
fn terminals(found: &[Delivered], key: u64) -> usize {
    found
        .iter()
        .filter(|delivered| delivered.key() == Some(key))
        .count()
}

fn has_key(found: &[Delivered], key: u64) -> bool {
    terminals(found, key) > 0
}

/// Whether every admitted request not refused at the door has a terminal among `found`.
fn every_admission_ended(admissions: &[Admission], found: &[Delivered]) -> bool {
    admissions
        .iter()
        .filter(|admission| admission.refused.is_none())
        .all(|admission| has_key(found, admission.key()))
}

fn full_lane_answers_without_waiting(run: &mut Run<'_>) -> Result<(), Failure> {
    let contract = run.contract();
    let first = run.hold_one()?;
    let flood = match contract.bounds.waiting {
        Bound::At(declared) => declared + 4,
        Bound::Unbounded => 64,
    };
    let mut admissions = vec![first];
    let mut longest = Duration::ZERO;
    for question in 1..=flood as u64 {
        let (admission, took) = run.submit(0, question);
        longest = longest.max(took);
        admissions.push(admission);
    }
    // What the consumer hears while the executor is still held: the refusals, if the lane gives
    // them.
    let while_held = run.lane.drain();
    run.lane.gate().release();
    if longest > PROMPT {
        return Err(Failure::SubmissionWaited {
            longest_ms: longest.as_millis(),
        });
    }
    let mut found = while_held;
    match contract.replacement {
        Replacement::Fifo | Replacement::PerQuestion => {
            let refused = admissions
                .iter()
                .filter(|admission| admission.refused.is_some())
                .count()
                + found
                    .iter()
                    .filter(|delivered| matches!(delivered.outcome, Outcome::Refused(_)))
                    .count();
            let admitted = flood - refused;
            if !run.settle(&mut found, |found| {
                every_admission_ended(&admissions, found)
            }) {
                return Err(Failure::NoAnswer);
            }
            match contract.bounds.waiting {
                Bound::At(declared) if admitted != declared => Err(Failure::BoundBroken {
                    declared,
                    observed: admitted,
                }),
                Bound::At(_) => Ok(()),
                Bound::Unbounded if refused == 0 => Err(Failure::AdmissionUnbounded { admitted }),
                Bound::Unbounded => {
                    Err(Failure::DeclaredUnboundedButBounded { observed: admitted })
                }
            }
        }
        Replacement::LatestValue => {
            let newest = admissions.last().map_or(0, Admission::key);
            if !run.settle(&mut found, |found| has_key(found, newest)) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            let rounds = run.lane.gate().entered();
            let allowed = contract.bounds.executing
                + match contract.bounds.waiting {
                    Bound::At(declared) => declared,
                    Bound::Unbounded => flood,
                };
            if rounds > allowed {
                return Err(Failure::BoundBroken {
                    declared: allowed,
                    observed: rounds,
                });
            }
            Ok(())
        }
    }
}

fn every_request_has_its_own_identity(run: &mut Run<'_>) -> Result<(), Failure> {
    let (first, _) = run.submit(0, 7);
    let (second, _) = run.submit(0, 7);
    let mut found = Vec::new();
    let settled = match run.contract().replacement {
        Replacement::LatestValue => {
            let newest = second.key();
            run.settle(&mut found, |found| has_key(found, newest))
        }
        Replacement::Fifo | Replacement::PerQuestion => {
            run.settle(&mut found, |found| found.len() >= 2)
        }
    };
    if !settled {
        return Err(Failure::NoAnswer);
    }
    match (first.ticket, second.ticket) {
        (Some(one), Some(other)) if one == other => Err(Failure::IdentityRepeated { ticket: one }),
        (Some(one), Some(other)) => {
            if found
                .iter()
                .all(|delivered| delivered.ticket == Some(one) || delivered.ticket == Some(other))
            {
                Ok(())
            } else {
                Err(Failure::IdentityAmbiguous {
                    answers: found.len(),
                })
            }
        }
        _ => Err(Failure::IdentityAmbiguous {
            answers: found.len(),
        }),
    }
}

fn order_is_as_declared(run: &mut Run<'_>) -> Result<(), Failure> {
    let contract = run.contract();
    let first = run.hold_one()?;
    let more = match (contract.replacement, contract.bounds.waiting) {
        (Replacement::LatestValue, _) => 2,
        // One past the bound, so the declared delivery order meets a refusal.
        (_, Bound::At(declared)) => declared + 1,
        (_, Bound::Unbounded) => 5,
    };
    let mut admissions = vec![first];
    for question in 1..=more as u64 {
        admissions.push(run.submit(0, question).0);
    }
    run.lane.gate().release();
    let mut found = Vec::new();
    match contract.replacement {
        Replacement::LatestValue => {
            let newest = admissions.last().map_or(0, Admission::key);
            if !run.settle(&mut found, |found| has_key(found, newest)) {
                return Err(Failure::NoAnswer);
            }
            let delivered: Vec<u64> = found.iter().filter_map(Delivered::key).collect();
            let rising = delivered.windows(2).all(|pair| pair[0] < pair[1]);
            if contract.delivery != Delivery::NewestAdopted
                || !rising
                || delivered.last() != Some(&newest)
            {
                return Err(Failure::OrderBroken {
                    expected: vec![newest],
                    actual: delivered,
                });
            }
            let executed = run.lane.gate().executed();
            let newest_last = executed.windows(2).all(|pair| pair[0] < pair[1])
                && executed.last().is_none_or(|last| *last == newest);
            if contract.execution != Order::NewestRequested || !newest_last {
                return Err(Failure::OrderBroken {
                    expected: vec![newest],
                    actual: executed,
                });
            }
            Ok(())
        }
        Replacement::Fifo | Replacement::PerQuestion => {
            let all_ended = |found: &[Delivered]| {
                every_admission_ended(&admissions, found) && found.len() >= admissions.len()
            };
            if !run.settle(&mut found, all_ended) {
                return Err(Failure::NoAnswer);
            }
            let questions = |refused: bool| -> Vec<u64> {
                found
                    .iter()
                    .filter(|delivered| matches!(delivered.outcome, Outcome::Refused(_)) == refused)
                    .filter_map(|delivered| delivered.question)
                    .collect()
            };
            let actual: Vec<u64> = found
                .iter()
                .filter_map(|delivered| delivered.question)
                .collect();
            let expected: Vec<u64> = match contract.delivery {
                Delivery::RefusalsThenCompletions => {
                    let mut refusals = questions(true);
                    refusals.sort_unstable();
                    let mut answered = questions(false);
                    answered.sort_unstable();
                    refusals.into_iter().chain(answered).collect()
                }
                Delivery::Submission => {
                    let mut all = actual.clone();
                    all.sort_unstable();
                    all
                }
                // A lane that answers every request cannot deliver only the newest.
                Delivery::NewestAdopted => Vec::new(),
            };
            if actual != expected {
                return Err(Failure::OrderBroken { expected, actual });
            }
            let executed = run.lane.gate().executed();
            let mut answered = questions(false);
            answered.sort_unstable();
            if contract.execution != Order::Submission
                || (!executed.is_empty() && executed != answered)
            {
                return Err(Failure::OrderBroken {
                    expected: answered,
                    actual: executed,
                });
            }
            Ok(())
        }
    }
}

fn an_abandoned_or_older_answer(run: &mut Run<'_>) -> Result<(), Failure> {
    match run.contract().cancellation {
        Cancellation::Abandon => {
            let gone = run.hold_one()?;
            let (kept, _) = run.submit(1, 1);
            run.lane.close_target(0);
            run.lane.gate().release();
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| {
                has_key(found, gone.key()) && has_key(found, kept.key())
            }) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            for delivered in &found {
                if delivered.key() == Some(gone.key())
                    && let Some(target) = delivered.target
                {
                    return Err(Failure::RaisedInAnotherTarget { target });
                }
            }
            if found
                .iter()
                .any(|delivered| delivered.key() == Some(kept.key()) && delivered.target != Some(1))
            {
                return Err(Failure::RaisedInAnotherTarget { target: 1 });
            }
            Ok(())
        }
        Cancellation::Never => {
            let (older, _) = run.submit(0, 0);
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| has_key(found, older.key())) {
                return Err(Failure::NoAnswer);
            }
            let (newer, _) = run.submit(0, 1);
            if !run.settle(&mut found, |found| has_key(found, newer.key())) {
                return Err(Failure::NoAnswer);
            }
            run.lane.offer_stale(older.key());
            let mut after = Vec::new();
            run.watch(&mut after);
            match after.first() {
                Some(raised) => Err(Failure::StaleAnswerRaised {
                    key: raised.key().unwrap_or_default(),
                }),
                None => Ok(()),
            }
        }
    }
}

fn every_request_ends_exactly_once(run: &mut Run<'_>) -> Result<(), Failure> {
    let first = run.hold_one()?;
    let mut admissions = vec![first];
    for question in 1..=5 {
        admissions.push(run.submit(0, question).0);
    }
    run.lane.gate().release();
    let mut found = Vec::new();
    let newest = admissions.last().map_or(0, Admission::key);
    let settled = match run.contract().replacement {
        Replacement::LatestValue => run.settle(&mut found, |found| has_key(found, newest)),
        Replacement::Fifo | Replacement::PerQuestion => run.settle(&mut found, |found| {
            every_admission_ended(&admissions, found)
        }),
    };
    if !settled {
        return Err(Failure::NoAnswer);
    }
    run.watch(&mut found);
    let mut counts: BTreeMap<u64, usize> = BTreeMap::new();
    for admission in &admissions {
        let at_door = usize::from(admission.refused.is_some());
        counts.insert(
            admission.key(),
            at_door + terminals(&found, admission.key()),
        );
    }
    if let Some((key, terminals)) = counts.iter().find(|(_, count)| **count > 1) {
        return Err(Failure::MoreThanOneTerminal {
            key: *key,
            terminals: *terminals,
        });
    }
    let unanswered = counts.values().filter(|count| **count == 0).count();
    if unanswered > 0 {
        return Err(Failure::NoTerminal {
            requests: unanswered,
        });
    }
    Ok(())
}

fn the_wake_follows_the_publication(run: &mut Run<'_>) -> Result<(), Failure> {
    // The worker parks inside its wake; what the consumer drains then was published before it.
    run.lane.probe().arm_rendezvous();
    let (first, _) = run.submit(0, 0);
    if !run.lane.probe().wait_parked() {
        run.lane.probe().let_go();
        return Err(Failure::NoWake);
    }
    let seen = run.lane.drain();
    run.lane.probe().let_go();
    if !has_key(&seen, first.key()) {
        return Err(Failure::WakeBeforePublication);
    }
    // A wake that reaches nobody: the next drain, whatever brings it, still finds the answer.
    run.lane.probe().lose_wakes();
    let before = run.lane.probe().wakes();
    let (second, _) = run.submit(0, 1);
    if !run.lane.probe().wait_wakes_past(before) {
        return Err(Failure::NoWake);
    }
    let seen = run.lane.drain();
    if !has_key(&seen, second.key()) {
        return Err(Failure::AnswerLostWithTheWake);
    }
    Ok(())
}

fn answers_held_are_bounded(run: &mut Run<'_>) -> Result<(), Failure> {
    let declared = run.contract().bounds.answers_held;
    let asked = match declared {
        Bound::At(bound) => bound + 8,
        Bound::Unbounded => 40,
    };
    for question in 0..asked as u64 {
        let before = run.lane.probe().wakes();
        run.submit(0, question);
        if !run.lane.probe().wait_wakes_past(before) {
            return Err(Failure::NoWake);
        }
    }
    let held = run.lane.drain().len();
    match declared {
        Bound::At(bound) if held > bound => Err(Failure::BoundBroken {
            declared: bound,
            observed: held,
        }),
        Bound::At(_) => Ok(()),
        Bound::Unbounded if held >= asked => Err(Failure::AnswersUnbounded { held }),
        Bound::Unbounded => Err(Failure::DeclaredUnboundedButBounded { observed: held }),
    }
}

fn a_dead_worker_is_observable(run: &mut Run<'_>) -> Result<(), Failure> {
    let first = run.hold_one()?;
    let mut admissions = vec![first];
    for question in 1..=2 {
        admissions.push(run.submit(0, question).0);
    }
    run.lane.gate().kill();
    run.lane.gate().release();
    if !run.lane.gate().wait_killed() {
        return Err(Failure::NeverStarted);
    }
    let mut found = Vec::new();
    run.watch(&mut found);
    if found
        .iter()
        .any(|delivered| matches!(delivered.outcome, Outcome::Fault(_)))
    {
        return Ok(());
    }
    let unanswered = admissions
        .iter()
        .filter(|admission| admission.refused.is_none() && !has_key(&found, admission.key()))
        .count();
    if unanswered > 0 {
        return Err(Failure::DeathUnobservable { unanswered });
    }
    Ok(())
}

/// **Run every claim on one lane**, each on a fresh lane from the adapter.
pub(crate) fn run_suite(adapter: &Adapter, claims: &[Claim]) -> Vec<Verdict> {
    claims
        .iter()
        .map(|&claim| {
            let mut lane = (adapter.make)();
            assert_eq!(
                lane.contract().lane,
                adapter.lane,
                "the {:?} adapter reports another lane's contract",
                adapter.lane
            );
            let mut run = Run {
                lane: lane.as_mut(),
                requests: 0,
            };
            let result = claim.check(&mut run);
            let requests = run.requests;
            // Whatever the claim left held is let go before the lane is dropped.
            lane.gate().release();
            lane.probe().let_go();
            Verdict {
                lane: adapter.lane,
                claim,
                requests,
                result,
            }
        })
        .collect()
}

/// **What is wrong with a run of the suite**, as sentences — empty when nothing is.
///
/// `lanes` and `claims` are what the run was meant to cover: a verdict missing for any pair of
/// them is a skipped adapter. Every verdict must have exercised at least one request. A verdict
/// passes only if [`EXPECTED_FAILURES`] (here `expected`) holds no row for it, and fails only with
/// the kind its row names.
pub(crate) fn judge(
    verdicts: &[Verdict],
    expected: &[ExpectedFailure],
    lanes: &[LaneName],
    claims: &[Claim],
) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, row) in expected.iter().enumerate() {
        if expected[..index]
            .iter()
            .any(|earlier| earlier.lane == row.lane && earlier.claim == row.claim)
        {
            problems.push(format!(
                "{:?} × {:?} is declared twice in the expected failures",
                row.lane, row.claim
            ));
        }
        if row
            .repair
            .strip_prefix("D-")
            .is_none_or(|number| number.parse::<u32>().is_err())
        {
            problems.push(format!(
                "{:?} × {:?} names no ledger row as its repair: {:?}",
                row.lane, row.claim, row.repair
            ));
        }
    }
    for &lane in lanes {
        for &claim in claims {
            let found: Vec<&Verdict> = verdicts
                .iter()
                .filter(|verdict| verdict.lane == lane && verdict.claim == claim)
                .collect();
            let [verdict] = found.as_slice() else {
                problems.push(format!(
                    "{lane:?} × {claim:?} was run {} times, not once: a skipped adapter or claim",
                    found.len()
                ));
                continue;
            };
            if verdict.requests == 0 {
                problems.push(format!("{lane:?} × {claim:?} exercised no request"));
            }
            let row = expected
                .iter()
                .find(|row| row.lane == lane && row.claim == claim);
            match (&verdict.result, row) {
                (Ok(()), None) => {}
                (Ok(()), Some(row)) => problems.push(format!(
                    "{lane:?} × {claim:?} passed, but is declared to fail with {:?} ({}; repair \
                     {}): an unexpected pass — the repair has landed, or the claim no longer \
                     reaches the lane",
                    row.failure, row.why, row.repair
                )),
                (Err(failure), None) => problems.push(format!(
                    "{lane:?} × {claim:?} failed with {failure:?}, and no expected failure \
                     declares it"
                )),
                (Err(failure), Some(row)) if failure.kind() != row.failure => {
                    problems.push(format!(
                        "{lane:?} × {claim:?} failed with {failure:?}, a different failure from \
                         the declared {:?} (repair {})",
                        row.failure, row.repair
                    ));
                }
                (Err(_), Some(_)) => {}
            }
        }
    }
    problems
}
