//! **The lane contract's suite, run on every lane** (D-33; `crate::lane`; `docs/plans/design/
//! window-thread-budget-2026-09-25.md` §R-D).
//!
//! Five adapters — the OS hand-off lane (the reference), the font lane, the taskbar lane, the
//! computation lane and the program-walk lane — each run every [`Claim`]; [`judge`] holds the
//! verdicts to [`EXPECTED_FAILURES`]. The computation lane's adapter lives here because its lane
//! lives in `main.rs`; the others live beside their lanes, where the lanes' private parts are.
//!
//! The seam the lanes will be built on (`crate::lane`'s `Executor`, `Ledger` and hooks; S-3) is
//! proven here too: the ledger alone over every interleaving of three identities, the executor's
//! two ends (a task's panic, the worker's), and lanes the suite builds on the seam — each new
//! claim passed by one and failed by one planted to break it.

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};

use super::*;
use crate::lane::{
    Adapter, Admission, Bound, Bounds, COMPUTATION, Cancellation, Claim, Contract, Delivered,
    Delivery, EXPECTED_FAILURES, Executor, ExpectedFailure, Failure, FailureKind, Gate, HANDOFF,
    LaneName, LaneUnderTest, Ledger, Order, Outcome, Owed, PLANTED_PANIC, Refused, Replacement,
    Request, Saying, TaskFailure, Terminal, Verdict, WakeProbe, WorkerEnd, check, judge, receive,
    run_suite,
};

/// Every lane's adapter. A lane missing here is a lane [`judge`] reports as skipped.
fn adapters() -> [Adapter; 5] {
    [
        Adapter {
            lane: LaneName::Handoff,
            make: crate::handoff_lane::contract_adapter::make,
        },
        Adapter {
            lane: LaneName::Font,
            make: crate::settings::font_lane_adapter::make,
        },
        Adapter {
            lane: LaneName::Taskbar,
            make: crate::taskbar_lane::contract_adapter::make,
        },
        Adapter {
            lane: LaneName::Computation,
            make: computation_adapter,
        },
        Adapter {
            lane: LaneName::Programs,
            make: crate::programs_lane::contract_adapter::make,
        },
    ]
}

/// **The computation lane's adapter**: the real [`MathWorker`] — its three threads, the answer
/// sender they share, and `bt-math-worker`'s queue — with the suite's probe and gate in the wake
/// every thread calls after it publishes. The decoration thread's executor cannot be stood in for
/// (it is `run_decoration_worker`'s own match), so the gate holds the worker **after** it has
/// published, inside its wake: the requests behind it wait all the same.
///
/// A question is a pasted picture whose payload is empty, which the real decoder refuses at once
/// without touching a disk; the answer is still the lane's answer to that question. Acceptance is
/// the product's: `answers_for` with `claimed_by` over the open tabs, as `apply_math_results` asks
/// through `Runtime::owns`, and `disable_math_worker_state` for a disconnected channel.
struct ComputationAdapter {
    worker: MathWorker,
    gate: Arc<Gate>,
    probe: Arc<WakeProbe>,
    opened: BTreeSet<u32>,
    closed: BTreeSet<u32>,
    running: bool,
    notice_pending: bool,
}

/// The one window the adapter's tabs stand in.
fn adapter_window() -> WindowId {
    WindowId::from(1_u64)
}

fn tab_of(target: u32) -> TabId {
    TabId(u64::from(target) + 1)
}

fn computation_adapter() -> Box<dyn LaneUnderTest> {
    let gate = Arc::new(Gate::default());
    let probe = Arc::new(WakeProbe::default());
    let door = Arc::clone(&gate);
    let heard = Arc::clone(&probe);
    let worker = MathWorker::spawn(move || {
        heard.woke();
        door.pass(None);
    })
    .expect("the computation lane starts");
    Box::new(ComputationAdapter {
        worker,
        gate,
        probe,
        opened: BTreeSet::new(),
        closed: BTreeSet::new(),
        running: true,
        notice_pending: false,
    })
}

impl LaneUnderTest for ComputationAdapter {
    fn contract(&self) -> &'static Contract {
        &COMPUTATION
    }

    fn gate(&self) -> &Gate {
        &self.gate
    }

    fn probe(&self) -> &WakeProbe {
        &self.probe
    }

    fn submit(&mut self, target: u32, question: u64) -> Admission {
        // The decode below reaches the host's SVG codec for any payload no raster container
        // claims; install it as `main` does, so the lane's answer never depends on the payload.
        crate::test_support::install_host_answers();
        self.opened.insert(target);
        let leaf = ShellAddress {
            window: adapter_window(),
            leaf: LeafId {
                tab: tab_of(target),
                seat: SeatId(1),
            },
        };
        let request = MathWorkerRequest::InlineImage {
            leaf,
            task: bt_term::InlineImageTask {
                occurrence_id: question,
                source: bt_term::InlineImageSource::Osc1337(Vec::new()),
            },
        };
        // `dispatch_decoration_task`'s admission: a send, which fails only when the thread is gone.
        let refused = self
            .worker
            .tasks
            .send(request)
            .err()
            .map(|_| "the decoration thread has stopped".to_owned());
        Admission {
            ticket: None,
            question,
            refused,
        }
    }

    fn close_target(&mut self, target: u32) {
        self.closed.insert(target);
    }

    fn drain(&mut self) -> Vec<Delivered> {
        let mut batch = Vec::new();
        let mut gone = false;
        loop {
            match self.worker.results.try_recv() {
                Ok(result) => batch.push(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    gone = disable_math_worker_state(&mut self.running, &mut self.notice_pending);
                    break;
                }
            }
        }
        let open: Vec<TabId> = self
            .opened
            .difference(&self.closed)
            .map(|target| tab_of(*target))
            .collect();
        let taken = answers_for(&mut batch, |result| {
            claimed_by(result.owner(), adapter_window(), open.iter().copied())
        });
        let question = |result: &MathWorkerResult| match &result.completion {
            DecorationWorkerCompletion::InlineImage { task, .. } => Some(task.occurrence_id),
            _ => None,
        };
        let mut found: Vec<Delivered> = taken
            .iter()
            .map(|result| Delivered {
                target: u32::try_from(result.leaf.leaf.tab.0 - 1).ok(),
                ticket: None,
                question: question(result),
                outcome: Outcome::Answered,
            })
            .chain(batch.iter().map(|result| Delivered {
                target: None,
                ticket: None,
                question: question(result),
                outcome: Outcome::Answered,
            }))
            .collect();
        if gone {
            found.push(Delivered {
                target: None,
                ticket: None,
                question: None,
                outcome: Outcome::Fault(math_worker_stopped_notice().to_owned()),
            });
        }
        found
    }

    fn offer_stale(&mut self, _ticket: u64) {
        unreachable!("the computation lane's target is the asking tab, not the application");
    }
}

/// Run the suite on every adapter, one thread per lane.
fn run_every_lane(claims: &[Claim]) -> Vec<Verdict> {
    let adapters = adapters();
    std::thread::scope(|scope| {
        let runs: Vec<_> = adapters
            .iter()
            .map(|adapter| scope.spawn(move || run_suite(adapter, claims)))
            .collect();
        runs.into_iter()
            .flat_map(|run| run.join().expect("a lane's suite ran to its end"))
            .collect()
    })
}

fn report(verdicts: &[Verdict]) -> String {
    verdicts
        .iter()
        .map(|verdict| {
            format!(
                "  {:?} × {:?}: {:?} ({} requests)",
                verdict.lane, verdict.claim, verdict.result, verdict.requests
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// RED (A5) — **Every lane runs every claim of the contract, and fails only where the ledger says
/// it does.**
///
/// The lanes grew their policies one at a time, each stated in its own module doc. This runs one
/// suite over all four through their real admission, publication and acceptance, and holds the
/// result to [`EXPECTED_FAILURES`]: a lane that fails a claim with no row, fails it differently
/// from its row, passes a claim its row says it fails, is not run, or is run without a request is
/// red. The rows name the ledger entries that repair them (D-70…D-76), so a repair that lands
/// turns this red until its row is removed.
///
/// MUTATION: `send` instead of `try_send` in `HandoffLane::submit` — the hand-off lane's full
/// queue makes the asker wait (`SubmissionWaited`, an undeclared failure). Or delete the hand-off
/// lane's D-70 row from `EXPECTED_FAILURES` — its dead-worker failure is undeclared.
#[test]
fn every_lane_runs_every_claim_and_fails_only_where_the_ledger_says() {
    let verdicts = run_every_lane(&Claim::ALL);
    let problems = judge(&verdicts, EXPECTED_FAILURES, &LaneName::ALL, &Claim::ALL);
    assert!(
        problems.is_empty(),
        "{}\n\nverdicts:\n{}",
        problems.join("\n"),
        report(&verdicts)
    );
}

/// RED (A5) — **Every lane answers a full queue without waiting and raises no abandoned answer.**
///
/// The two claims the window thread leans on hardest: a press on a busy lane never holds the
/// thread that pressed, and an answer whose asker has gone — a closed window, a closed tab — or
/// that is older than the one adopted is raised nowhere. The computation lane admits without a
/// bound (D-74), which is declared; it still never makes the asker wait.
///
/// MUTATION: make `Pending::claim` answer the first duty it holds whatever the id
/// (`self.owed.drain().next().map(|(_, duty)| duty)`) — the hand-off lane raises a closed window's
/// answer in the other window (`RaisedInAnotherTarget`). Or drop the generation comparison in
/// `MonospaceFamilySlot::adopt` — the font lane raises a late answer (`StaleAnswerRaised`).
#[test]
fn every_lane_answers_a_full_queue_without_waiting_and_raises_no_abandoned_answer() {
    let claims = [
        Claim::FullLaneAnswersWithoutWaiting,
        Claim::AnAbandonedOrOlderAnswerIsNotRaised,
    ];
    let verdicts = run_every_lane(&claims);
    let problems = judge(&verdicts, EXPECTED_FAILURES, &LaneName::ALL, &claims);
    assert!(
        problems.is_empty(),
        "{}\n\nverdicts:\n{}",
        problems.join("\n"),
        report(&verdicts)
    );
}

/// RED (A5) — **The hand-off lane behaves exactly as before behind the contract.**
///
/// The hand-off lane is the reference instance: its declared policy is the one its module doc
/// states — 32 waiting behind one running, a press past that answered with `LANE_FULL` through
/// the drain, ids that rise with every press — and the adapter wraps it without a change. Its
/// verdicts are exactly its partial contract: every claim passes but the two its rows declare
/// (the unbounded answers, D-71; the dead worker's owed answers, D-70). The flood below is the
/// module's own `a_flood_of_presses_is_refused_rather_than_waited_on`, asked through the adapter.
///
/// MUTATION: set `CAPACITY` to 31 — the declared bound and the queue disagree (`BoundBroken`), and
/// one more press is refused.
#[test]
fn the_handoff_lane_behaves_exactly_as_before_behind_the_contract() {
    let [handoff, ..] = adapters();
    let verdicts = run_suite(&handoff, &Claim::ALL);
    let problems = judge(
        &verdicts,
        EXPECTED_FAILURES,
        &[LaneName::Handoff],
        &Claim::ALL,
    );
    assert!(
        problems.is_empty(),
        "{}\n\nverdicts:\n{}",
        problems.join("\n"),
        report(&verdicts)
    );
    assert_eq!(
        HANDOFF.bounds.waiting,
        Bound::At(crate::handoff_lane::CAPACITY),
        "the declared bound is the lane's own"
    );

    let mut lane = (handoff.make)();
    lane.gate().hold();
    let running = lane.submit(0, 0);
    // The first press is on the executor, not in the queue, before the queue is filled.
    assert!(
        lane.gate().wait_entered(1),
        "the lane starts the first press"
    );
    let flood: Vec<Admission> = (1..=crate::handoff_lane::CAPACITY as u64 + 1)
        .map(|question| lane.submit(0, question))
        .collect();
    let tickets: Vec<u64> = std::iter::once(&running)
        .chain(&flood)
        .filter_map(|admission| admission.ticket)
        .collect();
    assert!(
        tickets.windows(2).all(|pair| pair[0] < pair[1]),
        "every press is its own request: {tickets:?}"
    );
    let refused = lane.drain();
    lane.gate().release();
    assert_eq!(
        refused,
        vec![Delivered {
            target: Some(0),
            ticket: flood.last().and_then(|admission| admission.ticket),
            question: Some(crate::handoff_lane::CAPACITY as u64 + 1),
            outcome: Outcome::Refused(crate::handoff_lane::LANE_FULL.to_owned()),
        }],
        "the one press past the bound is refused in the lane's own words, to the window that asked"
    );
}

/// A verdict for every lane and claim, failing exactly as [`EXPECTED_FAILURES`] declares.
fn faithful_verdicts() -> Vec<Verdict> {
    LaneName::ALL
        .iter()
        .flat_map(|&lane| {
            Claim::ALL.iter().map(move |&claim| Verdict {
                lane,
                claim,
                requests: 3,
                result: match EXPECTED_FAILURES
                    .iter()
                    .find(|row| row.lane == lane && row.claim == claim)
                {
                    Some(row) => Err(example(row.failure)),
                    None => Ok(()),
                },
            })
        })
        .collect()
}

/// A failure of `kind`, with made-up counts.
fn example(kind: FailureKind) -> Failure {
    match kind {
        FailureKind::AnswersUnbounded => Failure::AnswersUnbounded { held: 40 },
        FailureKind::DeathUnobservable => Failure::DeathUnobservable { unanswered: 3 },
        FailureKind::NoTerminal => Failure::NoTerminal { requests: 4 },
        FailureKind::AdmissionUnbounded => Failure::AdmissionUnbounded { admitted: 64 },
        FailureKind::IdentityAmbiguous => Failure::IdentityAmbiguous { answers: 2 },
        FailureKind::SupersededInSilence => Failure::SupersededInSilence { requests: 3 },
        FailureKind::JoinedInSilence => Failure::JoinedInSilence { requests: 1 },
        FailureKind::NoFailedTerminal => Failure::NoFailedTerminal { key: 1 },
        FailureKind::OrphanNotSunk => Failure::OrphanNotSunk { key: 0 },
        other => panic!("no example for {other:?}: add one when a row declares it"),
    }
}

fn judged(verdicts: &[Verdict]) -> Vec<String> {
    judge(verdicts, EXPECTED_FAILURES, &LaneName::ALL, &Claim::ALL)
}

/// PIN (A5) — **The judge passes a run that fails exactly as declared, and nothing else.**
///
/// The four ways R4 of the Codex review named for an expected-failure harness to lie, each planted
/// in an otherwise faithful run: an unexpected pass, a failure of a different kind, a skipped
/// adapter, and a claim that exercised nothing. Each is red on its own.
///
/// MUTATION: make `judge` accept any `Err` where a row is declared (drop the `failure.kind() !=
/// row.failure` arm) — the different failure passes.
#[test]
fn the_judge_is_red_on_an_unexpected_pass_a_different_failure_a_skipped_lane_and_no_requests() {
    let faithful = faithful_verdicts();
    assert_eq!(judged(&faithful), Vec::<String>::new(), "the faithful run");

    let row: &ExpectedFailure = &EXPECTED_FAILURES[0];
    let planted = |change: &dyn Fn(&mut Verdict)| -> Vec<String> {
        let mut verdicts = faithful.clone();
        let verdict = verdicts
            .iter_mut()
            .find(|verdict| verdict.lane == row.lane && verdict.claim == row.claim)
            .expect("the row's verdict");
        change(verdict);
        judged(&verdicts)
    };

    let unexpected_pass = planted(&|verdict| verdict.result = Ok(()));
    assert!(
        unexpected_pass.len() == 1 && unexpected_pass[0].contains("unexpected pass"),
        "{unexpected_pass:?}"
    );
    let different = planted(&|verdict| {
        verdict.result = Err(Failure::SubmissionWaited { longest_ms: 5_000 });
    });
    assert!(
        different.len() == 1 && different[0].contains("different failure"),
        "{different:?}"
    );
    let nothing_asked = planted(&|verdict| verdict.requests = 0);
    assert!(
        nothing_asked.len() == 1 && nothing_asked[0].contains("exercised no request"),
        "{nothing_asked:?}"
    );
    let skipped: Vec<Verdict> = faithful
        .iter()
        .filter(|verdict| verdict.lane != LaneName::Taskbar)
        .cloned()
        .collect();
    let skipped = judged(&skipped);
    assert!(
        skipped.len() == Claim::ALL.len()
            && skipped.iter().all(|problem| problem.contains("skipped")),
        "{skipped:?}"
    );
    let mut undeclared = faithful.clone();
    let passing = undeclared
        .iter_mut()
        .find(|verdict| verdict.result.is_ok())
        .expect("a claim some lane passes");
    passing.result = Err(Failure::NoWake);
    let undeclared = judged(&undeclared);
    assert!(
        undeclared.len() == 1 && undeclared[0].contains("no expected failure declares it"),
        "{undeclared:?}"
    );
}

/// PIN (A5) — **Every declared failure names one ledger row, once per lane and claim.**
///
/// A row with no repair is an exception nobody owes; two rows for one claim leave the judge
/// matching whichever it finds first.
///
/// MUTATION: write `repair: "later"` on any row — red.
#[test]
fn every_declared_lane_failure_names_one_ledger_row() {
    let doubled: Vec<ExpectedFailure> = EXPECTED_FAILURES
        .iter()
        .chain(&EXPECTED_FAILURES[..1])
        .copied()
        .collect();
    assert!(
        judge(&faithful_verdicts(), &doubled, &LaneName::ALL, &Claim::ALL)
            .iter()
            .any(|problem| problem.contains("declared twice")),
        "a doubled row is refused"
    );
    let mut unrepaired = EXPECTED_FAILURES.to_vec();
    unrepaired[0].repair = "later";
    assert!(
        judge(
            &faithful_verdicts(),
            &unrepaired,
            &LaneName::ALL,
            &Claim::ALL
        )
        .iter()
        .any(|problem| problem.contains("names no ledger row")),
        "a row with no ledger id is refused"
    );
    assert!(
        judge(
            &faithful_verdicts(),
            EXPECTED_FAILURES,
            &LaneName::ALL,
            &Claim::ALL
        )
        .is_empty(),
        "and the table as it stands is well formed"
    );
}

// ------------------------------------------------------------------------------------------------
// The seam: the ledger's exhaustive proof, the executor, and lanes the suite builds on the seam
// ------------------------------------------------------------------------------------------------

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a terminal is, as the suite's consumer reads it.
fn outcome(terminal: Terminal<u64, u64, String>) -> Outcome {
    match terminal {
        Terminal::Done(_) => Outcome::Answered,
        Terminal::Failed(failure) => Outcome::Failed(failure.to_string()),
        Terminal::Cancelled => Outcome::Cancelled,
        Terminal::Superseded { by } => Outcome::Superseded { by },
        Terminal::Joined {
            with,
            outcome: carried,
        } => Outcome::Joined {
            with,
            carried: Box::new(outcome(*carried)),
        },
        Terminal::WorkerGone => Outcome::WorkerGone,
    }
}

/// How each of the proof's three identities ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fate {
    Done,
    Failed,
    /// The lane publishes `Cancelled`, and the worker's answer races it, published after it.
    Cancelled,
    /// Superseded by the next identity (the first by the second, …, the third by the first).
    Superseded,
    /// Joined to the identity before it (and so, through it, to the first that is not joined).
    Joined,
    /// Still owed when the worker ends.
    WorkerGone,
}

/// One event of an interleaving.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    /// The identity's own terminal is published (`Fate::Done`, `Failed`, `Cancelled`, `Superseded`).
    End(usize),
    /// The answer that races a cancel is published.
    Race(usize),
    /// The requester drops its `Request`.
    Drop(usize),
    /// The executor reports the worker's end.
    WorkerEnded,
}

/// One case: how each identity ends, whose requester goes, and whether the worker ends.
#[derive(Clone, Copy, Debug)]
struct Case {
    fates: [Fate; 3],
    drops: [bool; 3],
    worker_ends: bool,
}

const FATES: [Fate; 6] = [
    Fate::Done,
    Fate::Failed,
    Fate::Cancelled,
    Fate::Superseded,
    Fate::Joined,
    Fate::WorkerGone,
];

/// Every case: the first identity has no earlier one to join, and an identity owed when the
/// worker ends needs the worker to end.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for first in FATES.into_iter().filter(|fate| *fate != Fate::Joined) {
        for second in FATES {
            for third in FATES {
                let fates = [first, second, third];
                for drops in 0..8_u8 {
                    for worker_ends in [false, true] {
                        if !worker_ends && fates.contains(&Fate::WorkerGone) {
                            continue;
                        }
                        cases.push(Case {
                            fates,
                            drops: [0, 1, 2].map(|at| drops & (1 << at) != 0),
                            worker_ends,
                        });
                    }
                }
            }
        }
    }
    cases
}

fn steps_of(case: &Case) -> Vec<Step> {
    let mut steps = Vec::new();
    for (at, fate) in case.fates.iter().enumerate() {
        match fate {
            Fate::Done | Fate::Failed | Fate::Superseded => steps.push(Step::End(at)),
            Fate::Cancelled => steps.extend([Step::End(at), Step::Race(at)]),
            Fate::Joined | Fate::WorkerGone => {}
        }
        if case.drops[at] {
            steps.push(Step::Drop(at));
        }
    }
    if case.worker_ends {
        steps.push(Step::WorkerEnded);
    }
    steps
}

/// Every order of `steps` in which a racing answer comes after the cancel it races, each handed to
/// `visit`. Deterministic: the orders come in the order a depth-first walk over `steps` meets them.
fn interleavings(steps: &[Step], visit: &mut impl FnMut(&[Step])) {
    fn walk(steps: &[Step], used: u32, order: &mut Vec<Step>, visit: &mut impl FnMut(&[Step])) {
        if order.len() == steps.len() {
            visit(order);
            return;
        }
        for (at, step) in steps.iter().enumerate() {
            if used & (1 << at) != 0 {
                continue;
            }
            if let Step::Race(identity) = step
                && !order.contains(&Step::End(*identity))
            {
                continue;
            }
            order.push(*step);
            walk(steps, used | (1 << at), order, visit);
            order.pop();
        }
    }
    walk(steps, 0, &mut Vec::with_capacity(steps.len()), visit);
}

/// The proof's sink: what reached it, by identity.
struct ProofSink(Arc<Mutex<Vec<(u64, ProofTerminal)>>>);

/// A terminal of the proof: identities, answers and errors as the suite spells them.
type ProofTerminal = Terminal<u64, u64, String>;

impl Owed<u64> for ProofSink {
    type Answer = u64;
    type Error = String;

    fn sink(self, id: u64, terminal: Terminal<u64, u64, String>) {
        locked(&self.0).push((id, terminal));
    }
}

/// The terminal identity `at`'s own step publishes, its identities being `ids`.
fn published(case: &Case, at: usize, ids: [u64; 3]) -> Terminal<u64, u64, String> {
    match case.fates[at] {
        Fate::Failed => Terminal::Failed(TaskFailure::Error(format!("failure {at}"))),
        Fate::Cancelled => Terminal::Cancelled,
        Fate::Superseded => Terminal::Superseded {
            by: ids[(at + 1) % 3],
        },
        Fate::Done | Fate::Joined | Fate::WorkerGone => Terminal::Done(at as u64),
    }
}

/// **The reference**: what each identity must end with, read off the order by the contract's own
/// words — the first terminal stands, a joined identity ends with its first unjoined twin's, and
/// the worker's end ends whatever is still owed.
fn expected(case: &Case, order: &[Step], ids: [u64; 3]) -> [Option<Terminal<u64, u64, String>>; 3] {
    let root = |mut at: usize| {
        while case.fates[at] == Fate::Joined {
            at -= 1;
        }
        at
    };
    let mut ended: [Option<Terminal<u64, u64, String>>; 3] = [None, None, None];
    let end = |ended: &mut [Option<Terminal<u64, u64, String>>; 3],
               at: usize,
               terminal: Terminal<u64, u64, String>| {
        if ended[at].is_some() {
            return;
        }
        for (twin, slot) in ended.iter_mut().enumerate() {
            if twin != at && case.fates[twin] == Fate::Joined && root(twin) == at && slot.is_none()
            {
                *slot = Some(Terminal::Joined {
                    with: ids[at],
                    outcome: Box::new(terminal.clone()),
                });
            }
        }
        ended[at] = Some(terminal);
    };
    for step in order {
        match *step {
            Step::End(at) => end(&mut ended, at, published(case, at, ids)),
            Step::Race(at) => end(&mut ended, at, Terminal::Done(at as u64)),
            Step::Drop(_) => {}
            Step::WorkerEnded => {
                for slot in ended.iter_mut().filter(|slot| slot.is_none()) {
                    *slot = Some(Terminal::WorkerGone);
                }
            }
        }
    }
    ended
}

/// **Play one interleaving on a real [`Ledger`]** and say what is wrong with it, if anything.
fn play(case: &Case, order: &[Step]) -> Option<String> {
    let said = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&said);
    let sunk = Arc::new(Mutex::new(Vec::new()));
    let ledger: Arc<Ledger<u64, ProofSink>> =
        Ledger::new("proof", None, Saying::PerRequest, move |_line: &str| {
            counting.fetch_add(1, Ordering::SeqCst);
        });
    let mut requests: Vec<Option<Request<u64, ProofSink>>> = Vec::new();
    for at in 0..3 {
        let owed = ProofSink(Arc::clone(&sunk));
        let admitted = match (case.fates[at], requests.last()) {
            (Fate::Joined, Some(Some(twin))) => ledger.admit_joined(twin.id(), owed),
            _ => ledger.admit(owed),
        };
        requests.push(Some(
            admitted.expect("an unbounded, unfaulted ledger admits"),
        ));
    }
    let ids: [u64; 3] = [0, 1, 2].map(|at| requests[at].as_ref().map_or(0, Request::id));
    let mut accepted = [0_usize; 3];
    for step in order {
        match *step {
            Step::End(at) => {
                accepted[at] += usize::from(ledger.publish(ids[at], published(case, at, ids)));
            }
            Step::Race(at) => {
                accepted[at] += usize::from(ledger.publish(ids[at], Terminal::Done(at as u64)));
            }
            Step::Drop(at) => drop(requests[at].take()),
            Step::WorkerEnded => ledger.worker_ended(WorkerEnd::Panicked("the proof".to_owned())),
        }
    }
    let mut taken: [Vec<Terminal<u64, u64, String>>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (at, request) in requests.iter_mut().enumerate() {
        if let Some(request) = request {
            // The consumer asks twice: taking consumes.
            taken[at].extend(request.take());
            taken[at].extend(request.take());
        }
    }
    let sunk = locked(&sunk).clone();
    let want = expected(case, order, ids);
    for at in 0..3 {
        let key = ids[at];
        let in_sink: Vec<&Terminal<u64, u64, String>> = sunk
            .iter()
            .filter(|(id, _)| *id == key)
            .map(|(_, terminal)| terminal)
            .collect();
        let deliveries = taken[at].len() + in_sink.len();
        if accepted[at] > 1 || deliveries > 1 {
            return Some(format!(
                "{:?}: identity {at} published {} time(s), delivered {deliveries} time(s)",
                Failure::Twice { key, deliveries },
                accepted[at]
            ));
        }
        let Some(got) = taken[at].first().or(in_sink.first().copied()) else {
            return Some(format!(
                "{:?}: identity {at}",
                Failure::NoTerminal { requests: 1 }
            ));
        };
        if case.drops[at] == in_sink.is_empty() {
            return Some(format!(
                "identity {at} ended with {got:?} at its {}, its requester having {}",
                if in_sink.is_empty() {
                    "requester"
                } else {
                    "sink"
                },
                if case.drops[at] { "gone" } else { "stayed" }
            ));
        }
        if want[at].as_ref() != Some(got) {
            return Some(format!(
                "identity {at} ended with {got:?}; the contract says {:?}",
                want[at]
            ));
        }
    }
    let failed = want
        .iter()
        .filter(|terminal| matches!(terminal, Some(Terminal::Failed(_))))
        .count();
    let lines = failed + usize::from(case.worker_ends);
    if said.load(Ordering::SeqCst) != lines {
        return Some(format!(
            "{:?}",
            Failure::SaidOtherThanOnce {
                expected: lines,
                said: said.load(Ordering::SeqCst)
            }
        ));
    }
    let refused = ledger.admit(ProofSink(Arc::new(Mutex::new(Vec::new()))));
    if ledger.fault().is_some() != case.worker_ends
        || matches!(refused, Err(Refused::Faulted(_))) != case.worker_ends
    {
        return Some(format!(
            "the worker {} and the lane's fault reads {:?}, a new request {:?}",
            if case.worker_ends {
                "ended"
            } else {
                "did not end"
            },
            ledger.fault(),
            refused.as_ref().err()
        ));
    }
    None
}

/// How many interleavings [`the_ledger_ends_every_identity_exactly_once_in_every_interleaving`]
/// plays: 2 240 cases, each every order of its steps with a racing answer after its cancel.
const INTERLEAVINGS: usize = 3_302_423;

/// RED (S-3) — **The ledger ends every identity exactly once, in every interleaving.**
///
/// The exhaustive proof over [`Ledger`] alone (revision (b)7), the product code every lane will
/// keep its requests in. Three identities; each ends `Done`, `Failed`, `Cancelled` (with the
/// worker's answer racing the cancel), `Superseded { by }` the next, `Joined { with }` the one
/// before, or `WorkerGone`; each requester stays or drops its `Request`; the worker ends or does
/// not. Every order of those events is played on a fresh ledger, and each identity must be
/// delivered exactly once — to its requester, or to its sink when the requester has gone — with
/// the terminal the contract's reference ([`expected`]) reads off the order; taking twice gives
/// one; each failed identity is said once and the worker's end once; the lane is in fault, and
/// refuses at the door, exactly when the worker ended. No clock and no thread is in the proof:
/// the order is the script. The cases are shared among scoped threads only to finish sooner; the
/// count and the verdict do not depend on how.
///
/// MUTATIONS, each red here: in `Ledger::publish`, return `true` without recording a
/// `Superseded` terminal (`NoTerminal`); in `Ledger::publish`, accept a terminal for an identity
/// that already has one (`Some(_) => {}`) — the answer racing a cancel is delivered too
/// (`Twice`); in `Ledger::worker_ended`, skip ending the owed identities (`NoTerminal`).
#[test]
fn the_ledger_ends_every_identity_exactly_once_in_every_interleaving() {
    let cases = cases();
    assert_eq!(cases.len(), 2_240, "the cases");
    let shares = 8;
    let results: Vec<(usize, Vec<String>)> = std::thread::scope(|scope| {
        let runs: Vec<_> = (0..shares)
            .map(|share| {
                let cases = &cases;
                scope.spawn(move || {
                    let mut played = 0;
                    let mut problems = Vec::new();
                    for case in cases.iter().skip(share).step_by(shares) {
                        interleavings(&steps_of(case), &mut |order| {
                            played += 1;
                            if problems.len() < 3
                                && let Some(problem) = play(case, order)
                            {
                                problems.push(format!("{case:?} {order:?}: {problem}"));
                            }
                        });
                    }
                    (played, problems)
                })
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("a share of the proof ran to its end"))
            .collect()
    });
    let played: usize = results.iter().map(|(played, _)| played).sum();
    let problems: Vec<String> = results
        .into_iter()
        .flat_map(|(_, problems)| problems)
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert_eq!(played, INTERLEAVINGS, "every interleaving was played");
}

/// PIN (S-3) — **The ledger refuses at the door, at once, and says why** (revision (b)3): past its
/// bound, after its worker has ended, and a request asked to join one that has ended. A refusal
/// mints no identity, so nothing is owed for it.
///
/// MUTATION: drop the bound's check in `Ledger::mint` — the third request is admitted.
#[test]
fn the_ledger_refuses_at_the_door_synchronously() {
    let sunk = Arc::new(Mutex::new(Vec::new()));
    let owed = || ProofSink(Arc::clone(&sunk));
    let ledger: Arc<Ledger<u64, ProofSink>> =
        Ledger::new("refusals", Some(2), Saying::PerRequest, |_: &str| {});
    let mut first = ledger.admit(owed()).expect("the first is admitted");
    let second = ledger.admit(owed()).expect("the second is admitted");
    assert_eq!(
        ledger.admit(owed()).err(),
        Some(Refused::Full { bound: 2 }),
        "a third, past the bound of two"
    );
    assert!(ledger.publish(first.id(), Terminal::Done(1)));
    assert_eq!(
        ledger.admit(owed()).err(),
        Some(Refused::Full { bound: 2 }),
        "an answer its requester has not taken is still held"
    );
    assert_eq!(first.take(), Some(Terminal::Done(1)));
    assert_eq!(
        ledger.admit_joined(first.id(), owed()).err(),
        Some(Refused::TwinEnded),
        "joining one that has ended"
    );
    let joined = ledger
        .admit_joined(second.id(), owed())
        .expect("joining one still owed");
    ledger.worker_ended(WorkerEnd::Returned);
    assert_eq!(
        ledger.admit(owed()).err(),
        Some(Refused::Faulted(WorkerEnd::Returned)),
        "after the worker's end"
    );
    drop((second, joined));
    assert_eq!(
        locked(&sunk)
            .iter()
            .map(|(_, terminal)| terminal.clone())
            .collect::<Vec<_>>(),
        vec![Terminal::WorkerGone, Terminal::WorkerGone],
        "the two owed when the worker ended went to their sink when their requesters went"
    );
}

/// PIN (S-3) — **A streak of the same failure is said once** where the lane declares streaks; an
/// answer delivered between two failures ends the streak, and a lane that does not declare
/// streaks says every failure.
///
/// MUTATION: compare nothing in `Ledger::delivered` (say every failure) — the streak lane says
/// three, not two.
#[test]
fn a_streak_of_the_same_failure_is_said_once() {
    for (saying, lines) in [(Saying::PerStreak, 2), (Saying::PerRequest, 3)] {
        let said = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&said);
        let ledger: Arc<Ledger<u64, ProofSink>> =
            Ledger::new("streaks", None, saying, move |_: &str| {
                counting.fetch_add(1, Ordering::SeqCst);
            });
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let failure = || Terminal::Failed(TaskFailure::Error("the disk refused".to_owned()));
        for terminal in [failure(), failure(), Terminal::Done(0), failure()] {
            let mut request = ledger
                .admit(ProofSink(Arc::clone(&sunk)))
                .expect("admitted");
            assert!(ledger.publish(request.id(), terminal));
            assert!(request.take().is_some());
        }
        assert_eq!(said.load(Ordering::SeqCst), lines, "{saying:?}");
    }
}

/// What the scripted lanes keep for the suite: their sink's terminals and the lines their seam
/// over `diagnostics::note` was handed.
#[derive(Default)]
struct Kept {
    sunk: Mutex<Vec<Delivered>>,
    said: AtomicUsize,
}

/// One scripted request's sink.
struct Owe {
    kept: Arc<Kept>,
    question: u64,
    /// The planted sink that drops what it is handed.
    drops: bool,
}

impl Owed<u64> for Owe {
    type Answer = u64;
    type Error = String;

    fn sink(self, id: u64, terminal: Terminal<u64, u64, String>) {
        if !self.drops {
            locked(&self.kept.sunk).push(Delivered {
                target: None,
                ticket: Some(id),
                question: Some(self.question),
                outcome: outcome(terminal),
            });
        }
    }
}

/// How a scripted lane relates a new request to the ones it holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Policy {
    /// Every request is run in order; the same question asked while one is owed joins it.
    Fifo,
    /// A new request displaces the one waiting, which ends `Superseded { by }` the new one.
    Latest,
}

/// What a scripted lane is planted with: each a way a lane built on the seam could still break a
/// claim, and the claim that must go red on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Planted {
    Nothing,
    /// A displaced request is dropped from the queue with no terminal.
    SilentSupersede,
    /// The same question asked again supersedes the first instead of joining it.
    TwinSuperseded,
    /// The worker tries a failed task three times and says each failed try itself.
    SaysEveryRetry,
    /// The sink drops what it is handed.
    SinkDrops,
    /// The worker hands every answer to the sink as well as publishing it.
    AlsoSinks,
}

/// How many identities the FIFO scripted lane's ledger owes at most.
const SCRIPTED_BOUND: usize = 5;

/// A FIFO lane on the seam: one running, four waiting, the rest refused at the door; every answer
/// held until taken; a closed target's request ends in its sink.
const SCRIPTED_FIFO: Contract = Contract {
    lane: None,
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        waiting: Bound::At(SCRIPTED_BOUND - 1),
        executing: 1,
        answers_held: Bound::At(SCRIPTED_BOUND),
    },
    cancellation: Cancellation::Abandon,
    saying: Saying::PerRequest,
};

/// The same lane, declaring that a streak of the same failure is said once.
const SCRIPTED_STREAKS: Contract = Contract {
    saying: Saying::PerStreak,
    ..SCRIPTED_FIFO
};

/// A latest-value lane on the seam: one running, one waiting, a newer request displacing it.
const SCRIPTED_LATEST: Contract = Contract {
    lane: None,
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        answers_held: Bound::Unbounded,
    },
    cancellation: Cancellation::Abandon,
    saying: Saying::PerRequest,
};

#[derive(Default)]
struct Jobs {
    waiting: VecDeque<(u64, u64)>,
    closed: bool,
}

/// The scripted lane's queue: `(identity, question)` pairs, and the worker's signal.
#[derive(Default)]
struct Queue {
    jobs: Mutex<Jobs>,
    more: Condvar,
}

impl Queue {
    fn push(&self, job: (u64, u64)) {
        locked(&self.jobs).waiting.push_back(job);
        self.more.notify_all();
    }

    /// The next job, waiting for one; `None` once the lane is closed and the queue is empty.
    fn next(&self) -> Option<(u64, u64)> {
        let mut jobs = locked(&self.jobs);
        loop {
            if let Some(job) = jobs.waiting.pop_front() {
                return Some(job);
            }
            if jobs.closed {
                return None;
            }
            jobs = self.more.wait(jobs).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn close(&self) {
        locked(&self.jobs).closed = true;
        self.more.notify_all();
    }
}

/// One request a scripted lane's consumer holds.
struct Asked {
    target: u32,
    question: u64,
    request: Request<u64, Owe>,
}

/// **A lane built on the seam**: a real [`Ledger`], a worker started by [`Executor::scripted`] whose
/// tasks run in [`Worker::task`], and a body the suite writes — take a job, run its task (which
/// answers its question, or panics where the suite planted a panic), publish, wake.
struct ScriptedLane {
    contract: &'static Contract,
    policy: Policy,
    planted: Planted,
    ledger: Arc<Ledger<u64, Owe>>,
    queue: Arc<Queue>,
    gate: Arc<Gate>,
    probe: Arc<WakeProbe>,
    kept: Arc<Kept>,
    asked: Vec<Asked>,
    fault_seen: bool,
}

fn scripted_lane(contract: &'static Contract, policy: Policy, planted: Planted) -> ScriptedLane {
    let kept = Arc::new(Kept::default());
    let counting = Arc::clone(&kept);
    let bound = (policy == Policy::Fifo).then_some(SCRIPTED_BOUND);
    let ledger = Ledger::new("scripted", bound, contract.saying, move |_line: &str| {
        counting.said.fetch_add(1, Ordering::SeqCst);
    });
    let queue = Arc::new(Queue::default());
    let gate = Arc::new(Gate::default());
    let probe = Arc::new(WakeProbe::default());
    let (jobs, publish, door, wake, retries) = (
        Arc::clone(&queue),
        Arc::clone(&ledger),
        Arc::clone(&gate),
        Arc::clone(&probe),
        Arc::clone(&kept),
    );
    Executor::scripted(Arc::clone(&gate))
        .start_worker(
            "lane-contract-scripted",
            Arc::clone(&ledger),
            move |worker| {
                while let Some((id, question)) = jobs.next() {
                    let task = || {
                        worker.task(|_ctx| {
                            door.execute(question);
                            Ok::<u64, String>(question)
                        })
                    };
                    let mut result = task();
                    if planted == Planted::SaysEveryRetry {
                        for _ in 0..2 {
                            if result.is_ok() {
                                break;
                            }
                            // The retry's own line, as `diagnostics::note` would hear it.
                            retries.said.fetch_add(1, Ordering::SeqCst);
                            result = task();
                        }
                    }
                    let terminal = match result {
                        Ok(answer) => Terminal::Done(answer),
                        Err(failure) => Terminal::Failed(failure),
                    };
                    if planted == Planted::AlsoSinks {
                        locked(&retries.sunk).push(Delivered {
                            target: None,
                            ticket: Some(id),
                            question: Some(question),
                            outcome: outcome(terminal.clone()),
                        });
                    }
                    publish.publish(id, terminal);
                    wake.woke();
                }
            },
        )
        .expect("the scripted worker starts");
    ScriptedLane {
        contract,
        policy,
        planted,
        ledger,
        queue,
        gate,
        probe,
        kept,
        asked: Vec::new(),
        fault_seen: false,
    }
}

impl Drop for ScriptedLane {
    fn drop(&mut self) {
        self.queue.close();
    }
}

impl ScriptedLane {
    fn owe(&self, question: u64) -> Owe {
        Owe {
            kept: Arc::clone(&self.kept),
            question,
            drops: self.planted == Planted::SinkDrops,
        }
    }

    /// The lane's own admission, by its policy.
    fn admit(&mut self, question: u64) -> Result<Request<u64, Owe>, Refused> {
        let twin = self
            .asked
            .iter()
            .find(|asked| asked.question == question)
            .map(|asked| asked.request.id());
        match (self.policy, twin) {
            (Policy::Fifo, Some(twin)) if self.planted == Planted::TwinSuperseded => {
                let request = self.ledger.admit(self.owe(question))?;
                self.ledger
                    .publish(twin, Terminal::Superseded { by: request.id() });
                self.queue.push((request.id(), question));
                Ok(request)
            }
            (Policy::Fifo, Some(twin)) => {
                match self.ledger.admit_joined(twin, self.owe(question)) {
                    Err(Refused::TwinEnded) => {}
                    joined => return joined,
                }
                let request = self.ledger.admit(self.owe(question))?;
                self.queue.push((request.id(), question));
                Ok(request)
            }
            (Policy::Fifo, None) => {
                let request = self.ledger.admit(self.owe(question))?;
                self.queue.push((request.id(), question));
                Ok(request)
            }
            (Policy::Latest, _) => {
                let request = self.ledger.admit(self.owe(question))?;
                let displaced = {
                    let mut jobs = locked(&self.queue.jobs);
                    let displaced = jobs.waiting.pop_back();
                    jobs.waiting.push_back((request.id(), question));
                    displaced
                };
                self.queue.more.notify_all();
                if let Some((old, _)) = displaced
                    && self.planted != Planted::SilentSupersede
                {
                    self.ledger
                        .publish(old, Terminal::Superseded { by: request.id() });
                }
                Ok(request)
            }
        }
    }
}

impl LaneUnderTest for ScriptedLane {
    fn contract(&self) -> &'static Contract {
        self.contract
    }

    fn gate(&self) -> &Gate {
        &self.gate
    }

    fn probe(&self) -> &WakeProbe {
        &self.probe
    }

    fn submit(&mut self, target: u32, question: u64) -> Admission {
        match self.admit(question) {
            Ok(request) => {
                let ticket = Some(request.id());
                self.asked.push(Asked {
                    target,
                    question,
                    request,
                });
                Admission {
                    ticket,
                    question,
                    refused: None,
                }
            }
            Err(refused) => Admission {
                ticket: None,
                question,
                refused: Some(format!("{refused:?}")),
            },
        }
    }

    fn close_target(&mut self, target: u32) {
        self.asked.retain(|asked| asked.target != target);
    }

    fn drain(&mut self) -> Vec<Delivered> {
        let mut found = Vec::new();
        if !self.fault_seen
            && let Some(end) = self.ledger.fault()
        {
            self.fault_seen = true;
            found.push(Delivered {
                target: None,
                ticket: None,
                question: None,
                outcome: Outcome::Fault(end.to_string()),
            });
        }
        self.asked.retain_mut(|asked| match asked.request.take() {
            Some(terminal) => {
                found.push(Delivered {
                    target: Some(asked.target),
                    ticket: Some(asked.request.id()),
                    question: Some(asked.question),
                    outcome: outcome(terminal),
                });
                false
            }
            None => true,
        });
        found
    }

    fn offer_stale(&mut self, _ticket: u64) {
        unreachable!("a scripted lane's target is the asking target, not the application");
    }

    fn sunk(&mut self) -> Vec<Delivered> {
        std::mem::take(&mut *locked(&self.kept.sunk))
    }

    fn said(&self) -> Option<usize> {
        Some(self.kept.said.load(Ordering::SeqCst))
    }
}

/// `claim` on a fresh scripted lane.
fn on_scripted(
    contract: &'static Contract,
    policy: Policy,
    planted: Planted,
    claim: Claim,
) -> Result<(), Failure> {
    let mut lane = scripted_lane(contract, policy, planted);
    let (requests, result) = check(&mut lane, claim);
    assert!(requests > 0, "{claim:?} asked the scripted lane nothing");
    result
}

/// PIN (S-3) — **A lane built on the seam keeps every claim of the contract.**
///
/// The FIFO scripted lane — a real [`Ledger`] bounded at five, its worker started by the scripted
/// [`Executor`], its tasks run in [`Worker::task`], the same question asked while one is owed
/// joined to it — passes all twelve claims through the same [`check`] the five product lanes are
/// run with; so does the lane that declares streaks on the claim that reads them, and the
/// latest-value scripted lane on the claims a displaced request bears on.
///
/// MUTATION: in `Worker::task`, re-raise a caught panic (`Err(panic) =>
/// std::panic::resume_unwind(panic)`) — the task panic ends the worker, and `AFailureIsSaidOnce`
/// fails with `NoFailedTerminal`.
#[test]
fn a_lane_built_on_the_seam_keeps_every_claim() {
    let mut failures = Vec::new();
    for claim in Claim::ALL {
        if let Err(failure) = on_scripted(&SCRIPTED_FIFO, Policy::Fifo, Planted::Nothing, claim) {
            failures.push(format!("FIFO × {claim:?}: {failure:?}"));
        }
    }
    for (contract, policy, claim) in [
        (&SCRIPTED_STREAKS, Policy::Fifo, Claim::AFailureIsSaidOnce),
        (
            &SCRIPTED_LATEST,
            Policy::Latest,
            Claim::ASupersededRequestSaysSo,
        ),
        (
            &SCRIPTED_LATEST,
            Policy::Latest,
            Claim::EveryRequestEndsExactlyOnce,
        ),
        (&SCRIPTED_LATEST, Policy::Latest, Claim::AFailureIsSaidOnce),
    ] {
        if let Err(failure) = on_scripted(contract, policy, Planted::Nothing, claim) {
            failures.push(format!("{policy:?} × {claim:?}: {failure:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// RED (S-3) — **A superseded request says so**: the latest-value lane on the seam ends each
/// displaced request `Superseded { by }` the request that displaced it; the same lane planted to
/// drop a displaced request from its queue without a terminal is red.
///
/// MUTATION: plant `Planted::Nothing` in the second run — it passes, and the test is red.
#[test]
fn a_superseded_request_says_so_and_a_silent_one_is_red() {
    assert_eq!(
        on_scripted(
            &SCRIPTED_LATEST,
            Policy::Latest,
            Planted::Nothing,
            Claim::ASupersededRequestSaysSo
        ),
        Ok(())
    );
    let silent = on_scripted(
        &SCRIPTED_LATEST,
        Policy::Latest,
        Planted::SilentSupersede,
        Claim::ASupersededRequestSaysSo,
    );
    assert_eq!(
        silent.map_err(|failure| failure.kind()),
        Err(FailureKind::SupersededInSilence)
    );
}

/// RED (S-3) — **A joined request carries its twin**: the FIFO lane on the seam ends the same
/// question asked again `Joined { with }` the first, carrying its answer; the same lane planted to
/// supersede the first by its twin instead is red.
///
/// MUTATION: in `Ledger::publish`, hand each twin `Joined` with an outcome of `Cancelled` instead
/// of the terminal published — `JoinedCarriesAnotherAnswer`.
#[test]
fn a_joined_request_carries_its_twin_and_a_superseding_twin_is_red() {
    assert_eq!(
        on_scripted(
            &SCRIPTED_FIFO,
            Policy::Fifo,
            Planted::Nothing,
            Claim::AJoinedRequestCarriesItsTwin
        ),
        Ok(())
    );
    let superseded = on_scripted(
        &SCRIPTED_FIFO,
        Policy::Fifo,
        Planted::TwinSuperseded,
        Claim::AJoinedRequestCarriesItsTwin,
    );
    assert_eq!(
        superseded.map_err(|failure| failure.kind()),
        Err(FailureKind::SupersededByItsTwin)
    );
}

/// RED (S-3) — **A failure is said once**, counted through the ledger's seam over
/// `diagnostics::note`: three panicking requests are three lines on the lane that says every
/// failure and two on the lane that declares streaks (the first two are one streak); the worker
/// that says each failed retry itself is red.
///
/// MUTATION: in `Ledger::delivered`, say a failure twice — `SaidOtherThanOnce` on both lanes.
#[test]
fn a_failure_is_said_once_and_a_worker_saying_every_retry_is_red() {
    for contract in [&SCRIPTED_FIFO, &SCRIPTED_STREAKS] {
        assert_eq!(
            on_scripted(
                contract,
                Policy::Fifo,
                Planted::Nothing,
                Claim::AFailureIsSaidOnce
            ),
            Ok(()),
            "{:?}",
            contract.saying
        );
    }
    let retries = on_scripted(
        &SCRIPTED_FIFO,
        Policy::Fifo,
        Planted::SaysEveryRetry,
        Claim::AFailureIsSaidOnce,
    );
    assert_eq!(
        retries,
        Err(Failure::SaidOtherThanOnce {
            expected: 3,
            said: 9
        })
    );
}

/// RED (S-3) — **An orphaned answer reaches its sink**: a request whose target closes while it
/// runs ends in the sink it is owed and in no target; the lane whose sink drops what it is
/// handed is red.
///
/// MUTATION: in `Ledger::requester_gone`, forget to mark the requester gone — the answer waits for
/// a requester that has gone, nothing reaches the sink, and the lane on the seam fails with
/// `OrphanNotSunk`.
#[test]
fn an_orphaned_answer_reaches_its_sink_and_a_dropping_sink_is_red() {
    assert_eq!(
        on_scripted(
            &SCRIPTED_FIFO,
            Policy::Fifo,
            Planted::Nothing,
            Claim::AnOrphanedAnswerReachesItsSink
        ),
        Ok(())
    );
    let dropped = on_scripted(
        &SCRIPTED_FIFO,
        Policy::Fifo,
        Planted::SinkDrops,
        Claim::AnOrphanedAnswerReachesItsSink,
    );
    assert_eq!(
        dropped.map_err(|failure| failure.kind()),
        Err(FailureKind::OrphanNotSunk)
    );
}

/// RED (S-3) — **A second delivery is `Twice`**: a worker that hands its answer to the sink as
/// well as publishing it delivers each request twice, and `EveryRequestEndsExactlyOnce` says so.
///
/// MUTATION: plant `Planted::Nothing` — it passes, and the test is red.
#[test]
fn a_second_delivery_is_twice() {
    let twice = on_scripted(
        &SCRIPTED_FIFO,
        Policy::Fifo,
        Planted::AlsoSinks,
        Claim::EveryRequestEndsExactlyOnce,
    );
    assert_eq!(
        twice.map_err(|failure| failure.kind()),
        Err(FailureKind::Twice)
    );
}

/// The sink of [`a_worker_s_end_leaves_its_owed_requests_worker_gone`]: each terminal sent on.
struct SentOn(mpsc::Sender<Terminal<u64, u64, String>>);

impl Owed<u64> for SentOn {
    type Answer = u64;
    type Error = String;

    fn sink(self, _id: u64, terminal: Terminal<u64, u64, String>) {
        let _ = self.0.send(terminal);
    }
}

/// RED (S-3) — **A task panic is that request's failure; the worker's end is every owed request's
/// `WorkerGone`.** On the product's executor (`Executor::thread_door`): a worker whose first task
/// panics publishes `Failed(Panicked)` for it and goes on to answer the second; then its body
/// panics outside any task while a third request is owed, and the executor's report ends that one
/// `WorkerGone`, leaves the lane in fault, and refuses the next request at the door. The order is
/// the script's: each step waits on the one before through a channel.
///
/// MUTATION: in `Worker::task`, re-raise a caught panic — the first request ends `WorkerGone`, not
/// `Failed`. Or in `Executor::start_worker`, do not report the end — the third is never sent on.
#[test]
fn a_task_panic_fails_its_request_and_a_worker_s_end_leaves_the_owed_ones_worker_gone() {
    let (sent, sunk) = mpsc::channel();
    let said = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&said);
    let ledger: Arc<Ledger<u64, SentOn>> =
        Ledger::new("thread-door", None, Saying::PerRequest, move |_: &str| {
            counting.fetch_add(1, Ordering::SeqCst);
        });
    let (go, jobs) = mpsc::channel::<u64>();
    let (name, named) = mpsc::channel();
    let publish = Arc::clone(&ledger);
    Executor::thread_door(bt_platform::ThreadPriority::Normal)
        .start_worker(
            "lane-contract-thread-door",
            Arc::clone(&ledger),
            move |worker| {
                name.send(worker.ctx().name())
                    .expect("the test is listening");
                for id in jobs.iter() {
                    if id == 3 {
                        panic!("the worker's body itself fails");
                    }
                    let result = worker.task(|_ctx| {
                        if id == 1 {
                            panic!("{PLANTED_PANIC}");
                        }
                        Ok::<u64, String>(id)
                    });
                    publish.publish(
                        id,
                        match result {
                            Ok(answer) => Terminal::Done(answer),
                            Err(failure) => Terminal::Failed(failure),
                        },
                    );
                }
            },
        )
        .expect("the worker starts");
    // Each requester goes at once, so each terminal is sent on as it lands, in order.
    for _ in 0..3 {
        let request = ledger.admit(SentOn(sent.clone())).expect("admitted");
        go.send(request.id()).expect("the worker is listening");
    }
    let ended: Vec<Terminal<u64, u64, String>> = (0..3)
        .map(|_| receive(&sunk, "each request's end"))
        .collect();
    assert_eq!(
        ended,
        vec![
            Terminal::Failed(TaskFailure::Panicked(PLANTED_PANIC.to_owned())),
            Terminal::Done(2),
            Terminal::WorkerGone,
        ]
    );
    assert_eq!(
        ledger.fault(),
        Some(WorkerEnd::Panicked(
            "the worker's body itself fails".to_owned()
        ))
    );
    assert!(matches!(
        ledger.admit(SentOn(sent.clone())),
        Err(Refused::Faulted(_))
    ));
    assert_eq!(
        said.load(Ordering::SeqCst),
        2,
        "the failure, and the worker's end"
    );
    assert_eq!(
        receive(&named, "the worker's name"),
        "lane-contract-thread-door",
        "the worker runs as the thread door named it"
    );
}
