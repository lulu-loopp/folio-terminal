//! **What both appliers share** (0.4.6 tickets U-28, U-23 and U-24;
//! `docs/plans/design/self-update-2026-09-16.md` §C.4, §C.5, revision (b)
//! §(b).2 and "Who may write what").
//!
//! The macOS applier (`update_apply_macos`, U-28/U-29/U-29b) and the Windows
//! one (`update_apply_windows`, U-23/U-24) take a handed-over transaction to
//! `Committed`, or back to the old build, by different effects — one exchange
//! of a bundle, or one move per file — but around those effects they are one
//! machine, and what of it both use lives here, lifted out of U-28's module as
//! `update_prepare` was lifted out of U-27's. **The Windows applier and
//! recovery are built on all of it; the macOS applier on the waits, the stop
//! of a trial, the rule for a live applier and what opens after** — its
//! `Limits` and `Ended` are still its own, wider since U-29 (U-24's report,
//! "for U-31"):
//!
//! * [`Limits`] — §C.4's 60 s for the old build, §C.5's 90 s for the trial,
//!   W9's 5 s grace for a trial asked to quit, and how often a waiting applier
//!   looks;
//! * [`Ended`] — where the Windows applier or recovery stopped, and the exit
//!   code it answers;
//! * [`Recording`] and [`Journaled`] — the journal as it stands durably and
//!   every phase this process wrote: a phase is recorded through
//!   `update_txn::Journal::advance` (the protocol's refusal), only when
//!   `update_txn::may_record` says the actor may, and written with
//!   [`write_journal`] — `install_txn::durable_write`, asked again for
//!   [`JOURNAL_WRITE_WITHIN`] while another program holds the journal open
//!   (U-34); an effect is asked of `update_txn::may` first;
//! * [`wait_for_the_claim`] — §C.4's authoritative test that the old build is
//!   gone: the data directory's claim, tried until had and let go at once;
//! * [`watch_trial`] — a trial waited for (W7, W8, M7, M8), on both platforms:
//!   the one the journal records (`Trial`, or the retrial over `Stuck`), or
//!   the one this holder just launched and has not yet found — named only by
//!   exact evidence, never because it started after the launch
//!   ([`launched_trial`], U-40); its receipt offered to the protocol,
//!   `Committed` recorded on the one it accepts; else the trial gone or its
//!   deadline passed (U-29b's watch, moved here by U-24, the coordinator's
//!   ruling 4);
//! * [`stop_trial`] — W9/M9's stop: asked to quit, 5 s of grace, then ended —
//!   each through `bt_platform::install_flip::ask`, which touches nothing that
//!   is not that very trial (pid, start instant and image);
//! * [`before_deciding`], [`survey`] and [`adopting`] — **what a lock holder
//!   does before `decide`** (0.4.7 ticket U-37, design revision (h) H.3): a
//!   running trial whose receipt names it exactly (pid and start instant) is
//!   recorded and its receipt commits; a handed-back trial that never became
//!   ready is ended; any other process of the new build, a held or unaskable
//!   claim, or a process list that cannot be read defers — never a second
//!   trial beside a candidate, never a rollback under it;
//! * [`OWNER_FILE`] — **the window's owner** (U-34): the one process that has
//!   taken the duty that a Folio window follows *Restart to update*, handed
//!   from O to P explicitly ([`take_the_window`]); the mark names a recorded
//!   live owner, the lock names a live unrecorded owner, and the journal says
//!   whether an owner whose lock is now gone already took its road; recovery
//!   that finds `Handoff` leaves it to a live process the mark names or whose
//!   election lock is still held ([`window_holder`]), writes nothing and opens
//!   nothing;
//! * [`ExitGuard`] — **the one way a road process leaves** (U-34): at its
//!   exit, whatever the reason, a successor it holds still running opens
//!   Folio, else it starts what the disk names ([`Opens`]); the recovery run
//!   at logon with nothing done is the one exception ([`Opener`]); since U-35
//!   (Windows), two OS launch refusals at `Moving` reserve one last trial
//!   durably (`TrialStarting`, [`reserve_last_trial`]) before asking the same
//!   installed image to start once more, and that trial can commit itself
//!   ([`commit_last_trial`]).
//!
//! Every wait here sleeps through the worker's wait door
//! (`bt_platform::wait::sleep_within`), on the `WorkerCtx` of the standalone
//! main the applier runs on.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::{self, Ask, Running};
use bt_platform::install_txn::{self, Hold};

use crate::cli;
use crate::update_txn::{
    Actor, Effect, Event, Home, Journal, Nonce, ParseRefusal, Phase, PhaseKind, Receipt, Role,
    Sight, TrialProcess, TxnId, receipt_sight, sight_of_read,
};

/// **How long the Windows applier and recovery wait, and how often they
/// look.**
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// For O's lock, O's claim and the exclusive admission together (§C.4's
    /// 60 s); and, for recovery and a rollback, for the transaction lock and
    /// for the exclusive admission before the moves back.
    pub(crate) old_within: Duration,
    /// For the trial's receipt, counted from the launch (§C.5's 90 s,
    /// `update_txn::TRIAL_DEADLINE_MS`).
    pub(crate) trial_within_ms: u64,
    /// Between two looks at the claim, at the admission, and at the trial.
    pub(crate) poll: Duration,
    /// **How long a trial asked to quit has before it is ended** (W9's "after
    /// 5 s grace").
    pub(crate) quit_within: Duration,
    /// How long an ended trial has to leave the process list before the
    /// rollback records that it would not end.
    pub(crate) end_within: Duration,
}

impl Limits {
    /// The product's.
    pub(crate) const PRODUCT: Self = Self {
        old_within: Duration::from_secs(60),
        trial_within_ms: crate::update_txn::TRIAL_DEADLINE_MS,
        poll: Duration::from_millis(250),
        quit_within: Duration::from_secs(5),
        end_within: Duration::from_secs(5),
    };
}

/// **A trial deadline no honest test run reaches**, on either platform's road, for a test whose
/// verdict is the receipt it writes, or the recorded process's end, and
/// not the deadline. Such a test writes the receipt from its own thread
/// while the applier watches, so a deadline within its reach would make
/// the verdict a measure of how busy the machine is. Ten minutes bounds a
/// broken run; a working one never waits for it, because the watch reads
/// the receipt before it reads the clock.
#[cfg(test)]
pub(crate) const TRIAL_NOT_UNDER_TEST_MS: u64 = 600_000;

/// **Where the Windows applier, or the Windows recovery, stopped.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// `Committed`, then the rollback material and the entrance removed.
    Committed,
    /// `Committed` is durable; a deletion after it failed and is the next
    /// actor's debt (W12, M11), never a rollback.
    CommittedWithDebt(String),
    /// `RolledBack`, then the entrance removed and `Retired{RolledBack}`: the
    /// old set is installed, verified by digest (W9, W11).
    RolledBack,
    /// `RolledBack` is durable; the entrance or the retirement failed and is
    /// the next lock holder's to finish (W11).
    RolledBackWithDebt(String),
    /// A step of the rollback failed: `Stuck` is durable with this error, and
    /// everything is kept (W10).
    Stuck(String),
    /// `Stuck` at its bound (`update_txn::STUCK_ATTEMPT_LIMIT`): nothing was
    /// tried; the last error.
    GaveUp(String),
    /// A running copy held the admission through the wait: no move back was
    /// tried, nothing was recorded, and the next lock holder tries.
    RollbackWaits(String),
    /// `Reverted` to `Prepared`: the entrance was found or removed, admission
    /// was refused, or the files were not the journal's.
    Reverted,
    /// `Abandoned`: O did not let go, the entrance could not be armed, or the
    /// staged set was no longer what was verified.
    Abandoned,
    /// O still held the transaction lock at the end of the wait: nothing was
    /// written.
    OldHeldTheLock,
    /// Another lock holder kept the transaction lock through the recovery's
    /// wait, or an applier that may still be alive has the handed-off
    /// transaction: nothing was written, and that holder opens Folio.
    LockHeld,
    /// The phase found is no step of this process's to take, and it was left
    /// as it is.
    Left(String),
    /// The line, the home or the journal is not an applier's to act on:
    /// nothing was touched.
    Refused(String),
    /// A write or a read failed; the journal holds its last durable phase.
    Failed(String),
    /// **Nothing was recorded, started or moved** (U-37, H.3): a process of the
    /// new build that the journal does not record runs, the data directory is
    /// held, or what runs or who holds it cannot be known.
    Deferred(Deferral),
    /// **The journal is one this build cannot read whole** (0.4.8 E1): this
    /// holder recorded, removed and ended nothing and let the lock go; the
    /// rescue build the journal names settles it.
    StoodAside(String),
}

impl Ended {
    /// **Whether this end deferred to a Folio proved to hold the data
    /// directory** (H.3): the exit guard then owes no start.
    pub(crate) fn deferred_to_a_holder(&self) -> bool {
        matches!(self, Ended::Deferred(Deferral::Held | Deferral::WindowDuty))
    }

    /// The process's exit code.
    pub(crate) fn code(&self) -> i32 {
        match self {
            Ended::Committed
            | Ended::CommittedWithDebt(_)
            | Ended::RolledBack
            | Ended::RolledBackWithDebt(_) => 0,
            Ended::Refused(_) => 2,
            _ => 1,
        }
    }

    /// **Whether this end leaves a rollback behind** — finished or not — so
    /// that the build started next carries `--update-failed` (U-29).
    pub(crate) fn rolled_back(&self) -> bool {
        matches!(
            self,
            Ended::RolledBack
                | Ended::RolledBackWithDebt(_)
                | Ended::Stuck(_)
                | Ended::GaveUp(_)
                | Ended::RollbackWaits(_)
        )
    }
}

/// **How long a journal write refused because another program has
/// `journal.json` open is asked again** (0.4.6 ticket U-34): a scanner, an
/// indexer, a backup or sync tool that opened it without delete sharing lets
/// go within moments; about 2 s in all, the first pause 10 ms and each next
/// one twice the last.
pub(crate) const JOURNAL_WRITE_WITHIN: Duration = Duration::from_secs(2);

fn retry_within<T, E>(
    until: Instant,
    mut operation: impl FnMut() -> Result<T, E>,
    is_retryable: impl Fn(&E) -> bool,
    wait: &mut impl FnMut(Duration) -> bool,
) -> Result<T, E> {
    let mut pause = Duration::from_millis(10);
    loop {
        match operation() {
            Ok(answer) => return Ok(answer),
            Err(failure) if is_retryable(&failure) => {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() || !wait(pause.min(left)) {
                    return Err(failure);
                }
                pause = pause.saturating_mul(2);
            }
            Err(failure) => return Err(failure),
        }
    }
}

/// **Write the journal's bytes durably** (`install_txn::durable_write`), and
/// while the rename is refused because another program holds `journal.json`
/// open (`install_txn::Failure::refused_while_open`: Windows only), ask again
/// with a growing pause through the wait door until [`JOURNAL_WRITE_WITHIN`]
/// has passed (U-34; the clean VM's rows W4 and W12 lost a Restart to one
/// such refusal). The door itself never waits: it has other callers and no
/// wait door, so the bound and the sleep are the applier's.
///
/// # Errors
/// The last failure, as a sentence; the journal keeps its last durable
/// bytes.
pub(crate) fn write_journal(worker: &WorkerCtx, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut wait = |pause| {
        bt_platform::wait::sleep_within(worker, pause);
        true
    };
    retry_within(
        Instant::now() + JOURNAL_WRITE_WITHIN,
        || install_txn::durable_write(path, bytes),
        install_txn::Failure::refused_while_open,
        &mut wait,
    )
    .map_err(|failure| failure.to_string())
}

/// **A holder of the journal under its lock**, as [`watch_trial`] needs it:
/// the journal as it stands durably, and the one road a phase is recorded by.
pub(crate) trait Recording {
    /// The journal as it stands durably.
    fn journal(&self) -> &Journal;
    /// **Record `event` as `actor`**: the next phase by the protocol, allowed
    /// to this actor, then durable.
    ///
    /// # Errors
    /// The protocol's refusal, the writer table's, or the write's failure; the
    /// journal stays at its last durable phase.
    fn record(&mut self, actor: Actor, event: &Event) -> Result<(), String>;
}

/// **The journal of one transaction under its lock**: as it stands durably,
/// and every phase this process wrote, in order — each written on the worker
/// the holder runs on, whose wait door a refused write sleeps through
/// ([`write_journal`]).
pub(crate) struct Journaled<'w> {
    /// `H\journal.json`.
    path: PathBuf,
    worker: &'w WorkerCtx,
    pub(crate) journal: Journal,
    pub(crate) written: Vec<PhaseKind>,
}

impl<'w> Journaled<'w> {
    /// The journal `journal`, as read from the home `home`, written from
    /// `worker`.
    pub(crate) fn of(home: &Home, worker: &'w WorkerCtx, journal: Journal) -> Self {
        Self {
            path: home.journal(),
            worker,
            journal,
            written: Vec::new(),
        }
    }

    pub(crate) fn phase(&self) -> PhaseKind {
        self.journal.body.phase.kind()
    }

    /// **Record `event` as `actor`**: the next phase by the protocol, allowed
    /// to this actor, then durable.
    ///
    /// # Errors
    /// The protocol's refusal, the writer table's, or the write's failure; the
    /// journal on disk and here stays at its last durable phase.
    pub(crate) fn record(&mut self, actor: Actor, event: &Event) -> Result<(), String> {
        if !event.kind().authors().contains(&actor) {
            return Err(format!("{actor:?} may not record {:?}", event.kind()));
        }
        let next = self
            .journal
            .advance(event)
            .map_err(|refusal| format!("{refusal:?}"))?;
        let phase = next.body.phase.kind();
        if !crate::update_txn::may_record(actor, phase) {
            return Err(format!("{actor:?} may not record {phase:?}"));
        }
        write_journal(self.worker, &self.path, &next.encode())?;
        self.journal = next;
        self.written.push(phase);
        Ok(())
    }

    /// Whether `actor` may do `effect` now.
    ///
    /// # Errors
    /// A sentence naming the actor, the effect and the phase.
    pub(crate) fn may(&self, actor: Actor, effect: Effect) -> Result<(), String> {
        if crate::update_txn::may(actor, effect, self.phase()) {
            Ok(())
        } else {
            Err(format!(
                "{actor:?} may not {effect:?} in {:?}",
                self.phase()
            ))
        }
    }
}

impl Recording for Journaled<'_> {
    fn journal(&self) -> &Journal {
        &self.journal
    }

    fn record(&mut self, actor: Actor, event: &Event) -> Result<(), String> {
        Journaled::record(self, actor, event)
    }
}

/// **§C.4's authoritative test that O is gone**: the data directory `data`'s
/// claim, tried until it is had and let go at once, sleeping `poll` between
/// tries through the wait door, until `until`. A live holder or a transient
/// sweep is asked about again; a claim that cannot be asked about is not
/// (`ClaimRefusal::QueryDenied`: no answer is coming).
///
/// # Errors
/// Why O is taken to have stayed, as a sentence.
pub(crate) fn wait_for_the_claim(
    worker: &WorkerCtx,
    data: &Path,
    poll: Duration,
    until: Instant,
) -> Result<(), String> {
    loop {
        match crate::persist::try_claim(data) {
            Ok(claim) => {
                drop(claim);
                return Ok(());
            }
            Err(
                bt_platform::instance::ClaimRefusal::Held
                | bt_platform::instance::ClaimRefusal::Sweeping,
            ) => {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err("still held at the end of the wait".to_owned());
                }
                bt_platform::wait::sleep_within(worker, poll.min(left));
            }
            Err(refusal) => return Err(format!("{refusal:?}")),
        }
    }
}

/// **Wait until nothing `held` names is held any more, within `window`**
/// (E-7 on Windows; the macOS process check since U-40): ask, and while
/// something is held sleep one `poll` (never past `window`) and ask again — so
/// a hold let go is seen at the next poll, and a hold that outlasts `window`
/// refuses with the names still held. A holder decides from what is still
/// held when its window ends, never from a sighting of something already
/// leaving — a person's start that hands itself to the recovery build is gone
/// within moments. `now` is the clock and `sleep` the pause (the worker's wait
/// door in the product), so a test can count the polls (U-42d, review
/// finding 5).
///
/// # Errors
/// What `held` refused with, or what is still held when `window` passed.
pub(crate) fn until_let_go(
    window: Instant,
    poll: Duration,
    now: &mut dyn FnMut() -> Instant,
    held: &mut dyn FnMut() -> Result<Vec<String>, String>,
    sleep: &mut dyn FnMut(Duration),
) -> Result<(), String> {
    loop {
        let names = held()?;
        if names.is_empty() {
            return Ok(());
        }
        let left = window.saturating_duration_since(now());
        if left.is_zero() {
            return Err(format!(
                "held open by another process: {}",
                names.join(", ")
            ));
        }
        sleep(poll.min(left));
    }
}

/// The receipt at `path`: `None` while there is none, else what it says.
pub(crate) fn read_receipt(path: &Path) -> Option<Result<Receipt, String>> {
    match file_reads::read(Lane::UpdateJournal, path) {
        Ok(bytes) => Some(receipt_sight(&bytes).known(Role::WindowsReceiptWatch)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => Some(Err(error.to_string())),
    }
}

/// Wall-clock milliseconds, as `Trial`'s `began_ms` is recorded (recovery in
/// another process measures the same deadline from it).
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// **What U-35's reservation found** ([`reserve_last_trial`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reserved {
    /// `TrialStarting` is durable: the frozen trial words' transaction and
    /// nonce.
    Trial(TxnId, Nonce),
    /// The journal is in another phase than `Moving`: not eligible.
    NotEligible,
    /// **The journal is one this build cannot read whole** (0.4.8 E1):
    /// nothing was recorded, and the lock is let go.
    StoodAside(String),
}

/// **Reserve U-35's one last trial before it is launched.** The caller has
/// already proved that the new image is live and that both the first start of
/// it and the rescue copy were refused by the operating system. This takes the
/// transaction lock once, records `TrialStarting` durably only over `Moving`
/// (so a failed trial can never come round here again; any other phase is
/// not eligible), and returns the frozen trial words' transaction and nonce.
/// A journal this build cannot read whole is stood aside from
/// ([`Role::LastTrialReserve`]).
///
/// # Errors
/// The lock or the journal could not be had, or the write failed.
pub(crate) fn reserve_last_trial(
    worker: &WorkerCtx,
    home: &Home,
    actor: Actor,
) -> Result<Reserved, String> {
    let _lock = match install_txn::try_hold(&home.lock(), Hold::Exclusive) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Err("the transaction lock is held".to_owned()),
        Err(failure) => return Err(failure.to_string()),
    };
    let journal = match sight_of_read(file_reads::read(Lane::UpdateJournal, home.journal())) {
        Some(Sight::Known(journal)) => journal,
        Some(beyond) => return Ok(Reserved::StoodAside(beyond.said(Role::LastTrialReserve))),
        None => return Err("the journal could not be read: there is none".to_owned()),
    };
    if journal.body.phase != Phase::Moving {
        return Ok(Reserved::NotEligible);
    }
    let nonce = crate::update_job::mint_nonce();
    let mut journaled = Journaled::of(home, worker, journal);
    journaled.record(
        actor,
        &Event::TrialPlanned {
            nonce,
            began_ms: now_ms(),
        },
    )?;
    Ok(Reserved::Trial(journaled.journal.txn, nonce))
}

/// **What U-35's reserved trial found when it asked to commit itself**
/// ([`commit_last_trial`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LastTrialCommit {
    /// Not yet: a holder has the transaction lock, or this trial's receipt is
    /// not on disk yet — nothing there, or an earlier attempt's that it is
    /// about to replace. The watch asks again at its next turn.
    Pending,
    /// `Committed` is durable — recorded here, or found already recorded.
    Committed,
    /// The journal no longer waits for this trial's own commit — a holder
    /// adopted it into `Trial`, declared a rollback, or the transaction is
    /// gone or another — so it never asks again; the watch's ordinary read
    /// of the header decides from here.
    NotItsOwn,
    /// **This process cannot read its own start instant** (the review's F3),
    /// so its receipt names nobody and can never commit it: said once, never
    /// asked again; the trial stays uncommitted, its writes held, and a
    /// recovery decides the transaction.
    Unprovable,
    /// **The journal is one this build cannot read whole** (0.4.8 E1):
    /// nothing was recorded and the lock is let go; never asked again — the
    /// trial stays uncommitted, its writes held, and the rescue build the
    /// journal names decides.
    StoodAside,
}

/// **U-35's reserved trial commits its own transaction** (U-35 round 2, the
/// review's B3) — the one commit road that needs no rescue-copy process,
/// because the operating system refused that copy and every holder runs it.
///
/// **The evidence is the ordinary one** (U-37, H.1/H.3): the receipt the
/// trial's storage worker wrote, create-new, once its claim was adopted and its
/// first pane text reached the glass. A holder commits a handed-back trial when
/// that receipt names the running process exactly (pid and start instant);
/// here the process it must name is the caller's own, and the receipt is read
/// back from disk, so what commits is what is durable. **What replaces "a
/// holder observed it"** is that this process is the reserved trial — admitted
/// by its exact nonce ([`crate::update_txn::Phase::reserved_trial`]) — and
/// asks only after its readiness edge.
///
/// **The lock.** Nobody holds `H\lock` while the trial becomes ready: the exit
/// guard that reserved it let go and left. This takes it, one non-blocking
/// attempt, for the read and the one write; a holder that has it (a recovery
/// that could run after all) makes this `Pending`, and what it records first
/// wins. The trial's shared `H\admission` keeps every rollback move off the
/// install while it runs.
///
/// # Errors
/// The journal or the receipt could not be read, or the protocol refused the
/// event (a receipt of another process); nothing was recorded.
pub(crate) fn commit_last_trial(
    worker: &WorkerCtx,
    home: &Home,
    txn: TxnId,
    nonce: Nonce,
) -> Result<LastTrialCommit, String> {
    let me = std::process::id();
    commit_last_trial_as(worker, home, txn, nonce, me, install_flip::started_of(me))
}

/// [`commit_last_trial`] as the process `pid`, started at `started` — the
/// caller's own in the product; a test names its own process too.
pub(crate) fn commit_last_trial_as(
    worker: &WorkerCtx,
    home: &Home,
    txn: TxnId,
    nonce: Nonce,
    pid: u32,
    started: Option<u64>,
) -> Result<LastTrialCommit, String> {
    commit_last_trial_reading(
        worker,
        home,
        (txn, nonce),
        (pid, started),
        || file_reads::read(Lane::UpdateJournal, home.journal()),
        |pause| {
            bt_platform::wait::sleep_within(worker, pause);
            true
        },
    )
}

/// [`commit_last_trial_as`] with the journal's bytes (`journal_bytes`, one
/// read of the file) and the pause between two reads handed in. **A read that fails other than "no such file" is
/// asked again** within [`JOURNAL_WRITE_WITHIN`] — a scanner or a sync tool
/// that holds `journal.json` lets go within moments, as the window election
/// asks again ([`PhaseRead::NotRead`], E1 round 3) — and only a journal that
/// still cannot be read is stood aside from. `wait` answers whether to ask
/// again.
fn commit_last_trial_reading(
    worker: &WorkerCtx,
    home: &Home,
    (txn, nonce): (TxnId, Nonce),
    (pid, started): (u32, Option<u64>),
    mut journal_bytes: impl FnMut() -> io::Result<Vec<u8>>,
    mut wait: impl FnMut(Duration) -> bool,
) -> Result<LastTrialCommit, String> {
    // Its own start instant is half of what its receipt must name (H.1): a
    // process that cannot read it wrote a receipt that names nobody, which
    // neither it nor any holder can ever accept.
    let Some(started) = started else {
        return Ok(LastTrialCommit::Unprovable);
    };
    let _lock = match install_txn::try_hold(&home.lock(), Hold::Exclusive) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Ok(LastTrialCommit::Pending),
        Err(failure) => return Err(failure.to_string()),
    };
    let read_once = || match sight_of_read(journal_bytes()) {
        Some(Sight::Unreadable(ParseRefusal::Unread(error))) => Err(error),
        seen => Ok(seen),
    };
    let journal = match retry_within(
        Instant::now() + JOURNAL_WRITE_WITHIN,
        read_once,
        |_| true,
        &mut wait,
    ) {
        Ok(Some(Sight::Known(journal))) => journal,
        Ok(None) => return Ok(LastTrialCommit::NotItsOwn),
        Ok(Some(Sight::Header { .. } | Sight::Envelope { .. } | Sight::Unreadable(_))) | Err(_) => {
            return Ok(LastTrialCommit::StoodAside);
        }
    };
    if journal.txn != txn {
        return Ok(LastTrialCommit::NotItsOwn);
    }
    if journal.body.phase == Phase::Committed {
        return Ok(LastTrialCommit::Committed);
    }
    if journal.body.phase.reserved_trial() != Some(nonce) {
        return Ok(LastTrialCommit::NotItsOwn);
    }
    let Some(receipt) = read_receipt(&home.receipt_path(txn, &nonce)) else {
        return Ok(LastTrialCommit::Pending);
    };
    let receipt = receipt?;
    let process = TrialProcess { pid, started };
    let names_it = receipt.pid == pid && receipt.started == Some(started);
    // An earlier attempt of this same trial, which ended before the
    // transaction was decided (U-35 round 3, F1): its receipt names a process
    // that no longer runs, and this trial's own receipt replaces it
    // (`update_trial::write_receipt`) — not yet, so ask again.
    if !names_it
        && receipt.txn == txn
        && receipt.nonce == nonce
        && !crate::update_trial::names_a_running_process(&receipt)
    {
        return Ok(LastTrialCommit::Pending);
    }
    let mut journaled = Journaled::of(home, worker, journal);
    journaled.record(Actor::Trial, &Event::LastTrialReady { receipt, process })?;
    Ok(LastTrialCommit::Committed)
}

/// **How a trial's watch ended.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Watched {
    /// A receipt the journal accepted: `Committed` is durable.
    Committed,
    /// The trial gone without a receipt, or its deadline passed; nothing more
    /// was recorded.
    NoReceipt,
    /// **The trial this holder launched runs, and its start could not be
    /// recorded** (`TrialBegan` or `RetrialBegan`, U-34): the journal does not
    /// know it. The caller ends it, or says why it is the window.
    Unrecorded { process: TrialProcess, why: String },
}

/// **What a trial's watch is over**: the home, who records, the trial this
/// holder just launched if the journal records none yet, and the clock.
pub(crate) struct Watch<'a> {
    pub(crate) home: &'a Home,
    /// The actor every phase of the watch is recorded as.
    pub(crate) actor: Actor,
    /// The nonce and `began_ms` of a trial this holder launched and has not
    /// recorded; `None` for the one the journal records.
    pub(crate) started: Option<(Nonce, u64)>,
    /// Between two looks.
    pub(crate) poll: Duration,
    /// The trial's deadline, counted from its `began_ms`.
    pub(crate) trial_within_ms: u64,
}

/// **The trial, waited for** (W7, W8, M7, M8): the one the journal records —
/// `Trial`, or the retrial a holder started over `Stuck` (U-29b) — or, while
/// the journal records none, the one this holder just launched (`started`:
/// its nonce and when), which `find` names by exact evidence only (given the
/// launch's nonce and the receipt at its name, if one is there already: on
/// macOS [`launched_trial`]) and which is then recorded (`TrialBegan` over
/// `Moving`, `RetrialBegan` over `Stuck`).
/// Then, polling through the wait door until the deadline counted from its
/// start: a receipt the journal accepts — this transaction's, this trial's
/// nonce — is recorded as `Committed` by the watch's
/// actor ([`Watched::Committed`]); any other is said once and waited past. The
/// recorded process no longer `alive` (by its pid, its start time and its
/// image), or the deadline passed → [`Watched::NoReceipt`]. The launched trial
/// found but not recorded → [`Watched::Unrecorded`]. **While the launched
/// trial has not been seen, `launch_over` says whether the launch itself has
/// ended** (U-38): on macOS the `open -W` that started it, which returns when
/// the application it opened ends — a launch over with nothing seen is a trial
/// that ended before it could be seen, [`Watched::NoReceipt`] at once rather
/// than at the deadline; on Windows the trial's pid is the child's own and is
/// recorded at its launch, so it is never unseen.
///
/// # Errors
/// The journal records no trial and none was started, or a write after the
/// trial's record failed; nothing more was recorded.
pub(crate) fn watch_trial(
    worker: &WorkerCtx,
    txn: &mut impl Recording,
    watch: &Watch<'_>,
    find: &mut dyn FnMut(Nonce, Option<&Receipt>) -> Option<TrialProcess>,
    launch_over: &mut dyn FnMut() -> bool,
    alive: &mut dyn FnMut(TrialProcess) -> bool,
    say: &mut dyn FnMut(&str),
) -> Result<Watched, String> {
    let (actor, started) = (watch.actor, watch.started);
    let mut said_refusal = false;
    loop {
        let (nonce, began_ms, recorded) = match (&txn.journal().body.phase, started) {
            (
                Phase::Trial {
                    nonce,
                    process,
                    began_ms,
                },
                _,
            ) => (*nonce, *began_ms, Some(*process)),
            (
                Phase::Stuck {
                    retrial: Some(retrial),
                    trial,
                    ..
                },
                _,
            ) if started.is_none_or(|(nonce, _)| nonce == retrial.nonce) => {
                (retrial.nonce, retrial.began_ms, *trial)
            }
            (_, Some((nonce, began_ms))) => (nonce, began_ms, None),
            (other, None) => {
                return Err(format!("{:?} has no trial to wait for", other.kind()));
            }
        };
        let deadline = began_ms.saturating_add(watch.trial_within_ms);
        let receipt_path = watch.home.receipt_path(txn.journal().txn, &nonce);
        let receipt = read_receipt(&receipt_path);
        match recorded {
            None => {
                // Asked before the look, so a launch that ended after the look
                // is seen at the next turn, never taken for one that ended
                // before its trial could be seen (U-38).
                let over = launch_over();
                let found = receipt.as_ref().and_then(|read| read.as_ref().ok());
                if let Some(process) = find(nonce, found) {
                    let event = if txn.journal().body.phase.kind() == PhaseKind::Stuck {
                        Event::RetrialBegan {
                            nonce,
                            process,
                            began_ms,
                        }
                    } else {
                        Event::TrialBegan {
                            nonce,
                            process,
                            began_ms,
                        }
                    };
                    if let Err(why) = txn.record(actor, &event) {
                        return Ok(Watched::Unrecorded { process, why });
                    }
                    continue;
                }
                if over {
                    // **The launch is over and its trial was never seen**
                    // (U-38): it ended before the list showed it, or never
                    // started — nothing is left to wait for.
                    say("BT_UPDATE_APPLY the trial ended before it could be seen");
                    return Ok(Watched::NoReceipt);
                }
            }
            Some(process) => {
                match receipt {
                    Some(Ok(receipt)) => {
                        let event = Event::ReceiptAccepted(receipt);
                        match txn.journal().advance(&event) {
                            Ok(_) => {
                                txn.record(actor, &event)?;
                                return Ok(Watched::Committed);
                            }
                            Err(refusal) if !said_refusal => {
                                said_refusal = true;
                                say(&format!(
                                    "BT_UPDATE_APPLY the receipt at {} is refused: {refusal:?}",
                                    receipt_path.display()
                                ));
                            }
                            Err(_) => {}
                        }
                    }
                    Some(Err(error)) if !said_refusal => {
                        said_refusal = true;
                        say(&format!("BT_UPDATE_APPLY the receipt: {error}"));
                    }
                    _ => {}
                }
                if !alive(process) {
                    say(&format!(
                        "BT_UPDATE_APPLY the trial {} ended without a receipt",
                        process.pid
                    ));
                    return Ok(Watched::NoReceipt);
                }
            }
        }
        let now = now_ms();
        if now >= deadline {
            say("BT_UPDATE_APPLY no receipt by the trial's deadline");
            return Ok(Watched::NoReceipt);
        }
        bt_platform::wait::sleep_within(
            worker,
            watch.poll.min(Duration::from_millis(deadline - now)),
        );
    }
}

/// **Whether the recorded trial still runs as the new build**: its pid, with
/// its start instant, in the process list of the new build's executable
/// (`images`: on macOS the launch path or `stage/`, whichever holds the new
/// identity; on Windows `<install>\folio.exe`). A list that cannot be read
/// says it does not (the rollback then goes on, and nothing is asked:
/// [`install_flip::ask`] reads the list itself).
pub(crate) fn trial_runs(process: TrialProcess, images: &[&Path]) -> bool {
    install_flip::runs_from(
        Running {
            pid: process.pid,
            started: process.started,
        },
        images,
    )
    .unwrap_or(false)
}

/// **The trial this holder launched, by exact evidence only** (0.4.7 ticket
/// U-40): the process its launch's own receipt names — the receipt at the
/// nonce's name, of this transaction and nonce, by its pid and the start
/// instant it carries (none carried names no running process; the receipt
/// still answers by its nonce) — else the process of the new build's
/// executable `program` that carries this trial's own words,
/// `--update-trial <txn> <nonce>` ([`trial_words`]), in its arguments
/// (`install_flip::arguments_of`, by pid and start instant). The nonce is the
/// launch's own, so no other process carries it. **A process is never taken
/// for the trial because it started after the launch**: a person's start of
/// the new build in that instant hands itself to the recovery build and
/// leaves at once, and a holder that recorded it saw its trial end without a
/// receipt and rolled back a healthy one (the macOS rehearsal's defect 9, in
/// the instant between the launch and the trial's record). `None` until one
/// of the two is there. The macOS holders' `find`; the Windows trial's pid is
/// its launch's own.
pub(crate) fn launched_trial(
    txn: TxnId,
    nonce: Nonce,
    program: &Path,
    receipt: Option<&Receipt>,
) -> Option<TrialProcess> {
    if let Some(receipt) = receipt.filter(|receipt| receipt.txn == txn && receipt.nonce == nonce) {
        return Some(TrialProcess {
            pid: receipt.pid,
            started: receipt.started.unwrap_or(0),
        });
    }
    let words = trial_words(txn, &nonce);
    install_flip::running_from(program)
        .ok()?
        .into_iter()
        .find(|process| {
            install_flip::arguments_of(*process).is_ok_and(|arguments| {
                arguments
                    .windows(words.len())
                    .any(|carried| carried == words)
            })
        })
        .map(|process| TrialProcess {
            pid: process.pid,
            started: process.started,
        })
}

/// **What a lock holder finds of the processes of the new build that the
/// journal does not record** (0.4.7 ticket U-37, design revision (h) H.1 and
/// H.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Survey {
    /// A process runs whose receipt in `H\<txn>` names it exactly — its pid
    /// **and** its start instant (the receipt's `started`, H.1 R3). It is
    /// recorded with the receipt's own identity.
    Adoptable {
        process: TrialProcess,
        receipt: Receipt,
    },
    /// Processes of the new build run, and no receipt names one of them
    /// exactly: each is a trial nobody records, or a start about to become
    /// one — whether or not it has taken the data directory yet (H.3 step 3).
    Candidates(Vec<Running>),
    /// No process of the new build runs, besides those excluded.
    Nothing,
    /// The process list, or `H\<txn>`, could not be read: whether a candidate
    /// runs is not known (H.3's "unlistable").
    Unlistable(String),
}

/// **Survey the processes of the new build's executable `program`** that the
/// journal of `txn` does not record, leaving out `excluded` — each by its
/// exact pid and start instant (the recorded trial of a `Stuck`, the one a
/// holder has just ended, this recovery's own starter). H.1 R3 and R4: a
/// receipt counts only when it is this transaction's, sits at its own nonce's
/// name, carries `started`, and a running process has exactly
/// `(receipt.pid, receipt.started)`. A stale receipt whose pid another process
/// now has, a receipt without `started`, and a receipt of another transaction
/// name nobody; several receipts are each asked, and at most one can name a
/// running process exactly.
///
/// The receipts are found by listing `H\<txn>` (`files::read_directory`, the
/// product's one listing): the trial's nonce is its own, and its receipt is the
/// one place it says it.
pub(crate) fn survey(home: &Home, txn: TxnId, program: &Path, excluded: &[Running]) -> Survey {
    let running: Vec<Running> = match install_flip::running_from(program) {
        Ok(list) => list
            .into_iter()
            .filter(|process| !excluded.contains(process))
            .collect(),
        Err(error) => return Survey::Unlistable(format!("the process list: {error}")),
    };
    if running.is_empty() {
        return Survey::Nothing;
    }
    let folder = home.transaction(txn);
    let listing = match crate::files::read_directory(&folder) {
        crate::files::DirOutcome::Listed(listing) => listing,
        crate::files::DirOutcome::Failed(fault) => {
            return Survey::Unlistable(format!("{}: {fault:?}", folder.display()));
        }
    };
    let adoptable = listing
        .entries
        .iter()
        .filter(|entry| {
            !entry.is_dir
                && entry
                    .name
                    .starts_with(crate::update_txn::RECEIPT_FILE_PREFIX)
        })
        .find_map(|entry| {
            let path = folder.join(&entry.name);
            let receipt = read_receipt(&path)?.ok()?;
            // This transaction's, at its own nonce's name, naming its own
            // start instant (H.1 R3).
            if receipt.txn != txn || path != home.receipt_path(txn, &receipt.nonce) {
                return None;
            }
            let exactly = Running {
                pid: receipt.pid,
                started: receipt.started?,
            };
            running.contains(&exactly).then_some((
                TrialProcess {
                    pid: exactly.pid,
                    started: exactly.started,
                },
                receipt,
            ))
        });
    match adoptable {
        Some((process, receipt)) => Survey::Adoptable { process, receipt },
        None => Survey::Candidates(running),
    }
}

/// **The record of a running trial the journal did not know** (U-37): over
/// `Moving`, `TrialBegan`; over `Stuck`, `RetrialBegan` — with the nonce its
/// own receipt carries, so that receipt then answers for it, the process its
/// receipt names exactly (H.1 R3), and `began_ms` now (the record's instant;
/// its receipt is already here).
pub(crate) fn adopting(over_stuck: bool, receipt: &Receipt, process: TrialProcess) -> Event {
    let (nonce, began_ms) = (receipt.nonce, now_ms());
    if over_stuck {
        Event::RetrialBegan {
            nonce,
            process,
            began_ms,
        }
    } else {
        Event::TrialBegan {
            nonce,
            process,
            began_ms,
        }
    }
}

/// **Why a lock holder deferred** instead of deciding (H.3 step 3): the
/// transaction is left as it stands — nothing recorded, started or moved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Deferral {
    /// A process of the new build runs that no receipt names (by its pid and
    /// start instant): it is the window while it runs.
    Candidate(Running),
    /// The data directory's claim is held: a Folio runs on this data, and its
    /// holder is the acknowledged window (U-34's delivery test).
    Held,
    /// Another process holds the update window's election lock without a
    /// readable mark. Its exit guard, not a data-directory holder, opens the
    /// window.
    WindowDuty,
    /// Whether a candidate runs cannot be known.
    Unlistable(String),
    /// The data directory's claim could not be asked about
    /// (`ClaimRefusal::QueryDenied`): no evidence either way.
    Denied(String),
}

impl Deferral {
    /// One line of it.
    pub(crate) fn said(&self) -> String {
        match self {
            Deferral::Candidate(process) => format!(
                "{} runs the new build and no receipt names it; nothing is started beside it",
                process.pid
            ),
            Deferral::Held => {
                String::from("a Folio holds the data directory; nothing is started beside it")
            }
            Deferral::WindowDuty => String::from(
                "another process holds the update window duty; nothing is started beside it",
            ),
            Deferral::Unlistable(why) => format!("what runs cannot be read ({why})"),
            Deferral::Denied(why) => format!("the data directory cannot be asked about ({why})"),
        }
    }
}

pub(crate) use crate::cli::HandedBack;

/// **What a lock holder does before `decide`** over `Moving`, or over a `Stuck`
/// whose recorded trial no longer runs, with the new build live (H.3).
#[derive(Debug)]
pub(crate) enum BeforeDeciding {
    /// Step 1: record this event (the adopted trial); it is the successor.
    Adopt { event: Event, successor: Running },
    /// Step 3: leave the transaction as it stands (`Ended::Deferred`).
    Defer(Deferral),
    /// Step 4: no candidate and the claim free — `decide` as before.
    Decide,
}

/// **What [`before_deciding`] looks at**: the home and transaction, the new
/// build's executable, whether the phase is a `Stuck`, the exact instances
/// that are no candidate (the recorded trial of a `Stuck`, this recovery's own
/// starter), the trial `--from-trial` named, and the data directory whose claim
/// is asked.
#[derive(Clone, Copy)]
pub(crate) struct Before<'a> {
    pub(crate) home: &'a Home,
    pub(crate) txn: TxnId,
    pub(crate) program: &'a Path,
    pub(crate) over_stuck: bool,
    pub(crate) excluded: &'a [Running],
    pub(crate) handed_back: Option<HandedBack>,
    pub(crate) data: &'a Path,
}

/// **H.3's order**, both platforms: (1) adopt a running trial its receipt names
/// exactly; (2) end the handed-back trial named by `--from-trial` when it is
/// unready (`end`, W9's stop of that exact instance), then look again; (3) fail
/// closed — a candidate seen, the claim held, what runs unreadable, or the
/// claim question denied each defer; (4) otherwise decide. `excluded` are the
/// exact instances that are no candidate (the recorded trial of a `Stuck`,
/// this recovery's own starter); `data` is the data directory whose claim is
/// asked.
pub(crate) fn before_deciding(
    what: &Before<'_>,
    end: &mut dyn FnMut(TrialProcess) -> Result<(), String>,
) -> BeforeDeciding {
    let Before {
        home,
        txn,
        program,
        over_stuck,
        excluded,
        handed_back,
        data,
    } = *what;
    // The handed-back trial is never excluded, even when it is this recovery's
    // own starter (the watchdog's hand-back): it is ended here, or it is a
    // candidate (Codex's check of (h), round 3).
    let mut excluded: Vec<Running> = excluded
        .iter()
        .copied()
        .filter(|process| handed_back.is_none_or(|handed| handed.process != *process))
        .collect();
    let mut found = survey(home, txn, program, &excluded);
    if let (Some(handed), Survey::Candidates(running)) = (handed_back, &found)
        && !handed.ready
        && running.contains(&handed.process)
    {
        let process = TrialProcess {
            pid: handed.process.pid,
            started: handed.process.started,
        };
        if let Err(why) = end(process) {
            return BeforeDeciding::Defer(Deferral::Unlistable(format!(
                "the handed-back trial {}: {why}",
                process.pid
            )));
        }
        excluded.push(handed.process);
        found = survey(home, txn, program, &excluded);
    }
    let unlistable = match found {
        Survey::Adoptable { process, receipt } => {
            return BeforeDeciding::Adopt {
                event: adopting(over_stuck, &receipt, process),
                successor: Running {
                    pid: process.pid,
                    started: process.started,
                },
            };
        }
        Survey::Candidates(running) => {
            return BeforeDeciding::Defer(Deferral::Candidate(running[0]));
        }
        Survey::Unlistable(why) => Some(why),
        Survey::Nothing => None,
    };
    let claim = match crate::persist::try_claim(data) {
        Ok(claim) => {
            drop(claim);
            None
        }
        Err(bt_platform::instance::ClaimRefusal::Held) => {
            return BeforeDeciding::Defer(Deferral::Held);
        }
        Err(refusal) => Some(format!("{refusal:?}")),
    };
    match (unlistable, claim) {
        (Some(why), _) => BeforeDeciding::Defer(Deferral::Unlistable(why)),
        (None, Some(why)) => BeforeDeciding::Defer(Deferral::Denied(why)),
        (None, None) => BeforeDeciding::Decide,
    }
}

/// **Stop the trial** (W9/M9; the coordinator's ruling 2 of U-29 and ruling 1
/// of U-24): ask it to quit, wait up to `quit_within` for it to leave the list,
/// then end it and wait up to `end_within`, polling every `poll` through the
/// wait door. Each ask goes through [`install_flip::ask`], which sends nothing
/// to a process that is not that very trial running from one of `images`.
///
/// # Errors
/// The process list could not be read, the trial could not be asked, or it
/// did not end.
pub(crate) fn stop_trial(
    worker: &WorkerCtx,
    process: TrialProcess,
    images: &[&Path],
    (quit_within, end_within, poll): (Duration, Duration, Duration),
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let running = Running {
        pid: process.pid,
        started: process.started,
    };
    for (ask, within) in [(Ask::Quit, quit_within), (Ask::End, end_within)] {
        match install_flip::ask(running, images, ask) {
            Ok(true) => say(&format!(
                "BT_UPDATE_ROLLBACK the trial {} is asked to {}",
                process.pid,
                match ask {
                    Ask::Quit => "quit",
                    Ask::End => "end",
                }
            )),
            Ok(false) => return Ok(()),
            Err(error) => return Err(format!("the trial {}: {error}", process.pid)),
        }
        let until = Instant::now() + within;
        loop {
            if !trial_runs(process, images) {
                return Ok(());
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            bt_platform::wait::sleep_within(worker, poll.min(left));
        }
    }
    Err(format!("the trial {} did not end", process.pid))
}

/// **The window's owner** (U-34, round 2): the file `H\<txn>\owner`, holding
/// `<pid>:<creation>` of the one process that has taken the duty that a Folio
/// window follows *Restart to update*. It is the explicit hand-over of that
/// duty between the outgoing build O and its applier P: pid liveness decides
/// nothing between two live processes that could each defer to the other.
///
/// * O clears it as it hands the transaction over (before `Handoff`, while O
///   holds the transaction lock), so no mark of an earlier attempt stands;
/// * P takes it as soon as it knows its transaction, before it waits for O's
///   lock ([`take_the_window`]) — and only a P that took it runs its road and
///   opens a window; one that finds it taken leaves everything alone;
/// * O takes it at its very end, after letting go of its claim — and starts
///   Folio only if it took it;
/// * the recovery build R never takes it: at `Handoff` it leaves the
///   transaction to a live process the mark names, and ignores any other
///   process (a P that never took the mark is not an applier to wait for).
///
/// Taking is one election under an exclusive operating-system lock,
/// `H\<txn>\owner.lock` ([`take_the_window`]): read the mark, and write this
/// process into it only when it is absent or its process no longer runs (pid
/// and start instant) — its owner died. The lock goes with a holder that dies
/// inside the election.
pub(crate) const OWNER_FILE: &str = "owner";

/// `H\<txn>\owner`.
pub(crate) fn owner_path(home: &Home, txn: TxnId) -> PathBuf {
    home.transaction(txn).join(OWNER_FILE)
}

/// This process, by its pid and start instant (`started` 0 when it cannot be
/// read: nothing then matches it).
pub(crate) fn this_process() -> Running {
    let pid = std::process::id();
    Running {
        pid,
        started: install_flip::started_of(pid).unwrap_or(0),
    }
}

/// `<pid>:<creation>`, both in decimal.
fn owner_value(owner: Running) -> String {
    format!("{}:{}", owner.pid, owner.started)
}

/// The process a mark's bytes name, or `None` for bytes that are not exactly
/// `<pid>:<creation>`.
fn owner_named(bytes: &[u8]) -> Option<Running> {
    let (pid, started) = std::str::from_utf8(bytes).ok()?.trim().split_once(':')?;
    Some(Running {
        pid: pid.parse().ok()?,
        started: started.parse().ok()?,
    })
}

/// **The process the window's mark names**, or `None` when there is none, or
/// it cannot be read.
pub(crate) fn window_owner(home: &Home, txn: TxnId) -> Option<Running> {
    read_window_owner(home, txn).ok().flatten()
}

/// The mark's parsed owner. A missing or malformed mark has no owner; another
/// read failure is kept distinct because it is not evidence that the mark is
/// stale.
fn read_window_owner(home: &Home, txn: TxnId) -> io::Result<Option<Running>> {
    match file_reads::read(Lane::UpdateJournal, owner_path(home, txn)) {
        Ok(bytes) => Ok(owner_named(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Why the election's read of the phase gave no phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PhaseRead {
    /// There is no journal, or it is another transaction's: asked again
    /// within the election's wait. (A read that failed is
    /// [`PhaseRead::NotRead`].)
    Unread(String),
    /// **The journal is one this build cannot read whole** (0.4.8 E1): the
    /// applier stands aside ([`Role::WindowElection`]) and writes no mark.
    StoodAside(String),
    /// **The journal's file could not be read** (E1 round 2): asked again
    /// within the election's wait — a scanner lets go within moments — and
    /// stood aside from as [`PhaseRead::StoodAside`] when it never reads.
    NotRead(String),
}

/// The durable transaction phase that says whether an unmarked election is
/// still open. The transaction identity is checked with the phase: a journal
/// for another transaction cannot authorize this contender.
pub(crate) fn read_window_phase(home: &Home, txn: TxnId) -> Result<PhaseKind, PhaseRead> {
    let journal = match sight_of_read(file_reads::read(Lane::UpdateJournal, home.journal())) {
        Some(Sight::Known(journal)) => journal,
        Some(unread @ Sight::Unreadable(ParseRefusal::Unread(_))) => {
            return Err(PhaseRead::NotRead(unread.said(Role::WindowElection)));
        }
        Some(beyond) => return Err(PhaseRead::StoodAside(beyond.said(Role::WindowElection))),
        None => return Err(PhaseRead::Unread("there is no journal".to_owned())),
    };
    if journal.txn != txn {
        return Err(PhaseRead::Unread(format!(
            "the journal is transaction {}, not {txn}",
            journal.txn
        )));
    }
    Ok(journal.body.phase.kind())
}

/// Before and through `Handoff`, no applier road has been taken. Every later
/// or terminal phase is the durable evidence that one has.
pub(crate) const fn window_duty_is_open(phase: PhaseKind) -> bool {
    matches!(
        phase,
        PhaseKind::Allocated | PhaseKind::Prepared | PhaseKind::Handoff
    )
}

/// **Whether an applier's road ever held the transaction lock `H\lock`** —
/// what [`ExitGuard::road_ended`] decides by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransactionLock {
    /// The road stopped before it had the lock: O may still hold it.
    NeverHeld,
    /// The road had the lock, so O's process had already ended.
    Held,
}

/// A successful election. When the owner mark could not be replaced before
/// its rename, `held` is the election lock itself: the exit guard owns it
/// through the road and its final delivery, so another live contender can
/// never become a second owner. Once that guard is gone, the journal records
/// that the road was taken.
pub(crate) struct WindowDuty {
    _held: Option<install_txn::Held>,
    warning: Option<String>,
}

impl WindowDuty {
    fn recorded(held: install_txn::Held, warning: Option<String>) -> Self {
        drop(held);
        Self {
            _held: None,
            warning,
        }
    }

    fn held(held: install_txn::Held, warning: String) -> Self {
        Self {
            _held: Some(held),
            warning: Some(warning),
        }
    }

    /// The non-fatal mark failure that made the lock, rather than durable
    /// bytes, the record of this process's duty.
    pub(crate) fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// Whether the mark did not land and the real election lock is this
    /// duty's live record.
    pub(crate) const fn is_lock_backed(&self) -> bool {
        self._held.is_some()
    }
}

impl std::fmt::Debug for WindowDuty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.warning {
            Some(warning) => f.debug_tuple("Mine").field(warning).finish(),
            None => f.write_str("Mine"),
        }
    }
}

/// Why a contender could not enter the election.
pub(crate) struct WindowRefusal {
    why: String,
    contended: bool,
    retryable: bool,
    outgoing_keeps_duty: bool,
}

impl WindowRefusal {
    /// The refusal's diagnostic sentence.
    pub(crate) fn why(&self) -> &str {
        &self.why
    }

    /// Whether another process currently holds the election and therefore
    /// carries the duty.
    #[cfg(test)]
    pub(crate) const fn contended(&self) -> bool {
        self.contended
    }

    /// Whether an applier should ask the same election again inside its one
    /// existing deadline.
    const fn retryable(&self) -> bool {
        self.retryable
    }

    /// Whether O remains the only candidate after this refusal. This is true
    /// only when the election lock could not be opened; contention or an
    /// unreadable mark is evidence that the duty may already be elsewhere.
    pub(crate) const fn outgoing_keeps_duty(&self) -> bool {
        self.outgoing_keeps_duty
    }
}

impl std::fmt::Debug for WindowRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Refused").field(&self.why).finish()
    }
}

/// **Who has the duty a window follows**, as [`take_the_window`] found it.
#[derive(Debug)]
pub(crate) enum Window {
    /// This process: it opens a window when it leaves.
    Mine(WindowDuty),
    /// This live process has it: it opens the window.
    Theirs(Running),
    /// The mark and lock are now gone, but the journal records that an
    /// applier already took or finished the road. Its exit guard opened the
    /// window, so this later contender starts nothing.
    RoadTaken(PhaseKind),
    /// This process proved no duty. Contention or an unreadable mark leaves
    /// it elsewhere; a lock-open failure is left to O, which armed the duty.
    Refused(WindowRefusal),
    /// **An applier found a journal this build cannot read whole** (0.4.8
    /// E1): it wrote no mark and stands aside ([`Ended::StoodAside`]).
    StoodAside(String),
}

impl Window {
    /// Whether this process won the duty.
    #[cfg(test)]
    pub(crate) const fn is_mine(&self) -> bool {
        matches!(self, Self::Mine(_))
    }
}

impl PartialEq for Window {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Mine(_), Self::Mine(_)) => true,
            (Self::Theirs(left), Self::Theirs(right)) => left == right,
            (Self::RoadTaken(left), Self::RoadTaken(right)) => left == right,
            (Self::StoodAside(left), Self::StoodAside(right)) => left == right,
            (Self::Refused(left), Self::Refused(right)) => {
                left.why == right.why
                    && left.contended == right.contended
                    && left.retryable == right.retryable
                    && left.outgoing_keeps_duty == right.outgoing_keeps_duty
            }
            _ => false,
        }
    }
}

impl Eq for Window {}

/// `H\<txn>\owner.lock`: the election's lock (round 5).
pub(crate) fn owner_lock_path(home: &Home, txn: TxnId) -> PathBuf {
    home.transaction(txn).join(format!("{OWNER_FILE}.lock"))
}

/// **How long a contender waits for the election's lock** (round 5): its
/// holder normally keeps it for the few milliseconds of one read and one
/// durable write. A contender that cannot take it within this bound refuses
/// the duty: the holder may merely be delayed inside the election.
pub(crate) const ELECTION_WITHIN: Duration = Duration::from_secs(5);

/// The election-lock part of the applier's one wait for O. Supplying `now`
/// keeps the bound itself deterministic in tests.
pub(crate) fn election_within(until: Instant, now: Instant) -> Duration {
    ELECTION_WITHIN.min(until.saturating_duration_since(now))
}

/// **Take the duty a window follows, unless a live process already has it**
/// ([`OWNER_FILE`]; round 5, Codex's finding 14): one exclusive operating-
/// system lock, `H\<txn>\owner.lock` (`install_txn::hold_within`: `LockFileEx`
/// on Windows, `flock` on Unix), is held around the whole decision. **The one
/// rule is: a readable live mark owns, a held lock owns while it lives, and
/// without either an applier may claim only while the journal still says the
/// applier road has not been taken; the outgoing build, which holds the
/// transaction lock while it asks, may claim without either
/// ([`Contender`]).** A mark read or replacement refused by a scanner is
/// re-asked with [`write_journal`]'s [`JOURNAL_WRITE_WITHIN`] discipline. After
/// a successful rename the visible mark records the duty; before the rename
/// the exit guard retains the lock through the road and its final delivery
/// ([`ExitGuard::road_ended`] says when that duty goes back to O). Once that
/// guard is gone, a phase past `Handoff` is the durable record that the road
/// already ran. A holder that dies before acting releases its lock while the
/// journal remains `Handoff`, so the next contender can take the duty.
///
/// **By `until`** (round 8, Codex's finding 15): the wait for the lock is the
/// smaller of [`ELECTION_WITHIN`] and what is left before `until` — the
/// applier's road deadline — so an election begun near that deadline never
/// carries the applier's wait for O past it.
pub(crate) fn take_the_window(
    worker: Option<&WorkerCtx>,
    home: &Home,
    txn: TxnId,
    me: Running,
    until: Instant,
) -> Window {
    take_the_window_within(
        worker,
        home,
        txn,
        me,
        election_within(until, Instant::now()),
        Contender::Applier,
    )
}

/// [`take_the_window`], as `contender`, waiting up to `within` for the
/// election's lock — zero in O's panic road, which waits for nothing.
pub(crate) fn take_the_window_within(
    worker: Option<&WorkerCtx>,
    home: &Home,
    txn: TxnId,
    me: Running,
    within: Duration,
    contender: Contender,
) -> Window {
    take_the_window_within_using(
        home,
        txn,
        me,
        within,
        contender,
        ElectionOps {
            after_lock: || {},
            read_owner: || read_window_owner(home, txn),
            read_phase: || read_window_phase(home, txn),
            write: |path: &Path, bytes: &[u8]| {
                MarkWriteFailure::from_result(install_txn::durable_write(path, bytes))
            },
            wait: |pause| match worker {
                Some(worker) => {
                    bt_platform::wait::sleep_within(worker, pause);
                    true
                }
                None => false,
            },
        },
    )
}

pub(crate) struct MarkWriteFailure {
    why: String,
    refused_while_open: bool,
    after_rename: bool,
}

impl MarkWriteFailure {
    fn from_result(result: Result<(), install_txn::Failure>) -> Result<(), Self> {
        result.map_err(|failure| Self {
            refused_while_open: failure.refused_while_open(),
            after_rename: matches!(
                failure.stage,
                install_txn::Stage::OpenDirectory | install_txn::Stage::FlushDirectory
            ),
            why: failure.to_string(),
        })
    }

    #[cfg(test)]
    pub(crate) fn before_rename_for_test(why: &str, refused_while_open: bool) -> Self {
        Self {
            why: why.to_owned(),
            refused_while_open,
            after_rename: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn after_rename_for_test(why: &str) -> Self {
        Self {
            why: why.to_owned(),
            refused_while_open: false,
            after_rename: true,
        }
    }
}

struct ElectionOps<AfterLock, ReadOwner, ReadPhase, Write, Wait> {
    after_lock: AfterLock,
    read_owner: ReadOwner,
    read_phase: ReadPhase,
    write: Write,
    wait: Wait,
}

/// **Who asks for the window duty.** Only an applier reads the journal in an
/// unmarked election.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Contender {
    /// The outgoing build O. It holds the transaction lock `H\lock` until its
    /// process ends (`update_handoff::Staged::lock`, kept by `update_job` until
    /// the process leaves), and every applier road takes that lock before it
    /// writes the journal (`update_apply_windows::apply_under_the_lock_with`,
    /// `update_apply_macos::Txn::hold`). So while O asks, no applier road can
    /// have moved the journal past `Handoff`: a later phase there is O's own
    /// record (`Abandoned` when its applier could not be started), never
    /// evidence of a road taken, and O does not read it.
    Outgoing,
    /// An applier: an absent or dead mark may be claimed only while the
    /// journal says no applier road has been taken.
    Applier,
}

fn take_the_window_within_using<AfterLock, ReadOwner, ReadPhase, Write, Wait>(
    home: &Home,
    txn: TxnId,
    me: Running,
    within: Duration,
    contender: Contender,
    operations: ElectionOps<AfterLock, ReadOwner, ReadPhase, Write, Wait>,
) -> Window
where
    AfterLock: FnOnce(),
    ReadOwner: FnMut() -> io::Result<Option<Running>>,
    ReadPhase: FnMut() -> Result<PhaseKind, PhaseRead>,
    Write: FnMut(&Path, &[u8]) -> Result<(), MarkWriteFailure>,
    Wait: FnMut(Duration) -> bool,
{
    let ElectionOps {
        after_lock,
        mut read_owner,
        mut read_phase,
        mut write,
        mut wait,
    } = operations;
    let until = Instant::now() + within;
    let held = match install_txn::hold_within(&owner_lock_path(home, txn), Hold::Exclusive, within)
    {
        Ok(Some(held)) => held,
        Ok(None) => {
            return Window::Refused(WindowRefusal {
                why: "the window election is still held".to_owned(),
                contended: true,
                retryable: true,
                outgoing_keeps_duty: false,
            });
        }
        Err(failure) => {
            return Window::Refused(WindowRefusal {
                why: format!("the window election: {failure}"),
                contended: false,
                retryable: false,
                outgoing_keeps_duty: true,
            });
        }
    };
    after_lock();
    let read_until = until.min(Instant::now() + JOURNAL_WRITE_WITHIN);
    match retry_within(read_until, &mut read_owner, |_| true, &mut wait) {
        Err(error) => {
            drop(held);
            Window::Refused(WindowRefusal {
                why: format!("the window's mark could not be read: {error}"),
                contended: false,
                retryable: true,
                outgoing_keeps_duty: false,
            })
        }
        Ok(Some(owner)) if owner == me => Window::Mine(WindowDuty::recorded(held, None)),
        Ok(Some(owner)) if install_flip::still_running(owner) => {
            drop(held);
            Window::Theirs(owner)
        }
        // Absent, malformed, or its process gone: for an applier the journal
        // decides whether the duty is still open before this process records
        // itself; the outgoing build has nothing to learn there
        // ([`Contender::Outgoing`]).
        Ok(_) => {
            if contender == Contender::Applier {
                match retry_within(
                    read_until,
                    &mut read_phase,
                    |read| matches!(read, PhaseRead::Unread(_) | PhaseRead::NotRead(_)),
                    &mut wait,
                ) {
                    Ok(phase) if window_duty_is_open(phase) => {}
                    Ok(phase) => {
                        drop(held);
                        return Window::RoadTaken(phase);
                    }
                    Err(PhaseRead::StoodAside(why) | PhaseRead::NotRead(why)) => {
                        drop(held);
                        return Window::StoodAside(why);
                    }
                    Err(PhaseRead::Unread(why)) => {
                        drop(held);
                        return Window::Refused(WindowRefusal {
                            why: format!(
                                "the update journal could not be read for the window election: {why}"
                            ),
                            contended: false,
                            retryable: true,
                            outgoing_keeps_duty: false,
                        });
                    }
                }
            }
            let mark = owner_path(home, txn);
            let bytes = owner_value(me);
            let write_until = until.min(Instant::now() + JOURNAL_WRITE_WITHIN);
            match retry_within(
                write_until,
                || write(&mark, bytes.as_bytes()),
                |failure| failure.refused_while_open,
                &mut wait,
            ) {
                Ok(()) => Window::Mine(WindowDuty::recorded(held, None)),
                Err(failure) if failure.after_rename => Window::Mine(WindowDuty::recorded(
                    held,
                    Some(format!(
                        "the window's mark was replaced but its directory was not flushed: {}",
                        failure.why
                    )),
                )),
                Err(failure) => Window::Mine(WindowDuty::held(
                    held,
                    format!(
                        "the window's mark could not be replaced; owner.lock records the duty: {}",
                        failure.why
                    ),
                )),
            }
        }
    }
}

/// [`take_the_window_within`] with a test step after the election lock is held
/// and before the mark is read. It lets a test hold a contender at that exact
/// state without using the clock as synchronization.
#[cfg(test)]
pub(crate) fn take_the_window_within_at(
    home: &Home,
    txn: TxnId,
    me: Running,
    within: Duration,
    after_lock: impl FnOnce(),
) -> Window {
    take_the_window_within_using(
        home,
        txn,
        me,
        within,
        Contender::Applier,
        ElectionOps {
            after_lock,
            read_owner: || read_window_owner(home, txn),
            read_phase: || read_window_phase(home, txn),
            write: |path: &Path, bytes: &[u8]| {
                MarkWriteFailure::from_result(install_txn::durable_write(path, bytes))
            },
            wait: |_| false,
        },
    )
}

/// [`take_the_window_within_at`] with an injected mark writer and pause. The
/// pause returns immediately, so scanner retries are deterministic.
#[cfg(test)]
pub(crate) fn take_the_window_within_writes_at(
    home: &Home,
    txn: TxnId,
    me: Running,
    within: Duration,
    write: impl FnMut(&Path, &[u8]) -> Result<(), MarkWriteFailure>,
) -> Window {
    take_the_window_within_using(
        home,
        txn,
        me,
        within,
        Contender::Applier,
        ElectionOps {
            after_lock: || {},
            read_owner: || read_window_owner(home, txn),
            read_phase: || read_window_phase(home, txn),
            write,
            wait: |_| true,
        },
    )
}

/// [`take_the_window_within_at`] with an injected mark reader and immediate
/// pauses. It pins transient read refusals without depending on the clock.
#[cfg(test)]
pub(crate) fn take_the_window_within_reads_at(
    home: &Home,
    txn: TxnId,
    me: Running,
    within: Duration,
    read: impl FnMut() -> io::Result<Option<Running>>,
) -> Window {
    take_the_window_within_using(
        home,
        txn,
        me,
        within,
        Contender::Applier,
        ElectionOps {
            after_lock: || {},
            read_owner: read,
            read_phase: || read_window_phase(home, txn),
            write: |path: &Path, bytes: &[u8]| {
                MarkWriteFailure::from_result(install_txn::durable_write(path, bytes))
            },
            wait: |_| true,
        },
    )
}

fn ask_for_the_window_until(
    until: Instant,
    poll: Duration,
    mut ask: impl FnMut() -> Window,
    mut wait: impl FnMut(Duration),
) -> Window {
    loop {
        let answer = ask();
        let asks_again = matches!(&answer, Window::Theirs(_))
            || matches!(&answer, Window::Refused(refusal) if refusal.retryable());
        let left = until.saturating_duration_since(Instant::now());
        if !asks_again || left.is_zero() {
            return answer;
        }
        wait(poll.min(left));
    }
}

/// **The one applier election loop**, shared by Windows and macOS: another
/// live marked owner or a contended election is asked again while the one
/// `old_within` deadline has time left. No retry receives a fresh budget.
pub(crate) fn take_the_window_for_applier(
    worker: &WorkerCtx,
    home: &Home,
    txn: TxnId,
    me: Running,
    until: Instant,
    poll: Duration,
) -> Window {
    ask_for_the_window_until(
        until,
        poll,
        || take_the_window(Some(worker), home, txn, me, until),
        |pause| bt_platform::wait::sleep_within(worker, pause),
    )
}

/// [`take_the_window_for_applier`] with injected asks and pauses.
#[cfg(test)]
pub(crate) fn take_the_window_for_applier_at(
    until: Instant,
    poll: Duration,
    ask: impl FnMut() -> Window,
    wait: impl FnMut(Duration),
) -> Window {
    ask_for_the_window_until(until, poll, ask, wait)
}

/// **Clear the window's mark** — O, as it hands the transaction over, before
/// `Handoff` is written: no mark of an earlier attempt stands. None there is
/// success.
///
/// # Errors
/// The removal's failure, as a sentence.
pub(crate) fn clear_the_window(owner: &Path) -> Result<(), String> {
    install_txn::durable_remove(owner).map_err(|failure| failure.to_string())
}

/// A live owner recovery finds at `Handoff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowHolder {
    /// The durable mark names the owner.
    Marked(Running),
    /// Another process holds `owner.lock`; a pre-rename failure left no mark,
    /// but the lock is the live authority.
    Unmarked,
}

/// **The live owner of the window duty, if it is not `me`**, as recovery asks
/// at `Handoff`. Recovery first tries the election lock without waiting. A
/// holder is an owner even without readable mark bytes; with the lock free,
/// the exact live mark remains the durable answer. If the lock file itself
/// cannot be opened, recovery falls back to that mark: an open refusal is not
/// allowed to stall a persistent `Handoff` forever.
pub(crate) fn window_holder(
    home: &Home,
    txn: TxnId,
    me: Running,
) -> Result<Option<WindowHolder>, String> {
    match install_txn::try_hold(&owner_lock_path(home, txn), Hold::Exclusive) {
        Ok(None) => Ok(Some(WindowHolder::Unmarked)),
        Ok(Some(_held)) => Ok(window_owner(home, txn)
            .filter(|owner| *owner != me && install_flip::still_running(*owner))
            .map(WindowHolder::Marked)),
        Err(_failure) => Ok(window_owner(home, txn)
            .filter(|owner| *owner != me && install_flip::still_running(*owner))
            .map(WindowHolder::Marked)),
    }
}

/// **What a road process's exit guard asks of the platform it runs on**
/// ([`ExitGuard`], U-34).
pub(crate) trait Leave {
    /// One line of what happened.
    fn say(&mut self, line: &str);
    /// **What the disk names now**: the program and its words — the handed
    /// command line after them — read from the journal and the installation
    /// at this instant ([`Opens`]), or `None` when nothing can be named (no
    /// home to read).
    fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)>;
    /// Start `program` with `words`, detached.
    ///
    /// # Errors
    /// It could not be started.
    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()>;
    /// **What to start when [`Leave::opening`]'s program would not start**, or
    /// started and never acknowledged: the previous build's rescue copy with
    /// `--update-failed`, or `None`.
    fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        None
    }
    /// **Reserve and name the one last trial after both ordinary deliveries
    /// were refused by the operating system** (0.4.7 U-35; only the Windows
    /// holders answer it). The reservation is durable before this returns.
    /// `Ok(None)`: not eligible — no new build live and uncommitted, or a
    /// phase other than `Moving` (`TrialStarting` already reserved; `Stuck`
    /// has U-29b's retrial road). `Err` names why it could not be reserved.
    fn last_trial(&mut self) -> Result<Option<(PathBuf, Vec<OsString>)>, String> {
        Ok(None)
    }
    /// **Whether the start just made was delivered** (round 2, blocker 2):
    /// a Folio holds the data directory's claim within
    /// [`ACKNOWLEDGED_WITHIN`] ([`claimed_within`]) — the one that was
    /// started, or one already running, which that start hands its launch to.
    /// A start that died, or never got that far, is not.
    fn acknowledged(&mut self) -> bool;
    /// **The failure window, in this very process** (round 2, blocker 2): when
    /// no start was delivered, this process — a Folio build — shows *Update
    /// incomplete.* and the folder itself, without a spawn. `why` is what
    /// failed.
    fn show_here(&mut self, why: &str);
}

/// **How long a start a road process makes has to be acknowledged** (U-34,
/// round 2): the Folio it started — or one already running — holding the data
/// directory's claim. A start of an image the machine has run takes a second
/// or two; 20 s leaves room for a first start a scanner reads whole (W9's
/// 5.9 s) and is still a bound a person waiting after *Restart to update*
/// meets only when something is wrong.
pub(crate) const ACKNOWLEDGED_WITHIN: Duration = Duration::from_secs(20);

/// **Whether a Folio holds the data directory `data`'s claim within
/// `within`** — the acknowledgement of a start (round 2, blocker 2). Asked
/// every quarter second through the wait door; without a worker to sleep on,
/// asked once. Only `ClaimRefusal::Held` — a live holder — acknowledges. A
/// claim this process could take is let go at once (the start that should
/// hold it has not yet), and neither a question the platform did not answer
/// (`ClaimRefusal::QueryDenied`) nor a sweep in progress
/// (`ClaimRefusal::Sweeping`) is evidence that anybody holds anything: all
/// three are asked again until the bound, and then the start is not delivered
/// (round 4, Codex's finding 12; U-43 round 3).
pub(crate) fn claimed_within(worker: Option<&WorkerCtx>, data: &Path, within: Duration) -> bool {
    let until = Instant::now() + within;
    loop {
        match crate::persist::try_claim(data) {
            // Only a live holder is a delivery. A question the platform did
            // not answer is no evidence that anybody holds anything (round 4).
            Err(bt_platform::instance::ClaimRefusal::Held) => return true,
            Err(
                bt_platform::instance::ClaimRefusal::Sweeping
                | bt_platform::instance::ClaimRefusal::QueryDenied(_),
            ) => {}
            Ok(claim) => drop(claim),
        }
        let left = until.saturating_duration_since(Instant::now());
        let Some(worker) = worker.filter(|_| !left.is_zero()) else {
            return false;
        };
        bt_platform::wait::sleep_within(worker, Duration::from_millis(250).min(left));
    }
}

/// **The failure window's words** (round 2): the update card's *Update
/// incomplete.* and the installation home's folder, as `--update-failed`'s
/// card says them.
pub(crate) fn failure_text(home: Option<&Home>) -> String {
    let incomplete = crate::i18n::Text::UpdateCardIncomplete.text();
    match home {
        Some(home) => format!("{incomplete}\n\n{}", home.root().display()),
        None => incomplete.to_owned(),
    }
}

/// **How a road process left** — what its [`ExitGuard`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Left {
    /// A successor this process started, or found, still runs — pid and start
    /// instant — and opens Folio: nothing was started.
    Succeeded(u32),
    /// Another process has the duty a window follows ([`OWNER_FILE`]), or
    /// this process left the duty with the outgoing build: nothing was
    /// started here.
    NotMine(Option<u32>),
    /// The recovery run at logon, with nothing done and nobody waiting (W8):
    /// nothing was started.
    NobodyWaiting,
    /// **A Folio holds the data directory** (U-37, H.3's deferral over a held
    /// claim): it is the window — U-34's own acknowledgement — and nothing was
    /// started beside it.
    Elsewhere,
    /// This program was started, and acknowledged: a Folio holds the data
    /// directory.
    Started(PathBuf),
    /// No start was delivered — nothing could be named, the operating system
    /// refused each program, or each started and never took the data
    /// directory — and this process showed the failure window itself; why.
    ShownHere(String),
}

impl Left {
    /// The words the door's one line ends with.
    pub(crate) fn said(&self) -> String {
        match self {
            Left::Succeeded(pid) => format!("{pid} runs and opens Folio; nothing else was started"),
            Left::NotMine(Some(pid)) => {
                format!("{pid} has the duty to open Folio; nothing was started here")
            }
            Left::NotMine(None) => String::from(
                "the window duty stayed with another process; nothing was started here",
            ),
            Left::NobodyWaiting => String::from("nobody is waiting; nothing was started"),
            Left::Elsewhere => String::from(
                "a Folio holds the data directory and opens Folio; nothing was started",
            ),
            Left::Started(program) => format!("started {}", program.display()),
            Left::ShownHere(why) => {
                format!("no start was delivered ({why}); the failure window was shown here")
            }
        }
    }
}

/// **The one way a road process leaves** (0.4.6 ticket U-34; the owner's
/// ruling of 2026-09-25 that every phase opens Folio, and the verifier's table
/// of every exit in `reports/U-31-W-verify.md`): the applier and the recovery
/// build, on both platforms, hold one from the moment they know their home
/// until they have left — a normal return, any end, any refusal, and a panic
/// unwinding (`Drop`; the workspace builds with `panic = "unwind"`, and the
/// update doors' panic hook lets it unwind).
///
/// **On leaving it does one of two things.** A successor it holds — a trial
/// this process started, the trial it found recorded, the applier it found at
/// `Handoff` — still running by its pid and start instant
/// (`install_flip::still_running`) opens Folio, and nothing is started.
/// Otherwise it starts what the disk names at that instant ([`Leave::opening`]:
/// the installed build, with `--update-failed <journal>` while the journal is
/// not over; the new build before `Committed` only as a trial; the rescue copy
/// where neither whole set is installed). A start is delivered only when it is
/// acknowledged ([`Leave::acknowledged`]: a Folio holds the data directory);
/// otherwise the next program the rule names ([`Leave::fallback`]); and when
/// both process creations are refused by the OS, U-35 reserves one last
/// trial before asking for it once ([`Leave::last_trial`]; Windows only — a
/// macOS start goes through `/usr/bin/open`, which reports no refusal by
/// LaunchServices, so the cell cannot be observed there). A created but
/// unacknowledged process does not trigger that reservation. When no start is delivered,
/// this process shows the failure window itself
/// ([`Leave::show_here`]). **The irrecoverable boundary** is what no process
/// can survive from inside: the operating system refusing to show a window at
/// all, or this process ended from outside (a kill, a power cut — the entrance
/// at logon and the next start finish those). **Two exceptions**: a process that does not have the duty a
/// window follows ([`OWNER_FILE`], [`ExitGuard::not_mine`]), and the recovery
/// run at logon that did nothing a person is owed a window for — nobody is
/// waiting ([`ExitGuard::nobody_waiting`]).
///
/// It replaces the per-road answers U-29b's rules had spread over each end
/// (the table of who owed a window after which end, and the refusals that left
/// silently): what a road decides now is only whom it leaves behind, and
/// whether it has the duty at all.
pub(crate) struct ExitGuard<L: Leave> {
    leave: L,
    /// This process's election success. An unrecorded one carries the live
    /// election lock, held in product and tests alike through the road and
    /// the guard's final delivery, and released only when the guard itself is
    /// dropped.
    window_duty: Option<WindowDuty>,
    successor: Option<Running>,
    waiting: bool,
    /// A Folio is proved to hold the data directory (U-37, H.3).
    elsewhere: bool,
    /// `Some` once this process knows it does not have the duty a window
    /// follows: the process that has it, if one is proven.
    not_mine: Option<Option<u32>>,
    left: Option<Left>,
}

impl<L: Leave> ExitGuard<L> {
    /// A guard over `leave`, owed a window until told otherwise.
    pub(crate) fn new(leave: L) -> Self {
        Self {
            leave,
            window_duty: None,
            successor: None,
            waiting: true,
            elsewhere: false,
            not_mine: None,
            left: None,
        }
    }

    /// Own an election success for the rest of this guard's lifetime. A
    /// recorded success carries no lock; an unrecorded one carries the real
    /// operating-system lock in [`WindowDuty`].
    pub(crate) fn owns_window(&mut self, duty: WindowDuty) {
        self.window_duty = Some(duty);
    }

    /// **An applier's road has ended**, having held the transaction lock or
    /// not: the one place that decides whether a duty this guard owns only
    /// through the election lock (no mark landed) goes back to the outgoing
    /// build O.
    ///
    /// O holds `H\lock` until its process ends (`update_handoff::Staged::lock`;
    /// `update_job` keeps the staged transaction "until the process leaves"),
    /// and an applier's road takes that lock before anything else
    /// (`update_apply_windows::apply_under_the_lock_with`,
    /// `update_apply_macos::Txn::hold`). So:
    ///
    /// * **held** — O's process had ended before this road began. While this
    ///   guard held `owner.lock`, O found no mark, and its election met that
    ///   lock and stood down. Nobody else is left to open Folio, so this guard
    ///   keeps the duty, whatever phase the road left the journal in.
    /// * **never held** — the road took no step, and O may still be waiting.
    ///   This guard starts nothing and lets `owner.lock` go when it is dropped.
    ///   O then finds the lock free, the mark absent and `Handoff`, and takes
    ///   the duty.
    pub(crate) fn road_ended(&mut self, lock: TransactionLock) {
        if lock == TransactionLock::NeverHeld
            && self
                .window_duty
                .as_ref()
                .is_some_and(WindowDuty::is_lock_backed)
        {
            self.not_mine(None);
        }
    }

    /// **This process does not have the duty a window follows**
    /// ([`OWNER_FILE`]): the named owner, an election-lock holder, or the
    /// outgoing build keeps it. The guard starts nothing.
    pub(crate) fn not_mine(&mut self, owner: Option<u32>) {
        self.not_mine = Some(owner);
    }

    /// What the road acts through while the guard holds it.
    pub(crate) fn inner(&mut self) -> &mut L {
        &mut self.leave
    }

    /// **The successor this process leaves behind**, by pid and start
    /// instant, if any: it opens Folio while it runs.
    pub(crate) fn succeeded_by(&mut self, successor: Option<Running>) {
        self.successor = successor;
    }

    /// **Nobody is waiting for a window** — the recovery run at logon that
    /// did nothing a person is owed one for.
    pub(crate) fn nobody_waiting(&mut self) {
        self.waiting = false;
    }

    /// **A Folio holds the data directory** (U-37, H.3): the road deferred to
    /// it, and it is the window a start would only hand itself to.
    pub(crate) fn window_elsewhere(&mut self) {
        self.elsewhere = true;
    }

    /// **Hand the duty on** to a guard constructed inside this one's scope,
    /// which now carries it: this one starts nothing when it is dropped.
    pub(crate) fn hand_on(&mut self) {
        self.left = Some(Left::NotMine(None));
    }

    /// **Leave now**: the start the exit owes, once — a second call answers
    /// the first one's result, and the drop then does nothing.
    pub(crate) fn leave(&mut self) -> Left {
        if let Some(left) = &self.left {
            return left.clone();
        }
        let left = if let Some(owner) = self.not_mine {
            Left::NotMine(owner)
        } else {
            match self
                .successor
                .filter(|successor| install_flip::still_running(*successor))
            {
                Some(successor) => Left::Succeeded(successor.pid),
                None if self.elsewhere => Left::Elsewhere,
                None if !self.waiting => Left::NobodyWaiting,
                None => self.start(),
            }
        };
        self.left = Some(left.clone());
        left
    }

    /// The start the disk names, or its fallback.
    fn start(&mut self) -> Left {
        let mut why = Vec::new();
        let mut attempts = 0usize;
        let mut refused = 0usize;
        match self.leave.opening() {
            None => why.push(String::from("nothing could be named to start")),
            Some((program, words)) => {
                let delivered = self.deliver(&program, &words, &mut why);
                attempts += 1;
                refused += usize::from(delivered.refused);
                if let Some(left) = delivered.left {
                    return left;
                }
                if let Some((next, words)) =
                    self.leave.fallback().filter(|(next, _)| *next != program)
                {
                    let delivered = self.deliver(&next, &words, &mut why);
                    attempts += 1;
                    refused += usize::from(delivered.refused);
                    if let Some(left) = delivered.left {
                        return left;
                    }
                }
            }
        }
        // "Could not be launched" is the operating system refusing both
        // process creations. A process that was created but never
        // acknowledged is a different failure and is never started again as
        // U-35's trial.
        if attempts >= 2 && refused == attempts {
            match self.leave.last_trial() {
                Ok(Some((program, words))) => {
                    let delivered = self.deliver(&program, &words, &mut why);
                    if let Some(left) = delivered.left {
                        return left;
                    }
                }
                Ok(None) => {}
                Err(error) => why.push(format!("the last trial was not recorded: {error}")),
            }
        }
        let why = why.join("; ");
        self.leave.show_here(&why);
        Left::ShownHere(why)
    }

    /// One start, and its acknowledgement: `Some` once delivered; otherwise
    /// what failed is added to `why`.
    fn deliver(&mut self, program: &Path, words: &[OsString], why: &mut Vec<String>) -> Delivery {
        match self.leave.start(program, words) {
            Ok(()) if self.leave.acknowledged() => {
                return Delivery {
                    left: Some(Left::Started(program.to_path_buf())),
                    refused: false,
                };
            }
            Ok(()) => why.push(format!(
                "{} started and no Folio took the data directory",
                program.display()
            )),
            Err(error) => {
                why.push(format!(
                    "{} could not be started: {error}",
                    program.display()
                ));
                return Delivery {
                    left: None,
                    refused: true,
                };
            }
        }
        Delivery {
            left: None,
            refused: false,
        }
    }
}

struct Delivery {
    left: Option<Left>,
    /// The operating system refused process creation (not merely an
    /// unacknowledged process).
    refused: bool,
}

impl<L: Leave> Drop for ExitGuard<L> {
    /// **The exit nobody asked for** — an early return that did not call
    /// [`ExitGuard::leave`], or a panic unwinding: the same start, and one
    /// line saying so.
    fn drop(&mut self) {
        if self.left.is_none() {
            let left = self.leave();
            let line = format!("BT_UPDATE_EXIT the road left early; {}", left.said());
            self.leave.say(&line);
        }
    }
}

/// **The words the build started after a rollback carries**: `--update-failed`
/// and the journal (the coordinator's ruling 3, U-29).
pub(crate) fn failed_words(home: &Home) -> [OsString; 2] {
    [
        OsString::from(cli::UPDATE_FAILED_FLAG),
        home.journal().into_os_string(),
    ]
}

/// **The words that start the installed build as the trial of `txn`**:
/// `--update-trial <txn> <nonce>` (U-12's frozen v1 flag).
pub(crate) fn trial_words(txn: TxnId, nonce: &Nonce) -> [OsString; 3] {
    [
        OsString::from(cli::UPDATE_TRIAL_FLAG),
        OsString::from(txn.to_string()),
        OsString::from(nonce.to_string()),
    ]
}

/// **What the disk names to start** when a road process leaves with no
/// successor running (the coordinator's rulings 2 and 3 of U-29b, adopted on
/// Windows by U-24; since U-34 read only by an [`ExitGuard`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Opens {
    /// The installed build as an ordinary start, with `--update-failed
    /// <journal>` first when `failed`: the journal is still `destructive`
    /// (the start must not hand itself back) or a retired rollback (its
    /// card).
    Installed { failed: bool },
    /// The installed build — the new one, live and not committed — as a trial
    /// of `txn` with a fresh nonce no journal records, then `--update-failed
    /// <journal>`: its writes are held back and its receipt is never heard
    /// (the new build is never started plainly before `Committed`, ruling 2).
    Trial { txn: TxnId },
    /// **Windows only, U-35**: the journal is `TrialStarting`, so the one
    /// trial it may run is its reserved one (`update_txn::Phase::reserved_trial`)
    /// — the installed new build with that nonce, then `--update-failed
    /// <journal>`, which a start admits as that very trial. Never a fresh
    /// nonce: a start with any other is handed back to recovery.
    LastTrial { txn: TxnId, nonce: Nonce },
    /// **Windows only, the fallback** (U-24): the journal is still
    /// `destructive` and the install folder holds neither whole set — a
    /// rollback that could not finish, or a layout that cannot be read — so
    /// the rescue copy, O's own image, is started with `--update-failed
    /// <journal>`: the one executable known to run, whose own home holds no
    /// journal. A macOS holder never answers it.
    Rescue,
}

impl Opens {
    /// The words the build is started with, before a handed command line.
    pub(crate) fn words(&self, home: &Home) -> Vec<OsString> {
        match self {
            Opens::Installed { failed: false } => Vec::new(),
            Opens::Installed { failed: true } | Opens::Rescue => failed_words(home).to_vec(),
            Opens::Trial { txn } => {
                let mut words = trial_words(*txn, &crate::update_job::mint_nonce()).to_vec();
                words.extend(failed_words(home));
                words
            }
            Opens::LastTrial { txn, nonce } => {
                let mut words = trial_words(*txn, nonce).to_vec();
                words.extend(failed_words(home));
                words
            }
        }
    }
}

/// **Who a recovery run is for**, which decides whether anybody waits for a
/// window when nothing was done (U-29b; since U-34 only that).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opener {
    /// R from the entrance at logon: nobody started anything, so a window is
    /// owed only after a revert or a rollback it finished — the Restart that
    /// preceded them never got one (W11) — or when a person's start is its
    /// own successor.
    Login,
    /// R handed a person's start (`--then-launch`): a window always follows.
    Start,
}

/// **Whether a recovery at logon that ended `ended` owes a window** (U-34,
/// round 2, blocker 4): every end that attempted the transaction does — a
/// revert or a rollback, finished or not (`Stuck`, `GaveUp`,
/// `RollbackWaits`), and a recovery that failed — because the Restart before it
/// never got one. Only the genuinely no-op ends owe nothing: nothing to
/// recover (`Left`), a commit's retirement finished (the update succeeded; its
/// trial was the window), the line refused, or the lock another holder keeps —
/// the holder of the window's mark, which opens it.
pub(crate) fn owed_at_logon(ended: &Ended) -> bool {
    !matches!(
        ended,
        Ended::Left(_)
            | Ended::Committed
            | Ended::CommittedWithDebt(_)
            | Ended::Refused(_)
            | Ended::LockHeld
            | Ended::OldHeldTheLock
            | Ended::Abandoned
            | Ended::Deferred(_)
            | Ended::StoodAside(_)
    )
}

/// **The lock holders and the receipt watch over a journal or a receipt this
/// build cannot read whole** (0.4.8 E1): each role is fed the three inputs of
/// `update_txn::beyond_inputs` over a home in a temporary folder, with the
/// real lock, read and write, on a worker the thread door lends.
#[cfg(test)]
mod beyond_tests {
    use super::*;
    use crate::update_txn::{Adapter, Body, Inventories, Layout, beyond_inputs};

    const TXN: TxnId = TxnId::new([0x6e; 16]);

    fn nonce() -> Nonce {
        Nonce::new([0x2d; 32])
    }

    /// A home in a temporary folder, its transaction's folder made, and the
    /// bytes of its journal at `phase`, as this build writes them.
    fn home_at(tag: &str, phase: Phase) -> (PathBuf, Home, Vec<u8>) {
        let root = bt_testpath::temp_path(&format!("bt-update-beyond-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        let home = Home::at(root.join("home"));
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let bytes = Journal {
            txn: TXN,
            rescue: "rescue".to_owned(),
            body: Body {
                phase,
                layout: Layout::Members(Inventories {
                    old_shipped: vec!["folio.exe".to_owned()],
                    old_present: Vec::new(),
                    new: Vec::new(),
                }),
                adapter: Adapter::Ours,
            },
        }
        .encode();
        (root, home, bytes)
    }

    fn on_a_worker<T: Send + 'static>(body: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        bt_platform::spawn_at_priority(
            "bt-update-beyond-test",
            bt_platform::ThreadPriority::BelowNormal,
            body,
        )
        .expect("the thread door starts a thread")
        .join()
        .expect("the worker does not panic")
    }

    /// The transaction lock is free: nobody kept it.
    fn lock_is_free(home: &Home) -> bool {
        install_txn::try_hold(&home.lock(), Hold::Exclusive)
            .unwrap()
            .is_some()
    }

    /// RED (E1; role #6, U-35's reservation, site J3 `reserve_last_trial`) —
    /// **the exit guard's reservation stands aside from a journal this build
    /// cannot read whole**: it records nothing, the journal is byte for byte
    /// as it was, and the lock is let go; the guard then shows its window
    /// (`WindowsLeave::last_trial` answers the line as its refusal).
    ///
    /// MUTATION: restore the pre-E1 read in `reserve_last_trial`
    /// (`Journal::parse(&bytes).map_err(..)?`: an `Err`, not a stand-aside).
    #[test]
    fn the_reservation_stands_aside_from_what_it_cannot_read_whole() {
        let (root, home, known) = home_at("reserve", Phase::Moving);
        for (what, bytes) in beyond_inputs(&known) {
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            let at = home.clone();
            let reserved =
                on_a_worker(move |worker| reserve_last_trial(worker, &at, Actor::Applier));
            assert!(
                matches!(reserved, Ok(Reserved::StoodAside(_))),
                "{what}: {reserved:?}"
            );
            assert_eq!(std::fs::read(home.journal()).unwrap(), bytes, "{what}");
            assert!(lock_is_free(&home), "{what}: the lock is let go");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (E1; role #7, U-35's self-commit, site J4 `commit_last_trial_as`)
    /// — **the reserved trial stands aside from a journal this build cannot
    /// read whole**: it records nothing and never asks again
    /// (`LastTrialCommit::StoodAside`); the journal is byte for byte as it
    /// was and the lock is let go — its writes stay held, and the build that
    /// wrote the journal decides.
    ///
    /// MUTATION: restore the pre-E1 read in `commit_last_trial_as`
    /// (`Journal::parse(&bytes).map_err(..)?`: an `Err`, asked again every
    /// turn).
    #[test]
    fn the_reserved_trial_stands_aside_from_what_it_cannot_read_whole() {
        let (root, home, known) = home_at(
            "commit",
            Phase::TrialStarting {
                nonce: nonce(),
                began_ms: 42,
            },
        );
        for (what, bytes) in beyond_inputs(&known) {
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            let at = home.clone();
            let committed = on_a_worker(move |worker| {
                commit_last_trial_as(worker, &at, TXN, nonce(), 4242, Some(7))
            });
            assert_eq!(committed, Ok(LastTrialCommit::StoodAside), "{what}");
            assert_eq!(std::fs::read(home.journal()).unwrap(), bytes, "{what}");
            assert!(lock_is_free(&home), "{what}: the lock is let go");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (E1 round 3; role #7, U-35's self-commit, site J4) — **a journal
    /// read that fails other than "no such file" is asked again before the
    /// reserved trial stands aside**: one refused read (a scanner holding
    /// `journal.json`) is followed by a read that answers, and the commit goes
    /// on (`Pending`: its receipt is not there yet); a read that keeps
    /// failing until the wait gives up is stood aside from. The journal is byte
    /// for byte as it was and the lock is let go. No clock: the pause is the
    /// test's own, and answers when to stop asking.
    ///
    /// MUTATION: in `commit_last_trial_reading`, ask once only
    /// (`|_| false` as the retry's test) — one transient refusal ends the
    /// self-commit for good.
    #[test]
    fn the_reserved_trial_asks_again_before_it_stands_aside_from_a_journal_it_cannot_read() {
        let (root, home, known) = home_at(
            "commit-transient",
            Phase::TrialStarting {
                nonce: nonce(),
                began_ms: 42,
            },
        );
        install_txn::durable_write(&home.journal(), &known).unwrap();
        let refused = || io::Error::from(io::ErrorKind::PermissionDenied);

        let at = home.clone();
        let bytes = known.clone();
        let (answer, reads) = on_a_worker(move |worker| {
            let mut reads = 0;
            let answer = commit_last_trial_reading(
                worker,
                &at,
                (TXN, nonce()),
                (4242, Some(7)),
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(refused())
                    } else {
                        Ok(bytes.clone())
                    }
                },
                |_| true,
            );
            (answer, reads)
        });
        assert_eq!(
            answer,
            Ok(LastTrialCommit::Pending),
            "asked again, then read"
        );
        assert_eq!(reads, 2);

        let at = home.clone();
        let (answer, pauses) = on_a_worker(move |worker| {
            let mut pauses = 0;
            let answer = commit_last_trial_reading(
                worker,
                &at,
                (TXN, nonce()),
                (4242, Some(7)),
                || Err(refused()),
                |_| {
                    pauses += 1;
                    pauses < 3
                },
            );
            (answer, pauses)
        });
        assert_eq!(answer, Ok(LastTrialCommit::StoodAside), "never read");
        assert_eq!(pauses, 3);
        assert_eq!(std::fs::read(home.journal()).unwrap(), known);
        assert!(lock_is_free(&home), "the lock is let go");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (E1; role #8, the applier's window election, site J5
    /// `read_window_phase`) — **an applier stands aside from a journal this
    /// build cannot read whole**: it writes no mark, the journal is byte for
    /// byte as it was, and its answer is `Window::StoodAside`, which ends the
    /// applier as `Ended::StoodAside` with the window duty left to the build
    /// that armed it.
    ///
    /// MUTATION: map `PhaseRead::StoodAside` back to the pre-E1 retryable
    /// `Window::Refused` in the election.
    #[test]
    fn the_window_election_stands_aside_from_what_it_cannot_read_whole() {
        let (root, home, known) = home_at("election", Phase::Handoff { applier: nonce() });
        for (what, bytes) in beyond_inputs(&known) {
            install_txn::durable_write(&home.journal(), &bytes).unwrap();
            let window = take_the_window_within_reads_at(
                &home,
                TXN,
                this_process(),
                Duration::from_millis(500),
                || Ok(None),
            );
            assert!(
                matches!(window, Window::StoodAside(_)),
                "{what}: {window:?}"
            );
            assert!(!owner_path(&home, TXN).exists(), "{what}: no mark");
            assert_eq!(std::fs::read(home.journal()).unwrap(), bytes, "{what}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (E1; role #5, the Windows lock holder's receipt watch, site R1
    /// `read_receipt`) — **a receipt this build cannot read is never
    /// accepted**: a later receipt version, a v1 receipt with a word this
    /// build does not read, and bytes that are no receipt each read as a
    /// refusal, said and never turned into `Committed`; the bytes are left as
    /// they are.
    ///
    /// MUTATION: drop the version check from the receipt's read (`versioned`
    /// in `Receipt::parse`): a later receipt version is accepted.
    #[test]
    fn the_receipt_watch_never_accepts_what_it_cannot_read() {
        let (root, home, _) = home_at("receipt", Phase::Moving);
        let receipt = Receipt {
            txn: TXN,
            nonce: nonce(),
            pid: 4242,
            version: crate::update_txn::LATER_BUILD.to_owned(),
            started: Some(7),
        };
        let path = home.receipt_path(TXN, &nonce());
        for (what, bytes) in crate::update_txn::receipt_beyond_inputs(&receipt) {
            std::fs::write(&path, &bytes).unwrap();
            let read = read_receipt(&path);
            assert!(matches!(read, Some(Err(_))), "{what}: {read:?}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{what}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
#[cfg(test)]
mod exit_guard_tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum StartStatus {
        Created,
        Refused,
        Unacknowledged,
    }

    struct Door {
        statuses: [StartStatus; 3],
        starts: Vec<PathBuf>,
        last_trials: usize,
    }

    impl Leave for Door {
        fn say(&mut self, _line: &str) {}

        fn opening(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
            Some((PathBuf::from("primary"), Vec::new()))
        }

        fn start(&mut self, program: &Path, _words: &[OsString]) -> io::Result<()> {
            self.starts.push(program.to_path_buf());
            if self.statuses[self.starts.len() - 1] == StartStatus::Refused {
                Err(io::Error::other("refused (test)"))
            } else {
                Ok(())
            }
        }

        fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
            Some((PathBuf::from("fallback"), Vec::new()))
        }

        fn last_trial(&mut self) -> Result<Option<(PathBuf, Vec<OsString>)>, String> {
            self.last_trials += 1;
            Ok(Some((PathBuf::from("last-trial"), Vec::new())))
        }

        fn acknowledged(&mut self) -> bool {
            self.statuses[self.starts.len() - 1] == StartStatus::Created
        }

        fn show_here(&mut self, _why: &str) {}
    }

    /// RED (U-35 round 2) — **the complete primary × rescue × last-start
    /// table**: created, refused and created-but-unacknowledged are distinct at
    /// every position. The last start exists only after two refusals; its
    /// creation is a delivery, while its refusal or non-acknowledgement names
    /// the failure without a fourth request.
    ///
    /// MUTATION: count an unacknowledged `Ok` as a refusal, or trigger after
    /// only one `Err`.
    #[test]
    fn u35_the_last_trial_requires_two_os_launch_refusals() {
        let statuses = [
            StartStatus::Created,
            StartStatus::Refused,
            StartStatus::Unacknowledged,
        ];
        for primary in statuses {
            for rescue in statuses {
                for last in statuses {
                    let mut guard = ExitGuard::new(Door {
                        statuses: [primary, rescue, last],
                        starts: Vec::new(),
                        last_trials: 0,
                    });
                    let left = guard.leave();
                    let (attempts, last_trials, delivered) = match (primary, rescue, last) {
                        (StartStatus::Created, _, _) => (1, 0, true),
                        (_, StartStatus::Created, _) => (2, 0, true),
                        (StartStatus::Refused, StartStatus::Refused, StartStatus::Created) => {
                            (3, 1, true)
                        }
                        (StartStatus::Refused, StartStatus::Refused, _) => (3, 1, false),
                        _ => (2, 0, false),
                    };
                    assert_eq!(
                        matches!(left, Left::Started(_)),
                        delivered,
                        "{primary:?} / {rescue:?} / {last:?}: {left:?}"
                    );
                    assert_eq!(guard.inner().last_trials, last_trials);
                    assert_eq!(guard.inner().starts.len(), attempts);
                }
            }
        }
    }
}
