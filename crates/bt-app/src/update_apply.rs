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
//! * [`an_earlier_holder`] — **the one rule for a live applier at `Handoff`**
//!   (U-24, the coordinator's ruling 3): recovery that finds `Handoff` while a
//!   process of the rescue executable started no later than it runs — and is
//!   not the predecessor that started this chain ([`PREDECESSOR_VARIABLE`],
//!   U-34) — yields to it, writes nothing and opens nothing: that holder opens
//!   Folio;
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

/// **The one rule for a live applier at `Handoff`** (U-29b's, made both
/// platforms' by U-24, the coordinator's ruling 3): of `listed` — the
/// processes running from the rescue executable — one that is not `mine`,
/// is not `predecessor`, and started no later than `mine`. The applier O
/// started takes the transaction lock the moment it is free, so a recovery
/// that finds `Handoff` under the lock while such a process lives leaves the
/// transaction to it: it writes nothing, waits for nothing and opens nothing —
/// that holder opens Folio.
///
/// **`predecessor` is the road process whose exit guard started this chain**
/// (U-34, [`PREDECESSOR_VARIABLE`]): a rescue-image process that is leaving,
/// not an applier to come, and never taken for one. The inference that an
/// older process of the rescue image is the applier is kept for the one road
/// that needs it — O's hand-over, whose applier may start late.
pub(crate) fn an_earlier_holder(
    mine: Running,
    predecessor: Option<Running>,
    listed: &[Running],
) -> Option<Running> {
    listed
        .iter()
        .find(|other| {
            other.pid != mine.pid && Some(**other) != predecessor && other.started <= mine.started
        })
        .copied()
}

/// **The environment word every start an exit guard makes carries** (U-34):
/// `FOLIO_UPDATE_PREDECESSOR=<pid>:<creation>`, the starting process's pid and
/// its start instant (`bt_platform::install_flip::started_of`: on Windows the
/// creation time in 100 ns since 1601, on macOS microseconds since 1970).
/// Frozen at v1: the build it is handed to may be another version, and it
/// passes the word on untouched — an environment is inherited by every child
/// without the child's grammar knowing it, so the ordinary start between a
/// guard and the recovery build it hands itself to carries the mark whatever
/// its version. The recovery build reads it before it infers anything from an
/// older process of the rescue image ([`an_earlier_holder`]).
pub(crate) const PREDECESSOR_VARIABLE: &str = "FOLIO_UPDATE_PREDECESSOR";

/// This process, by its pid and start instant (`started` 0 when it cannot be
/// read: nothing then matches it).
pub(crate) fn this_process() -> Running {
    let pid = std::process::id();
    Running {
        pid,
        started: install_flip::started_of(pid).unwrap_or(0),
    }
}

/// **The value of [`PREDECESSOR_VARIABLE`] naming `me`**: `<pid>:<creation>`,
/// both in decimal.
pub(crate) fn predecessor_value(me: Running) -> OsString {
    OsString::from(format!("{}:{}", me.pid, me.started))
}

/// **The predecessor a value of [`PREDECESSOR_VARIABLE`] names**, or `None`
/// for none, or for a value that is not exactly `<pid>:<creation>`.
pub(crate) fn predecessor_named(value: Option<&std::ffi::OsStr>) -> Option<Running> {
    let (pid, started) = value?.to_str()?.split_once(':')?;
    Some(Running {
        pid: pid.parse().ok()?,
        started: started.parse().ok()?,
    })
}

/// The predecessor this process was started with, as [`take_predecessor`]
/// read it; `None` before that, or when there was none.
pub(crate) fn predecessor_here() -> Option<Running> {
    PREDECESSOR.get().copied().flatten()
}

/// What [`take_predecessor`] read.
static PREDECESSOR: std::sync::OnceLock<Option<Running>> = std::sync::OnceLock::new();

/// **Read this process's predecessor mark once, and take it out of the
/// environment** (U-34): every process — the update doors and the ordinary
/// start alike — does it first thing in `main`, before it spawns anything, so
/// no pane, shell or later process inherits a mark meant for this one. A road
/// process started from here gets a mark of its own: an exit guard's start
/// names its maker, and an ordinary start that hands itself to the recovery
/// build passes on the mark it was started with
/// (`update_startup`'s `Machine::spawn_detached`).
pub(crate) fn take_predecessor() -> Option<Running> {
    *PREDECESSOR.get_or_init(|| {
        predecessor_named(install_flip::take_environment_variable(PREDECESSOR_VARIABLE).as_deref())
    })
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
    /// Start `program` with `words`, detached, marked with this process as
    /// its predecessor ([`PREDECESSOR_VARIABLE`]) where the start can carry
    /// an environment.
    ///
    /// # Errors
    /// It could not be started.
    fn start(&mut self, program: &Path, words: &[OsString]) -> io::Result<()>;
    /// **What to start when [`Leave::opening`]'s program would not start**:
    /// the next program the disk rule names — on Windows the rescue copy with
    /// `--update-failed`, O's own image, known to run — or `None`.
    fn fallback(&mut self) -> Option<(PathBuf, Vec<OsString>)> {
        None
    }
    /// **The build that handed this update over, still before its own exit
    /// guard** — an applier's only ([`the_old_build_still_leaves`]): its pid,
    /// or `None`.
    fn predecessor_opens(&mut self) -> Option<u32> {
        None
    }
}

/// **Whether the build that handed this update over is still on its way out,
/// before its own exit guard** (U-34): the applier's predecessor — the mark O
/// started it with — still running by pid and start instant, from the
/// installed program `installed` (so it is O, not a rescue-image process), the
/// journal still `Handoff`, and O's data-directory claim at `data` still held
/// (O lets go of it immediately before its guard). Then O's guard is still to
/// come and will find this applier gone, and an applier that leaves before its
/// road (a refusal, a panic) starts nothing itself: one start, not two. Its
/// pid, or `None`.
///
/// **The one accepted race** (design revision (e)): between O letting go of
/// the claim and O looking at its applier there are two statements; an applier
/// that looks in that instant, or leaves after O looked, may start a second
/// Folio beside O's — harmless, the second start finds the first one's claim
/// and hands its launch to it.
pub(crate) fn the_old_build_still_leaves(
    predecessor: Option<Running>,
    installed: &Path,
    home: &Home,
    data: &Path,
) -> Option<u32> {
    let old = predecessor.filter(|old| install_flip::still_running(*old))?;
    if !install_flip::running_from(installed).ok()?.contains(&old) {
        return None;
    }
    let bytes = file_reads::read(Lane::UpdateJournal, home.journal()).ok()?;
    if Journal::parse(&bytes).ok()?.body.phase.kind() != PhaseKind::Handoff {
        return None;
    }
    match crate::persist::try_claim(data) {
        Err(bt_platform::instance::ClaimRefusal::Held) => Some(old.pid),
        _ => None,
    }
}

/// **How a road process left** — what its [`ExitGuard`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Left {
    /// A successor this process started, or found, still runs — pid and start
    /// instant — and opens Folio: nothing was started.
    Succeeded(u32),
    /// The build that handed this update over is still on its way out, before
    /// its own exit guard, which starts Folio: nothing was started here.
    PredecessorOpens(u32),
    /// The recovery run at logon, with nothing done and nobody waiting (W8):
    /// nothing was started.
    NobodyWaiting,
    /// Nothing could be named to start: no home to read.
    Nameless,
    /// This program was started.
    Started(PathBuf),
    /// This program could not be started, for this reason.
    NotStarted(PathBuf, String),
}

impl Left {
    /// The words the door's one line ends with.
    pub(crate) fn said(&self) -> String {
        match self {
            Left::Succeeded(pid) => format!("{pid} runs and opens Folio; nothing else was started"),
            Left::PredecessorOpens(pid) => format!(
                "{pid}, the build that handed the update over, opens Folio as it leaves; nothing was started"
            ),
            Left::NobodyWaiting => String::from("nobody is waiting; nothing was started"),
            Left::Nameless => String::from("nothing could be named to start"),
            Left::Started(program) => format!("started {}", program.display()),
            Left::NotStarted(program, error) => {
                format!("{} could not be started: {error}", program.display())
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
/// where neither whole set is installed), marked with this process as its
/// predecessor — and, when that program will not start, the next one the rule
/// names ([`Leave::fallback`]) before it gives up with one line. **One exception**: the recovery run at logon that did nothing a
/// person is owed a window for — nobody is waiting ([`ExitGuard::nobody_waiting`]).
///
/// It replaces the per-road answers U-29b's rules had spread over each end
/// (`opens_after`'s table of who owes what, the refusals that left silently):
/// what a road decides now is only whom it leaves behind.
pub(crate) struct ExitGuard<L: Leave> {
    leave: L,
    successor: Option<Running>,
    waiting: bool,
    left: Option<Left>,
}

impl<L: Leave> ExitGuard<L> {
    /// A guard over `leave`, owed a window until told otherwise.
    pub(crate) fn new(leave: L) -> Self {
        Self {
            leave,
            successor: None,
            waiting: true,
            left: None,
        }
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

    /// **Leave now**: the start the exit owes, once — a second call answers
    /// the first one's result, and the drop then does nothing.
    pub(crate) fn leave(&mut self) -> Left {
        if let Some(left) = &self.left {
            return left.clone();
        }
        let left = match self
            .successor
            .filter(|successor| install_flip::still_running(*successor))
        {
            Some(successor) => Left::Succeeded(successor.pid),
            None if !self.waiting => Left::NobodyWaiting,
            None => match self.leave.predecessor_opens() {
                Some(old) => Left::PredecessorOpens(old),
                None => self.start(),
            },
        };
        self.left = Some(left.clone());
        left
    }

    /// The start the disk names, or its fallback.
    fn start(&mut self) -> Left {
        match self.leave.opening() {
            None => Left::Nameless,
            Some((program, words)) => match self.leave.start(&program, &words) {
                Ok(()) => Left::Started(program),
                Err(error) => match self.leave.fallback() {
                    Some((next, words)) if next != program => match self.leave.start(&next, &words)
                    {
                        Ok(()) => Left::Started(next),
                        Err(again) => Left::NotStarted(
                            next,
                            format!("{again}, after {}: {error}", program.display()),
                        ),
                    },
                    _ => Left::NotStarted(program, error.to_string()),
                },
            },
        }
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

/// **Whether a recovery at logon that ended `ended` did something a person is
/// owed a window for** — the Restart before it ended in a revert or a
/// finished rollback (W11); every other end at logon leaves nobody waiting.
pub(crate) fn owed_at_logon(ended: &Ended) -> bool {
    matches!(
        ended,
        Ended::Reverted | Ended::RolledBack | Ended::RolledBackWithDebt(_)
    )
}
