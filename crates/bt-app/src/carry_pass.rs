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
//! carry rule's other owner-checked things to [`CARRY_STEPS`] rather than a second worker.
//!
//! A request while the pass runs is not lost and not doubled: it is written down, and the worker
//! runs every step once more before it ends ([`begin_carry_pass_with`]).

use std::sync::atomic::{AtomicBool, Ordering};

/// **Every member, in the order the pass runs them**: each is one run of a step on the pass's
/// worker, and asks the trial's gate for its own writer before it writes.
pub(crate) const CARRY_STEPS: [fn(); 1] = [crate::agent_hooks_step::run_agent_hooks_step];

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

    /// `true` when the caller is to start a worker; `false` when one is running and has been told
    /// to go round once more.
    fn ask_for_round(&self) -> bool {
        if self.running.swap(true, Ordering::AcqRel) {
            self.again.store(true, Ordering::Release);
            false
        } else {
            true
        }
    }

    /// The worker's question at the end of a round: go round again, or end. A request that lands
    /// between the two halves either finds the worker still running (and is taken here) or finds
    /// it gone and starts its own.
    fn round_again(&self) -> bool {
        if self.again.swap(false, Ordering::AcqRel) {
            return true;
        }
        self.running.store(false, Ordering::Release);
        self.again.swap(false, Ordering::AcqRel) && !self.running.swap(true, Ordering::AcqRel)
    }

    /// A worker that never started, or that unwound: the next request starts one.
    fn carry_ended(&self) {
        self.running.store(false, Ordering::Release);
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
        &CARRY_STEPS,
    );
}

/// [`begin_carry_pass`] with the latch, the executor, the voice and the steps handed in — the
/// seam the tests press. `true` when a worker was started, or one running was told to go round
/// again.
///
/// **A start the door refuses is said once** and the latch let go, so the next request starts
/// one; nothing was asked of the trial's gate, so nothing is pending for a commit either. A step
/// that panics ends the worker, said once the same way; the steps after it run at the next start.
pub(crate) fn begin_carry_pass_with(
    latch: &'static CarryLatch,
    executor: &crate::lane::Executor,
    say: impl Fn(&str) + Send + Sync + 'static,
    steps: &'static [fn()],
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
            move |_ctx| loop {
                for step in steps {
                    step();
                }
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
        assert!(
            !LATCH.ask_for_round(),
            "a second, while it runs, starts none"
        );
        assert!(LATCH.round_again(), "and is the worker's next round");
        assert!(!LATCH.round_again(), "after which it ends");
        assert!(
            LATCH.ask_for_round(),
            "and the next request starts a worker again"
        );
        LATCH.carry_ended();
    }

    /// **Every step runs, in order, on the pass's worker**, and the pass's end lets the latch go.
    #[test]
    fn the_pass_runs_every_step_on_its_worker() {
        static LATCH: CarryLatch = CarryLatch::new();
        static RAN: AtomicUsize = AtomicUsize::new(0);
        fn first() {
            assert_eq!(
                RAN.fetch_add(1, Ordering::AcqRel),
                0,
                "the first step runs first"
            );
        }
        fn second() {
            assert_eq!(
                RAN.fetch_add(1, Ordering::AcqRel),
                1,
                "the second runs after it"
            );
        }
        static STEPS: [fn(); 2] = [first, second];
        let executor = crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal);
        assert!(begin_carry_pass_with(&LATCH, &executor, |_| {}, &STEPS));
        let waited = std::time::Instant::now();
        while LATCH.running.load(Ordering::Acquire) {
            assert!(
                waited.elapsed() < std::time::Duration::from_secs(30),
                "the pass ends"
            );
            std::thread::yield_now();
        }
        assert_eq!(RAN.load(Ordering::Acquire), 2);
    }
}
