//! **The start-time carry pass: one worker that brings what an older Folio wrote to this build's
//! form** (0.4.10 plan rows 67b and 68; the owner's ruling of 2026-11-04 that every new version
//! brings the old version's things along).
//!
//! One pass and not one worker per thing (row 68, R1): a list of steps, run in order on one
//! below-normal worker started through the thread door, never on the window thread. Each step asks
//! the update trial's gate for its own writer before it writes (`update_trial::defer`, the
//! `explorer_menu::probe_at_start` shape), publishes what it found, and wakes the window, which
//! only adopts.
//!
//! **Its first member is the agent hooks** ([`crate::agent_hooks_step`], row 67b); row 68a adds the
//! carry rule's other owner-checked things to [`run_carry_steps`] rather than a second worker.
//!
//! A request while the pass runs is not lost and not doubled: it is written down, and the worker
//! runs every step once more before it ends ([`begin_carry_pass_with`]).

use std::sync::atomic::{AtomicBool, Ordering};

use bt_platform::admission::WorkerCtx;

/// **One round of the pass: every member, in the order the pass runs them**. Each is one run of a
/// step on the pass's worker, handed the worker's context — a step may ask a program a question
/// there, as the agent hooks ask codex its version (0.4.10 plan row 69c) — and asks the trial's
/// gate for its own writer before it writes.
///
/// Each member is called by name, never through a list of function pointers: the worker's context
/// is followed from the thread door to every function it is handed along calls the source guards
/// resolve (`window_waits_tests::every_worker_door_body_is_reached_from_a_completions_door`), and a
/// call through a pointer is one they do not follow.
pub(crate) fn run_carry_steps(ctx: &WorkerCtx) {
    crate::agent_hooks_step::run_agent_hooks_step(ctx);
}

/// The pass's two facts between its starts: whether a worker is running it, and whether somebody
/// asked again while one was.
pub(crate) struct CarryLatch {
    running: AtomicBool,
    again: AtomicBool,
}

impl CarryLatch {
    pub(crate) const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            again: AtomicBool::new(false),
        }
    }

    /// `true` when the caller is to start a worker; `false` when one is running and will go round
    /// once more.
    ///
    /// **The request is written down before the worker is asked about**: a worker that is letting
    /// the latch go reads it after it has let go ([`Self::let_the_pass_go`]), so a request is
    /// either taken by the running worker or finds none running and starts its own — never neither.
    /// Every operation is sequentially consistent: the two sides each write one flag and then read
    /// the other, which no weaker order makes see each other.
    fn ask_for_round(&self) -> bool {
        self.again.store(true, Ordering::SeqCst);
        !self.running.swap(true, Ordering::SeqCst)
    }

    /// The worker's first act of each round: what was asked before now, this round answers.
    fn round_begins(&self) {
        self.again.store(false, Ordering::SeqCst);
    }

    /// The worker's question at the end of a round: go round again, or end.
    fn round_again(&self) -> bool {
        self.asked_since_round_began() || !self.let_the_pass_go()
    }

    /// Whether a request was written down since this round began.
    fn asked_since_round_began(&self) -> bool {
        self.again.load(Ordering::SeqCst)
    }

    /// The worker lets the latch go: `true` when it has; `false` when a request landed as it let
    /// go and the worker took the latch back for it (a request that started a worker of its own
    /// meanwhile leaves this one free to end).
    fn let_the_pass_go(&self) -> bool {
        self.running.store(false, Ordering::SeqCst);
        !self.asked_since_round_began() || self.running.swap(true, Ordering::SeqCst)
    }

    /// A worker that never started, or that unwound: the next request starts one.
    fn carry_ended(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// This process's pass.
static CARRY_LATCH: CarryLatch = CarryLatch::new();

/// **Run the pass**, on its worker — at the moments the members' rulings name: after the launch's
/// first program walk has said where the agents keep their files, when a walk moves one of them,
/// at each open of the Agents page, after a press on one of its rows, and when an update's trial
/// is committed (`update_trial::Writer::AgentHooksRepair`).
pub(crate) fn begin_carry_pass() {
    begin_carry_pass_with(
        &CARRY_LATCH,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
        crate::diagnostics::note,
        run_carry_steps,
    );
}

/// [`begin_carry_pass`] with the latch, the executor, the voice and the round's steps handed in —
/// the seam the tests press. `true` when a worker was started, or one running was told to go round
/// again.
///
/// **A start the door refuses is said once** and the latch let go, so the next request starts
/// one; nothing was asked of the trial's gate, so nothing is pending for a commit either. A step
/// that panics ends the worker, said once the same way; the steps after it run at the next start.
pub(crate) fn begin_carry_pass_with(
    latch: &'static CarryLatch,
    executor: &crate::lane::Executor,
    say: impl Fn(&str) + Send + Sync + 'static,
    steps: fn(&WorkerCtx),
) -> bool {
    if !latch.ask_for_round() {
        return true;
    }
    executor
        .start_worker(
            "folio-carry-pass",
            crate::lane::Answered::new("the start-time carry pass", say, move |_end| {
                latch.carry_ended();
            }),
            move |ctx| loop {
                latch.round_begins();
                steps(ctx);
                if !latch.round_again() {
                    break;
                }
            },
        )
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// **A request while the pass runs is run once more, not lost and not doubled**: the second ask
    /// starts no worker, and the worker goes round again for it.
    #[test]
    fn a_request_while_the_pass_runs_is_one_more_round() {
        static LATCH: CarryLatch = CarryLatch::new();
        assert!(LATCH.ask_for_round(), "the first request starts the worker");
        LATCH.round_begins();
        assert!(
            !LATCH.ask_for_round(),
            "a second, while it runs, starts none"
        );
        assert!(LATCH.round_again(), "and is the worker's next round");
        LATCH.round_begins();
        assert!(!LATCH.round_again(), "after which it ends");
        assert!(
            LATCH.ask_for_round(),
            "and the next request starts a worker again"
        );
        LATCH.carry_ended();
    }

    /// **A request that lands while the worker lets the latch go is not lost**: asked after the
    /// worker found nothing written down and before it let go, it starts no worker of its own, and
    /// the worker takes the latch back and goes round for it.
    ///
    /// The requester's half — the request written down before the worker is asked about — is
    /// [`CarryLatch::ask_for_round`]'s order, which this sequence cannot interleave; this pins the
    /// worker's half, which reads the book again after it lets go.
    ///
    /// MUTATION: the worker lets go without reading again (`plant-67b-worker-ends-blind.py`).
    #[test]
    fn a_request_as_the_worker_lets_go_is_one_more_round() {
        static LATCH: CarryLatch = CarryLatch::new();
        assert!(LATCH.ask_for_round());
        LATCH.round_begins();
        assert!(
            !LATCH.asked_since_round_began(),
            "the worker finds nothing asked"
        );
        assert!(
            !LATCH.ask_for_round(),
            "the request finds the worker running"
        );
        assert!(
            !LATCH.let_the_pass_go(),
            "so the worker keeps the latch for it"
        );
        LATCH.round_begins();
        assert!(!LATCH.round_again(), "and ends after that round");
        assert!(
            LATCH.ask_for_round(),
            "the latch is free for the next request"
        );
        LATCH.carry_ended();
    }

    /// **Every step runs, in order, on the pass's worker**, and the pass's end lets the latch go.
    #[test]
    fn the_pass_runs_every_step_on_its_worker() {
        static LATCH: CarryLatch = CarryLatch::new();
        static RAN: AtomicUsize = AtomicUsize::new(0);
        fn first(_: &WorkerCtx) {
            assert_eq!(
                RAN.fetch_add(1, Ordering::AcqRel),
                0,
                "the first step runs first"
            );
        }
        fn second(_: &WorkerCtx) {
            assert_eq!(
                RAN.fetch_add(1, Ordering::AcqRel),
                1,
                "the second runs after it"
            );
        }
        fn round(ctx: &WorkerCtx) {
            first(ctx);
            second(ctx);
        }
        let executor = crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal);
        // The worker's end is the signal: its voice holds the sender, and the worker lets the
        // voice go when it has ended — after the last round has let the latch go. A voice that is
        // used (a refused start, a step that panicked) sends instead.
        let (said, ended) = std::sync::mpsc::channel::<()>();
        let voice = move |_: &str| {
            let _ = said.send(());
        };
        assert!(begin_carry_pass_with(&LATCH, &executor, voice, round));
        assert!(
            ended.recv().is_err(),
            "the pass ended without a word: it started, and no step panicked"
        );
        assert_eq!(RAN.load(Ordering::Acquire), 2, "both steps ran");
        assert!(
            !LATCH.running.load(Ordering::Acquire),
            "the pass's end let the latch go"
        );
    }
}
