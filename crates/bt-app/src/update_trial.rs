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
//! `committed` releases; `rolled_back`, a `terminal` class, the journal gone or
//! naming another transaction **drops** the
//! pending writes, and the gate stays shut for the rest of the process: the
//! process runs on, writing nothing, until the applier ends it (F-7's live-child
//! policy; nothing here ends a process). `diagnostics.log` is exempt (F-7), and
//! so are the other diagnostics a run writes about itself — a hang report, the
//! panic log — which are append-only accounts, not state a later run reads.
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
//! **The journal has one writer, the lock holder.** N writes only its receipt;
//! turning it into `Committed` is the applier's. A receipt that lands after the
//! journal says `RollbackIntent` is ignored **by rule** — the lock holder's rule
//! (`update_txn::next` refuses it; U-18/U-21 hold it), not this module's.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use bt_platform::file_reads::{self, Lane};

use crate::persist;
use crate::update_startup;
use crate::update_txn::{Receipt, TrialSight, TxnId, trial_sight};

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
    /// The launch's replacement of Folio's own older PSReadLine and its stamp
    /// (`psreadline::upgrade_recorded`, ticket 56).
    PsReadLineUpgrade,
    /// The launch probe's repair of the Explorer package registration
    /// (`explorer_menu::begin_probe`).
    ExplorerRepair,
    /// The toast sender's identity in the registry (`NotificationDesk::show` →
    /// `bt_platform::Notifier::register_identity`).
    ToastIdentity,
}

impl Writer {
    /// Every writer, in release order.
    #[cfg(test)]
    pub(crate) const ALL: [Writer; 15] = [
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
    let (Some((txn, _)), Some(home)) = (update_startup::trial(), update_startup::trial_home())
    else {
        return;
    };
    let journal = home.journal();
    let started = bt_platform::spawn_at_priority(
        "folio-trial-watch",
        bt_platform::ThreadPriority::BelowNormal,
        move |_ctx| watch(&GATE, &journal, txn, WATCH_INTERVAL, &wake),
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

/// **The watch's loop**: read, decide, wait, until the transaction is decided.
/// Read-only: the journal has one writer, and it is not this process.
pub(crate) fn watch(gate: &Gate, journal: &Path, txn: TxnId, interval: Duration, wake: &dyn Fn()) {
    loop {
        let bytes = match file_reads::read(Lane::UpdateJournal, journal) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            // Held by its writer for the instant of a rename, or a share that
            // stopped answering: not an answer about the transaction.
            Err(_) => {
                std::thread::sleep(interval);
                continue;
            }
        };
        if let Some(written) = gate.receipt_answered() {
            match written.result {
                Ok(()) => eprintln!(
                    "BT_UPDATE_TRIAL receipt of {txn} written by {:?}",
                    written.by
                ),
                Err(error) => eprintln!(
                    "BT_UPDATE_TRIAL receipt of {txn} not written by {:?}: {error}",
                    written.by
                ),
            }
        }
        let sight = trial_sight(bytes.as_deref(), &txn);
        match sight {
            TrialSight::Undecided => std::thread::sleep(interval),
            TrialSight::Committed => {
                if gate.decide(sight) {
                    eprintln!(
                        "BT_UPDATE_TRIAL transaction {txn} is committed; its writes are released"
                    );
                    wake();
                }
                return;
            }
            TrialSight::Ended => {
                gate.decide(sight);
                eprintln!(
                    "BT_UPDATE_TRIAL transaction {txn} ended without a commit; this run writes nothing"
                );
                return;
            }
        }
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

/// **The storage worker has the receipt**: its answer is read by the watch,
/// which says in the log what became of it.
pub(crate) fn receipt_handed(answer: mpsc::Receiver<persist::ReceiptWritten>) {
    GATE.state().receipt_answer = Some(answer);
}

/// **The trial is ready: its receipt, once** — asked by the window thread at
/// the first pane text it puts on the glass. `Some` exactly once in a trial
/// whose claim was adopted and whose transaction is still undecided; `None`
/// in every other start and on every later ask.
pub(crate) fn receipt_due() -> Option<ReceiptJob> {
    let (txn, nonce) = update_startup::trial()?;
    let home = update_startup::trial_home()?;
    GATE.hand_receipt().then(|| ReceiptJob {
        path: home.receipt_path(txn, &nonce),
        bytes: Receipt {
            txn,
            nonce,
            pid: std::process::id(),
            version: crate::version::VERSION.to_owned(),
        }
        .encode(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
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
        let root =
            std::env::temp_dir().join(format!("bt-update-trial-{tag}-{}", std::process::id()));
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

    /// RED (U-13) — **a trial reads `Committed` only where the journal says
    /// it, and reads every other decision and every disappearance as an end.**
    ///
    /// From the frozen header alone (F-8, coordinator ruling 2026-09-27):
    /// `outcome` says committed or rolled back, since the class cannot —
    /// `Trial`, `Committed` and `RollbackIntent` share `destructive`, both
    /// retirements `terminal`. A bare header, with no body at all, decides the
    /// same way.
    ///
    /// MUTATION: in `update_txn::trial_sight`, answer `Committed` for a
    /// `terminal` class whatever its outcome.
    #[test]
    fn a_trial_reads_committed_only_where_the_journal_says_it() {
        let cases = [
            (Phase::Moving, TrialSight::Undecided),
            (trial_phase(), TrialSight::Undecided),
            (Phase::Committed, TrialSight::Committed),
            (
                Phase::Retired {
                    outcome: Outcome::Committed,
                },
                TrialSight::Committed,
            ),
            (
                Phase::RollbackIntent {
                    trial: Some(TrialProcess { pid: 1, started: 2 }),
                },
                TrialSight::Ended,
            ),
            (
                Phase::Stuck {
                    trial: None,
                    last_error: "held".to_owned(),
                },
                TrialSight::Ended,
            ),
            (Phase::RolledBack, TrialSight::Ended),
            (Phase::Abandoned, TrialSight::Ended),
            (
                Phase::Retired {
                    outcome: Outcome::RolledBack,
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
            watch(gate, &watched, TXN, Duration::from_millis(5), &|| {
                watch_woken.fetch_add(1, Ordering::SeqCst);
            });
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
                Phase::RollbackIntent {
                    trial: Some(TrialProcess { pid: 1, started: 2 }),
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
            watch(&gate, &journal, TXN, Duration::from_millis(5), &|| {
                woken.fetch_add(1, Ordering::SeqCst);
            });
            assert_eq!(woken.load(Ordering::SeqCst), 0, "{ending:?}");
            assert!(gate.take_released().is_empty());
            assert!(gate.pending().is_empty(), "dropped");
            assert!(gate.defers(true), "shut for the rest of the process");
            assert!(gate.defer(true, Writer::Session));
            assert!(gate.pending().is_empty(), "and nothing is held for later");
        }
        let _ = std::fs::remove_dir_all(&root);
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
        assert_eq!(write(1), Ok(()));
        let first = std::fs::read(&path).unwrap();
        assert!(write(2).is_err(), "the second receipt is refused");
        assert_eq!(std::fs::read(&path).unwrap(), first);
        store.close();
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
