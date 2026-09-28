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
//!   the one this holder just launched and has not yet found; its receipt
//!   offered to the protocol, `Committed` recorded on the one it accepts;
//!   else the trial gone or its deadline passed (U-29b's watch, moved here by
//!   U-24, the coordinator's ruling 4);
//! * [`stop_trial`] — W9/M9's stop: asked to quit, 5 s of grace, then ended —
//!   each through `bt_platform::install_flip::ask`, which touches nothing that
//!   is not that very trial (pid, start instant and image);
//! * [`OWNER_FILE`] — **the window's owner** (U-34): the one process that has
//!   taken the duty that a Folio window follows *Restart to update*, handed
//!   from O to P explicitly ([`take_the_window`]); recovery that finds
//!   `Handoff` leaves it to a live process the mark names
//!   ([`the_window_is_theirs`]), writes nothing and opens nothing;
//! * [`ExitGuard`] — **the one way a road process leaves** (U-34): at its
//!   exit, whatever the reason, a successor it holds still running opens
//!   Folio, else it starts what the disk names ([`Opens`]); the recovery run
//!   at logon with nothing done is the one exception ([`Opener`]).
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
use bt_platform::install_txn;

use crate::cli;
use crate::update_txn::{
    Actor, Effect, Event, Home, Journal, Nonce, Phase, PhaseKind, Receipt, TrialProcess, TxnId,
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
}

impl Ended {
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
    let until = Instant::now() + JOURNAL_WRITE_WITHIN;
    let mut pause = Duration::from_millis(10);
    loop {
        match install_txn::durable_write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(failure) if failure.refused_while_open() => {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(failure.to_string());
                }
                bt_platform::wait::sleep_within(worker, pause.min(left));
                pause = pause.saturating_mul(2);
            }
            Err(failure) => return Err(failure.to_string()),
        }
    }
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
/// tries through the wait door, until `until`. A claim that cannot be asked
/// about is not asked again (`ClaimRefusal::QueryDenied`: no answer is
/// coming).
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
            Err(bt_platform::instance::ClaimRefusal::Held) => {
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

/// The receipt at `path`: `None` while there is none, else what it says.
pub(crate) fn read_receipt(path: &Path) -> Option<Result<Receipt, String>> {
    match file_reads::read(Lane::UpdateJournal, path) {
        Ok(bytes) => Some(Receipt::parse(&bytes).map_err(|refusal| refusal.to_string())),
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
/// its nonce and when), which `find` names from what it can see (given the
/// launch's `began_ms` and the receipt, if one is there already) and which is
/// then recorded (`TrialBegan` over `Moving`, `RetrialBegan` over `Stuck`).
/// Then, polling through the wait door until the deadline counted from its
/// start: a receipt the journal accepts — this transaction's, this trial's
/// nonce — is recorded as `Committed` by the watch's
/// actor ([`Watched::Committed`]); any other is said once and waited past. The
/// recorded process no longer `alive` (by its pid, its start time and its
/// image), or the deadline passed → [`Watched::NoReceipt`]. The launched trial
/// found but not recorded → [`Watched::Unrecorded`].
///
/// # Errors
/// The journal records no trial and none was started, or a write after the
/// trial's record failed; nothing more was recorded.
pub(crate) fn watch_trial(
    worker: &WorkerCtx,
    txn: &mut impl Recording,
    watch: &Watch<'_>,
    find: &mut dyn FnMut(u64, Option<&Receipt>) -> Option<TrialProcess>,
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
                let found = receipt.as_ref().and_then(|read| read.as_ref().ok());
                if let Some(process) = find(began_ms, found) {
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
/// Taking is `install_txn::durable_create`, which never replaces: of two
/// processes that try, exactly one creates it. A mark whose process no longer
/// runs (pid and start instant) is stale — its owner died — and is taken over
/// by a single-winner election keyed on the stale value read (a ballot file
/// created the same never-replacing way), whose one winner replaces it.
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
    let bytes = file_reads::read(Lane::UpdateJournal, owner_path(home, txn)).ok()?;
    owner_named(&bytes)
}

/// **Who has the duty a window follows**, as [`take_the_window`] found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Window {
    /// This process: it opens a window when it leaves.
    Mine,
    /// This live process has it: it opens the window.
    Theirs(Running),
    /// The mark could be neither made nor read: nobody is proven to have it.
    Unknown(String),
}

/// **Take the duty a window follows, unless a live process already has it**
/// ([`OWNER_FILE`]): the mark created for `me`, never over another's; a mark
/// naming a process that no longer runs is stale and is replaced.
pub(crate) fn take_the_window(home: &Home, txn: TxnId, me: Running) -> Window {
    let path = owner_path(home, txn);
    let value = owner_value(me);
    // A few rounds: each ends in a decision, unless the mark changed under us.
    for _ in 0..4 {
        match install_txn::durable_create(&path, value.as_bytes()) {
            Ok(()) => return Window::Mine,
            Err(failure) if failure.error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(failure) => return Window::Unknown(failure.to_string()),
        }
        let read = match file_reads::read(Lane::UpdateJournal, &path) {
            Ok(bytes) => bytes,
            // Cleared between the create and the read: try the create again.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Window::Unknown(error.to_string()),
        };
        match owner_named(&read) {
            Some(owner) if owner == me => return Window::Mine,
            Some(owner) if install_flip::still_running(owner) => return Window::Theirs(owner),
            // Stale, or unreadable: its owner cannot open anything.
            _ => {}
        }
        // **The takeover is an election keyed on the value read** (round 4,
        // Codex's finding 14): of the contenders that read this very stale
        // value, only the one whose ballot `owner.takeover.<value>` is created
        // (a create that never replaces) replaces the mark — and nothing else
        // ever writes over that value, so the replacement is of exactly the
        // value it read as stale.
        let ballot = home
            .transaction(txn)
            .join(format!("{OWNER_FILE}.takeover.{}", ballot_key(&read)));
        match install_txn::durable_create(&ballot, value.as_bytes()) {
            Ok(()) => {
                return match install_txn::durable_write(&path, value.as_bytes()) {
                    Ok(()) => Window::Mine,
                    Err(failure) => Window::Unknown(failure.to_string()),
                };
            }
            Err(failure) if failure.error.kind() == io::ErrorKind::AlreadyExists => {
                let winner = file_reads::read(Lane::UpdateJournal, &ballot)
                    .ok()
                    .and_then(|bytes| owner_named(&bytes));
                match winner {
                    Some(winner) if winner == me => return Window::Mine,
                    Some(winner) if install_flip::still_running(winner) => {
                        return Window::Theirs(winner);
                    }
                    // The winner is gone too: read the mark again — it has
                    // either the dead winner's value, a fresh election of its
                    // own, or still the old one.
                    _ => {}
                }
            }
            Err(failure) => return Window::Unknown(failure.to_string()),
        }
    }
    Window::Unknown(String::from("the window's mark kept changing"))
}

/// The ballot name for a stale mark's bytes: its digits and separator, or
/// `unreadable`.
fn ballot_key(bytes: &[u8]) -> String {
    let key: String = bytes
        .iter()
        .take(48)
        .filter_map(|byte| match byte {
            b'0'..=b'9' => Some(char::from(*byte)),
            b':' => Some('-'),
            _ => None,
        })
        .collect();
    if key.is_empty() {
        String::from("unreadable")
    } else {
        key
    }
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

/// **The live process the window's mark names, if it is not `me`** — the one
/// R leaves a `Handoff` to. A process the mark does not name is never waited
/// for, however it runs.
pub(crate) fn the_window_is_theirs(home: &Home, txn: TxnId, me: Running) -> Option<Running> {
    window_owner(home, txn).filter(|owner| *owner != me && install_flip::still_running(*owner))
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
    /// started and never acknowledged: the next program the disk rule names —
    /// on Windows the rescue copy with `--update-failed`, O's own image, known
    /// to run — or `None`.
    fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        None
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
/// hold it has not yet), and a question the platform did not answer
/// (`ClaimRefusal::QueryDenied`) is no evidence that anybody holds anything:
/// both are asked again until the bound, and then the start is not delivered
/// (round 4, Codex's finding 12).
pub(crate) fn claimed_within(worker: Option<&WorkerCtx>, data: &Path, within: Duration) -> bool {
    let until = Instant::now() + within;
    loop {
        match crate::persist::try_claim(data) {
            // Only a live holder is a delivery. A question the platform did
            // not answer is no evidence that anybody holds anything (round 4).
            Err(bt_platform::instance::ClaimRefusal::Held) => return true,
            Err(bt_platform::instance::ClaimRefusal::QueryDenied(_)) => {}
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
    /// nobody is proven to have it and this process never took it: nothing
    /// was started here.
    NotMine(Option<u32>),
    /// The recovery run at logon, with nothing done and nobody waiting (W8):
    /// nothing was started.
    NobodyWaiting,
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
            Left::NotMine(None) => {
                String::from("this process never took the duty to open Folio; nothing was started")
            }
            Left::NobodyWaiting => String::from("nobody is waiting; nothing was started"),
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
/// no start is delivered, this process shows the failure window itself
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
    successor: Option<Running>,
    waiting: bool,
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
            successor: None,
            waiting: true,
            not_mine: None,
            left: None,
        }
    }

    /// **This process does not have the duty a window follows**
    /// ([`OWNER_FILE`]): `owner` has it, or nobody is proven to — then the
    /// process that armed first (O) keeps it. The guard starts nothing.
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
        match self.leave.opening() {
            None => why.push(String::from("nothing could be named to start")),
            Some((program, words)) => {
                if let Some(left) = self.deliver(&program, &words, &mut why) {
                    return left;
                }
                if let Some((next, words)) =
                    self.leave.fallback().filter(|(next, _)| *next != program)
                    && let Some(left) = self.deliver(&next, &words, &mut why)
                {
                    return left;
                }
            }
        }
        let why = why.join("; ");
        self.leave.show_here(&why);
        Left::ShownHere(why)
    }

    /// One start, and its acknowledgement: `Some` once delivered; otherwise
    /// what failed is added to `why`.
    fn deliver(
        &mut self,
        program: &Path,
        words: &[OsString],
        why: &mut Vec<String>,
    ) -> Option<Left> {
        match self.leave.start(program, words) {
            Ok(()) if self.leave.acknowledged() => {
                return Some(Left::Started(program.to_path_buf()));
            }
            Ok(()) => why.push(format!(
                "{} started and no Folio took the data directory",
                program.display()
            )),
            Err(error) => why.push(format!(
                "{} could not be started: {error}",
                program.display()
            )),
        }
        None
    }
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
    )
}
