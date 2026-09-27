//! **What both appliers share** (0.4.6 tickets U-28 and U-23;
//! `docs/plans/design/self-update-2026-09-16.md` §C.4, §C.5, revision (b)
//! §(b).2 and "Who may write what").
//!
//! The macOS applier (`update_apply_macos`, U-28) and the Windows one
//! (`update_apply_windows`, U-23) take a handed-over transaction to
//! `Committed` by different effects — one exchange of a bundle, or one move
//! per file — but around those effects they are one machine, and it lives
//! here, lifted out of U-28's module as `update_prepare` was lifted out of
//! U-27's. **The Windows applier and recovery are built on it; the macOS
//! applier still carries its own copy** — U-29 and U-29b grew it (the
//! rollback, `Stuck`, the recovery of every phase, wider `Limits` and
//! `Ended`), and moving it onto this module is a follow-up of its own (U-23's
//! report, "where the two must be reconciled"):
//!
//! * [`Limits`] — §C.4's 60 s for the old build, §C.5's 90 s for the trial,
//!   and how often a waiting applier looks;
//! * [`Ended`] — where an applier (or the Windows recovery) stopped, and the
//!   exit code it answers;
//! * [`Journaled`] — the journal as it stands durably and every phase this
//!   process wrote: a phase is recorded through `update_txn::Journal::advance`
//!   (the protocol's refusal), only when `update_txn::may_record` says the
//!   actor may, and written with `install_txn::durable_write`; an effect is
//!   asked of `update_txn::may` first;
//! * [`wait_for_the_claim`] — §C.4's authoritative test that the old build is
//!   gone: the data directory's claim, tried until had and let go at once;
//! * [`await_receipt`] — from `Trial` to `Committed` on a receipt the journal
//!   accepts, or to `RollbackIntent` when the trial dies without one or its
//!   deadline passes (W7, W8, M7, M8). The Windows recovery waits on a trial
//!   a dead applier started through the same loop, as `Actor::Recovery`.
//!
//! Every wait here sleeps through the worker's wait door
//! (`bt_platform::wait::sleep_within`), on the `WorkerCtx` of the standalone
//! main the applier runs on.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bt_platform::admission::WorkerCtx;
use bt_platform::file_reads::{self, Lane};
use bt_platform::install_txn;

use crate::update_txn::{Actor, Effect, Event, Home, Journal, Phase, PhaseKind, Receipt};

/// **How long an applier waits, and how often it looks.**
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// For O's lock, O's claim and the exclusive admission together (§C.4's
    /// 60 s).
    pub(crate) old_within: Duration,
    /// For the trial's receipt, counted from the launch (§C.5's 90 s,
    /// `update_txn::TRIAL_DEADLINE_MS`).
    pub(crate) trial_within_ms: u64,
    /// Between two looks at the claim, at the admission, and at the trial.
    pub(crate) poll: Duration,
}

impl Limits {
    /// The product's.
    pub(crate) const PRODUCT: Self = Self {
        old_within: Duration::from_secs(60),
        trial_within_ms: crate::update_txn::TRIAL_DEADLINE_MS,
        poll: Duration::from_millis(250),
    };
}

/// **Where an applier, or the Windows recovery, stopped.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// `Committed`, then the rollback material and the entrance removed.
    Committed,
    /// `Committed` is durable; a deletion after it failed and is the next
    /// actor's debt (W12, M11), never a rollback.
    CommittedWithDebt(String),
    /// `RollbackIntent` is durable; the rollback is the next lock holder's
    /// (U-24 on Windows, U-29 on macOS).
    RollbackIntent,
    /// `Reverted` to `Prepared`: the entrance was found or removed, admission
    /// was refused, or the files were not the journal's.
    Reverted,
    /// `Abandoned`: O did not let go, the entrance could not be armed, or the
    /// staged set was no longer what was verified.
    Abandoned,
    /// O still held the transaction lock at the end of the wait: nothing was
    /// written.
    OldHeldTheLock,
    /// Another lock holder kept the transaction lock for the whole of the
    /// recovery's wait: nothing was written (the Windows recovery).
    LockHeld,
    /// The phase found is no step of this process's to take (the Windows
    /// recovery: a rollback is U-24's), and it was left as it is.
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
            Ended::Committed | Ended::CommittedWithDebt(_) => 0,
            Ended::Refused(_) => 2,
            _ => 1,
        }
    }
}

/// **The journal of one transaction under its lock**: as it stands durably,
/// and every phase this process wrote, in order.
pub(crate) struct Journaled {
    /// `H\journal.json`.
    path: PathBuf,
    pub(crate) journal: Journal,
    pub(crate) written: Vec<PhaseKind>,
}

impl Journaled {
    /// The journal `journal`, as read from the home `home`.
    pub(crate) fn of(home: &Home, journal: Journal) -> Self {
        Self {
            path: home.journal(),
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
        install_txn::durable_write(&self.path, &next.encode())
            .map_err(|failure| failure.to_string())?;
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

/// **How a trial's wait ended.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// `Committed` is durable.
    Committed,
    /// `RollbackIntent{trial}` is durable.
    RollbackIntent,
}

/// **From `Trial` to a decided outcome** (W7, W8, M7, M8): each look reads
/// the receipt `H\<txn>\health-<nonce>` of the journal's trial and offers it to
/// the protocol — a receipt of this transaction and this trial's nonce, while
/// the journal says `Trial`, is recorded as `Committed` by `actor`; any other
/// is said once and waited past. Otherwise the recorded trial process is asked
/// after (`alive`, by its pid *and* its start time): gone without a receipt,
/// or `trial_within_ms` after the launch, is `RollbackIntent`. Between looks
/// the wait door sleeps `poll`.
///
/// # Errors
/// The journal is not at `Trial`, or a write failed; nothing more was
/// recorded.
pub(crate) fn await_receipt(
    worker: &WorkerCtx,
    txn: &mut Journaled,
    home: &Home,
    actor: Actor,
    limits: &Limits,
    alive: &mut dyn FnMut(crate::update_txn::TrialProcess) -> bool,
    say: &mut dyn FnMut(&str),
) -> Result<Verdict, String> {
    let Phase::Trial {
        nonce,
        process,
        began_ms,
    } = txn.journal.body.phase.clone()
    else {
        return Err(format!("the journal says {:?}, not Trial", txn.phase()));
    };
    let deadline = began_ms.saturating_add(limits.trial_within_ms);
    let receipt_path = home.receipt_path(txn.journal.txn, &nonce);
    let mut said_refusal = false;
    loop {
        match read_receipt(&receipt_path) {
            Some(Ok(receipt)) => {
                let event = Event::ReceiptAccepted(receipt);
                match txn.journal.advance(&event) {
                    Ok(_) => {
                        txn.record(actor, &event)?;
                        return Ok(Verdict::Committed);
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
            txn.record(actor, &Event::RollbackDeclared)?;
            return Ok(Verdict::RollbackIntent);
        }
        let now = now_ms();
        if now >= deadline {
            say("BT_UPDATE_APPLY no receipt by the trial's deadline");
            txn.record(actor, &Event::RollbackDeclared)?;
            return Ok(Verdict::RollbackIntent);
        }
        bt_platform::wait::sleep_within(
            worker,
            limits.poll.min(Duration::from_millis(deadline - now)),
        );
    }
}
