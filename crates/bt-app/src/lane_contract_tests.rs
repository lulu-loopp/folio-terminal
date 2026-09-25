//! **The lane contract's suite, run on every lane** (D-33; `crate::lane`; `docs/plans/design/
//! window-thread-budget-2026-09-25.md` §R-D).
//!
//! Four adapters — the OS hand-off lane (the reference), the font lane, the taskbar lane and the
//! computation lane — each run every [`Claim`]; [`judge`] holds the verdicts to
//! [`EXPECTED_FAILURES`]. The computation lane's adapter lives here because its lane lives in
//! `main.rs`; the others live beside their lanes, where the lanes' private parts are.

use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};

use super::*;
use crate::lane::{
    Adapter, Admission, Bound, COMPUTATION, Claim, Contract, Delivered, EXPECTED_FAILURES,
    ExpectedFailure, Failure, FailureKind, Gate, HANDOFF, LaneName, LaneUnderTest, Outcome,
    Verdict, WakeProbe, judge, run_suite,
};

/// Every lane's adapter. A lane missing here is a lane [`judge`] reports as skipped.
fn adapters() -> [Adapter; 4] {
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
