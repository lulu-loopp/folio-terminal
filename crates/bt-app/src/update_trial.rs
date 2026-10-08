//! **An update's trial writes nothing durable until its transaction is
//! committed, and says it is ready through a receipt the storage worker writes**
//! (`docs/plans/design/self-update-2026-09-16.md` revision (b): F-7, F-14 and
//! §(b).2's health paragraph; 0.4.6 ticket U-13).
//!
//! # The gate (F-7)
//!
//! A start the journal confirmed as a transaction's trial
//! ([`update_startup::trial`], U-12) is N: the new build, started by the
//! applier with a per-trial nonce, running on O's data while the transaction
//! can still roll back. **Everything durable a start would write is held back**
//! — the data folder and its documents, the integration marks, the Explorer and
//! toast registrations, the PSReadLine upgrade and `update-check.json` — so a
//! rollback leaves O's data exactly as O left it. Each writer asks one
//! predicate, [`defer`] (or [`writes_are_deferred`] where the answer only
//! chooses how to read), and the answer is `false` in every start that is not
//! a trial: there the gate is inert and nothing changes. [`Writer`] is the
//! inventory: one variant per writer, each naming its site.
//!
//! A deferred write is **not lost**. [`defer`] records its writer as pending in
//! memory; when the trial's watch reads `Committed` in the journal, the pending
//! writers are **released** — the window thread is woken and runs each of them
//! again ([`take_released`], the application's `release_trial_writes`). What
//! they write then is what the process holds then: the document as it stands,
//! the migration as it would run, not a replay of each held-back call.
//!
//! **The watch** is a worker of its own (`folio-trial-watch`, below normal):
//! one read of `H\journal.json` every [`WATCH_INTERVAL`] through `file_reads`
//! on [`Lane::UpdateJournal`], never a write, until the transaction is decided
//! ([`update_txn::trial_sight`]), from the frozen header alone: `outcome`
//! `committed` releases; a `terminal` class, the journal gone or naming
//! another transaction **drops** the
//! pending writes, and the gate stays shut for the rest of the process: the
//! process runs on, writing nothing, until the applier ends it (F-7's live-child
//! policy; nothing here ends a process). `diagnostics.log` is exempt (F-7), and
//! so are the other diagnostics a run writes about itself — a hang report, the
//! panic log — which are append-only accounts, not state a later run reads.
//!
//! # The watchdog (0.4.7 ticket U-37, design revision (h) H.2 and H.4)
//!
//! A trial whose transaction is still undecided [`WATCHDOG`] (102 s) after its
//! watch began — longer than any lock holder alive takes to decide a trial it
//! watches — is watched by nobody: its applier could not record it or died,
//! or an exit guard started it with a nonce no journal records. It then hands
//! its transaction back — at 102, 204, 408 and 816 s, [`HAND_BACKS`] times at
//! most, never while the recovery it started before still runs: the recovery
//! build is started from the watch worker with the entrance's own line and
//! `--from-trial <pid>:<started>:<ready|unready>` ([`hand_back`]), only when
//! that rescue build is 0.4.7 or later. The recovery records this trial if its
//! receipt names it exactly and commits, ends it if it never became ready, and
//! otherwise defers. **The trial never ends itself.** An unreadable journal is
//! never handed back. A receipt the storage worker could not write is written
//! again by the watch, with a growing pause, until it lands.
//!
//! # Readiness and the receipt (F-14, (b).2)
//!
//! Only N gives evidence of health, and its evidence is a file,
//! `H\<txn>\health-<nonce>` = [`Receipt`] `{v, txn, nonce, pid, version}`. N is
//! ready once it holds the data directory's claim — taken through `try_claim`
//! and adopted into the claim table before anything asks who writes there
//! ([`take_the_claim`], §C.7) — **and** its first pane text has reached the
//! glass (the window's existing first-text edge). The window thread then asks
//! [`receipt_due`] once and hands the receipt to the **storage worker**, which
//! writes it create-new with `install_txn::durable_create` (flush file, rename
//! that never replaces, flush directory). The window thread never writes it.
//!
//! **The journal has one writer, the lock holder** — with one exception. N
//! writes only its receipt, except U-35's reserved trial (Windows; started
//! because the operating system refused the rescue copy every holder runs):
//! once ready, its watch takes the transaction lock and records `Committed`
//! from its own receipt as it is on disk, which must name this very process
//! (`update_apply::commit_last_trial`). A receipt that lands after the
//! journal says `RollbackIntent` is ignored **by rule** — the lock holder's rule
//! (`update_txn::next` refuses it; U-18/U-21 hold it), not this module's. A
//! rollback that is still `destructive` is therefore not an end here: its
//! trial holds its writes, and the one trial whose receipt can still count —
//! the one a lock holder starts over a `Stuck` transaction whose new bundle is
//! live (U-29b) — writes it and is released if that commits forward.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use bt_platform::file_reads::{self, Lane};
use bt_platform::install_flip::Running;

use crate::persist;
use crate::update_apply::LastTrialCommit;
use crate::update_startup;
use crate::update_txn::{Home, Receipt, TrialSight, TxnId, trial_sight};

/// **Every durable writer a start makes, each held back by the gate while the
/// start is a trial** — the inventory, one variant per writer and its site.
///
/// The order is the order a release runs them in: the folder before anything
/// written into it, the copies of refused documents before the documents that
/// would replace them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Writer {
    /// The data folder's one-time move from its old name
    /// (`persist::storage_dir` → `relocate`). A trial keeps using the old
    /// folder, as a start whose move failed does; the move is left to the next
    /// start, because the folder is in use by then.
    DataFolderMove,
    /// The data folder itself, made by the stores that open in it
    /// (`SessionStore::open`, `SettingsStore::open`, `KeybindingsStore::open`,
    /// `ProfilesStore::open`, `PinsStore::open`).
    DataFolder,
    /// The copy of a document the read refused, kept beside it
    /// (`bt_persist`'s keeping, read with [`bt_persist::Keeping::Owed`]).
    RefusedCopies,
    /// `session.json` (`SessionStore::hand_over`) and the crash sentinel
    /// `session.lock` (`SessionStore::open`).
    Session,
    /// `settings.json` (`SettingsStore::write_now`), including the
    /// carry-forward `Runtime::create` makes.
    Settings,
    /// `keybindings.json` (`KeybindingsStore::store`).
    Keybindings,
    /// `profiles.json` (`ProfilesStore::store`).
    Profiles,
    /// `pins.json` (`PinsStore::store`).
    Pins,
    /// `update-check.json` and its claim file: the check (`update::begin`) and
    /// every change to the file (`OfferState::transact`).
    UpdateCheck,
    /// The integration marks: the startup migration of the PowerShell profiles
    /// and the marks record it keeps
    /// (`shell_integration::begin_startup_migration`).
    ProfileMigration,
    /// The bash integration script a starting bash sources, written into the
    /// data folder (`shell_integration::script_path`).
    BashScript,
    /// The zsh integration directory a starting zsh is pointed at
    /// (`shell_integration::zdotdir_path`).
    ZshScripts,
    /// The PowerShell integration script a starting PowerShell loads, in the
    /// data folder (`shell_integration::powershell_script_for_birth`). Held
    /// back, a trial names the durable copy only when it already holds this
    /// build's bytes and otherwise writes its own copy under the system
    /// temporary directory; the release repairs the durable copy on a worker.
    PowerShellScript,
    /// The launch's replacement of Folio's own older PSReadLine and its stamp
    /// (`psreadline::upgrade_recorded`, ticket 56).
    PsReadLineUpgrade,
    /// The launch probe's repair and renewal of the Explorer package
    /// registration (`explorer_menu::begin_probe`; the gate is asked in
    /// `explorer_menu::probe_at_start`, U-25).
    ExplorerRepair,
    /// The toast sender's identity in the registry (`NotificationDesk::show` →
    /// `bt_platform::Notifier::register_identity`).
    ToastIdentity,
}

impl Writer {
    /// Every writer, in release order.
    #[cfg(test)]
    pub(crate) const ALL: [Writer; 16] = [
        Writer::DataFolderMove,
        Writer::DataFolder,
        Writer::RefusedCopies,
        Writer::Session,
        Writer::Settings,
        Writer::Keybindings,
        Writer::Profiles,
        Writer::Pins,
        Writer::UpdateCheck,
        Writer::ProfileMigration,
        Writer::BashScript,
        Writer::ZshScripts,
        Writer::PowerShellScript,
        Writer::PsReadLineUpgrade,
        Writer::ExplorerRepair,
        Writer::ToastIdentity,
    ];
}

/// **How often the trial's watch reads the journal** while its transaction is
/// undecided. A quarter of a second against a trial deadline of ninety
/// (`update_txn::TRIAL_DEADLINE_MS`): a commit is seen at once, and the read is
/// one small file.
pub(crate) const WATCH_INTERVAL: Duration = Duration::from_millis(250);

/// **How long a trial asks for the data directory's claim** before it gives up
/// (§C.7): the old build is letting go of it as the trial starts.
pub(crate) const CLAIM_WAIT: Duration = Duration::from_secs(30);

/// How often a trial asks for the claim again inside [`CLAIM_WAIT`].
const CLAIM_RETRY: Duration = Duration::from_millis(100);

/// How the trial's transaction was decided, once it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decided {
    /// `Committed` was read: the pending writes are released.
    Committed,
    /// Decided otherwise, or gone: the pending writes are dropped and the gate
    /// stays shut.
    Ended,
}

/// A document whose refused bytes are owed a copy, and the read that keeps it.
type OwedCopy = (PathBuf, fn(&Path));

/// **The gate's state**, one per process ([`GATE`]); a test builds its own.
pub(crate) struct Gate {
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    decided: Option<Decided>,
    /// The writers held back so far, released or dropped together.
    pending: BTreeSet<Writer>,
    /// The documents whose refused bytes are owed a copy, each with the read
    /// that keeps it ([`Writer::RefusedCopies`]).
    owed_copies: Vec<OwedCopy>,
    /// What a commit released of them.
    released_copies: Vec<OwedCopy>,
    /// What a commit released and the window thread has not yet run.
    released: Vec<Writer>,
    /// The trial's claim was adopted into the claim table (§C.7).
    claim_adopted: bool,
    /// The receipt has been handed to the storage worker.
    receipt_handed: bool,
    /// Where the storage worker answers for the receipt; the watch reads it
    /// and says in the log what became of it.
    receipt_answer: Option<mpsc::Receiver<persist::ReceiptWritten>>,
    /// **The receipt as it was handed over** (U-37): what the watch writes
    /// again when the storage worker's write was refused.
    receipt_job: Option<ReceiptJob>,
    /// The storage worker's write was refused and the watch has not yet
    /// written it ([`Gate::write_owed_receipt`]).
    receipt_owed: bool,
}

impl Gate {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(GateState {
                decided: None,
                pending: BTreeSet::new(),
                owed_copies: Vec::new(),
                released_copies: Vec::new(),
                released: Vec::new(),
                claim_adopted: false,
                receipt_handed: false,
                receipt_answer: None,
                receipt_job: None,
                receipt_owed: false,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        // Nothing panics while holding it; a poisoned gate would still hold
        // the right answer, so it is read through rather than propagated.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether writes are held back, for a process that is (`trial`) or is not
    /// a trial.
    pub(crate) fn defers(&self, trial: bool) -> bool {
        trial && self.state().decided != Some(Decided::Committed)
    }

    /// **The one question every writer asks**: `true` — do not write, the
    /// writer is recorded as pending — or `false`, write as always.
    pub(crate) fn defer(&self, trial: bool, writer: Writer) -> bool {
        if !trial {
            return false;
        }
        let mut state = self.state();
        match state.decided {
            Some(Decided::Committed) => false,
            Some(Decided::Ended) => true,
            None => {
                state.pending.insert(writer);
                true
            }
        }
    }

    /// Record what the watch read. `true` when this decided the transaction as
    /// committed, so the caller wakes the window thread.
    fn decide(&self, sight: TrialSight) -> bool {
        let mut state = self.state();
        if state.decided.is_some() {
            return false;
        }
        match sight {
            TrialSight::Undecided => false,
            TrialSight::Committed => {
                state.decided = Some(Decided::Committed);
                let released: Vec<Writer> =
                    std::mem::take(&mut state.pending).into_iter().collect();
                state.released.extend(released);
                let copies = std::mem::take(&mut state.owed_copies);
                state.released_copies.extend(copies);
                true
            }
            TrialSight::Ended => {
                state.decided = Some(Decided::Ended);
                state.pending.clear();
                state.owed_copies.clear();
                false
            }
        }
    }

    /// The writers a commit released, in release order, once.
    pub(crate) fn take_released(&self) -> Vec<Writer> {
        let mut released = std::mem::take(&mut self.state().released);
        released.sort_unstable();
        released.dedup();
        released
    }

    /// A read refused `path` and left its copy owed; `keep` reads it again with
    /// the copy kept. `true` when it was recorded (a trial, undecided).
    fn owe_copy(&self, trial: bool, path: &Path, keep: fn(&Path)) -> bool {
        if !self.defer(trial, Writer::RefusedCopies) {
            return false;
        }
        let mut state = self.state();
        if state.decided.is_none() && !state.owed_copies.iter().any(|(owed, _)| owed == path) {
            state.owed_copies.push((path.to_path_buf(), keep));
        }
        true
    }

    /// The owed copies a commit released, once.
    fn take_released_copies(&self) -> Vec<OwedCopy> {
        std::mem::take(&mut self.state().released_copies)
    }

    /// The writers held back and not yet decided.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Vec<Writer> {
        self.state().pending.iter().copied().collect()
    }

    /// What the storage worker answered for the receipt, once it has.
    fn receipt_answered(&self) -> Option<persist::ReceiptWritten> {
        let mut state = self.state();
        let written = state.receipt_answer.as_ref()?.try_recv().ok()?;
        state.receipt_answer = None;
        Some(written)
    }

    fn adopt_claim(&self) {
        self.state().claim_adopted = true;
    }

    /// Where the storage worker answers for the receipt it was handed.
    fn await_receipt(&self, answer: mpsc::Receiver<persist::ReceiptWritten>) {
        self.state().receipt_answer = Some(answer);
    }

    /// **Whether this trial became ready** — its receipt fell due: the claim
    /// adopted and its first pane text on the glass (U-37).
    fn ready(&self) -> bool {
        self.state().receipt_handed
    }

    /// **Ready, for a test's trial in a process of its own**: its claim
    /// adopted and its receipt fallen due, as a trial's first pane text makes
    /// it.
    #[cfg(test)]
    pub(crate) fn ready_for_a_test(&self) {
        self.adopt_claim();
        let _ = self.hand_receipt();
    }

    /// Keep the receipt as handed, for a write the watch owes again.
    fn keep_receipt(&self, job: ReceiptJob) {
        self.state().receipt_job = Some(job);
    }

    /// The storage worker's write of the receipt was refused: the watch owes
    /// it.
    fn owe_receipt(&self) {
        self.state().receipt_owed = true;
    }

    /// **The receipt written again when a write of it was refused** (U-37): a
    /// home that refused it — made read-only, a scanner's handle — may take it
    /// a moment later, and a receipt that never lands is a healthy trial nobody
    /// can commit. `None` when nothing is owed; written by the one rule for
    /// what is already at the receipt's name ([`write_receipt`]).
    fn write_owed_receipt(&self) -> Option<Result<(), String>> {
        let job = {
            let state = self.state();
            if !state.receipt_owed {
                return None;
            }
            state.receipt_job.clone()?
        };
        let written = write_receipt(&job);
        if written.is_ok() {
            self.state().receipt_owed = false;
        }
        Some(written)
    }

    /// `true` once: the claim is adopted, the transaction undecided, and the
    /// receipt not yet handed over.
    fn hand_receipt(&self) -> bool {
        let mut state = self.state();
        let due = state.claim_adopted && state.decided.is_none() && !state.receipt_handed;
        if due {
            state.receipt_handed = true;
        }
        due
    }
}

/// This process's gate.
static GATE: Gate = Gate::new();

/// Whether this process is a trial (U-12's fact).
fn is_trial() -> bool {
    update_startup::trial().is_some()
}

/// **Whether this process's durable writes are held back now**: it is a trial
/// whose transaction has not been read as committed. Always `false` outside a
/// trial.
pub(crate) fn writes_are_deferred() -> bool {
    GATE.defers(is_trial())
}

/// **Ask before a durable write**: `true` means do not write — the writer is
/// recorded as pending and runs again when the trial is committed. Always
/// `false` outside a trial, which is every start but an update's trial.
pub(crate) fn defer(writer: Writer) -> bool {
    GATE.defer(is_trial(), writer)
}

/// How a document is read in this process: a refused file's copy is owed while
/// writes are held back ([`Writer::RefusedCopies`]).
pub(crate) fn keeping() -> bt_persist::Keeping {
    if writes_are_deferred() {
        bt_persist::Keeping::Owed
    } else {
        bt_persist::Keeping::Now
    }
}

/// A read with [`keeping`] refused the document at `path` and left its copy
/// owed: record it, with `keep` — the same read with the copy kept — so a
/// commit reads the document again and keeps it before anything replaces it.
pub(crate) fn owe_copy(report: &bt_persist::ReadReport, path: &Path, keep: fn(&Path)) {
    if report.owes_a_copy() {
        GATE.owe_copy(is_trial(), path, keep);
    }
}

/// **Keep the copies a commit released** — [`Writer::RefusedCopies`]'s
/// release, run before any document that would replace them is written.
pub(crate) fn keep_owed_copies() {
    for (path, keep) in GATE.take_released_copies() {
        keep(&path);
    }
}

/// **The writers a commit released**, for the window thread to run, once.
pub(crate) fn take_released() -> Vec<Writer> {
    GATE.take_released()
}

// ───────────────────────────────── the watch ─────────────────────────────────

/// **Start the trial's watch**, when this process is a trial: a worker reads
/// the journal every [`WATCH_INTERVAL`] until the transaction is decided, and
/// calls `wake` when a commit released the pending writes. Nothing at all
/// outside a trial.
pub(crate) fn begin_watch(wake: impl Fn() + Send + 'static) {
    let (Some((txn, nonce)), Some(home)) = (update_startup::trial(), update_startup::trial_home())
    else {
        return;
    };
    let journal = home.journal();
    let last = update_startup::is_last_trial();
    let started = bt_platform::spawn_at_priority(
        "folio-trial-watch",
        bt_platform::ThreadPriority::BelowNormal,
        move |ctx| {
            let mut commit = || crate::update_apply::commit_last_trial(ctx, home, txn, nonce);
            let mut hand_back =
                |ready: bool| hand_back(ctx, home, txn, ready, FROM_TRIAL_SINCE, &mut Detached);
            let mut watchdog = Watchdog {
                every: WATCHDOG,
                hand_back: &mut hand_back,
            };
            let own_commit: Option<&mut CommitItself<'_>> =
                if last { Some(&mut commit) } else { None };
            watch(
                &GATE,
                &journal,
                txn,
                WATCH_INTERVAL,
                &wake,
                own_commit,
                &mut watchdog,
            );
        },
    );
    if let Err(error) = started {
        // No watch, so nothing will ever be released: the run writes nothing,
        // which is what a trial that is never committed does anyway.
        eprintln!(
            "BT_UPDATE_TRIAL no watch on {}: {error}",
            home.journal().display()
        );
    }
}

/// **How long a trial's transaction may stay undecided before the trial hands
/// it back** (0.4.7 ticket U-37) — and again each time as long, while it stays
/// so. Derived from the road's own budgets: a lock holder that watches a trial
/// decides it by its deadline (`update_txn::TRIAL_DEADLINE_MS`, 90 s from the
/// launch), and has stopped a trial that gave no receipt within the stop's
/// two graces (5 s to quit, 5 s to end, `update_apply::Limits::PRODUCT`), its
/// rollback's declaration asked again for at most
/// `update_apply::JOURNAL_WRITE_WITHIN` (2 s) before that. A trial still
/// undecided past their sum — 102 s — is watched by nobody alive: its applier
/// could not record it or died, or it was started by an exit guard with a
/// nonce no journal records (`update_apply::Opens::Trial`). The receipt
/// normally lands within seconds of the launch, so the watchdog is never the
/// way a healthy update ends.
pub(crate) const WATCHDOG: Duration = Duration::from_millis(crate::update_txn::TRIAL_DEADLINE_MS)
    .saturating_add(crate::update_apply::Limits::PRODUCT.quit_within)
    .saturating_add(crate::update_apply::Limits::PRODUCT.end_within)
    .saturating_add(crate::update_apply::JOURNAL_WRITE_WITHIN);

/// **How many times a trial hands its transaction back** (design revision
/// (h) H.2): at `every`, 2 × `every`, 4 × `every` and 8 × `every` after its
/// watch began — 102 s, 204 s, 408 s and 816 s in the product — and never
/// again; the next start or logon decides after that.
pub(crate) const HAND_BACKS: u32 = 4;

/// **The first pause before a refused receipt is written again**, doubled at
/// each refusal up to [`RECEIPT_RETRY_CAP`] (H.2).
pub(crate) const RECEIPT_RETRY_FIRST: Duration = Duration::from_millis(250);

/// **The longest pause between two writes of a refused receipt** (H.2): about
/// 127 attempts in an hour of a home that stays read-only.
pub(crate) const RECEIPT_RETRY_CAP: Duration = Duration::from_secs(30);

/// **The trial's watchdog** (U-37, H.2): at each due time of an undecided
/// transaction, `hand_back` is called with whether this trial became ready
/// (its receipt fell due) and answers the recovery it started, if any, by pid
/// and start instant — in the product, [`hand_back`].
pub(crate) struct Watchdog<'a> {
    pub(crate) every: Duration,
    pub(crate) hand_back: &'a mut dyn FnMut(bool) -> Option<Running>,
}

/// A watchdog that does not fire within any test.
#[cfg(test)]
pub(crate) fn watchdog_asleep() -> Watchdog<'static> {
    Watchdog {
        every: Duration::from_secs(3600),
        hand_back: Box::leak(Box::new(|_: bool| None)),
    }
}

/// **U-35's reserved trial asking to commit itself** — in the product
/// `update_apply::commit_last_trial`; a test answers what it is told.
pub(crate) type CommitItself<'a> =
    dyn FnMut() -> Result<crate::update_apply::LastTrialCommit, String> + 'a;

/// **What one read of the journal told the watch** (H.2 step 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Read {
    /// The header, or no journal at all.
    Seen(TrialSight),
    /// Any other read error: not an answer about the transaction.
    Unreadable,
}

/// **The watch's loop** (design revision (h) H.2), each turn in this order,
/// until the transaction is decided: (1) the receipt — the storage worker's
/// answer taken, a refused receipt written again, create-new, after a pause
/// from [`RECEIPT_RETRY_FIRST`] doubling to [`RECEIPT_RETRY_CAP`]; (2) the
/// journal read — a header decides, `NotFound` is the end, **any other error
/// is `Unreadable`** and skips neither step 1 nor step 3; (3) the watchdog —
/// at each of its [`HAND_BACKS`] due times, an `Undecided` transaction is
/// handed back, single-flight (not while the recovery an earlier hand-back
/// started still runs, by pid and start instant), and an `Unreadable` one is
/// not (no holder could read it either); (4) the pause. Read-only on the
/// journal — its writer is the lock holder — **except for U-35's reserved
/// trial** (`commit_itself`, U-35 round 2): once this trial is ready (its
/// receipt fell due), between steps 1 and 2 it asks to commit itself on that
/// receipt, each turn until it is `Committed` (released at once), or the
/// journal no longer waits for it (it never asks again). It is the one trial
/// no holder can adopt — every holder runs the rescue copy the operating
/// system refused — so without it a healthy trial would keep nothing.
pub(crate) fn watch(
    gate: &Gate,
    journal: &Path,
    txn: TxnId,
    interval: Duration,
    wake: &dyn Fn(),
    mut commit_itself: Option<&mut CommitItself<'_>>,
    watchdog: &mut Watchdog<'_>,
) {
    let begun = Instant::now();
    let mut spent: u32 = 0;
    let mut recovery: Option<Running> = None;
    let mut unreadable_since: Option<Instant> = None;
    let (mut retry_at, mut pause) = (Instant::now(), RECEIPT_RETRY_FIRST);
    let mut said_commit_refusal = false;
    loop {
        // (1) The receipt.
        if let Some(written) = gate.receipt_answered() {
            match written.result {
                Ok(()) => eprintln!(
                    "BT_UPDATE_TRIAL receipt of {txn} written by {:?}",
                    written.by
                ),
                Err(error) => {
                    eprintln!(
                        "BT_UPDATE_TRIAL receipt of {txn} not written by {:?}: {error}; the watch writes it again",
                        written.by
                    );
                    gate.owe_receipt();
                    retry_at = Instant::now() + pause;
                }
            }
        }
        if Instant::now() >= retry_at {
            match gate.write_owed_receipt() {
                Some(Ok(())) => {
                    eprintln!("BT_UPDATE_TRIAL receipt of {txn} written by the trial's watch");
                }
                Some(Err(_)) => {
                    pause = pause.saturating_mul(2).min(RECEIPT_RETRY_CAP);
                    retry_at = Instant::now() + pause;
                }
                None => {}
            }
        }
        // U-35's reserved trial, once ready: the ordinary readiness edge, and
        // what commits is the receipt as it is on disk, under the lock.
        if gate.ready()
            && let Some(commit) = commit_itself.as_deref_mut()
        {
            match commit() {
                Ok(LastTrialCommit::Committed) => {
                    if gate.decide(TrialSight::Committed) {
                        eprintln!(
                            "BT_UPDATE_TRIAL transaction {txn} is committed by its own receipt; its writes are released"
                        );
                        wake();
                    }
                    return;
                }
                Ok(LastTrialCommit::Pending) => {}
                Ok(LastTrialCommit::NotItsOwn) => commit_itself = None,
                Ok(LastTrialCommit::Unprovable) => {
                    eprintln!(
                        "BT_UPDATE_TRIAL transaction {txn}: this process cannot read its own start instant, so its receipt names nobody; it is not committed by itself, and a recovery decides"
                    );
                    commit_itself = None;
                }
                Err(error) => {
                    if !said_commit_refusal {
                        said_commit_refusal = true;
                        eprintln!(
                            "BT_UPDATE_TRIAL transaction {txn} could not be committed by its own receipt: {error}"
                        );
                    }
                }
            }
        }
        // (2) The journal.
        let read = match file_reads::read(Lane::UpdateJournal, journal) {
            Ok(bytes) => Read::Seen(trial_sight(Some(&bytes), &txn)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Read::Seen(trial_sight(None, &txn))
            }
            // Held by its writer for the instant of a rename, or a share that
            // stopped answering: not an answer about the transaction.
            Err(_) => Read::Unreadable,
        };
        match read {
            Read::Seen(sight @ TrialSight::Committed) => {
                if gate.decide(sight) {
                    eprintln!(
                        "BT_UPDATE_TRIAL transaction {txn} is committed; its writes are released"
                    );
                    wake();
                }
                return;
            }
            Read::Seen(sight @ TrialSight::Ended) => {
                gate.decide(sight);
                eprintln!(
                    "BT_UPDATE_TRIAL transaction {txn} ended without a commit; this run writes nothing"
                );
                return;
            }
            Read::Seen(TrialSight::Undecided) => unreadable_since = None,
            Read::Unreadable => {
                unreadable_since.get_or_insert_with(Instant::now);
            }
        }
        // (3) The watchdog.
        if spent < HAND_BACKS && Instant::now() >= begun + watchdog.every * (1 << spent) {
            spent += 1;
            if let Some(since) = unreadable_since {
                eprintln!(
                    "BT_UPDATE_TRIAL transaction {txn}: the journal has been unreadable for {} s; it is not handed back",
                    since.elapsed().as_secs()
                );
            } else if recovery.is_some_and(bt_platform::install_flip::still_running) {
                eprintln!(
                    "BT_UPDATE_TRIAL transaction {txn} is undecided; the recovery handed it before still runs"
                );
            } else {
                recovery = (watchdog.hand_back)(gate.ready());
            }
            if spent == HAND_BACKS {
                eprintln!(
                    "BT_UPDATE_TRIAL transaction {txn}: the watchdog is spent; the next start or logon decides"
                );
            }
        }
        // (4) The pause.
        std::thread::sleep(interval);
    }
}

// ───────────────────────────── the claim and the receipt ─────────────────────────────

/// **A trial takes the data directory's claim before anything asks who writes
/// there** (§C.7): `persist::try_claim` every [`CLAIM_RETRY`] for up to
/// [`CLAIM_WAIT`] — the old build is letting go of it — then
/// `persist::adopt_claim`, so `is_writer_of` answers "this process" from the
/// first time it is asked. `Ok(())` at once outside a trial.
///
/// # Errors
/// The one line to say when the claim was not had inside the wait. §C.7: such
/// a trial does not start anyway and does not hand itself to the holder — the
/// caller leaves with a failure, and the applier, which sees its trial gone
/// without a receipt, rolls back.
pub(crate) fn take_the_claim(storage: &Path) -> Result<(), String> {
    if !is_trial() {
        return Ok(());
    }
    take_the_claim_within(&GATE, storage, CLAIM_WAIT)
}

pub(crate) fn take_the_claim_within(
    gate: &Gate,
    storage: &Path,
    wait: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + wait;
    loop {
        match persist::try_claim(storage) {
            Ok(claim) => {
                persist::adopt_claim(storage, claim);
                gate.adopt_claim();
                return Ok(());
            }
            Err(refusal) => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "BT_UPDATE_TRIAL {} was not free within {} s ({refusal:?}); this trial does not start",
                        storage.display(),
                        wait.as_secs()
                    ));
                }
                std::thread::sleep(CLAIM_RETRY);
            }
        }
    }
}

/// **The trial's receipt, to be written by the storage worker**: where, and
/// the bytes (v1, frozen).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReceiptJob {
    pub(crate) path: PathBuf,
    pub(crate) bytes: Vec<u8>,
}

/// **What a receipt already at a trial's own name is to that trial** (U-35
/// round 3) — the one meaning the writer ([`write_receipt`]), U-35's
/// self-commit (`update_apply::commit_last_trial`) and, by the same test, a
/// recovery's survey (`update_apply::survey`, which adopts only a receipt
/// naming a running process exactly) all read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtItsName {
    /// This very receipt: the same bytes are already there.
    Its,
    /// **An earlier attempt of the same trial**: a receipt of the same
    /// transaction and nonce that names no running process — the process it
    /// names has ended, or its pid now belongs to a process started at
    /// another instant (pid and start instant together, so a reused pid is
    /// never mistaken for it), or it names no start instant and so names
    /// nobody. It is evidence about a process that cannot answer for this
    /// one, and only the trial with that nonce writes there, so this trial
    /// replaces it.
    Earlier,
    /// A receipt naming a running process, a receipt of another transaction
    /// or nonce, or bytes that are no receipt: never written over.
    Kept,
}

/// [`AtItsName`] for `there` — what was read at the name of `mine`'s receipt.
pub(crate) fn at_its_name(there: &Receipt, mine: &Receipt) -> AtItsName {
    if there == mine {
        return AtItsName::Its;
    }
    if there.txn == mine.txn && there.nonce == mine.nonce && !names_a_running_process(there) {
        AtItsName::Earlier
    } else {
        AtItsName::Kept
    }
}

/// **Whether `receipt` names a process that runs now** — by its pid and its
/// start instant together; a receipt without a start instant names nobody.
pub(crate) fn names_a_running_process(receipt: &Receipt) -> bool {
    receipt.started.is_some_and(|started| {
        bt_platform::install_flip::still_running(Running {
            pid: receipt.pid,
            started,
        })
    })
}

/// **Write a trial's receipt** — create-new, through
/// `install_txn::durable_create`; where the name is taken, by [`AtItsName`]:
/// the same receipt is already written; **an earlier attempt of the same trial
/// is replaced whole** by `install_txn::durable_write` (a temporary, a flush,
/// one rename, a flush of the folder), so a ready trial that died before its
/// transaction was decided never stops the next attempt of that trial from
/// proving itself (U-35 round 3, the review's F1); anything else is refused
/// and left byte for byte. Two attempts of one trial can never both be ready:
/// each must first hold the data directory's claim (§C.7).
///
/// # Errors
/// The write failed, or the name holds a receipt that is kept.
pub(crate) fn write_receipt(job: &ReceiptJob) -> Result<(), String> {
    let failure = match bt_platform::install_txn::durable_create(&job.path, &job.bytes) {
        Ok(()) => return Ok(()),
        Err(failure) if failure.error.kind() == io::ErrorKind::AlreadyExists => failure,
        Err(failure) => return Err(failure.to_string()),
    };
    let mine = Receipt::parse(&job.bytes).map_err(|refusal| refusal.to_string())?;
    let there = crate::update_apply::read_receipt(&job.path).and_then(Result::ok);
    match there.map(|there| at_its_name(&there, &mine)) {
        Some(AtItsName::Its) => Ok(()),
        Some(AtItsName::Earlier) => bt_platform::install_txn::durable_write(&job.path, &job.bytes)
            .map_err(|failure| failure.to_string()),
        Some(AtItsName::Kept) | None => Err(format!("{failure}; the receipt there is kept")),
    }
}

/// **The storage worker has the receipt**: its answer is read by the watch,
/// which says in the log what became of it.
pub(crate) fn receipt_handed(answer: mpsc::Receiver<persist::ReceiptWritten>) {
    GATE.await_receipt(answer);
}

/// **The trial is ready: its receipt, once** — asked by the window thread at
/// the first pane text it puts on the glass. `Some` exactly once in a trial
/// whose claim was adopted and whose transaction is still undecided; `None`
/// in every other start and on every later ask.
pub(crate) fn receipt_due() -> Option<ReceiptJob> {
    let (txn, nonce) = update_startup::trial()?;
    let home = update_startup::trial_home()?;
    GATE.hand_receipt().then(|| {
        let job = ReceiptJob {
            path: home.receipt_path(txn, &nonce),
            bytes: Receipt {
                txn,
                nonce,
                pid: std::process::id(),
                version: crate::version::VERSION.to_owned(),
                // Its own start instant (H.1): what binds the receipt to
                // this very process for a holder that did not start it.
                started: bt_platform::install_flip::started_of(std::process::id()),
            }
            .encode(),
        };
        GATE.keep_receipt(job.clone());
        job
    })
}

/// **The first version whose recovery build knows `--from-trial`** (design
/// revision (h) H.4, rule A2): a trial hands its transaction back only to a
/// rescue build of this version or later — for every update whose source build
/// is older (0.4.6 to any later version) the watchdog is inert.
pub(crate) const FROM_TRIAL_SINCE: (u16, u16, u16) = (0, 4, 7);

/// **How a hand-back starts the recovery build** — the product's starts it
/// detached through `quiet_command`; a test's records the line.
pub(crate) trait Starter {
    /// Start `program` with `line`, detached: the started process's pid.
    ///
    /// # Errors
    /// It could not be started.
    fn start(&mut self, program: &Path, line: &[std::ffi::OsString]) -> io::Result<u32>;
}

/// The product's [`Starter`]: `quiet_command`, the child dropped at once —
/// never waited on or ended.
pub(crate) struct Detached;

impl Starter for Detached {
    fn start(&mut self, program: &Path, line: &[std::ffi::OsString]) -> io::Result<u32> {
        bt_platform::quiet_command(program)
            .args(line)
            .spawn()
            .map(|child| child.id())
    }
}

/// **The rescue build's version**, as `(major, minor, patch)`: on Windows the
/// rescue executable's `VERSIONINFO` (`bt_platform::trust::file_version`), on
/// macOS the rescue clone's `CFBundleShortVersionString`
/// (`bt_platform::macos_update::short_version`, on this worker); `None` when it
/// cannot be read.
pub(crate) fn rescue_version(
    worker: &bt_platform::admission::WorkerCtx,
    home: &Home,
    rescue: &str,
) -> Option<(u16, u16, u16)> {
    if home.installed_bundle().is_some() {
        let version = bt_platform::macos_update::short_version(worker, Path::new(rescue)).ok()?;
        let mut parts = version
            .trim()
            .split('.')
            .map(|part| part.parse::<u16>().ok());
        Some((
            parts.next()??,
            parts.next()??,
            parts.next().unwrap_or(Some(0))?,
        ))
    } else {
        let bt_platform::trust::FileVersion([major, minor, patch, _]) =
            bt_platform::trust::file_version(Path::new(rescue)).ok()?;
        Some((major, minor, patch))
    }
}

/// **The trial's watchdog, in the product** (0.4.7 ticket U-37, design revision
/// (h) H.4): this trial's transaction is still undecided at a due time, so
/// nobody alive is deciding it — and it hands the transaction back to the
/// recovery build with the entrance's own line and this trial's exact identity
/// and state, `--from-trial <pid>:<started>:<ready|unready>`
/// (`cli::recover_line_at_logon`, started through `starter` from this worker,
/// never waited on) — **only to a rescue build of `since` or later** (rule A2;
/// the product's is [`FROM_TRIAL_SINCE`]). The recovery, once it holds the
/// transaction, records this trial when its receipt names it and commits; ends
/// it when it never became ready; and otherwise defers while it runs (H.3).
/// **This trial never ends itself** (rule A3): a recovery that dies in the
/// loader, cannot take the lock or refuses the line leaves it running as the
/// window, and the next due time tries again. It shows no card of its own.
/// Answers the recovery it started, by pid and start instant.
pub(crate) fn hand_back(
    worker: &bt_platform::admission::WorkerCtx,
    home: &Home,
    txn: TxnId,
    ready: bool,
    since: (u16, u16, u16),
    starter: &mut dyn Starter,
) -> Option<Running> {
    let Some(header) = file_reads::read(Lane::UpdateJournal, home.journal())
        .ok()
        .and_then(|bytes| crate::update_txn::Header::parse(&bytes).ok())
    else {
        // No journal to read: the watch reads the end on its next turn.
        return None;
    };
    let program = home.rescue_program(&header.rescue);
    match rescue_version(worker, home, &header.rescue) {
        Some(version) if version >= since => {}
        other => {
            eprintln!(
                "BT_UPDATE_TRIAL transaction {txn} is undecided; its rescue build ({other:?}) does not take a hand-back"
            );
            return None;
        }
    }
    let me = std::process::id();
    let process = Running {
        pid: me,
        started: bt_platform::install_flip::started_of(me)?,
    };
    let named = home.installed_bundle().map(|_| home.root());
    let mut line = crate::cli::recover_line_at_logon(named);
    line.push(std::ffi::OsString::from(crate::cli::FROM_TRIAL_FLAG));
    line.push(std::ffi::OsString::from(
        crate::cli::HandedBack { process, ready }.word(),
    ));
    match starter.start(&program, &line) {
        Ok(pid) => {
            eprintln!(
                "BT_UPDATE_TRIAL transaction {txn} is undecided; it is handed back to {} ({})",
                program.display(),
                if ready { "ready" } else { "not ready" }
            );
            bt_platform::install_flip::started_of(pid).map(|started| Running { pid, started })
        }
        Err(error) => {
            eprintln!(
                "BT_UPDATE_TRIAL transaction {txn} is undecided and {} could not be started: {error}",
                program.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use super::*;
    use crate::update_txn::{
        Body, Home, Inventories, Journal, Layout, Nonce, Outcome, Phase, Receipt, TrialProcess,
        TxnId,
    };

    const TXN: TxnId = TxnId::new([0x5a; 16]);

    fn nonce() -> Nonce {
        Nonce::new([0x3c; 32])
    }

    /// A private folder for one test, empty.
    fn scratch(tag: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-update-trial-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private folder for this test");
        // macOS hands out its temporary folder through a link (`/var` →
        // `/private/var`), and the integration marks refuse a path through
        // one; the folder's own name is what a real data folder has.
        if bt_platform::host_platform() == bt_platform::HostPlatform::Windows {
            root
        } else {
            std::fs::canonicalize(&root).expect("the private folder has a real name")
        }
    }

    /// The journal of `txn` in `phase`, as the lock holder encodes it.
    fn journal_bytes(txn: TxnId, phase: Phase) -> Vec<u8> {
        Journal {
            txn,
            rescue: "rescue".to_owned(),
            body: Body {
                adapter: crate::update_txn::Adapter::Ours,
                phase,
                layout: Layout::Members(Inventories {
                    old_shipped: Vec::new(),
                    old_present: Vec::new(),
                    new: Vec::new(),
                }),
            },
        }
        .encode()
    }

    /// The header alone of `phase`'s journal: no body at all.
    fn journal_bytes_header_only(phase: Phase) -> Vec<u8> {
        let bytes = journal_bytes(TXN, phase);
        crate::update_txn::Header::parse(&bytes)
            .expect("a journal's header")
            .encode()
    }

    fn trial_phase() -> Phase {
        Phase::Trial {
            nonce: nonce(),
            process: TrialProcess {
                pid: 4242,
                started: 7,
            },
            began_ms: 1_700_000_000_000,
        }
    }

    /// RED (U-13, U-29b) — **a trial reads `Committed` only where the journal
    /// says it, reads every retirement without a commit and every
    /// disappearance as an end, and a rollback still `destructive` as not
    /// decided yet.**
    ///
    /// From the frozen header alone (F-8, coordinator ruling 2026-09-27):
    /// `outcome` says committed or rolled back, since the class cannot —
    /// `Trial`, `Committed` and `RollbackIntent` share `destructive`, both
    /// retirements `terminal`. A bare header, with no body at all, decides the
    /// same way. A `Stuck` whose new bundle is live recovers forward on the
    /// receipt of the trial its holder starts over it (U-29b, the
    /// coordinator's ruling 3), so a `destructive` rollback cannot end a
    /// trial's watch: the writes stay held, never written, until the
    /// transaction retires one way or the other.
    ///
    /// MUTATION: in `update_txn::trial_sight`, answer `Ended` for the outcome
    /// `rolled_back` whatever the class.
    #[test]
    fn a_trial_reads_committed_only_where_the_journal_says_it() {
        let cases = [
            (Phase::Moving, TrialSight::Undecided),
            (trial_phase(), TrialSight::Undecided),
            (Phase::Committed, TrialSight::Committed),
            (
                Phase::Retired {
                    outcome: Outcome::Committed,
                    untried: false,
                },
                TrialSight::Committed,
            ),
            (
                Phase::RollbackIntent {
                    trial: Some(TrialProcess { pid: 1, started: 2 }),
                    trial_started: false,
                },
                TrialSight::Undecided,
            ),
            (
                Phase::Stuck {
                    trial: None,
                    trial_started: false,
                    last_error: "held".to_owned(),
                    attempts: 1,
                    retrial: None,
                },
                TrialSight::Undecided,
            ),
            (Phase::RolledBack { untried: false }, TrialSight::Undecided),
            (Phase::Abandoned, TrialSight::Ended),
            (
                Phase::Retired {
                    outcome: Outcome::RolledBack,
                    untried: false,
                },
                TrialSight::Ended,
            ),
        ];
        for (phase, seen) in cases {
            let bytes = journal_bytes(TXN, phase.clone());
            assert_eq!(trial_sight(Some(&bytes), &TXN), seen, "{phase:?}");
        }
        let another = journal_bytes(TxnId::new([0x01; 16]), Phase::Committed);
        assert_eq!(
            trial_sight(Some(&another), &TXN),
            TrialSight::Ended,
            "another transaction's journal: ours is gone"
        );
        assert_eq!(trial_sight(None, &TXN), TrialSight::Ended, "no journal");
        for (phase, seen) in [
            (Phase::Committed, TrialSight::Committed),
            (trial_phase(), TrialSight::Undecided),
            (Phase::Abandoned, TrialSight::Ended),
        ] {
            let header = journal_bytes_header_only(phase.clone());
            assert_eq!(
                trial_sight(Some(&header), &TXN),
                seen,
                "header only: {phase:?}"
            );
        }
        let torn = journal_bytes(TXN, Phase::Committed);
        assert_eq!(
            trial_sight(Some(&torn[..torn.len() / 2]), &TXN),
            TrialSight::Undecided,
            "a journal this build cannot read is not an answer"
        );
    }

    /// RED (U-13) — **outside a trial the gate is inert: every writer writes,
    /// and nothing is recorded.**
    ///
    /// Every start but an update's trial is every start Folio has ever made,
    /// and none of them may change: the gate answers `false` to every writer
    /// and holds nothing.
    ///
    /// MUTATION: drop the `if !trial { return false; }` from `Gate::defer`.
    #[test]
    fn a_start_that_is_no_trial_defers_nothing() {
        let gate = Gate::new();
        for writer in Writer::ALL {
            assert!(!gate.defer(false, writer), "{writer:?}");
        }
        assert!(!gate.defers(false));
        assert!(gate.pending().is_empty());
        assert!(!writes_are_deferred(), "the test process is no trial");
    }

    /// RED (U-13) — **the writes a trial held back are released when its
    /// watch reads `Committed`, and from then on nothing is held back.**
    ///
    /// A fake journal flips from `Trial` to `Committed` under a real watch on a
    /// real file: the wake comes once, the writers held back come out in
    /// release order, and a writer asking afterwards writes at once.
    ///
    /// MUTATION: in `Gate::decide`, leave `pending` in place on `Committed`
    /// (release nothing).
    #[test]
    fn pending_writes_are_released_when_the_watch_reads_committed() {
        let root = scratch("release");
        let journal = root.join("journal.json");
        std::fs::write(&journal, journal_bytes(TXN, trial_phase())).unwrap();
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        assert!(gate.defer(true, Writer::Settings));
        assert!(gate.defer(true, Writer::DataFolder));
        assert!(gate.defer(true, Writer::Settings), "asked twice, held once");
        assert_eq!(gate.pending(), vec![Writer::DataFolder, Writer::Settings]);

        let woken = Arc::new(AtomicUsize::new(0));
        let watch_woken = Arc::clone(&woken);
        let watched = journal.clone();
        let watcher = std::thread::spawn(move || {
            watch(
                gate,
                &watched,
                TXN,
                Duration::from_millis(5),
                &|| {
                    watch_woken.fetch_add(1, Ordering::SeqCst);
                },
                None,
                &mut watchdog_asleep(),
            );
        });
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(woken.load(Ordering::SeqCst), 0, "undecided: nothing yet");
        assert!(gate.defers(true));
        std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap();
        watcher
            .join()
            .expect("the watch stops once it has read the commit");

        assert_eq!(woken.load(Ordering::SeqCst), 1, "woken once");
        assert_eq!(
            gate.take_released(),
            vec![Writer::DataFolder, Writer::Settings]
        );
        assert!(gate.take_released().is_empty(), "released once");
        assert!(!gate.defers(true), "a committed trial writes");
        assert!(!gate.defer(true, Writer::Pins), "and records nothing more");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-35 round 2, the review's B3) — **U-35's reserved trial asks to
    /// commit itself only once it is ready, asks again while the answer is
    /// `Pending`, and a commit of its own releases what it held back** —
    /// with the journal still saying `TrialStarting` and nobody else writing
    /// it, as when every holder's rescue copy is refused. A trial that is not
    /// ready never asks: its writes stay held.
    ///
    /// The watch's own order is the seam: the watchdog (due at once here)
    /// runs after the own-commit step of a turn, so the first turn sees a
    /// trial that is not ready, and the watchdog's first call makes it ready
    /// for the turns after it. No clock is waited on.
    ///
    /// MUTATION: drop `gate.ready() &&` from the watch's own-commit step (it
    /// asks before readiness); or treat `Pending` as the end of asking.
    #[test]
    fn the_reserved_trial_commits_itself_only_once_ready() {
        let root = scratch("own-commit");
        let journal = root.join("journal.json");
        std::fs::write(
            &journal,
            journal_bytes(
                TXN,
                Phase::TrialStarting {
                    nonce: nonce(),
                    began_ms: 1_700_000_000_000,
                },
            ),
        )
        .unwrap();
        let gate = Gate::new();
        assert!(gate.defer(true, Writer::Settings));
        let (asked, asked_unready, woken) = (
            std::cell::Cell::new(0_usize),
            std::cell::Cell::new(0_usize),
            std::cell::Cell::new(0_usize),
        );
        let mut commit = || {
            asked.set(asked.get() + 1);
            if !gate.ready() {
                asked_unready.set(asked_unready.get() + 1);
            }
            // Pending twice — the lock held, the receipt not yet on disk —
            // then committed.
            if asked.get() <= 2 {
                Ok(LastTrialCommit::Pending)
            } else {
                Ok(LastTrialCommit::Committed)
            }
        };
        let mut becomes_ready = |_: bool| {
            gate.ready_for_a_test();
            None
        };
        watch(
            &gate,
            &journal,
            TXN,
            Duration::ZERO,
            &|| woken.set(woken.get() + 1),
            Some(&mut commit),
            &mut Watchdog {
                every: Duration::ZERO,
                hand_back: &mut becomes_ready,
            },
        );
        assert_eq!(asked_unready.get(), 0, "not ready: never asked");
        assert_eq!(asked.get(), 3, "asked until committed");
        assert_eq!(woken.get(), 1);
        assert_eq!(gate.take_released(), vec![Writer::Settings]);
        assert!(!gate.defers(true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-13) — **a watch that reads an end without a commit stops, wakes
    /// nobody, drops what was held back, and the gate stays shut.**
    ///
    /// F-7: a rollback leaves O's data exactly as O left it, so nothing the
    /// trial held back may ever be written — not at the end, not later.
    ///
    /// MUTATION: in `Gate::decide`, treat `TrialSight::Ended` as `Committed`.
    #[test]
    fn the_watch_stops_on_an_end_without_a_commit_and_drops_the_pending_writes() {
        let root = scratch("ended");
        let journal = root.join("journal.json");
        for ending in [
            Some(journal_bytes(
                TXN,
                Phase::Retired {
                    outcome: Outcome::RolledBack,
                    untried: false,
                },
            )),
            None,
        ] {
            match &ending {
                Some(bytes) => std::fs::write(&journal, bytes).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&journal);
                }
            }
            let gate = Gate::new();
            assert!(gate.defer(true, Writer::Settings));
            let woken = AtomicUsize::new(0);
            watch(
                &gate,
                &journal,
                TXN,
                Duration::from_millis(5),
                &|| {
                    woken.fetch_add(1, Ordering::SeqCst);
                },
                None,
                &mut watchdog_asleep(),
            );
            assert_eq!(woken.load(Ordering::SeqCst), 0, "{ending:?}");
            assert!(gate.take_released().is_empty());
            assert!(gate.pending().is_empty(), "dropped");
            assert!(gate.defers(true), "shut for the rest of the process");
            assert!(gate.defer(true, Writer::Session));
            assert!(gate.pending().is_empty(), "and nothing is held for later");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Watch `journal` on a thread of its own with the watchdog's period
    /// `every`, `hand_back` answering each hand-back; the thread, and every
    /// hand-back's instant (from the watch's start) and `ready` word.
    #[allow(clippy::type_complexity)]
    fn watching(
        gate: &'static Gate,
        journal: &Path,
        every: Duration,
        answer: Option<bt_platform::install_flip::Running>,
        begun: Instant,
    ) -> (
        std::thread::JoinHandle<()>,
        Arc<Mutex<Vec<(Duration, bool)>>>,
    ) {
        let handed: Arc<Mutex<Vec<(Duration, bool)>>> = Arc::default();
        let seen = Arc::clone(&handed);
        let watched = journal.to_path_buf();
        let watcher = std::thread::spawn(move || {
            let mut hand_back = |ready: bool| {
                seen.lock().unwrap().push((begun.elapsed(), ready));
                answer
            };
            let mut watchdog = Watchdog {
                every,
                hand_back: &mut hand_back,
            };
            watch(
                gate,
                &watched,
                TXN,
                Duration::from_millis(5),
                &|| {},
                None,
                &mut watchdog,
            );
        });
        (watcher, handed)
    }

    /// RED (U-37, design revision (h) H.2) — **an undecided transaction is
    /// handed back at one, two, four and eight watchdog periods after the
    /// watch began, never more; not while the recovery an earlier hand-back
    /// started still runs; with the trial's readiness as it is at each; and
    /// never once the transaction is decided.**
    ///
    /// Codex's review of `caea1099`, finding 2: the watchdog rearmed every
    /// period for ever, and each hand-back could add a contender. Here with a
    /// 40 ms period: a hand-back that answers no recovery is made four times;
    /// one that answers a recovery still running (this process, by its pid and
    /// start instant) is made once.
    ///
    /// MUTATION: in `watch`, hand back at every due time whatever `spent` and
    /// the earlier recovery say.
    #[test]
    fn an_undecided_trial_hands_back_at_most_four_times_and_one_at_a_time() {
        let root = scratch("watchdog");
        let journal = root.join("journal.json");
        std::fs::write(&journal, journal_bytes(TXN, trial_phase())).unwrap();
        let every = Duration::from_millis(40);

        // Waited for by what happened, never by a window of time: a slow
        // runner only makes it slower.
        let until = |done: &dyn Fn() -> bool| {
            let give_up = Instant::now() + Duration::from_secs(60);
            while !done() {
                assert!(Instant::now() < give_up, "the watch never got there");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        let begun = Instant::now();
        let (watcher, handed) = watching(gate, &journal, every, None, begun);
        until(&|| !handed.lock().unwrap().is_empty());
        gate.adopt_claim();
        assert!(gate.hand_receipt(), "the receipt falls due");
        let ready_from = begun.elapsed();
        until(&|| handed.lock().unwrap().len() >= HAND_BACKS as usize);
        std::thread::sleep(every * 4);
        let made = handed.lock().unwrap().clone();
        assert_eq!(made.len(), HAND_BACKS as usize, "never more: {made:?}");
        for (at, (when, ready)) in made.iter().enumerate() {
            assert!(*when >= every * (1 << at), "due {at} at {when:?}");
            if *when > ready_from {
                assert!(*ready, "ready once the receipt fell due: {made:?}");
            }
        }
        assert!(!made[0].1, "not ready at the first: {made:?}");
        std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap();
        watcher.join().expect("the watch stops at the commit");

        std::fs::write(&journal, journal_bytes(TXN, trial_phase())).unwrap();
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        let (watcher, handed) = watching(
            gate,
            &journal,
            every,
            Some(crate::update_apply::this_process()),
            Instant::now(),
        );
        until(&|| !handed.lock().unwrap().is_empty());
        // Past every later due time (8 periods from the start): each is spent
        // beside the recovery still running.
        std::thread::sleep(every * 10);
        assert_eq!(
            handed.lock().unwrap().len(),
            1,
            "never beside the recovery it started"
        );
        std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap();
        watcher.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-37, H.2) — **a journal that cannot be read is no answer and no
    /// excuse: the refused receipt is still written again, and nothing is
    /// handed back while it stays unreadable — no holder could read it
    /// either.**
    ///
    /// Codex's review of `caea1099`, finding 2: a read error `continue`d past
    /// the receipt's retry and the watchdog's deadline.
    ///
    /// MUTATIONS: in `watch`, read the journal first and go back to the top of
    /// the loop on a read error (the round-1 order); or hand back an
    /// `Unreadable` transaction as an undecided one.
    #[test]
    fn an_unreadable_journal_still_retries_the_receipt_and_is_never_handed_back() {
        let root = scratch("unreadable");
        let home = Home::at(root.join("home"));
        // A folder where the journal is: every read fails, and not with
        // `NotFound`.
        let journal = root.join("journal.json");
        std::fs::create_dir_all(&journal).unwrap();
        let job = ReceiptJob {
            path: home.receipt_path(TXN, &nonce()),
            bytes: b"{\"v\":1}".to_vec(),
        };
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        gate.adopt_claim();
        assert!(gate.hand_receipt());
        gate.keep_receipt(job.clone());
        gate.owe_receipt();
        let (watcher, handed) = watching(
            gate,
            &journal,
            Duration::from_millis(30),
            None,
            Instant::now(),
        );
        std::thread::sleep(Duration::from_millis(100));
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let give_up = Instant::now() + Duration::from_secs(10);
        while !job.path.exists() {
            assert!(Instant::now() < give_up, "the receipt is written again");
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(400));
        assert!(handed.lock().unwrap().is_empty(), "never handed back");
        std::fs::remove_dir_all(&journal).unwrap();
        std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap();
        watcher.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-37, H.4 rules A1 and A3) — **a trial's watch, in a process of its
    /// own, hands its transaction back to a rescue build that knows the word
    /// with its own exact identity and state — `--from-trial
    /// <pid>:<started>:unready` — and the process does not end itself.**
    ///
    /// The child is a copy of this test binary running the real watch and the
    /// product's `hand_back`, whose start is recorded into a file (the rescue
    /// build's argv); the rescue executable is a real program carrying
    /// `VERSIONINFO` 0.4.7. The parent reads the line through the recovery
    /// door's own grammar and ends the child by its handle. (Withdrawn (g).2
    /// ended a not-ready trial with exit code 3 right after the start.)
    ///
    /// MUTATION: in `update_trial::hand_back`, leave the `--from-trial` word
    /// out of the line.
    #[test]
    fn a_trial_watch_in_a_process_of_its_own_hands_back_its_identity_and_stays() {
        if bt_platform::host_platform() != bt_platform::HostPlatform::Windows {
            return;
        }
        if let Ok(root) = std::env::var("BT_U37_WATCH_CHILD") {
            let root = PathBuf::from(root);
            let home = Home::at(root.join("home"));
            struct IntoAFile(PathBuf);
            impl Starter for IntoAFile {
                fn start(
                    &mut self,
                    program: &Path,
                    line: &[std::ffi::OsString],
                ) -> io::Result<u32> {
                    let mut said = program.display().to_string();
                    for word in line {
                        said.push('\n');
                        said.push_str(&word.to_string_lossy());
                    }
                    std::fs::write(&self.0, said)?;
                    Ok(std::process::id())
                }
            }
            let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
            let spawned = bt_platform::spawn_at_priority(
                "bt-u37-watch-child",
                bt_platform::ThreadPriority::BelowNormal,
                move |ctx| {
                    let mut starter = IntoAFile(root.join("line.txt"));
                    let mut hand_back = |ready: bool| {
                        hand_back(ctx, &home, TXN, ready, FROM_TRIAL_SINCE, &mut starter)
                    };
                    let mut watchdog = Watchdog {
                        every: Duration::from_millis(100),
                        hand_back: &mut hand_back,
                    };
                    watch(
                        gate,
                        &home.journal(),
                        TXN,
                        Duration::from_millis(10),
                        &|| {},
                        None,
                        &mut watchdog,
                    );
                },
            )
            .unwrap();
            // The child ends by itself, whatever the parent does: its watch
            // never decides, so it is left behind when this returns.
            drop(spawned);
            // The parent ends it by its handle; this only bounds an orphan.
            std::thread::sleep(Duration::from_secs(600));
            return;
        }
        let root = scratch("watch-child");
        let home = Home::at(root.join("home"));
        std::fs::create_dir_all(home.root()).unwrap();
        let rescue = root.join("rescue-folio.exe");
        bt_platform::trust_harness::program(
            &rescue,
            bt_platform::trust::FileVersion([0, 4, 7, 0]),
            bt_platform::trust_harness::Behaviour::Returns,
        )
        .unwrap();
        let mut journal = Journal::parse(&journal_bytes(TXN, trial_phase())).unwrap();
        journal.rescue = rescue.display().to_string();
        std::fs::write(home.journal(), journal.encode()).unwrap();
        let child = bt_platform::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "update_trial::tests::a_trial_watch_in_a_process_of_its_own_hands_back_its_identity_and_stays",
                "--nocapture",
            ])
            .env("BT_U37_WATCH_CHILD", &root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        /// Ends the child by its own handle whatever this test does.
        struct Ended(std::process::Child);
        impl Drop for Ended {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Ended(child);
        let line = root.join("line.txt");
        let give_up = Instant::now() + Duration::from_secs(60);
        while !line.exists() {
            assert!(Instant::now() < give_up, "the child hands back");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
        let said = std::fs::read_to_string(&line).unwrap();
        let mut words = said.lines();
        assert_eq!(words.next(), Some(rescue.display().to_string().as_str()));
        let line: Vec<std::ffi::OsString> = words.map(std::ffi::OsString::from).collect();
        let Some(Ok(crate::cli::UpdateDoor::Recover {
            home: None,
            then_launch: None,
            handed_back: Some(handed),
        })) = crate::cli::update_door(line.clone())
        else {
            panic!("the recovery door's line: {line:?}");
        };
        let pid = child.0.id();
        assert_eq!(
            handed.process,
            bt_platform::install_flip::Running {
                pid,
                started: bt_platform::install_flip::started_of(pid).unwrap(),
            }
        );
        assert!(!handed.ready, "its first text never reached the glass");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            matches!(child.0.try_wait(), Ok(None)),
            "the trial does not end itself"
        );
        drop(child);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-37) — **a receipt whose write the storage worker could not make
    /// is written again by the trial's watch once the home takes it**, create-
    /// new, byte for byte the receipt that was handed over.
    ///
    /// The macOS rehearsal's D14: the home made read-only while the trial
    /// starts refuses its one receipt, and a healthy trial nobody can commit
    /// runs on with its writes held back. The real storage worker is asked
    /// here; its transaction folder is missing, so its write is refused, and
    /// the watch owes it; the folder then appears.
    ///
    /// MUTATION: in `Gate::write_owed_receipt`, answer `None` (never write it
    /// again).
    #[test]
    fn a_refused_receipt_is_written_again_by_the_watch() {
        let root = scratch("receipt-again");
        let home = Home::at(root.join("home"));
        let journal = root.join("journal.json");
        std::fs::write(&journal, journal_bytes(TXN, trial_phase())).unwrap();
        let mut store = persist::SessionStore::at(
            root.join("data").join("session.json"),
            root.join("data").join("session.lock"),
        );
        let job = ReceiptJob {
            path: home.receipt_path(TXN, &nonce()),
            bytes: Receipt {
                txn: TXN,
                nonce: nonce(),
                pid: std::process::id(),
                version: crate::version::VERSION.to_owned(),
                started: None,
            }
            .encode(),
        };
        let gate: &'static Gate = Box::leak(Box::new(Gate::new()));
        gate.adopt_claim();
        assert!(gate.hand_receipt());
        gate.keep_receipt(job.clone());
        gate.await_receipt(store.write_receipt(job.clone()).expect("a writer"));
        let watched = journal.clone();
        let watcher = std::thread::spawn(move || {
            watch(
                gate,
                &watched,
                TXN,
                Duration::from_millis(5),
                &|| {},
                None,
                &mut watchdog_asleep(),
            );
        });
        std::thread::sleep(Duration::from_millis(150));
        assert!(!job.path.exists(), "the home refuses it for now");
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let give_up = Instant::now() + Duration::from_secs(10);
        while !job.path.exists() {
            assert!(Instant::now() < give_up, "the watch writes it again");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(std::fs::read(&job.path).unwrap(), job.bytes);
        std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap();
        watcher.join().unwrap();
        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN (U-37) — **the watchdog's period is the road's own budgets summed**:
    /// the trial's deadline, the stop's two graces and the declaration's retry
    /// — past every decision a lock holder alive would make about a trial it
    /// watches, and still under two minutes.
    ///
    /// MUTATION: `WATCHDOG` = the trial's deadline alone (90 s).
    #[test]
    fn the_watchdog_comes_after_every_decision_a_live_holder_makes() {
        let limits = crate::update_apply::Limits::PRODUCT;
        let decided = Duration::from_millis(limits.trial_within_ms)
            + limits.quit_within
            + limits.end_within
            + crate::update_apply::JOURNAL_WRITE_WITHIN;
        assert_eq!(WATCHDOG, decided);
        assert!(WATCHDOG < Duration::from_secs(120), "{WATCHDOG:?}");
    }

    /// RED (U-13) — **the receipt is written by the storage worker, never by
    /// the thread that asks for it, and is the frozen receipt of this trial.**
    ///
    /// F-14: the window thread emits readiness and stays runnable; a
    /// storage-lane worker writes the receipt create-new and flushes it. The
    /// store is the product's own, over a private folder; the answer says
    /// which thread wrote.
    ///
    /// MUTATION: in `SessionWriter::send_receipt`, answer
    /// `write_receipt(&job)` on the calling thread instead of sending the job.
    #[test]
    fn the_receipt_is_written_off_the_window_thread() {
        let root = scratch("receipt");
        let home = Home::at(root.join("home"));
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let mut store = persist::SessionStore::at(
            root.join("data").join("session.json"),
            root.join("data").join("session.lock"),
        );
        let receipt = Receipt {
            txn: TXN,
            nonce: nonce(),
            pid: std::process::id(),
            version: crate::version::VERSION.to_owned(),
            started: None,
        };
        let path = home.receipt_path(TXN, &nonce());
        let answer = store
            .write_receipt(ReceiptJob {
                path: path.clone(),
                bytes: receipt.encode(),
            })
            .expect("the store has a writer");
        let written = answer
            .recv_timeout(Duration::from_secs(10))
            .expect("the worker answers");
        assert_eq!(written.result, Ok(()));
        assert_eq!(
            written.by,
            bt_platform::admission::Role::Worker("session-writer")
        );
        assert_ne!(bt_platform::admission::role(), written.by);
        assert_eq!(
            Receipt::parse(&std::fs::read(&path).unwrap()).unwrap(),
            receipt
        );
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("health-{}", nonce())
        );
        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-13) — **an existing receipt is not overwritten**: the second
    /// write is refused and the first receipt stays byte for byte.
    ///
    /// MUTATION: write the receipt with `install_txn::durable_write` (which
    /// replaces) in `persist::write_receipt`.
    #[test]
    fn an_existing_receipt_is_not_overwritten() {
        let root = scratch("receipt-once");
        let home = Home::at(root.join("home"));
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let path = home.receipt_path(TXN, &nonce());
        let mut store = persist::SessionStore::at(
            root.join("data").join("session.json"),
            root.join("data").join("session.lock"),
        );
        let mut write = |pid: u32| {
            let bytes = Receipt {
                txn: TXN,
                nonce: nonce(),
                pid,
                version: "0.0.1".to_owned(),
                // A process that runs: this test process, by its own start
                // instant; any other pid at that instant names nobody.
                started: bt_platform::install_flip::started_of(std::process::id()),
            }
            .encode();
            store
                .write_receipt(ReceiptJob {
                    path: path.clone(),
                    bytes,
                })
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .result
        };
        assert_eq!(write(std::process::id()), Ok(()));
        let first = std::fs::read(&path).unwrap();
        assert!(write(2).is_err(), "the second receipt is refused");
        assert_eq!(std::fs::read(&path).unwrap(), first);
        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-35 round 3, the review's F1) — **what is already at a trial's
    /// receipt name decides whether its receipt is written there**: the same
    /// receipt is already written; an earlier attempt of the same trial — this
    /// transaction and nonce, naming no running process (its pid at another
    /// start instant, as a reused pid would be, or no start instant at all) —
    /// is replaced; a receipt naming a running process, or one of another
    /// transaction or nonce, is never written over.
    ///
    /// MUTATION: in `at_its_name`, drop `!names_a_running_process(there)` (a
    /// live receipt is replaced); drop the transaction and nonce check (a
    /// foreign one is replaced); or in `write_receipt`, never replace (the
    /// earlier attempt stays).
    #[test]
    fn a_receipt_is_replaced_only_when_it_is_an_earlier_attempt_of_the_same_trial() {
        let root = scratch("receipt-rule");
        let home = Home::at(root.join("home"));
        std::fs::create_dir_all(home.transaction(TXN)).unwrap();
        let path = home.receipt_path(TXN, &nonce());
        let me = std::process::id();
        let now = bt_platform::install_flip::started_of(me);
        let receipt = |txn: TxnId, nonce: Nonce, pid: u32, started: Option<u64>| Receipt {
            txn,
            nonce,
            pid,
            version: "0.4.7".to_owned(),
            started,
        };
        let mine = receipt(TXN, nonce(), me, now);
        let job = ReceiptJob {
            path: path.clone(),
            bytes: mine.encode(),
        };
        assert!(now.is_some(), "this process reads its own start instant");
        let other_txn = TxnId::new([0x11; 16]);
        let other_nonce = Nonce::new([0x22; 32]);
        let running = Receipt {
            version: "0.4.6".to_owned(),
            ..mine.clone()
        };
        // (what is there, whether this receipt replaces it)
        for (there, replaced) in [
            // A running process: this one, by its true start instant.
            (running, false),
            // Its pid at another start instant: a reused pid names nobody.
            (receipt(TXN, nonce(), me, now.map(|at| at + 1)), true),
            // No start instant: names nobody.
            (receipt(TXN, nonce(), 4, None), true),
            // Another transaction's, or another nonce's, at this name.
            (receipt(other_txn, nonce(), 4, None), false),
            (receipt(TXN, other_nonce, 4, None), false),
        ] {
            let written_there = there.encode();
            std::fs::write(&path, &written_there).unwrap();
            let answer = write_receipt(&job);
            let on_disk = std::fs::read(&path).unwrap();
            if replaced {
                assert_eq!(answer, Ok(()), "an earlier attempt: {there:?}");
                assert_eq!(on_disk, job.bytes, "replaced: {there:?}");
            } else {
                assert!(answer.is_err(), "kept: {there:?}");
                assert_eq!(on_disk, written_there, "byte for byte: {there:?}");
            }
        }
        // The same receipt already there is written.
        std::fs::write(&path, &job.bytes).unwrap();
        assert_eq!(write_receipt(&job), Ok(()));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-35 round 3, the review's F3) — **`Unprovable` is said once and
    /// never asked again**: the trial stays uncommitted, its writes held,
    /// until the journal is decided by somebody else.
    ///
    /// The seam is the watch's own order, as in
    /// `the_reserved_trial_commits_itself_only_once_ready`: the watchdog, due
    /// every turn, makes the trial ready on its first call and writes the
    /// decision a recovery would on its third.
    ///
    /// MUTATION: in the watch, treat `Unprovable` as `Pending` (asked every
    /// turn for ever).
    #[test]
    fn an_unprovable_trial_stops_asking_to_commit_itself() {
        let root = scratch("unprovable");
        let journal = root.join("journal.json");
        std::fs::write(
            &journal,
            journal_bytes(
                TXN,
                Phase::TrialStarting {
                    nonce: nonce(),
                    began_ms: 1_700_000_000_000,
                },
            ),
        )
        .unwrap();
        let gate = Gate::new();
        assert!(gate.defer(true, Writer::Settings));
        let (asked, turns) = (std::cell::Cell::new(0_usize), std::cell::Cell::new(0_usize));
        let mut commit = || {
            asked.set(asked.get() + 1);
            Ok(LastTrialCommit::Unprovable)
        };
        let mut watchdog_turn = |_: bool| {
            turns.set(turns.get() + 1);
            match turns.get() {
                1 => gate.ready_for_a_test(),
                3 => std::fs::write(&journal, journal_bytes(TXN, Phase::Committed)).unwrap(),
                _ => {}
            }
            None
        };
        watch(
            &gate,
            &journal,
            TXN,
            Duration::ZERO,
            &|| {},
            Some(&mut commit),
            &mut Watchdog {
                every: Duration::ZERO,
                hand_back: &mut watchdog_turn,
            },
        );
        assert_eq!(asked.get(), 1, "asked once, then never again");
        assert!(turns.get() >= 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-13) — **the receipt is due once, only after the trial's claim
    /// was adopted and only while its transaction is undecided.**
    ///
    /// MUTATION: drop `state.claim_adopted &&` from `Gate::hand_receipt`.
    #[test]
    fn the_receipt_is_due_once_after_the_claim_and_before_a_decision() {
        let gate = Gate::new();
        assert!(!gate.hand_receipt(), "no claim adopted yet");
        gate.adopt_claim();
        assert!(gate.hand_receipt());
        assert!(!gate.hand_receipt(), "once");
        let decided = Gate::new();
        decided.adopt_claim();
        decided.decide(TrialSight::Ended);
        assert!(!decided.hand_receipt(), "a rollback wants no receipt");
    }

    /// RED (U-13) — **a trial adopts the data directory's claim, so the claim
    /// table answers "this process" from the first ask; a claim held elsewhere
    /// is asked for until the wait runs out, and then refused by name.**
    ///
    /// MUTATION: drop `persist::adopt_claim(storage, claim);` from
    /// `take_the_claim_within` (the claim is taken and let go).
    #[test]
    fn a_trial_adopts_its_claim_before_anything_asks_who_writes() {
        let root = scratch("claim");
        let free = root.join("free");
        let held = root.join("held");
        std::fs::create_dir_all(&free).unwrap();
        std::fs::create_dir_all(&held).unwrap();
        let gate = Gate::new();
        assert_eq!(
            take_the_claim_within(&gate, &free, Duration::from_secs(2)),
            Ok(())
        );
        assert!(
            persist::try_claim(&free).is_err(),
            "the claim is still held — by the table it was adopted into"
        );
        assert!(persist::is_writer_of(&free), "adopted: this process writes");
        assert!(gate.hand_receipt(), "and the trial may say it is ready");

        let holder = persist::try_claim(&held).expect("the test holds this one");
        let gate = Gate::new();
        let refused = take_the_claim_within(&gate, &held, Duration::from_millis(300))
            .expect_err("held elsewhere for the whole wait");
        assert!(refused.contains("does not start"), "{refused}");
        assert!(!gate.hand_receipt(), "no claim, no receipt");
        drop(holder);
        let _ = std::fs::remove_dir_all(&root);
    }

    // ─────────────────────── a start's writers, in a process of their own ───────────────────────

    /// The variable that makes this test binary's child run one test.
    const CHILD: &str = "BT_UPDATE_TRIAL_TEST_CHILD";
    /// The child's private root, from the parent.
    const CHILD_ROOT: &str = "BT_UPDATE_TRIAL_TEST_ROOT";

    /// **Run `selector` again, alone, in a process whose data folder, local
    /// folder and home are under `root`**, and pass only if it passed there.
    ///
    /// A process of its own because the trial is a fact once per process
    /// (`update_startup::become_trial`) and the data folder is resolved once
    /// per process (`persist::storage_dir`).
    fn run_in_a_process_of_its_own(selector: &str, root: &Path) {
        let output = bt_platform::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", selector, "--nocapture", "--test-threads=1"])
            .env(CHILD, selector)
            .env(CHILD_ROOT, root)
            .env("APPDATA", root.join("roaming"))
            .env("LOCALAPPDATA", root.join("local"))
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("xdg"))
            .env("BT_POWERSHELL_PROFILE", root.join("profile.ps1"))
            .output()
            .expect("the harness can run one of its own tests");
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && said.contains("test result: ok. 1 passed"),
            "`{selector}` did not pass in its own process ({}):\n{said}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The child's root, when this process is the child running `selector`.
    fn child_root(selector: &str) -> Option<PathBuf> {
        std::env::var_os(CHILD)
            .is_some_and(|named| named == selector)
            .then(|| std::env::var_os(CHILD_ROOT).map(PathBuf::from))
            .flatten()
    }

    /// The data folder a start in `root` finds before it has moved anything:
    /// the old name where the platform had one.
    fn seeded_data_folder(root: &Path) -> PathBuf {
        match bt_platform::host_platform() {
            bt_platform::HostPlatform::Windows => {
                root.join("roaming").join(persist::PREVIOUS_STORAGE_NAME)
            }
            bt_platform::HostPlatform::MacOs => root
                .join("home")
                .join("Library")
                .join("Application Support")
                .join(persist::STORAGE_NAME),
            bt_platform::HostPlatform::OtherUnix => root.join("xdg").join(persist::STORAGE_NAME),
        }
    }

    /// **The folder O left**: a data folder under its old name (where the
    /// platform had one), with a `settings.json` O wrote, a `keybindings.json`
    /// this build cannot read, the integration marks record and the reader's
    /// own profile — and, where there is PSReadLine, Folio's own older build in
    /// `Documents`. Answers whether that last one stands.
    fn seed_what_the_old_build_left(root: &Path) -> bool {
        let data = seeded_data_folder(root);
        std::fs::create_dir_all(&data).unwrap();
        let settings = bt_persist::SettingsV1 {
            psreadline_invite: bt_persist::PsReadLineInviteV1::Installed,
            ..bt_persist::SettingsV1::default()
        };
        bt_persist::write_settings_atomic(&data.join(persist::SETTINGS_FILE_NAME), &settings)
            .unwrap();
        std::fs::write(
            data.join(persist::KEYBINDINGS_FILE_NAME),
            "{ not what this build reads",
        )
        .unwrap();
        crate::shell_integration::profile_marks::Marks::default()
            .write(&data)
            .unwrap();
        std::fs::write(root.join("profile.ps1"), "# the reader's own profile\n").unwrap();
        crate::psreadline::tests::an_older_build_stands_in(&root.join("documents"))
    }

    /// Every file under `root`, with its bytes.
    fn every_file(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, at: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, into);
                } else {
                    let bytes = std::fs::read(&path).unwrap();
                    into.insert(path.strip_prefix(root).unwrap().to_path_buf(), bytes);
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(root, root, &mut files);
        files
    }

    /// Which writer a changed path belongs to, so a red names it.
    fn writer_of(path: &Path) -> &'static str {
        let text = path.to_string_lossy();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if text.contains("shell-integration") {
            "BashScript / ZshScripts"
        } else if name.contains(".rejected-") {
            "RefusedCopies"
        } else if name == "session.json" || name == "session.lock" {
            "Session"
        } else if name == persist::SETTINGS_FILE_NAME {
            "Settings"
        } else if name == persist::KEYBINDINGS_FILE_NAME {
            "Keybindings"
        } else if name == persist::PROFILES_FILE_NAME {
            "Profiles"
        } else if name == "pins.json" {
            "Pins"
        } else if name.starts_with("update-check") {
            "UpdateCheck"
        } else if text.starts_with("documents") {
            "PsReadLineUpgrade"
        } else if name == "profile.ps1" || text.contains("marks") {
            "ProfileMigration"
        } else if text.contains(persist::STORAGE_NAME) {
            "DataFolderMove"
        } else {
            "unknown"
        }
    }

    /// **A start's durable writers, in `Runtime::create`'s order**, each driven
    /// through its own product entry and each asked to write something: the
    /// data folder, the stores and a change to each, the update check's state,
    /// the integration marks' migration, the shell scripts and the PSReadLine
    /// upgrade. The registry writers (the Explorer repair, the toast identity)
    /// are not run here: a test that reached them would change the machine it
    /// runs on.
    ///
    /// Answers whether the marks' migration started (its worker's wake came).
    fn run_the_start_writers(root: &Path) -> bool {
        let storage = persist::storage_dir();
        let now = std::time::Instant::now();

        let mut session = persist::SessionStore::open();
        let mut document = session.loaded().clone();
        document.cursor_style = bt_persist::SessionCursorStyleV1::Underline;
        session.record(document, now);
        session.flush();

        let mut settings = persist::SettingsStore::open();
        let mut changed = settings.loaded().clone();
        changed.update_check = !changed.update_check;
        settings.store(changed);

        let mut keybindings = persist::KeybindingsStore::open();
        keybindings.store(vec![
            serde_json::from_value(serde_json::json!({ "action": "new-tab", "chord": "Ctrl+T" }))
                .unwrap(),
        ]);

        let mut profiles = persist::ProfilesStore::open();
        profiles.store(bt_persist::ProfilesV1 {
            schema_version: bt_persist::PROFILES_SCHEMA_VERSION,
            profiles: vec![bt_persist::ProfileEntryV1 {
                id: "pwsh".to_owned(),
                ..bt_persist::ProfileEntryV1::default()
            }],
        });

        let mut pins = crate::pins::PinsStore::open();
        pins.store(bt_persist::PinsV1 {
            pins: vec![
                serde_json::from_value(serde_json::json!({ "kind": "folder", "target": root }))
                    .unwrap(),
            ],
            ..bt_persist::PinsV1::default()
        });

        let offers = crate::update::OfferState::load(&storage, true);
        offers.skip("v9.9.9").unwrap();

        let (migrated, migration) = mpsc::channel();
        crate::shell_integration::install_wake(move || {
            let _ = migrated.send(());
        });
        crate::shell_integration::begin_startup_migration();

        let _ = crate::shell_integration::script_path();
        let _ = crate::shell_integration::zdotdir_path();

        let documents = root.join("documents");
        if documents.exists() {
            let _ = crate::psreadline::upgrade_recorded(
                &documents,
                &storage,
                bt_persist::PsReadLineInviteV1::Installed,
            );
        }

        let started = migration.recv_timeout(Duration::from_secs(3)).is_ok();
        // The session's own writer finishes what it was handed, or nothing.
        session.close();
        started
    }

    /// Every path whose bytes differ between `before` and `after`, or that one
    /// of them lacks, each with the writer it belongs to.
    fn changes(
        before: &BTreeMap<PathBuf, Vec<u8>>,
        after: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> Vec<String> {
        after
            .iter()
            .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
            .map(|(path, _)| format!("{} ({})", path.display(), writer_of(path)))
            .chain(
                before
                    .keys()
                    .filter(|path| !after.contains_key(*path))
                    .map(|path| format!("{} removed ({})", path.display(), writer_of(path))),
            )
            .collect()
    }

    /// RED (U-13) — **a trial start writes nothing durable before its
    /// transaction is committed**: after the stores are opened and changed,
    /// the update check's state changed, the marks' migration, the scripts and
    /// the PSReadLine upgrade asked for, every file under the start's private
    /// root is byte for byte what the old build left, and nothing was added —
    /// the data folder is not moved, no refused copy is kept, no sentinel is
    /// armed, and the migration never starts.
    ///
    /// Run in a process of its own over a private `APPDATA`, local folder and
    /// home, through each writer's product entry. `diagnostics.log` would be
    /// exempt (F-7); nothing here opens it.
    ///
    /// MUTATION: skip the gate in one writer — drop the
    /// `update_trial::defer(Writer::Settings)` from `SettingsStore::write_now`
    /// — and the red names the file and its writer (`settings.json
    /// (Settings)`).
    #[test]
    fn a_trial_start_writes_nothing_durable_before_committed() {
        const SELECTOR: &str =
            "update_trial::tests::a_trial_start_writes_nothing_durable_before_committed";
        if let Some(root) = child_root(SELECTOR) {
            assert!(update_startup::become_trial(
                TXN,
                nonce(),
                Home::at(root.join("install").join(".folio-update"))
            ));
            assert!(
                !run_the_start_writers(&root),
                "the marks' migration started (ProfileMigration)"
            );
            let pending = GATE.pending();
            for writer in [
                Writer::Session,
                Writer::Settings,
                Writer::RefusedCopies,
                Writer::UpdateCheck,
                Writer::ProfileMigration,
            ] {
                assert!(pending.contains(&writer), "{writer:?} in {pending:?}");
            }
            return;
        }
        let root = scratch("trial-start");
        seed_what_the_old_build_left(&root);
        let before = every_file(&root);
        run_in_a_process_of_its_own(SELECTOR, &root);
        let changed: Vec<String> = changes(&before, &every_file(&root))
            .into_iter()
            .filter(|line| !line.contains(crate::diagnostics::LOG_FILENAME))
            .collect();
        assert!(
            changed.is_empty(),
            "the trial wrote before it was committed:\n{}",
            changed.join("\n")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN (U-13) — **a start that is no trial writes as it always has**: the
    /// same writers over the same folder move the data folder to its new name
    /// (where there was an old one), keep the refused `keybindings.json`, write
    /// the documents and the update check's state, start
    /// the marks' migration, write the scripts and replace the older
    /// PSReadLine.
    ///
    /// MUTATION: make `Gate::defer` answer `true` whether or not the process
    /// is a trial.
    #[test]
    fn a_start_that_is_no_trial_writes_as_it_always_has() {
        const SELECTOR: &str =
            "update_trial::tests::a_start_that_is_no_trial_writes_as_it_always_has";
        if let Some(root) = child_root(SELECTOR) {
            assert!(run_the_start_writers(&root), "the migration starts");
            return;
        }
        let root = scratch("ordinary-start");
        let psreadline = seed_what_the_old_build_left(&root);
        let before = every_file(&root);
        run_in_a_process_of_its_own(SELECTOR, &root);
        let after = every_file(&root);
        let data = match bt_platform::host_platform() {
            bt_platform::HostPlatform::Windows => {
                let moved = root.join("roaming").join(persist::STORAGE_NAME);
                assert!(moved.is_dir(), "the data folder moved to its new name");
                assert!(!seeded_data_folder(&root).exists());
                moved
            }
            _ => seeded_data_folder(&root),
        };
        let data = data.strip_prefix(&root).unwrap().to_path_buf();
        for name in [
            persist::SETTINGS_FILE_NAME,
            persist::KEYBINDINGS_FILE_NAME,
            persist::PROFILES_FILE_NAME,
            "pins.json",
            "session.json",
            "update-check.json",
        ] {
            let path = data.join(name);
            assert!(
                after.contains_key(&path) && before.get(&path) != after.get(&path),
                "{name} is written"
            );
        }
        let changed = changes(&before, &after).join("\n");
        for (what, found) in [
            ("the refused keybindings.json is kept", ".rejected-"),
            ("the scripts are written", "shell-integration"),
        ] {
            assert!(changed.contains(found), "{what}:\n{changed}");
        }
        if psreadline {
            assert!(
                changed.contains("PsReadLineUpgrade"),
                "the older PSReadLine is replaced:\n{changed}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
