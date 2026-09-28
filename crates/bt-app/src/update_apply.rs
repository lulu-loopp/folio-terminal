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
//!   `install_txn::durable_write`; an effect is asked of `update_txn::may`
//!   first;
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
//! * [`an_earlier_holder`] — **the one rule for a live applier at `Handoff`**
//!   (U-24, the coordinator's ruling 3): recovery that finds `Handoff` while a
//!   process of the rescue executable started no later than it runs yields to
//!   it, writes nothing and opens nothing — that holder opens Folio;
//! * [`Opens`] / [`Opener`] — exactly one start after a road, or none (U-29b's
//!   rules, adopted on Windows by U-24).
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

impl Recording for Journaled {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Watched {
    /// A receipt the journal accepted: `Committed` is durable.
    Committed,
    /// The trial gone without a receipt, or its deadline passed; nothing more
    /// was recorded.
    NoReceipt,
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
/// image), or the deadline passed → [`Watched::NoReceipt`].
///
/// # Errors
/// The journal records no trial and none was started, or a write failed;
/// nothing more was recorded.
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
                    txn.record(actor, &event)?;
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

/// **The one rule for a live applier at `Handoff`** (U-29b's, made both
/// platforms' by U-24, the coordinator's ruling 3): of `listed` — the
/// processes running from the rescue executable — one that is not `mine` and
/// started no later than it. The applier O started takes the transaction lock
/// the moment it is free, so a recovery that finds `Handoff` under the lock
/// while such a process lives leaves the transaction to it: it writes nothing,
/// waits for nothing and opens nothing — that holder opens Folio.
pub(crate) fn an_earlier_holder(mine: Running, listed: &[Running]) -> Option<u32> {
    listed
        .iter()
        .find(|other| other.pid != mine.pid && other.started <= mine.started)
        .map(|other| other.pid)
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

/// **What opens once a lock holder has let the lock go** (the coordinator's
/// rulings 2 and 3 of U-29b, adopted on Windows by U-24): one start, or none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Opens {
    /// Nothing: a trial this holder started is the window, another holder is
    /// at work, or nothing is owed.
    Nothing,
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
    /// The words the build is started with, before a handed command line;
    /// `None` for [`Opens::Nothing`].
    pub(crate) fn words(&self, home: &Home) -> Option<Vec<OsString>> {
        match self {
            Opens::Nothing => None,
            Opens::Installed { failed: false } => Some(Vec::new()),
            Opens::Installed { failed: true } | Opens::Rescue => Some(failed_words(home).to_vec()),
            Opens::Trial { txn } => {
                let mut words = trial_words(*txn, &crate::update_job::mint_nonce()).to_vec();
                words.extend(failed_words(home));
                Some(words)
            }
        }
    }
}

/// **Who starts a build once the lock is let go**, which decides what is owed
/// (U-29b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opener {
    /// P at the end of its road: what the restart it took over owes — the
    /// old build after a revert, the live build after a rollback, nothing
    /// after a commit (the trial is the window) or when it did not get that
    /// far (U-29's ruling 1).
    Applier,
    /// R from the entrance at logon: a start only after a revert or a
    /// finished rollback (W11).
    Login,
    /// R handed a person's start (`--then-launch`): always one start, even
    /// when the recovery itself failed (the coordinator's ruling 2).
    Start,
}
