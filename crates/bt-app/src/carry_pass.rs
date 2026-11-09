//! **The start-time carry pass: one worker that brings what an older Folio wrote to this build's
//! form** (0.4.10 plan rows 67b and 68; the owner's ruling of 2026-11-04 that every new version
//! brings the old version's things along).
//!
//! **Every owned thing says how an update carries it** (row 68a, R1): each row of the uninstall
//! inventory (`uninstall::INVENTORY`) declares its [`CarryRule`]s — a document found by name and
//! walked forward by its reads, bytes this build writes again where they differ, an entry that
//! names a program and is checked by this pass, a transaction's own, a process's own, the
//! system's under an identity no copy changes, a ruled retirement, or nothing at all. The guard
//! beside the inventory (`uninstall_tests::every_owned_thing_says_how_an_update_carries_it`) holds
//! every row to one, every start writer of the update trial to a row, and every owner check to a
//! call in [`run_carry_steps`] or to the part that owes it (`Owed`).
//!
//! One pass and not one worker per thing (row 68, R1): a list of steps, run in order on one
//! below-normal worker started through the thread door, never on the window thread. Each step asks
//! the update trial's gate for its own writer before it writes (`update_trial::defer`, the
//! `explorer_menu::probe_at_start` shape) and answers its part of the round; the round is
//! published as one [`CarrySnapshot`] and wakes the window once, and the window only adopts.
//! Every start begins it ([`begin_carry_at_start`], the `startup::FirstWindow` step after the
//! script's), and so do the moments its members name.
//!
//! **Its first member is the agent hooks** ([`crate::agent_hooks_step`], row 67b, with row 69c's
//! codex version). Row 68b (the Explorer registrations, the update entrances' orphans) adds its
//! members to [`run_carry_steps`] rather than a second worker. **Its last member is Folio's old
//! `$PROFILE` lines** (`shell_integration::run_profile_lines_step`, row 68c), run once a process
//! and after the round so far is published: the one member that starts a PowerShell, so nothing
//! the window adopts — the hooks' answers, and the first-run card that waits on them (row 73) —
//! waits for one (the coordinator's R8 of 2026-11-09).
//!
//! A request while the pass runs is not lost and not doubled: it is written down, and the worker
//! runs every step once more before it ends ([`begin_carry_pass_with`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::agent_hooks_step::HooksSnapshot;
use crate::update_trial::Writer;

/// **How an update brings one owned thing to this build's form** (0.4.10 plan row 68a, R1) —
/// declared on every row of the uninstall inventory, a row holding as many as it carries things.
///
/// Read by the guard beside the inventory and by nothing in the product: the rule is what the
/// code does, said where the thing is listed, so a new thing or a new start writer cannot land
/// without saying it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CarryRule {
    /// Found by name in a folder every copy of the account shares, and brought forward by its
    /// `readers` (a document's migration chain, a record's versioned read); `writers` are the
    /// update trial's writers that hold its writes. **`readers` and `writers` both empty** is a
    /// thing no build reads back (a copy of the person's bytes, a staging folder, a log): every
    /// build's shape is the same, so there is nothing to carry.
    Document {
        readers: &'static [&'static str],
        writers: &'static [Writer],
    },
    /// Rewritten from this build's own bytes wherever it differs, by the writer named.
    BuildBytes(Writer),
    /// Names a program: the pass classifies each entry, repairs only this copy's old shape and a
    /// dead owner's through the switch's own writer, held during a trial by `writer`.
    OwnerChecked { check: CarryCheck, writer: Writer },
    /// Exists for one transaction — an update's, an uninstall's — and is retired by it.
    Transaction,
    /// Made again by each process; `held` is the trial writer that holds it, where one does.
    Process { held: Option<Writer> },
    /// Kept by the system under an identity no copy changes (the toast identity, the bundle id);
    /// `held` as for [`Self::Process`].
    Identity { held: Option<Writer> },
    /// Written only by older Folios, kept or offered by the dated ruling named.
    Retired { ruling: &'static str },
    /// Nothing writes it.
    Absent,
}

/// **One owner check of the pass** — what a [`CarryRule::OwnerChecked`] row is checked by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CarryCheck {
    /// An agent's hooks, by its index in `agent_hooks_step::HOOKS_FAMILIES`.
    AgentHooks(usize),
    /// The Explorer first page's package registration.
    ExplorerPackage,
    /// The Explorer classic verb.
    ExplorerClassic,
    /// Folio's own old `$PROFILE` line forms.
    ProfileLines,
}

/// **A check the inventory declares and a later part owes the pass** (0.4.10 plan row 68a, the
/// named seam; R-A of 2026-11-07): the part, the call it still runs by today in
/// `Runtime::create`, and the test that part lands with.
///
/// **It cannot outlive its part** (the review's N2). The guard holds `runs_today` to
/// `Runtime::create`'s statements, holds `landed_by` to being no test of the program yet, and
/// reads every member [`run_carry_steps`] hands the worker's context as a declared check: a part
/// that moves its check takes the call out of `Runtime::create`, one that lands its test makes
/// `landed_by` found, and one that adds a member leaves a call the guard reads as undeclared —
/// each is red until the check answers `Ok` here instead.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owed {
    pub(crate) part: &'static str,
    pub(crate) runs_today: &'static str,
    /// The test the part lands with — its module and its name — which the brief of that part
    /// names as its proof.
    pub(crate) landed_by: (&'static str, &'static str),
}

/// **A check that runs in the pass**: its member's call in [`run_carry_steps`], and the sites it
/// ran by before it was a member, which `Runtime::create` must no longer have (the review's N2: a
/// part that adds its member and forgets the old site would run the check twice).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PassMember {
    pub(crate) call: &'static str,
    pub(crate) retired: &'static [&'static str],
}

#[cfg(test)]
impl CarryCheck {
    /// **Where the check runs**: `Ok` with its member in [`run_carry_steps`], or the part that owes
    /// the move.
    pub(crate) fn in_the_pass(self) -> Result<PassMember, Owed> {
        match self {
            // The hooks' own wake left the start when the pass took the round (row 68a).
            Self::AgentHooks(_) => Ok(PassMember {
                call: "crate::agent_hooks_step::run_agent_hooks_step",
                retired: &["agent_hooks_step::install_hooks_wake("],
            }),
            Self::ExplorerPackage => Err(Owed {
                part: "68b",
                runs_today: "explorer_menu::begin_probe();",
                landed_by: (
                    "crate::explorer_menu::tests",
                    "the_carry_pass_probes_the_package_registration",
                ),
            }),
            Self::ExplorerClassic => Err(Owed {
                part: "68b",
                runs_today: "context_menu_installed: context_menu::reassert(),",
                landed_by: (
                    "crate::context_menu::tests",
                    "the_carry_pass_reasserts_the_classic_verb",
                ),
            }),
            // The startup's own migration worker left the start when the pass took the lines
            // (row 68c).
            Self::ProfileLines => Ok(PassMember {
                call: "crate::shell_integration::run_profile_lines_step",
                retired: &["migration: shell_integration::begin_startup_migration,"],
            }),
        }
    }

    /// **The test that brings the old shape this check repairs to today's**: its module and its
    /// name. `None` only where the ruling that owes the check owes the test too (the classic verb,
    /// 68b's finding, R-C of 2026-11-07).
    pub(crate) fn old_shape_test(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::AgentHooks(1) => Some((
                "crate::agent_hooks_step::tests",
                "a_gone_copys_codex_hook_and_line_are_repaired_into_the_hook",
            )),
            Self::AgentHooks(_) => Some((
                "crate::agent_hooks_step::tests",
                "every_historical_shape_of_this_copy_is_brought_to_the_current_one",
            )),
            Self::ExplorerPackage => Some((
                "crate::explorer_menu::tests",
                "an_older_registration_is_renewed_only_after_the_trial_commits",
            )),
            Self::ExplorerClassic => None,
            Self::ProfileLines => Some((
                "crate::shell_integration::profile_marks::tests",
                "the_carry_pass_brings_folios_old_profile_lines_forward",
            )),
        }
    }
}

/// **A member's answer for one round**: its part of the snapshot, how many checks it ran, and how
/// many of them said something new (`changed`, the diagnostics lines it wrote).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemberAnswer<T> {
    pub(crate) part: T,
    pub(crate) checks: usize,
    pub(crate) changed: usize,
}

/// **One round's answers, member by member**; `None` for a member that had nothing to look at yet
/// (the agents' before the program walk has answered) or nothing to do in it (the `$PROFILE`
/// lines', which run once a process, and are published on their own after the rest).
#[derive(Debug, Default)]
pub(crate) struct CarryRound {
    pub(crate) hooks: Option<MemberAnswer<HooksSnapshot>>,
    /// Folio's old `$PROFILE` lines (row 68c): counted in the round's line, nothing for the window.
    pub(crate) profile_lines: Option<MemberAnswer<()>>,
}

/// **What the window adopts**: the newest part of every member, kept until the window takes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CarrySnapshot {
    pub(crate) hooks: Option<HooksSnapshot>,
}

/// **The pass's one published snapshot** (row 68a): written at the end of each round, taken by
/// the window.
pub(crate) struct CarryPublished {
    snapshot: Mutex<Option<CarrySnapshot>>,
}

impl CarryPublished {
    pub(crate) const fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
        }
    }

    /// **Put one round where the window takes it**, and answer its line.
    ///
    /// A member that answered replaces its part, handing on the notices a part the window has not
    /// taken still owes; a member that did not answer leaves the untaken part as it was. The line,
    /// one per round (R-D of 2026-11-07), says how many checks ran and how many said something
    /// new, so a quiet round is in the log too. **A round no member answered** (the start's own,
    /// before the program walk has answered) publishes nothing and has no line (the review's N8):
    /// a round is empty only when every part is `None`. The round is taken apart with no `..`, so
    /// a part a later member adds does not compile here until it is handled — in the empty test
    /// and in the merge (the re-review's R2).
    pub(crate) fn publish(&self, round: CarryRound) -> Option<String> {
        let CarryRound {
            hooks,
            profile_lines,
        } = round;
        if hooks.is_none() && profile_lines.is_none() {
            return None;
        }
        let mut published = self.snapshot.lock().unwrap_or_else(PoisonError::into_inner);
        let untaken = published.take().unwrap_or_default();
        let (mut checks, mut changed) = (0, 0);
        let hooks = match hooks {
            Some(answer) => {
                checks += answer.checks;
                changed += answer.changed;
                Some(answer.part.keeping_untaken(untaken.hooks))
            }
            None => untaken.hooks,
        };
        if let Some(answer) = profile_lines {
            checks += answer.checks;
            changed += answer.changed;
        }
        *published = Some(CarrySnapshot { hooks });
        Some(format!("BT_CARRY round checks={checks} changed={changed}"))
    }

    /// **The newest snapshot, once** — the window's adoption.
    pub(crate) fn take(&self) -> Option<CarrySnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// This process's published snapshot.
static CARRY_PUBLISHED: CarryPublished = CarryPublished::new();

/// How a round asks for a frame — installed once, at startup ([`install_carry_wake`]).
static CARRY_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install the wake, once.
pub(crate) fn install_carry_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = CARRY_WAKE.set(Box::new(wake));
}

/// **The window's adoption**: the snapshot the newest rounds published, once.
pub(crate) fn take_carry_snapshot() -> Option<CarrySnapshot> {
    CARRY_PUBLISHED.take()
}

/// **One round of the pass: every member, in the order the pass runs them**, then the round
/// published and the window woken once — when a member answered. **The `$PROFILE` lines run last,
/// after what the others answered is published** (row 68c, R8): they may start a PowerShell to ask
/// where its `$PROFILE` is, and nothing the window adopts waits for that; their answer is published
/// as the round's second line, at its end. Each member is one run of a
/// step on the pass's worker, handed the worker's context — a step may ask a program a question
/// there, as the agent hooks ask codex its version (0.4.10 plan row 69c) — and asks the trial's
/// gate for its own writer before it writes.
///
/// Each member is called by name, never through a list of function pointers: the worker's context
/// is followed from the thread door to every function it is handed along calls the source guards
/// resolve (`window_waits_tests::every_worker_door_body_is_reached_from_a_completions_door`), and a
/// call through a pointer is one they do not follow. The inventory's guard reads this body for its
/// members (`CarryCheck::in_the_pass`).
pub(crate) fn run_carry_steps(ctx: &WorkerCtx) {
    publish_carry_round(CarryRound {
        hooks: crate::agent_hooks_step::run_agent_hooks_step(ctx),
        profile_lines: None,
    });
    publish_carry_round(CarryRound {
        hooks: None,
        profile_lines: crate::shell_integration::run_profile_lines_step(ctx),
    });
}

/// Publish what a round's members answered, say its line, and wake the window once — when one did.
fn publish_carry_round(round: CarryRound) {
    if let Some(line) = CARRY_PUBLISHED.publish(round) {
        crate::diagnostics::stderr_line(line);
        if let Some(wake) = CARRY_WAKE.get() {
            wake();
        }
    }
}

/// **That a start began the carry pass** (0.4.10 plan row 68a): the value
/// `startup::FirstWindow::carry` must answer, and only [`begin_carry_at_start`] makes one.
pub(crate) struct CarryBegun(());

#[cfg(test)]
impl CarryBegun {
    /// The headless start's stand-in for the step.
    pub(crate) fn stand_in() -> Self {
        Self(())
    }
}

/// **The start's step: begin the pass** — after the script's preparation and before the window,
/// on its worker, which nobody waits for. A member with nothing to look at yet (the agents' before
/// the program walk has answered) answers nothing this round; the moment it can, it begins the
/// pass again.
pub(crate) fn begin_carry_at_start() -> CarryBegun {
    begin_carry_pass();
    CarryBegun(())
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

/// **Run the pass**, on its worker — at every start ([`begin_carry_at_start`]), and at the moments
/// the members' rulings name: after the launch's first program walk has said where the agents keep
/// their files, when a walk moves one of them, at each open of the Agents page, after a press on
/// one of its rows, and when an update's trial is committed
/// (`update_trial::Writer::AgentHooksRepair`, and `Writer::ProfileMigration` for the `$PROFILE`
/// lines the trial held back).
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

    fn hooks_part(notices: Vec<crate::i18n::Text>) -> HooksSnapshot {
        use crate::attention_ownership::HooksFound;
        HooksSnapshot {
            found: [
                HooksFound::Current,
                HooksFound::Nothing,
                HooksFound::Nothing,
            ],
            unrepaired: [None; 3],
            copilot_readiness: crate::attention_copilot::Readiness::Unknown,
            notices,
        }
    }

    /// RED (0.4.10 plan row 68a) — **a round is one snapshot and one line**: what its members
    /// answered is taken once by the window, and the round's line counts the checks that ran and
    /// the ones that said something new — a quiet round says so too (R-D of 2026-11-07); a round
    /// no member answered says nothing and publishes nothing (the review's N8).
    ///
    /// MUTATION: a round no member answered is said (`plant-68a-empty-round-said.py`).
    #[test]
    fn a_round_publishes_one_snapshot_and_its_line() {
        use crate::i18n::Text;
        let published = CarryPublished::new();
        let line = published.publish(CarryRound {
            hooks: Some(MemberAnswer {
                part: hooks_part(vec![Text::AgentHooksRepairedClaude]),
                checks: 3,
                changed: 2,
            }),
            profile_lines: None,
        });
        assert_eq!(line.as_deref(), Some("BT_CARRY round checks=3 changed=2"));
        let taken = published.take().expect("the round is published");
        assert_eq!(
            taken.hooks,
            Some(hooks_part(vec![Text::AgentHooksRepairedClaude]))
        );
        assert_eq!(published.take(), None, "and taken once");
        assert_eq!(
            published.publish(CarryRound::default()),
            None,
            "a round whose members had nothing to look at says nothing"
        );
        assert_eq!(published.take(), None, "and publishes nothing");
    }

    /// RED (0.4.10 plan row 68a) — **a notice the window has not taken survives the rounds after
    /// it**: a round with no answer from the agents keeps their untaken part whole, and a round
    /// that answers again hands the untaken notices on, before its own.
    ///
    /// MUTATION: `CarryPublished::publish` drops the untaken part
    /// (`plant-68a-untaken-notice-dropped.py`).
    #[test]
    fn an_untaken_notice_survives_the_rounds_after_it() {
        use crate::i18n::Text;
        let published = CarryPublished::new();
        let answer = |notices| {
            Some(MemberAnswer {
                part: hooks_part(notices),
                checks: 3,
                changed: 1,
            })
        };
        published.publish(CarryRound {
            hooks: answer(vec![Text::AgentHooksRepairedClaude]),
            profile_lines: None,
        });
        published.publish(CarryRound::default());
        published.publish(CarryRound {
            hooks: answer(vec![Text::AgentHooksRepairedCopilot]),
            profile_lines: None,
        });
        assert_eq!(
            published.take().and_then(|snapshot| snapshot.hooks),
            Some(hooks_part(vec![
                Text::AgentHooksRepairedClaude,
                Text::AgentHooksRepairedCopilot,
            ]))
        );
    }

    /// RED (0.4.10 plan row 68c, the coordinator's R8 of 2026-11-09) — **the `$PROFILE` lines run
    /// last, after the round before them is published**: `run_carry_steps` publishes what the
    /// earlier members answered before it calls the one member that may start a PowerShell, and
    /// publishes that member's answer at the end; its round is said with its own counts, leaves
    /// the window's untaken part as it was, and a round no member answered still says nothing.
    ///
    /// MUTATIONS: the member is dropped from the pass (`plant-68c-member-not-in-the-pass.py`); a
    /// round no member answered is said (`plant-68c-empty-round-said.py`).
    #[test]
    fn the_profile_lines_are_published_after_the_round_before_them() {
        use crate::i18n::Text;
        let steps =
            crate::test_support::squeezed(crate::test_support::free_fn_body("run_carry_steps"));
        let hooks = steps
            .find("crate::agent_hooks_step::run_agent_hooks_step(ctx)")
            .expect("the hooks are a member");
        let lines = steps
            .find("crate::shell_integration::run_profile_lines_step(ctx)")
            .expect("the $PROFILE lines are a member");
        let rounds: Vec<usize> = steps
            .match_indices("publish_carry_round(")
            .map(|(at, _)| at)
            .collect();
        assert_eq!(rounds.len(), 2, "{steps}");
        assert!(
            rounds[0] < hooks && hooks < rounds[1] && rounds[1] < lines,
            "the hooks' round is published before the $PROFILE lines run: {steps}"
        );

        let published = CarryPublished::new();
        published.publish(CarryRound {
            hooks: Some(MemberAnswer {
                part: hooks_part(vec![Text::AgentHooksRepairedClaude]),
                checks: 3,
                changed: 1,
            }),
            profile_lines: None,
        });
        assert_eq!(
            published
                .publish(CarryRound {
                    hooks: None,
                    profile_lines: Some(MemberAnswer {
                        part: (),
                        checks: 2,
                        changed: 1,
                    }),
                })
                .as_deref(),
            Some("BT_CARRY round checks=2 changed=1")
        );
        assert_eq!(
            published.take().and_then(|snapshot| snapshot.hooks),
            Some(hooks_part(vec![Text::AgentHooksRepairedClaude])),
            "the window's part is as the round before left it"
        );
        assert_eq!(published.publish(CarryRound::default()), None);
    }
}
