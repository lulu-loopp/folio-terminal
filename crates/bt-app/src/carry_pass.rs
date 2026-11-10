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
//! **Its members**: the agent hooks ([`crate::agent_hooks_step`], row 67b, with row 69c's codex
//! version); since row 68b the Explorer package registration
//! ([`crate::explorer_menu::run_package_step`], once a worker of its own), the Explorer classic
//! verb ([`crate::context_menu::run_classic_step`], once a read and a write on the window thread in
//! `Runtime::create`) and the update entrances whose rescue program is gone
//! ([`crate::update_orphans::run_orphans_step`]). Row 68c (Folio's old `$PROFILE` lines) adds its
//! own to [`run_carry_steps`] rather than a second worker.
//!
//! A request while the pass runs is not lost and not doubled: it is written down, and the worker
//! runs the members asked for once more before it ends ([`begin_carry_pass_with`]); each moment
//! asks for the members its ruling names ([`CarryAsk`], row 68b, R2).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use bt_platform::admission::WorkerCtx;

use crate::agent_hooks_step::HooksSnapshot;
use crate::context_menu::ClassicAnswer;
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
    /// An update's entrance whose rescue program is gone: the Windows `Run` value, the macOS
    /// LaunchAgent (row 68b, R5 of 2026-11-04).
    UpdateOrphans,
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

/// **A check that runs in the pass**: its member's call in [`run_carry_steps`], the sites it ran
/// by before it was a member, which neither `Runtime::create` nor `release_trial_writer` may have
/// any more (the review's N2: a part that adds its member and forgets the old site would run the
/// check twice; row 68b, R9: the release arm is a start site too), and the test it landed with,
/// which must stay (R9: a landed test is not deleted later unseen).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PassMember {
    pub(crate) call: &'static str,
    pub(crate) retired: &'static [&'static str],
    /// The test the member landed with — its module and its name, as `Owed::landed_by` named
    /// it before the part landed — which `bt_source` must find in that module's tests: a check
    /// moving from `Err(Owed { .. })` to `Ok(PassMember { .. })` carries its `landed_by` here.
    pub(crate) proved_by: (&'static str, &'static str),
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
                proved_by: (
                    "crate::agent_hooks_step::tests",
                    "the_window_thread_reads_no_agent_file_at_start",
                ),
            }),
            // Row 68b moved both Explorer checks in: their own worker and the window thread's
            // read are gone from the start and from the trial's release.
            Self::ExplorerPackage => Ok(PassMember {
                call: "crate::explorer_menu::run_package_step",
                retired: &[
                    "explorer_menu::begin_probe();",
                    "explorer_menu::begin_probe(",
                ],
                proved_by: (
                    "crate::explorer_menu::tests",
                    "the_carry_pass_probes_the_package_registration",
                ),
            }),
            Self::ExplorerClassic => Ok(PassMember {
                call: "crate::context_menu::run_classic_step",
                retired: &[
                    "context_menu_installed: context_menu::reassert(),",
                    "context_menu::reassert(",
                ],
                proved_by: (
                    "crate::context_menu::tests",
                    "the_carry_pass_reasserts_the_classic_verb",
                ),
            }),
            Self::UpdateOrphans => Ok(PassMember {
                call: "crate::update_orphans::run_orphans_step",
                retired: &[],
                proved_by: (
                    "crate::update_orphans::tests",
                    "the_sweep_waits_for_the_trials_commit",
                ),
            }),
            Self::ProfileLines => Err(Owed {
                part: "68c",
                runs_today: "migration: shell_integration::begin_startup_migration,",
                landed_by: (
                    "crate::shell_integration::profile_marks::tests",
                    "the_carry_pass_brings_folios_old_profile_lines_forward",
                ),
            }),
        }
    }

    /// **The test that brings the old shape this check repairs to today's**: its module and its
    /// name. `None` only where the ruling that owes the check owes the test too (none since row
    /// 68b wrote the classic verb's, R-C of 2026-11-07).
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
            Self::ExplorerClassic => Some((
                "crate::context_menu::tests",
                "every_historical_shape_of_the_classic_verb_is_brought_to_this_builds",
            )),
            Self::ProfileLines => Some((
                "crate::shell_integration::profile_marks::tests",
                "shell_integration_rewrite_table_preserves_every_other_byte",
            )),
            // One shape since 0.4.6: a dead entrance is its only old one.
            Self::UpdateOrphans => Some((
                "crate::update_orphans::tests",
                "a_dead_update_entrance_is_swept_and_a_live_one_is_left",
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

/// **A member's last line, said again only when it differs** (R-D's change rule; row 68b, R10): a
/// start says each member's finding once, and a later round — a commit's, a language change's —
/// says it again only when the finding changed.
pub(crate) struct LastSaid(Mutex<Option<String>>);

impl LastSaid {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Say `line` unless it is the last said; whether it was said.
    pub(crate) fn say_if_new(&self, line: String, say: &dyn Fn(String)) -> bool {
        let mut last = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if last.as_deref() == Some(line.as_str()) {
            return false;
        }
        say(line.clone());
        *last = Some(line);
        true
    }
}

/// **One round's answers, member by member**; `None` for a member that had nothing to look at yet
/// (the agents' before the program walk has answered).
/// The package's and the sweep's parts are bare answers (row 68b, R1): the package's facts stay in
/// `explorer_menu`'s own cells, which its press job writes too, and no row shows an entrance.
#[derive(Debug, Default)]
pub(crate) struct CarryRound {
    pub(crate) hooks: Option<MemberAnswer<HooksSnapshot>>,
    pub(crate) package: Option<MemberAnswer<()>>,
    pub(crate) classic: Option<MemberAnswer<ClassicAnswer>>,
    pub(crate) orphans: Option<MemberAnswer<()>>,
}

/// **What the window adopts**: the newest part of every member, kept until the window takes it.
/// `explorer` is the Explorer members' part: the classic verb's answer (row 68b).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CarrySnapshot {
    pub(crate) hooks: Option<HooksSnapshot>,
    pub(crate) explorer: Option<ClassicAnswer>,
}

/// One member's part of a round's line: how many checks it ran and how many said something new.
fn member_tally<T>(answer: Option<&MemberAnswer<T>>) -> (usize, usize) {
    answer.map_or((0, 0), |answer| (answer.checks, answer.changed))
}

/// **What a round asks for**: a set of the pass's members (row 68b, R2). A moment asks for the
/// members its ruling names, so an Agents-page open asks no deployment database and a trial's
/// release of the Explorer writer runs no agent's scan; the start asks for all of them.
///
/// **One bit per member**, four of the eight taken: a part that adds a member adds its bit here,
/// joins it into [`Self::ALL`], and names it at the asks its ruling gives (row 68c's profile lines
/// is the next one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CarryAsk(u8);

impl CarryAsk {
    /// The agents' hooks ([`crate::agent_hooks_step`]).
    pub(crate) const HOOKS: Self = Self(1);
    /// The Explorer first page's package registration.
    pub(crate) const EXPLORER_PACKAGE: Self = Self(1 << 1);
    /// The Explorer classic verb.
    pub(crate) const EXPLORER_CLASSIC: Self = Self(1 << 2);
    /// The update entrances whose rescue program is gone.
    pub(crate) const UPDATE_ORPHANS: Self = Self(1 << 3);
    /// Both Explorer registrations: what a trial's release of `Writer::ExplorerRepair` asks.
    pub(crate) const EXPLORER: Self = Self::EXPLORER_PACKAGE.joined_with(Self::EXPLORER_CLASSIC);
    /// Every member: the start's round.
    pub(crate) const ALL: Self = Self::HOOKS
        .joined_with(Self::EXPLORER)
        .joined_with(Self::UPDATE_ORPHANS);

    /// Both sets.
    pub(crate) const fn joined_with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether this set asks for `member`.
    pub(crate) const fn names_member(self, member: Self) -> bool {
        self.0 & member.0 == member.0
    }
}

/// `member`'s answer when the round asked for it; `Ok(None)`, and nothing run, when it did not.
///
/// **A member's panic withholds only its own answer** (row 68b, review N8): the member runs inside
/// the one unwind boundary (`contained_panic::contain`), so a panic in it is said once, named by
/// `name`, and answers [`MemberPanicked`], and the round goes on to the members after it and
/// publishes their answers. A member that holds a latch lets it go as it unwinds (the package
/// member's `LatchUnwound`); what a latch held behind it is the caller's to run
/// ([`package_member_with`]).
fn run_if_asked<T>(
    asked: CarryAsk,
    member: CarryAsk,
    name: &str,
    run: impl FnOnce() -> Option<MemberAnswer<T>>,
) -> Result<Option<MemberAnswer<T>>, MemberPanicked> {
    if !asked.names_member(member) {
        return Ok(None);
    }
    crate::contained_panic::contain(run).map_err(|panic| {
        crate::diagnostics::stderr_line(format!(
            "BT_CARRY member={name} panicked: {}",
            crate::pty_door::panic_message(panic.as_ref())
        ));
        MemberPanicked
    })
}

/// A member's run that ended in a panic, contained where it ran ([`run_if_asked`]): it answers
/// nothing this round.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MemberPanicked;

/// **The press of the Explorer first page a panicking holder of the deployment latch leaves
/// written down, and how it is run**: the executor the pass was handed, at the press's own band,
/// and the pass's voice (row 68b, re-review R1).
pub(crate) struct PressBehindTheLatch {
    executor: crate::lane::Executor,
    say: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
}

impl PressBehindTheLatch {
    /// Run the press written down while the latch was held, if one was
    /// (`explorer_menu::run_queued_press`): nothing written down starts nothing, and a press
    /// taken by a job still running is written down again.
    pub(crate) fn run_written_down(&self) {
        crate::explorer_menu::run_queued_press(&self.executor, &self.say);
    }
}

/// **The package member, run as [`run_if_asked`] runs any member — and, when it panicked, the
/// press written down behind its deployment latch run at once** (row 68b, re-review R1).
///
/// The member's panic is contained where it runs, so the worker does not end and the pass's end
/// never sees it; the latch let go as the member unwound (`explorer_menu::LatchUnwound`), and a
/// press of the Explorer first page that arrived while the member held it is otherwise written
/// down for nobody, until the next press clears it unread. Run here rather than after the round:
/// the hooks member after it may ask a program its version.
pub(crate) fn package_member_with(
    asked: CarryAsk,
    behind: &PressBehindTheLatch,
    member: impl FnOnce() -> Option<MemberAnswer<()>>,
) -> Option<MemberAnswer<()>> {
    let ran = run_if_asked(
        asked,
        CarryAsk::EXPLORER_PACKAGE,
        "explorer-package",
        member,
    );
    if ran.is_err() {
        behind.run_written_down();
    }
    ran.ok().flatten()
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
            package,
            classic,
            orphans,
        } = round;
        let parts = [
            member_tally(hooks.as_ref()),
            member_tally(package.as_ref()),
            member_tally(classic.as_ref()),
            member_tally(orphans.as_ref()),
        ];
        if hooks.is_none() && package.is_none() && classic.is_none() && orphans.is_none() {
            return None;
        }
        let checks: usize = parts.iter().map(|(checks, _)| checks).sum();
        let changed: usize = parts.iter().map(|(_, changed)| changed).sum();
        let mut published = self.snapshot.lock().unwrap_or_else(PoisonError::into_inner);
        let untaken = published.take().unwrap_or_default();
        let hooks = match hooks {
            Some(answer) => Some(answer.part.keeping_untaken(untaken.hooks)),
            None => untaken.hooks,
        };
        // A newer classic answer replaces an untaken one: it holds no notice.
        let explorer = classic.map(|answer| answer.part).or(untaken.explorer);
        *published = Some(CarrySnapshot { hooks, explorer });
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
/// published and the window woken once — when a member answered. Each member is one run of a
/// step on the pass's worker, handed the worker's context — a step may ask a program a question
/// there, as the agent hooks ask codex its version (0.4.10 plan row 69c) — and asks the trial's
/// gate for its own writer before it writes.
///
/// Each member is called by name, never through a list of function pointers: the worker's context
/// is followed from the thread door to every function it is handed along calls the source guards
/// resolve (`window_waits_tests::every_worker_door_body_is_reached_from_a_completions_door`), and a
/// call through a pointer is one they do not follow. The inventory's guard reads this body for its
/// members (`CarryCheck::in_the_pass`).
///
/// A round runs the members `asked` names (row 68b, R2), in this order (R11): the three that need
/// no program walk first, so the start's own round answers them, then the agents'. A member that
/// panics answers nothing ([`run_if_asked`]); the package member's panic also runs the press
/// written down behind its latch (`behind`, [`package_member_with`]).
pub(crate) fn run_carry_steps(ctx: &WorkerCtx, asked: CarryAsk, behind: &PressBehindTheLatch) {
    let package = package_member_with(asked, behind, || {
        crate::explorer_menu::run_package_step(ctx)
    });
    let classic = run_if_asked(
        asked,
        CarryAsk::EXPLORER_CLASSIC,
        "explorer-classic",
        || crate::context_menu::run_classic_step(ctx),
    )
    .ok()
    .flatten();
    let orphans = run_if_asked(asked, CarryAsk::UPDATE_ORPHANS, "update-orphans", || {
        crate::update_orphans::run_orphans_step(ctx)
    })
    .ok()
    .flatten();
    let round = CarryRound {
        hooks: run_if_asked(asked, CarryAsk::HOOKS, "agent-hooks", || {
            crate::agent_hooks_step::run_agent_hooks_step(ctx)
        })
        .ok()
        .flatten(),
        package,
        classic,
        orphans,
    };
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
    begin_carry_pass(CarryAsk::ALL);
    CarryBegun(())
}

/// The pass's two facts between its starts: whether a worker is running it, and which members
/// somebody asked for while one was (row 68b, R2: a set, where 68a kept one flag).
pub(crate) struct CarryLatch {
    running: AtomicBool,
    again: AtomicU8,
}

impl CarryLatch {
    pub(crate) const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            again: AtomicU8::new(0),
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
    fn ask_for_round(&self, members: CarryAsk) -> bool {
        self.again.fetch_or(members.0, Ordering::SeqCst);
        !self.running.swap(true, Ordering::SeqCst)
    }

    /// The worker's first act of each round: what was asked before now, this round answers — the
    /// members it is to run.
    fn round_begins(&self) -> CarryAsk {
        CarryAsk(self.again.swap(0, Ordering::SeqCst))
    }

    /// The worker's question at the end of a round: go round again, or end.
    fn round_again(&self) -> bool {
        self.asked_since_round_began() || !self.let_the_pass_go()
    }

    /// Whether a request was written down since this round began.
    fn asked_since_round_began(&self) -> bool {
        self.again.load(Ordering::SeqCst) != 0
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
/// (`update_trial::Writer::AgentHooksRepair`, `ExplorerRepair`, `OrphanSweep`) — each asking for
/// the members its ruling names (row 68b, R2).
pub(crate) fn begin_carry_pass(members: CarryAsk) {
    begin_carry_pass_with(
        &CARRY_LATCH,
        &crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
        crate::diagnostics::note,
        members,
        run_carry_steps,
    );
}

/// [`begin_carry_pass`] with the latch, the executor, the voice and the round's steps handed in —
/// the seam the tests press. `true` when a worker was started, or one running was told to go round
/// again.
///
/// **A start the door refuses is said once** and the latch let go, so the next request starts
/// one; nothing was asked of the trial's gate, so nothing is pending for a commit either. A member
/// that panics withholds only its own answer, said once where it runs ([`run_if_asked`]); **a press
/// of the Explorer first page written down behind the package member that panicked holding the
/// deployment latch is run right after it** ([`package_member_with`], through the
/// [`PressBehindTheLatch`] the round is handed: this executor at the press's band, this voice). A
/// panic outside the members ends the worker, said once the same way, the next ask starts one, and
/// the pass's end runs a press written down meanwhile (`explorer_menu::run_queued_press`, the
/// probe's own end before row 68b).
pub(crate) fn begin_carry_pass_with(
    latch: &'static CarryLatch,
    executor: &crate::lane::Executor,
    say: impl Fn(&str) + Send + Sync + 'static,
    members: CarryAsk,
    steps: fn(&WorkerCtx, CarryAsk, &PressBehindTheLatch),
) -> bool {
    if !latch.ask_for_round(members) {
        return true;
    }
    let say: std::sync::Arc<dyn Fn(&str) + Send + Sync> = std::sync::Arc::new(say);
    let voice = std::sync::Arc::clone(&say);
    // A press the package member's latch held is a person's: run at the press's own band.
    let press = executor.with_band(bt_platform::ThreadPriority::Normal);
    let behind = PressBehindTheLatch {
        executor: press.clone(),
        say: std::sync::Arc::clone(&say),
    };
    executor
        .start_worker(
            "folio-carry-pass",
            crate::lane::Answered::new(
                "the start-time carry pass",
                move |line| voice(line),
                move |end| {
                    latch.carry_ended();
                    if end.started() {
                        crate::explorer_menu::run_queued_press(&press, &say);
                    }
                },
            ),
            move |ctx| loop {
                let asked = latch.round_begins();
                steps(ctx, asked, &behind);
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

    /// RED (0.4.10 plan row 68b, review N8) — **a member's panic withholds only its own answer**:
    /// the member that panics answers nothing, the members after it still run, and the round
    /// publishes their answers, so the window adopts them.
    ///
    /// MUTATION: run the member bare again (`plant-68b-member-panic-ends-the-round.py`) and the
    /// panic unwinds out of the round: this test goes red at its first member.
    #[test]
    fn a_members_panic_withholds_only_its_own_answer() {
        let classic = run_if_asked(
            CarryAsk::ALL,
            CarryAsk::EXPLORER_CLASSIC,
            "explorer-classic",
            || -> Option<MemberAnswer<ClassicAnswer>> { panic!("scripted: a member's fault") },
        );
        assert_eq!(
            classic,
            Err(MemberPanicked),
            "the member that panicked answers nothing"
        );
        let classic = classic.ok().flatten();
        let ran_after = std::cell::Cell::new(false);
        let orphans = run_if_asked(
            CarryAsk::ALL,
            CarryAsk::UPDATE_ORPHANS,
            "update-orphans",
            || {
                ran_after.set(true);
                Some(MemberAnswer {
                    part: (),
                    checks: 1,
                    changed: 1,
                })
            },
        );
        assert!(ran_after.get(), "the member after it runs");
        let orphans = orphans.expect("it answered");
        let published = CarryPublished::new();
        assert_eq!(
            published
                .publish(CarryRound {
                    hooks: None,
                    package: None,
                    classic,
                    orphans,
                })
                .as_deref(),
            Some("BT_CARRY round checks=1 changed=1"),
            "and the round publishes its answer"
        );
        assert!(published.take().is_some(), "for the window to adopt");
    }

    /// **A request while the pass runs is run once more, not lost and not doubled**: the second ask
    /// starts no worker, and the worker goes round again for it.
    #[test]
    fn a_request_while_the_pass_runs_is_one_more_round() {
        static LATCH: CarryLatch = CarryLatch::new();
        assert!(
            LATCH.ask_for_round(CarryAsk::HOOKS),
            "the first request starts the worker"
        );
        LATCH.round_begins();
        assert!(
            !LATCH.ask_for_round(CarryAsk::HOOKS),
            "a second, while it runs, starts none"
        );
        assert!(LATCH.round_again(), "and is the worker's next round");
        LATCH.round_begins();
        assert!(!LATCH.round_again(), "after which it ends");
        assert!(
            LATCH.ask_for_round(CarryAsk::HOOKS),
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
        assert!(LATCH.ask_for_round(CarryAsk::HOOKS));
        LATCH.round_begins();
        assert!(
            !LATCH.asked_since_round_began(),
            "the worker finds nothing asked"
        );
        assert!(
            !LATCH.ask_for_round(CarryAsk::HOOKS),
            "the request finds the worker running"
        );
        assert!(
            !LATCH.let_the_pass_go(),
            "so the worker keeps the latch for it"
        );
        LATCH.round_begins();
        assert!(!LATCH.round_again(), "and ends after that round");
        assert!(
            LATCH.ask_for_round(CarryAsk::HOOKS),
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
        fn round(ctx: &WorkerCtx, _: CarryAsk, _: &PressBehindTheLatch) {
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
        assert!(begin_carry_pass_with(
            &LATCH,
            &executor,
            voice,
            CarryAsk::ALL,
            round
        ));
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
            copilot_read_after_probe: false,
            codex_install_refused: false,
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
            ..CarryRound::default()
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
            ..CarryRound::default()
        });
        published.publish(CarryRound::default());
        published.publish(CarryRound {
            hooks: answer(vec![Text::AgentHooksRepairedCopilot]),
            ..CarryRound::default()
        });
        assert_eq!(
            published.take().and_then(|snapshot| snapshot.hooks),
            Some(hooks_part(vec![
                Text::AgentHooksRepairedClaude,
                Text::AgentHooksRepairedCopilot,
            ]))
        );
    }

    /// RED (0.4.10 plan row 68b, R2) — **a round runs the members it was asked for, and an ask
    /// while a round runs is the next round's**: an Agents-page ask runs the hooks alone; the
    /// Explorer writer's release and the sweep's, asked while it runs, are one more round of
    /// exactly those; a member not asked for is not run at all. The start asks for every member,
    /// and each trial release asks for its own.
    ///
    /// MUTATION: `round_begins` answers every member whatever was asked
    /// (`plant-68b-ask-runs-everything.py`).
    #[test]
    fn an_ask_runs_only_the_members_it_names() {
        use crate::test_support::{free_fn_body, squeezed};
        static LATCH: CarryLatch = CarryLatch::new();
        assert!(
            LATCH.ask_for_round(CarryAsk::HOOKS),
            "the ask starts the worker"
        );
        let first = LATCH.round_begins();
        assert!(first.names_member(CarryAsk::HOOKS));
        for member in [
            CarryAsk::EXPLORER_PACKAGE,
            CarryAsk::EXPLORER_CLASSIC,
            CarryAsk::UPDATE_ORPHANS,
        ] {
            assert!(
                !first.names_member(member),
                "an Agents-page ask asks for no other member: {first:?}"
            );
        }
        assert!(!LATCH.ask_for_round(CarryAsk::EXPLORER));
        assert!(!LATCH.ask_for_round(CarryAsk::UPDATE_ORPHANS));
        assert!(LATCH.round_again(), "the asks are one more round");
        let second = LATCH.round_begins();
        assert_eq!(
            second,
            CarryAsk::EXPLORER.joined_with(CarryAsk::UPDATE_ORPHANS)
        );
        assert!(
            !second.names_member(CarryAsk::HOOKS),
            "and not the agents' scan"
        );
        assert!(!LATCH.round_again(), "after which it ends");
        LATCH.carry_ended();

        let unasked = run_if_asked::<()>(
            CarryAsk::HOOKS,
            CarryAsk::EXPLORER_PACKAGE,
            "explorer-package",
            || panic!("a member the round did not ask for is run"),
        );
        assert_eq!(unasked, Ok(None));
        for member in [
            CarryAsk::HOOKS,
            CarryAsk::EXPLORER_PACKAGE,
            CarryAsk::EXPLORER_CLASSIC,
            CarryAsk::UPDATE_ORPHANS,
        ] {
            assert!(
                CarryAsk::ALL.names_member(member),
                "the start asks for {member:?}"
            );
        }
        assert!(
            squeezed(free_fn_body("begin_carry_at_start"))
                .contains(&squeezed("begin_carry_pass(CarryAsk::ALL)")),
            "the start's round asks for every member"
        );
        let release = squeezed(free_fn_body("release_trial_writer"));
        for arm in [
            "Writer::ExplorerRepair => carry_pass::begin_carry_pass(CarryAsk::EXPLORER)",
            "Writer::AgentHooksRepair => carry_pass::begin_carry_pass(CarryAsk::HOOKS)",
            "Writer::OrphanSweep => carry_pass::begin_carry_pass(CarryAsk::UPDATE_ORPHANS)",
        ] {
            assert!(
                release.contains(&squeezed(arm)),
                "a trial's release asks for its own members: {arm}"
            );
        }
    }

    /// RED (0.4.10 plan row 68b) — **every member's answer is the round's**: a round in which only
    /// the Explorer members and the sweep answered publishes, counts their checks, and hands the
    /// window the classic verb's answer; a later round with no classic answer keeps the untaken
    /// one, and a newer one replaces it.
    ///
    /// MUTATION: `publish` counts only the agents' part (`plant-68b-round-forgets-a-member.py`).
    #[test]
    fn a_round_answered_by_the_explorer_members_alone_is_published() {
        let published = CarryPublished::new();
        let classic = |installed| ClassicAnswer {
            installed,
            presses_seen: 0,
        };
        let line = published.publish(CarryRound {
            package: Some(MemberAnswer {
                part: (),
                checks: 1,
                changed: 1,
            }),
            classic: Some(MemberAnswer {
                part: classic(true),
                checks: 1,
                changed: 1,
            }),
            orphans: Some(MemberAnswer {
                part: (),
                checks: 1,
                changed: 0,
            }),
            ..CarryRound::default()
        });
        assert_eq!(line.as_deref(), Some("BT_CARRY round checks=3 changed=2"));
        published.publish(CarryRound {
            orphans: Some(MemberAnswer {
                part: (),
                checks: 1,
                changed: 0,
            }),
            ..CarryRound::default()
        });
        let taken = published.take().expect("published");
        assert_eq!(
            taken.explorer,
            Some(classic(true)),
            "the untaken answer kept"
        );
        assert_eq!(taken.hooks, None);
        published.publish(CarryRound {
            classic: Some(MemberAnswer {
                part: classic(true),
                checks: 1,
                changed: 0,
            }),
            ..CarryRound::default()
        });
        published.publish(CarryRound {
            classic: Some(MemberAnswer {
                part: classic(false),
                checks: 1,
                changed: 1,
            }),
            ..CarryRound::default()
        });
        assert_eq!(
            published.take().and_then(|snapshot| snapshot.explorer),
            Some(classic(false)),
            "a newer answer replaces an untaken one"
        );
    }
}
