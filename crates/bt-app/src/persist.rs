//! Where `session.json` and `settings.json` live, when they are written, and
//! what a leftover sentinel means.
//!
//! `bt-persist` is deliberately timer-free and path-free: it knows how to read
//! and write two JSON documents given explicit paths, and how to degrade when
//! either is missing or damaged. Everything it left to "the chrome slice that
//! consumes this crate" is here — the actual storage directory, the debounce
//! duration, when `mark_dirty` fires, and who calls `probe_sentinel` at
//! startup.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use bt_platform::admission::{WaitToken, WorkerCtx, admitted, doors};

use bt_persist::{
    BindingOverrideV1, Debouncer, DocumentBytes, ExitState, KEYBINDINGS_SCHEMA_VERSION,
    KeybindingsPlatformV1, KeybindingsV1, ProfilesV1, ReadReport, SessionV1, SettingsV1,
    WriteAlertAction, WriteFailureTracker, create_sentinel, probe_sentinel, read_keybindings,
    read_keybindings_keeping_bytes, read_profiles, read_profiles_keeping_bytes, read_session,
    read_settings, read_settings_keeping, read_settings_keeping_bytes, remove_sentinel,
};

use crate::document_request::{DocumentState, QuitDocument, ReadNews};
use crate::startup::{Settled, SettledOnce};

/// The name the session document wears on disk, which is also what a notice
/// about it has to say out loud — [`KEYBINDINGS_FILE_NAME`]'s rule, one file
/// over.
pub const SESSION_FILE_NAME: &str = "session.json";

/// The single restore-ready generation kept beside [`SESSION_FILE_NAME`].
/// Folio writes this file and never reads it: it is the layout Folio had when
/// the current run started, for a person to put back deliberately if needed.
const PREVIOUS_SESSION_FILE_NAME: &str = "session.prev.json";

/// And the preferences document's, on the same terms.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// **How many refusals in a row a document's writes meet before the reader is
/// told** (review row R4-10; `docs/M2-persistence-schema-v1.md` §5.3).
///
/// Nothing writes a refused document again on a clock (the side-effects
/// contract's C5, owner ruling Q3 2026-10-11): each refusal counted here is the
/// answer to one trigger — a change, a press, a storage pass, a quit. Five is
/// enough to ride out the transient case (an antivirus holding the file open for
/// a moment, a sync client mid-rename) and short enough that a disk that is
/// full, a folder that has gone read-only or a volume that was unplugged is said
/// while the reader can still do something about it.
///
/// **Said once per streak, and a streak ends only at a success** (§5.3, ruled
/// 2026-08-03): a change does not start the count again, so a reader who keeps
/// changing things over a disk that keeps refusing hears it once.
const REFUSALS_BEFORE_TOLD: u32 = 5;

/// **The disk half of a store that writes a whole document on a press** (review
/// row R4-10).
///
/// Every store here used to assign the new value into itself, write, and answer
/// `true` whatever the write did. Two things followed, and both of them were
/// silent. The first is that a failed save was remembered as done: the value the
/// store holds is what the next call compares against, so picking the same row
/// again was an early return and retried nothing — the one gesture a reader
/// makes when something did not take. The second is that nothing anywhere
/// counted, so a folder that had gone read-only produced one line on `stderr`
/// per press, for ever.
///
/// So the value is adopted — the window must show what the reader chose — and
/// whether it is on the disk is the store's own comparison: for a hand-editable
/// document, the document Folio wants against the revision the file is known to
/// hold (`document_request::DocumentState::owed`, design note 2026-10-11 (b)6);
/// for the session, the document the store owes after a refusal
/// (`SessionStore::owes_a_write`).
#[derive(Debug)]
pub(crate) struct DocumentWrites {
    failures: WriteFailureTracker,
    /// Refusals since the last success.
    streak: u32,
    /// Whether this streak has been said: a card is owed once per streak.
    said: bool,
    /// The sentence the reader is owed, until it is taken.
    fault: Option<String>,
    /// How many writes of this document the reader has asked for — the event
    /// the storage watch retries an unheard folder on.
    generation: u64,
}

impl DocumentWrites {
    pub(crate) fn new() -> Self {
        Self {
            failures: WriteFailureTracker::new(),
            streak: 0,
            said: false,
            fault: None,
            generation: 0,
        }
    }

    /// Count a write the reader asked for and the store accepted.
    pub(crate) fn requested(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// How many writes of this document the reader has asked for.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Book one attempt, and answer whether the document is on the disk now.
    ///
    /// `file` is named in both the `stderr` line (§5.3) and the sentence, because
    /// a reader who is told a save failed can only act on it if they are told
    /// which file.
    pub(crate) fn record(&mut self, file: &str, result: Result<(), String>) -> bool {
        let landed = result.is_ok();
        if self.failures.record(landed) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one line per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write {file}: {error}");
        }
        if landed {
            self.streak = 0;
            self.said = false;
            return true;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak >= REFUSALS_BEFORE_TOLD {
            self.say(file);
        }
        false
    }

    /// **A refusal that trying again will not change** — a link, a volume that
    /// cannot exchange two files (design note 2026-10-11 (b)7): said at its
    /// first occurrence, not after a streak — and, like any refusal, once per
    /// streak.
    pub(crate) fn refused_for_good(&mut self, file: &str, error: String) {
        eprintln!("BT_PERSIST could not write {file}: {error}");
        self.failures.record(false);
        self.streak = self.streak.saturating_add(1);
        self.say(file);
    }

    /// Owe the reader the sentence, unless this streak has been said.
    fn say(&mut self, file: &str) {
        if !std::mem::replace(&mut self.said, true) {
            self.fault = Some(crate::i18n::persisted_file_unwritable(file));
        }
    }

    /// **This streak has been said by another card**: nothing more is owed for it.
    pub(crate) fn said_elsewhere(&mut self) {
        self.said = true;
        self.fault = None;
    }

    /// Take the sentence, so a card about it is raised once and not once a frame.
    pub(crate) fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
    }
}

/// **What a hand-editable document's file is known to hold**: the bytes Folio
/// last read from it or wrote to it, and the document they parse to
/// (`docs/plans/design/settings-reread-2026-10-11.md` §3.1, revision (b)10).
///
/// Each of the four stores — settings, keybindings, profiles, pins — holds one,
/// and it is that store's alone: set when the store opens, on every write that
/// lands, and by a re-read that takes the file.
///
/// - At open it is what the read had in hand: no file gives `bytes: None` and
///   the default document; a file refused at start gives the refused bytes and
///   the default document, because that file was not taken and is what the
///   next write replaces (its copy is kept beside it first, review row R4-3).
/// - A landed write sets it to the bytes written and their document.
///
/// **A re-read compares the file's bytes with this, never with the document in
/// memory.** Equal bytes are Folio's own write, or no change at all. The store's
/// in-memory document is ahead of the file whenever a change has not reached the
/// disk — a write that failed, or a process that is not the writer — and a
/// re-read that compared with it would take the older file as an edit and undo
/// the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Revision<T> {
    /// The file's bytes; `None` when there was no file, or none whose bytes
    /// were obtained.
    pub(crate) bytes: Option<Arc<[u8]>>,
    /// The document `bytes` parse to — the default for a refused file.
    pub(crate) document: T,
}

impl<T: PartialEq> Revision<T> {
    /// The revision a store opens with: `document` as the read returned it, and
    /// the bytes the read had in hand.
    pub(crate) fn opened(document: T, bytes: DocumentBytes) -> Self {
        Self {
            bytes: bytes.into_bytes(),
            document,
        }
    }

    /// A write landed: the file holds `bytes`, which are `document`.
    pub(crate) fn landed(&mut self, bytes: Arc<[u8]>, document: T) {
        self.bytes = Some(bytes);
        self.document = document;
    }

    /// Whether a read found exactly the bytes this revision records. Bytes that
    /// were not obtained are never a match: nothing is known about them.
    pub(crate) fn holds(&self, read: &DocumentBytes) -> bool {
        match read {
            DocumentBytes::Absent => self.bytes.is_none(),
            DocumentBytes::Loaded(bytes) | DocumentBytes::Refused { bytes } => {
                self.bytes.as_deref() == Some(&**bytes)
            }
            DocumentBytes::Unobtained => false,
        }
    }

    /// Take a file that is not Folio's own write and that parsed: its bytes
    /// become the revision's, and the answer is whether its document differs.
    ///
    /// A second spelling of the same document — an editor that re-indents — is
    /// a hand edit that changes no field: the bytes are adopted, so the next
    /// read of them is no change, and nothing else moves.
    pub(crate) fn adopt(&mut self, document: T, bytes: DocumentBytes) -> bool {
        self.bytes = bytes.into_bytes();
        if self.document == document {
            return false;
        }
        self.document = document;
        true
    }
}

/// **What a document that would not read owes the reader** — the sentence, and
/// the copy of it that was kept (review row R4-3).
///
/// One function for all six files, because the fix is one rule: the read path
/// now writes the refused bytes beside the file before anything can replace
/// them, and the sentence the reader is shown has to name where they went or the
/// copy might as well not exist. `in_force` is the half that differs per file —
/// what stands in for the damaged document — and it is a closure rather than a
/// parameter because a sentence that called all six "the defaults" would be the
/// vaguest of the six everywhere.
pub(crate) fn read_fault(
    report: &ReadReport,
    file: &str,
    in_force: impl FnOnce(&str) -> String,
) -> Option<String> {
    let ReadReport::FellBackToDefaults { reason, kept } = report else {
        return None;
    };
    eprintln!("BT_PERSIST {file} fell back to defaults: {reason:?} kept={kept:?}");
    let sentence = in_force(file);
    Some(match kept {
        Some(kept) => crate::i18n::persisted_file_kept_copy(
            &sentence,
            &kept
                .file_name()
                .unwrap_or(kept.as_os_str())
                .to_string_lossy(),
        ),
        None => sentence,
    })
}

/// **Whether this process is the one that writes `%APPDATA%\Folio\`** (review
/// row R4-5).
///
/// Two Folio processes over one data directory had no lock and no re-read: each
/// held the whole of `settings.json` and `session.json` from the moment it
/// started and wrote the whole document back, so the second one to write erased
/// everything the first had done since — a preference, a window, every tab.
///
/// **A lock, and — since 2026-09-08 — a hand-over** (§7.59). R4-5 asked only for
/// the lock, and for one release that is all this was: the second process kept
/// its window, kept every gesture in it working, and did not touch the two
/// documents. Whether Folio is a single-instance application was a *product*
/// decision this row deliberately did not take, and the owner took it after
/// 0.2.4: it is. So a launch that finds this claim already held now hands its
/// command line down `crate::launch_wire`'s pipe and exits, and the running
/// Folio opens the tab.
///
/// **The lock is still the thing that decides**, and that is why this function
/// has not changed. A second *process* is started only when this answers `true`
/// — including for `--new-window`, which asks the running Folio for a second
/// window rather than starting a second program. What the safety net now catches
/// is the launches that could not reach the running process at all: no endpoint,
/// nobody listening, an answer that never came. Those still open a window, and
/// that window still writes nothing here.
///
/// A settled answer is asked once and remembered, because the claim is held for
/// the life of the process: the answer cannot change while this process runs,
/// and a second call that took a second claim would refuse itself. A transient
/// stale-claim sweep is not an answer and is asked again.
pub fn is_storage_writer() -> bool {
    is_writer_of(&storage_dir())
}

/// **Whether this process is the one that writes `directory`** — the question
/// above, asked of the directory that is about to be written rather than of the
/// one this process happens to own.
///
/// The two are the same question for the product, where every store opens under
/// `%APPDATA%\Folio`, and they are not the same question for anything opened
/// anywhere else: a claim is on a *directory*, and a store that answered with
/// [`is_storage_writer`] was answering about a directory it never touches. That
/// is a wrong answer in both directions — it refuses to write a directory nobody
/// holds because some other Folio holds the data directory, and it writes into a
/// directory another process does hold.
///
/// **One claim per directory, taken once and held for the life of the process.**
/// Two stores over one directory are one writer rather than a first one and a
/// refused second: the claim is a kernel name, so a second attempt at it — from
/// this thread or any other — is refused while the first is held, and a store
/// that asked again would refuse itself. That is `is_storage_writer`'s own
/// argument for remembering the answer, one directory wider; the table below is
/// keyed by [`bt_platform::instance::claim_name`] rather than by the path so
/// that two spellings of one directory are one row, which is the same folding
/// the kernel name itself is under.
///
/// **A settled refusal is remembered too** (self-update R-3, as refined by
/// revision 2026-09-25 (b)): a process told once that it is not the writer is
/// not made the writer later by this question, because everything it opened in
/// between was opened as a non-writer. `Sweeping` is not a refusal of this
/// directory and is not remembered. The one road into the table for a claim
/// taken after the first ask is [`adopt_claim`], and it does not overwrite an
/// answer.
pub(crate) fn is_writer_of(directory: &Path) -> bool {
    let name = bt_platform::instance::claim_name(directory);
    let mut table = claim_table();
    if let Some(answer) = table.get(&name) {
        return answer.is_some();
    }
    let answer = bt_platform::instance::try_claim_data_directory(directory);
    remember_claim_answer(&mut table, name, answer)
}

/// Keep a settled claim answer for this process. `Sweeping` is not an answer
/// about the directory: the launch can take its existing hand-over road, and
/// the next [`is_writer_of`] ask goes to the platform again instead of
/// inheriting a refusal from the sweep.
fn remember_claim_answer(
    table: &mut HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>,
    name: String,
    answer: Result<bt_platform::instance::DataDirectoryClaim, bt_platform::instance::ClaimRefusal>,
) -> bool {
    match answer {
        Ok(claim) => {
            table.insert(name, Some(claim));
            true
        }
        Err(bt_platform::instance::ClaimRefusal::Sweeping) => false,
        Err(
            bt_platform::instance::ClaimRefusal::Held
            | bt_platform::instance::ClaimRefusal::QueryDenied(_),
        ) => {
            table.insert(name, None);
            false
        }
    }
}

/// **Let go of every claim this process holds** (0.4.6 U-34): O at its very
/// end, after an update's hand-over, so that the start its exit guard makes
/// takes the claim and is the writer — not a launch handed back to this
/// process, which is leaving. Each row then answers "not the writer": nothing
/// is written after this.
///
/// It never waits for the table: it also runs in the panic hook, possibly on the
/// thread that holds it — then the claims go with the process, a moment later.
pub(crate) fn let_go_of_every_claim() {
    let mut table = match claims().try_lock() {
        Ok(table) => table,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    for row in table.values_mut() {
        *row = None;
    }
}

/// **The claim table** — one row per claim name, holding either the claim this
/// process took (it is the writer, and the guard lives here for the life of
/// the process) or `None` (it asked and was refused). Written by
/// [`is_writer_of`] on a first settled ask and by [`adopt_claim`]; a transient
/// sweep leaves no row. Read by nothing else.
fn claim_table() -> std::sync::MutexGuard<
    'static,
    HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>,
> {
    claims()
        .lock()
        .expect("the claim table is locked to read or take one entry and nothing else")
}

/// The table [`claim_table`] locks.
fn claims() -> &'static Mutex<HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>> {
    static CLAIMS: OnceLock<
        Mutex<HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>>,
    > = OnceLock::new();
    CLAIMS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// **Ask for the claim on `directory` now, and remember nothing** (§C.7 of
/// `docs/plans/design/self-update-2026-09-16.md`).
///
/// For a caller that waits for the claim — the updated build, started while
/// the old one is still letting go, asks again until it is handed the claim
/// or its deadline passes. [`is_writer_of`] cannot be that question: it
/// remembers its first answer, so one refusal would be the answer for ever.
/// This one goes to the platform every time and leaves the table alone; the
/// claim it hands back is the caller's until it gives it to [`adopt_claim`].
///
/// The refusal distinguishes a live holder, a transient sweep, and a question
/// the platform did not answer ([`bt_platform::instance::ClaimRefusal`]). The
/// first two are worth asking again; the last is not evidence that anybody
/// holds anything.
pub(crate) fn try_claim(
    directory: &Path,
) -> Result<bt_platform::instance::DataDirectoryClaim, bt_platform::instance::ClaimRefusal> {
    bt_platform::instance::try_claim_data_directory(directory)
}

/// **Make a claim this process already holds the answer [`is_writer_of`]
/// gives, with no gap** (§C.7).
///
/// One lock of the table, one insert under
/// [`bt_platform::instance::claim_name`] — the key `is_writer_of` asks under,
/// so every spelling of the directory finds the row. The guard moves from the
/// caller into the table without ever being dropped, so there is no instant at
/// which the claim is free for another process to take, and no instant at
/// which a caller of `is_writer_of` could be told "not the writer" and have the
/// table remember it. Call it before anything asks `is_writer_of` of this
/// directory.
///
/// **A row already there is a programming error**, because it means something
/// asked first. A refusal already remembered stays remembered — a non-writer
/// must not become the writer mid-run (revision (b)'s refinement of R-3) — and
/// the adopted claim is dropped, which lets it go. Debug builds and tests stop
/// on it; a release build keeps the earlier answer and says so in the log.
pub(crate) fn adopt_claim(directory: &Path, claim: bt_platform::instance::DataDirectoryClaim) {
    let name = bt_platform::instance::claim_name(directory);
    // The earlier answer, if there was one — decided under the lock, reported
    // after it, so that a debug stop never leaves the table poisoned.
    let earlier = match claim_table().entry(name.clone()) {
        std::collections::hash_map::Entry::Vacant(row) => {
            row.insert(Some(claim));
            None
        }
        std::collections::hash_map::Entry::Occupied(row) => Some(row.get().is_some()),
    };
    if let Some(was_writer) = earlier {
        eprintln!(
            "BT_PERSIST a claim was adopted for {name} after this process had already answered \
             whether it writes there (writer={was_writer}); the earlier answer stands"
        );
    }
    debug_assert!(
        earlier.is_none(),
        "adopt_claim for {name} after is_writer_of had already answered for it"
    );
}

/// Whether this process may write the document at `path` — [`is_writer_of`]
/// asked of the directory the document lives in, because that is what a claim is
/// on.
pub(crate) fn is_writer_of_document(path: &Path) -> bool {
    is_writer_of(path.parent().unwrap_or_else(|| Path::new("")))
}

/// docs/M2-persistence-schema-v1.md §5.1 rules "debounce roughly 1-2 seconds
/// after a meaningful change", and leaves the exact figure to the call site.
/// The slower end of the band: a session write is never urgent, and every
/// hundred milliseconds spent waiting is a divider drag that does not turn into
/// its own disk write.
const SESSION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// **How long anything in this process waits for one session write to land**
/// (T-QUIT-HAS-A-DEADLINE).
///
/// An `atomic_write` of `session.json` is a `File::create`, a `write_all`, a `sync_all` and a
/// `rename` under `%APPDATA%` — a path the reader is free to have redirected onto a network
/// share or a cloud-sync folder, where the `fsync` is a round trip with no bound this process
/// can state. Three seconds is far longer than an honest local write and short enough that a
/// quit does not look wedged; past it the answer is "this did not land", which is a fact the
/// caller acts on rather than a wait it cannot leave.
///
/// **The product's budget, and every product store's** — each [`SessionWriter`] carries it as
/// its own `budget` from [`SessionWriter::open`]; only a test may give one store another
/// ([`SessionStore::set_save_budget`]).
pub(crate) const SESSION_SAVE_BUDGET: Duration = Duration::from_secs(3);

/// How often the writer's bounded retirement asks whether the storage worker has ended.
const SESSION_JOIN_POLL: Duration = Duration::from_millis(2);

/// What a store with nowhere to send a document says (release review X-9).
///
/// `spawn_at_priority` is `Builder::spawn` with a priority set inside the thread, so this is the
/// operating system refusing a thread — which is a state, not a verdict about the disk. The
/// document stays owed and the next hand-over asks for a thread again.
const NO_WRITER_THREAD: &str = "the session writer could not be started";

/// What a store whose writer thread has gone says about a document it was
/// waiting to hear about: nothing is on its way, so this is a refusal.
const WRITER_STOPPED: &str = "the session writer stopped before this document reached the disk";

/// What a hand-editable document's store says when no storage worker took its
/// request (`document_request::DocumentState::send_owed`): a refusal, and the
/// next trigger asks for a worker again.
pub(crate) const NO_STORAGE_WORKER: &str = "the storage worker could not be started";

/// What it says when the worker ended before answering a request it took.
pub(crate) const STORAGE_WORKER_GONE: &str =
    "the storage worker stopped before this document reached the disk";

/// What a door says when the storage worker's ledger already owes [`STORAGE_BOUND`]
/// requests (T-STORAGE-WORKER-ON-THE-SEAM, R1): a refusal at the door, and the next
/// trigger asks again. The fixed askers never reach it ([`STRUCTURAL_ASKERS`]); the
/// preview refuses a save at its own desk first (`preview::PREVIEW_SAVES_OWED`).
pub(crate) const STORAGE_LANE_FULL: &str =
    "the storage worker already has as many requests on their way as it takes";

/// **What a request the storage worker failed on says** (T-STORAGE-WORKER-ON-THE-SEAM, R2):
/// its task panicked, the ledger has said so once, and the worker went on. `failure` is the
/// task's failure in the seam's words (`lane::TaskFailure`).
pub(crate) fn storage_task_failed(failure: &str) -> String {
    format!("the storage worker failed on this request: {failure}")
}

/// **How many requests the fixed askers can have outstanding on the storage worker at once**
/// while it answers (T-STORAGE-WORKER-ON-THE-SEAM, R1): the session's three — one autosave
/// (`SessionWriter::busy` keeps the next in the pending slot), a quit's judged write and an
/// update's final one, each sent whatever is in flight; the launch backup, one per launch; an
/// update trial's receipt, one in flight; an update's hand-over, one per quit; and each of the
/// four hand-editable documents' stores (`document_request::DocumentState`) one write, one
/// re-read and one quit copy — 3 + 1 + 1 + 1 + 4 × 3. A request counts from its admission
/// until its terminal is taken: the session's, the backup's, the receipt's and the hand-over's
/// are left to the lane's sink at admission (their readers read their own roads), so they
/// count while owed; a store's until the store reads its answer.
pub(crate) const STRUCTURAL_ASKERS: usize = 18;

/// **How many requests the storage worker's ledger owes at once** (R1): the fixed askers'
/// [`STRUCTURAL_ASKERS`] and the preview's desk, [`crate::preview::PREVIEW_SAVES_OWED`] — so a
/// flood of preview saves never crowds a quit's session write out of the queue.
pub(crate) const STORAGE_BOUND: usize = STRUCTURAL_ASKERS + crate::preview::PREVIEW_SAVES_OWED;

/// The storage worker's name in its ledger's lines (`diagnostics.log`).
const STORAGE_LANE: &str = "the storage worker";

/// **One storage request's name**, minted by the lane's ledger ([`StorageLane`]) and unique for
/// the life of the lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct StorageId(u64);

impl From<u64> for StorageId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

/// **How one storage request ended**: done (its answer, if it has one, is on its own road),
/// failed (its task panicked), or the worker gone with it owed.
pub(crate) type StorageTerminal = crate::lane::Terminal<StorageId, (), std::convert::Infallible>;

/// The lane's sink for a request whose asker has gone, shared by every request it admits.
type StorageSink = Arc<dyn Fn(StorageId, StorageTerminal) + Send + Sync>;

/// **What a storage request is owed when its asker has gone** (C6): the lane's sink, which
/// lets the terminal go — a success with nobody left to tell, a failure the ledger has said.
pub(crate) struct StorageOwed(StorageSink);

impl crate::lane::Owed<StorageId> for StorageOwed {
    type Answer = ();
    type Error = std::convert::Infallible;

    fn sink(self, id: StorageId, terminal: StorageTerminal) {
        (self.0)(id, terminal);
    }
}

/// The storage worker's ledger.
type StorageLedger = crate::lane::Ledger<StorageId, StorageOwed>;

/// One admitted storage request, held by its asker beside the road its answer comes on.
pub(crate) type StorageRequest = crate::lane::Request<StorageId, StorageOwed>;

/// **Why a door did not admit a request**, before the asker says it in its own words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NotAdmitted {
    /// No worker serves the lane: none was started, the operating system refused one, it has
    /// ended, or the lane was closed.
    NoWorker,
    /// The ledger already owes [`STORAGE_BOUND`] requests.
    Full,
}

impl NotAdmitted {
    /// The refusal's words: `no_worker`, the asker's own sentence for a lane no worker serves,
    /// or [`STORAGE_LANE_FULL`].
    pub(crate) fn said(self, no_worker: &str) -> String {
        match self {
            Self::NoWorker => no_worker.to_owned(),
            Self::Full => STORAGE_LANE_FULL.to_owned(),
        }
    }

    fn of(refused: crate::lane::Refused) -> Self {
        match refused {
            crate::lane::Refused::Full { .. } => Self::Full,
            crate::lane::Refused::Faulted(_) => Self::NoWorker,
            crate::lane::Refused::TwinEnded | crate::lane::Refused::NotStarted(_) => {
                unreachable!("the storage lane joins nothing and starts no worker at its doors")
            }
        }
    }
}

/// **One request on the storage worker, as its asker holds it** (T-STORAGE-WORKER-ON-THE-SEAM):
/// the road its answer comes on, and the identity the lane's ledger minted for it.
///
/// The worker sends the answer on the road and then publishes the identity's terminal, so the
/// road is read only once the terminal has been taken: `Done` says the answer is on the road; a
/// failed task (`Failed`, R2) or the worker's end (`WorkerGone`) is the request's end, said in
/// its words, whatever the road holds. The worker lets a job — and with it its road — go only
/// after its terminal is published, so a road read closed after a `Done` is a worker that ended
/// in between, and a road the door answered itself (no identity: refused there) is read as it
/// is. The asker's going drops it, and its terminal goes to the lane's sink.
pub(crate) struct OnTheWorker<A> {
    road: mpsc::Receiver<A>,
    request: Option<StorageRequest>,
}

/// What an asker hears of one request on the storage worker ([`OnTheWorker::hear`]).
#[derive(Debug)]
pub(crate) enum Reply<A> {
    /// Nothing yet.
    Waiting,
    Answered(A),
    /// Its task failed, in these words ([`storage_task_failed`]).
    Failed(String),
    /// The worker ended with it unanswered.
    Gone,
}

impl<A> OnTheWorker<A> {
    /// A request the lane admitted as `request`, answered on `road`.
    pub(crate) fn admitted(road: mpsc::Receiver<A>, request: StorageRequest) -> Self {
        Self {
            road,
            request: Some(request),
        }
    }

    /// A request refused at the door, whose refusal the door put on `road` itself.
    pub(crate) fn answered_at_the_door(road: mpsc::Receiver<A>) -> Self {
        Self {
            road,
            request: None,
        }
    }

    /// **What has become of it**, never waiting.
    pub(crate) fn hear(&mut self) -> Reply<A> {
        if let Some(request) = &mut self.request {
            match request.take() {
                None => return Reply::Waiting,
                Some(crate::lane::Terminal::Done(())) => self.request = None,
                Some(crate::lane::Terminal::Failed(failure)) => {
                    self.request = None;
                    return Reply::Failed(storage_task_failed(&failure.to_string()));
                }
                Some(_) => {
                    self.request = None;
                    return Reply::Gone;
                }
            }
        }
        match self.road.try_recv() {
            Ok(answer) => Reply::Answered(answer),
            Err(mpsc::TryRecvError::Empty) => Reply::Waiting,
            Err(mpsc::TryRecvError::Disconnected) => Reply::Gone,
        }
    }

    /// [`Self::hear`], until it hears something, within the suite's patience
    /// (`lane::PATIENCE`) — a test's way to let the worker finish. A lost answer fails the test
    /// that awaited `what`, by name, rather than hanging it.
    #[cfg(test)]
    pub(crate) fn within_patience(&mut self, what: &str) -> Reply<A> {
        let mut heard = Reply::Waiting;
        crate::lane::wait_until(what, || {
            heard = self.hear();
            !matches!(heard, Reply::Waiting)
        });
        heard
    }
}

/// **How the storage worker wakes the loop after a document answer**
/// (`AppEvent::DocumentsAnswered`): installed once, by the process's first
/// window ([`install_documents_wake`]). Unset in a test, where no loop waits.
static DOCUMENTS_WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Install [`DOCUMENTS_WAKE`]. The first install stands.
pub(crate) fn install_documents_wake(wake: impl Fn() + Send + Sync + 'static) {
    let _ = DOCUMENTS_WAKE.set(Box::new(wake));
}

/// Wake the loop: a document answer is in its channel, or the worker ended
/// and every request it held is now answered by its absence. A product lane's
/// wake ([`StorageLane::wake`]).
fn wake_documents() {
    if let Some(wake) = DOCUMENTS_WAKE.get() {
        wake();
    }
}

/// The lane's wake: what tells the loop a document answer is in its channel.
type LaneWake = Arc<dyn Fn() + Send + Sync>;

/// What the storage worker's queue carries: an admitted job with its identity, or the token
/// that wakes the worker for a job admitted ahead of the queue ([`StorageLane::submit_ahead`]).
enum Queued {
    Job(StorageId, Box<StorageJob>),
    Ahead,
}

/// The jobs admitted ahead of the queue, with their identities.
type AheadJobs = Arc<Mutex<std::collections::VecDeque<(StorageId, StorageJob)>>>;

/// **The storage worker's end, however it ends** (review round 3, C2). It owns
/// the worker's channel ends and the jobs it has taken and not begun, so the
/// queue's receiver lives exactly as long as it. Under the queue's lock — the lock
/// every door admits under — it marks the lane down and drops every job still
/// queued, ahead or taken (the one being served, held by the loop, went with the
/// loop's unwinding), so each such request's answer channel disconnects; no request can
/// be admitted after that, because admission asks `up` under the same lock. The
/// session receipts' channel closes after them, so a session write the worker
/// took and will not answer reads as the worker's end on the turn the wake brings
/// ([`SessionStore::take_receipts`]). The executor then tells the lane's ledger
/// ([`StorageEnds`]), which ends every request still owed `WorkerGone`, and only
/// then is the loop woken: the wake follows every terminal.
struct WorkerEnd {
    /// The queue's receiving end.
    incoming: mpsc::Receiver<Queued>,
    /// Where the session's receipts go; `None` only once the end has closed it.
    receipts: Option<mpsc::Sender<SessionWriteReceipt>>,
    /// The queue's sending end, weakly: the worker must not keep its own queue
    /// open.
    requests: std::sync::Weak<Mutex<Option<mpsc::Sender<Queued>>>>,
    up: Arc<std::sync::atomic::AtomicBool>,
    /// The jobs admitted ahead of the queue ([`StorageLane::submit_ahead`]): dropped
    /// with the queued ones, so each such request's answer channel disconnects too.
    ahead: AheadJobs,
    /// **The jobs the worker has taken off the queue and not yet begun** — the ones
    /// admitted ahead, then the queue's — in the order it serves them. The one it
    /// serves is taken out of here and held by the loop itself until its terminal
    /// is published; these are let go — their roads with them — here.
    in_hand: std::collections::VecDeque<(StorageId, StorageJob)>,
}

impl Drop for WorkerEnd {
    fn drop(&mut self) {
        {
            let requests = self.requests.upgrade();
            let _admission = requests.as_deref().map(lane_lock);
            self.up.store(false, std::sync::atomic::Ordering::Release);
            for job in self.incoming.try_iter() {
                drop(job);
            }
            lane_lock(&self.ahead).clear();
            self.in_hand.clear();
        }
        drop(self.receipts.take());
    }
}

/// **What the executor tells when the storage worker ends** (`lane::Ends`): the ledger ends
/// every request still owed `WorkerGone`, says so once, faults the lane — every later
/// admission is refused — and wakes the loop; a worker that ends owing nothing is announced by
/// the same wake, as it always was. A start the thread door refused is no end: the worker
/// never ran, [`SessionWriter::start`] answers it, and the next hand-over starts it again.
struct StorageEnds {
    ledger: Arc<StorageLedger>,
    wake: LaneWake,
    /// Set once the end is told, for [`SessionWriter::close`]'s bounded wait.
    ended: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::lane::Ends for StorageEnds {
    fn worker_ended(&self, cause: crate::lane::WorkerEnd) {
        if matches!(cause, crate::lane::WorkerEnd::NotStarted(_)) {
            return;
        }
        let owed = self.ledger.earliest_owed().is_some();
        self.ledger.worker_ended(cause);
        if !owed {
            (self.wake)();
        }
        self.ended.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// What a save that ran out of time says, on `stderr` and to the caller, naming the writer's
/// `budget`. The bound that ended it may instead have been a quit's `until` or an update's
/// receipt deadline, and a wait that was not admitted did not wait at all; the sentence names
/// the writer's budget in every case, the product's three seconds in the product.
///
/// One sentence for both, because they are the same fact: the document in hand did not reach
/// the disk, and whatever the last completed save left there is still what the next start
/// reads.
fn save_did_not_finish(budget: Duration) -> String {
    let seconds = budget.as_secs();
    format!("session save did not finish within {seconds} s; the last completed save stands")
}

/// What a quit whose one budget was spent before its session write says (T-QUIT-DOCUMENTS).
fn quit_budget_spent() -> String {
    let seconds = SESSION_SAVE_BUDGET.as_secs();
    format!(
        "the quit's {seconds} s were spent before the session could be written; the last \
         completed save stands"
    )
}

/// Say it once, in the words the log is read in.
fn report_save_did_not_finish(budget: Duration) {
    eprintln!("Folio: {}", save_did_not_finish(budget));
}

/// **Why a judged save did not land, and what a quit is entitled to do about it**
/// (T-QUIT-TIMEOUT-PROCEEDS, release review X-8).
///
/// The two arms are one word apart and a whole policy apart, which is why they are a type
/// rather than a string:
///
/// - [`Self::Refused`] is the disk saying no — no directory, no room, a volume that went away,
///   a document that would not serialize. Nothing landed, nothing is on its way, and nothing
///   about waiting longer would have changed it. A quit stops on this: hiding a window over a
///   file that was refused is the session gone.
/// - [`Self::TimedOut`] is the writer still being *inside* the write when its budget ran out.
///   The last completed save is what a reader would find on the disk, this run's document may
///   yet land on top of it, and neither of those is a reason to keep a window open in front of
///   somebody who asked to leave. A quit goes on — with `session.lock` left standing, because
///   this run cannot claim it saw the save finish.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveRefusal {
    /// The write was refused, and the sentence to say about it.
    Refused(String),
    /// The write did not finish inside the writer's budget ([`SESSION_SAVE_BUDGET`]), and the
    /// sentence to say.
    TimedOut(String),
}

impl SaveRefusal {
    /// What to say about it.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Refused(message) | Self::TimedOut(message) => message,
        }
    }

    /// **Whether a quit may carry on past this one.** The whole of X-8, asked as a question.
    #[must_use]
    pub fn quit_may_proceed(&self) -> bool {
        matches!(self, Self::TimedOut(_))
    }
}

/// One document on its way to the disk, addressed by the order it was decided in.
struct SessionWriteRequest {
    generation: u64,
    path: PathBuf,
    bytes: Vec<u8>,
}

/// What became of one of them.
struct SessionWriteReceipt {
    generation: u64,
    result: Result<(), String>,
}

/// The session document this run read, on its way to the one-generation
/// backup. `source` lends its permissions; `path` is always its sibling.
struct SessionBackupRequest {
    source: PathBuf,
    path: PathBuf,
    bytes: Arc<[u8]>,
}

/// **One job for the storage worker**: a session document, an update
/// trial's receipt (U-13), an update's hand-over to its applier (U-21), or a
/// hand-editable document's request — its conditional write or its stable
/// read (`document_request`, T-STORAGE-WORKER-REQUEST).
enum StorageJob {
    Session(SessionWriteRequest),
    /// Queued before this run's first `Session` job, on the same one writer.
    /// It has no receipt because failure is non-fatal and reported once here.
    SessionBackup(SessionBackupRequest),
    /// Written create-new through the `install_txn` door, and answered on its
    /// own channel: a receipt is not a session document and has no generation.
    Receipt {
        job: crate::update_trial::ReceiptJob,
        answer: mpsc::Sender<ReceiptWritten>,
    },
    /// `Handoff` written durably through the `install_txn` door, then the
    /// applier started (`update_handoff::perform`), answered on its own
    /// channel. Here and not on the window thread, because a durable write
    /// waits for the device.
    ///
    /// `job` is taken by the one serving that performs it; the answer's road
    /// stays with the job until its terminal is published.
    Handoff {
        job: Option<crate::update_handoff::HandoffJob>,
        answer: mpsc::Sender<crate::update_handoff::HandedOff>,
    },
    /// A hand-editable document, written only if the file still holds what
    /// the request expects, and answered on its own channel.
    Document(crate::document_request::DocumentJob),
    /// A hand-editable document, read while it stands still.
    Read(crate::document_request::ReadJob),
    /// A quit's copy of a hand-editable document into the `recovered` folder,
    /// answered on its own channel (design note 2026-10-11, revision (c)2).
    Recovered(crate::document_request::RecoveredJob),
}

impl StorageJob {
    /// **Whether serving it wakes the loop**: every answer the loop reads is followed by the
    /// wake (`AppEvent::DocumentsAnswered`). The launch backup has no reader, and a trial's
    /// receipt is read by the trial's watch on its own turn (ruled 2026-10-14), so neither
    /// wakes it.
    fn wakes(&self) -> bool {
        !matches!(self, Self::SessionBackup(_) | Self::Receipt { .. })
    }
}

/// What serving one job came to, for the worker's loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Served {
    Done,
    /// A session receipt found nobody reading the receipts: the session store is gone, and
    /// the worker stops serving.
    ReceiptsGone,
}

/// **Serve one storage job on the worker** (`worker`, the capability the thread door lent its
/// body), its answer put on the job's own road. Runs inside the request's `lane::task`; the
/// job is lent, so its road outlives a panic here until the request's terminal is published.
fn serve_storage_job(
    worker: &WorkerCtx,
    job: &mut StorageJob,
    receipts: &mpsc::Sender<SessionWriteReceipt>,
) -> Served {
    match job {
        StorageJob::Session(request) => {
            let result = bt_persist::atomic_write(&request.path, &request.bytes)
                .map_err(|error| error.to_string());
            let receipt = SessionWriteReceipt {
                generation: request.generation,
                result,
            };
            // The receipt is in its channel before the wake, which the loop answers by taking
            // it (`SessionStore::take_receipts`).
            if receipts.send(receipt).is_err() {
                return Served::ReceiptsGone;
            }
        }
        StorageJob::SessionBackup(request) => {
            if let Err(error) = bt_persist::atomic_write_carrying_from(
                &request.path,
                &request.bytes,
                &request.source,
            ) {
                crate::diagnostics::note(&format!(
                    "BT_PERSIST {PREVIOUS_SESSION_FILE_NAME} backup failed: {error}"
                ));
            }
        }
        // Read by the trial's watch, on its own turn (`update_trial::watch`).
        StorageJob::Receipt { job, answer } => {
            let _ = answer.send(write_receipt(job));
        }
        // Read by an update's way out on the turn the wake brings
        // (`FolioApp::hand_the_update_over`).
        StorageJob::Handoff { job, answer } => {
            if let Some(job) = job.take() {
                let _ = answer.send(crate::update_handoff::perform(job));
            }
        }
        StorageJob::Document(job) => crate::document_request::serve_document(worker, job),
        StorageJob::Read(job) => crate::document_request::serve_read(worker, job),
        StorageJob::Recovered(job) => crate::document_request::serve_recovered(worker, job),
    }
    Served::Done
}

/// **A door the lane contract's suite serves its jobs through** instead of
/// [`serve_storage_job`] (`contract_adapter`): it runs where the product's does, inside each
/// request's task, so the suite's gate holds, counts and fails what the product's would run.
#[cfg(test)]
type TestDoor = Arc<
    dyn Fn(&WorkerCtx, &mut StorageJob, &mpsc::Sender<SessionWriteReceipt>) -> Served + Send + Sync,
>;

/// **What became of a trial's receipt**, and the thread that wrote it.
#[derive(Debug)]
pub(crate) struct ReceiptWritten {
    /// The role of the thread the receipt was written on — the storage
    /// worker's, never the window's (F-14).
    pub(crate) by: bt_platform::admission::Role,
    pub(crate) result: Result<(), String>,
}

/// Write a trial's receipt, on the thread that calls this — the storage
/// worker. Create-new; an existing receipt is never written over unless it is
/// an earlier attempt of the same trial (`update_trial::write_receipt`).
fn write_receipt(job: &crate::update_trial::ReceiptJob) -> ReceiptWritten {
    let result = crate::update_trial::write_receipt(job);
    ReceiptWritten {
        by: bt_platform::admission::role(),
        result,
    }
}

/// The channel ends one writer thread works from: requests in, receipts out.
struct SessionWriterEnds {
    incoming: mpsc::Receiver<Queued>,
    outgoing: mpsc::Sender<SessionWriteReceipt>,
}

/// **The storage worker's queue, as every asker holds it**
/// (T-STORAGE-WORKER-REQUEST; on the completion seam since
/// T-STORAGE-WORKER-ON-THE-SEAM).
///
/// One queue and one thread for the process: the session store holds it, and
/// so do the four hand-editable documents' stores, each a clone of the same
/// lane. That is what makes per-file order a property rather than a promise:
/// every request naming one path is served in the order it was queued,
/// whichever asker queued it (design note 2026-10-11, revision (c)3 item 5).
///
/// **Built on the completion seam** (`crate::lane`; the side-effects contract's
/// C1–C7): the worker is started by a [`crate::lane::Executor`] (the thread
/// door's in the product, the suite's scripted one under test); every request
/// of every kind is admitted by the lane's [`crate::lane::Ledger`], which mints
/// its identity ([`StorageId`]) or refuses it at the door — at
/// [`STORAGE_BOUND`], or once no worker serves the lane — under the queue's
/// lock; each job runs inside [`crate::lane::task`], so a panic in one is that
/// request's `Failed`, said once, and the worker goes on (R2); and every
/// request ends exactly once, `Done`, `Failed` or — at the worker's end, from
/// the ledger — `WorkerGone`. What stays the worker's own: one FIFO with its one
/// `recv`, the jobs admitted ahead of it ([`Self::submit_ahead`]), and each
/// kind's own answer road, which its asker reads where it lives
/// ([`OnTheWorker`]).
///
/// The thread is started by [`SessionWriter::start`], the only place one is,
/// on the first request any asker hands over, and again on a later one while
/// there is none. A worker that has ended takes no request again: the request
/// is refused where it was handed over.
#[derive(Clone)]
pub(crate) struct StorageLane {
    shared: Arc<LaneShared>,
}

/// What the lane's clones share.
struct LaneShared {
    /// The queue's sending end; `None` once the session store closed the lane.
    /// Its lock is the one every request is admitted under, and the one the
    /// worker's end takes ([`WorkerEnd`]).
    requests: Arc<Mutex<Option<mpsc::Sender<Queued>>>>,
    /// The ends the thread works from, until one thread has them (release
    /// review X-9).
    ends: Arc<Mutex<Option<SessionWriterEnds>>>,
    /// The thread, once one runs.
    worker: Mutex<LaneWorker>,
    /// **Whether a worker serves the queue** — written only under the queue's
    /// lock: set by [`SessionWriter::start`] before its thread starts, cleared if
    /// the spawn is refused and by the worker's end ([`WorkerEnd`]). A request is
    /// admitted only when it is set, asked under the same lock, so the document
    /// doors never start a thread or wait on the worker's lock (C1), and no door
    /// admits a request after the worker's end (C2).
    up: Arc<std::sync::atomic::AtomicBool>,
    /// How the loop is woken after an answer ([`StorageLane::wake`]).
    wake: LaneWake,
    /// **The jobs admitted ahead of the queue** — an update trial's receipt
    /// ([`StorageLane::submit_ahead`]); written under the queue's lock.
    ahead: AheadJobs,
    /// The worker factory: the thread door below normal in the product.
    executor: crate::lane::Executor,
    /// Mints and ends every request the lane admits.
    ledger: Arc<StorageLedger>,
    /// What a request whose asker has gone is owed.
    sink: StorageSink,
    /// Whether the worker the executor started has ended and told the ledger.
    ended: Arc<std::sync::atomic::AtomicBool>,
    /// The suite's door, served in place of [`serve_storage_job`].
    #[cfg(test)]
    door: Option<TestDoor>,
    /// The worker thread's identity, once it runs — for a test.
    #[cfg(test)]
    thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
}

/// Whether a thread serves the lane.
enum LaneWorker {
    /// None yet, or none any more: the lane was closed or its thread let go.
    Waiting,
    /// The executor started one.
    Running,
    /// A test serves the queue itself ([`StorageLane::held`]).
    #[cfg(test)]
    ByTest,
}

/// One of the lane's locks, taken whatever a panic elsewhere left in it: each
/// holds a queue end or a handle, never an invariant a panic could break
/// halfway.
fn lane_lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The parts one lane is made of, before they are shared.
struct LaneParts {
    requests: Option<mpsc::Sender<Queued>>,
    ends: Option<SessionWriterEnds>,
    worker: LaneWorker,
    up: bool,
    wake: LaneWake,
    executor: crate::lane::Executor,
    say: Box<dyn Fn(&str) + Send + Sync>,
    sink: StorageSink,
}

impl LaneShared {
    /// A lane of `parts`, its ledger saying through `say` and waking through the lane's wake.
    fn of(parts: LaneParts) -> Self {
        let LaneParts {
            requests,
            ends,
            worker,
            up,
            wake,
            executor,
            say,
            sink,
        } = parts;
        let ledger_wake = Arc::clone(&wake);
        let ledger = crate::lane::Ledger::new(
            STORAGE_LANE,
            Some(STORAGE_BOUND),
            crate::lane::Saying::PerRequest,
            say,
            move || ledger_wake(),
        );
        Self {
            requests: Arc::new(Mutex::new(requests)),
            ends: Arc::new(Mutex::new(ends)),
            worker: Mutex::new(worker),
            up: Arc::new(std::sync::atomic::AtomicBool::new(up)),
            wake,
            ahead: Arc::default(),
            executor,
            ledger,
            sink,
            ended: Arc::default(),
            #[cfg(test)]
            door: None,
            #[cfg(test)]
            thread: Arc::default(),
        }
    }
}

/// The product's sink for a storage request whose asker has gone: nothing to tell.
fn let_go(_id: StorageId, _terminal: StorageTerminal) {}

impl StorageLane {
    /// A lane and the session receipts its worker sends, with no thread yet.
    fn fresh() -> (Self, mpsc::Receiver<SessionWriteReceipt>) {
        Self::fresh_with(
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            Arc::new(wake_documents),
            Box::new(crate::diagnostics::note),
            Arc::new(let_go),
        )
    }

    /// [`Self::fresh`], its worker started by `executor`, woken through `wake`, its ledger's
    /// lines said through `say` and a gone asker's request ended in `sink`.
    fn fresh_with(
        executor: crate::lane::Executor,
        wake: LaneWake,
        say: Box<dyn Fn(&str) + Send + Sync>,
        sink: StorageSink,
    ) -> (Self, mpsc::Receiver<SessionWriteReceipt>) {
        let (requests, incoming) = mpsc::channel::<Queued>();
        let (outgoing, receipts) = mpsc::channel::<SessionWriteReceipt>();
        let lane = Self {
            shared: Arc::new(LaneShared::of(LaneParts {
                requests: Some(requests),
                ends: Some(SessionWriterEnds { incoming, outgoing }),
                worker: LaneWorker::Waiting,
                up: false,
                wake,
                executor,
                say,
                sink,
            })),
        };
        (lane, receipts)
    }

    /// **A lane nobody serves**: every job handed over is queued and stays
    /// there, for as long as the [`UnservedJobs`] it answers with is held — a
    /// test outside this module that hands a store its answers itself.
    #[cfg(test)]
    pub(crate) fn unserved() -> (Self, UnservedJobs) {
        let (lane, jobs) = Self::held();
        (lane, UnservedJobs { _jobs: jobs })
    }

    /// **A lane whose queue a test outside this module reads** ([`ScriptedJobs`]):
    /// every job handed over is there, in the order it was queued, and nothing
    /// else takes it.
    #[cfg(test)]
    pub(crate) fn scripted() -> (Self, ScriptedJobs) {
        let (lane, jobs) = Self::held();
        (lane, ScriptedJobs { jobs })
    }

    /// **A lane whose queue the test serves**: every job handed over arrives on
    /// the queue this returns, in order — one admitted ahead first, as the worker
    /// takes it — and nothing else takes it. The test is the worker: a job it
    /// takes is published done ([`HeldJobs`]), and its letting the queue go is
    /// the worker's end.
    #[cfg(test)]
    fn held() -> (Self, HeldJobs) {
        let (requests, incoming) = mpsc::channel::<Queued>();
        let lane = Self {
            shared: Arc::new(LaneShared::of(LaneParts {
                requests: Some(requests),
                ends: None,
                worker: LaneWorker::ByTest,
                up: true,
                wake: Arc::new(|| {}),
                executor: crate::lane::Executor::thread_door(
                    bt_platform::ThreadPriority::BelowNormal,
                ),
                say: Box::new(crate::diagnostics::note),
                sink: Arc::new(let_go),
            })),
        };
        let jobs = HeldJobs {
            incoming,
            ahead: Arc::clone(&lane.shared.ahead),
            ledger: Arc::clone(&lane.shared.ledger),
        };
        (lane, jobs)
    }

    /// **A lane no worker serves** — the operating system refused the thread —
    /// with `wake` as its wake, so a test hears what the loop would.
    #[cfg(test)]
    pub(crate) fn without_a_worker(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(LaneShared::of(LaneParts {
                requests: None,
                ends: None,
                worker: LaneWorker::Waiting,
                up: false,
                wake: Arc::new(wake),
                executor: crate::lane::Executor::thread_door(
                    bt_platform::ThreadPriority::BelowNormal,
                ),
                say: Box::new(crate::diagnostics::note),
                sink: Arc::new(let_go),
            })),
        }
    }

    /// **A lane with a worker of its own** — for a store a test opens over a
    /// named path.
    #[cfg(test)]
    pub(crate) fn of_its_own() -> Self {
        Self::with_a_worker_and_wake(wake_documents)
    }

    /// [`Self::of_its_own`], with `wake` as its wake, so a test hears what the
    /// loop would.
    #[cfg(test)]
    pub(crate) fn with_a_worker_and_wake(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::with_a_worker_started_by(
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            wake,
        )
    }

    /// [`Self::with_a_worker_and_wake`], its worker started by `executor` — the suite's
    /// scripted one, whose gate can hold a job or end the worker where a job begins.
    #[cfg(test)]
    pub(crate) fn with_a_worker_started_by(
        executor: crate::lane::Executor,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (lane, _receipts) = Self::fresh_with(
            executor,
            Arc::new(wake),
            Box::new(crate::diagnostics::note),
            Arc::new(let_go),
        );
        SessionWriter::start(&lane);
        lane
    }

    /// **Admit one job** (C1): only while a worker serves the queue, and only
    /// through the queue's lock, held across the ledger's admission and the send
    /// (the mailbox lock) — so the worker's end, which takes the same lock, never
    /// misses a request it admitted (C2). `between` runs under the lock after the
    /// lane is found served and before anything is minted — where a worker's end
    /// would race it, were the decision not made under the lock. A job `ahead` is
    /// served before every job still queued, after the one the worker may be in.
    /// Never waits for the worker.
    fn admit_between(
        &self,
        job: StorageJob,
        ahead: bool,
        between: impl FnOnce(),
    ) -> Result<StorageRequest, NotAdmitted> {
        let requests = lane_lock(&self.shared.requests);
        let Some(queue) = requests.as_ref() else {
            return Err(NotAdmitted::NoWorker);
        };
        if !self.shared.up.load(std::sync::atomic::Ordering::Acquire) {
            return Err(NotAdmitted::NoWorker);
        }
        between();
        let request = self
            .shared
            .ledger
            .admit(StorageOwed(Arc::clone(&self.shared.sink)))
            .map_err(NotAdmitted::of)?;
        let id = request.id();
        // A worker serves the queue (`up`, under this lock), so its receiving end stands until
        // its end has taken this lock: the send reaches it, and its end drops the job.
        if ahead {
            lane_lock(&self.shared.ahead).push_back((id, job));
            let _ = queue.send(Queued::Ahead);
        } else {
            let _ = queue.send(Queued::Job(id, Box::new(job)));
        }
        Ok(request)
    }

    /// Hand one job over, starting the worker if there is none — the session
    /// store's doors. `NoWorker`: none took it — the operating system would not
    /// start a thread, the worker has ended, or the lane was closed.
    fn submit_storage_job(&self, job: StorageJob) -> Result<StorageRequest, NotAdmitted> {
        if !SessionWriter::start(self) {
            return Err(NotAdmitted::NoWorker);
        }
        self.admit_between(job, false, || {})
    }

    /// **Hand one job over ahead of the queue** (T-QUIT-DOCUMENTS, round 2): the worker
    /// serves it before any job still queued, after the one it may be inside. Only an
    /// update trial's receipt goes this way — a small create-new write without which the
    /// trial is rolled back — so a quit's documents, their copies and its session write
    /// never stand in front of it; every other job keeps the queue's order, and per-file
    /// order holds (the receipt is no other request's file). Its identity is minted when it
    /// is admitted, so it ends — not in the order minted — before the requests queued ahead
    /// of it; no storage asker takes its terminals by a prefix of that order. Refused as
    /// [`Self::submit_storage_job`] refuses.
    fn submit_ahead(&self, job: StorageJob) -> Result<StorageRequest, NotAdmitted> {
        if !SessionWriter::start(self) {
            return Err(NotAdmitted::NoWorker);
        }
        self.admit_between(job, true, || {})
    }

    /// **Admit one document request** — only while a worker serves the queue. It
    /// never starts a thread: the session store starts the worker at open and
    /// again at its own hand-overs ([`SessionWriter::start`]). Refused: the
    /// caller completes the request as refused.
    fn admit_document_job(&self, job: StorageJob) -> Result<StorageRequest, NotAdmitted> {
        self.admit_document_job_between(job, || {})
    }

    /// [`Self::admit_document_job`], with `between` run under the queue's lock
    /// after admission is decided and before the job is queued.
    fn admit_document_job_between(
        &self,
        job: StorageJob,
        between: impl FnOnce(),
    ) -> Result<StorageRequest, NotAdmitted> {
        self.admit_between(job, false, between)
    }

    /// Hand a document's conditional write over.
    pub(crate) fn send_document(
        &self,
        job: crate::document_request::DocumentJob,
    ) -> Result<StorageRequest, NotAdmitted> {
        self.admit_document_job(StorageJob::Document(job))
    }

    /// Hand a document's stable read over.
    pub(crate) fn send_read(
        &self,
        job: crate::document_request::ReadJob,
    ) -> Result<StorageRequest, NotAdmitted> {
        self.admit_document_job(StorageJob::Read(job))
    }

    /// Hand a quit's copy of a document into `recovered` over.
    pub(crate) fn send_recovered(
        &self,
        job: crate::document_request::RecoveredJob,
    ) -> Result<StorageRequest, NotAdmitted> {
        self.admit_document_job(StorageJob::Recovered(job))
    }

    /// Whether a worker serves the queue now ([`LaneShared::up`]). Never waits.
    fn is_served(&self) -> bool {
        self.shared.up.load(std::sync::atomic::Ordering::Acquire)
    }

    /// **Wake the loop** — after a document answer is in its channel: the
    /// worker's, or a refusal the request door completed itself.
    pub(crate) fn wake(&self) {
        (self.shared.wake)();
    }
}

/// What one wait answers: every receipt that arrived on the way, and the verdict for the
/// generation that was waited on.
type SessionWaitAnswer = (Vec<SessionWriteReceipt>, Result<(), SaveRefusal>);

/// **The thread that owns the disk, and the only thing in this process that writes
/// `session.json`** (window-thread unbounded-call sweep, 2026-08-24).
///
/// The store used to call `write_session_atomic` from `flush_if_due`, which runs on the window
/// thread every turn of the event loop. That call is a `File::create`, a `write_all`, a
/// **`sync_all`** and a `rename`, under `%APPDATA%` — a path the reader is free to have
/// redirected onto a roaming
/// profile, a network share or a cloud-sync folder, where an `fsync` is a round trip with no
/// bound anybody in this process can state. A terminal that stops answering the mouse for a
/// second and a half because OneDrive was thinking is a terminal that froze.
///
/// **What did *not* move is the decision.** `session_document()` still reads the window tree on
/// the window thread and [`bt_persist::serialize_session`] still turns it into bytes there, so
/// the document is a snapshot of one consistent instant and this thread never touches a
/// `WindowRuntime`. What crossed is the part that has no opinions: bytes, a path, an `fsync`.
///
/// **One writer, so ordering is not a question.** A single thread taking one channel in order is
/// what makes "the last document decided is the last document on the disk" true without anybody
/// comparing timestamps. The generation on each request is not for ordering — it is so a receipt
/// arriving after a newer request has already gone out can be recognised as stale and dropped
/// rather than being allowed to mark the store clean over a document that has since changed.
struct SessionWriter {
    /// **The queue, and the two ends the writer thread needs until one thread
    /// has them** (T-QUIT-TIMEOUT-PROCEEDS, release review X-9; shared with the
    /// document stores since T-STORAGE-WORKER-REQUEST, [`StorageLane`]).
    ///
    /// `spawn_at_priority` is `Builder::spawn` with a priority set inside the thread, so the
    /// only way it fails is that the operating system would not start a thread at all — a state
    /// a machine is in for a moment, not for a run. Holding the ends in the lane is what lets the
    /// next hand-over try again ([`Self::start`]) instead of the store growing a second road to
    /// the file; taking them under the lock, inside the thread, is what makes "a second thread"
    /// impossible rather than merely unlikely.
    lane: StorageLane,
    receipts: mpsc::Receiver<SessionWriteReceipt>,
    /// The newest request handed over. A receipt older than this is stale.
    sent: u64,
    /// The newest request a receipt has come back for. `sent > landed` is "a document is still
    /// in flight", which is the question a quit has to ask even when nothing is dirty.
    landed: u64,
    /// **The answer for the newest request**, once it has come back: its generation and what
    /// became of it (U-21). An update's quit waits for one named generation across turns, and
    /// the autosave's own turn may be the one that reads its receipt.
    answered: Option<(u64, Result<(), String>)>,
    /// **The worker has ended**: its receipts' channel was read closed, so no receipt will
    /// ever come again ([`Self::collect`]).
    ended: bool,
    /// **The receipts' sending end, held by a store over a scripted lane**
    /// ([`SessionStore::at_on`]): no worker answers there and none has ended, so a session
    /// write queued on it reads as unanswered, never as the worker's end.
    #[cfg(test)]
    _receipts_held_open: Option<mpsc::Sender<SessionWriteReceipt>>,
    /// **A wait on this writer has already run out of budget** (T-QUIT-HAS-A-DEADLINE).
    ///
    /// Once one has, the thread is inside a call nobody in this process can bound, and every
    /// later wait would be the same budget spent again for the same answer — a quit that asks
    /// three times turns a three-second deadline into a nine-second one. So the first expiry is
    /// remembered, and from then on this writer answers immediately with the same sentence.
    stalled: bool,
    /// **How long one wait on this writer may last**: [`SESSION_SAVE_BUDGET`] from
    /// [`Self::open`], for every store the product makes. Only a test changes it, for its own
    /// store alone ([`SessionStore::set_save_budget`]), when the budget is not what it tests.
    budget: Duration,
}

impl SessionWriter {
    fn open() -> Self {
        let (lane, receipts) = StorageLane::fresh();
        let writer = Self {
            lane,
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            #[cfg(test)]
            _receipts_held_open: None,
            stalled: false,
            budget: SESSION_SAVE_BUDGET,
        };
        Self::start(&writer.lane);
        writer
    }

    /// **Make sure there is one writer thread, and never a second.**
    ///
    /// Idempotent, and the only place a thread is started — for the session store and for every
    /// document store that shares its lane. The ends are taken *inside* the thread, under the
    /// lock, so a spawn that failed leaves them where the next attempt finds them and a spawn that
    /// succeeded leaves nothing for anybody else to take — which is what makes "one writer, so
    /// ordering is not a question" a property of the type rather than of the paths that call it
    /// (release review X-9).
    fn start(lane: &StorageLane) -> bool {
        let mut worker = lane_lock(&lane.shared.worker);
        match &*worker {
            LaneWorker::Running => return true,
            #[cfg(test)]
            LaneWorker::ByTest => return true,
            LaneWorker::Waiting => {}
        }
        if lane_lock(&lane.shared.ends).is_none() {
            // A thread already took them and has since ended, or `close` let them go. Either way
            // there is nothing left for a new thread to work from.
            return false;
        }
        let ends = Arc::clone(&lane.shared.ends);
        let up = Arc::clone(&lane.shared.up);
        let wake = Arc::clone(&lane.shared.wake);
        let requests = Arc::downgrade(&lane.shared.requests);
        let ahead = Arc::clone(&lane.shared.ahead);
        let ledger = Arc::clone(&lane.shared.ledger);
        #[cfg(test)]
        let door = lane.shared.door.clone();
        #[cfg(test)]
        let thread = Arc::clone(&lane.shared.thread);
        let told = Arc::new(StorageEnds {
            ledger: Arc::clone(&lane.shared.ledger),
            wake: Arc::clone(&lane.shared.wake),
            ended: Arc::clone(&lane.shared.ended),
        });
        // Up before the thread can end, so its end is the last word on it; under
        // the queue's lock, as every write of it is.
        {
            let _admission = lane_lock(&lane.shared.requests);
            lane.shared
                .up
                .store(true, std::sync::atomic::Ordering::Release);
        }
        // In the workers' band (§1.4): a session write must never be the reason a frame was late,
        // and it is never the thing anybody is waiting to see. The executor contains the body
        // (`contained_panic::contain`) and tells the ledger how it ended ([`StorageEnds`]).
        let started = lane
            .shared
            .executor
            .start_worker("session-writer", told, move |ctx| {
                let Some(SessionWriterEnds { incoming, outgoing }) =
                    ends.lock().ok().and_then(|mut held| held.take())
                else {
                    return;
                };
                #[cfg(test)]
                {
                    *lane_lock(&thread) = Some(std::thread::current().id());
                }
                let mut end = WorkerEnd {
                    incoming,
                    receipts: Some(outgoing),
                    requests,
                    up,
                    ahead,
                    in_hand: std::collections::VecDeque::new(),
                };
                'serving: while let Ok(next) = end.incoming.recv() {
                    // **Every job admitted ahead is served before the one the queue gave**
                    // ([`StorageLane::submit_ahead`]): the worker takes them at each job it
                    // takes, and the `Ahead` token wakes it for them when it is idle.
                    end.in_hand.extend(lane_lock(&end.ahead).drain(..));
                    if let Queued::Job(id, job) = next {
                        end.in_hand.push_back((id, *job));
                    }
                    while let Some((id, mut job)) = end.in_hand.pop_front() {
                        let Some(receipts) = end.receipts.as_ref() else {
                            end.in_hand.push_front((id, job));
                            break 'serving;
                        };
                        // **One request, one task** (R2): a panic in it is this request's
                        // `Failed`, said once by the ledger, and the worker goes on. The loop
                        // holds the job and lends it to the task, so its road stands until its
                        // terminal is published.
                        let served = crate::lane::task(ctx, |ctx| {
                            #[cfg(test)]
                            if let Some(door) = &door {
                                return Ok(door(ctx, &mut job, receipts));
                            }
                            Ok::<_, std::convert::Infallible>(serve_storage_job(
                                ctx, &mut job, receipts,
                            ))
                        });
                        let wakes = job.wakes();
                        let (terminal, gone) = match served {
                            Ok(served) => (
                                crate::lane::Terminal::Done(()),
                                served == Served::ReceiptsGone,
                            ),
                            Err(failure) => {
                                // A session write's answer is its receipt, on the session's own
                                // road: a failed one is a refusal in the failure's words.
                                let gone = match &job {
                                    StorageJob::Session(request) => receipts
                                        .send(SessionWriteReceipt {
                                            generation: request.generation,
                                            result: Err(storage_task_failed(&failure.to_string())),
                                        })
                                        .is_err(),
                                    _ => false,
                                };
                                (crate::lane::Terminal::Failed(failure), gone)
                            }
                        };
                        ledger.publish(id, terminal);
                        // The job, and its road, let go after its terminal.
                        drop(job);
                        if gone {
                            break 'serving;
                        }
                        // The answer is in its channel and its terminal published before the
                        // wake: the steward's order.
                        if wakes {
                            wake();
                        }
                    }
                }
            });
        match started {
            Ok(()) => {
                *worker = LaneWorker::Running;
                true
            }
            Err(_) => {
                let _admission = lane_lock(&lane.shared.requests);
                lane.shared
                    .up
                    .store(false, std::sync::atomic::Ordering::Release);
                false
            }
        }
    }

    /// The newest document still on its way to the disk, if one is.
    fn in_flight(&self) -> Option<u64> {
        (self.sent > self.landed).then_some(self.sent)
    }

    /// **Whether an autosave must wait for the one on its way**: a document is in flight and a
    /// worker serves the lane to answer it. A worker that has ended answers nothing more, so the
    /// next hand-over goes ahead and meets the refusal at the door.
    fn busy(&self) -> bool {
        self.in_flight().is_some() && self.lane.is_served()
    }

    /// Hand one document over. Answers the generation it was filed under.
    ///
    /// A thread the operating system would not start leaves this refused (`NoWorker`), and the
    /// caller reports that rather than writing the document itself: this store has exactly one
    /// road to `session.json` and it is not the window thread (release review X-9). The attempt
    /// is made again here on every hand-over, so a machine that could not spare a thread for a
    /// moment gets its writer at the next trigger's hand-over rather than going the rest of the
    /// run without one.
    ///
    /// **The request's identity is left to the lane's sink at once**: what becomes of the
    /// document is its receipt, on the session's own road ([`Self::collect`]), a failed task's
    /// included (R2), and the worker's end is that road closing — so nothing here reads the
    /// terminal. The ledger still mints it, bounds it and ends it.
    fn send(&mut self, path: &Path, bytes: Vec<u8>) -> Result<u64, NotAdmitted> {
        let generation = self.sent + 1;
        let request = self
            .lane
            .submit_storage_job(StorageJob::Session(SessionWriteRequest {
                generation,
                path: path.to_path_buf(),
                bytes,
            }))?;
        drop(request);
        self.sent = generation;
        Ok(generation)
    }

    /// Queue this run's one launch backup ahead of every session write. The
    /// request is consumed even when no worker can take it: one launch gets one
    /// attempt, and a refusal is a diagnostic rather than a second write road.
    /// Nobody reads its answer, so its identity goes to the lane's sink at once.
    fn send_backup(&mut self, request: SessionBackupRequest) -> Result<(), NotAdmitted> {
        self.lane
            .submit_storage_job(StorageJob::SessionBackup(request))
            .map(drop)
    }

    /// Hand an update trial's receipt to the writer thread (F-14). `None` when
    /// there is no writer thread: the receipt is then not written at all, and
    /// never on the calling thread, which is the window's. Its reader is the
    /// trial's watch, which reads the answer's own road, so its identity goes to
    /// the lane's sink at once.
    fn send_receipt(
        &mut self,
        job: crate::update_trial::ReceiptJob,
    ) -> Option<mpsc::Receiver<ReceiptWritten>> {
        let (answer, answered) = mpsc::channel();
        self.lane
            .submit_ahead(StorageJob::Receipt { job, answer })
            .ok()
            .map(|request| {
                drop(request);
                answered
            })
    }

    /// Hand an update's hand-over to the writer thread (U-21): the durable
    /// `Handoff`, then the applier's start. `None` when there is no writer
    /// thread: then nothing is written and nothing started. Its reader is the
    /// update's way out (`update_handoff::look`), which holds the request beside
    /// its road and tells a failed hand-over from the worker's end by its
    /// terminal.
    fn send_handoff(
        &mut self,
        job: crate::update_handoff::HandoffJob,
    ) -> Option<OnTheWorker<crate::update_handoff::HandedOff>> {
        let (answer, answered) = mpsc::channel();
        self.lane
            .submit_storage_job(StorageJob::Handoff {
                job: Some(job),
                answer,
            })
            .ok()
            .map(|request| OnTheWorker::admitted(answered, request))
    }

    /// Every receipt that has arrived, newest-relevant last; a channel read closed after them
    /// marks the writer [`Self::ended`]. Never waits.
    fn collect(&mut self) -> Vec<SessionWriteReceipt> {
        let mut arrived = Vec::new();
        loop {
            match self.receipts.try_recv() {
                Ok(receipt) => arrived.push(receipt),
                Err(mpsc::TryRecvError::Empty) => return arrived,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ended = true;
                    return arrived;
                }
            }
        }
    }

    /// Wait for one named generation to land — **the one place this store blocks**, and the one
    /// caller that is entitled to: a quit is a decision that rests on the answer (multiwindow
    /// slice E2 phase ③), and the window it is holding up is already hidden.
    ///
    /// Receipts for older generations are returned alongside, because a synchronous wait must not
    /// swallow the answers the ordinary path was going to read.
    ///
    /// **And it is a wait with a deadline** (T-QUIT-HAS-A-DEADLINE). The entitlement was to wait
    /// for an answer, never to wait for ever: the thread it is waiting on is inside an `fsync`
    /// on a path the reader may have redirected onto storage that has stopped answering, and a
    /// hidden window over a hung share is a quit that never happens. **A budget that ran out is
    /// not a refusal** (release review X-8): it comes back as [`SaveRefusal::TimedOut`], which
    /// is what lets the quit go on leaving rather than stopping over a document that may still
    /// be on its way.
    ///
    /// **An owner-thread door** (`doors::SessionWriteWait`, §5.3 row 16): admitted only on the
    /// way out, minted in [`SessionStore::wait_for_landing`].
    fn wait_for(
        &mut self,
        token: WaitToken<'_, doors::SessionWriteWait>,
        generation: u64,
        until: Instant,
    ) -> SessionWaitAnswer {
        let _ = token;
        let mut earlier = Vec::new();
        if self.stalled {
            return (
                earlier,
                Err(SaveRefusal::TimedOut(save_did_not_finish(self.budget))),
            );
        }
        // **Never past `until`** — a quit's one budget, started by its documents phase
        // (T-QUIT-DOCUMENTS) — and never past this wait's own bound.
        let deadline = self.wait_ends(Instant::now()).min(until);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.receipts.recv_timeout(left) {
                Ok(receipt) if receipt.generation == generation => {
                    return (earlier, receipt.result.map_err(SaveRefusal::Refused));
                }
                Ok(receipt) => earlier.push(receipt),
                // The budget is spent and the thread is still inside the write. It is left to
                // finish — the write is atomic, so whatever it does next either replaces the file
                // whole or leaves the last complete document exactly where it was.
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.stalled = true;
                    report_save_did_not_finish(self.budget);
                    return (
                        earlier,
                        Err(SaveRefusal::TimedOut(save_did_not_finish(self.budget))),
                    );
                }
                // The thread is gone and the answer is never coming. That is a refusal and not a
                // wait that ran long: nothing is on its way, so there is nothing to go on past.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.ended = true;
                    return (
                        earlier,
                        Err(SaveRefusal::Refused(WRITER_STOPPED.to_string())),
                    );
                }
            }
        }
    }

    /// **Where a wait on this writer that starts at `now` ends at the latest**: `now` plus this
    /// writer's budget. Every wait on the writer reads it — [`Self::wait_for`], [`Self::close`]
    /// and [`SessionStore::flush_judged`]'s `until`.
    fn wait_ends(&self, now: Instant) -> Instant {
        now + self.budget
    }

    /// The writer thread's id, once it runs and while the lane says one does.
    #[cfg(test)]
    fn thread_id(&self) -> Option<std::thread::ThreadId> {
        match &*lane_lock(&self.lane.shared.worker) {
            LaneWorker::Running => *lane_lock(&self.lane.shared.thread),
            _ => None,
        }
    }

    /// Forget that a writer thread runs, as a timed-out fallback once let go of its handle.
    #[cfg(test)]
    fn forget_thread(&mut self) {
        *lane_lock(&self.lane.shared.worker) = LaneWorker::Waiting;
    }

    /// Let the thread finish what is queued and end. Idempotent.
    ///
    /// **An owner-thread door** (`doors::SessionWriterRetire`, §5.3 row 16b): its bounded poll
    /// for the worker's end is admitted only on the way out, minted in [`SessionStore::close`].
    fn close(&mut self, token: WaitToken<'_, doors::SessionWriterRetire>) {
        let _ = token;
        // Dropping the sender is what ends the thread's `recv` loop — for every asker sharing the
        // lane, whose later requests are refused where they are handed over. The ends go with it
        // so no later hand-over can start a writer over a channel nobody feeds.
        drop(lane_lock(&self.lane.shared.requests).take());
        *lane_lock(&self.lane.shared.ends) = None;
        if !matches!(
            std::mem::replace(
                &mut *lane_lock(&self.lane.shared.worker),
                LaneWorker::Waiting,
            ),
            LaneWorker::Running
        ) {
            return;
        }
        // **Bounded, and for the reason the wait above is** (T-QUIT-HAS-A-DEADLINE). Everything
        // queued is already decided and each item is one atomic write, but "one atomic write" is
        // exactly the call that has no bound on storage that has stopped answering — an
        // unbounded join here would simply move the hang one statement later. Past the budget
        // the thread is let go rather than waited on: it is not holding anything this process
        // still needs, and the file it may yet replace it replaces whole.
        if self.stalled {
            return;
        }
        // The worker's end is told to the ledger as the thread's last act ([`StorageEnds`]).
        let deadline = self.wait_ends(Instant::now());
        while !self
            .lane
            .shared
            .ended
            .load(std::sync::atomic::Ordering::Acquire)
        {
            if Instant::now() >= deadline {
                self.stalled = true;
                report_save_did_not_finish(self.budget);
                return;
            }
            std::thread::sleep(SESSION_JOIN_POLL);
        }
    }
}

/// **The crash sentinel's name** beside `session.json` (§5.5): present while a
/// writer of record runs, dropped on its clean-exit path.
const SENTINEL_FILE_NAME: &str = "session.lock";

/// **Whether the run before this one over the data directory reached its
/// clean-exit path** — the session sentinel's answer, the fact diagnostics
/// says as "previous session did not reach its clean-exit path": `false` after
/// a crash, a power cut or an end from outside; `true` after an orderly quit
/// and after the system's end of the session (B-ENDSESSION drops it too).
/// Read once per process, before this run arms its own sentinel, whichever
/// asks first — [`SessionStore::open`] or the update job's launch pass
/// (`update_txn::launch_event`, 0.4.8 E3, which counts a deferral only after a
/// run that ended orderly).
pub(crate) fn previous_run_ended_orderly() -> bool {
    previous_exit(&storage_dir().join(SENTINEL_FILE_NAME)) == ExitState::Normal
}

/// The one probe of the sentinel at `sentinel`, kept for the process: the first
/// answer stands whatever path a later call names, so a test that seeds a
/// sentinel of its own asks in a process of its own (`update_trial`'s
/// `run_in_a_process_of_its_own` harness), never in the shared test process.
fn previous_exit(sentinel: &Path) -> ExitState {
    static PREVIOUS: OnceLock<ExitState> = OnceLock::new();
    *PREVIOUS.get_or_init(|| probe_sentinel(sentinel).unwrap_or(ExitState::Normal))
}

/// The session file, its sentinel, the debounce that stands between a change and the disk, and
/// the thread that actually touches it.
pub struct SessionStore {
    session_path: PathBuf,
    sentinel_path: PathBuf,
    session: SessionV1,
    debouncer: Debouncer,
    writes: DocumentWrites,
    writer: SessionWriter,
    /// **A refused document is owed, and no clock owes it** (the side-effects
    /// contract's C5; owner ruling Q3, 2026-10-11): set when the newest document
    /// handed over was refused, or could not be handed over at all; cleared when
    /// a document is handed over. Only a trigger sends it again — the next change
    /// (its debounce, [`Self::record`]), the next storage pass
    /// ([`Self::hear_the_storage_pass`]) or the quit's write.
    retry_on_trigger: bool,
    /// **The pending slot**: the quiet window ran out while a document was still
    /// on its way, so the document as it then stands — this store's own, not a
    /// copy — is handed over when that one is answered, whatever the answer
    /// says. One autosave on the writer at a time.
    send_again: bool,
    /// Held only while an update trial holds all durable writes. A commit queues
    /// it immediately before the first session write; an ended trial drops it.
    launch_backup: Option<SessionBackupRequest>,
    /// True once the sentinel for *this* run exists, so a clean exit knows
    /// there is something to remove.
    armed: bool,
    /// **The sentinel went because the system was ending the session** (B-ENDSESSION), so a
    /// shutdown taken back puts it back — [`Self::rearm_after_the_systems_end`].
    sentinel_dropped_for_the_systems_end: bool,
    /// **The launch backup waits** (T-QUIT-DOCUMENTS round 4): a quit that goes on with an
    /// update trial's session release still pending holds it back from its final write
    /// until the write's verdict says whether the release is performed or consumed.
    launch_backup_paused: bool,
    /// **Whether this process writes the session at all** (review row R4-5).
    ///
    /// False in the second Folio over one data directory: its window works, its
    /// tabs are its own, and nothing it does reaches `session.json` — which is
    /// the whole of the fix, because what a second writer costs is the first
    /// one's windows.
    writer_of_record: bool,
    /// The sentence a startup owes about this file, if it owes one — a document
    /// that would not read, or one larger than this build will open.
    fault: Option<String>,
}

/// Make the one snapshot a launch may owe. Missing, refused and tabless
/// documents leave an older backup untouched; only a successfully loaded
/// layout with at least one tab replaces it. The bytes are the reader's exact
/// input, not a serialization of the migrated and degraded model.
fn launch_backup_request(
    session_path: &Path,
    session: &SessionV1,
    report: &ReadReport,
    read_bytes: DocumentBytes,
) -> Option<SessionBackupRequest> {
    if !matches!(report, ReadReport::Loaded)
        || !session.windows.iter().any(|window| !window.tabs.is_empty())
    {
        return None;
    }
    let DocumentBytes::Loaded(bytes) = read_bytes else {
        return None;
    };
    Some(SessionBackupRequest {
        source: session_path.to_path_buf(),
        path: session_path.with_file_name(PREVIOUS_SESSION_FILE_NAME),
        bytes,
    })
}

impl SessionStore {
    /// Read the previous session and arm this one's sentinel.
    ///
    /// Every failure path here is non-fatal by construction: `read_session`
    /// never returns an error (a failure to load *is* the default document),
    /// and a storage directory that cannot be created leaves a store that
    /// simply never writes. A terminal that refuses to start because it could
    /// not write a layout file would be a worse product than one that forgets
    /// its layout.
    pub fn open() -> Self {
        let dir = storage_dir();
        let session_path = dir.join(SESSION_FILE_NAME);
        let sentinel_path = dir.join(SENTINEL_FILE_NAME);
        let writable = make_data_folder(&dir);
        // **Asked before the sentinel and before the read** (review row R4-5),
        // because both of those are things only the writer of record may do: a
        // second process that armed a sentinel would clear the first one's crash
        // record on its own clean exit. Asked of `dir`, which is the directory
        // this store's two files are in — see `is_writer_of`.
        let writer_of_record = is_writer_of(&dir);
        // Probe *before* creating: creating first would make every probe after
        // the first report a crash. Read once for the process
        // ([`previous_run_ended_orderly`] answers the same probe).
        let previous_exit = previous_exit(&sentinel_path);
        let (session, report, degradation, read_bytes) =
            bt_persist::read_session_keeping_bytes(&session_path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &session_path, |path| {
            let _ = read_session(path);
        });
        // §5.4 case 1 — no file yet — is the normal first run and must not
        // alert; every other non-`Loaded` outcome must (§5.3: "explicit alert,
        // never pretend it succeeded").
        let mut fault = read_fault(
            &report,
            SESSION_FILE_NAME,
            crate::i18n::session_file_unreadable,
        );
        if !degradation.is_clean() {
            eprintln!(
                "BT_PERSIST session.json degraded: {} clamped ratios, {} unknown leaves, \
                 {} windows, {} tabs and {} panes past the ceiling, {} impossible commands",
                degradation.clamped_ratios,
                degradation.unknown_leaves,
                degradation.dropped_windows,
                degradation.dropped_tabs,
                degradation.dropped_panes,
                degradation.dropped_commands
            );
        }
        // **Only the ceiling is said out loud** (review row R4-9). A clamped
        // ratio and an unknown leaf are already visible — the divider sits where
        // it can and the pane draws its placeholder — but a tab that was not
        // opened at all looks exactly like a tab the reader forgot they had.
        if degradation.dropped_windows + degradation.dropped_tabs + degradation.dropped_panes > 0 {
            fault.get_or_insert_with(|| crate::i18n::session_file_trimmed(SESSION_FILE_NAME));
        }
        if previous_exit == ExitState::Crashed {
            eprintln!("BT_PERSIST previous session did not reach its clean-exit path");
        }
        // An update's trial arms no sentinel until it is committed
        // (`update_trial`, F-7): `session.lock` is a file in O's folder.
        let writes_deferred =
            writer_of_record && crate::update_trial::defer(crate::update_trial::Writer::Session);
        let armed = writable
            && writer_of_record
            && !writes_deferred
            && create_sentinel(&sentinel_path).is_ok();
        let launch_backup = if writer_of_record {
            launch_backup_request(&session_path, &session, &report, read_bytes)
        } else {
            None
        };
        let mut store = Self {
            session_path,
            sentinel_path,
            session,
            debouncer: Debouncer::new(),
            writes: DocumentWrites::new(),
            writer: SessionWriter::open(),
            retry_on_trigger: false,
            send_again: false,
            launch_backup,
            armed,
            sentinel_dropped_for_the_systems_end: false,
            launch_backup_paused: false,
            writer_of_record,
            fault,
        };
        if !writes_deferred {
            store.queue_launch_backup();
        }
        store
    }

    /// **The storage worker's queue**, for the four hand-editable documents'
    /// stores to share ([`StorageLane`]): one worker, one queue, per-file order.
    pub(crate) fn lane(&self) -> StorageLane {
        self.writer.lane.clone()
    }

    /// Take the sentence this store owes the reader, so a card about it is
    /// raised once and not once a frame — every other store's `take_fault`, and
    /// for its reason.
    ///
    /// Two sources, one sentence: a document that would not read (and the copy
    /// of it that was kept), and a document whose writes have stopped landing.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.writes.take_fault())
    }

    /// A store over two named paths, for the tests that have to watch a write
    /// **fail**.
    ///
    /// Test-only, like `Text::ALL`: the product has one door onto this type and
    /// it is [`Self::open`], which resolves `%APPDATA%` and arms a sentinel.
    /// Neither of those is a thing a test may do to the machine it runs on, and
    /// the property multiwindow slice E2 has to pin — a quit that could not write
    /// does not leave — needs a store whose path is *guaranteed* unwritable,
    /// which is exactly what a caller-named path buys.
    #[cfg(test)]
    pub fn at(session_path: PathBuf, sentinel_path: PathBuf) -> Self {
        // **Asked of the directory this store writes, not asserted.** A named
        // path is very nearly always a directory nobody else holds — which is
        // what the assertion this replaces was reaching for — but "nearly
        // always" is a thing a store finds out by asking, and asking is what
        // makes a claim held on that directory mean something here.
        let writer_of_record = is_writer_of_document(&session_path);
        Self {
            session_path,
            sentinel_path,
            session: SessionV1::default(),
            debouncer: Debouncer::new(),
            writes: DocumentWrites::new(),
            writer: SessionWriter::open(),
            retry_on_trigger: false,
            send_again: false,
            launch_backup: None,
            armed: false,
            sentinel_dropped_for_the_systems_end: false,
            launch_backup_paused: false,
            writer_of_record,
            fault: None,
        }
    }

    /// [`Self::at`], its documents handed to `lane` — a scripted lane's queue
    /// holds them in order beside the documents' (no receipt comes back to it, and the
    /// receipts' channel stays open, so a write on it reads as unanswered, not refused).
    #[cfg(test)]
    pub(crate) fn at_on(session_path: PathBuf, sentinel_path: PathBuf, lane: StorageLane) -> Self {
        let mut store = Self::at(session_path, sentinel_path);
        let (held_open, receipts) = mpsc::channel();
        store.writer = SessionWriter {
            lane,
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            _receipts_held_open: Some(held_open),
            stalled: false,
            budget: SESSION_SAVE_BUDGET,
        };
        store
    }

    /// [`Self::at`], its writer a worker of its own that wakes `wake` where the product's wakes
    /// the loop (`AppEvent::DocumentsAnswered`) — so a test hears what the loop would, and takes
    /// the receipts on that wake as the loop's turn does.
    #[cfg(test)]
    pub(crate) fn at_waking(
        session_path: PathBuf,
        sentinel_path: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self::at_waking_started_by(
            session_path,
            sentinel_path,
            wake,
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
        )
    }

    /// [`Self::at_waking`], its worker started by `executor` — the suite's scripted one, whose
    /// gate can hold a job where it begins or end the worker there (`lane::Gate::kill`).
    #[cfg(test)]
    pub(crate) fn at_waking_started_by(
        session_path: PathBuf,
        sentinel_path: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
        executor: crate::lane::Executor,
    ) -> Self {
        let mut store = Self::at(session_path, sentinel_path);
        let (lane, receipts) = StorageLane::fresh_with(
            executor,
            Arc::new(wake),
            Box::new(crate::diagnostics::note),
            Arc::new(let_go),
        );
        store.writer = SessionWriter {
            lane,
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            #[cfg(test)]
            _receipts_held_open: None,
            stalled: false,
            budget: SESSION_SAVE_BUDGET,
        };
        SessionWriter::start(&store.writer.lane);
        store
    }

    /// [`Self::open`] over caller-owned paths: it reads the named session and
    /// queues the launch backup, without touching the machine's data folder or
    /// sentinel. Only tests need to observe a launch at this seam.
    #[cfg(test)]
    pub(crate) fn launched_at(session_path: PathBuf, sentinel_path: PathBuf) -> Self {
        let writer_of_record = is_writer_of_document(&session_path);
        let (session, report, _degradation, read_bytes) =
            bt_persist::read_session_keeping_bytes(&session_path, bt_persist::Keeping::Now);
        let writes_deferred =
            writer_of_record && crate::update_trial::defer(crate::update_trial::Writer::Session);
        let launch_backup = if writer_of_record {
            launch_backup_request(&session_path, &session, &report, read_bytes)
        } else {
            None
        };
        let mut store = Self {
            session_path,
            sentinel_path,
            session,
            debouncer: Debouncer::new(),
            writes: DocumentWrites::new(),
            writer: SessionWriter::open(),
            retry_on_trigger: false,
            send_again: false,
            launch_backup,
            armed: false,
            sentinel_dropped_for_the_systems_end: false,
            launch_backup_paused: false,
            writer_of_record,
            fault: None,
        };
        if !writes_deferred {
            store.queue_launch_backup();
        }
        store
    }

    /// [`Self::at`], with this run's sentinel created and armed as [`Self::open`] arms it — for
    /// the tests that watch a run's end drop it (B-ENDSESSION).
    #[cfg(test)]
    pub(crate) fn armed_at(session_path: PathBuf, sentinel_path: PathBuf) -> Self {
        let mut store = Self::at(session_path, sentinel_path);
        store.armed = create_sentinel(&store.sentinel_path).is_ok();
        store
    }

    /// This store, with its sentinel created and armed as [`Self::open`] arms it — for a test
    /// whose store is built on a lane of its own ([`Self::at_on`], [`Self::at_waking`]) and that
    /// watches the system's end drop the sentinel, or leave it standing (T-ENDSESSION-SAVES).
    #[cfg(test)]
    pub(crate) fn armed(mut self) -> Self {
        self.armed = create_sentinel(&self.sentinel_path).is_ok();
        self
    }

    /// The session document as it was read. The caller owns what the fields
    /// mean; this type owns only when they reach the disk.
    pub fn loaded(&self) -> &SessionV1 {
        &self.session
    }

    /// Replace the in-memory document and start the debounce window. Called on
    /// a meaningful change: a tree edit, or the end of a resize.
    ///
    /// **A change is a trigger** (C5): a document owed after a refusal goes with
    /// it, because what is handed over at the end of the quiet window is the
    /// document as it then stands.
    pub fn record(&mut self, session: SessionV1, now: Instant) {
        if self.session == session {
            return;
        }
        self.session = session;
        self.debouncer.mark_dirty(now);
    }

    /// **Whether this store owes the disk a document**: a change in its quiet
    /// window, a document owed after a refusal, or one waiting in the pending
    /// slot. A document on its way is not owed; it is answered.
    fn owes_a_write(&self) -> bool {
        self.debouncer.is_dirty() || self.retry_on_trigger || self.send_again
    }

    /// **A storage pass ran** (`storage_watch`, the storage folder's ear): an
    /// ear event, so a document owed after a refusal is handed over once more
    /// (C5's "next ear event"). A change already in its quiet window carries it
    /// there instead.
    pub(crate) fn hear_the_storage_pass(&mut self) {
        if self.retry_on_trigger && !self.debouncer.is_dirty() {
            self.hand_over();
        }
    }

    /// When the event loop should wake to write, if it should.
    pub fn deadline(&self) -> Option<Instant> {
        self.debouncer.deadline(SESSION_DEBOUNCE)
    }

    /// **Hand the writer the document if the quiet window has elapsed** — the clock's half of a
    /// session write. Its answer is not read here: the storage worker wakes the loop after each
    /// receipt (`AppEvent::DocumentsAnswered`), and that turn takes it ([`Self::take_receipts`]),
    /// so no clock turns for a receipt, and a receipt is heard the moment it lands.
    ///
    /// **And the pending slot** (one autosave on the writer at a time): a document that waited
    /// for the one before it is handed over on the first turn after that one was answered —
    /// the wake's own turn, since the wake follows the receipt.
    pub fn flush_if_due(&mut self, now: Instant) {
        crate::hang_watch::at(crate::hang_watch::Station::Autosave);
        if self.debouncer.should_flush(now, SESSION_DEBOUNCE)
            || (self.send_again && !self.writer.busy())
        {
            self.hand_over();
        }
    }

    /// **The autosave's turn, and the sentence it owes the reader**: [`Self::flush_if_due`],
    /// then the card a failure streak owes (once per streak, [`DocumentWrites::record`]) —
    /// whichever turn booked it, this one or an earlier one. The window raises what this
    /// answers; ignoring it is a session that cannot be saved and is never said.
    #[must_use = "a session that cannot be saved is said on a card"]
    pub fn autosave(&mut self, now: Instant) -> Option<String> {
        self.flush_if_due(now);
        self.take_fault()
    }

    /// Whether this process is the one that writes `session.json` — see
    /// [`is_writer_of`], asked of the directory this store's file is in. A store
    /// that is not stays exactly as useful as one that is, in memory; it simply
    /// reaches no disk.
    ///
    /// **And not while an update's trial holds its writes back**
    /// (`update_trial`, F-7): the document stays live here and is handed over
    /// when the trial is committed ([`Self::release_trial`]).
    fn writes_to_disk(&self) -> bool {
        self.writer_of_record && !crate::update_trial::defer(crate::update_trial::Writer::Session)
    }

    /// **Hand an update trial's receipt to the storage worker** (F-14): written
    /// there, create-new, through the `install_txn` door — never on the calling
    /// thread, which is the window's. The answer comes back on the channel this
    /// returns; `None` when there is no worker to write it.
    pub(crate) fn write_receipt(
        &mut self,
        receipt: crate::update_trial::ReceiptJob,
    ) -> Option<mpsc::Receiver<ReceiptWritten>> {
        self.writer.send_receipt(receipt)
    }

    /// **An update's trial was committed: arm this run's sentinel and hand the
    /// document over** — what [`Self::open`] and the autosave held back
    /// (`update_trial`, F-7).
    pub fn release_trial(&mut self) {
        if !self.writer_of_record {
            return;
        }
        self.arm_sentinel();
        self.queue_launch_backup();
        self.hand_over();
    }

    /// **Arm this run's sentinel, if it is not armed** — what [`Self::release_trial`] does
    /// first, on its own (T-QUIT-DOCUMENTS round 4): a quit or a stop that disposes of an
    /// update trial's held session release without performing it still owes the next start
    /// a truthful `session.lock`, so a final write that does not land is said there.
    pub(crate) fn arm_sentinel(&mut self) {
        if self.writer_of_record && !self.armed {
            self.armed = create_sentinel(&self.sentinel_path).is_ok();
        }
    }

    /// **Hold the launch backup back from the writes that follow** until
    /// [`Self::resume_launch_backup`] or [`Self::forgo_launch_backup`] (round 4).
    pub(crate) fn pause_launch_backup(&mut self) {
        self.launch_backup_paused = true;
    }

    /// Let the launch backup go ahead of the next session write again (round 4).
    pub(crate) fn resume_launch_backup(&mut self) {
        self.launch_backup_paused = false;
    }

    /// Whether this run's sentinel is armed — for a test.
    #[cfg(test)]
    pub(crate) fn sentinel_is_armed(&self) -> bool {
        self.armed && self.sentinel_path.is_file()
    }

    /// Put the launch backup on the session writer exactly once. FIFO ordering
    /// makes it land before this run's first session write without a wait on the
    /// window thread.
    fn queue_launch_backup(&mut self) {
        if self.launch_backup_paused {
            return;
        }
        let Some(request) = self.launch_backup.take() else {
            return;
        };
        if let Err(refused) = self.writer.send_backup(request) {
            crate::diagnostics::note(&format!(
                "BT_PERSIST {PREVIOUS_SESSION_FILE_NAME} backup failed: {}",
                refused.said(NO_WRITER_THREAD)
            ));
        }
    }

    /// Hand the current document to the writer, without waiting for it to land.
    ///
    /// The debounce is marked flushed here rather than at the receipt, and the two are different
    /// questions: the debouncer answers "when should this be tried", and handing it over is
    /// exactly the moment the answer becomes "not until something changes". Whether it *landed*
    /// is [`Self::take_receipts`]'s, and a refusal there leaves the document owed to the next
    /// trigger — never to a clock ([`Self::retry_on_trigger`]).
    ///
    /// **One autosave on the writer at a time.** While one is on its way the document goes to
    /// the pending slot ([`Self::send_again`]) and is handed over when that one is answered: a
    /// writer inside an `fsync` that does not come back is not handed a document every quiet
    /// window.
    fn hand_over(&mut self) {
        if !self.writes_to_disk() {
            // The second Folio over this directory (review row R4-5). Marked
            // flushed rather than left dirty, because nothing is owed: there is
            // no attempt to retry and no failure to report — this process was
            // never the one keeping this file.
            self.debouncer.mark_flushed();
            self.retry_on_trigger = false;
            self.send_again = false;
            return;
        }
        if self.writer.busy() {
            self.debouncer.mark_flushed();
            self.send_again = true;
            return;
        }
        self.queue_launch_backup();
        let bytes = match bt_persist::serialize_session(&self.session) {
            Ok(bytes) => bytes,
            // A document that cannot be turned into JSON is not a disk problem and no thread will
            // fix it. It goes through the same one-alert-per-streak tracker, and is owed to the
            // next trigger like any refusal.
            Err(error) => {
                self.debouncer.mark_flushed();
                self.send_again = false;
                self.report_write(Err(error.to_string()));
                return;
            }
        };
        if let Err(refused) = self.send_to_the_writer(bytes) {
            // **No writer thread, and therefore no write** (release review X-9). The only way
            // `spawn_at_priority` fails is that the operating system would not start a thread
            // at all, and the one thing this document may not be written on is this thread,
            // which is the window's. So it is reported instead: the document is owed, and the
            // next trigger's hand-over asks for a thread again.
            self.report_write(Err(refused.said(NO_WRITER_THREAD)));
        }
    }

    /// **Hand the document's bytes to the writer**: the one place a session document is sent.
    /// Whatever was owed — the quiet window, the pending slot, a refusal — goes with it; what
    /// becomes of it is its receipt's. Refused when no writer thread took it.
    fn send_to_the_writer(&mut self, bytes: Vec<u8>) -> Result<u64, NotAdmitted> {
        self.debouncer.mark_flushed();
        self.send_again = false;
        self.retry_on_trigger = false;
        self.writer.send(&self.session_path, bytes)
    }

    /// **Read whatever the writer has answered** — on the turn the storage worker's wake brings
    /// (`AppEvent::DocumentsAnswered`), and wherever a decision rests on the answer (a quit's
    /// write, an update's receipt) — and let a refusal leave the document owed to the next
    /// trigger.
    ///
    /// A receipt older than the newest request is **stale and dropped**: a newer document has
    /// already been handed over and will bring its own answer, so letting an old refusal leave
    /// the store owing would send again something that has since been superseded, and letting an
    /// old success mark it clean would be answering for a document nobody wrote.
    ///
    /// **The worker's end is the terminal of the document it took and will not answer** (C2):
    /// its receipts' channel closes before its end wakes the loop, so the document still in
    /// flight when the channel reads closed is booked here as refused — the writer stopped —
    /// once, as a refusal the disk gave would be.
    pub(crate) fn take_receipts(&mut self) {
        for receipt in self.writer.collect() {
            self.apply_receipt(receipt);
        }
        if self.writer.ended
            && let Some(generation) = self.writer.in_flight()
        {
            self.apply_receipt(SessionWriteReceipt {
                generation,
                result: Err(WRITER_STOPPED.to_string()),
            });
        }
    }

    fn apply_receipt(&mut self, receipt: SessionWriteReceipt) {
        // Recorded for every receipt, stale or not: what this answers is "is anything still on
        // its way", and a stale receipt is still one fewer document in flight.
        self.writer.landed = self.writer.landed.max(receipt.generation);
        if receipt.generation < self.writer.sent {
            return;
        }
        self.writer.answered = Some((receipt.generation, receipt.result.clone()));
        self.report_write(receipt.result);
    }

    /// One alert per failure streak (§5.3), and a refusal leaves the document owed — **to the
    /// next trigger, not to a clock** (the side-effects contract's C5; owner ruling Q3,
    /// 2026-10-11).
    ///
    /// A refused write is a terminal answer for that request. Nothing schedules another: the
    /// next change (its quiet window), the next storage pass ([`Self::hear_the_storage_pass`])
    /// or the quit's write ([`Self::flush_judged`]) is what sends the document again, as it then
    /// stands. A disk that is full or a volume that was unplugged is therefore not written to
    /// every quiet window while nothing changes, and the reader is told once per streak
    /// ([`DocumentWrites::record`]).
    fn report_write(&mut self, result: Result<(), String>) {
        self.retry_on_trigger = !self.writes.record(SESSION_FILE_NAME, result);
    }

    /// Write now, whatever the debounce says. The clean-exit path uses this:
    /// a pending change must not be lost because the window closed 300ms after
    /// it happened.
    pub fn flush(&mut self) {
        // The ordinary paths have nowhere to put a failure: a window is already
        // going, and the alert below has been said. See [`Self::flush_judged`]
        // for the one caller that can act on the answer.
        let _ = self.flush_judged();
    }

    /// **The same write, judged** (multiwindow slice E2 phase ③).
    ///
    /// A quit hands the store every window at once and then hides all of them,
    /// so it is the one caller for which "could not write" is not a line on
    /// `stderr` after the fact but a decision to make *before* the next step: a
    /// window hidden over a document that never reached the disk is the session
    /// gone, and `session.lock` left standing would then be this run truthfully
    /// reporting that it did not reach a clean exit.
    ///
    /// **It still goes through the one writer.** A second road to the same file, taken while
    /// that thread may have a document of its own in flight, is two writers racing over one path
    /// and the older one is free to land last. What is different here is only that this caller
    /// **waits** — the one wait in this store, and the one place it is owed.
    ///
    /// `Ok(())` with nothing written is the honest answer when the debounce is clean *and*
    /// nothing is still on its way. Those are two conditions and not one: the autosave hands
    /// documents over without waiting, so "clean" can mean "handed to the writer a moment ago",
    /// and a quit may not report as landed a document nobody has heard back about.
    ///
    /// **The two ways it can fail are not one answer** (release review X-8). A refusal ends the
    /// quit where it stands; a budget that ran out does not, because the disk still holds the
    /// last completed save and the reader still asked to leave. [`SaveRefusal`] is which.
    pub fn flush_judged(&mut self) -> Result<(), SaveRefusal> {
        self.flush_judged_until(self.writer.wait_ends(Instant::now()))
    }

    /// **Tests only: this store's writer waits `budget` instead of [`SESSION_SAVE_BUDGET`]** —
    /// in [`Self::flush_judged`], [`Self::save_for_the_systems_end`] and [`Self::close`].
    ///
    /// For a test whose subject is not the budget: one that writes real files and asks whether
    /// they landed sets the suite's patience (`lane::PATIENCE`), so a loaded machine answers the
    /// same thing later rather than a timeout. A test of the budget itself keeps the product's.
    /// This store alone; every other store, and every product store, keeps its own.
    #[cfg(test)]
    pub(crate) fn set_save_budget(&mut self, budget: Duration) {
        self.writer.budget = budget;
    }

    /// [`Self::flush_judged`], its wait ending at `until` at the latest — a quit's one
    /// budget (T-QUIT-DOCUMENTS).
    fn flush_judged_until(&mut self, until: Instant) -> Result<(), SaveRefusal> {
        if !self.writes_to_disk() {
            // The second Folio over this directory (review row R4-5). `Ok(())`
            // and not an error, because the quit's question is "did what this
            // process owed the disk reach it" and this process owes it nothing —
            // an error here would refuse to close a window over a document that
            // was never this window's.
            self.debouncer.mark_flushed();
            return Ok(());
        }
        self.queue_launch_backup();
        self.take_receipts();
        // **The quit's write is a trigger** (C5): a document owed after a refusal, or waiting in
        // the pending slot, is handed over here like a change in its quiet window.
        if !self.owes_a_write() {
            // Owing nothing, but not necessarily *landed*: a document handed over a moment ago
            // can still be on the writer's channel.
            let Some(outstanding) = self.writer.in_flight() else {
                return Ok(());
            };
            return self.wait_for_landing(outstanding, until);
        }
        let bytes = bt_persist::serialize_session(&self.session).map_err(|error| {
            let error = error.to_string();
            self.report_write(Err(error.clone()));
            SaveRefusal::Refused(error)
        })?;
        let generation = match self.send_to_the_writer(bytes) {
            Ok(generation) => generation,
            Err(refused) => {
                // No writer thread and no second road to the file (release review X-9). This is
                // a refusal and not a wait that ran long: nothing was queued, nothing is on its
                // way, and a window hidden over it would be the session gone.
                let said = refused.said(NO_WRITER_THREAD);
                self.report_write(Err(said.clone()));
                return Err(SaveRefusal::Refused(said));
            }
        };
        self.wait_for_landing(generation, until)
    }

    /// **A quit's session write, inside what is left of its one budget**
    /// (T-QUIT-DOCUMENTS; design note 2026-10-11, revision (c)2 as ruled in the
    /// review's round 2).
    ///
    /// `until` is `quit::Quit::session_write_until`: the documents phase's start plus
    /// [`SESSION_SAVE_BUDGET`]. Before it, this is [`Self::flush_judged`] with its wait
    /// ending at `until`. **At or past it, nothing is handed over and nothing is waited
    /// for**: the quit's budget is spent, so the store gives the write up as a write whose
    /// budget ran out does — the debounce is settled, the writer is marked stalled so the
    /// way out's [`Self::close`] neither flushes anew nor waits for the thread, and
    /// `session.lock` stays, because this run never saw this document land. The last
    /// completed save is what the next start reads. A store that owes nothing answers
    /// `Ok(())` either way.
    pub(crate) fn flush_for_quit(
        &mut self,
        until: Instant,
        now: Instant,
    ) -> Result<(), SaveRefusal> {
        if now < until {
            let verdict = self.flush_judged_until(until);
            self.said_by_the_quit(&verdict);
            return verdict;
        }
        if !self.writes_to_disk() {
            self.debouncer.mark_flushed();
            return Ok(());
        }
        self.take_receipts();
        if !self.owes_a_write() && self.writer.in_flight().is_none() {
            return Ok(());
        }
        // **Given up, all of it**: nothing this store owed — the quiet window, a document a
        // refusal left owed, the pending slot — is handed over by the way out's close either.
        self.writer.stalled = true;
        self.debouncer.mark_flushed();
        self.retry_on_trigger = false;
        self.send_again = false;
        let said = quit_budget_spent();
        eprintln!("Folio: {said}");
        Err(SaveRefusal::TimedOut(said))
    }

    /// **Skip this run's launch backup** (T-QUIT-DOCUMENTS round 3): a quit that goes on
    /// to its way out with an update trial's write release still held makes its own final
    /// session write the session document of record, and nothing goes ahead of it. Said once.
    pub(crate) fn forgo_launch_backup(&mut self) {
        self.launch_backup_paused = false;
        if self.launch_backup.take().is_some() {
            crate::diagnostics::note(&format!(
                "BT_PERSIST {PREVIOUS_SESSION_FILE_NAME} backup skipped: the quit's own session \
                 write is this run's document of record"
            ));
        }
    }

    /// A launch backup owed from `source`, as an update trial holds one — for a test.
    #[cfg(test)]
    pub(crate) fn owing_a_launch_backup(&mut self, source: PathBuf, bytes: &[u8]) {
        self.launch_backup = Some(SessionBackupRequest {
            path: source.with_file_name(PREVIOUS_SESSION_FILE_NAME),
            source,
            bytes: Arc::from(bytes),
        });
    }

    /// **Hand the final document over as a named generation, and do not wait for it** — an
    /// update's quit (0.4.6 U-21, `docs/plans/design/self-update-2026-09-16.md` §C.3).
    ///
    /// The same writer and the same road as [`Self::flush_judged`], and one difference that is
    /// the whole point: the window thread goes on turning, and the answer is read by
    /// [`Self::landing_of`] on a later turn, under a deadline the quit keeps as a clock rather
    /// than a wait. **Always a new generation**, clean or not: the update is handed on only on a
    /// receipt for *this* document — the photograph the quit just took — and never on an older
    /// one that happens to have landed.
    ///
    /// `Ok(None)` when this process owes the disk nothing ([`Self::flush_judged`]'s second Folio
    /// over this directory): there is no document to wait for, and none is this quit's.
    ///
    /// # Errors
    /// The document could not be serialised, or there is no writer thread: a refusal, as
    /// [`Self::flush_judged`] reports it.
    pub(crate) fn hand_over_final(&mut self) -> Result<Option<u64>, SaveRefusal> {
        let handed = self.hand_over_a_new_generation();
        if let Err(refusal) = &handed {
            self.said_by_the_quit(&Err(refusal.clone()));
        }
        handed
    }

    /// **The document as it stands, handed to the writer as a new generation, clean or not** —
    /// never waited for here: [`Self::hand_over_final`]'s hand-over and
    /// [`Self::save_for_the_systems_end`]'s. `Ok(None)` when this process owes the disk nothing
    /// (the second Folio over this directory).
    fn hand_over_a_new_generation(&mut self) -> Result<Option<u64>, SaveRefusal> {
        if !self.writes_to_disk() {
            self.debouncer.mark_flushed();
            return Ok(None);
        }
        self.queue_launch_backup();
        self.take_receipts();
        let bytes = bt_persist::serialize_session(&self.session).map_err(|error| {
            let error = error.to_string();
            self.report_write(Err(error.clone()));
            SaveRefusal::Refused(error)
        })?;
        match self.send_to_the_writer(bytes) {
            Ok(generation) => Ok(Some(generation)),
            Err(refused) => {
                let said = refused.said(NO_WRITER_THREAD);
                self.report_write(Err(said.clone()));
                Err(SaveRefusal::Refused(said))
            }
        }
    }

    /// **What became of generation `generation`, if the writer has said** — never waits (U-21);
    /// asked on the turn the storage worker's wake brings.
    ///
    /// Only the receipt for that generation answers: an older one is somebody else's document.
    /// A writer thread that has gone answers a refusal, as [`SessionWriter::wait_for`] does:
    /// nothing is on its way.
    pub(crate) fn landing_of(&mut self, generation: u64) -> Option<Result<(), SaveRefusal>> {
        self.take_receipts();
        let landing = if let Some((answered, result)) = &self.writer.answered
            && *answered == generation
        {
            Some(result.clone().map_err(SaveRefusal::Refused))
        } else {
            self.writer
                .ended
                .then(|| Err(SaveRefusal::Refused(WRITER_STOPPED.to_string())))
        };
        if let Some(verdict) = &landing {
            self.said_by_the_quit(verdict);
        }
        landing
    }

    /// **A quit's refusal is said by the quit's own card** (`QuitSessionNotWritten`, on every
    /// window): this streak counts as said, so the autosave's turn does not say it a second
    /// time after the quit is abandoned. A budget that ran out is not a refusal and raises no
    /// card — the process leaves.
    fn said_by_the_quit(&mut self, verdict: &Result<(), SaveRefusal>) {
        if verdict
            .as_ref()
            .is_err_and(|refusal| !refusal.quit_may_proceed())
        {
            self.writes.said_elsewhere();
        }
    }

    /// **The receipt for `generation` did not come back in time** (U-21): the same answer, and
    /// the same bookkeeping, as a synchronous wait whose budget ran out — the writer is marked
    /// stalled, so the close does not spend the budget again and leaves `session.lock` standing,
    /// and the one line is said.
    pub(crate) fn receipt_overdue(&mut self, generation: u64) -> SaveRefusal {
        self.writer.stalled = true;
        report_save_did_not_finish(self.writer.budget);
        let refusal = SaveRefusal::TimedOut(save_did_not_finish(self.writer.budget));
        self.apply_receipt(SessionWriteReceipt {
            generation,
            result: Err(refusal.message().to_string()),
        });
        refusal
    }

    /// **Hand an update's hand-over to the storage worker** (U-21): `Handoff` written durably
    /// there, then the applier started — never on the calling thread, which is the window's. The
    /// answer comes back on the request this returns; `None` when there is no worker.
    pub(crate) fn hand_off(
        &mut self,
        job: crate::update_handoff::HandoffJob,
    ) -> Option<OnTheWorker<crate::update_handoff::HandedOff>> {
        self.writer.send_handoff(job)
    }

    /// Stand still until one named document has an answer, and book every answer that arrives on
    /// the way — a synchronous wait must not swallow the receipts the ordinary path was going to
    /// read.
    fn wait_for_landing(&mut self, generation: u64, until: Instant) -> Result<(), SaveRefusal> {
        // The one wait in this store is an owner-thread door. A refusal did not wait: it is the
        // stalled answer a spent budget gives, so a quit goes on rather than stopping over a
        // document that may still be on its way.
        let budget = self.writer.budget;
        let (earlier, landed) = admitted::<doors::SessionWriteWait, _>(|token| {
            self.writer.wait_for(token, generation, until)
        })
        .unwrap_or_else(|_refused| {
            (
                Vec::new(),
                Err(SaveRefusal::TimedOut(save_did_not_finish(budget))),
            )
        });
        for receipt in earlier {
            self.apply_receipt(receipt);
        }
        // The tracker books one fact — "this document is not on the disk" — and both refusals
        // are that fact. Which of the two it was is the caller's question, not the clock's.
        self.apply_receipt(SessionWriteReceipt {
            generation,
            result: landed
                .clone()
                .map_err(|refusal| refusal.message().to_string()),
        });
        landed
    }

    /// **The system is ending the session: write the document this store holds and, once it has
    /// landed, drop this run's sentinel** (B-ENDSESSION; `docs/M2-persistence-schema-v1.md` §5.5:
    /// a system shutdown saves immediately and removes `session.lock`, like a quit).
    ///
    /// The quit's own write ([`Self::flush_judged`], through the one writer and the one bounded
    /// wait, `doors::SessionWriteWait`) and the quit's own rule for the sentinel ([`Self::close`]):
    /// it goes only once the document it vouches for has landed. A budget that ran out, or a
    /// refusal, leaves it standing, and the disk holds the last completed save.
    ///
    /// **What differs from the close is that the writer stays.** The session may yet go on —
    /// somebody can stop a shutdown at the last screen — and a store whose writer had been retired
    /// would never save the layout again for the rest of the run.
    ///
    /// **Always a new generation, clean or not** (T-ENDSESSION-SAVES), as an update's quit hands
    /// its document over: the system's end hands every preview save and every owed document to the
    /// same storage worker first, and the worker serves its queue in order, so this document's
    /// landing is what says everything queued ahead of it has been served. One that was already
    /// clean, or already on its way, would end the wait before them.
    ///
    /// **The budget counts from `asked_at`**, the instant the question's turn began: what was
    /// handed over ahead of this document spends it, and adds nothing to it.
    pub(crate) fn save_for_the_systems_end(
        &mut self,
        asked_at: Instant,
    ) -> Result<(), SaveRefusal> {
        let landed = match self.hand_over_a_new_generation() {
            Ok(Some(generation)) => {
                let until = self.writer.wait_ends(asked_at);
                self.wait_for_landing(generation, until)
            }
            Ok(None) => Ok(()),
            Err(refusal) => Err(refusal),
        };
        if landed.is_ok() && self.armed {
            let _ = remove_sentinel(&self.sentinel_path);
            self.armed = false;
            self.sentinel_dropped_for_the_systems_end = true;
        }
        landed
    }

    /// **The system took the end back** (B-ENDSESSION): the run goes on, so its claim to be
    /// running goes back where [`Self::save_for_the_systems_end`] took it from — and only then; a
    /// run that had no sentinel to drop gets none.
    pub(crate) fn rearm_after_the_systems_end(&mut self) {
        if !std::mem::take(&mut self.sentinel_dropped_for_the_systems_end) {
            return;
        }
        self.armed = create_sentinel(&self.sentinel_path).is_ok();
    }

    /// **The system took the end back, and its write's wait had run out** (T-ENDSESSION-SAVES):
    /// the writer was marked stalled, which makes every later wait answer "did not finish" at once
    /// and the close leave without waiting — right for a process that is leaving, wrong for a run
    /// that goes on. A writer whose thread is still there is waited on again, within its budget, by
    /// the next save that waits (a quit's); one whose thread has gone stays as it is, and its next
    /// wait is the refusal it always was.
    pub(crate) fn wait_again_after_the_systems_end(&mut self) {
        self.take_receipts();
        if !self.writer.ended {
            self.writer.stalled = false;
        }
    }

    /// Flush anything pending, let the writer finish, and drop this run's sentinel. Idempotent.
    ///
    /// The order is the whole of it: the sentinel's absence is this run's only claim to have
    /// exited cleanly, so it may not be removed until the document it is vouching for is on the
    /// disk — which is why the writer is closed, and therefore joined, before the sentinel goes.
    ///
    /// **And a save that ran out of its budget leaves it standing** (T-QUIT-HAS-A-DEADLINE).
    /// The deadline buys back the window thread; it does not buy back the claim. A run that
    /// walked out with a document still inside an `fsync` did not reach a clean exit, and
    /// removing the sentinel over it would be this run vouching for bytes nobody has heard
    /// about — the next start is owed the restore prompt instead.
    pub fn close(&mut self) {
        self.flush();
        // The writer's retirement is an owner-thread door. A refusal leaves the writer to the
        // process's exit, as its own spent budget does — and, like that budget, it leaves the
        // sentinel standing, because nobody waited to hear the document land.
        if admitted::<doors::SessionWriterRetire, _>(|token| self.writer.close(token)).is_err() {
            self.writer.stalled = true;
        }
        if self.armed && !self.writer.stalled {
            let _ = remove_sentinel(&self.sentinel_path);
            self.armed = false;
        }
    }
}

/// **The queue of a lane the test serves** ([`StorageLane::held`]): the test is
/// the worker. A job it takes is the one the worker would serve next — one
/// admitted ahead first — and is published done as it is taken, so the asker
/// reads the answer the test then puts on its road (or, if the test lets the job
/// go unanswered, the road's close: the worker gone). Letting the queue go is the
/// worker's end: the ledger ends what is still owed `WorkerGone` and refuses
/// every later request, as a worker's end does.
#[cfg(test)]
struct HeldJobs {
    incoming: mpsc::Receiver<Queued>,
    ahead: AheadJobs,
    ledger: Arc<StorageLedger>,
}

#[cfg(test)]
impl HeldJobs {
    /// The next job, if one is waiting. Never waits.
    fn try_recv(&self) -> Result<StorageJob, mpsc::TryRecvError> {
        let (id, job) = loop {
            if let Some(next) = lane_lock(&self.ahead).pop_front() {
                break next;
            }
            match self.incoming.try_recv()? {
                Queued::Ahead => {}
                Queued::Job(id, job) => break (id, *job),
            }
        };
        self.ledger.publish(id, crate::lane::Terminal::Done(()));
        Ok(job)
    }

    /// Every job waiting now, in the order the worker would serve them.
    fn try_iter(&self) -> impl Iterator<Item = StorageJob> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }

    /// The next job, waited for within the lane suite's patience (`lane::receive`): a job
    /// that never comes fails the test that awaited `what`, by name, rather than hanging it.
    fn receive(&self, what: &str) -> StorageJob {
        loop {
            if let Ok(job) = self.try_recv() {
                return job;
            }
            if let Queued::Job(id, job) = crate::lane::receive(&self.incoming, what) {
                self.ledger.publish(id, crate::lane::Terminal::Done(()));
                return *job;
            }
        }
    }
}

#[cfg(test)]
impl Drop for HeldJobs {
    fn drop(&mut self) {
        self.ledger.worker_ended(crate::lane::WorkerEnd::Returned);
    }
}

/// The queue of a [`StorageLane::unserved`] lane, kept so the lane stays open.
#[cfg(test)]
pub(crate) struct UnservedJobs {
    _jobs: HeldJobs,
}

/// **The queue of a [`StorageLane::scripted`] lane**: a test outside this module
/// takes each job in the order it was queued and answers it itself — the quit's
/// scripted disk.
#[cfg(test)]
pub(crate) struct ScriptedJobs {
    jobs: HeldJobs,
}

/// One job a scripted lane's queue held, as a test answers it.
#[cfg(test)]
pub(crate) enum ScriptedJob {
    Document(crate::document_request::DocumentJob),
    Recovered(crate::document_request::RecoveredJob),
    /// A document's stable read, which the test answers.
    Read(crate::document_request::ReadJob),
    /// A session document.
    Session,
    /// The launch's session backup.
    Backup,
    /// An update trial's receipt.
    Receipt,
    /// Anything else the worker takes.
    Other,
}

#[cfg(test)]
impl ScriptedJobs {
    /// The next job the worker would serve, if one is: a job admitted ahead first, as
    /// the worker takes it, then the queue's. Never waits.
    pub(crate) fn next(&self) -> Option<ScriptedJob> {
        let job = self.jobs.try_recv().ok()?;
        Some(match job {
            StorageJob::Document(job) => ScriptedJob::Document(job),
            StorageJob::Recovered(job) => ScriptedJob::Recovered(job),
            StorageJob::Read(job) => ScriptedJob::Read(job),
            StorageJob::Session(_) => ScriptedJob::Session,
            StorageJob::Receipt { .. } => ScriptedJob::Receipt,
            StorageJob::SessionBackup(_) => ScriptedJob::Backup,
            StorageJob::Handoff { .. } => ScriptedJob::Other,
        })
    }
}

/// `settings.json` and when it reaches the disk.
///
/// No debouncer, unlike [`SessionStore`], and §1.1 is explicit about why the two
/// files are separate at all: "设置改动应立即落盘(用户在设置面板点一下就该生效并
/// 持久,丢失更痛)". A settings write happens when a human clicks a row in a
/// dialog — it cannot arrive at the rate divider drags do, so there is nothing
/// to coalesce and a quiet window would only be a window in which the choice can
/// be lost.
///
/// **A press is handed to the storage worker at once, and lands by receipt**
/// (T-STORAGE-WORKER-REQUEST): the setting is in force in memory at the press,
/// and the file follows a few milliseconds later, written only if it still holds
/// what Folio last read or wrote there ([`crate::document_request`]).
pub struct SettingsStore {
    /// The document, what the file holds, and the requests on the worker.
    document: DocumentState<SettingsV1>,
    /// The sentence a startup owes about this file, if it owes one.
    fault: Option<String>,
    /// Whether there was no `settings.json` at all when this store opened.
    ///
    /// Kept rather than thrown away with the rest of the report, because it is
    /// the one thing `loaded()` cannot say afterwards: a file that was there and
    /// could not be read hands back the same defaults a machine with no file
    /// does, and only one of those two is a machine nobody has ever configured.
    /// The first-run card is the reader of this (`crate::first_run`), and it
    /// reads it beside the stored `first_run_card` rather than instead of it —
    /// a damaged file must not be mistaken for a new machine.
    missing: bool,
    /// **Whether writes are held for one batch of changes** (0.4.4 ticket 05),
    /// and whether anything was stored while they were.
    ///
    /// An import puts each setting through the door a press on that row goes
    /// through, and every one of those doors ends in [`Self::store`]. Forty rows
    /// changed by one gesture are one choice, and they reach the disk as one
    /// write — the one a single press makes — rather than forty. `None` is the
    /// ordinary state: every store hands its document over at once.
    held: Option<bool>,
    /// **A merge's doors are running** (design note 2026-10-11 (b)6): a store
    /// moves what is in force and nothing else. `desired` is already the merged
    /// document, and a door that puts in force something else than the file
    /// said — a font this machine lacks — must not write that back as a change.
    merging: bool,
    /// **What a quit's documents phase merged without putting it in force**
    /// (design note 2026-10-11, revision (c)2: merges at quit are pure, no door
    /// runs). `desired` already holds the merged document; a quit that was
    /// cancelled leaves the window standing, and the next storage pass puts it in
    /// force through its doors and says it ([`Self::begin_merge`]).
    merged_at_quit: Option<crate::settings_merge::Account>,
}

impl SettingsStore {
    /// Read `settings.json`, falling back to defaults on every failure — same
    /// contract as [`SessionStore::open`], and for the same reason: a terminal
    /// that refuses to start because it could not read a preferences file would
    /// be a worse product than one that starts with the default preferences.
    /// Its writes go to the storage worker on `lane`.
    pub(crate) fn open(lane: StorageLane) -> Self {
        let dir = storage_dir();
        let path = settings_write_path(&dir, SETTINGS_FILE_NAME);
        make_data_folder(path.parent().unwrap_or(&dir));
        Self::read_at(path, is_writer_of(&dir), lane)
    }

    /// Read the file at `path` into a store — [`Self::open`] once the path and
    /// the writer's answer are known.
    fn read_at(path: PathBuf, writer_of_record: bool, lane: StorageLane) -> Self {
        let (settings, report, bytes) =
            read_settings_keeping_bytes(&path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &path, |path| {
            let _ = read_settings(path);
        });
        // §5.4 case 1 — no file yet — is the normal first run and must not alert.
        let fault = read_fault(
            &report,
            SETTINGS_FILE_NAME,
            crate::i18n::settings_file_unreadable,
        );
        Self {
            document: DocumentState::opened(
                SETTINGS_FILE_NAME,
                path,
                Revision::opened(settings, bytes),
                writer_of_record,
                crate::update_trial::Writer::Settings,
                lane,
            ),
            fault,
            missing: report == ReadReport::NotFound,
            held: None,
            merging: false,
            merged_at_quit: None,
        }
    }

    /// **The language `settings.json` in `directory` names — read, and nothing
    /// else** (B-EXPLORER-CLAIM, 0.4.6).
    ///
    /// For the processes that never have a window and only need to know which
    /// language to say their few words in: `--explorer-command`'s COM server and
    /// the front door's refusal (`say_at_the_front_door`). [`Self::open`] is not
    /// that question. It asks [`is_writer_of`], which takes the data directory's
    /// claim — or remembers the refusal — for the life of the process, and it
    /// makes the folder and keeps a refused file beside it. A COM server that
    /// Explorer keeps alive while the menu it drew starts Folio would then hold
    /// the claim the Folio it started needs, and that Folio would run for its
    /// whole life as a window that saves nothing.
    ///
    /// So this reads the file's bytes through the settings lane
    /// ([`read_settings_keeping`] with [`bt_persist::Keeping::Owed`]: a refused
    /// file is reported, never kept, never moved) and answers the default on
    /// every failure, which is [`Self::open`]'s own contract. It asks nobody who
    /// writes here, and it is not a brief claim let go again either: a resident
    /// launch that asked in that instant would be refused, and would remember
    /// the refusal for its whole run.
    pub(crate) fn peek_language(directory: &Path) -> bt_persist::LanguageV1 {
        let (settings, _) = read_settings_keeping(
            &settings_write_path(directory, SETTINGS_FILE_NAME),
            bt_persist::Keeping::Owed,
        );
        settings.language
    }

    /// Take the sentence this store owes the reader — [`SessionStore::take_fault`]
    /// and every other store's, one file over.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.document.take_fault())
    }

    /// The same store over a named file, for the tests that have to watch a
    /// write **fail** — [`SessionStore::at`]'s door and its reason: neither
    /// resolving `%APPDATA%` nor writing into it is a thing a test may do to the
    /// machine it runs on. Its writes go to a worker of its own.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self::at_on(path, StorageLane::of_its_own())
    }

    /// [`Self::at`], over `lane`.
    #[cfg(test)]
    pub(crate) fn at_on(path: PathBuf, lane: StorageLane) -> Self {
        // [`SessionStore::at`]'s rule: asked of the directory this store
        // writes, rather than asserted about it.
        let writer_of_record = is_writer_of_document(&path);
        Self {
            document: DocumentState::opened(
                SETTINGS_FILE_NAME,
                path,
                Revision::opened(SettingsV1::default(), DocumentBytes::Absent),
                writer_of_record,
                crate::update_trial::Writer::Settings,
                lane,
            ),
            fault: None,
            missing: true,
            held: None,
            merging: false,
            merged_at_quit: None,
        }
    }

    /// The settings as they currently stand.
    pub fn loaded(&self) -> &SettingsV1 {
        self.document.projection()
    }

    /// What `settings.json` is known to hold.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> &Revision<SettingsV1> {
        self.document.revision()
    }

    /// The document half, for a test to look into.
    #[cfg(test)]
    pub(crate) fn document(&mut self) -> &mut DocumentState<SettingsV1> {
        &mut self.document
    }

    /// How many writes of this document the reader has asked for
    /// (`storage_watch::StorageWatch::advance`).
    pub(crate) fn write_generation(&self) -> u64 {
        self.document.write_generation()
    }

    /// Whether this machine had no `settings.json` when the process started.
    ///
    /// Still true after the first write: it is a fact about the launch, not
    /// about the file, and the surface that asks it (the first-run card) is
    /// raised after the card's own state has already been written down.
    pub fn was_missing(&self) -> bool {
        self.missing
    }

    /// Record a change and hand it to the storage worker now. Returns whether
    /// anything changed, so a caller can skip the repaint when a user picks the
    /// value already set.
    ///
    /// **A value that did not reach the disk is written again next time it is
    /// chosen** (review row R4-10): re-picking the row is exactly the gesture
    /// somebody makes when a setting did not take, and a document still owed is
    /// handed over again whether or not the press changed anything.
    pub fn store(&mut self, settings: SettingsV1) -> bool {
        if self.merging {
            let changed = self.document.projection() != &settings;
            self.document.project(settings);
            return changed;
        }
        let held = self.held.is_some();
        let stored = self.document.store(settings, held);
        if let Some(any) = self.held.as_mut() {
            *any |= stored || self.document.owed();
        }
        stored
    }

    /// **Hold every write until [`Self::release_writes`]** — one gesture's worth
    /// of stores, landing as one write. See the `held` field.
    pub fn hold_writes(&mut self) {
        self.held.get_or_insert(false);
    }

    /// Let the held writes go: the document as it now stands is handed over
    /// once, if anything was stored while they were held.
    pub fn release_writes(&mut self) {
        if self.held.take() == Some(true) {
            self.document.send_owed();
        }
    }

    /// **Ask for the file to be read again, because the storage watch heard
    /// it** (R33, owner ruling Q3 2026-10-11). The read runs on the storage
    /// worker; a file that changed is stored for the merge
    /// ([`Self::begin_merge`]), one that will not parse is not, and what is in
    /// force stays.
    pub(crate) fn ask_reread(&mut self) {
        self.document.ask_read();
    }

    /// Whether a re-read of `settings.json` is still to be answered.
    pub(crate) fn reading(&self) -> bool {
        self.document.reading()
    }

    /// **A storage pass's merge, its first three steps** (design note
    /// 2026-10-11 §3.3, (b)6, (b)8, (c)1): every file stored for the merge is
    /// merged field by field against its base ([`settings_merge::merge`]) and
    /// folded into `desired`, which moves the revision to the file the merge
    /// read; the three machine receipts are put in force by storing them; the
    /// writes are held in the import's batch shape, and every store until
    /// [`Self::end_merge`] moves only what is in force. The answer is the doors
    /// that put the merged document in force, derived from what is in force
    /// now — `None` when nothing was merged.
    ///
    /// [`settings_merge::merge`]: crate::settings_merge::merge
    pub(crate) fn begin_merge(
        &mut self,
        platform: bt_platform::HostPlatform,
    ) -> Option<crate::settings_merge::SettingsPass> {
        let at_quit = self.merged_at_quit.take();
        if !self.document.merge_waits() && at_quit.is_none() {
            return None;
        }
        let merges = self.document.merge_stored(|base, ours, theirs| {
            let merged = crate::settings_merge::merge(base, ours, theirs, platform);
            (merged.document, merged.account)
        });
        if merges.is_empty() && at_quit.is_none() {
            return None;
        }
        let mut account = crate::settings_merge::Account::default();
        for merge in merges {
            account.add(merge);
        }
        let merged = self.document.desired().clone();
        let mut in_force = self.document.projection().clone();
        crate::settings_merge::take_receipts(&mut in_force, &merged);
        self.document.project(in_force);
        self.hold_writes();
        self.merging = true;
        Some(crate::settings_merge::SettingsPass::new(
            crate::settings_merge::doors(self.document.projection(), &merged, platform),
            account,
            at_quit.unwrap_or_default(),
        ))
    }

    /// **A quit's merge** (design note 2026-10-11, revision (c)2): every file
    /// stored for the merge is merged and folded into `desired`, as
    /// [`Self::begin_merge`] does, and **nothing is put in force** — no door runs,
    /// the windows are leaving. Answers its one `diagnostics.log` line, said once
    /// whether the quit completes or is cancelled (T-STORE-NOTICE); what it merged
    /// is kept for the next storage pass, which puts it in force and names it on
    /// its card should the quit be cancelled, without counting it again.
    fn merge_at_quit(&mut self, platform: bt_platform::HostPlatform) -> Option<String> {
        if !self.document.merge_waits() {
            return None;
        }
        let merges = self.document.merge_stored(|base, ours, theirs| {
            let merged = crate::settings_merge::merge(base, ours, theirs, platform);
            (merged.document, merged.account)
        });
        if merges.is_empty() {
            return None;
        }
        let mut account = crate::settings_merge::Account::default();
        for merge in merges {
            account.add(merge);
        }
        let line = account.said(SETTINGS_FILE_NAME, crate::settings_merge::Said::AtQuit);
        self.merged_at_quit
            .get_or_insert_with(Default::default)
            .add(account);
        Some(line)
    }

    /// **A storage pass's merge, its last step**: the writes are released, and
    /// the merged document is handed to the worker, conditional on the file the
    /// merge read, if it differs from it — the units that kept Folio's value.
    pub(crate) fn end_merge(&mut self) {
        self.merging = false;
        self.held = None;
        self.document.send_owed();
    }

    /// **An update's trial was committed: the document as it stands is handed
    /// over** — every write the trial held back (`update_trial`), conditional on
    /// the revision, so a hand edit made during the trial is not written over.
    pub fn release_trial(&mut self) {
        self.document.send_owed();
    }

    /// **Take what the storage worker has answered** (`AppEvent::DocumentsAnswered`):
    /// whether a re-read found a file that would not parse.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        self.document.take_answers()
    }

    /// Let the worker answer everything this store asked, and take it.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) {
        let _ = self.document.settle();
    }
}

/// `settings.json` in a quit (design note 2026-10-11, revision (c)2): its
/// [`DocumentState`]'s answers, and a turn that folds a changed file in before
/// it sends — a `Moved` or `Displaced` answer at quit is merged as the running
/// window merges it, without the doors.
impl QuitDocument for SettingsStore {
    fn file(&self) -> &'static str {
        self.document.file()
    }

    fn quit_begin(&mut self) {
        self.document.quit_begin();
    }

    fn quit_turn(&mut self) -> Option<String> {
        let _ = self.document.take_answers();
        let merged = self.merge_at_quit(bt_platform::host_platform());
        if self.document.standing() == crate::quit::Standing::Unsent {
            self.document.send_owed();
        }
        merged
    }

    fn standing(&self) -> crate::quit::Standing {
        self.document.standing()
    }

    fn keep_copy(&mut self, folder: &Path, at: std::time::SystemTime) {
        self.document.keep_copy(folder, at);
    }

    fn copy_standing(&mut self) -> crate::quit::CopyStanding {
        self.document.copy_standing()
    }

    fn drop_owed(&mut self) -> Option<String> {
        self.document.drop_owed()
    }

    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>> {
        self.document.owed_bytes()
    }
}

/// The name the shortcut file wears on disk, which is also what a notice about
/// it has to say out loud.
pub const KEYBINDINGS_FILE_NAME: &str = "keybindings.json";

/// `keybindings.json` — the shortcut table's departures, and when they reach the
/// disk.
///
/// No debouncer, for [`SettingsStore`]'s reason and rather more sharply: a chord
/// arrives when a human has just pressed a key and watched a dialog change, and
/// a quiet window would be a window in which exactly that can be lost. A press
/// is handed to the storage worker at once and lands by receipt
/// (T-STORAGE-WORKER-REQUEST).
///
/// **A damaged file is reported, never repaired.** `fault` carries the sentence
/// a notice will say, and the read path leaves the file itself untouched — a
/// build that silently rewrote a shortcut file it could not parse would destroy
/// the one copy of a customisation the user could have fixed by hand. It is only
/// replaced when the user changes something, which is them choosing.
///
/// **What the file says and the table does not hold is carried** (design note
/// 2026-10-11 (b)6, (b)8): a line naming an action this build has no row for is
/// in the document Folio writes back, in its place, because a press changes
/// only the actions it changed.
///
/// **A hand edit is followed live and merged** (R33, owner ruling Q3
/// 2026-10-11; T-STORE-MERGE-KEYBINDINGS): the store sends `Merge` requests,
/// keeps every changed file for the merge, and holds its writes while one
/// waits; the storage pass merges it by action, then by chord
/// ([`Self::begin_merge`], `keybindings_merge`), and the window puts the
/// result in force.
pub struct KeybindingsStore {
    /// The departures, what the file holds, and the requests on the worker.
    document: DocumentState<KeybindingsV1>,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    /// **What a quit's documents phase merged without putting it in force**
    /// ([`SettingsStore`]'s `merged_at_quit`, for its reason): the next storage
    /// pass puts it in force and says it, should the quit be cancelled.
    merged_at_quit: Option<crate::keybindings_merge::About>,
}

/// `keybindings.json` in a quit and on a stop: its [`DocumentState`]'s answers (T-QUIT-DOCUMENTS).
impl QuitDocument for KeybindingsStore {
    fn file(&self) -> &'static str {
        self.document.file()
    }

    fn quit_begin(&mut self) {
        self.document.quit_begin();
    }

    /// A `Moved` or `Displaced` answer at quit is merged as the running window
    /// merges it, without the door ([`SettingsStore`]'s turn).
    fn quit_turn(&mut self) -> Option<String> {
        let _ = self.document.take_answers();
        let merged = self.merge_at_quit(bt_platform::host_platform());
        if self.document.standing() == crate::quit::Standing::Unsent {
            self.document.send_owed();
        }
        merged
    }

    fn standing(&self) -> crate::quit::Standing {
        self.document.standing()
    }

    fn keep_copy(&mut self, folder: &std::path::Path, at: std::time::SystemTime) {
        self.document.keep_copy(folder, at);
    }

    fn copy_standing(&mut self) -> crate::quit::CopyStanding {
        self.document.copy_standing()
    }

    fn drop_owed(&mut self) -> Option<String> {
        self.document.drop_owed()
    }

    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>> {
        self.document.owed_bytes()
    }
}

impl KeybindingsStore {
    /// Read `keybindings.json`, falling back to *no overrides* on every failure.
    /// Its writes go to the storage worker on `lane`.
    pub(crate) fn open(lane: StorageLane) -> Self {
        let dir = storage_dir();
        let path = settings_write_path(&dir, KEYBINDINGS_FILE_NAME);
        make_data_folder(path.parent().unwrap_or(&dir));
        Self::read_at(path, is_writer_of(&dir), lane)
    }

    /// Read the file at `path` into a store — [`Self::open`] once the path and
    /// the writer's answer are known.
    fn read_at(path: PathBuf, writer_of_record: bool, lane: StorageLane) -> Self {
        let (file, report, bytes) =
            read_keybindings_keeping_bytes(&path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &path, |path| {
            let _ = read_keybindings(path);
        });
        // §5.4 case 1 — no file — is the ordinary state of nearly every machine
        // and must not alert. Everything else must, naming the file (§5.3).
        let fault = read_fault(
            &report,
            KEYBINDINGS_FILE_NAME,
            crate::i18n::keybindings_file_unreadable,
        );
        Self {
            document: DocumentState::opened(
                KEYBINDINGS_FILE_NAME,
                path,
                Revision::opened(file, bytes),
                writer_of_record,
                crate::update_trial::Writer::Keybindings,
                lane,
            ),
            fault,
            merged_at_quit: None,
        }
    }

    /// The same store over a named file for tests that exercise the real
    /// keybindings writer without touching this machine's configuration.
    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self::at_on(path, StorageLane::of_its_own())
    }

    /// [`Self::at`] over a file that may hold a document, on `lane`.
    #[cfg(test)]
    pub(crate) fn at_on(path: PathBuf, lane: StorageLane) -> Self {
        let writer_of_record = is_writer_of_document(&path);
        Self::read_at(path, writer_of_record, lane)
    }

    /// The overrides as they stand: what a test asks of the projection — the
    /// launch reads [`Self::file_document`] and the window its one table.
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> &[BindingOverrideV1] {
        &self.document.projection().bindings
    }

    /// **The document the file says**, its dialect with it — what the reader
    /// reads at launch (`settings_bundle::shortcuts_at_start`).
    pub(crate) fn file_document(&self) -> &KeybindingsV1 {
        self.document.desired()
    }

    /// What `keybindings.json` is known to hold.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> &Revision<KeybindingsV1> {
        self.document.revision()
    }

    /// How many writes of this document the reader has asked for
    /// (`storage_watch::StorageWatch::advance`).
    pub(crate) fn write_generation(&self) -> u64 {
        self.document.write_generation()
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame. A write that has stopped landing is owed the same card, and is
    /// taken through the same door.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.document.take_fault())
    }

    /// **The departures the table put in force from the file** — what a press
    /// is measured against. The lines the table refused stay in the document
    /// and out of this ([`DocumentState::project`]).
    pub(crate) fn project(&mut self, overrides: Vec<BindingOverrideV1>) {
        self.project_on(overrides, keybindings_platform());
    }

    /// [`Self::project`], in `writing_platform`'s dialect — the store's
    /// ([`Self::store_on`]).
    pub(crate) fn project_on(
        &mut self,
        overrides: Vec<BindingOverrideV1>,
        writing_platform: KeybindingsPlatformV1,
    ) {
        self.document
            .project(keybindings_document(overrides, writing_platform));
    }

    /// Record the new set of departures and hand them to the storage worker.
    ///
    /// Returns whether anything changed, so a caller can skip the write when a
    /// user records the chord a row already had — and hands it over anyway when
    /// the last attempt did not land ([`SettingsStore::store`]'s rule, review
    /// row R4-10).
    pub fn store(&mut self, overrides: Vec<BindingOverrideV1>) -> bool {
        self.store_on(overrides, keybindings_platform())
    }

    /// [`Self::store`], written in `writing_platform`'s dialect.
    pub(crate) fn store_on(
        &mut self,
        overrides: Vec<BindingOverrideV1>,
        writing_platform: KeybindingsPlatformV1,
    ) -> bool {
        self.document
            .store(keybindings_document(overrides, writing_platform), false)
    }

    /// **An update's trial was committed: the departures as they stand are
    /// handed over** (`update_trial`).
    pub fn release_trial(&mut self) {
        self.document.send_owed();
    }

    /// **Ask for the file to be read again, because the storage watch heard
    /// it** (R33, owner ruling Q3 2026-10-11). The read runs on the storage
    /// worker; a file that changed is stored for the merge
    /// ([`Self::begin_merge`]), one that will not parse is not, and the table
    /// in force stays.
    pub(crate) fn ask_reread(&mut self) {
        self.document.ask_read();
    }

    /// Whether a re-read of `keybindings.json` is still to be answered.
    pub(crate) fn reading(&self) -> bool {
        self.document.reading()
    }

    /// **A storage pass's merge, purely** (design note 2026-10-11 §3.4, (b)8,
    /// (c)1): every file stored for the merge is merged by action, then by
    /// chord, against its base ([`keybindings_merge::merge`]) and folded into
    /// `desired`, which moves the revision to the file the merge read and
    /// releases the hold. Nothing is put in force here: the answer is the
    /// merged document for the window's door (`keybindings_merge::run_pass`),
    /// with what the pass says — `None` when nothing was merged.
    ///
    /// [`keybindings_merge::merge`]: crate::keybindings_merge::merge
    pub(crate) fn begin_merge(
        &mut self,
        platform: bt_platform::HostPlatform,
    ) -> Option<crate::keybindings_merge::KeybindingsPass> {
        let at_quit = self.merged_at_quit.take();
        if !self.document.merge_waits() && at_quit.is_none() {
            return None;
        }
        let merges = self.document.merge_stored(|base, ours, theirs| {
            let merged = crate::keybindings_merge::merge(base, ours, theirs, platform);
            (merged.document, merged.about)
        });
        if merges.is_empty() && at_quit.is_none() {
            return None;
        }
        let mut about = crate::keybindings_merge::About::default();
        for merge in merges {
            about.add(merge);
        }
        Some(crate::keybindings_merge::KeybindingsPass::new(
            self.document.desired().clone(),
            platform,
            about,
            at_quit.unwrap_or_default(),
        ))
    }

    /// **A quit's merge** (revision (c)2): [`Self::begin_merge`]'s merge and
    /// fold, and nothing put in force — no door runs, the windows are leaving.
    /// Answers its one `diagnostics.log` line ([`SettingsStore`]'s
    /// `merge_at_quit`, for its reason); what it merged is kept for the next
    /// storage pass, which puts it in force should the quit be cancelled.
    fn merge_at_quit(&mut self, platform: bt_platform::HostPlatform) -> Option<String> {
        if !self.document.merge_waits() {
            return None;
        }
        let merges = self.document.merge_stored(|base, ours, theirs| {
            let merged = crate::keybindings_merge::merge(base, ours, theirs, platform);
            (merged.document, merged.about)
        });
        if merges.is_empty() {
            return None;
        }
        let mut about = crate::keybindings_merge::About::default();
        for merge in merges {
            about.add(merge);
        }
        let line = about.said_at_quit();
        self.merged_at_quit
            .get_or_insert_with(Default::default)
            .add(about);
        Some(line)
    }

    /// **A storage pass's merge, its last step**: the door has run, and the
    /// merged document is handed to the worker, conditional on the file the
    /// merge read, if it differs from it.
    pub(crate) fn end_merge(&mut self) {
        self.document.send_owed();
    }

    /// **Take what the storage worker has answered** (`AppEvent::DocumentsAnswered`):
    /// whether a re-read found a file that would not parse.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        self.document.take_answers()
    }

    /// The document half, for a test to look into.
    #[cfg(test)]
    pub(crate) fn document(&mut self) -> &mut DocumentState<KeybindingsV1> {
        &mut self.document
    }

    /// Let the worker answer everything this store asked, and take it.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) {
        let _ = self.document.settle();
    }
}

pub(crate) fn keybindings_platform() -> KeybindingsPlatformV1 {
    keybindings_platform_on(bt_platform::host_platform())
}

pub(crate) fn keybindings_platform_on(
    platform: bt_platform::HostPlatform,
) -> KeybindingsPlatformV1 {
    match platform {
        bt_platform::HostPlatform::Windows => KeybindingsPlatformV1::Windows,
        bt_platform::HostPlatform::MacOs => KeybindingsPlatformV1::MacOs,
        bt_platform::HostPlatform::OtherUnix => KeybindingsPlatformV1::OtherUnix,
    }
}

pub(crate) fn keybindings_document(
    bindings: Vec<BindingOverrideV1>,
    writing_platform: KeybindingsPlatformV1,
) -> KeybindingsV1 {
    KeybindingsV1 {
        schema_version: KEYBINDINGS_SCHEMA_VERSION,
        writing_platform,
        bindings,
    }
}

/// The name the profile file wears on disk, which is also what a notice about it
/// has to say out loud.
pub const PROFILES_FILE_NAME: &str = "profiles.json";

/// `profiles.json` — the profile table's departures from the shipped five, and
/// when they reach the disk.
///
/// [`KeybindingsStore`]'s shape, deliberately: two files with the same job —
/// hold a list a person may also edit by hand — should not have two different
/// stores behind them. No debouncer for the same reason, sharpened: a reorder or
/// a duplicate happens because somebody pressed a button and watched a list move,
/// and a quiet window is a window in which exactly that can be lost.
///
/// **Nothing is written until something changes.** A machine that has never
/// touched a profile has no such file, and gets none: a feature does not announce
/// itself by putting an empty document in everybody's `%APPDATA%`
/// (`schemes.rs`'s judgment, and the same one).
pub struct ProfilesStore {
    /// The table, what the file holds — what a re-read compares with — and the
    /// requests on the worker.
    document: DocumentState<ProfilesV1>,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    /// **What a quit merged and did not put in force** ([`SettingsStore`]'s
    /// rule, one file over): the next storage pass installs the table and says
    /// it, should the quit be cancelled.
    merged_at_quit: Option<crate::settings_merge::Account>,
}

/// `profiles.json` in a quit and on a stop: its [`DocumentState`]'s answers (T-QUIT-DOCUMENTS).
impl QuitDocument for ProfilesStore {
    fn file(&self) -> &'static str {
        self.document.file()
    }

    fn quit_begin(&mut self) {
        self.document.quit_begin();
    }

    /// A turn that folds a changed file in before it sends: a `Moved` or
    /// `Displaced` answer at quit is merged as the running window merges it,
    /// without the door.
    fn quit_turn(&mut self) -> Option<String> {
        let _ = self.document.take_answers();
        let merged = self.merge_at_quit();
        if self.document.standing() == crate::quit::Standing::Unsent {
            self.document.send_owed();
        }
        merged
    }

    fn standing(&self) -> crate::quit::Standing {
        self.document.standing()
    }

    fn keep_copy(&mut self, folder: &std::path::Path, at: std::time::SystemTime) {
        self.document.keep_copy(folder, at);
    }

    fn copy_standing(&mut self) -> crate::quit::CopyStanding {
        self.document.copy_standing()
    }

    fn drop_owed(&mut self) -> Option<String> {
        self.document.drop_owed()
    }

    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>> {
        self.document.owed_bytes()
    }
}

impl ProfilesStore {
    /// Read `profiles.json`, falling back to *no departures* on every failure.
    /// Its writes and re-reads go to the storage worker on `lane`.
    pub(crate) fn open(lane: StorageLane) -> Self {
        let dir = storage_dir();
        make_data_folder(&dir);
        Self::at_on(dir.join(PROFILES_FILE_NAME), lane)
    }

    /// The same store over a named file, which is what makes the re-read
    /// testable: everything below this line is about a path, and only [`open`]
    /// knows which path this process's is.
    ///
    /// [`open`]: Self::open
    pub(crate) fn at_on(path: PathBuf, lane: StorageLane) -> Self {
        let (file, report, bytes) =
            read_profiles_keeping_bytes(&path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &path, keep_profiles);
        // §5.4 case 1 — no file — is the ordinary state of nearly every machine
        // and must not alert. Everything else must, naming the file (§5.3).
        let fault = read_fault(
            &report,
            PROFILES_FILE_NAME,
            crate::i18n::profiles_file_unreadable,
        );
        let writer_of_record = is_writer_of_document(&path);
        Self {
            document: DocumentState::opened(
                PROFILES_FILE_NAME,
                path,
                Revision::opened(file, bytes),
                writer_of_record,
                crate::update_trial::Writer::Profiles,
                lane,
            ),
            fault,
            merged_at_quit: None,
        }
    }

    /// [`Self::at_on`], with a worker of its own.
    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self::at_on(path, StorageLane::of_its_own())
    }

    /// The table as it stands.
    pub fn loaded(&self) -> &ProfilesV1 {
        self.document.projection()
    }

    /// What `profiles.json` is known to hold.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> &Revision<ProfilesV1> {
        self.document.revision()
    }

    /// How many writes of this document the reader has asked for
    /// (`storage_watch::StorageWatch::advance`).
    pub(crate) fn write_generation(&self) -> u64 {
        self.document.write_generation()
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame. A write that has stopped landing comes through the same door.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.document.take_fault())
    }

    /// **The table the installer put in force from the file** — what a press is
    /// measured against. The rows it refused stay in the document and out of
    /// this ([`DocumentState::project`]).
    pub(crate) fn project(&mut self, table: ProfilesV1) {
        self.document.project(table);
    }

    /// **Ask for the file to be read again, because the storage watch heard it**
    /// (§7.1.6c-6d). The read runs on the storage worker; its answer is taken by
    /// [`Self::take_answers`].
    ///
    /// Three answers, and each of them is a rule this slice had to choose:
    ///
    /// * a file whose bytes are the revision's — compared with the revision's
    ///   bytes, or with the write on its way, never with the document in memory —
    ///   is no news, and nothing else happens. This window's own writes to
    ///   `profiles.json` are the loudest of the file's changes, and a change this
    ///   store holds but has not written is not undone by a read of the older
    ///   file. A file with new bytes and the revision's document — an editor that
    ///   re-indents — is no news either, and its bytes become the revision's;
    /// * a document that parses and differs is stored for the merge
    ///   ([`Self::begin_merge`]), which folds it in and makes it the revision —
    ///   what makes the next re-read quiet — and the store holds its writes until
    ///   then; a change made here that is not yet on disk is merged with it, by
    ///   id and by field, never undone;
    /// * a document that will not parse is **not** taken, and what was already
    ///   in force stays in force — with no copy of it kept beside it: the file is
    ///   the person's, under its own name, and nothing will replace it. Falling
    ///   back to the shipped table here would be the worst of both — the reader's
    ///   list would empty *because* they typed a comma wrong. That is
    ///   `reread_schemes`'s own ruling for the scheme file in use, met one file
    ///   over. Startup is different and stays different ([`Self::open`]): there
    ///   is nothing yet in force to keep.
    pub(crate) fn ask_reread(&mut self) {
        self.document.ask_read();
    }

    /// Whether a re-read of `profiles.json` is still to be answered.
    pub(crate) fn reading(&self) -> bool {
        self.document.reading()
    }

    /// Record the table as it stands now and hand it to the storage worker.
    ///
    /// Returns whether anything changed, so a caller can skip the write when a
    /// press moved nothing.
    pub fn store(&mut self, file: ProfilesV1) -> bool {
        self.document.store(file, false)
    }

    /// **A storage pass's merge, the store's half** (design note 2026-10-11
    /// §3.4, (b)8, (c)1): every file stored for the merge is merged by id, by
    /// field and by order ([`crate::profiles_merge::merge`]) and folded into
    /// `desired`, which moves the revision to the file the merge read; the
    /// merged document is then what the installer reads — the door,
    /// `Runtime::install_merged_profile_table`, which runs after this and
    /// re-points nothing by position. `None` when nothing was merged.
    pub(crate) fn begin_merge(&mut self) -> Option<crate::table_merge::MergePass> {
        let at_quit = self.merged_at_quit.take();
        if !self.document.merge_waits() && at_quit.is_none() {
            return None;
        }
        let merges = self.document.merge_stored(merge_profiles);
        if merges.is_empty() && at_quit.is_none() {
            return None;
        }
        let mut account = crate::settings_merge::Account::default();
        for merge in merges {
            account.add(merge);
        }
        self.document.project(self.document.desired().clone());
        Some(crate::table_merge::MergePass::new(
            PROFILES_FILE_NAME,
            account,
            at_quit.unwrap_or_default(),
        ))
    }

    /// **A quit's merge**: [`Self::begin_merge`]'s fold, and nothing put in
    /// force — the windows are leaving. Answers its one `diagnostics.log` line
    /// ([`SettingsStore`]'s `merge_at_quit`, for its reason); what it merged is
    /// kept for the next storage pass, which installs it should the quit be
    /// cancelled.
    fn merge_at_quit(&mut self) -> Option<String> {
        if !self.document.merge_waits() {
            return None;
        }
        let merges = self.document.merge_stored(merge_profiles);
        if merges.is_empty() {
            return None;
        }
        let mut account = crate::settings_merge::Account::default();
        for merge in merges {
            account.add(merge);
        }
        let line = account.said(PROFILES_FILE_NAME, crate::settings_merge::Said::AtQuit);
        self.merged_at_quit
            .get_or_insert_with(Default::default)
            .add(account);
        Some(line)
    }

    /// **A storage pass's merge, its last step**: the merged document is handed
    /// to the worker, conditional on the file the merge read, if it differs
    /// from it — the units that kept Folio's value.
    pub(crate) fn end_merge(&mut self) {
        self.document.send_owed();
    }

    /// **An update's trial was committed: the table as it stands is handed
    /// over** (`update_trial`).
    pub fn release_trial(&mut self) {
        self.document.send_owed();
    }

    /// **Take what the storage worker has answered** (`AppEvent::DocumentsAnswered`):
    /// the re-reads' news for the reader.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        self.document.take_answers()
    }

    /// Let the worker answer everything this store asked, and take it.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) -> ReadNews {
        self.document.settle()
    }

    /// **A storage pass, in a test**: the file read again on the worker, the
    /// answer taken, and the store's merge run with the send after it (the
    /// door is the window's). `Changed` when the pass moved what the readers
    /// read.
    #[cfg(test)]
    pub(crate) fn reread(&mut self) -> crate::document_request::Heard {
        self.ask_reread();
        let news = self.settle();
        let before = self.document.projection().clone();
        if self.begin_merge().is_some() {
            self.end_merge();
        }
        match news.heard() {
            crate::document_request::Heard::Unchanged if *self.document.projection() != before => {
                crate::document_request::Heard::Changed
            }
            heard => heard,
        }
    }

    /// A read answer handed in as if the worker had served it now.
    #[cfg(test)]
    pub(crate) fn hear_read(&mut self, read: bt_persist::StableRead) -> ReadNews {
        self.document.hear_read(read)
    }

    /// Whether a file's bytes wait for the merge.
    #[cfg(test)]
    pub(crate) fn merge_waits(&self) -> bool {
        self.document.merge_waits()
    }
}

/// **What the storage worker's answers came to, for the window** (`AppEvent::DocumentsAnswered`).
#[derive(Debug, Default)]
pub(crate) struct DocumentsHeard {
    /// Whether a re-read of `settings.json` found a file that would not parse.
    pub(crate) settings: ReadNews,
    /// Whether a re-read of `keybindings.json` found a file that would not
    /// parse.
    pub(crate) keybindings: ReadNews,
    /// What the re-reads of `profiles.json` put in force.
    pub(crate) profiles: ReadNews,
    /// What the re-reads of `pins.json` put in force.
    pub(crate) pins: ReadNews,
    /// The sentences a write that will not land owes the reader, one per file.
    pub(crate) faults: Vec<String>,
}

/// **Every store takes every answer, then each says what it owes** (design note
/// 2026-10-11 (b)4, (b)7): the answers in the order the worker served them, and
/// then each store's fault — the card a write that will not land is owed after
/// start, which nothing raised before this ticket.
pub(crate) fn hear_documents(
    settings: &mut SettingsStore,
    keybindings: &mut KeybindingsStore,
    profiles: &mut ProfilesStore,
    pins: &mut crate::pins::PinsStore,
) -> DocumentsHeard {
    let settings_news = settings.take_answers();
    let keybindings_news = keybindings.take_answers();
    let profiles_news = profiles.take_answers();
    let pins_news = pins.take_answers();
    let faults = [
        settings.take_fault(),
        keybindings.take_fault(),
        profiles.take_fault(),
        pins.take_fault(),
    ]
    .into_iter()
    .flatten()
    .collect();
    DocumentsHeard {
        settings: settings_news,
        keybindings: keybindings_news,
        profiles: profiles_news,
        pins: pins_news,
        faults,
    }
}

/// **Whether any of the four stores has a re-read still to be answered** — the
/// batch of re-reads a storage pass asked is not complete, and its one card
/// waits (`MergeNotice::gather`; T-STORE-NOTICE, design note 2026-10-11 §3.6).
pub(crate) fn documents_reading(
    settings: &SettingsStore,
    keybindings: &KeybindingsStore,
    profiles: &ProfilesStore,
    pins: &crate::pins::PinsStore,
) -> bool {
    settings.reading() || keybindings.reading() || profiles.reading() || pins.reading()
}

/// **`profiles.json`'s merge, as the store's seam takes it**: a built-in row no
/// side titles is named by the title the table in force draws for it.
fn merge_profiles(
    base: &ProfilesV1,
    ours: &ProfilesV1,
    theirs: &ProfilesV1,
) -> (ProfilesV1, crate::settings_merge::Account) {
    let shipped_title = |id: &str| crate::profiles::row_of(id).map(|row| row.display_title);
    let usable = crate::profiles::installable();
    let merged = crate::profiles_merge::merge(base, ours, theirs, &shipped_title, &usable);
    (merged.document, merged.account)
}

/// `profiles.json` read again with a refused file's copy kept — what an update's
/// trial owed it (`update_trial::owe_copy`).
fn keep_profiles(path: &Path) {
    let _ = read_profiles(path);
}

/// **Make the data folder a store is about to open in**, and answer whether it
/// is there — unless an update's trial holds its writes back (`update_trial`,
/// F-7), which makes nothing and answers whether it was there already. The
/// commit makes it then, before the first document it released is written.
pub(crate) fn make_data_folder(dir: &Path) -> bool {
    if crate::update_trial::defer(crate::update_trial::Writer::DataFolder) {
        return dir.is_dir();
    }
    std::fs::create_dir_all(dir).is_ok()
}

/// The file a settings write resolves to, under the data root it belongs to.
fn settings_write_path(data_root: &Path, file_name: &str) -> PathBuf {
    data_root.join(file_name)
}

/// The directory this build wrote its files under before the product was named,
/// and the only reason this module knows the old brand at all.
///
/// It exists for one startup, on one machine, once: see [`relocate`].
pub(crate) const PREVIOUS_STORAGE_NAME: &str = "BetterTerminal";

/// The directory the product writes under, which is its name.
pub(crate) const STORAGE_NAME: &str = "Folio";

/// Where this build keeps its files, and whether anything has to be carried
/// there first.
///
/// Two fields rather than one path because the second question has a different
/// answer on each platform and the first one does not carry it: a directory
/// that exists is not evidence that a previous name ever did.
#[derive(Debug, PartialEq, Eq)]
struct StorageLocation {
    /// The directory itself, with [`STORAGE_NAME`] already joined on.
    directory: PathBuf,
    /// The directory the same product wrote under before it was named, on the
    /// one platform that has such a history. `None` is what keeps [`relocate`]
    /// from being called at all.
    previous: Option<PathBuf>,
}

/// **Which directory this platform keeps a program's files in**, as a decision
/// rather than as a `cfg` — `bt-app` asks [`bt_platform::host_platform`] what
/// machine this is, and `only_the_named_files_decide_what_platform_this_is`
/// keeps that true of this file (see `docs/plans/port/macos-plan-2026-09-12.md`
/// §4.3). The environment is handed in for the same reason the platform is: a
/// process-wide variable changed from a test is changed for every other test
/// running beside it, so the three arms are pinned by calling this with the
/// values instead of with a machine.
///
/// - **Windows:** `%APPDATA%\Folio\` (§1.2), and `%APPDATA%\BetterTerminal\` is
///   the name to carry over.
/// - **macOS:** `~/Library/Application Support/Folio`, and **nothing to carry**
///   — the `BetterTerminal` → `Folio` rename is a Windows-only history, because
///   the product never shipped under the old name on this platform. A
///   relocation offered here could only ever find a directory somebody else
///   made, and moving that would be worse than ignoring it.
/// - **Other Unix** (not a shipped platform): `$XDG_DATA_HOME`, or
///   `~/.local/share` when it is unset, joined with `Folio`.
///
/// Each arm falls back to the process temp directory when the variable naming
/// the home is unset — the panic log's reasoning, which the Windows arm has
/// always used: a diagnostic that cannot be written is worse than one written
/// somewhere less convenient.
fn storage_location(
    platform: bt_platform::HostPlatform,
    env: impl Fn(&str) -> Option<OsString>,
) -> StorageLocation {
    /// A home directory with the platform's data sub-path joined on, or the
    /// process temp directory when the environment did not say where home is.
    fn under(home: Option<OsString>, parts: &[&str]) -> PathBuf {
        let Some(home) = home else {
            return std::env::temp_dir();
        };
        let mut path = PathBuf::from(home);
        path.extend(parts);
        path
    }
    match platform {
        bt_platform::HostPlatform::Windows => {
            let appdata = under(env("APPDATA"), &[]);
            StorageLocation {
                directory: appdata.join(STORAGE_NAME),
                previous: Some(appdata.join(PREVIOUS_STORAGE_NAME)),
            }
        }
        bt_platform::HostPlatform::MacOs => StorageLocation {
            directory: under(env("HOME"), &["Library", "Application Support"]).join(STORAGE_NAME),
            previous: None,
        },
        bt_platform::HostPlatform::OtherUnix => {
            let data_home = match env("XDG_DATA_HOME") {
                Some(explicit) => PathBuf::from(explicit),
                None => under(env("HOME"), &[".local", "share"]),
            };
            StorageLocation {
                directory: data_home.join(STORAGE_NAME),
                previous: None,
            }
        }
    }
}

/// `%APPDATA%\Folio\` on Windows, `~/Library/Application Support/Folio` on
/// macOS — [`storage_location`] holds the rule and the reasons — as
/// [`settle_storage_dir`] settled it.
///
/// **Resolved once per process, and the move a rename owes the user is paid by
/// one named call.** Until 2026-10-24 this was a lazily initialised cell, and
/// the first caller — whichever it was — paid for the relocation; the doc here
/// argued that a relocation stapled to `main` would leave the answer depending
/// on whether that line ran. **Reversed on purpose**
/// (T-LAZY-INITIALIZERS-BEFORE-THE-LOOP): a cell any caller may initialise is a
/// cell a request door on the window thread may initialise, and the side-effects contract C1 needs that
/// proven impossible, not merely unlikely. So the move is
/// [`settle_storage_dir`]'s, `main` calls it after the update pass and before
/// the hand-over (its pin in `window_waits_tests`), its witness is what
/// `FolioApp::new` takes, and a read before it panics rather than moving
/// anything — the order no longer depends silently on a line, it is held by the
/// types and by the pinned place.
///
/// # Panics
///
/// Before [`settle_storage_dir`] ran in this process.
pub fn storage_dir() -> PathBuf {
    DIRECTORY.get().0.clone()
}

/// **The data directory as [`settle_storage_dir`] settled it** — a value only this module makes,
/// so the witness `FolioApp::new` takes (`Settled<DataDirectory>`) can only come from this cell's
/// settle, as `Settled<profiles::Registry>` can only come from the profile table's (review N1).
pub(crate) struct DataDirectory(PathBuf);

impl DataDirectory {
    /// The folder.
    pub(crate) fn data_folder(&self) -> &Path {
        &self.0
    }

    /// A test's own folder, for a witness over a value of the test's (`Settled::planted`).
    #[cfg(test)]
    pub(crate) fn planted(path: PathBuf) -> Self {
        Self(path)
    }
}

/// The data directory, once [`settle_storage_dir`] has made it.
#[cfg(not(test))]
static DIRECTORY: SettledOnce<DataDirectory> = SettledOnce::new(DIRECTORY_IS);

/// A test process's data directory is a scratch folder of its own, never `%APPDATA%`: a test
/// that reads [`storage_dir`] without settling it (it has no `main`) is settled there. A test
/// that drives a start in a process of its own calls [`settle_storage_dir`] as `main` does.
#[cfg(test)]
static DIRECTORY: SettledOnce<DataDirectory> =
    SettledOnce::settled_in_a_test_by(DIRECTORY_IS, || {
        DataDirectory(bt_testpath::temp_path("bt-app-test-process-data-directory"))
    });

/// What [`DIRECTORY`] holds, for the panic of a read before it is settled.
const DIRECTORY_IS: &str = "the data directory (persist::storage_dir)";

/// **Settle the data directory: resolve it, and pay the move a rename owes**,
/// once per process (later calls answer the first). Called by `fn main` in the
/// claim, after `update_startup::pass` and before the hand-over, and by the
/// `--remove-shell-integration` door, which reads the folder it removes from;
/// a process that must leave the folder as it found it reads
/// [`storage_dir_as_it_stands`] or [`storage_dir_unmoved`] instead and never
/// settles.
pub(crate) fn settle_storage_dir() -> Settled<DataDirectory> {
    DIRECTORY.settle(|| DataDirectory(relocated_storage_dir()))
}

/// [`settle_storage_dir`]'s body: the directory, moved from its previous name
/// when one is owed.
fn relocated_storage_dir() -> PathBuf {
    // A closure rather than `std::env::var_os` itself: the function item
    // is generic over the key's type, and handing it over directly binds
    // one lifetime where the parameter asks for any.
    let location = storage_location(bt_platform::host_platform(), |name: &str| {
        std::env::var_os(name)
    });
    let current = location.directory;
    let Some(previous) = location.previous else {
        return current;
    };
    // **An update's trial moves nothing** (`update_trial`, F-7): it runs
    // in the folder the old build left, as a start whose move failed
    // does below. The move is the next start's; this one's folder is in
    // use by then.
    if crate::update_trial::defer(crate::update_trial::Writer::DataFolderMove) {
        return as_it_stands(current, previous);
    }
    match relocate(&previous, &current) {
        Relocation::Nothing | Relocation::AlreadyHere => current,
        Relocation::Moved => {
            eprintln!(
                "BT_PERSIST moved {} to {}",
                previous.display(),
                current.display()
            );
            current
        }
        Relocation::Failed(error) => {
            // Fail-soft, and deliberately the *old* directory rather than
            // an empty new one: a user whose files could not be moved
            // keeps reading and writing the files they already have.
            // Starting fresh beside them would present as "the terminal
            // forgot everything" while the data sat one directory away.
            eprintln!(
                "BT_PERSIST could not move {} to {}: {error} — continuing in the old \
                 directory",
                previous.display(),
                current.display()
            );
            previous
        }
    }
}

/// **The data directory as the disk has it now, with nothing moved and nothing
/// made** (B-EXPLORER-CLAIM) — the folder [`storage_dir`] would answer if the
/// rename it owes were not this process's to pay: the folder under the previous
/// name while that is the only one there, the current one otherwise.
///
/// For the language lookups of the processes that never have a window
/// ([`SettingsStore::peek_language`]'s two callers), which must read the
/// settings the user's Folio reads and must leave the directory exactly as they
/// found it — the move is the next resident start's.
pub(crate) fn storage_dir_as_it_stands() -> PathBuf {
    let location = storage_location(bt_platform::host_platform(), |name: &str| {
        std::env::var_os(name)
    });
    match location.previous {
        Some(previous) => as_it_stands(location.directory, previous),
        None => location.directory,
    }
}

/// Which of the two names holds the files while nothing has been moved: the
/// previous one only when it is there and the current one is not.
pub(crate) fn as_it_stands(current: PathBuf, previous: PathBuf) -> PathBuf {
    if previous.is_dir() && !current.exists() {
        previous
    } else {
        current
    }
}

/// **The data directory by [`storage_location`]'s rule, and nothing else** —
/// no relocation, no creation: for a process that must not change the data
/// directory, the rescue build's recovery door (`update_recover`, U-22), which
/// only appends its one line to the `diagnostics.log` there.
pub(crate) fn storage_dir_unmoved() -> PathBuf {
    storage_location(bt_platform::host_platform(), |name: &str| {
        std::env::var_os(name)
    })
    .directory
}

/// What [`relocate`] found, and did.
///
/// No `PartialEq`: the failure arm carries the `io::Error` the user has to be
/// told about, and two of those are not comparable in any sense this code means.
#[derive(Debug)]
enum Relocation {
    /// No directory under the old name: a first run, or a machine that only ever
    /// knew this one. Nothing to carry.
    Nothing,
    /// Both names exist. The move already happened — or the user made the new
    /// directory themselves — and either way the current one is authoritative.
    /// The old one is left exactly where it is: it is not this code's to delete,
    /// and a stale copy is cheaper than a wrong deletion.
    AlreadyHere,
    /// The whole directory arrived under the new name.
    Moved,
    /// It could not, and the old directory is still the one with the files in it.
    Failed(std::io::Error),
}

/// Carry `previous` over to `current`, once, if that is what the disk says is
/// needed.
///
/// **One rename of the directory itself, not a walk that copies files.** The
/// contents are `session.json`, `settings.json`, a lock sentinel and the
/// installed shell-integration script — a set this code should not have to
/// enumerate, and would be wrong about the moment anything is added. A directory
/// rename within one volume is also atomic where a copy is not: it either
/// happened or it did not, and there is no state in which half the user's
/// settings are under each name.
///
/// The three states are decided by the two directories alone, so the answer does
/// not depend on a flag file that could be lost, and calling this again after a
/// successful move returns [`Relocation::Nothing`] rather than moving anything a
/// second time.
fn relocate(previous: &Path, current: &Path) -> Relocation {
    if !previous.is_dir() {
        return Relocation::Nothing;
    }
    if current.exists() {
        return Relocation::AlreadyHere;
    }
    // The parent is `%APPDATA%` and already exists — `previous` is inside it —
    // so there is nothing to create before the rename.
    match std::fs::rename(previous, current) {
        Ok(()) => Relocation::Moved,
        Err(error) => Relocation::Failed(error),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::document_request::Heard;
    use bt_source::{Index, ItemQuery, Needle, Pattern, Scope, Search, View};

    /// RED (T-HARDCODE-047 round 2, SET-3) — **the ordinary keybindings
    /// writer records the dialect whose chord spelling it writes.**
    ///
    /// MUTATION: write `KeybindingsPlatformV1::Unknown` from
    /// `KeybindingsStore::store_on`; the serialized platform and the
    /// opposite-platform command-modifier translation both fail.
    #[test]
    fn keybindings_store_writes_each_platform_dialect_and_it_imports_by_meaning() {
        let root = bt_testpath::temp_path("bt-app-keybindings-platform");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private keybindings directory");

        for (source, dialect, chord, target, expected) in [
            (
                bt_platform::HostPlatform::MacOs,
                KeybindingsPlatformV1::MacOs,
                "Cmd+Shift+Y",
                bt_platform::HostPlatform::Windows,
                "Ctrl+Shift+y",
            ),
            (
                bt_platform::HostPlatform::Windows,
                KeybindingsPlatformV1::Windows,
                "Ctrl+Shift+Y",
                bt_platform::HostPlatform::MacOs,
                "Cmd+Shift+y",
            ),
            (
                bt_platform::HostPlatform::OtherUnix,
                KeybindingsPlatformV1::OtherUnix,
                "Ctrl+Shift+Y",
                bt_platform::HostPlatform::MacOs,
                "Cmd+Shift+y",
            ),
        ] {
            let path = root.join(format!("{dialect:?}.json"));
            let mut store = KeybindingsStore::at(path.clone());
            store.store_on(
                vec![BindingOverrideV1 {
                    action: "new-tab".to_owned(),
                    chord: Some(chord.to_owned()),
                }],
                keybindings_platform_on(source),
            );
            store.settle();

            let (written, report) = read_keybindings(&path);
            assert_eq!(report, ReadReport::Loaded, "{}", path.display());
            assert_eq!(written.writing_platform, dialect);
            let compact = serde_json::to_string(&written).expect("the written file serializes");
            assert!(
                compact.contains(&format!(
                    "\"writing_platform\":{}",
                    serde_json::to_string(&dialect).unwrap()
                )),
                "{compact}"
            );

            let (imported, faults) = crate::settings_bundle::import_shortcuts(&written, target);
            assert!(faults.is_empty(), "{faults:?}");
            assert_eq!(
                crate::shortcuts::format_chord_on(
                    imported
                        .row("new-tab")
                        .and_then(|row| row.chord.as_ref())
                        .expect("new-tab keeps a chord"),
                    target,
                ),
                expected
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-34, round 2; Codex's review, finding 5) — **letting go of every
    /// claim never waits for a claim table another thread holds**: it runs in
    /// O's panic hook, possibly on the thread that holds the table, and gives
    /// up at once rather than deadlock.
    ///
    /// MUTATION: in `let_go_of_every_claim`, `lock()` the table instead of
    /// trying it.
    #[test]
    fn letting_go_of_the_claims_never_waits_for_a_held_table() {
        let (held, release) = std::sync::mpsc::channel::<()>();
        let (holding, is_held) = std::sync::mpsc::channel::<()>();
        let holder = bt_platform::spawn_at_priority(
            "bt-u34-claims",
            bt_platform::ThreadPriority::BelowNormal,
            move |_worker| {
                let _table = claims().lock().expect("the table");
                holding.send(()).unwrap();
                let _ = release.recv_timeout(std::time::Duration::from_secs(10));
            },
        )
        .unwrap();
        is_held.recv().unwrap();
        let began = std::time::Instant::now();
        let_go_of_every_claim();
        let took = began.elapsed();
        held.send(()).unwrap();
        holder.join().unwrap();
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
    }

    /// The writer's retirement through its admitted door (`doors::SessionWriterRetire`), on a test
    /// thread already entered as the window thread on its way out.
    fn retire(writer: &mut SessionWriter) {
        admitted::<doors::SessionWriterRetire, _>(|token| writer.close(token))
            .expect("admitted on the way out");
    }

    fn session_with_tabs(count: usize) -> SessionV1 {
        SessionV1 {
            windows: vec![bt_persist::SessionWindowV1 {
                tabs: (0..count)
                    .map(|index| {
                        crate::test_support::saved_tab(
                            "default",
                            "/launch-backup",
                            Some(&format!("tab-{index}")),
                            false,
                        )
                    })
                    .collect(),
                ..bt_persist::SessionWindowV1::default()
            }],
            ..SessionV1::default()
        }
    }

    fn write_session_fixture(path: &Path, session: &SessionV1) -> Vec<u8> {
        let bytes = bt_persist::serialize_session(session).expect("the fixture serialises");
        std::fs::write(path, &bytes).expect("the fixture session is laid down");
        bytes
    }

    /// RED (T-STORE-PWSH-AND-SESSION-BACKUP item 2) — **the backup is the
    /// layout read at launch, not the first layout this run later saves.**
    ///
    /// MUTATION: enqueue `SessionBackup` after `Session`, or serialize
    /// `self.session` from `hand_over`; the backup contains one tab instead of
    /// three. Drop the launch request and it is absent. Reserialize the loaded
    /// model and the unknown mixed-script field is lost.
    #[test]
    fn launch_keeps_the_session_it_read_before_this_runs_first_write() {
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-launch-backup");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private session directory");
        let path = root.join(SESSION_FILE_NAME);
        let previous = session_with_tabs(3);
        let serialized = bt_persist::serialize_session(&previous).expect("the fixture serialises");
        let mut launch_text = String::from_utf8(serialized).expect("session JSON is UTF-8");
        launch_text = launch_text.replacen("{\n", "{\n  \"future_note\": \"保留—keep\",\n", 1);
        let launch_bytes = launch_text.into_bytes();
        std::fs::write(&path, &launch_bytes).expect("the previous run's session");

        let mut store = SessionStore::launched_at(path.clone(), root.join(SENTINEL_FILE_NAME));
        let current = session_with_tabs(1);
        store.record(current.clone(), Instant::now());
        // Handed over as the autosave does, and the session write's receipt awaited on the
        // writer's channel within the suite's patience: the backup is queued ahead of it and
        // sends no receipt of its own, so the receipt says both have been written. How fast that
        // is on a loaded disk is not this test's subject, and the session's budget would make it
        // one.
        store.hand_over();
        let landed = crate::lane::receive(&store.writer.receipts, "the session write's receipt");
        assert_eq!(landed.result, Ok(()));

        let backup_path = root.join(PREVIOUS_SESSION_FILE_NAME);
        assert_eq!(read_session(&backup_path).0, previous);
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            launch_bytes,
            "the backup is the exact document read, including fields this build ignores"
        );
        assert_eq!(read_session(&path).0, current);

        retire(&mut store.writer);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORE-PWSH-AND-SESSION-BACKUP item 2) — **an empty launch does
    /// not erase the last useful generation.**
    ///
    /// MUTATION: remove the tab-presence guard in `launch_backup_request`; the
    /// prior backup becomes the empty session.
    #[test]
    fn an_empty_session_leaves_the_prior_backup_untouched() {
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-empty-session-launch-backup");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private session directory");
        let path = root.join(SESSION_FILE_NAME);
        let backup_path = root.join(PREVIOUS_SESSION_FILE_NAME);
        let useful = session_with_tabs(3);
        write_session_fixture(&backup_path, &useful);
        write_session_fixture(&path, &SessionV1::default());

        let mut store = SessionStore::launched_at(path, root.join(SENTINEL_FILE_NAME));
        retire(&mut store.writer);

        assert_eq!(read_session(&backup_path).0, useful);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORE-PWSH-AND-SESSION-BACKUP item 2) — **first run creates no
    /// invented backup.**
    ///
    /// MUTATION: treat `ReadReport::NotFound` as loaded; `session.prev.json`
    /// appears even though no session document was read.
    #[test]
    fn an_absent_session_creates_no_backup() {
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-absent-session-launch-backup");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private session directory");

        let mut store =
            SessionStore::launched_at(root.join(SESSION_FILE_NAME), root.join(SENTINEL_FILE_NAME));
        retire(&mut store.writer);

        assert!(!root.join(PREVIOUS_SESSION_FILE_NAME).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (multiwindow slice E2 phase ③, acceptance gate 1) — **a write that
    /// could not happen says so, and a write that happened says that.**
    ///
    /// The store's final flush is what a quit's decision to leave rests on, so it
    /// has to be *judgeable*. The failure is injected the only way a filesystem
    /// can be made to refuse honestly: a path whose parent directory does not
    /// exist, which is what a store on a volume that went away looks like from
    /// here — no mocked writer, no flag, the real `atomic_write` refusing for a
    /// real reason.
    ///
    /// Red gate: give `flush_judged` the old `flush`'s body — which reports
    /// nothing — and the first assertion cannot even be written; make it return
    /// `Ok(())` unconditionally and the process would go on to hide every window
    /// over a session file it never wrote.
    #[test]
    fn a_session_write_that_could_not_happen_is_reported_as_one() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-quit-flush");
        let _ = std::fs::remove_dir_all(&root);

        // Nothing has created `root`, so `root/session.json` has nowhere to be
        // written and the atomic write's own temporary has nowhere to live.
        let mut refused = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        // The verdicts are this test's subject, not the writer's budget: each wait lasts until
        // the answer, within the suite's patience.
        refused.set_save_budget(crate::lane::PATIENCE);
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        refused.record(document.clone(), Instant::now());
        let verdict = refused.flush_judged();
        assert!(
            verdict.is_err(),
            "a quit must be able to find out that its document did not land"
        );

        // And the same document, with somewhere to go.
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let mut landed = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        landed.set_save_budget(crate::lane::PATIENCE);
        landed.record(document, Instant::now());
        assert_eq!(landed.flush_judged(), Ok(()));
        assert!(root.join("session.json").is_file(), "and it is on the disk");
        // A second flush with nothing pending is honestly `Ok` and writes nothing.
        assert_eq!(landed.flush_judged(), Ok(()));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (0.4.6 U-21) — **an update's quit is answered only by the receipt for the document it
    /// photographed.**
    ///
    /// §C.3: the session write is a named generation, the window thread keeps turning, and it
    /// acts on the receipt. The autosave hands documents to the same writer all the time, so the
    /// store has usually heard *a* landing a moment before the quit hands its own over — and that
    /// landing is somebody else's document. Only the receipt for the quit's own generation may
    /// answer, and a stale one arriving late does not either; what then lands is the photograph,
    /// byte for byte.
    ///
    /// The writer is the real thread and the file the real `atomic_write`'s. Its receipt for the
    /// photograph is relayed through a channel this test holds, so the moment the quit hears it is
    /// decided here and not by the thread's timing.
    ///
    /// MUTATION: in `SessionStore::landing_of`, answer whatever the newest receipt said (drop
    /// `*answered == generation`) — the autosave's earlier landing then answers for the
    /// photograph.
    #[test]
    fn session_receipt_matches_the_final_snapshot() {
        let root = appdata("final-snapshot");
        let mut store = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        let earlier = SessionV1 {
            windows: vec![bt_persist::SessionWindowV1::default()],
            ..SessionV1::default()
        };
        let mut photograph = earlier.clone();
        photograph
            .windows
            .push(bt_persist::SessionWindowV1::default());

        store.record(earlier, Instant::now());
        let first = store
            .hand_over_final()
            .expect("handed over")
            .expect("this store writes");
        let receipt = store
            .writer
            .receipts
            .recv()
            .expect("the writer answers for the earlier document");
        assert_eq!(receipt.generation, first);
        store.apply_receipt(receipt);
        let landed = store
            .landing_of(first)
            .expect("the signalled receipt answers this generation");
        assert_eq!(landed, Ok(()), "an earlier document has landed");

        store.record(photograph.clone(), Instant::now());
        let (relay, held) = mpsc::channel();
        let real = std::mem::replace(&mut store.writer.receipts, held);
        let second = store
            .hand_over_final()
            .expect("handed over")
            .expect("this store writes");
        assert!(second > first, "the photograph is a generation of its own");
        assert_eq!(
            store.landing_of(second),
            None,
            "the earlier landing is not the photograph's"
        );
        relay
            .send(SessionWriteReceipt {
                generation: first,
                result: Ok(()),
            })
            .expect("the relay");
        assert_eq!(
            store.landing_of(second),
            None,
            "nor is an older receipt arriving late"
        );
        let receipt = real.recv().expect("the writer answers for the photograph");
        assert_eq!(receipt.generation, second);
        relay.send(receipt).expect("the relay");
        assert_eq!(store.landing_of(second), Some(Ok(())));
        assert_eq!(
            std::fs::read(root.join("session.json")).expect("the file"),
            bt_persist::serialize_session(&photograph).expect("the photograph serialises"),
            "and what landed is the photograph"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED — **the autosave leaves the window thread before it is known to have landed**
    /// (window-thread unbounded-call sweep, 2026-08-24).
    ///
    /// `flush_if_due` runs on the window thread, once per turn of the event loop. It used to call
    /// `write_session_atomic` there: `File::create`, `write_all`, **`sync_all`**, `rename`, under
    /// a `%APPDATA%` the reader is free to have redirected onto a network share or a cloud-sync
    /// folder. An `fsync` on one of those has no bound anybody in this process can state, and a
    /// terminal that stops answering the mouse because OneDrive was thinking is a terminal that
    /// froze.
    ///
    /// The observable difference between the two designs is exactly this: **whether the verdict
    /// is known when the call returns.** The store is pointed at a directory that does not exist,
    /// so the write cannot succeed. Synchronously that failure was already in hand when
    /// `flush_if_due` came back, and the document was still owed. Off-thread it cannot be: the
    /// call returns having handed the document over and owing nothing more *this turn*, and the
    /// failure arrives later, as a receipt, which is what leaves the document owed to the next
    /// trigger.
    ///
    /// Red gate: put `write_session_atomic` back into `hand_over` and the first assertion goes —
    /// the store already owes the document again when the call returns, because the `fsync`
    /// happened on this thread.
    #[test]
    fn the_autosave_hands_the_document_over_and_hears_the_verdict_later() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-writer");
        let _ = std::fs::remove_dir_all(&root);

        // Nowhere to write: no directory, so not even the atomic write's temporary sibling has a
        // home. The real `atomic_write` refusing for a real reason, as in the quit test above.
        let mut store = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        let changed_at = Instant::now();
        store.record(document, changed_at);
        assert!(store.debouncer.is_dirty(), "a change is owed a write");

        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        assert!(
            !store.owes_a_write(),
            "the turn that hands the document over owes nothing more; a store that already owes \
             it again knows the verdict, which means the fsync happened on this thread"
        );

        // And the verdict does arrive — asked as "has it happened yet", not as "have I slept
        // long enough", so a busy machine delivers the same answer later rather than a different
        // one.
        assert!(
            crate::lane::within_patience(|| {
                store.take_receipts();
                store.owes_a_write()
            }),
            "no receipt ever came back for a write that cannot have succeeded"
        );
        assert!(
            !store.debouncer.is_dirty(),
            "and the refusal owes the document to the next trigger, not to a quiet window"
        );

        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-BUDGET-SEAM) — **the writer's budget is the product's unless a test
    /// sets it, a test sets it for its own store alone, and every wait on the writer honours
    /// it.**
    ///
    /// Every store begins with [`SESSION_SAVE_BUDGET`], the product's three seconds, whatever
    /// another store was given before it; [`SessionStore::set_save_budget`] changes the store it
    /// is called on and no other, made before it or after. Where a wait starting at one instant
    /// ends is read off the writer at a named `now`, so no clock is in the test. And a real wait
    /// is driven: a store given no budget at all, over a writer that never answers, comes back
    /// from `flush_judged` with the timed-out answer naming its own budget — the wait ended on
    /// it and not on the product's three seconds.
    ///
    /// MUTATION: in `SessionWriter::wait_ends`, read [`SESSION_SAVE_BUDGET`] instead of the
    /// field — the store given the suite's patience still ends its waits three seconds out.
    /// MUTATION: keep the budget a test sets in one process-wide value that every writer opens
    /// with — the store made after the setter reads the first store's budget.
    #[test]
    fn the_writers_budget_is_the_products_unless_a_test_sets_it() {
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-budget");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        assert_eq!(SESSION_SAVE_BUDGET, Duration::from_secs(3));
        let now = Instant::now();

        let mut set = SessionStore::at(root.join("会话-set.json"), root.join("set.lock"));
        let mut before = SessionStore::at(root.join("before.json"), root.join("before.lock"));
        assert_eq!(
            set.writer.wait_ends(now),
            now + SESSION_SAVE_BUDGET,
            "a store opens with the product's"
        );
        set.set_save_budget(crate::lane::PATIENCE);
        assert_eq!(
            set.writer.wait_ends(now),
            now + crate::lane::PATIENCE,
            "the setter sets its store's waits"
        );
        assert_eq!(
            before.writer.wait_ends(now),
            now + SESSION_SAVE_BUDGET,
            "a store made before it keeps the product's"
        );
        let mut after = SessionStore::at(root.join("after.json"), root.join("after.lock"));
        assert_eq!(
            after.writer.wait_ends(now),
            now + SESSION_SAVE_BUDGET,
            "and so does a store made after it"
        );
        for store in [&mut set, &mut before, &mut after] {
            retire(&mut store.writer);
        }

        // A real wait, ended by the store's own budget.
        let mut stalled = SessionStore::at(root.join("stalled.json"), root.join("stalled.lock"));
        retire(&mut stalled.writer);
        let (never, release) = a_writer_that_never_answers();
        stalled.writer = never;
        stalled.set_save_budget(Duration::ZERO);
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        stalled.record(document, Instant::now());
        assert_eq!(
            stalled.flush_judged(),
            Err(SaveRefusal::TimedOut(save_did_not_finish(Duration::ZERO))),
            "the wait ends on the store's budget"
        );
        assert!(
            stalled.writer.stalled,
            "and the writer is known to be stalled"
        );

        drop(release);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED — **a quit leaves even when the writer never answers** (T-QUIT-HAS-A-DEADLINE,
    /// crash review C-4).
    ///
    /// The writer thread is stood inside a call that never returns, which is what one parked in
    /// `sync_all` on a share that has stopped answering looks like from this side: the requests
    /// channel still takes documents, the receipts channel is still connected, and no receipt
    /// ever comes back. Both things a quit does with this store then have to end on their own —
    /// `flush_judged`, which the quit's `Write` step waits on, and `close`, which the process
    /// exit calls after it.
    ///
    /// **And it is a timeout and not a refusal** (release review X-8): the quit is entitled to
    /// carry on past it, so the answer says which of the two it was and `session.lock` stays
    /// where it is.
    ///
    /// Red gate: put `recv()` back in `wait_for`, or an unconditional `thread.join()` back in
    /// `SessionWriter::close`, and this test does not fail — it never returns. Report the expiry
    /// as `SaveRefusal::Refused` and the verdict assertion below goes red, which is the quit
    /// keeping its windows open over a disk that has stopped answering.
    #[test]
    fn a_quit_leaves_a_writer_that_never_answers_behind() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-stall");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let sentinel = root.join("session.lock");
        std::fs::write(&sentinel, b"").expect("this run's claim to still be running");

        let mut store = SessionStore::at(root.join("session.json"), sentinel.clone());
        store.armed = true;
        // The real writer this store opened goes first, and the one that never answers takes its
        // place.
        retire(&mut store.writer);
        let (stalled, release) = a_writer_that_never_answers();
        store.writer = stalled;

        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        store.record(document, Instant::now());

        let started = Instant::now();
        let verdict = store.flush_judged();
        let waited = started.elapsed();
        assert_eq!(
            verdict,
            Err(SaveRefusal::TimedOut(save_did_not_finish(
                SESSION_SAVE_BUDGET
            ))),
            "a quit is told the save ran out of its budget, which is not a disk that refused"
        );
        assert!(
            verdict.is_err_and(|refusal| refusal.quit_may_proceed()),
            "and that is the answer the transaction reads as `leave anyway`"
        );
        assert!(
            waited >= SESSION_SAVE_BUDGET && waited < SESSION_SAVE_BUDGET * 3,
            "it waited its budget and then went on, not {waited:?}"
        );

        // And the exit that follows does not spend the same budget again, because this writer
        // has already said it is not answering.
        let started = Instant::now();
        store.close();
        assert!(
            started.elapsed() < SESSION_SAVE_BUDGET,
            "a writer already known to be stalled is not waited on a second time"
        );
        assert!(
            sentinel.is_file(),
            "and this run claims no clean exit over a document nobody heard about"
        );

        drop(release);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (B-ENDSESSION) — **a layout write that misses its budget at the system's question still
    /// answers the system yes, and leaves the last completed save and `session.lock` standing.**
    ///
    /// Rule 1's fallback: the system is never kept waiting past the one bounded wait, and a run
    /// that did not see its document land claims no clean exit. What the disk holds is the last
    /// autosave, which is whole — the hold kept every later change off it. The real platform
    /// answer, the real `session_end` hold and settle, a real first landing through the real
    /// writer, then the writer that never answers.
    ///
    /// MUTATION: in `SessionStore::save_for_the_systems_end`, drop the sentinel whatever
    /// `flush_judged` answered.
    #[test]
    fn a_write_that_misses_the_budget_still_answers_true_and_keeps_the_last_layout() {
        crate::test_support::on_the_window_thread_exiting();
        crate::session_end::forget();
        let root = bt_testpath::temp_path("bt-app-endsession-stall");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let sentinel = root.join("session.lock");
        let mut store = SessionStore::armed_at(root.join("session.json"), sentinel.clone());
        assert!(sentinel.is_file(), "the run's sentinel stands");
        // The last autosave is real and only the setting for what follows: it lands within the
        // suite's patience. The writer that never answers keeps the product's budget, which is
        // what this test is about.
        store.set_save_budget(crate::lane::PATIENCE);

        let last_save = SessionV1 {
            windows: vec![bt_persist::SessionWindowV1::default()],
            ..SessionV1::default()
        };
        store.record(last_save.clone(), Instant::now());
        assert_eq!(store.flush_judged(), Ok(()), "the last autosave lands");
        // An edit made just before the shutdown, owed to the disk when the question comes.
        let mut owed = last_save.clone();
        owed.windows.push(bt_persist::SessionWindowV1::default());
        store.record(owed, Instant::now());
        retire(&mut store.writer);
        let (stalled, release) = a_writer_that_never_answers();
        store.writer = stalled;

        let answered = bt_platform::session_end::answer(
            bt_platform::session_end::WM_QUERYENDSESSION,
            0,
            &crate::session_end::hear,
        );
        assert_eq!(
            answered,
            Some(1),
            "the system is answered yes before anything is written"
        );
        let settled: Vec<_> = crate::session_end::take()
            .into_iter()
            .map(|end| crate::session_end::settle(end, &mut store, false, Instant::now()))
            .collect();
        assert_eq!(
            settled,
            vec![crate::session_end::Settled::Saved(Err(
                SaveRefusal::TimedOut(save_did_not_finish(SESSION_SAVE_BUDGET))
            ))]
        );
        assert!(
            sentinel.is_file(),
            "a run that did not see its document land claims no clean exit"
        );
        assert_eq!(
            bt_persist::read_session(&root.join("session.json"))
                .0
                .windows,
            last_save.windows,
            "the disk holds the last completed save"
        );

        drop(release);
        crate::session_end::forget();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (release review X-9) — **one thread takes this channel, and a second cannot.**
    ///
    /// The deadline's first shape grew a fallback writer per write for the store whose thread
    /// would not start, and dropped its handle when it timed out: two independent writers could
    /// then be inside `atomic_write` on one path with the older free to land last, and neither
    /// was in the `stalled` account the sentinel is kept by. There is one writer again. It is
    /// started lazily so a machine that could not spare a thread for a moment still gets one,
    /// and the ends it works from are taken under a lock *inside* it, so the attempt that
    /// arrives second finds nothing to start on — whatever became of the first one's handle.
    ///
    /// Red gate: hand the ends to the closure instead of letting the thread take them, and the
    /// last assertion raises a second writer over a channel the first is already reading.
    #[test]
    fn only_one_thread_ever_takes_the_session_writers_channel() {
        // `retire` closes the writer through an owner-thread door, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-one-writer");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let path = root.join("session.json");

        let mut writer = SessionWriter::open();
        // The thread names itself as it begins (`SessionWriter::start`'s body).
        crate::lane::wait_until("the writer thread", || writer.thread_id().is_some());
        let started = writer.thread_id().expect("a writer thread");

        // Two documents, in the order they were decided, down one channel.
        let first = writer.send(&path, b"first".to_vec()).expect("queued");
        let second = writer.send(&path, b"second".to_vec()).expect("queued");
        assert_eq!((first, second), (1, 2));
        // The receipts as the writer sends them, each awaited on the channel within the suite's
        // patience: how fast two atomic writes land on a loaded disk is not this test's subject,
        // and the session's own budget would make it one.
        let earlier = crate::lane::receive(&writer.receipts, "the older document's receipt");
        let landed = crate::lane::receive(&writer.receipts, "the newer document's receipt");
        assert_eq!(
            [earlier.generation, landed.generation],
            [first, second],
            "the older document answered first, which is what one writer buys"
        );
        assert_eq!((earlier.result, landed.result), (Ok(()), Ok(())));
        assert_eq!(
            std::fs::read(&path).expect("the file the writer left"),
            b"second".to_vec(),
            "and the document decided last is the one on the disk"
        );

        // Asking again is the same thread and never a second.
        assert!(
            SessionWriter::start(&writer.lane),
            "the writer is already running"
        );
        assert_eq!(writer.thread_id(), Some(started), "still the same thread");

        // And a writer whose handle has gone — which is what a timed-out fallback used to leave
        // behind — cannot raise another beside the one that is still reading the channel.
        writer.forget_thread();
        assert!(
            !SessionWriter::start(&writer.lane),
            "the ends are taken, so there is nothing to start a second writer on"
        );

        retire(&mut writer);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (release review X-8) — **a wait for one generation hands back the receipts of the older
    /// generations it passed on the way**, so the store books them as well; none is lost to the
    /// wait that read past it. The receipts are put on the channel by the test, so the wait has
    /// them before it starts and no disk or clock is in it.
    ///
    /// MUTATION (observed red): drop `earlier.push(receipt)` in `SessionWriter::wait_for` — the
    /// older receipt is not handed back.
    #[test]
    fn a_wait_for_one_generation_hands_back_the_older_receipts_it_passed() {
        // The wait is an owner-thread door, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let mut writer = SessionWriter::open();
        let (answer, receipts) = mpsc::channel();
        writer.receipts = receipts;
        for (generation, result) in [
            (1, Err("卷已离开 the volume went away".to_owned())),
            (2, Ok(())),
        ] {
            answer
                .send(SessionWriteReceipt { generation, result })
                .expect("the test holds the channel");
        }
        let (earlier, landed) = admitted::<doors::SessionWriteWait, _>(|token| {
            writer.wait_for(token, 2, Instant::now() + SESSION_SAVE_BUDGET)
        })
        .expect("admitted on the way out");
        assert_eq!(landed, Ok(()), "the awaited generation's own answer");
        assert_eq!(
            earlier
                .iter()
                .map(|receipt| (receipt.generation, receipt.result.clone()))
                .collect::<Vec<_>>(),
            vec![(1, Err("卷已离开 the volume went away".to_owned()))],
            "the older generation's receipt comes back beside it"
        );
        retire(&mut writer);
    }

    /// PIN (release review X-9) — **a store with nowhere to send reports; it does not write
    /// here.**
    ///
    /// `spawn_at_priority` is `Builder::spawn`, so "no writer thread" is the operating system
    /// refusing a thread rather than a verdict about the disk, and the one thread this document
    /// may not be written on is this one. The quit hears a refusal, the autosave puts the
    /// document back on its clock, and neither of them leaves an unbounded `fsync` on the window
    /// thread.
    ///
    /// Red gate: write the document here when `send` answers `None` and the last assertion
    /// finds a file the window thread wrote.
    #[test]
    fn a_store_with_no_writer_thread_reports_rather_than_writing() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-no-writer");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");

        let mut store = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        // The state a machine that would not start a thread leaves this store in.
        retire(&mut store.writer);
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        store.record(document, Instant::now());

        assert_eq!(
            store.flush_judged(),
            Err(SaveRefusal::Refused(NO_WRITER_THREAD.to_string())),
            "a quit hears a refusal, which is the answer that keeps its windows"
        );
        assert!(
            !root.join("session.json").exists(),
            "and nothing was written on the thread the window is drawn from"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A writer thread that takes documents, stays connected, and answers nothing — the shape of
    /// one inside an `fsync` that has not come back. It ends when the returned sender is
    /// dropped, so nothing of this test outlives the test.
    fn a_writer_that_never_answers() -> (SessionWriter, mpsc::Sender<()>) {
        let (requests, incoming) = mpsc::channel::<super::Queued>();
        let (outgoing, receipts) = mpsc::channel::<SessionWriteReceipt>();
        let (release, released) = mpsc::channel::<()>();
        // Its end is never told (`StorageEnds`): it stands until the returned sender goes.
        drop(std::thread::spawn(move || {
            // Both ends are held for as long as this thread stands, which is what makes the
            // wait on it a timeout rather than a disconnect.
            let _requests = incoming;
            let _receipts = outgoing;
            let _ = released.recv();
        }));
        let writer = SessionWriter {
            // Nothing left for `start` to hand a second thread, which is the state a writer
            // that is already running is in.
            lane: StorageLane {
                shared: Arc::new(LaneShared::of(super::LaneParts {
                    requests: Some(requests),
                    ends: None,
                    worker: LaneWorker::Running,
                    up: true,
                    wake: Arc::new(|| {}),
                    executor: crate::lane::Executor::thread_door(
                        bt_platform::ThreadPriority::BelowNormal,
                    ),
                    say: Box::new(crate::diagnostics::note),
                    sink: Arc::new(super::let_go),
                })),
            },
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            #[cfg(test)]
            _receipts_held_open: None,
            stalled: false,
            budget: SESSION_SAVE_BUDGET,
        };
        (writer, release)
    }

    /// PIN — **a receipt for a document that has already been replaced answers for nothing.**
    ///
    /// One writer thread means the writes land in the order they were decided, so the *last*
    /// request is the one that says what is on the disk. An older receipt arriving afterwards is
    /// news about a document that no longer exists: letting its success mark the store clean
    /// would be answering for bytes nobody wrote, and letting its failure leave the store owing
    /// would send again something already superseded.
    ///
    /// Red gate: drop the generation comparison in `apply_receipt` and the stale failure below
    /// leaves owed a document that a newer write has already carried.
    #[test]
    fn a_receipt_for_a_superseded_document_is_dropped() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-stale");
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let mut store = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        // Two documents have gone out; the second is the one that speaks for the store.
        store.writer.sent = 2;

        store.apply_receipt(SessionWriteReceipt {
            generation: 1,
            result: Err("the volume went away".to_string()),
        });
        assert!(
            !store.owes_a_write(),
            "an older document's failure is not this document's problem"
        );

        store.apply_receipt(SessionWriteReceipt {
            generation: 2,
            result: Err("the volume went away".to_string()),
        });
        assert!(
            store.owes_a_write(),
            "the newest one's failure is, and the document is owed to the next trigger"
        );
        assert!(
            !store.debouncer.is_dirty(),
            "and not to the clock: no quiet window was started by the refusal"
        );

        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// GUARD — **one writer, and it is not the window thread.** Its subject is how the session
    /// file is reached in the source: one raw atomic write in `persist`, inside the writer
    /// thread's spawn, and no other road (CONVENTIONS, "A test pins behaviour by running it").
    ///
    /// The behavioural half above is about when a verdict is known; this is about there being
    /// only one road to the file at all. Two roads — a thread and a window thread that sometimes
    /// writes for itself — is two writers over one path, and the older one can land last.
    ///
    /// **And now there is only the one** (release review X-9). The window-thread fallback for a
    /// store whose writer thread would not start is gone: `send` starts one when it can and says
    /// so when it cannot, so a hand-over that finds no thread reports rather than writing, and
    /// two writers can never be in flight over this path at all.
    ///
    /// Mutation: call the session's atomic write from `flush_if_due`, from `hand_over` or from
    /// a fallback of its own, and the counts below name it.
    #[test]
    fn the_only_thread_that_fsyncs_session_json_is_the_writer() {
        let source = Index::of_package("bt-app");
        let body = |owner: &str, name: &str| -> &'static str {
            source
                .body_of(&ItemQuery::method(owner, name))
                .unwrap_or_else(|failure| panic!("{failure}"))
        };
        assert!(
            !body("SessionStore", "flush_if_due").contains("atomic"),
            "the turn-by-turn autosave does not touch a disk"
        );
        let raw = "bt_persist::atomic_write(";
        let persist = Scope::Module("crate::persist".to_owned());
        let raw_hits = source
            .search(
                &Search::new(
                    Needle::new(Pattern::path("bt_persist::atomic_write")),
                    View::Identifiers,
                )
                .in_scope(persist.clone()),
            )
            .unwrap_or_else(|failure| panic!("{failure}"))
            .in_the_product(source);
        assert_eq!(
            raw_hits.len(),
            1,
            "the raw atomic write appears once, inside the writer thread's loop"
        );
        // Since T-STORAGE-WORKER-ON-THE-SEAM the worker's loop serves each job through
        // `serve_storage_job`, inside the request's task, and the lane's executor — the thread
        // door below normal, built with the lane — starts the thread.
        let serve = source
            .body_of(&ItemQuery::function("serve_storage_job").in_module("crate::persist"))
            .unwrap_or_else(|failure| panic!("{failure}"));
        assert!(
            serve.contains(raw)
                && body("SessionWriter", "start").contains("serve_storage_job(")
                && body("SessionWriter", "start").contains("start_worker(\"session-writer\"")
                && body("StorageLane", "fresh").contains("ThreadPriority::BelowNormal"),
            "and that one is downstream of the spawn that puts it on its own thread"
        );
        // And the session's own named write is not called anywhere in this module
        // (release review X-9): the store reaches the file through the writer thread's loop or
        // it does not reach it at all. `SettingsStore` and friends still write where they are
        // called, and that is a human's click rather than a per-turn autosave.
        assert_eq!(
            source
                .search(
                    &Search::new(
                        Needle::new(Pattern::call("write_session_atomic")),
                        View::Identifiers,
                    )
                    .in_scope(persist),
                )
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(source)
                .len(),
            0,
            "no road to `session.json` outside the one thread that owns it"
        );
        assert!(
            !body("SessionStore", "hand_over").contains("atomic"),
            "and the hand-over that finds no thread reports instead of writing"
        );
    }

    /// A private `%APPDATA%` for one test, with nothing of the real one in it.
    ///
    /// Named from the test rather than from a counter so a failure leaves a
    /// directory whose name says which case left it, and cleaned on the way *in*
    /// as well as out: a run that panicked half way through must not hand the
    /// next run a directory that already has both names in it.
    fn appdata(case: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-relocate-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private APPDATA for this test");
        root
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    /// RED (review row R4-10) — **a value that did not reach the disk is written
    /// again the next time it is chosen.**
    ///
    /// Every store here assigned the new value into itself, wrote, and answered
    /// `true` whatever the write did — so a failed save was *remembered as done*.
    /// The value the store holds is what the next call compares against, so
    /// picking the same row again matched, returned early and attempted nothing:
    /// the one gesture a person makes when a setting did not take was the one
    /// gesture guaranteed to do nothing.
    ///
    /// The failure is injected the way this file's other write tests inject one
    /// — a path whose parent directory does not exist, which is what a store on a
    /// volume that went away looks like from here. No mock, no flag: the real
    /// `atomic_write` refusing for a real reason.
    ///
    /// Red gate: compare on equality alone (`if self.settings == settings { return
    /// false }`) and the second write never happens, so the file below is still
    /// missing after the directory comes back.
    #[test]
    fn a_setting_that_could_not_be_saved_is_saved_when_it_is_chosen_again() {
        let root = appdata("retry");
        let gone = root.join("not-yet").join("settings.json");
        let mut store = SettingsStore::at(gone.clone());

        let chosen = SettingsV1 {
            terminal_font_size: 22,
            ..SettingsV1::default()
        };
        assert!(store.store(chosen.clone()), "the value changed");
        store.settle();
        assert!(!gone.exists(), "and the write could not happen");
        assert_eq!(
            store.loaded().terminal_font_size,
            22,
            "the window still shows what the reader chose"
        );

        // The volume comes back — or the reader, seeing the size did not change,
        // picks the same row again.
        std::fs::create_dir_all(gone.parent().unwrap()).unwrap();
        assert!(
            !store.store(chosen),
            "nothing changed, so no repaint is owed"
        );
        store.settle();
        assert!(
            gone.is_file(),
            "but the write was attempted again, and this time it landed"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (0.4.4 ticket 05) — **a batch of settings held for one gesture is in
    /// force the moment each is stored, and reaches the file once, when the
    /// batch is let go.**
    ///
    /// An import puts every changed row through the door a press on it goes
    /// through, and each of those ends in `store`. Held, the in-process document
    /// is what every reader sees at once — nothing waits on the disk, and nothing
    /// re-reads it — while the file is written a single time, as one press
    /// writes it.
    ///
    /// MUTATION: let `store` write while held and the file exists before the
    /// release.
    #[test]
    fn a_held_batch_of_settings_is_in_force_at_once_and_on_disk_once() {
        let root = appdata("held");
        let path = root.join("settings.json");
        let mut store = SettingsStore::at(path.clone());
        store.hold_writes();
        let first = SettingsV1 {
            terminal_font_size: 19,
            ..SettingsV1::default()
        };
        assert!(store.store(first.clone()));
        let second = SettingsV1 {
            git_panel: !first.git_panel,
            ..first
        };
        assert!(store.store(second.clone()));
        assert_eq!(store.loaded(), &second, "both are in force at once");
        assert!(!path.exists(), "and neither has been written yet");
        assert_eq!(store.document().sent(), 0, "nothing was handed over");
        store.release_writes();
        assert_eq!(store.document().sent(), 1, "one document, handed over once");
        store.settle();
        let written = read(&path).expect("the batch is written when it is let go");
        let on_disk: SettingsV1 = serde_json::from_str(&written).unwrap();
        assert_eq!(on_disk, second, "as the document in force");
        store.release_writes();
        assert_eq!(
            store.document().sent(),
            1,
            "and a second release hands nothing more over"
        );
        assert!(store.store(SettingsV1::default()));
        assert_eq!(
            store.document().sent(),
            2,
            "once let go, a store writes at once again"
        );
        store.settle();
        let on_disk: SettingsV1 =
            serde_json::from_str(&read(&path).expect("the file stands")).unwrap();
        assert_eq!(on_disk, SettingsV1::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED — **a store asks whether it may write the folder it writes to.**
    ///
    /// The same rule `pins.rs` pins one file over, and the reason it is pinned
    /// here as well: the writer-of-record question belongs to a *directory*, and
    /// every store that answered it with `is_storage_writer()` — or, in the
    /// test-only doors, asserted `true` without asking — was answering about
    /// `%APPDATA%\Folio` while writing somewhere else entirely.
    ///
    /// Red gate: hard-code `writer_of_record: true` in `SettingsStore::at` and
    /// the second half fails; ask `is_storage_writer()` there and the first half
    /// fails whenever a Folio is running on this machine.
    #[test]
    fn a_settings_store_asks_the_folder_it_writes_to_and_not_the_process_one() {
        let root = appdata("claims");
        let held = root.join("held");
        let free = root.join("free");
        std::fs::create_dir_all(&held).expect("a scratch folder");
        std::fs::create_dir_all(&free).expect("a scratch folder");

        let claim = bt_platform::instance::claim_data_directory(&held)
            .expect("nothing else on this machine has this folder");

        let chosen = SettingsV1 {
            terminal_font_size: 22,
            ..SettingsV1::default()
        };

        let mut free_store = SettingsStore::at(free.join(SETTINGS_FILE_NAME));
        assert!(free_store.store(chosen.clone()), "the value changed");
        free_store.settle();
        assert!(
            free.join(SETTINGS_FILE_NAME).is_file(),
            "a store over a folder nobody holds writes it"
        );

        let mut held_store = SettingsStore::at(held.join(SETTINGS_FILE_NAME));
        assert!(
            held_store.store(chosen),
            "the choice is live in this window"
        );
        held_store.settle();
        assert!(
            !held.join(SETTINGS_FILE_NAME).exists(),
            "and a store over a folder somebody else holds writes nothing into it"
        );

        drop(claim);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED — **a second claimant is not the writer of that directory, and goes
    /// on not being it** (audit 3 A-3).
    ///
    /// The value half of the endpoint rule. Both endpoints of a data directory —
    /// the launch wire and, on Unix, the attention doorbell — are that directory's
    /// writer saying so to other processes, and both names are first-come. The
    /// race that mattered was two ordinary launches a few hundred milliseconds
    /// apart: the one that lost the claim went on to bind the names anyway,
    /// because the binding asked nothing, and every later launch was then
    /// answered by the window whose session writes this file discards.
    /// `open_the_data_directorys_endpoints` asks this question instead, and this
    /// is the answer it gets.
    ///
    /// The second half is the memoisation, and it matters as much as the first:
    /// a claim is taken once and held for the life of the process, so a process
    /// refused it stays refused even after the holder has gone. A door that
    /// re-asked would bind a name on the strength of a claim it does not hold.
    ///
    /// Red gate: take the claim below out and both assertions flip — this test
    /// process *is* the only claimant of a folder nothing else knows about.
    #[test]
    fn a_second_claimant_is_not_the_writer_of_that_directory() {
        let root = appdata("second-claimant");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        // Somebody else got here first. On Windows this is a named kernel
        // object and on Unix an `flock` on a descriptor; in both, a second
        // attempt from anywhere — including this process — is refused.
        let first = bt_platform::instance::claim_data_directory(&directory)
            .expect("nothing else on this machine has this folder");

        assert!(
            !is_writer_of(&directory),
            "a process that was refused the claim must not read itself as the writer, \
             which is the fact the endpoints are opened off"
        );

        drop(first);
        assert!(
            !is_writer_of(&directory),
            "and the answer is the one this process took, not the one the folder \
             would give it now"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same directory as `directory`, spelled with a trailing separator —
    /// a second spelling the claim name folds to the same row.
    fn spelled_with_a_trailing_separator(directory: &Path) -> PathBuf {
        PathBuf::from(format!(
            "{}{}",
            directory.display(),
            std::path::MAIN_SEPARATOR
        ))
    }

    /// RED (U-5, self-update R-3) — **a claim this process took is the writer's
    /// claim from the moment it is adopted, with no instant in between at which
    /// anybody else could take it or anybody here be told otherwise.**
    ///
    /// The updated build takes the claim with `try_claim` while the old build is
    /// letting go (`docs/plans/design/self-update-2026-09-16.md` §C.7), and then
    /// every store it opens asks `is_writer_of`. Rev 1's `--await-exit` took the
    /// claim outside the table, so the table's first ask was refused by the
    /// process's own guard and it handed itself off to nobody. Here a second
    /// claimant keeps asking for the directory from before the adoption to after
    /// it, and never gets it; and the first `is_writer_of` after the adoption —
    /// under either spelling — answers that this process writes it.
    ///
    /// MUTATION: have `adopt_claim` insert under a key other than
    /// `claim_name(directory)`, or ask `is_writer_of` before it inserts, and
    /// this goes red.
    #[test]
    fn acquired_claim_is_adopted_without_a_gap() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let root = appdata("adopt-without-a-gap");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        let claim = try_claim(&directory).expect("nothing else on this machine has this folder");

        let stop = Arc::new(AtomicBool::new(false));
        let contender = {
            let stop = Arc::clone(&stop);
            let directory = directory.clone();
            std::thread::spawn(move || {
                let mut asked = 0_u32;
                loop {
                    asked += 1;
                    if bt_platform::instance::claim_data_directory(&directory).is_some() {
                        return (asked, true);
                    }
                    if stop.load(Ordering::Acquire) && asked > 1 {
                        return (asked, false);
                    }
                    std::thread::yield_now();
                }
            })
        };

        adopt_claim(&directory, claim);
        let writer = is_writer_of(&directory);
        let writer_by_another_spelling =
            is_writer_of(&spelled_with_a_trailing_separator(&directory));
        stop.store(true, Ordering::Release);
        let (asked, won) = contender.join().expect("the contender thread");

        assert!(
            !won,
            "the claim was free for another claimant somewhere between being taken and \
             being adopted (after {asked} asks)"
        );
        assert!(
            writer,
            "the first question after the adoption must answer that this process writes \
             the directory"
        );
        assert!(
            writer_by_another_spelling,
            "and so must the same question asked of another spelling of it"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A data directory of a test's own with a `settings.json` that names
    /// `language`, written the way the product writes it.
    fn a_home_that_speaks(case: &str, language: bt_persist::LanguageV1) -> (PathBuf, PathBuf) {
        let root = appdata(case);
        let home = root.join(STORAGE_NAME);
        std::fs::create_dir_all(&home).expect("a scratch data directory");
        let settings = SettingsV1 {
            language,
            ..SettingsV1::default()
        };
        bt_persist::write_settings_atomic(&home.join(SETTINGS_FILE_NAME), &settings)
            .expect("a settings file to read the language from");
        (root, home)
    }

    /// Whether this process's claim table has an answer — a claim held or a
    /// refusal remembered — for `directory`.
    fn the_claim_table_has_answered_for(directory: &Path) -> bool {
        claim_table().contains_key(&bt_platform::instance::claim_name(directory))
    }

    /// RED (B-EXPLORER-CLAIM) — **`--explorer-command`'s language lookup reads
    /// the language and leaves the data directory's claim free, with no answer
    /// remembered in this process's claim table and nothing written in the
    /// directory.**
    ///
    /// The COM server opened a `SettingsStore` to learn the language, and
    /// opening one asks `is_writer_of`, which takes the claim and keeps it — or
    /// keeps the refusal — for the life of the process. Explorer keeps that
    /// process alive while the menu it drew starts Folio and ten seconds after,
    /// so the Folio it started found the claim held by a process with no launch
    /// endpoint and ran as a window that saves nothing. The lookup is the one
    /// `explorer_menu::serve` makes (`crate::door_language`), over a data
    /// directory of the test's own; a damaged file beside it is answered with
    /// the default and left exactly where it was, not kept aside.
    ///
    /// MUTATION: have `SettingsStore::peek_language` ask `is_writer_of(directory)`
    /// first, which is what `SettingsStore::open` does, and this goes red.
    #[test]
    fn an_explorer_command_server_never_holds_the_claim() {
        let (root, home) =
            a_home_that_speaks("explorer-server-claim", bt_persist::LanguageV1::Chinese);

        let language = crate::door_language(&home);

        assert_eq!(
            language,
            crate::i18n::Lang::Chinese,
            "the lookup reads the language the settings file names"
        );
        assert!(
            !the_claim_table_has_answered_for(&home),
            "the server's lookup must leave no answer about the data directory in the \
             process's claim table — neither a claim nor a refusal"
        );
        assert!(
            bt_platform::instance::claim_data_directory(&home).is_some(),
            "and the claim itself must be free for the Folio the menu starts"
        );

        let damaged_home = root.join("damaged");
        write(&damaged_home.join(SETTINGS_FILE_NAME), "{ not json");
        assert_eq!(
            SettingsStore::peek_language(&damaged_home),
            bt_persist::LanguageV1::default(),
            "a file that cannot be read answers the default"
        );
        let left: Vec<_> = std::fs::read_dir(&damaged_home)
            .expect("the damaged directory")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(
            left,
            vec![OsString::from(SETTINGS_FILE_NAME)],
            "and the lookup keeps nothing aside: the directory is as it was found"
        );
        assert!(!the_claim_table_has_answered_for(&damaged_home));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (B-EXPLORER-CLAIM) — **after the Explorer server has looked up its
    /// language, the resident Folio its menu launches is the writer of the data
    /// directory and opens its launch endpoint.**
    ///
    /// The sequence of a right-click with no Folio running: the server's lookup
    /// first, then the launch. The launch is another process with a claim table
    /// of its own, so what it asks is the kernel — `is_writer_of`'s first ask,
    /// `claim_data_directory` — and, as the writer, it opens the launch
    /// endpoint (`open_the_data_directorys_endpoints`'s gate). With the lookup
    /// going through a store, the server still held the claim here, the launch
    /// was refused, and no endpoint was ever opened for the next launch to find.
    ///
    /// MUTATION: have `SettingsStore::peek_language` ask `is_writer_of(directory)`
    /// first, and this goes red.
    #[test]
    fn a_resident_launch_after_the_server_is_the_writer() {
        let (root, home) =
            a_home_that_speaks("server-then-resident", bt_persist::LanguageV1::English);

        let _ = crate::door_language(&home);

        let resident = bt_platform::instance::claim_data_directory(&home);
        assert!(
            resident.is_some(),
            "the Folio the menu launched must be the writer of its data directory"
        );
        let endpoint = resident.as_ref().map(|_| {
            bt_platform::launch_pipe::LaunchPipe::start(
                &home,
                |_| None::<bt_platform::launch_pipe::Decision<()>>,
                |()| {},
            )
        });
        assert!(
            matches!(endpoint, Some(Ok(_))),
            "and, the writer, it opens the launch endpoint the next launch hands over to"
        );

        if let Some(Ok(endpoint)) = endpoint {
            let _ = endpoint.retire_within(crate::lane::PATIENCE);
        }
        drop(resident);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (B-EXPLORER-CLAIM) — **a front-door refusal that looks up its
    /// language at the same moment as an ordinary launch leaves that launch the
    /// writer, and remembers no answer of its own.**
    ///
    /// `say_at_the_front_door` (a mistyped argument, `--version`, `--help`)
    /// used to open a `SettingsStore` for its language, and so raced every
    /// launch started beside it for the claim: whichever lost remembered the
    /// loss for its life. The two run on two threads released together; the
    /// launch is another process, so it asks the kernel (`claim_data_directory`,
    /// its own `is_writer_of`'s first ask). Whichever order they run in, the
    /// launch must get the claim and the refusal path must leave no row — a
    /// row would be the claim held against the launch, or a refusal kept.
    ///
    /// MUTATION: have `SettingsStore::peek_language` ask `is_writer_of(directory)`
    /// first, and this goes red in either order.
    #[test]
    fn a_front_door_refusal_does_not_cache_a_refusal_for_the_launch() {
        let (root, home) =
            a_home_that_speaks("front-door-beside-launch", bt_persist::LanguageV1::English);
        let start = Arc::new(std::sync::Barrier::new(2));

        let refusal = {
            let start = Arc::clone(&start);
            let home = home.clone();
            std::thread::spawn(move || {
                start.wait();
                crate::door_language(&home)
            })
        };
        let launch = {
            let start = Arc::clone(&start);
            let home = home.clone();
            std::thread::spawn(move || {
                start.wait();
                // Returned, so it is held until the refusal path is done, as a
                // resident holds it.
                bt_platform::instance::claim_data_directory(&home)
            })
        };
        let _ = refusal.join().expect("the refusal thread");
        let claim = launch.join().expect("the launch thread");

        assert!(
            claim.is_some(),
            "the ordinary launch must be the writer of the data directory"
        );
        assert!(
            !the_claim_table_has_answered_for(&home),
            "and the front door must have remembered nothing about it"
        );

        drop(claim);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-5, self-update R-3 as refined by revision (b)) — **`is_writer_of`
    /// keeps its first answer, a refusal included, whatever `try_claim` does
    /// afterwards.**
    ///
    /// A process that was told it is not the writer opened everything since as a
    /// non-writer; making it the writer mid-run would leave windows that decided
    /// "do not write" beside ones that write. `try_claim` is the question that
    /// does not remember, and it must not become a way to rewrite the table's
    /// memory: the holder going away does not change the answer, and neither does
    /// this process then taking the claim itself.
    ///
    /// MUTATION: let `try_claim`'s success write into the table, or let
    /// `is_writer_of` ask the platform again, and this goes red.
    #[test]
    fn is_writer_of_still_remembers_a_refusal() {
        let root = appdata("still-remembers-a-refusal");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        let other = bt_platform::instance::claim_data_directory(&directory)
            .expect("nothing else on this machine has this folder");
        assert!(
            !is_writer_of(&directory),
            "a process refused the claim is not the writer"
        );

        drop(other);
        assert!(
            !is_writer_of(&directory),
            "the holder leaving does not make this process the writer mid-run"
        );

        let taken = try_claim(&directory)
            .expect("try_claim asks the platform now, and now the folder is free");
        assert!(
            !is_writer_of(&directory),
            "and taking the claim through try_claim does not rewrite the answer either"
        );

        drop(taken);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-43, round 3) — **a sweep is not a refusal of this process's
    /// claim and is therefore never memoised in the claim table.** The launch
    /// may take the existing hand-over road, but the next writer question asks
    /// the platform again.
    ///
    /// MUTATION: insert `None` for `ClaimRefusal::Sweeping`; the row remains
    /// after the transient answer and this test is red.
    #[test]
    fn a_sweep_is_never_remembered_as_a_claim_refusal() {
        let mut table = HashMap::new();
        let name = "swept-directory".to_owned();
        assert!(!remember_claim_answer(
            &mut table,
            name.clone(),
            Err(bt_platform::instance::ClaimRefusal::Sweeping),
        ));
        assert!(
            !table.contains_key(&name),
            "the next writer question must ask the platform again"
        );
    }

    /// **The claim on `directory`, asked for until it is let go** — within 5 s,
    /// and only a live holder is asked about again.
    ///
    /// A claim this process dropped is not always free at once off Windows: a
    /// `flock` belongs to the open file description, and a child that another
    /// test thread is starting at that moment holds a copy of every descriptor
    /// from its fork until its exec closes them (on macOS that exec validates
    /// the signature of a freshly copied test binary, which takes a while). The
    /// lock is let go when that exec is done; asking once would read the
    /// child's copy as a holder.
    pub(crate) fn claim_once_let_go(directory: &Path) -> bt_platform::instance::DataDirectoryClaim {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match try_claim(directory) {
                Ok(claim) => return claim,
                Err(
                    bt_platform::instance::ClaimRefusal::Held
                    | bt_platform::instance::ClaimRefusal::Sweeping,
                ) if std::time::Instant::now() < until => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(refusal) => panic!("asked again, the platform answers again: {refusal:?}"),
            }
        }
    }

    /// RED (U-5, self-update R-3) — **`try_claim` asks the platform every time
    /// and remembers nothing.**
    ///
    /// The waiter's loop is this function called again and again; an answer it
    /// remembered would be the first refusal for the whole of the wait. And it
    /// leaves the claim table alone: a refusal it was given is not a refusal
    /// `is_writer_of` later reads.
    ///
    /// MUTATION: cache a refusal in `try_claim` (in the claim table or its own),
    /// and the second or the last half goes red.
    #[test]
    fn try_claim_caches_nothing() {
        let root = appdata("try-claim-caches-nothing");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        let other = bt_platform::instance::claim_data_directory(&directory)
            .expect("nothing else on this machine has this folder");
        assert!(
            matches!(
                try_claim(&directory),
                Err(bt_platform::instance::ClaimRefusal::Held)
            ),
            "a live holder is a held claim"
        );

        drop(other);
        let taken = claim_once_let_go(&directory);
        assert!(
            matches!(
                try_claim(&directory),
                Err(bt_platform::instance::ClaimRefusal::Held)
            ),
            "and a third ask is a third answer: this call's own claim now holds it"
        );

        drop(taken);
        drop(claim_once_let_go(&directory));
        assert!(
            is_writer_of(&directory),
            "no refusal try_claim was given was left in the table for is_writer_of to read"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-5, self-update R-3) — **adopting a claim for a directory this
    /// process has already answered for is refused, loudly in a debug build, and
    /// the earlier answer stands.**
    ///
    /// The one way a row can already be there is that something asked
    /// `is_writer_of` first, which is the ordering `adopt_claim` exists to rule
    /// out. (Two real claims on one directory cannot coexist, so the first
    /// answer here is the table's own, a remembered refusal.) The claim being
    /// adopted must not replace the row — a non-writer does not become the
    /// writer mid-run — so it is dropped and the directory is free again.
    ///
    /// MUTATION: let `adopt_claim` overwrite an occupied row, or take its
    /// `debug_assert!` out, and this goes red.
    #[test]
    fn adopt_claim_twice_is_refused() {
        let root = appdata("adopt-twice");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        let other = bt_platform::instance::claim_data_directory(&directory)
            .expect("nothing else on this machine has this folder");
        assert!(
            !is_writer_of(&directory),
            "the first answer for this directory"
        );
        drop(other);

        let late = try_claim(&directory).expect("the folder is free again");
        let adoption = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            adopt_claim(&directory, late)
        }));
        assert_eq!(
            adoption.is_err(),
            cfg!(debug_assertions),
            "a second answer for one directory stops a debug build, and only a debug build"
        );
        assert!(
            !is_writer_of(&directory),
            "the earlier answer stands, and the table is still readable"
        );
        assert!(
            bt_platform::instance::claim_data_directory(&directory).is_some(),
            "and the claim that was refused a row was let go rather than kept anywhere"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (U-5, self-update §C.7) — **a launch by hand and the updated build's
    /// relaunch are never both the writer.**
    ///
    /// Two claimants stand in for the two processes: the launch by hand takes the
    /// directory first, so the relaunch's `try_claim` is told it is held; when
    /// the launch by hand goes, the relaunch takes the claim and adopts it; and a
    /// launch by hand arriving after that is refused.
    ///
    /// MUTATION: have `try_claim` answer from anything but the platform, or
    /// `adopt_claim` drop the guard it is given, and one of the three halves
    /// goes red.
    #[test]
    fn manual_launch_and_relaunch_have_one_writer() {
        let root = appdata("one-writer");
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).expect("a scratch folder");

        let by_hand = bt_platform::instance::claim_data_directory(&directory)
            .expect("nothing else on this machine has this folder");
        assert!(
            matches!(
                try_claim(&directory),
                Err(bt_platform::instance::ClaimRefusal::Held)
            ),
            "while the launch by hand holds the directory the relaunch is not handed it"
        );

        drop(by_hand);
        let relaunch = try_claim(&directory).expect("the launch by hand has gone");
        adopt_claim(&directory, relaunch);
        assert!(is_writer_of(&directory), "the relaunch is the writer");
        assert!(
            bt_platform::instance::claim_data_directory(&directory).is_none(),
            "and a launch by hand after it is refused"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A session store whose writer is the test: every document it hands over arrives on the
    /// queue this returns, in order, and the test answers each on the receipts sender — the
    /// answer arrives when the test sends it, and no thread decides when.
    fn a_store_the_test_answers(
        case: &str,
    ) -> (
        SessionStore,
        super::HeldJobs,
        mpsc::Sender<SessionWriteReceipt>,
        PathBuf,
    ) {
        let root = appdata(case);
        let mut store = SessionStore::at(root.join(SESSION_FILE_NAME), root.join("session.lock"));
        let (lane, jobs) = StorageLane::held();
        let (answer, receipts) = mpsc::channel();
        store.writer = SessionWriter {
            lane,
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            // The test holds the sending end itself, and drops it for a worker's end.
            _receipts_held_open: None,
            stalled: false,
            budget: SESSION_SAVE_BUDGET,
        };
        (store, jobs, answer, root)
    }

    /// The generations of the session documents handed over since the last look.
    fn documents_handed_over(jobs: &super::HeldJobs) -> Vec<u64> {
        jobs.try_iter()
            .filter_map(|job| match job {
                StorageJob::Session(request) => Some(request.generation),
                _ => None,
            })
            .collect()
    }

    /// A layout that differs from every other `step`: one recent folder, its name mixed-script.
    fn a_layout(step: u32) -> SessionV1 {
        let mut layout = SessionV1::default();
        layout.recent_folders.push(bt_persist::RecentFolderV1 {
            path: format!(r"D:\项目 notes\第{step}章 chapter"),
            opened_at: "2026-10-13T00:00:00Z".to_owned(),
        });
        layout
    }

    /// The disk's refusal of generation `generation`.
    fn refused(generation: u64) -> SessionWriteReceipt {
        SessionWriteReceipt {
            generation,
            result: Err("磁盘已满 the disk is full".to_owned()),
        }
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY; the side-effects contract's C5, owner ruling Q3
    /// 2026-10-11) — **a refused session write is not sent again because time passed.**
    ///
    /// The store used to put a refused document back on the debounce, so a disk that stayed full
    /// was handed the same document every 1.5 seconds. Here the writer refuses the one document
    /// handed over, and eight quiet windows go by with nothing changing: nothing else is handed
    /// over, and the store asks the loop for no wake.
    ///
    /// MUTATION: in `SessionStore::report_write`, put a refused document back on the debounce
    /// (`self.debouncer.mark_dirty(Instant::now())`) — the later turns hand it over again.
    #[test]
    fn a_refused_session_write_is_not_retried_by_the_clock() {
        let (mut store, jobs, answer, root) = a_store_the_test_answers("no-clock-retry");
        let changed = Instant::now();
        store.record(a_layout(1), changed);
        store.flush_if_due(changed + SESSION_DEBOUNCE);
        assert_eq!(
            documents_handed_over(&jobs),
            vec![1],
            "the change is handed over"
        );

        answer
            .send(refused(1))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        for period in 2..10 {
            store.flush_if_due(changed + SESSION_DEBOUNCE * period);
        }
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "eight quiet windows later, nothing more was handed over"
        );
        assert_eq!(store.deadline(), None, "and no wake is asked for");
        assert!(store.owes_a_write(), "the document is still owed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY) — **a refused session write is sent again by the
    /// next change**, as the document then stands; and a change made while the refused write was
    /// still on its way waits in the pending slot and goes once the refusal is heard.
    ///
    /// MUTATION: drop the pending slot's send (`|| (self.send_again && !self.writer.busy())` in
    /// `SessionStore::flush_if_due`) — the change made while the write was out is never handed
    /// over.
    /// MUTATION: a refusal stops the autosave until a success (`if self.retry_on_trigger
    /// { self.debouncer.mark_flushed(); return; }` at the head of `SessionStore::hand_over`) —
    /// the next change is not handed over.
    #[test]
    fn a_refused_session_write_is_resent_on_the_next_change() {
        let (mut store, jobs, answer, root) = a_store_the_test_answers("resent-on-change");
        let start = Instant::now();
        store.record(a_layout(1), start);
        store.flush_if_due(start + SESSION_DEBOUNCE);
        assert_eq!(documents_handed_over(&jobs), vec![1]);
        answer
            .send(refused(1))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        store.flush_if_due(start + SESSION_DEBOUNCE * 2);
        assert_eq!(documents_handed_over(&jobs), Vec::<u64>::new());

        // The next change, after the refusal was heard.
        let changed = start + SESSION_DEBOUNCE * 3;
        store.record(a_layout(2), changed);
        store.flush_if_due(changed + SESSION_DEBOUNCE);
        assert_eq!(
            documents_handed_over(&jobs),
            vec![2],
            "the change hands the document over again"
        );

        // A change while that write is still on its way: one autosave on the writer at a time.
        let again = changed + SESSION_DEBOUNCE * 2;
        store.record(a_layout(3), again);
        store.flush_if_due(again + SESSION_DEBOUNCE);
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "it waits in the pending slot"
        );
        assert_eq!(store.deadline(), None, "and asks for no wake of its own");
        answer
            .send(refused(2))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        store.flush_if_due(again + SESSION_DEBOUNCE * 2);
        assert_eq!(
            documents_handed_over(&jobs),
            vec![3],
            "the refusal is heard, and the change made meanwhile goes"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY) — **a refused session write is sent again by the
    /// next storage pass**, once: the pass is the trigger, not a clock, and a pass while that
    /// write is out, or after it landed, sends nothing.
    ///
    /// MUTATION: the owed flag dropped (`SessionStore::report_write` never sets
    /// `retry_on_trigger`) — the pass hands nothing over.
    #[test]
    fn a_refused_session_write_is_resent_on_the_next_ear_pass() {
        let (mut store, jobs, answer, root) = a_store_the_test_answers("resent-on-the-ear");
        let start = Instant::now();
        store.record(a_layout(1), start);
        store.flush_if_due(start + SESSION_DEBOUNCE);
        answer
            .send(refused(1))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        store.flush_if_due(start + SESSION_DEBOUNCE * 4);
        assert_eq!(documents_handed_over(&jobs), vec![1]);

        store.hear_the_storage_pass();
        assert_eq!(
            documents_handed_over(&jobs),
            vec![2],
            "the pass hands the owed document over"
        );
        store.hear_the_storage_pass();
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "once: a second pass while it is out sends nothing"
        );
        answer
            .send(SessionWriteReceipt {
                generation: 2,
                result: Ok(()),
            })
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        store.flush_if_due(start + SESSION_DEBOUNCE * 5);
        store.hear_the_storage_pass();
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "and nothing once it landed"
        );
        assert!(!store.owes_a_write());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY) — **a refused session write is sent again by the
    /// quit's write**, which waits for it and finds it on the disk.
    ///
    /// The real writer and the real `atomic_write`: the folder is not there when the autosave
    /// hands the document over, and is there by the time the reader quits. Nothing changed in
    /// between, so only the owed document can make the quit write.
    ///
    /// MUTATION: the owed flag dropped (`SessionStore::report_write` never sets
    /// `retry_on_trigger`) — the quit owes nothing, writes nothing and answers `Ok(())` over a
    /// file that is not there.
    #[test]
    fn a_refused_session_write_is_resent_at_quit() {
        crate::test_support::on_the_window_thread_exiting();
        let root = appdata("resent-at-quit");
        let folder = root.join("会话 sessions");
        let path = folder.join(SESSION_FILE_NAME);
        let mut store = SessionStore::at(path.clone(), root.join("session.lock"));
        store.set_save_budget(crate::lane::PATIENCE);
        let changed = Instant::now();
        store.record(a_layout(1), changed);
        store.flush_if_due(changed + SESSION_DEBOUNCE);
        let receipt = crate::lane::receive(&store.writer.receipts, "the autosave's receipt");
        assert!(
            receipt.result.is_err(),
            "no folder, no file: {:?}",
            receipt.result
        );
        store.apply_receipt(receipt);
        assert!(
            !store.debouncer.is_dirty(),
            "owed to a trigger, not to the clock"
        );

        std::fs::create_dir_all(&folder).expect("the folder comes back");
        assert_eq!(store.flush_judged(), Ok(()), "the quit's write lands");
        assert_eq!(
            read_session(&path).0,
            a_layout(1),
            "and what it wrote is the owed document"
        );
        retire(&mut store.writer);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY; `docs/M2-persistence-schema-v1.md` §5.3) — **a
    /// failure streak is said once, a change does not start it again, and a success ends it.**
    ///
    /// Each refusal here answers a change the reader made. The fifth in a row owes the card's
    /// sentence, naming the file; the refusals after it, each for a new change, owe nothing more.
    /// A landing ends the streak, and the fifth refusal of the next one is said again.
    ///
    /// What the turn owes is read where the window reads it, from `SessionStore::autosave`
    /// (the frame's turn raises it; `#[must_use]` keeps that call from dropping it).
    ///
    /// MUTATION: say every refusal past the bound (drop the `said` guard in
    /// `DocumentWrites::say`) — the sixth refusal owes a second card.
    /// MUTATION: `SessionStore::autosave` answers `None` instead of `self.take_fault()` — the
    /// fifth refusal reaches no card.
    /// MUTATION: a change starts the count again (the old re-arm: `self.writes.streak = 0` in
    /// `SessionStore::record`) — the fifth refusal is never reached and nothing is said.
    #[test]
    fn a_failure_streak_is_said_once_and_a_success_resets_it() {
        let (mut store, jobs, answer, root) = a_store_the_test_answers("said-once");
        let mut now = Instant::now();
        let mut step = 0;
        // One change, its write, and the disk's answer to it; what the turn owes the reader.
        let mut change_answered = |store: &mut SessionStore, result: Result<(), String>| {
            step += 1;
            store.record(a_layout(step), now);
            now += SESSION_DEBOUNCE;
            store.flush_if_due(now);
            let generation = *documents_handed_over(&jobs)
                .last()
                .expect("each change is handed over");
            answer
                .send(SessionWriteReceipt { generation, result })
                .expect("the store holds its receipts");
            store.take_receipts(); // the turn the receipt's wake brings
            now += SESSION_DEBOUNCE;
            store.autosave(now)
        };
        let refusal = || Err("拒绝访问 access is denied".to_owned());
        let sentence = crate::i18n::persisted_file_unwritable(SESSION_FILE_NAME);

        for _ in 1..REFUSALS_BEFORE_TOLD {
            assert_eq!(change_answered(&mut store, refusal()), None, "not yet");
        }
        assert_eq!(
            change_answered(&mut store, refusal()),
            Some(sentence.clone()),
            "the fifth refusal in a row is said, naming the file"
        );
        for _ in 0..3 {
            assert_eq!(
                change_answered(&mut store, refusal()),
                None,
                "once per streak, whatever changed since"
            );
        }

        assert_eq!(change_answered(&mut store, Ok(())), None, "a landing");
        for _ in 1..REFUSALS_BEFORE_TOLD {
            assert_eq!(change_answered(&mut store, refusal()), None);
        }
        assert_eq!(
            change_answered(&mut store, refusal()),
            Some(sentence),
            "a new streak is said again"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 2, review blocker; T-QUIT-DOCUMENTS round 2,
    /// blocker 1) — **a quit with no budget left gives up a document a refusal left owed**: the
    /// way out's close hands nothing over.
    ///
    /// The quit tests' own pin of this records a fresh change; here the document is owed because
    /// the autosave's write was refused and nothing changed since, so only the owed flag and the
    /// pending slot can make the close write.
    ///
    /// MUTATION: leave `retry_on_trigger` set on `SessionStore::flush_for_quit`'s past-budget
    /// road — the close's flush hands a second document over.
    #[test]
    fn a_quit_with_no_budget_left_gives_up_a_document_a_refusal_left_owed() {
        crate::test_support::on_the_window_thread_exiting();
        let (mut store, jobs, answer, root) = a_store_the_test_answers("no-budget-owed");
        let start = Instant::now();
        store.record(a_layout(1), start);
        store.flush_if_due(start + SESSION_DEBOUNCE);
        answer
            .send(refused(1))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        store.flush_if_due(start + SESSION_DEBOUNCE * 2);
        assert_eq!(documents_handed_over(&jobs), vec![1]);
        assert!(store.owes_a_write(), "owed after the refusal");

        let spent = start + SESSION_DEBOUNCE * 3;
        let verdict = store.flush_for_quit(spent, spent);
        assert!(
            verdict.as_ref().is_err_and(SaveRefusal::quit_may_proceed),
            "given up as a write whose budget ran out: {verdict:?}"
        );
        store.close();
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "the way out's close hands nothing over"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 2, nit b) — **a save after the worker ended
    /// does not wait for the one it took**: that one will never be answered, so the next save
    /// goes ahead and is refused at the door, and is owed like any refusal — not parked in the
    /// pending slot for ever.
    ///
    /// And the quit that follows meets the same refusal, in its own words.
    ///
    /// MUTATION: `SessionWriter::busy` reduced to `self.in_flight().is_some()` — the change waits
    /// in the pending slot and no refusal is booked.
    #[test]
    fn a_save_after_the_worker_ended_goes_ahead_and_is_refused_at_the_door() {
        crate::test_support::on_the_window_thread_exiting();
        let (mut store, jobs, answer, root) = a_store_the_test_answers("worker-ended");
        let start = Instant::now();
        store.record(a_layout(1), start);
        store.flush_if_due(start + SESSION_DEBOUNCE);
        assert_eq!(
            documents_handed_over(&jobs),
            vec![1],
            "one write on its way"
        );

        // The worker's end, as `WorkerEnd` makes it: under the queue's lock the lane is marked
        // down, and the queue's receiving end — which the worker owned — is gone, so a job sent
        // to it is refused. The write it took is never answered. Its wake has not been heard
        // yet: the frame's turn comes first.
        {
            let _admission = lane_lock(&store.writer.lane.shared.requests);
            store
                .writer
                .lane
                .shared
                .up
                .store(false, std::sync::atomic::Ordering::Release);
        }
        drop(jobs);

        let changed = start + SESSION_DEBOUNCE * 2;
        store.record(a_layout(2), changed);
        store.flush_if_due(changed + SESSION_DEBOUNCE);
        assert!(!store.send_again, "not parked in the pending slot");
        assert_eq!(
            store.writer.answered, None,
            "the write the worker took is still unanswered"
        );
        assert!(
            store.retry_on_trigger,
            "the change went ahead, was refused at the door, and is owed"
        );
        assert_eq!(store.writes.streak, 1, "and the refusal is booked");

        // The wake's turn: the receipts' channel reads closed, and the write the worker took is
        // booked refused — the writer stopped — as the streak's second refusal.
        drop(answer);
        store.take_receipts();
        assert_eq!(
            store.writer.answered,
            Some((1, Err(WRITER_STOPPED.to_string()))),
            "the worker's end is the terminal of the write it took"
        );
        assert_eq!(store.writes.streak, 2);
        assert_eq!(
            store.flush_judged(),
            Err(SaveRefusal::Refused(NO_WRITER_THREAD.to_string())),
            "the quit hands the owed document over and meets the refusal at the door"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 3, nit 1) — **a quit with no budget left gives
    /// up the pending slot too**: a write is on its way and a change waits behind it; the way
    /// out's close hands neither over again.
    ///
    /// MUTATION: leave `send_again` set on `SessionStore::flush_for_quit`'s past-budget road
    /// (clear only `retry_on_trigger`) — the close's flush hands the parked change over.
    #[test]
    fn a_quit_with_no_budget_left_gives_up_a_parked_change() {
        crate::test_support::on_the_window_thread_exiting();
        let (mut store, jobs, _answer, root) = a_store_the_test_answers("no-budget-parked");
        let start = Instant::now();
        store.record(a_layout(1), start);
        store.flush_if_due(start + SESSION_DEBOUNCE);
        let changed = start + SESSION_DEBOUNCE * 2;
        store.record(a_layout(2), changed);
        store.flush_if_due(changed + SESSION_DEBOUNCE);
        assert_eq!(
            documents_handed_over(&jobs),
            vec![1],
            "one write on its way"
        );
        assert!(store.send_again, "and a change parked behind it");

        let spent = changed + SESSION_DEBOUNCE * 2;
        let verdict = store.flush_for_quit(spent, spent);
        assert!(
            verdict.as_ref().is_err_and(SaveRefusal::quit_may_proceed),
            "given up as a write whose budget ran out: {verdict:?}"
        );
        store.close();
        assert_eq!(
            documents_handed_over(&jobs),
            Vec::<u64>::new(),
            "the way out's close hands nothing over"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 4) — **an update quit's refusal that is the
    /// fifth in a row is one card too**: the receipt for the quit's photograph is heard on the
    /// turn its wake brings, `landing_of` hands the quit its refusal, and the quit's card says
    /// the streak — the autosave's turn after it owes nothing.
    ///
    /// MUTATION: drop `self.said_by_the_quit(verdict)` from `SessionStore::landing_of` — the
    /// autosave's turn owes the sentence as well.
    #[test]
    fn an_update_quits_refusal_that_is_the_fifth_in_a_row_is_one_card() {
        let (mut store, jobs, answer, root) = a_store_the_test_answers("update-quit-one-card");
        let mut now = Instant::now();
        for step in 1..REFUSALS_BEFORE_TOLD {
            store.record(a_layout(step), now);
            now += SESSION_DEBOUNCE;
            store.flush_if_due(now);
            let generation = *documents_handed_over(&jobs)
                .last()
                .expect("each change is handed over");
            answer
                .send(refused(generation))
                .expect("the store holds its receipts");
            store.take_receipts(); // the turn the receipt's wake brings
            now += SESSION_DEBOUNCE;
            assert_eq!(store.autosave(now), None, "not yet");
        }

        let photograph = store
            .hand_over_final()
            .expect("handed over")
            .expect("this store writes");
        assert_eq!(documents_handed_over(&jobs), vec![photograph]);
        answer
            .send(refused(photograph))
            .expect("the store holds its receipts");
        store.take_receipts(); // the turn the receipt's wake brings
        assert_eq!(
            store.landing_of(photograph),
            Some(Err(SaveRefusal::Refused(
                "磁盘已满 the disk is full".to_owned()
            ))),
            "the quit hears its refusal"
        );
        assert_eq!(
            store.writes.streak, REFUSALS_BEFORE_TOLD,
            "the fifth in a row"
        );
        now += SESSION_DEBOUNCE;
        assert_eq!(
            store.autosave(now),
            None,
            "the quit's card said it; the autosave's turn says nothing more"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 2, nit d) — **a quit's refusal that is the
    /// fifth in a row is one card**: the quit's own (`QuitSessionNotWritten`, on every window),
    /// and not the autosave's sentence again on the turn after the quit is abandoned. The quit's
    /// card is what says this streak; the next one is said as usual.
    ///
    /// Four refused autosaves, then the quit's judged write, which the test's disk refuses as
    /// soon as it is handed over.
    ///
    /// MUTATION: drop `self.said_by_the_quit(&verdict)` from `SessionStore::flush_for_quit` —
    /// the autosave's turn after the abandoned quit owes the sentence too.
    #[test]
    fn a_quits_refusal_that_is_the_fifth_in_a_row_is_one_card() {
        crate::test_support::on_the_window_thread_exiting();
        let (mut store, jobs, answer, root) = a_store_the_test_answers("quit-is-one-card");
        let mut now = Instant::now();
        for step in 1..REFUSALS_BEFORE_TOLD {
            store.record(a_layout(step), now);
            now += SESSION_DEBOUNCE;
            store.flush_if_due(now);
            let generation = *documents_handed_over(&jobs)
                .last()
                .expect("each change is handed over");
            answer
                .send(refused(generation))
                .expect("the store holds its receipts");
            store.take_receipts(); // the turn the receipt's wake brings
            now += SESSION_DEBOUNCE;
            assert_eq!(store.autosave(now), None, "not yet");
        }

        store.set_save_budget(crate::lane::PATIENCE);
        let quits_disk = answer.clone();
        let (verdict, jobs) = std::thread::scope(|scope| {
            let disk = scope.spawn(move || {
                match jobs.receive("the quit's session write") {
                    StorageJob::Session(request) => quits_disk
                        .send(refused(request.generation))
                        .expect("the store holds its receipts"),
                    _ => panic!("the quit hands a session document over"),
                }
                jobs
            });
            let verdict = store.flush_for_quit(now + crate::lane::PATIENCE, now);
            (verdict, disk.join().expect("the test's disk"))
        });
        assert!(
            verdict
                .as_ref()
                .is_err_and(|refusal| !refusal.quit_may_proceed()),
            "the quit's write is refused, and the quit is abandoned: {verdict:?}"
        );
        assert_eq!(
            store.writes.streak, REFUSALS_BEFORE_TOLD,
            "the fifth in a row"
        );
        assert!(jobs.try_recv().is_err(), "one document for the quit");
        now += SESSION_DEBOUNCE;
        assert_eq!(
            store.autosave(now),
            None,
            "the quit's card said it; the autosave's turn says nothing more"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An environment that answers only what it was handed, so a platform's arm
    /// can be asked about a machine this test is not running on. The real
    /// `std::env::var_os` is process-wide and a test that set it would be
    /// setting it for every test running beside it.
    fn machine(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let known: HashMap<String, OsString> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(*value)))
            .collect();
        move |name: &str| known.get(name).cloned()
    }

    /// PIN — **Windows is `%APPDATA%\Folio`, and it is the one platform with a
    /// name to carry** (M2-6).
    ///
    /// The dialect this product has shipped under since before it was named.
    /// Both halves are pinned in one place because they are one sentence: the
    /// directory, and the `BetterTerminal` beside it that a single startup owes
    /// the reader.
    ///
    /// Red gate: give the macOS arm's shape to this one and the first assertion
    /// names it; drop the old name from this arm and the second does, which is
    /// every upgrading reader's files left behind.
    #[test]
    fn windows_is_appdata_folio_with_the_old_name_beside_it() {
        let appdata = PathBuf::from(r"X:\Users\dev\AppData\Roaming");
        let chosen = storage_location(
            bt_platform::HostPlatform::Windows,
            machine(&[("APPDATA", r"X:\Users\dev\AppData\Roaming")]),
        );
        assert_eq!(chosen.directory, appdata.join("Folio"));
        assert_eq!(chosen.previous, Some(appdata.join("BetterTerminal")));
    }

    /// PIN — **no `APPDATA`, so the temp directory, and the old name follows it
    /// there** (M2-6).
    ///
    /// The fallback is not a courtesy: a store that cannot resolve a directory
    /// has nowhere to write the diagnostic that would say so. The relocation
    /// still applies, because a previous build with the same broken environment
    /// wrote to the same place.
    #[test]
    fn windows_without_appdata_falls_back_to_the_temp_directory() {
        let chosen = storage_location(bt_platform::HostPlatform::Windows, machine(&[]));
        assert_eq!(chosen.directory, std::env::temp_dir().join("Folio"));
        assert_eq!(
            chosen.previous,
            Some(std::env::temp_dir().join("BetterTerminal"))
        );
    }

    /// PIN — **macOS is `~/Library/Application Support/Folio`, and there is
    /// nothing to relocate** (M2-6).
    ///
    /// The second assertion is the ruling, not an implementation detail: the
    /// `BetterTerminal` → `Folio` rename is a Windows-only history, so a
    /// `~/Library/Application Support/BetterTerminal` on a Mac was made by
    /// somebody else and moving it would be a bug wearing a migration's clothes.
    /// `None` is what stops `relocate` from ever being asked.
    ///
    /// Red gate: hand the macOS arm a previous name and the second assertion
    /// names it.
    #[test]
    fn macos_is_the_application_support_directory_and_carries_nothing() {
        let chosen = storage_location(
            bt_platform::HostPlatform::MacOs,
            machine(&[("HOME", "/Users/dev")]),
        );
        assert_eq!(
            chosen.directory,
            PathBuf::from("/Users/dev")
                .join("Library")
                .join("Application Support")
                .join("Folio")
        );
        assert_eq!(chosen.previous, None);
    }

    /// PIN — **no `HOME`, so the temp directory, exactly as the Windows arm
    /// answers a missing `APPDATA`** (M2-6).
    ///
    /// One fallback for both platforms, for one reason, and the test says so by
    /// asking for the same path the Windows case above asks for.
    #[test]
    fn macos_without_home_falls_back_to_the_temp_directory() {
        let chosen = storage_location(bt_platform::HostPlatform::MacOs, machine(&[]));
        assert_eq!(chosen.directory, std::env::temp_dir().join("Folio"));
        assert_eq!(chosen.previous, None);
    }

    /// PIN — **other Unix takes `$XDG_DATA_HOME` when it is set** (M2-6).
    ///
    /// Not a shipped platform; pinned so that the arm which exists because
    /// `HostPlatform` has three variants says something true rather than
    /// something Windows-shaped.
    #[test]
    fn other_unix_takes_xdg_data_home_when_it_is_set() {
        let chosen = storage_location(
            bt_platform::HostPlatform::OtherUnix,
            machine(&[
                ("XDG_DATA_HOME", "/home/dev/elsewhere"),
                ("HOME", "/home/dev"),
            ]),
        );
        assert_eq!(
            chosen.directory,
            PathBuf::from("/home/dev/elsewhere").join("Folio")
        );
        assert_eq!(chosen.previous, None);
    }

    /// PIN — **and `~/.local/share` when it is not** (M2-6).
    #[test]
    fn other_unix_falls_back_to_the_local_share_directory() {
        let chosen = storage_location(
            bt_platform::HostPlatform::OtherUnix,
            machine(&[("HOME", "/home/dev")]),
        );
        assert_eq!(
            chosen.directory,
            PathBuf::from("/home/dev")
                .join(".local")
                .join("share")
                .join("Folio")
        );
        assert_eq!(chosen.previous, None);
    }

    /// PIN — state one of three: nothing was ever written under the old name, so
    /// there is nothing to carry and nothing is invented.
    ///
    /// This is every machine that meets this product for the first time, and the
    /// claim worth making about it is the negative one: the relocation does not
    /// create the directory. Creating it here would take the decision away from
    /// `SessionStore::open`, which is the code that knows whether a directory it
    /// cannot create is fatal (it is not).
    ///
    /// Red gate: drop the `previous.is_dir()` guard and a first run starts
    /// reporting a move it did not make.
    #[test]
    fn a_machine_that_never_knew_the_old_name_has_nothing_to_carry() {
        let root = appdata("fresh");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        assert!(
            matches!(relocate(&previous, &current), Relocation::Nothing),
            "neither name exists, so there is nothing to carry"
        );
        assert!(
            !current.exists(),
            "the relocation does not create a directory"
        );
        assert!(!previous.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — state two of three: the whole directory arrives under the new name,
    /// with everything that was in it.
    ///
    /// The nested file is the point rather than decoration. The directory holds
    /// `session.json`, `settings.json`, a lock sentinel *and* an installed
    /// `shell-integration/folio.bash`, and a relocation written as a list of
    /// known filenames would carry the two documents and silently leave the
    /// script — which would then be rewritten on next use and look fine, hiding
    /// the fact that the same bug drops whatever is added next.
    ///
    /// Red gate: replace the directory rename with a copy of the two JSON
    /// documents and the nested assertion fails.
    #[test]
    fn the_whole_old_directory_arrives_under_the_new_name() {
        let root = appdata("carry");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        write(&previous.join("settings.json"), r#"{"version":4}"#);
        write(&previous.join("session.json"), r#"{"version":6}"#);
        write(
            &previous.join("shell-integration/folio.bash"),
            "# installed",
        );

        assert!(
            matches!(relocate(&previous, &current), Relocation::Moved),
            "the old name exists and the new one does not: this is the one start \
             that moves anything"
        );

        assert_eq!(
            read(&current.join("settings.json")).as_deref(),
            Some(r#"{"version":4}"#)
        );
        assert_eq!(
            read(&current.join("session.json")).as_deref(),
            Some(r#"{"version":6}"#)
        );
        assert_eq!(
            read(&current.join("shell-integration/folio.bash")).as_deref(),
            Some("# installed"),
            "a rename carries what it does not have to know the name of"
        );
        assert!(
            !previous.exists(),
            "the old name is gone, so the next start takes the first branch"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — state three of three: a directory that already moved is not moved
    /// again, and the old name — if something recreated it — cannot overwrite it.
    ///
    /// The second start is the ordinary case for every upgraded machine, and the
    /// dangerous one: a relocation that ran unconditionally would, on a machine
    /// where anything ever recreated the old directory, replace a live settings
    /// file with a stale one. So the assertion is not merely `AlreadyHere` — it
    /// is that the current document is *still the current document*.
    ///
    /// Red gate: drop the `current.exists()` guard. `fs::rename` onto an existing
    /// directory fails on Windows, so the visible symptom would be a spurious
    /// failure banner on every start; on a platform where it succeeds it is data
    /// loss. The guard is what makes the operation idempotent rather than lucky.
    #[test]
    fn a_directory_that_already_moved_is_left_alone() {
        let root = appdata("already");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join(STORAGE_NAME);
        write(&previous.join("settings.json"), "stale");
        write(&current.join("settings.json"), "live");

        assert!(
            matches!(relocate(&previous, &current), Relocation::AlreadyHere),
            "both names exist, so the move has already happened"
        );

        assert_eq!(
            read(&current.join("settings.json")).as_deref(),
            Some("live"),
            "the directory in use outranks the one it replaced"
        );
        assert_eq!(
            read(&previous.join("settings.json")).as_deref(),
            Some("stale"),
            "and the old one is left where it is rather than deleted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — a move that cannot happen loses nothing.
    ///
    /// The failure is made by pointing the new name at a directory whose parent
    /// does not exist, which is what an `%APPDATA%` that has gone away underneath
    /// a running process looks like; the realistic cause on a live machine is a
    /// second process holding the old directory open. Either way the outcome the
    /// user must get is the same one: their files are still there, under the name
    /// they were under, and [`storage_dir`] keeps reading them.
    ///
    /// Red gate: turn the error arm into `Relocation::Moved` — or make
    /// `storage_dir` return `current` regardless — and an upgrade that could not
    /// move the directory presents as a terminal that forgot every setting, with
    /// the settings sitting one directory away.
    #[test]
    fn a_move_that_cannot_happen_leaves_the_files_where_they_are() {
        let root = appdata("refused");
        let previous = root.join(PREVIOUS_STORAGE_NAME);
        let current = root.join("no-such-parent").join(STORAGE_NAME);
        write(&previous.join("settings.json"), "mine");

        assert!(
            matches!(relocate(&previous, &current), Relocation::Failed(_)),
            "a rename into a directory that does not exist cannot succeed"
        );

        assert_eq!(
            read(&previous.join("settings.json")).as_deref(),
            Some("mine"),
            "the files are exactly where they were"
        );
        assert!(!current.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — the two names, spelled out.
    ///
    /// The old one is a fact about disks that already exist and can never be
    /// edited to something else; the new one is the product's name and is what
    /// `%APPDATA%\Folio` in `docs/M2-persistence-schema-v1.md` §1.2 means.
    #[test]
    fn the_storage_directory_is_named_for_the_product() {
        assert_eq!(STORAGE_NAME, crate::APP_NAME);
        assert_eq!(PREVIOUS_STORAGE_NAME, "BetterTerminal");
    }

    // ── what a re-read of `profiles.json` finds (§7.1.6c-6d) ────────────────

    /// One entry, so a document can be told from the empty one by looking at it.
    fn one_profile(id: &str) -> ProfilesV1 {
        ProfilesV1 {
            schema_version: bt_persist::PROFILES_SCHEMA_VERSION,
            profiles: vec![bt_persist::ProfileEntryV1 {
                id: id.to_owned(),
                ..bt_persist::ProfileEntryV1::default()
            }],
        }
    }

    /// PIN — **this window's own writing is not news.**
    ///
    /// The watch is on `%APPDATA%\Folio\` and every keystroke in the profile
    /// editor writes a file in it, so the folder moves constantly *because of
    /// this window*. The comparison of the file's bytes with the store's
    /// revision is the whole of what keeps that from being a re-read that
    /// reinstalls the table under the reader's hands.
    ///
    /// Red gate: answer `Changed` whenever the file parses and a window loses
    /// its editor's focus every time it saves.
    #[test]
    fn a_file_that_says_what_it_already_said_is_not_news() {
        let root = appdata("profiles-unchanged");
        let path = root.join(PROFILES_FILE_NAME);
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();

        let mut store = ProfilesStore::at(path.clone());
        assert_eq!(store.loaded().profiles.len(), 1);
        assert!(matches!(store.reread(), Heard::Unchanged));

        // Written again, byte for byte — which is what an editor that saves an
        // unmodified buffer does, and what this window does on a keystroke that
        // changes nothing.
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();
        assert!(matches!(store.reread(), Heard::Unchanged));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **a hand edit is news, and taking it once is what makes it news
    /// once.**
    #[test]
    fn a_hand_edit_is_read_once_and_then_stands_as_the_document_in_force() {
        let root = appdata("profiles-changed");
        let path = root.join(PROFILES_FILE_NAME);
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();
        let mut store = ProfilesStore::at(path.clone());

        bt_persist::write_profiles_atomic(&path, &one_profile("cmd")).unwrap();
        assert!(matches!(store.reread(), Heard::Changed));
        assert_eq!(store.loaded().profiles[0].id, "cmd");
        assert!(
            matches!(store.reread(), Heard::Unchanged),
            "the news was taken, so reading again finds none"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **a file that stops parsing leaves the last good document in
    /// force**, which is the schemes watcher's ruling one file over: the
    /// window must not change under somebody *because* they typed a comma
    /// wrong, and the copy they can fix by hand must not be the one thing the
    /// window threw away.
    ///
    /// Startup is deliberately not this: with nothing yet in force there is no
    /// last good document to keep, so `open` falls back to the shipped table and
    /// says so. This is the same file read from the other end of a session.
    ///
    /// Red gate: hand `Changed` back for a damaged file and one stray keystroke
    /// in an editor empties somebody's profile list.
    #[test]
    fn a_damaged_file_keeps_the_last_good_one_and_is_reported_rather_than_taken() {
        let root = appdata("profiles-damaged");
        let path = root.join(PROFILES_FILE_NAME);
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();
        let mut store = ProfilesStore::at(path.clone());

        std::fs::write(&path, "{ \"schema_version\": 1, \"profiles\": [ ").unwrap();
        assert!(matches!(store.reread(), Heard::Unreadable));
        assert_eq!(
            store.loaded().profiles[0].id,
            "pwsh",
            "the last document that parsed is still the one in force"
        );

        // And the way back is the file itself becoming readable again — no
        // relaunch, and no flag left set by the failure.
        bt_persist::write_profiles_atomic(&path, &one_profile("cmd")).unwrap();
        assert!(matches!(store.reread(), Heard::Changed));
        assert_eq!(store.loaded().profiles[0].id, "cmd");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **deleting the file is a legible edit, not a failure**: no file at
    /// all is `profiles.json`'s own way of spelling "no departures from the
    /// shipped table", and it means that whether it has never existed or has
    /// just been thrown away.
    #[test]
    fn a_file_that_is_deleted_means_the_shipped_table_rather_than_an_error() {
        let root = appdata("profiles-deleted");
        let path = root.join(PROFILES_FILE_NAME);
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();
        let mut store = ProfilesStore::at(path.clone());

        std::fs::remove_file(&path).unwrap();
        assert!(matches!(store.reread(), Heard::Changed));
        assert_eq!(
            store.loaded(),
            &ProfilesV1::default(),
            "no file is no departures, which is the shipped five"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (A1d, revision (c)8 item 9) — **the session writer's two waits take their own doors'
    /// tokens, by value.**
    ///
    /// MUTATION: take the token off `SessionWriter::close`, or give it `SessionWriteWait`'s, and
    /// this does not compile.
    #[test]
    fn the_session_writers_two_waits_take_their_own_doors_tokens() {
        let _: fn(
            &mut SessionWriter,
            WaitToken<'_, doors::SessionWriteWait>,
            u64,
            Instant,
        ) -> SessionWaitAnswer = SessionWriter::wait_for;
        let _: fn(&mut SessionWriter, WaitToken<'_, doors::SessionWriterRetire>) =
            SessionWriter::close;
    }

    /// RED (A1d, rows 16 and 16b) — **on the way out, the quit's judged save is one admitted
    /// `SessionWriteWait` and the store's close is one admitted `SessionWriterRetire`.**
    ///
    /// Through the real roads: `flush_judged` (what `QuitStep::Write` calls) with a document to
    /// land, then `close` (what `App::finish` calls) with nothing left to land, on a test thread
    /// entered as the window thread and on its way out. The document is on the disk and the
    /// sentinel is gone, exactly as before A1d.
    ///
    /// MUTATION: mint the wait anywhere but `wait_for_landing` (in `flush_judged`'s first branch
    /// too, say) and the list grows; drop `Exiting` from either door and its call is refused.
    #[test]
    fn on_the_way_out_the_quits_save_and_the_writers_retirement_are_each_one_admission() {
        crate::test_support::on_the_window_thread_exiting();
        let root = bt_testpath::temp_path("bt-app-session-admitted");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let sentinel = root.join("session.lock");
        std::fs::write(&sentinel, b"").expect("this run's claim to still be running");
        let mut store = SessionStore::at(root.join("session.json"), sentinel.clone());
        store.armed = true;
        // The admissions are the subject; the save and the writer's end are awaited within the
        // suite's patience.
        store.set_save_budget(crate::lane::PATIENCE);
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        store.record(document, Instant::now());
        let _ = crate::hang_watch::admissions_on_this_thread();

        assert_eq!(store.flush_judged(), Ok(()), "the quit's save lands");
        assert_eq!(
            crate::hang_watch::admissions_on_this_thread(),
            ["SessionWriteWait"],
            "and its one wait was one admission"
        );
        store.close();
        assert_eq!(
            crate::hang_watch::admissions_on_this_thread(),
            ["SessionWriterRetire"],
            "the close had nothing left to wait for but the writer's end"
        );
        assert!(
            root.join("session.json").exists(),
            "the document is on the disk"
        );
        assert!(!sentinel.exists(), "and the run vouched for it");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (A1d, rows 16 and 16b) — **a session wait asked before the way out is not waited: the
    /// quit hears the stalled answer and may go on, and the close leaves the writer and the
    /// sentinel as a spent budget does.**
    ///
    /// The window thread in `Running`, where neither door is admitted. Nothing is admitted, the
    /// judged save answers `TimedOut` — the answer the transaction reads as "leave anyway" — and
    /// the sentinel stays, because nobody heard the document land.
    ///
    /// MUTATION: map the wait's refusal to `SaveRefusal::Refused` and the quit would stop over it
    /// (the verdict assertion goes red); drop the `stalled` in the close's refusal and the
    /// sentinel goes.
    #[test]
    fn a_session_wait_asked_before_the_way_out_is_the_stalled_answer() {
        crate::test_support::on_the_window_thread();
        let root = bt_testpath::temp_path("bt-app-session-refused");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let sentinel = root.join("session.lock");
        std::fs::write(&sentinel, b"").expect("this run's claim to still be running");
        let mut store = SessionStore::at(root.join("session.json"), sentinel.clone());
        store.armed = true;
        let mut document = SessionV1::default();
        document
            .windows
            .push(bt_persist::SessionWindowV1::default());
        store.record(document, Instant::now());

        let verdict = store.flush_judged();
        assert_eq!(
            verdict,
            Err(SaveRefusal::TimedOut(save_did_not_finish(
                SESSION_SAVE_BUDGET
            ))),
            "a wait that was not admitted is the stalled answer"
        );
        assert!(
            verdict.is_err_and(|refusal| refusal.quit_may_proceed()),
            "which the quit reads as `leave anyway`"
        );
        store.close();
        assert!(
            sentinel.exists(),
            "a close that could not wait for the writer does not vouch for the document"
        );
        assert!(
            crate::hang_watch::admissions_on_this_thread().is_empty(),
            "and nothing was admitted"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **The session's answers on the storage worker's wake** (T-SESSION-RECEIPTS-ON-THE-WAKE; the
/// side-effects contract's C2): a receipt, the worker's end and an update's hand-over are each
/// announced by the wake the loop answers, after they are in their channels, and nothing turns a
/// clock to look for them.
#[cfg(test)]
mod receipt_wake_tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Instant;

    use bt_persist::{SessionV1, SessionWindowV1};

    use super::{
        SENTINEL_FILE_NAME, SESSION_DEBOUNCE, SESSION_FILE_NAME, SaveRefusal, SessionStore,
        SessionWriter, StorageLane, WRITER_STOPPED, lane_lock,
    };
    use crate::document_request::ReadJob;
    use crate::quit::{HANDOFF_DEADLINE, Quit, Reason, UPDATE_RECEIPT_DEADLINE, WriteVerdict};
    use crate::update_handoff::{HandedOff, Looked, look};
    use crate::update_txn::TxnId;

    /// A private folder for one test, named in two scripts.
    fn folder(case: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-receipt-wake-会话-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private folder for this test");
        root
    }

    /// A wake the test hears: one `()` per wake, as the loop would be woken.
    fn wakes() -> (impl Fn() + Send + Sync + 'static, mpsc::Receiver<()>) {
        let (woke, wakes) = mpsc::channel();
        (
            move || {
                let _ = woke.send(());
            },
            wakes,
        )
    }

    /// A session with one window whose tab carries a Chinese title, so the document is a
    /// change worth writing.
    fn a_session() -> SessionV1 {
        SessionV1 {
            windows: vec![SessionWindowV1 {
                tabs: vec![crate::test_support::saved_tab(
                    "default",
                    "/会话 session",
                    Some("标签 tab"),
                    false,
                )],
                ..SessionWindowV1::default()
            }],
            ..SessionV1::default()
        }
    }

    /// **Hold the storage worker inside a re-read's answer** until the returned sender says go.
    /// The re-read then wakes the loop after its answer, as every document answer does.
    fn hold_the_worker(store: &SessionStore) -> mpsc::Sender<()> {
        let (entered, inside) = mpsc::channel::<()>();
        let (open, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let admitted = store.lane().send_read(ReadJob {
            which: bt_testpath::temp_path("bt-app-receipt-wake-held-读"),
            generation: 1,
            answer: Box::new(move |_, _| {
                let _ = entered.send(());
                crate::lane::receive(
                    &gate.lock().expect("the test's gate"),
                    "the test's word to go on",
                );
            }),
        });
        assert!(admitted.is_ok(), "the worker takes the re-read");
        crate::lane::receive(&inside, "the worker inside the held re-read");
        open
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE) — **a session receipt is heard on its wake, not on
    /// the clock.** The autosave hands a document over on the clock's turn; the worker writes it
    /// and wakes the loop; the turn that wake brings takes the receipt
    /// (`SessionStore::take_receipts`, what `AppEvent::DocumentsAnswered` runs) and the store
    /// owes nothing more — no later clock turn is needed, and none is asked for.
    ///
    /// MUTATION: drop the `wake()` after a session receipt in the worker's loop
    /// (`SessionWriter::start`) — no wake comes, and the bounded wait runs out.
    #[test]
    fn a_session_receipt_is_heard_on_its_wake_not_on_the_clock() {
        crate::test_support::on_the_window_thread_exiting();
        let root = folder("heard");
        let (wake, wakes) = wakes();
        let mut store = SessionStore::at_waking(
            root.join(SESSION_FILE_NAME),
            root.join(SENTINEL_FILE_NAME),
            wake,
        );
        let changed_at = Instant::now();
        store.record(a_session(), changed_at);
        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        assert_eq!(
            store.writer.in_flight(),
            Some(1),
            "the clock handed it over"
        );
        assert_eq!(
            store.deadline(),
            None,
            "and asks the clock back for nothing while the write is out"
        );

        crate::lane::wait_for_a_wake(&wakes, "the worker's wake after the receipt");
        store.take_receipts();
        assert_eq!(
            store.writer.in_flight(),
            None,
            "the wake's turn heard the receipt"
        );
        assert_eq!(store.writer.answered, Some((1, Ok(()))));
        assert_eq!(store.deadline(), None, "nothing is owed");
        assert_eq!(
            std::fs::read(root.join(SESSION_FILE_NAME)).expect("the file"),
            bt_persist::serialize_session(&a_session()).expect("the session serialises"),
        );
        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE, C2) — **a storage worker that ends wakes the loop,
    /// and the session write it took reads as refused on that wake's turn.** The worker is held
    /// where a re-read begins, at the suite's gate, and ended there (`lane::Gate::kill`: a panic
    /// outside any request's task, which the executor's containment makes the worker's end —
    /// since T-STORAGE-WORKER-ON-THE-SEAM a panic inside one request is only that request's
    /// failure); the autosave's document was queued behind it. The end drops it, closes the
    /// receipts' channel, and the ledger ends what was owed and wakes the loop: the turn books
    /// the document as refused — the writer stopped — once, and an update's look at it answers
    /// the same.
    ///
    /// MUTATION: drop the worker's-end arm of `SessionStore::take_receipts` — the document stays
    /// in flight, unanswered, after the wake.
    #[test]
    fn a_storage_worker_that_ends_wakes_the_loop_and_its_receipts_read_as_refused() {
        crate::test_support::on_the_window_thread_exiting();
        let root = folder("ended");
        let (wake, wakes) = wakes();
        let gate = Arc::new(crate::lane::Gate::default());
        let mut store = SessionStore::at_waking_started_by(
            root.join(SESSION_FILE_NAME),
            root.join(SENTINEL_FILE_NAME),
            wake,
            crate::lane::Executor::scripted(Arc::clone(&gate)),
        );
        gate.hold();
        let held = store.lane().send_read(ReadJob {
            which: bt_testpath::temp_path("bt-app-receipt-wake-ended-读"),
            generation: 1,
            answer: Box::new(|_, _| {}),
        });
        assert!(held.is_ok(), "the worker takes the re-read");
        assert!(
            gate.wait_entered(1),
            "the worker stands where the re-read begins"
        );
        let changed_at = Instant::now();
        store.record(a_session(), changed_at);
        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        assert_eq!(
            store.writer.in_flight(),
            Some(1),
            "queued behind the held re-read"
        );
        gate.kill();
        gate.release();

        crate::lane::wait_for_a_wake(&wakes, "the worker's end");
        store.take_receipts();
        assert_eq!(
            store.writer.in_flight(),
            None,
            "the document has its terminal"
        );
        assert_eq!(
            store.writer.answered,
            Some((1, Err(WRITER_STOPPED.to_string()))),
            "refused: the writer stopped"
        );
        assert_eq!(
            store.landing_of(1),
            Some(Err(SaveRefusal::Refused(WRITER_STOPPED.to_string())))
        );
        assert!(wakes.try_recv().is_err(), "the end woke the loop once");
        assert!(!root.join(SESSION_FILE_NAME).exists(), "nothing landed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORAGE-WORKER-ON-THE-SEAM, R2) — **a session write the storage worker fails on is
    /// a refusal in the failure's words, and the worker goes on.** The worker serves the
    /// session's first write through a door that panics inside the request's task; its receipt
    /// is a refusal naming the failure, booked on the wake's turn as any refusal is — the document
    /// is owed — and the next change's write lands on the same worker.
    ///
    /// MUTATION: send no receipt for a failed session write in `SessionWriter::start`'s loop —
    /// the wake's turn finds nothing booked, and the first write stays in flight.
    #[test]
    fn a_session_write_the_worker_fails_on_is_refused_in_its_words_and_the_worker_goes_on() {
        crate::test_support::on_the_window_thread_exiting();
        let root = folder("failed");
        let (wake, wakes) = wakes();
        let (mut lane, receipts) = StorageLane::fresh_with(
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            Arc::new(wake),
            Box::new(crate::diagnostics::note),
            Arc::new(super::let_go),
        );
        Arc::get_mut(&mut lane.shared)
            .expect("a lane just made has one owner")
            .door = Some(Arc::new(|worker, job, receipts| {
            if let super::StorageJob::Session(request) = &*job
                && request.generation == 1
            {
                panic!("scripted: the session write fails 写入失败");
            }
            super::serve_storage_job(worker, job, receipts)
        }));
        let mut store =
            SessionStore::at(root.join(SESSION_FILE_NAME), root.join(SENTINEL_FILE_NAME));
        store.writer = SessionWriter {
            lane,
            receipts,
            sent: 0,
            landed: 0,
            answered: None,
            ended: false,
            _receipts_held_open: None,
            stalled: false,
            budget: super::SESSION_SAVE_BUDGET,
        };
        assert!(
            SessionWriter::start(&store.writer.lane),
            "a worker serves the lane"
        );

        let changed_at = Instant::now();
        store.record(a_session(), changed_at);
        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        crate::lane::wait_for_a_wake(&wakes, "the failed write's receipt");
        store.take_receipts();
        let Some((1, Err(said))) = store.writer.answered.clone() else {
            panic!(
                "the first write is a refusal, not {:?}",
                store.writer.answered
            );
        };
        assert!(
            said.contains("the storage worker failed on this request")
                && said.contains("scripted: the session write fails 写入失败"),
            "refused in the failure's words: {said}"
        );
        assert!(store.retry_on_trigger, "and the document is owed");

        let mut changed = a_session();
        changed.windows.push(SessionWindowV1::default());
        let changed_at = Instant::now();
        store.record(changed.clone(), changed_at);
        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        crate::lane::wait_for_a_wake(&wakes, "the next write's receipt");
        store.take_receipts();
        assert_eq!(
            store.writer.answered,
            Some((2, Ok(()))),
            "the worker went on and the next write landed"
        );
        assert_eq!(
            std::fs::read(root.join(SESSION_FILE_NAME)).expect("the file"),
            bt_persist::serialize_session(&changed).expect("the session serialises"),
        );
        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE, C2's order) — **the worker's end closes the session
    /// receipts' channel before it wakes the loop**, so the turn that wake brings reads the end
    /// rather than an open channel nobody will ever answer on.
    ///
    /// MUTATION: keep the receipts' sending end past `WorkerEnd::drop` until after the end's
    /// wake (hand it to `StorageEnds`, dropped after its wake) — at the wake the channel is
    /// still open.
    #[test]
    fn the_workers_end_closes_the_session_receipts_before_its_wake() {
        let seen: Arc<Mutex<Option<mpsc::Receiver<super::SessionWriteReceipt>>>> = Arc::default();
        let looked = Arc::clone(&seen);
        let (woke, wakes) = mpsc::channel::<bool>();
        let (lane, receipts) = StorageLane::fresh_with(
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            Arc::new(move || {
                let closed =
                    looked
                        .lock()
                        .expect("the receipts")
                        .as_ref()
                        .is_some_and(|receipts| {
                            matches!(receipts.try_recv(), Err(mpsc::TryRecvError::Disconnected))
                        });
                let _ = woke.send(closed);
            }),
            Box::new(crate::diagnostics::note),
            Arc::new(super::let_go),
        );
        *seen.lock().expect("the receipts") = Some(receipts);
        assert!(SessionWriter::start(&lane), "a worker serves the lane");
        drop(lane_lock(&lane.shared.requests).take());
        assert!(
            crate::lane::receive(&wakes, "the worker's end"),
            "the receipts' channel was closed when the loop was woken"
        );
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE) — **an update's quit hears its session's receipt on
    /// a wake, across turns.** The photograph goes over as a named generation behind a held
    /// re-read; the quit's first turn finds no answer and asks the clock back only at the
    /// receipt's bound. Released, the worker answers the re-read (its wake) and then writes the
    /// photograph and wakes the loop again; that second wake's turn finds the landing.
    ///
    /// MUTATION: drop the `wake()` after a session receipt in the worker's loop — the second
    /// wake never comes, and the bounded wait runs out.
    #[test]
    fn the_quits_session_receipt_arrives_on_a_wake_across_turns() {
        let root = folder("quit");
        let (wake, wakes) = wakes();
        let mut store = SessionStore::at_waking(
            root.join(SESSION_FILE_NAME),
            root.join(SENTINEL_FILE_NAME),
            wake,
        );
        store.record(a_session(), Instant::now());
        let mut quit = Quit::begin_for(
            Vec::new(),
            Reason::UpdateRestart {
                txn: TxnId::new([0x5e; 16]),
            },
        );
        let start = Instant::now();
        quit.documents(&[], start);
        quit.photographed();
        let go = hold_the_worker(&store);
        let generation = store
            .hand_over_final()
            .expect("handed over")
            .expect("this store writes");
        quit.requested(generation, start);

        assert_eq!(
            store.landing_of(generation),
            None,
            "the first turn: the write is behind the held re-read"
        );
        assert_eq!(
            quit.deadline(),
            Some((start + UPDATE_RECEIPT_DEADLINE).min(quit.session_write_until(start))),
            "the loop is asked back at the receipt's bound, not to look"
        );

        go.send(()).expect("the worker waits at the gate");
        crate::lane::wait_for_a_wake(&wakes, "the re-read's own answer");
        crate::lane::wait_for_a_wake(&wakes, "the session receipt's wake");
        assert_eq!(
            store.landing_of(generation),
            Some(Ok(())),
            "the second wake's turn finds the landing"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE) — **`Quit::deadline` is an owed answer's bound,
    /// never a look.** An update's quit awaiting its session's receipt, and then its hand-over's
    /// answer, answers each one's bound — the value the loop's `ControlFlow::WaitUntil` is built
    /// from — and nothing sooner: the answers themselves are announced by the worker's wake. What
    /// is pinned is the value; the loop's wake-ups are not counted (no counter for them exists).
    ///
    /// MUTATION: put the look back — `Quit::deadline` answers 15 ms from now while a receipt or
    /// a hand-over's answer is owed (`quit::ANSWER_LOOK` as it was) — the value is not the bound.
    #[test]
    fn quit_deadline_is_the_owed_answers_bound_never_a_look() {
        let start = Instant::now();
        let mut quit = Quit::begin_for(
            Vec::new(),
            Reason::UpdateRestart {
                txn: TxnId::new([0x6f; 16]),
            },
        );
        quit.documents(&[], start);
        quit.photographed();
        quit.requested(7, start);
        assert_eq!(
            quit.deadline(),
            Some((start + UPDATE_RECEIPT_DEADLINE).min(quit.session_write_until(start))),
            "the receipt's bound, and no look before it"
        );
        quit.written(WriteVerdict::Landed);
        quit.retired(start);
        quit.pages(true, start);
        quit.handoff_sent(start);
        assert_eq!(
            quit.deadline(),
            Some(start + HANDOFF_DEADLINE),
            "the hand-over's bound, and no look before it"
        );
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE round 2) — **a session write on a scripted lane reads
    /// as unanswered, not refused**, and asks the clock back for nothing. No worker serves the
    /// lane and none has ended: the store holds its receipts' channel open
    /// (`SessionStore::at_on`), so taking the receipts finds none and books no end.
    ///
    /// MUTATION: in `SessionStore::at_on`, drop the receipts' sending end at once (as before round
    /// 2) — the channel reads closed, and the write is booked refused, the writer stopped.
    #[test]
    fn a_scripted_lanes_session_write_reads_as_unanswered_not_refused() {
        let root = folder("scripted");
        let (lane, _jobs) = StorageLane::unserved();
        let mut store = SessionStore::at_on(
            root.join(SESSION_FILE_NAME),
            root.join(SENTINEL_FILE_NAME),
            lane,
        );
        let changed_at = Instant::now();
        store.record(a_session(), changed_at);
        store.flush_if_due(changed_at + SESSION_DEBOUNCE);
        store.take_receipts();
        assert_eq!(
            store.writer.in_flight(),
            Some(1),
            "the document is out, unanswered"
        );
        assert_eq!(store.writer.answered, None, "and nothing was booked for it");
        assert_eq!(store.deadline(), None, "and no clock turns for its receipt");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-RECEIPTS-ON-THE-WAKE, C2) — **a hand-over the storage worker ended without
    /// answering is over on the turn its end's wake brings**, as an overdue one is at its bound:
    /// no applier is known, and the window's mark decides.
    ///
    /// MUTATION: in `update_handoff::look`, read a closed channel as no answer yet — the look
    /// waits for the deadline.
    #[test]
    fn a_hand_over_its_worker_dropped_is_over_on_the_wake() {
        let txn = TxnId::new([0x7a; 16]);
        let (answer, answered) = mpsc::channel::<HandedOff>();
        drop(answer);
        match look(
            Some(&mut super::OnTheWorker::answered_at_the_door(answered)),
            txn,
            false,
        ) {
            Looked::Over { line, applier } => {
                assert_eq!(applier, None, "no applier is known");
                assert!(line.contains("the storage worker stopped"), "{line}");
            }
            Looked::Waiting => panic!("a closed channel is the worker's end, not a wait"),
        }
        let (_open, waiting) = mpsc::channel::<HandedOff>();
        assert_eq!(
            look(
                Some(&mut super::OnTheWorker::answered_at_the_door(waiting)),
                txn,
                false
            ),
            Looked::Waiting,
            "an open channel with nothing in it is no answer yet"
        );
    }

    /// RED (T-STORAGE-WORKER-ON-THE-SEAM, the review's nit 5) — **a hand-over the storage
    /// worker failed on says so, not that the worker stopped**: the request's terminal is
    /// `Failed` (its task panicked, and the worker went on), its road closed with no answer;
    /// the look is over — no applier is known, the window's mark decides — and its line names
    /// the failure. A request the worker's end left owed (`WorkerGone`) still says the worker
    /// stopped.
    ///
    /// MUTATION: in `update_handoff::look`, read `Reply::Failed` as `Reply::Gone` — the line
    /// says the worker stopped.
    #[test]
    fn a_hand_over_the_worker_failed_on_says_it_failed() {
        let txn = TxnId::new([0x7b; 16]);
        let (lane, _jobs) = StorageLane::held();
        let asked = |lane: &StorageLane| {
            lane.send_read(ReadJob {
                which: bt_testpath::temp_path("bt-app-handoff-failed-读"),
                generation: 1,
                answer: Box::new(|_, _| {}),
            })
            .expect("admitted")
        };
        let failed = asked(&lane);
        lane.shared.ledger.publish(
            failed.id(),
            crate::lane::Terminal::Failed(crate::lane::TaskFailure::Panicked(
                "scripted: the hand-over fails 交接失败".to_owned(),
            )),
        );
        let (answer, answered) = mpsc::channel::<HandedOff>();
        drop(answer);
        match look(
            Some(&mut super::OnTheWorker::admitted(answered, failed)),
            txn,
            false,
        ) {
            Looked::Over { line, applier } => {
                assert_eq!(applier, None, "no applier is known");
                assert!(
                    line.contains("the storage worker failed on it")
                        && line.contains("scripted: the hand-over fails 交接失败")
                        && !line.contains("stopped"),
                    "{line}"
                );
            }
            Looked::Waiting => panic!("a failed hand-over is over"),
        }
        let gone = asked(&lane);
        lane.shared
            .ledger
            .worker_ended(crate::lane::WorkerEnd::Returned);
        let (answer, answered) = mpsc::channel::<HandedOff>();
        drop(answer);
        match look(
            Some(&mut super::OnTheWorker::admitted(answered, gone)),
            txn,
            false,
        ) {
            Looked::Over { line, .. } => {
                assert!(line.contains("the storage worker stopped"), "{line}");
            }
            Looked::Waiting => panic!("a hand-over the worker's end left is over"),
        }
    }
}

/// The four stores' revisions (`docs/plans/design/settings-reread-2026-10-11.md`
/// §3.1, §9.2, revision (b)10 and (b)14-16): what each is set to at open and on
/// a landed write, and the re-read of profiles and pins measured against it.
#[cfg(test)]
mod revision_tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use bt_persist::{BindingOverrideV1, PinKind, PinsV1, ProfileEntryV1, ProfilesV1, SettingsV1};

    use super::{
        KEYBINDINGS_FILE_NAME, KeybindingsStore, PROFILES_FILE_NAME, ProfilesStore,
        SETTINGS_FILE_NAME, SettingsStore, is_writer_of_document,
    };
    use crate::document_request::Heard;
    use crate::pins::{PINS_FILE_NAME, PinsStore};

    /// A private folder for one case, emptied on the way in.
    fn scratch(case: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-revision-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        root
    }

    /// A profile table of one row, titled in mixed script.
    fn profiles(id: &str, title: &str) -> ProfilesV1 {
        ProfilesV1 {
            schema_version: bt_persist::PROFILES_SCHEMA_VERSION,
            profiles: vec![ProfileEntryV1 {
                id: id.to_owned(),
                display_title: Some(title.to_owned()),
                ..ProfileEntryV1::default()
            }],
        }
    }

    /// A pin table of folders, in the order given.
    fn pins(folders: &[&str]) -> PinsV1 {
        let mut table = PinsV1::default();
        for folder in folders {
            crate::pins::toggle_pin(&mut table, PinKind::Folder, folder);
        }
        table
    }

    fn file_bytes(path: &Path) -> Vec<u8> {
        std::fs::read(path).expect("the document is on disk")
    }

    fn bytes_of(bytes: Option<&Arc<[u8]>>) -> Option<&[u8]> {
        bytes.map(|bytes| &**bytes)
    }

    /// Whether a `.rejected-` copy of `file_name` stands in `root`.
    fn a_copy_is_kept(root: &Path, file_name: &str) -> bool {
        std::fs::read_dir(root)
            .expect("the scratch folder lists")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{file_name}.rejected-"))
            })
    }

    /// RED (T-STORE-REVISION, §9.2; §1.5 defect 1) — **a change the store holds
    /// but has not written is not undone by a re-read.**
    ///
    /// The store adopts a change before it writes, and a change that does not
    /// reach the disk — a write refused, or, as here, a process that is not the
    /// folder's writer — leaves the in-memory document ahead of the file. The
    /// storage watch then hears the folder and the store re-reads the older
    /// file. Measured against the document in memory, that file is "different"
    /// and was installed, so the change vanished with no card. Measured against
    /// the revision, it is the file Folio left: no change.
    ///
    /// MUTATION: in `ProfilesStore::reread` / `PinsStore::reread`, compare the
    /// parsed file with `loaded()` (`if self.loaded == file { Unchanged }`,
    /// else take it) — both answer `Changed` and the held change is gone.
    #[test]
    fn an_unwritten_change_is_not_undone_by_a_reread() {
        let root = scratch("unwritten");
        let profiles_path = root.join(PROFILES_FILE_NAME);
        let pins_path = root.join(PINS_FILE_NAME);
        bt_persist::write_profiles_atomic(&profiles_path, &profiles("pwsh", "PowerShell 终端"))
            .unwrap();
        bt_persist::write_pins_atomic(&pins_path, &pins(&["项目/folio 笔记"])).unwrap();
        let profiles_on_disk = file_bytes(&profiles_path);
        let pins_on_disk = file_bytes(&pins_path);

        // Another process holds this folder's claim, so nothing this process
        // stores reaches the disk.
        let holder = bt_platform::instance::claim_data_directory(&root)
            .expect("nothing else on this machine has this folder");
        assert!(!is_writer_of_document(&profiles_path));

        let mut profile_store = ProfilesStore::at(profiles_path.clone());
        let held = profiles("claude", "Claude 代理");
        assert!(profile_store.store(held.clone()));
        assert_eq!(
            file_bytes(&profiles_path),
            profiles_on_disk,
            "nothing was written"
        );
        assert_eq!(profile_store.reread(), Heard::Unchanged);
        assert_eq!(profile_store.loaded(), &held, "the held change stands");

        let mut pin_store = PinsStore::at(pins_path.clone());
        assert!(pin_store.toggle(PinKind::Folder, "文档/Claude 代理"));
        assert_eq!(file_bytes(&pins_path), pins_on_disk, "nothing was written");
        assert_eq!(pin_store.reread(), Heard::Unchanged);
        assert_eq!(
            pin_store.targets(PinKind::Folder),
            vec!["项目/folio 笔记".to_owned(), "文档/Claude 代理".to_owned()],
            "the held pin stands"
        );

        drop(holder);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (§8 test 2, without its in-flight variant) — **Folio's own write is
    /// not a hand edit**, for every store: a landed write sets the revision to
    /// the bytes written and their document, so the re-read that write's own
    /// notice schedules answers `Unchanged`.
    ///
    /// MUTATION: do not set the revision on a landed write (drop the
    /// `revision.landed(..)` in any store's `write_now`) — the revision keeps the
    /// bytes it opened with, the bytes assertions fail, and the profiles re-read
    /// answers `Changed`.
    #[test]
    fn folios_own_write_is_not_a_hand_edit() {
        let root = scratch("own-write");

        let profiles_path = root.join(PROFILES_FILE_NAME);
        bt_persist::write_profiles_atomic(&profiles_path, &profiles("pwsh", "PowerShell 终端"))
            .unwrap();
        let mut profile_store = ProfilesStore::at(profiles_path.clone());
        let written = profiles("claude", "Claude 代理");
        assert!(profile_store.store(written.clone()));
        profile_store.settle();
        assert_eq!(
            bytes_of(profile_store.revision().bytes.as_ref()),
            Some(&file_bytes(&profiles_path)[..])
        );
        assert_eq!(profile_store.revision().document, written);
        assert_eq!(profile_store.reread(), Heard::Unchanged);
        assert_eq!(profile_store.loaded(), &written);

        let pins_path = root.join(PINS_FILE_NAME);
        let mut pin_store = PinsStore::at(pins_path.clone());
        assert!(pin_store.toggle(PinKind::Folder, "项目/folio 笔记"));
        pin_store.settle();
        assert_eq!(
            bytes_of(pin_store.revision().bytes.as_ref()),
            Some(&file_bytes(&pins_path)[..])
        );
        assert_eq!(pin_store.revision().document, pins(&["项目/folio 笔记"]));
        assert_eq!(pin_store.reread(), Heard::Unchanged);

        let settings_path = root.join(SETTINGS_FILE_NAME);
        let mut settings_store = SettingsStore::at(settings_path.clone());
        let settings = SettingsV1 {
            terminal_font_family: "更纱黑体 Sarasa Mono SC".to_owned(),
            ..SettingsV1::default()
        };
        assert!(settings_store.store(settings.clone()));
        settings_store.settle();
        assert_eq!(
            bytes_of(settings_store.revision().bytes.as_ref()),
            Some(&file_bytes(&settings_path)[..])
        );
        assert_eq!(settings_store.revision().document, settings);

        let keybindings_path = root.join(KEYBINDINGS_FILE_NAME);
        let mut keybindings_store = KeybindingsStore::at(keybindings_path.clone());
        let overrides = vec![BindingOverrideV1 {
            action: "new-tab".to_owned(),
            chord: Some("Ctrl+Shift+Y".to_owned()),
        }];
        assert!(keybindings_store.store(overrides.clone()));
        keybindings_store.settle();
        assert_eq!(
            bytes_of(keybindings_store.revision().bytes.as_ref()),
            Some(&file_bytes(&keybindings_path)[..])
        );
        assert_eq!(keybindings_store.revision().document.bindings, overrides);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (§8 test 2's in-flight variant; carry-forward 4) — **Folio's own
    /// write still on its way is not a hand edit**: a re-read that finds the
    /// bytes of the write in flight — landed on disk, its answer not yet taken —
    /// answers no change and keeps nothing for a merge.
    ///
    /// MUTATION: compare only with the revision in
    /// `DocumentState::is_folios_own_write` — the in-flight bytes look like a
    /// hand edit made while a change is not on disk, and are kept for the merge.
    #[test]
    fn folios_own_write_in_flight_is_not_a_hand_edit() {
        let root = scratch("in-flight");
        let path = root.join(PINS_FILE_NAME);
        let (lane, jobs) = super::StorageLane::held();
        let mut store = PinsStore::at_on(path.clone(), lane);
        assert!(store.toggle(PinKind::Folder, "项目/folio 笔记"));
        let write = match jobs.try_recv() {
            Ok(super::StorageJob::Document(write)) => write,
            _ => panic!("the write is on the lane"),
        };

        let in_flight = bt_persist::StableRead::Bytes(std::sync::Arc::clone(&write.bytes));
        assert_eq!(
            store.document().hear_read(in_flight).heard(),
            Heard::Unchanged
        );
        assert!(
            store.document().merges().is_empty(),
            "nothing of Folio's own write waits for a merge"
        );
        assert_eq!(
            store.targets(PinKind::Folder),
            vec!["项目/folio 笔记".to_owned()]
        );
        drop(write);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)14-16) — **identical bytes, and a return to the old bytes, are no
    /// change.**
    ///
    /// The question a re-read asks is "is this the file Folio left?", and only
    /// the bytes answer it: an editor's autosave of an unmodified buffer gives a
    /// new modified time and the same bytes, and a file that moved away and back
    /// before the pass ran is the same file. It is sharpest on a file refused at
    /// start (half-typed, `项目/folio 笔记` mid-line): its bytes are the revision,
    /// so saving it again unedited is no change — not a second refusal and a
    /// second card.
    ///
    /// MUTATION: answer `is_folios_own_write` by comparing the file's modified
    /// time with the one recorded at open instead of the bytes — the identical
    /// save answers `Unreadable`.
    #[test]
    fn identical_bytes_and_a_return_to_old_bytes_are_no_change() {
        let root = scratch("same-bytes");
        let path = root.join(PROFILES_FILE_NAME);
        let half_typed = "{ \"schema_version\": 1, \"profiles\": [ { \"id\": \"项目/folio 笔记\"";
        std::fs::write(&path, half_typed).unwrap();
        let mut store = ProfilesStore::at(path.clone());
        assert!(store.take_fault().is_some(), "refused at start, and said");

        // Saved again, byte for byte.
        std::fs::write(&path, half_typed).unwrap();
        assert_eq!(store.reread(), Heard::Unchanged);

        // Moved to other bytes and back before the pass ran.
        std::fs::write(
            &path,
            "{ \"schema_version\": 1, \"profiles\": [ { \"id\": \"Claude 代理\"",
        )
        .unwrap();
        std::fs::write(&path, half_typed).unwrap();
        assert_eq!(store.reread(), Heard::Unchanged);

        // A finished document is a change, once; its own bytes again are not.
        let finished = profiles("项目/folio 笔记", "Claude 代理");
        bt_persist::write_profiles_atomic(&path, &finished).unwrap();
        assert_eq!(store.reread(), Heard::Changed);
        assert_eq!(store.loaded(), &finished);
        bt_persist::write_profiles_atomic(&path, &profiles("pwsh", "PowerShell 终端")).unwrap();
        bt_persist::write_profiles_atomic(&path, &finished).unwrap();
        assert_eq!(store.reread(), Heard::Unchanged);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (§3.1's example) — **a re-indented file is a hand edit that changes
    /// no field**: its bytes become the revision's and nothing else moves.
    ///
    /// MUTATION: in `Revision::adopt`, answer an equal document before taking
    /// the bytes — the answer is still `Unchanged`, but the revision keeps the
    /// old spelling and the bytes assertion fails.
    #[test]
    fn a_reindented_file_is_a_hand_edit_that_changes_no_field() {
        let root = scratch("reindented");
        let path = root.join(PINS_FILE_NAME);
        let table = pins(&["项目/folio 笔记", "文档/Claude 代理"]);
        bt_persist::write_pins_atomic(&path, &table).unwrap();
        let mut store = PinsStore::at(path.clone());

        let compact = serde_json::to_vec(&table).expect("the table serializes");
        assert_ne!(compact, file_bytes(&path), "a second spelling");
        std::fs::write(&path, &compact).unwrap();

        assert_eq!(store.reread(), Heard::Unchanged);
        assert_eq!(
            bytes_of(store.revision().bytes.as_ref()),
            Some(&compact[..]),
            "the new spelling is the revision"
        );
        assert_eq!(store.revision().document, table);
        assert_eq!(
            store.targets(PinKind::Folder),
            vec!["项目/folio 笔记".to_owned(), "文档/Claude 代理".to_owned()]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)10's `Refused { bytes }`; §1.4) — **a file refused at start is
    /// the revision, and the next write replaces it**: the revision holds the
    /// refused bytes beside the default document, the copy R4-3 keeps is beside
    /// the file, and a landed write makes the written bytes the revision.
    ///
    /// MUTATION: have `read_with_fallback_bytes` return no bytes for a refused
    /// document (`DocumentBytes::Unobtained` in place of `Refused`) — the
    /// revision opens with `None`.
    #[test]
    fn a_file_refused_at_start_is_the_revision_the_next_write_replaces() {
        let root = scratch("refused-at-start");

        let settings_path = root.join(SETTINGS_FILE_NAME);
        let settings_text = "{ \"schema_version\": 9, \"terminal_font_family\": \"更纱黑体";
        std::fs::write(&settings_path, settings_text).unwrap();
        let mut settings_store = SettingsStore::read_at(
            settings_path.clone(),
            is_writer_of_document(&settings_path),
            super::StorageLane::of_its_own(),
        );
        assert!(settings_store.take_fault().is_some());
        assert!(a_copy_is_kept(&root, SETTINGS_FILE_NAME));
        assert_eq!(
            bytes_of(settings_store.revision().bytes.as_ref()),
            Some(settings_text.as_bytes())
        );
        assert_eq!(settings_store.revision().document, SettingsV1::default());
        let chosen = SettingsV1 {
            terminal_font_family: "更纱黑体 Sarasa Mono SC".to_owned(),
            ..SettingsV1::default()
        };
        assert!(settings_store.store(chosen.clone()));
        settings_store.settle();
        assert_eq!(
            bytes_of(settings_store.revision().bytes.as_ref()),
            Some(&file_bytes(&settings_path)[..])
        );
        assert_eq!(settings_store.revision().document, chosen);

        let pins_path = root.join(PINS_FILE_NAME);
        let pins_text = "{ \"schema_version\": 1, \"pins\": [ { \"kind\": \"folder\", \"target\": \"项目/folio 笔记\"";
        std::fs::write(&pins_path, pins_text).unwrap();
        let mut pin_store = PinsStore::at(pins_path.clone());
        assert!(pin_store.take_fault().is_some());
        assert!(a_copy_is_kept(&root, PINS_FILE_NAME));
        assert_eq!(
            bytes_of(pin_store.revision().bytes.as_ref()),
            Some(pins_text.as_bytes())
        );
        assert_eq!(pin_store.revision().document, PinsV1::default());
        assert_eq!(
            pin_store.reread(),
            Heard::Unchanged,
            "the refused file, unedited, is no change"
        );
        assert!(pin_store.toggle(PinKind::Folder, "文档/Claude 代理"));
        pin_store.settle();
        assert_eq!(
            bytes_of(pin_store.revision().bytes.as_ref()),
            Some(&file_bytes(&pins_path)[..])
        );
        assert_eq!(pin_store.revision().document, pins(&["文档/Claude 代理"]));

        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The storage worker's request protocol, served on a real disk in a test's own folder, with a
/// crossing writer put exactly where the protocol says one can land
/// (`document_request::serve_document_between`), and the store half that takes its answers
/// (T-STORAGE-WORKER-REQUEST; design note 2026-10-11, (b)3, (b)4, (b)14, (c)1, (c)3, (d)1).
#[cfg(test)]
mod conditional_write_tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};

    use bt_persist::{SettingsV1, StableRead};
    use bt_platform::admission::WorkerCtx;

    use super::{
        SETTINGS_FILE_NAME, SettingsStore, StorageJob, StorageLane, is_writer_of_document,
    };
    use crate::document_request::{
        DocumentAnswer, DocumentJob, KeptFile, LinkPolicy, Mode, Outcome, Precondition, Refusal,
        Step, precondition, retired_by, serve_document, serve_document_between, settle_exchange,
        settle_restoring_exchange,
    };

    /// A private folder for one case, emptied on the way in.
    pub(super) fn scratch(case: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-worker-request-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        root
    }

    /// Run `body` on a worker the thread door started, and wait for its answer
    /// within the suite's patience (`lane::PATIENCE`): a worker that never
    /// finishes fails the test instead of hanging it. A panic in `body` is
    /// carried back and raised here.
    pub(super) fn on_a_worker<T: Send + 'static>(
        body: impl FnOnce(&WorkerCtx) -> T + Send + 'static,
    ) -> T {
        let (sent, answer) = mpsc::channel();
        bt_platform::spawn_at_priority(
            "bt-worker-request-test",
            bt_platform::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = sent.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || body(worker),
                )));
            },
        )
        .expect("the thread door starts a thread");
        match answer.recv_timeout(crate::lane::PATIENCE) {
            Ok(Ok(answer)) => answer,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => panic!("the worker did not finish within the suite's patience"),
        }
    }

    /// A settings document told apart by its font, in mixed script.
    pub(super) fn settings(face: &str) -> SettingsV1 {
        SettingsV1 {
            terminal_font_family: face.to_owned(),
            ..SettingsV1::default()
        }
    }

    pub(super) fn bytes(document: &SettingsV1) -> Vec<u8> {
        bt_persist::serialize_settings(document).expect("a settings document serializes")
    }

    pub(super) fn file_bytes(path: &Path) -> Vec<u8> {
        std::fs::read(path).expect("the document is on disk")
    }

    /// The names in `root`, sorted.
    pub(super) fn names_in(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(root)
            .expect("the scratch folder lists")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The next job on a held lane, which must be a document's write.
    pub(super) fn next_write(jobs: &super::HeldJobs) -> DocumentJob {
        match jobs.try_recv() {
            Ok(StorageJob::Document(job)) => job,
            Ok(_) => panic!("the next job is not a document's write"),
            Err(error) => panic!("no job is queued: {error:?}"),
        }
    }

    /// Serve one write on a worker, a crossing writer putting `crossing` under its name the moment
    /// the precondition has been read, when there is one.
    pub(super) fn serve_crossed(job: DocumentJob, crossing: Option<Vec<u8>>) {
        let path = job.which.clone();
        on_a_worker(move |worker| {
            serve_document_between(worker, &job, &mut |step| {
                if step == Step::Read
                    && let Some(crossing) = &crossing
                {
                    std::fs::write(&path, crossing).expect("the crossing writer writes");
                }
            });
        });
    }

    /// A settings store over `path` on a held lane, writing `Merge` requests.
    fn merging_store(path: &Path) -> (SettingsStore, super::HeldJobs) {
        let (lane, jobs) = StorageLane::held();
        let mut store =
            SettingsStore::read_at(path.to_path_buf(), is_writer_of_document(path), lane);
        store.document().sending(Mode::Merge);
        (store, jobs)
    }

    /// A job written by hand: `bytes` under `which` if it holds `expected`.
    fn job(
        which: &Path,
        expected: Option<&[u8]>,
        bytes: &[u8],
        mode: Mode,
        version: u64,
        kept: Vec<KeptFile>,
    ) -> (DocumentJob, mpsc::Receiver<DocumentAnswer>) {
        let (answer, answered) = mpsc::channel();
        (
            DocumentJob {
                which: which.to_path_buf(),
                generation: 1,
                version,
                expected: expected.map(Arc::from),
                bytes: Arc::from(bytes),
                mode,
                link_policy: LinkPolicy::Refuse,
                no_exchange: crate::document_request::NoExchange::Refuse,
                asker: 7,
                kept,
                answer,
            },
            answered,
        )
    }

    /// RED ((b)14-4, (c)1) — **a displaced file merges against the pre-exchange revision, and
    /// the next write is conditional on what the exchange wrote.**
    ///
    /// Folio writes its font family; a hand edit of the font size lands between the precondition
    /// read and the exchange; a press of the theme arrives while the write is on its way. The
    /// exchange puts Folio's document on disk and hands the person's bytes back, kept beside it.
    /// The merge's base is the revision the write was conditional on, its `O` the document
    /// written, its `T` the person's: it keeps both, names no conflict, and lays the theme press
    /// over the result. The revision is what the target holds — the document written — so the
    /// merged document is sent conditional on those bytes.
    ///
    /// MUTATION: in `DocumentState::on_document`'s `Displaced` arm, set the revision to `theirs`
    /// (revision (b)'s text) — the revision assertion fails, and the next write would be
    /// conditional on bytes the file does not hold.
    #[test]
    fn displaced_merges_against_the_pre_exchange_revision() {
        let root = scratch("displaced-base");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);

        let ours = settings("我们的 Ours Mono");
        assert!(store.store(ours.clone()));
        let write = next_write(&jobs);
        let pressed = SettingsV1 {
            theme_mode: bt_persist::ThemeModeV1::Light,
            ..ours.clone()
        };
        assert!(store.store(pressed));
        let theirs = SettingsV1 {
            terminal_font_size: 19,
            ..base.clone()
        };
        serve_crossed(write, Some(bytes(&theirs)));
        store.settle();
        assert!(jobs.try_recv().is_err(), "nothing is sent before the merge");

        let merges = store.document().merges().to_vec();
        assert_eq!(merges.len(), 1, "the crossing edit is stored for the merge");
        assert_eq!(
            merges[0].base.document, base,
            "against the pre-exchange revision"
        );
        let kept = merges[0]
            .kept
            .clone()
            .expect("the person's bytes are kept beside the file");
        assert_eq!(
            file_bytes(&kept),
            bytes(&theirs),
            "and nothing of them is lost"
        );
        assert_eq!(
            file_bytes(&path),
            bytes(&ours),
            "Folio's document is on disk"
        );
        assert_eq!(
            store.revision().bytes.as_deref(),
            Some(&bytes(&ours)[..]),
            "and the revision is what the file holds"
        );

        let pass = store
            .begin_merge(bt_platform::HostPlatform::Windows)
            .expect("the displaced file is merged");
        store.end_merge();
        assert!(pass.account.conflicts.is_empty(), "no conflict is named");
        let merged = SettingsV1 {
            terminal_font_size: 19,
            theme_mode: bt_persist::ThemeModeV1::Light,
            ..ours.clone()
        };
        assert_eq!(
            store.document().desired(),
            &merged,
            "both edits and the press"
        );
        let next = next_write(&jobs);
        assert_eq!(
            next.expected.as_deref(),
            Some(&bytes(&ours)[..]),
            "conditional on the document written"
        );
        assert_eq!(&*next.bytes, &bytes(&merged)[..]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((c)1) — **a unit recovered from a displaced file survives a later `Moved`.**
    ///
    /// Folio writes its font family (A); a crossing edit sets the size (B), displaced and
    /// recovered into `desired`; before the merged document is written the person changes the
    /// theme in the file (C). The `Moved` merge is measured against the revision the target
    /// held — the document written — so B, which only Folio's `desired` holds now, is Folio's
    /// unit, and the merge keeps A, B and C, and lands them.
    ///
    /// MUTATION: revision (b)'s bookkeeping — in `DocumentState::on_document`'s `Displaced` arm,
    /// set the revision to `theirs` — the `Moved` merge sees B as unchanged against its base and
    /// the file's old size as the file's change: B is lost.
    #[test]
    fn a_unit_recovered_from_a_displaced_file_survives_a_later_moved() {
        let root = scratch("recovered-survives");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);
        let platform = bt_platform::HostPlatform::Windows;

        // A, crossed by B.
        let a = settings("甲 A Mono");
        assert!(store.store(a.clone()));
        let with_b = SettingsV1 {
            terminal_font_size: 21,
            ..base.clone()
        };
        serve_crossed(next_write(&jobs), Some(bytes(&with_b)));
        store.settle();
        store.begin_merge(platform).expect("Displaced is merged");
        store.end_merge();
        let a_b = SettingsV1 {
            terminal_font_size: 21,
            ..a.clone()
        };
        assert_eq!(store.document().desired(), &a_b);

        // C, on the file the target holds (A), before A+B is written.
        let write = next_write(&jobs);
        let a_c = SettingsV1 {
            theme_mode: bt_persist::ThemeModeV1::Dark,
            ..a.clone()
        };
        std::fs::write(&path, bytes(&a_c)).unwrap();
        serve_crossed(write, None);
        store.settle();
        let pass = store.begin_merge(platform).expect("Moved is merged");
        store.end_merge();
        let a_b_c = SettingsV1 {
            terminal_font_size: 21,
            theme_mode: bt_persist::ThemeModeV1::Dark,
            ..a.clone()
        };
        assert_eq!(store.document().desired(), &a_b_c, "A, B and C");
        assert!(pass.account.conflicts.is_empty());
        serve_crossed(next_write(&jobs), None);
        store.settle();
        assert_eq!(file_bytes(&path), bytes(&a_b_c), "and they land");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((c)1, (d)1) — **a kept file is removed only after a document derived from it
    /// lands.** The displaced copy stays on disk through the `Displaced` answer and the merge;
    /// the landing of the merged document removes it.
    ///
    /// MUTATION: remove `kept` on the `Displaced` answer (in `settle_exchange`'s `Merge` arm,
    /// remove the displaced file) — the copy is gone before the merged document lands.
    #[test]
    fn a_kept_file_is_removed_only_after_a_document_derived_from_it_lands() {
        let root = scratch("kept-derived");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);

        assert!(store.store(settings("我们的 Ours Mono")));
        let theirs = SettingsV1 {
            terminal_font_size: 18,
            ..base.clone()
        };
        serve_crossed(next_write(&jobs), Some(bytes(&theirs)));
        store.settle();
        let kept = store.document().merges()[0]
            .kept
            .clone()
            .expect("the person's bytes are kept");
        assert!(kept.is_file(), "the kept file is on disk after `Displaced`");
        store
            .begin_merge(bt_platform::HostPlatform::Windows)
            .expect("merged");
        store.end_merge();
        assert!(
            kept.is_file(),
            "and after the merge, until a document derived from it lands"
        );
        assert_eq!(store.document().kept().len(), 1, "one obligation");
        serve_crossed(next_write(&jobs), None);
        store.settle();
        assert!(!kept.exists(), "the derived document landed: the copy goes");
        assert!(store.document().kept().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (carry-forward 1) — **a held settings document is folded, and the next trigger writes
    /// conditionally.** A write answers `Moved`: the file's bytes are stored and the store is held
    /// — a press sends nothing. The merge folds the file in and releases the hold; the merged
    /// document and the next press are each sent conditional on the bytes the file holds.
    ///
    /// MUTATION: `DocumentState::fold` leaves the hold (drop its release) — nothing is queued
    /// after the merge.
    #[test]
    fn a_held_settings_document_is_folded_and_the_next_trigger_writes_conditionally() {
        let root = scratch("held-folded");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);

        assert!(store.store(settings("我们的 Ours Mono")));
        let write = next_write(&jobs);
        let theirs = SettingsV1 {
            terminal_font_size: 20,
            ..base.clone()
        };
        std::fs::write(&path, bytes(&theirs)).unwrap();
        serve_crossed(write, None);
        store.settle();
        assert!(store.store(settings("再按 Pressed Mono")));
        assert!(
            jobs.try_recv().is_err(),
            "held: nothing is written over the file"
        );

        store
            .begin_merge(bt_platform::HostPlatform::Windows)
            .expect("the moved file is merged");
        store.end_merge();
        let sent = next_write(&jobs);
        assert_eq!(
            sent.expected.as_deref(),
            Some(&bytes(&theirs)[..]),
            "the merged document, conditional on the file"
        );
        assert_eq!(
            &*sent.bytes,
            &bytes(&SettingsV1 {
                terminal_font_size: 20,
                ..settings("再按 Pressed Mono")
            })[..]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (round 2, finding 1) — **an unparseable displaced copy releases the hold and keeps
    /// the file.** A hand edit that will not parse crosses Folio's write: Folio's document is on
    /// disk, the person's bytes are kept beside it. There is nothing to merge, so the pass releases
    /// the hold in the same step — `desired` unchanged, the revision the document written, the kept
    /// copy on disk with no obligation on it — and the next press writes, conditional on the
    /// document written.
    ///
    /// MUTATION: leave the hold (in `DocumentState::merge_stored`, only ever set
    /// `held_for_merge`, never clear it) — the press queues nothing.
    #[test]
    fn an_unparseable_displaced_copy_releases_the_hold_and_keeps_the_file() {
        let root = scratch("unparseable-displaced");
        let path = root.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, bytes(&settings("基线 Base Mono"))).unwrap();
        let (mut store, jobs) = merging_store(&path);

        let ours = settings("我们的 Ours Mono");
        assert!(store.store(ours.clone()));
        let broken = "{ \"theme_mode\": 半截".as_bytes().to_vec();
        serve_crossed(next_write(&jobs), Some(broken.clone()));
        store.settle();
        let kept = store.document().merges()[0]
            .kept
            .clone()
            .expect("the person's bytes are kept");

        assert!(
            store
                .begin_merge(bt_platform::HostPlatform::Windows)
                .is_none(),
            "nothing is merged"
        );
        assert_eq!(file_bytes(&kept), broken, "the copy stays, as it was");
        assert!(store.document().kept().is_empty(), "and nothing retires it");
        assert!(store.document().merges().is_empty());
        assert_eq!(store.document().desired(), &ours, "`desired` is unchanged");
        assert_eq!(
            store.revision().bytes.as_deref(),
            Some(&bytes(&ours)[..]),
            "the revision is the document written"
        );

        let pressed = settings("再按 Pressed Mono");
        assert!(store.store(pressed.clone()));
        let next = next_write(&jobs);
        assert_eq!(
            next.expected.as_deref(),
            Some(&bytes(&ours)[..]),
            "the next trigger writes, conditional on the document written"
        );
        assert_eq!(&*next.bytes, &bytes(&pressed)[..]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (round 2, finding 2; (c)1's `Moved` row) — **a re-read answered after a `Displaced`
    /// in the same turn merges against the document written.** Folio writes its font family; a
    /// crossing edit sets the size and is displaced; then, before the turn that hears both, the
    /// person reverts the family in the file Folio left. The re-read's merge is measured against
    /// the revision the `Displaced` moved to — the document written — so the revert is the file's
    /// change and wins, and the displaced size survives.
    ///
    /// MUTATION: measure a held re-read against the previous stored merge's base (the stale
    /// pre-exchange revision) — the revert looks like no change, and Folio's family stays.
    #[test]
    fn a_reread_after_displaced_merges_against_the_document_written() {
        let root = scratch("reread-after-displaced");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);

        let ours = settings("我们的 Ours Mono");
        assert!(store.store(ours.clone()));
        store.document().ask_read();
        let write = next_write(&jobs);
        let read = match jobs.try_recv() {
            Ok(StorageJob::Read(read)) => read,
            _ => panic!("the read is queued behind the write"),
        };
        let with_size = SettingsV1 {
            terminal_font_size: 19,
            ..base.clone()
        };
        serve_crossed(write, Some(bytes(&with_size)));
        // The person reverts the family in the file Folio left.
        let reverted = SettingsV1 {
            terminal_font_family: base.terminal_font_family.clone(),
            ..ours.clone()
        };
        std::fs::write(&path, bytes(&reverted)).unwrap();
        on_a_worker(move |worker| crate::document_request::serve_read(worker, &read));
        store.settle();

        let merges = store.document().merges().to_vec();
        assert_eq!(merges.len(), 2, "the displaced copy, then the re-read");
        assert_eq!(
            merges[0].base.document, base,
            "Displaced: the pre-exchange revision"
        );
        assert_eq!(
            merges[1].base.document, ours,
            "the re-read: the document written"
        );
        store
            .begin_merge(bt_platform::HostPlatform::Windows)
            .expect("both are merged");
        store.end_merge();
        assert_eq!(
            store.document().desired(),
            &SettingsV1 {
                terminal_font_size: 19,
                ..base.clone()
            },
            "the file's revert wins, and the displaced size survives"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORE-MERGE-KEYBINDINGS; ticket 6's hold) — **a held keybindings document is
    /// released by the first merge, and the next trigger writes conditionally.** A re-read
    /// finds a hand edit: it is stored for the merge, nothing is taken, and the store is
    /// held — a press sends nothing. The merge folds the file in, keeping the press, and
    /// releases the hold: the merged document is sent conditional on the person's bytes.
    ///
    /// MUTATION: the hold stays (in `DocumentState::merge_stored`, only ever set
    /// `held_for_merge`, never clear it) — nothing is queued after the merge.
    #[test]
    fn a_held_keybindings_document_is_released_by_the_first_merge() {
        let root = scratch("keybindings-held");
        let path = root.join(super::KEYBINDINGS_FILE_NAME);
        let line = |action: &str, chord: &str| bt_persist::BindingOverrideV1 {
            action: action.to_owned(),
            chord: Some(chord.to_owned()),
        };
        let platform = super::keybindings_platform();
        let document = |lines| {
            bt_persist::serialize_keybindings(&super::keybindings_document(lines, platform))
                .expect("a keybindings document serializes")
        };
        std::fs::write(&path, document(vec![line("new-tab", "Ctrl+j")])).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = super::KeybindingsStore::at_on(path.clone(), lane);
        let in_force = store.loaded().to_vec();

        let hand = document(vec![
            line("new-tab", "Ctrl+j"),
            line("close-pane", "Ctrl+k"),
        ]);
        std::fs::write(&path, &hand).unwrap();
        let news = store
            .document()
            .hear_read(StableRead::Bytes(Arc::from(&hand[..])));
        assert_eq!(
            news.unreadable,
            Some(false),
            "it read, and nothing is taken"
        );
        assert_eq!(store.loaded(), &in_force[..], "the table in force stays");
        assert_eq!(store.document().merges().len(), 1, "stored for the merge");
        assert!(store.store(vec![line("new-tab", "Ctrl+j"), line("quit", "Ctrl+u")]));
        assert!(
            jobs.try_recv().is_err(),
            "held: nothing is written over the file"
        );

        let pass = store
            .begin_merge(bt_platform::host_platform())
            .expect("the hand edit is merged");
        store.end_merge();
        assert_eq!(pass.about.account.took, 1, "the file's line");
        assert_eq!(pass.about.account.kept, 1, "the press");
        let sent = next_write(&jobs);
        assert_eq!(
            sent.expected.as_deref(),
            Some(&hand[..]),
            "the merged document, conditional on the person's file"
        );
        assert_eq!(
            bt_persist::parse_keybindings(&sent.bytes)
                .expect("the merged document parses")
                .bindings,
            vec![
                line("new-tab", "Ctrl+j"),
                line("close-pane", "Ctrl+k"),
                line("quit", "Ctrl+u"),
            ]
        );
        assert_eq!(
            file_bytes(&path),
            hand,
            "the person's file stands until it lands"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)14-5) — **a stale receipt with displaced bytes is never dropped** ((b)4): a read
    /// asked after the write makes the write's answer older than the newest request, and the
    /// answer is processed all the same — its displaced bytes and its kept file are the person's.
    ///
    /// MUTATION: filter receipts by generation (in `DocumentState::on_document`, return at once
    /// when the answer's generation is older than the store's newest request) — the displaced
    /// bytes are dropped and nothing is stored for the merge.
    #[test]
    fn a_stale_receipt_with_displaced_bytes_is_never_dropped() {
        let root = scratch("stale-receipt");
        let path = root.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, bytes(&settings("基线 Base Mono"))).unwrap();
        let (mut store, jobs) = merging_store(&path);

        assert!(store.store(settings("我们的 Ours Mono")));
        store.document().ask_read();
        let write = next_write(&jobs);
        let read = match jobs.try_recv() {
            Ok(StorageJob::Read(read)) => read,
            _ => panic!("the read is queued behind the write"),
        };
        assert!(
            read.generation > write.generation,
            "the write's answer is the older one"
        );
        serve_crossed(write, Some(bytes(&settings("他们的 Theirs Mono"))));
        on_a_worker(move |worker| crate::document_request::serve_read(worker, &read));
        store.settle();

        let merges = store.document().merges().to_vec();
        assert_eq!(merges.len(), 1, "the stale answer was processed");
        assert!(
            merges[0].kept.as_ref().is_some_and(|kept| kept.is_file()),
            "its kept file is on disk and recorded"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((c)3 item 1) — **`Restore` puts the displaced file back and answers a conflict.**
    ///
    /// A buffer with no merge — the preview, and the four stores until the merge tickets — must
    /// never leave Folio's document over a hand edit that crossed its write: the worker exchanges
    /// again, conditional on the file still holding what it wrote, so the person's file is back
    /// under its name, and nothing is left beside it. The asker comes back on the answer.
    ///
    /// MUTATION: answer `Conflict` without the second exchange (in `settle_exchange`, return
    /// `Outcome::Conflict { theirs, kept: Some(displaced) }` for `Restore`) — Folio's document stays
    /// on disk and the file assertion fails.
    #[test]
    fn restore_puts_the_displaced_file_back_and_answers_conflict() {
        let root = scratch("restore");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();
        let ours = bytes(&settings("我们的 Ours Mono"));
        let theirs = bytes(&settings("他们的 Theirs Mono — 手改"));

        let (job, answered) = job(&path, Some(&base), &ours, Mode::Restore, 1, Vec::new());
        serve_crossed(job, Some(theirs.clone()));
        let answer = answered
            .try_recv()
            .expect("the worker answered before it finished");

        assert_eq!(
            answer.outcome,
            Outcome::Conflict {
                theirs: Arc::from(&theirs[..]),
                kept: None,
            }
        );
        assert_eq!(answer.asker, 7, "the asker comes back on the answer");
        assert_eq!(
            file_bytes(&path),
            theirs,
            "the person's file is back under its name"
        );
        assert_eq!(
            names_in(&root),
            vec![SETTINGS_FILE_NAME.to_owned()],
            "and nothing is left beside it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)4) — **one document write in flight, and the pending slot keeps the newest.**
    ///
    /// Three presses while the first write is on the worker are one write on the lane; when its
    /// answer has been processed, the newest document — the third press — is sent, and the second
    /// never reaches the lane at all, so it has no receipt to lose.
    ///
    /// MUTATION: send every change at once (in `DocumentState::send_owed`, drop the in-flight
    /// check) — three writes are queued at once.
    #[test]
    fn one_document_write_in_flight_and_the_pending_slot_keeps_the_newest() {
        let root = scratch("pending-slot");
        let path = root.join(SETTINGS_FILE_NAME);
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::at_on(path.clone(), lane);

        for face in ["一 One Mono", "二 Two Mono", "三 Three Mono"] {
            assert!(store.store(settings(face)));
        }
        let first = next_write(&jobs);
        assert!(jobs.try_recv().is_err(), "one write is on the lane");
        assert_eq!(&*first.bytes, &bytes(&settings("一 One Mono"))[..]);

        serve_crossed(first, None);
        store.settle();
        let newest = next_write(&jobs);
        assert_eq!(
            &*newest.bytes,
            &bytes(&settings("三 Three Mono"))[..],
            "the pending slot held the newest press"
        );
        assert!(
            jobs.try_recv().is_err(),
            "and the middle one never reached the lane"
        );
        serve_crossed(newest, None);
        store.settle();
        assert_eq!(file_bytes(&path), bytes(&settings("三 Three Mono")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((c)3 item 5) — **per-file order holds across askers.**
    ///
    /// Two askers name one file — a settings store and a second one over the same path, standing
    /// in for a preview buffer open on `settings.json`. Their requests go down one queue in the
    /// order they were made, and the second sees the first's write as a file that moved.
    ///
    /// MUTATION: separate queues per store (give `SettingsStore::at_on` a lane of its own instead
    /// of the one it is handed) — the second write never reaches this queue.
    #[test]
    fn per_file_order_holds_across_askers() {
        let root = scratch("per-file-order");
        let path = root.join(SETTINGS_FILE_NAME);
        let (lane, jobs) = StorageLane::held();
        let mut first = SettingsStore::at_on(path.clone(), lane.clone());
        let mut second = SettingsStore::at_on(path.clone(), lane);

        assert!(first.store(settings("先 First Mono")));
        assert!(second.store(settings("后 Second Mono")));
        let a = next_write(&jobs);
        let b = next_write(&jobs);
        assert_eq!(
            &*a.bytes,
            &bytes(&settings("先 First Mono"))[..],
            "queue order"
        );
        assert_eq!(&*b.bytes, &bytes(&settings("后 Second Mono"))[..]);

        serve_crossed(a, None);
        serve_crossed(b, None);
        first.settle();
        second.settle();
        assert_eq!(file_bytes(&path), bytes(&settings("先 First Mono")));
        let merges = second.document().merges().to_vec();
        assert_eq!(
            merges.first().and_then(|merge| merge.theirs.as_deref()),
            Some(&bytes(&settings("先 First Mono"))[..]),
            "the second asker saw the first one's write, and wrote nothing over it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (carry-forward 3) — **a read that could not obtain the file's bytes never satisfies an
    /// absent precondition.** `expected: None` means *no file*; a file whose bytes were not read is
    /// a file, and Folio does not write over a file it has not read.
    ///
    /// Here the name holds something that opens and does not read (a folder). The worker answers a
    /// refusal and leaves it alone; the name-refusing rename of a first write is a second guard
    /// behind the decision, so the decision is also asked directly.
    ///
    /// MUTATION: treat `Unobtained` as absence (in `document_request::precondition`, answer
    /// `Holds` for `(Unobtained, None)`) — the decision assertion fails.
    #[test]
    fn an_unobtained_read_never_satisfies_an_absent_precondition() {
        assert_eq!(
            precondition(StableRead::Unobtained("拒绝 refused".to_owned()), None),
            Precondition::Unread("拒绝 refused".to_owned())
        );

        let root = scratch("unobtained");
        let path = root.join(SETTINGS_FILE_NAME);
        std::fs::create_dir_all(&path).unwrap();
        let (job, answered) = job(
            &path,
            None,
            &bytes(&settings("我们的 Ours Mono")),
            Mode::Restore,
            1,
            Vec::new(),
        );
        on_a_worker(move |worker| serve_document(worker, &job));
        let answer = answered
            .try_recv()
            .expect("the worker answered before it finished");
        assert!(
            matches!(
                answer.outcome,
                Outcome::Refused {
                    cause: Refusal::Unread(_)
                }
            ),
            "{:?}",
            answer.outcome
        );
        assert!(path.is_dir(), "what stands under the name is untouched");
        assert_eq!(names_in(&root), vec![SETTINGS_FILE_NAME.to_owned()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (carry-forward 5) — **a landing that is not known to be durable keeps the displaced
    /// path as an obligation**, removed by the same rule as a `Displaced` file's: a later durable
    /// landing.
    ///
    /// The worker half: an exchange whose directory flush failed keeps what it displaced, even
    /// when that is Folio's own last document, because it is the only copy known to be on the
    /// device. The store half: the answer is a success for the revision and an obligation for the
    /// kept file, which the next durable landing retires.
    ///
    /// MUTATION: remove the kept file on `NotDurable` (in `settle_exchange`, remove the displaced
    /// file whatever `not_durable` says) — the worker's assertion fails.
    #[test]
    fn not_durable_keeps_the_displaced_path_as_an_obligation() {
        let root = scratch("not-durable");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();

        // The worker half.
        let displaced = root.join("settings.json.displaced-测试");
        std::fs::write(&displaced, &base).unwrap();
        let (probe, _answered) = job(
            &path,
            Some(&base),
            &bytes(&settings("我们的 Ours Mono")),
            Mode::Restore,
            1,
            Vec::new(),
        );
        let kept_path = displaced.clone();
        let served = on_a_worker(move |worker| {
            settle_exchange(
                worker,
                &crate::document_request::TheVolume,
                &probe,
                &base,
                &kept_path,
                Some("the directory flush failed".to_owned()),
                &mut |_| {},
            )
        });
        assert_eq!(
            served.outcome,
            Outcome::Landed {
                bytes: Arc::from(&bytes(&settings("我们的 Ours Mono"))[..]),
                kept: Some(displaced.clone()),
                ordinary_on: None,
            }
        );
        assert!(displaced.is_file(), "the displaced copy stays");

        // The store half.
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::read_at(path.clone(), is_writer_of_document(&path), lane);
        assert!(store.store(settings("我们的 Ours Mono")));
        let write = next_write(&jobs);
        std::fs::write(&path, &*write.bytes).unwrap();
        let _ = write.answer.send(DocumentAnswer {
            generation: write.generation,
            version: write.version,
            asker: write.asker,
            outcome: Outcome::Landed {
                bytes: Arc::clone(&write.bytes),
                kept: Some(displaced.clone()),
                ordinary_on: None,
            },
            not_durable: Some("the directory flush failed".to_owned()),
            retired: Vec::new(),
            left: None,
        });
        drop(write);
        store.settle();
        let next = store.document().version() + 1;
        assert_eq!(
            store.document().kept(),
            &[KeptFile {
                path: displaced.clone(),
                version: next,
            }][..],
            "an obligation, retired by a later landing"
        );
        assert!(displaced.is_file());
        assert!(store.store(settings("再一次 Again Mono")));
        serve_crossed(next_write(&jobs), None);
        store.settle();
        assert!(!displaced.exists(), "a later durable landing removes it");
        assert!(store.document().kept().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((d)1) — **kept files survive `Displaced` → `Moved` → `Displaced` until a derived
    /// version lands**: the review's sequence, on a real disk. Both kept files stay through the
    /// three answers, and one landing of a version at or after both merges removes both.
    ///
    /// The merges are this test's (T-STORE-MERGE-SETTINGS builds them): each folds the file's
    /// size into Folio's document, which is what makes each new version derive from the kept
    /// file's units.
    ///
    /// MUTATION: revision (c)'s rule — a landing retires only the kept file of its own write's
    /// base, written here as `document_request::retired_by` matching `file.version == landed` —
    /// `kept₁` is never removed and the last assertion fails.
    #[test]
    fn kept_files_survive_displaced_moved_displaced_until_a_derived_version_lands() {
        let root = scratch("kept-sequence");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (mut store, jobs) = merging_store(&path);
        let sized = |face: &str, size: u8| SettingsV1 {
            terminal_font_size: size,
            ..settings(face)
        };

        // 1. Displaced₁.
        assert!(store.store(settings("我们的 Ours Mono")));
        serve_crossed(next_write(&jobs), Some(bytes(&sized("基线 Base Mono", 15))));
        store.settle();
        let first = store.document().take_merges();
        let kept_1 = first[0]
            .kept
            .clone()
            .expect("Displaced₁ keeps the person's bytes");
        for merge in first {
            let theirs = sized("基线 Base Mono", 15);
            store
                .document()
                .fold(merge, sized("我们的 Ours Mono", 15), theirs);
        }
        let m_1 = store.document().version();

        // 2. Moved: the file changes before the next write is served.
        store.document().send_owed();
        let second = next_write(&jobs);
        std::fs::write(&path, bytes(&sized("手改 Hand Mono", 16))).unwrap();
        serve_crossed(second, None);
        store.settle();
        for merge in store.document().take_merges() {
            assert!(merge.kept.is_none(), "a moved file keeps nothing beside it");
            store.document().fold(
                merge,
                sized("我们的 Ours Mono", 16),
                sized("手改 Hand Mono", 16),
            );
        }
        assert!(kept_1.is_file(), "Moved removes no kept file");

        // 3. Displaced₂.
        store.document().send_owed();
        serve_crossed(
            next_write(&jobs),
            Some(bytes(&sized("我们的 Ours Mono", 17))),
        );
        store.settle();
        let third = store.document().take_merges();
        let kept_2 = third[0]
            .kept
            .clone()
            .expect("Displaced₂ keeps the person's bytes");
        for merge in third {
            store.document().fold(
                merge,
                sized("我们的 Ours Mono — 合并", 17),
                sized("我们的 Ours Mono", 17),
            );
        }
        let m_2 = store.document().version();
        assert!(m_2 > m_1);
        assert!(
            kept_1.is_file() && kept_2.is_file(),
            "both survive three answers"
        );

        // 4. A landing of a version at or after both.
        store.document().send_owed();
        let last = next_write(&jobs);
        assert!(last.version >= m_2);
        serve_crossed(last, None);
        store.settle();
        assert!(!kept_2.exists(), "the newer kept file is retired");
        assert!(
            !kept_1.exists(),
            "and the older one, which the landed version derives from"
        );
        assert!(store.document().kept().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((d)1) — **a landing of an older version removes only the kept files it derives
    /// from**: with obligations at versions 3 and 5, a landing of version 4 retires the first and
    /// leaves the second.
    ///
    /// MUTATION: on any `Landed`, remove every obligation (`retired_by` answers all of `kept`) —
    /// the second file is removed too.
    #[test]
    fn a_landed_older_version_removes_only_the_kept_files_it_derives_from() {
        let root = scratch("kept-older");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();
        let older = root.join("settings.json.displaced-旧");
        let newer = root.join("settings.json.displaced-新");
        std::fs::write(&older, "旧 old").unwrap();
        std::fs::write(&newer, "新 new").unwrap();
        let kept = vec![
            KeptFile {
                path: older.clone(),
                version: 3,
            },
            KeptFile {
                path: newer.clone(),
                version: 5,
            },
        ];
        assert_eq!(
            retired_by(&kept, 4)
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec![older.clone()]
        );

        let (job, answered) = job(
            &path,
            Some(&base),
            &bytes(&settings("我们的 Ours Mono")),
            Mode::Merge,
            4,
            kept,
        );
        on_a_worker(move |worker| serve_document(worker, &job));
        let answer = answered
            .try_recv()
            .expect("the worker answered before it finished");
        assert!(matches!(answer.outcome, Outcome::Landed { .. }));
        assert_eq!(answer.retired, vec![older.clone()]);
        assert!(
            !older.exists(),
            "the file version 4 derives from is retired"
        );
        assert!(newer.is_file(), "the one merged after it stays");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((d)1, rule 5) — **no answer but a landing removes a kept file**: not `Moved`, not
    /// `Displaced`, not `Refused`, each for a version past the kept file's.
    ///
    /// MUTATION: remove `kept` on the `Displaced` answer (in `serve_document_between`, retire on
    /// `Outcome::Displaced` too) — the kept file is gone after the second answer.
    #[test]
    fn no_answer_but_landed_removes_a_kept_file() {
        let root = scratch("kept-only-landed");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();
        let kept_path = root.join("settings.json.displaced-保留");
        std::fs::write(&kept_path, "保留 kept").unwrap();
        let kept = || {
            vec![KeptFile {
                path: kept_path.clone(),
                version: 1,
            }]
        };
        let ours = bytes(&settings("我们的 Ours Mono"));

        // Moved: the file is not what the write expects.
        let (moved, answered) = job(&path, Some(b"not this"), &ours, Mode::Merge, 5, kept());
        on_a_worker(move |worker| serve_document(worker, &moved));
        assert!(matches!(
            answered.try_recv().unwrap().outcome,
            Outcome::Moved { .. }
        ));
        assert!(kept_path.is_file(), "Moved");

        // Displaced: a hand edit crosses the exchange.
        let (displaced, answered) = job(&path, Some(&base), &ours, Mode::Merge, 5, kept());
        serve_crossed(displaced, Some(bytes(&settings("手改 Hand Mono"))));
        let answer = answered.try_recv().unwrap();
        assert!(matches!(answer.outcome, Outcome::Displaced { .. }));
        assert!(kept_path.is_file(), "Displaced");

        // Refused: the name holds a folder.
        let folder = root.join("folder.json");
        std::fs::create_dir_all(&folder).unwrap();
        let (refused, answered) = job(&folder, None, &ours, Mode::Merge, 5, kept());
        on_a_worker(move |worker| serve_document(worker, &refused));
        assert!(matches!(
            answered.try_recv().unwrap().outcome,
            Outcome::Refused { .. }
        ));
        assert!(kept_path.is_file(), "Refused");

        // Landed.
        let (landed, answered) = job(&path, Some(&ours), &base, Mode::Merge, 5, kept());
        on_a_worker(move |worker| serve_document(worker, &landed));
        assert!(matches!(
            answered.try_recv().unwrap().outcome,
            Outcome::Landed { .. }
        ));
        assert!(!kept_path.exists(), "only a landing retires it");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (review round 2, blocker 1; (c)3) — **a `Restore` conflict records
    /// the person's file as the revision and holds the owed write.** After the
    /// crossing edit is put back, the revision is what the file holds; a later
    /// press moves `desired` and sends nothing, so the hand edit is not written
    /// over before the merge.
    ///
    /// MUTATIONS: keep the old revision on `Conflict` — the revision assertion
    /// fails; send after the press (drop `held_for_merge` from
    /// `DocumentState::send_owed`) — a write is queued.
    #[test]
    fn a_restore_conflict_records_theirs_and_holds_the_owed_write() {
        let root = scratch("restore-held");
        let path = root.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, bytes(&settings("基线 Base Mono"))).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::read_at(path.clone(), is_writer_of_document(&path), lane);
        // The `Restore` policy the three other stores still send, carried on a settings document.
        store.document().sending(Mode::Restore);

        assert!(store.store(settings("我们的 Ours Mono")));
        let theirs = bytes(&settings("他们的 Theirs Mono — 手改"));
        serve_crossed(next_write(&jobs), Some(theirs.clone()));
        store.settle();
        assert_eq!(
            store.revision().bytes.as_deref(),
            Some(&theirs[..]),
            "the revision is the file the person now has"
        );
        assert_eq!(
            store.revision().document,
            settings("他们的 Theirs Mono — 手改")
        );

        assert!(store.store(settings("再按一次 Pressed Again Mono")));
        store.release_trial();
        assert!(
            jobs.try_recv().is_err(),
            "nothing is written over the hand edit"
        );
        assert_eq!(file_bytes(&path), theirs, "the file still holds it");
        assert_eq!(
            store.loaded(),
            &settings("再按一次 Pressed Again Mono"),
            "and the press is in force in memory"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (review round 3, finding 1) — **a re-read while held keeps the hold**:
    /// only the merge releases it. The person's file read back is nothing; the
    /// file changed again is the new revision and is stored for the merge after
    /// the conflict's, against the revision as it stood (T-STORE-MERGE-SETTINGS
    /// round 2: a `Moved` or re-read merge's base is always the current
    /// revision); and still nothing is written.
    ///
    /// MUTATION: let a re-read release the hold (put `self.held_for_merge = false`
    /// back in `DocumentState::on_read`'s taking branch and let a held read take
    /// the file) — a write is queued after the pass.
    #[test]
    fn a_reread_while_held_keeps_the_hold() {
        let root = scratch("reread-held");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = settings("基线 Base Mono");
        std::fs::write(&path, bytes(&base)).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::read_at(path.clone(), is_writer_of_document(&path), lane);
        // The `Restore` policy the three other stores still send, carried on a settings document.
        store.document().sending(Mode::Restore);
        assert!(store.store(settings("我们的 Ours Mono")));
        let theirs = bytes(&settings("他们的 Theirs Mono"));
        serve_crossed(next_write(&jobs), Some(theirs.clone()));
        store.settle();

        // The person's file, read back: nothing.
        let news = store
            .document()
            .hear_read(StableRead::Bytes(Arc::from(&theirs[..])));
        assert_eq!(news.unreadable, Some(false));
        // A press that returns `desired` to the person's document, then the file
        // changed again: the revision follows the file, the hold stays.
        assert!(store.store(settings("他们的 Theirs Mono")));
        let again = bytes(&settings("又改了 Edited Again Mono"));
        std::fs::write(&path, &again).unwrap();
        let news = store
            .document()
            .hear_read(StableRead::Bytes(Arc::from(&again[..])));
        assert_eq!(
            news.unreadable,
            Some(false),
            "a held document takes nothing from a re-read"
        );
        assert_eq!(store.revision().bytes.as_deref(), Some(&again[..]));
        let merges = store.document().merges().to_vec();
        assert_eq!(merges.len(), 2, "the conflict's file, then the newest");
        assert_eq!(
            merges[0].base.document, base,
            "the conflict, against its own base"
        );
        assert_eq!(
            merges[1].theirs.as_deref(),
            Some(&again[..]),
            "the newest file"
        );
        assert_eq!(
            merges[1].base.document,
            settings("他们的 Theirs Mono"),
            "against the revision it was read over"
        );

        assert!(store.store(settings("再按 Pressed Mono")));
        store.release_trial();
        assert!(
            jobs.try_recv().is_err(),
            "nothing is written before the merge"
        );
        assert_eq!(file_bytes(&path), again);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// End the storage worker — a session write whose receipt nobody hears —
    /// and give its end the suite's patience to run to its wake; a wake heard
    /// here is a wake that came while the request was still being admitted.
    fn end_the_worker_and_give_it_its_chance(
        poison: &mpsc::Sender<super::Queued>,
        session: PathBuf,
        wakes: &mpsc::Receiver<bool>,
    ) {
        // Put on the queue past its door — the door's lock is the admission's, held now — under
        // an identity the ledger never minted, whose publication it refuses.
        poison
            .send(super::Queued::Job(
                super::StorageId::from(u64::MAX),
                Box::new(StorageJob::Session(super::SessionWriteRequest {
                    generation: 1,
                    path: session,
                    bytes: b"{}".to_vec(),
                })),
            ))
            .expect("the worker takes it");
        if let Ok(gone) = wakes.recv_timeout(crate::lane::PATIENCE) {
            panic!(
                "the worker's end woke the loop during an admission (terminal published: {gone})"
            );
        }
    }

    /// The next wake within the suite's patience, and whether the request's
    /// terminal was published when it came.
    fn next_wake(wakes: &mpsc::Receiver<bool>) -> bool {
        wakes
            .recv_timeout(crate::lane::PATIENCE)
            .expect("the worker's end wakes the loop")
    }

    /// RED (review round 3, finding 2; C2) — **a request that races the worker's
    /// end gets exactly one terminal, and the wake follows it.** The worker is
    /// made to end at the moment between the admission's decision and the
    /// enqueue; because admission is decided under the queue's lock and the
    /// worker's end takes that lock to mark the lane down and drop what is
    /// queued, the request is either refused at the door or queued and then
    /// dropped by the end — and the end's wake comes after.
    ///
    /// MUTATION: decide admission before taking the queue's lock (load `up`
    /// first, no recheck under the lock) — the end runs to its wake between the
    /// decision and the enqueue, and the request is then refused at the door
    /// after an admission was decided (or queued after the wake).
    #[test]
    fn a_request_racing_the_workers_end_gets_one_terminal_and_a_wake_after_it() {
        let root = scratch("end-race");
        let path = root.join(SETTINGS_FILE_NAME);
        let (answer, answered) = mpsc::channel::<DocumentAnswer>();
        let answered = Arc::new(std::sync::Mutex::new(answered));
        let (woke, wakes) = mpsc::channel::<bool>();
        let seen = Arc::clone(&answered);
        let lane = StorageLane::with_a_worker_and_wake(move || {
            // At the wake: was this request's terminal already published?
            let gone = matches!(
                seen.lock().expect("the answer").try_recv(),
                Err(mpsc::TryRecvError::Disconnected)
            );
            let _ = woke.send(gone);
        });
        let poison = super::lane_lock(&lane.shared.requests)
            .clone()
            .expect("the lane is open");
        let (mut probe, _unused) = job(
            &path,
            None,
            &bytes(&settings("我们的 Ours Mono")),
            Mode::Restore,
            1,
            Vec::new(),
        );
        probe.answer = answer;
        let session = root.join("session.json");
        let admitted = lane.admit_document_job_between(StorageJob::Document(probe), || {
            end_the_worker_and_give_it_its_chance(&poison, session, &wakes);
        });
        let request = admitted.expect("admitted, under the lock, before the end");
        assert!(
            next_wake(&wakes),
            "the wake came after the request's terminal"
        );
        let mut heard = super::OnTheWorker::admitted(mpsc::channel::<()>().1, request);
        assert!(
            matches!(heard.hear(), super::Reply::Gone),
            "and the ledger ended it WorkerGone, once"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
    /// known to be durable keeps what it displaced**: the person's file is back
    /// under its name, and the displaced copy — Folio's own write — stays as an
    /// obligation, never deleted.
    ///
    /// MUTATION: delete the displaced copy on `NotDurable` (in
    /// `settle_restoring_exchange`, remove it whenever it holds Folio's bytes) —
    /// the copy is gone.
    #[test]
    fn restore_not_durable_keeps_the_displaced_path() {
        let root = scratch("restore-not-durable");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        let ours = bytes(&settings("我们的 Ours Mono"));
        let theirs = bytes(&settings("他们的 Theirs Mono"));
        std::fs::write(&path, &theirs).unwrap();
        let back = root.join("settings.json.displaced-回来");
        std::fs::write(&back, &ours).unwrap();
        let (probe, _answered) = job(&path, Some(&base), &ours, Mode::Restore, 3, Vec::new());
        let back_path = back.clone();
        let theirs_bytes: Arc<[u8]> = Arc::from(&theirs[..]);
        let served = on_a_worker(move |worker| {
            settle_restoring_exchange(
                worker,
                &probe,
                theirs_bytes,
                Err(bt_platform::ExchangeRefusal::NotDurable {
                    displaced: bt_platform::Displaced { path: back_path },
                    error: std::io::Error::other("the directory flush failed"),
                }),
            )
        });
        assert_eq!(
            served.outcome,
            Outcome::Conflict {
                theirs: Arc::from(&theirs[..]),
                kept: Some(back.clone()),
            }
        );
        assert!(
            served.not_durable.is_some(),
            "said as not known to be durable"
        );
        assert!(back.is_file(), "the displaced copy stays");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (review round 2, blocker 2; C2) — **a request no worker takes ends
    /// in a refusal, and the loop is woken for it**, so the store hears it the
    /// way it hears the worker, and the fifth in a row is a card.
    ///
    /// MUTATION: drop the wake (`self.lane.wake()` in `DocumentState::send_owed`)
    /// — no wake is heard.
    #[test]
    fn a_refused_spawn_is_a_refused_terminal_and_a_wake() {
        let root = scratch("refused-spawn");
        let path = root.join(SETTINGS_FILE_NAME);
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let heard = Arc::clone(&wakes);
        let lane = StorageLane::without_a_worker(move || {
            heard.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let mut store = SettingsStore::at_on(path.clone(), lane);
        // The same row chosen five times: a refusal is retried by the next press,
        // and the streak runs until a write lands.
        for press in 0..5 {
            assert_eq!(store.store(settings("选五次 Five Times Mono")), press == 0);
            assert_eq!(
                wakes.load(std::sync::atomic::Ordering::SeqCst),
                press + 1,
                "each refusal wakes the loop"
            );
            store.take_answers();
        }
        assert_eq!(
            store.take_fault(),
            Some(crate::i18n::persisted_file_unwritable(SETTINGS_FILE_NAME)),
            "the fifth refusal in a row is the card's sentence"
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (review round 2, blocker 3; C5) — **a refused write is sent once
    /// more by the next ear pass**, and only once: the pass is the trigger, not a
    /// clock.
    ///
    /// MUTATION: no resend on the ear (drop `ear_retry` in
    /// `DocumentState::after_answers`) — nothing is queued after the pass.
    #[test]
    fn a_refused_write_is_sent_once_more_by_the_next_ear_pass() {
        let root = scratch("ear-retry");
        let path = root.join(SETTINGS_FILE_NAME);
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::at_on(path.clone(), lane);

        assert!(store.store(settings("我们的 Ours Mono")));
        let write = next_write(&jobs);
        let _ = write.answer.send(DocumentAnswer {
            generation: write.generation,
            version: write.version,
            asker: write.asker,
            outcome: Outcome::Refused {
                cause: Refusal::BackupNameTaken(root.join("settings.json.displaced-占用")),
            },
            not_durable: None,
            retired: Vec::new(),
            left: None,
        });
        drop(write);
        store.settle();
        assert!(
            jobs.try_recv().is_err(),
            "a refusal is not retried on its own"
        );

        store.document().ask_read();
        let read = match jobs.try_recv() {
            Ok(StorageJob::Read(read)) => read,
            _ => panic!("the pass's read is queued"),
        };
        on_a_worker(move |worker| crate::document_request::serve_read(worker, &read));
        store.settle();
        let again = next_write(&jobs);
        assert_eq!(&*again.bytes, &bytes(&settings("我们的 Ours Mono"))[..]);
        assert!(jobs.try_recv().is_err(), "exactly once");
        drop(again);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 2, nit c; `docs/M2-persistence-schema-v1.md`
    /// §5.3) — **a streak of changed presses is said at its fifth refusal**: a changed value
    /// does not start the count again, only a landing ends it.
    ///
    /// MUTATION: start the count again on a changed press (`self.writes.streak = 0` in
    /// `DocumentState::store`'s `changed` arm, the old `rearm`) — five changed presses are never
    /// said.
    #[test]
    fn a_streak_of_changed_presses_is_said_at_the_fifth_refusal() {
        let root = scratch("changed-presses-streak");
        let path = root.join(SETTINGS_FILE_NAME);
        let mut store = SettingsStore::at_on(path.clone(), StorageLane::without_a_worker(|| {}));
        let faces = [
            "宋体 Song Mono",
            "黑体 Hei Mono",
            "楷体 Kai Mono",
            "仿宋 FangSong Mono",
            "圆体 Yuan Mono",
        ];
        for (press, face) in faces.iter().enumerate() {
            assert!(store.store(settings(face)), "each press changes the value");
            store.take_answers();
            let said = store.take_fault();
            if press + 1 < faces.len() {
                assert_eq!(said, None, "not yet, at press {}", press + 1);
            } else {
                assert_eq!(
                    said,
                    Some(crate::i18n::persisted_file_unwritable(SETTINGS_FILE_NAME)),
                    "the fifth refusal in a row is said, though every press changed the value"
                );
            }
        }
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-SESSION-WRITER-NO-TIMER-RETRY round 2, nit c) — **a refusal said at once is said
    /// once per streak**: a link refused on one press is a card; the same refusal on the next
    /// press is not a second one.
    ///
    /// MUTATION: `DocumentWrites::refused_for_good` says every time (owe the sentence without
    /// asking `said`) — the second press owes a second card.
    #[test]
    fn a_link_refused_on_every_press_is_said_once_per_streak() {
        let root = scratch("link-said-once");
        let path = root.join(SETTINGS_FILE_NAME);
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::at_on(path.clone(), lane);
        let refuse_as_a_link = |jobs: &super::HeldJobs| {
            let write = next_write(jobs);
            let _ = write.answer.send(DocumentAnswer {
                generation: write.generation,
                version: write.version,
                asker: write.asker,
                outcome: Outcome::Refused {
                    cause: Refusal::Link,
                },
                not_durable: None,
                retired: Vec::new(),
                left: None,
            });
        };

        assert!(store.store(settings("链接 Linked Mono")));
        refuse_as_a_link(&jobs);
        store.settle();
        assert_eq!(
            store.take_fault(),
            Some(crate::i18n::persisted_file_unwritable(SETTINGS_FILE_NAME)),
            "said at once"
        );

        assert!(
            !store.store(settings("链接 Linked Mono")),
            "the same row again: owed, so sent again"
        );
        refuse_as_a_link(&jobs);
        store.settle();
        assert_eq!(store.take_fault(), None, "and not said twice in one streak");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-PREVIEW-NO-EXCHANGE-RULING, owner ruling 2026-10-15) — **a configuration store on
    /// a drive that cannot exchange still refuses.** The preview's save writes plainly on such a
    /// drive; `settings.json` does not: the store's own request asks for the refusal, and served
    /// on a drive whose exchange answers `ERROR_NOT_SUPPORTED` (50) nothing is written, nothing is
    /// left beside the file, and the store says at once that the file cannot be written.
    ///
    /// MUTATION: the stores' request with `NoExchange::PlainWriter` (`DocumentState::send_owed`)
    /// — the file holds the change and no fault is said.
    #[test]
    fn a_configuration_store_on_a_drive_that_cannot_exchange_still_refuses() {
        let root = scratch("store-no-exchange");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::read_at(path.clone(), is_writer_of_document(&path), lane);

        let _ = store.store(settings("我们的 Ours Mono"));
        let write = next_write(&jobs);
        assert_eq!(
            write.expected.as_deref(),
            Some(&base[..]),
            "a write over the file, which only an exchange replaces"
        );
        on_a_worker(move |worker| {
            crate::document_request::serve_document_on(
                worker,
                &crate::test_support::NoExchangeDrive::new(7),
                &write,
                &mut |_| {},
            );
        });
        store.settle();

        assert_eq!(file_bytes(&path), base, "nothing was written");
        assert_eq!(
            names_in(&root),
            [SETTINGS_FILE_NAME],
            "nothing is left beside it"
        );
        assert_eq!(
            store.take_fault(),
            Some(crate::i18n::persisted_file_unwritable(SETTINGS_FILE_NAME)),
            "the refusal is said at once"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-PREVIEW-NO-EXCHANGE-RULING, stacked re-check) — **a preview save on a drive that
    /// cannot exchange lands plainly through the storage lane itself, and says so once.** The
    /// lane is the product's (`StorageLane::fresh_with` on the thread door, its worker started by
    /// `SessionWriter::start`): the press is admitted by the lane's ledger and served inside the
    /// request's `lane::task`, through the suite's door (`TestDoor`), which serves a document's
    /// write exactly as `serve_storage_job` does but on a drive whose exchange answers
    /// `ERROR_NOT_SUPPORTED` (50) (`test_support::NoExchangeDrive`). Its answer comes back on the
    /// request's own road after the wake: the file holds the body, the copy the exchange was
    /// handed is the copy the rename put in place, and the pane hears the sentence on the first
    /// save and "Saved" on the second.
    ///
    /// MUTATION: `preview::ON_A_VOLUME_THAT_CANNOT_EXCHANGE` back to `Refuse` — the plain road is
    /// bypassed: the save is refused and the file keeps its old bytes.
    #[test]
    fn a_preview_save_through_the_storage_lane_lands_plainly_on_a_drive_that_cannot_exchange() {
        let root = scratch("lane-no-exchange");
        let path = root.join("备忘 memo.md");
        std::fs::write(&path, "原文 original\n").unwrap();
        let drive = crate::test_support::NoExchangeDrive::new(7);
        let seen = Arc::clone(&drive.seen);
        let served = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&served);
        let (mut lane, _receipts) = StorageLane::fresh_with(
            crate::lane::Executor::thread_door(bt_platform::ThreadPriority::BelowNormal),
            Arc::new(super::wake_documents),
            Box::new(crate::diagnostics::note),
            Arc::new(super::let_go),
        );
        Arc::get_mut(&mut lane.shared)
            .expect("a lane just made has one owner")
            .door = Some(Arc::new(move |worker, job, receipts| {
            if let StorageJob::Document(job) = &*job {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                crate::document_request::serve_document_on(worker, &drive, job, &mut |_| {});
                return super::Served::Done;
            }
            super::serve_storage_job(worker, job, receipts)
        }));
        assert!(
            super::SessionWriter::start(&lane),
            "a worker serves the lane"
        );
        let mut saves = crate::preview::PreviewSaves::on(lane);
        let mut buffer = crate::preview::PreviewBuffer::new(
            crate::preview::PreviewSource::file(path.clone()),
            "备忘 memo.md".to_owned(),
        );
        buffer.accept(crate::preview::read_head(&path));

        for (line, said) in [
            ("第一 first\n", crate::preview::SaveOutcome::SavedOrdinarily),
            ("第二 second\n", crate::preview::SaveOutcome::Saved),
        ] {
            assert!(buffer.edit_content(|content| {
                content.push_str(line);
                true
            }));
            assert_eq!(
                crate::test_support::save_through(&mut buffer, &mut saves),
                Some(said),
                "the sentence is said on the drive's first save only"
            );
        }
        assert_eq!(
            served.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "both saves were served inside the lane's tasks"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "原文 original\n第一 first\n第二 second\n",
            "the saves landed with their bytes"
        );
        {
            let seen = seen.lock().expect("the drive's record");
            assert_eq!(
                seen.exchanged.len(),
                2,
                "each save asked the exchange, refused"
            );
            assert_eq!(
                seen.committed, seen.exchanged,
                "each landing is the copy staged before its check"
            );
        }
        assert_eq!(
            names_in(&root),
            ["备忘 memo.md"],
            "nothing is left beside the file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((c)3 item 2) — **a link is written the plain way only when the request says so.** A
    /// file with a second name is refused for a request that refuses links — nothing is written —
    /// and written through its name for one whose policy is `PlainWriter`, the preview's save as it
    /// stands.
    ///
    /// MUTATION: honour `PlainWriter` for nothing (answer `Refusal::Link` whatever the policy) —
    /// the second write is refused.
    #[test]
    fn a_link_is_written_the_plain_way_only_when_the_request_says_so() {
        let root = scratch("link-policy");
        let path = root.join(SETTINGS_FILE_NAME);
        let base = bytes(&settings("基线 Base Mono"));
        std::fs::write(&path, &base).unwrap();
        std::fs::hard_link(&path, root.join("点文件 dotfiles.json")).unwrap();
        let ours = bytes(&settings("我们的 Ours Mono"));

        let (refused, answered) = job(&path, Some(&base), &ours, Mode::Restore, 1, Vec::new());
        on_a_worker(move |worker| serve_document(worker, &refused));
        assert_eq!(
            answered.try_recv().unwrap().outcome,
            Outcome::Refused {
                cause: Refusal::Link
            }
        );
        assert_eq!(file_bytes(&path), base, "nothing was written");

        let (mut plain, answered) = job(&path, Some(&base), &ours, Mode::Restore, 1, Vec::new());
        plain.link_policy = LinkPolicy::PlainWriter;
        on_a_worker(move |worker| serve_document(worker, &plain));
        assert!(matches!(
            answered.try_recv().unwrap().outcome,
            Outcome::Landed { .. }
        ));
        assert_eq!(file_bytes(&path), ours, "written through its name");
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The storage worker's stable read (design note 2026-10-11, (b)9), as a store takes it.
#[cfg(test)]
mod stable_read_tests {
    use bt_persist::{PinKind, StableRead};

    use super::StorageLane;
    use super::conditional_write_tests::{on_a_worker, scratch};
    use crate::document_request::Heard;
    use crate::pins::{PINS_FILE_NAME, PinsStore};

    fn table(folders: &[&str]) -> Vec<u8> {
        let mut table = bt_persist::PinsV1::default();
        for folder in folders {
            crate::pins::toggle_pin(&mut table, PinKind::Folder, folder);
        }
        bt_persist::serialize_pins(&table).expect("a pin table serializes")
    }

    /// A read of `path` with a writer putting `racing` there between the bytes and the second
    /// look at the file.
    fn raced(path: &std::path::Path, racing: Vec<u8>) -> StableRead {
        let path = path.to_path_buf();
        on_a_worker(move |worker| {
            bt_persist::read_stable_between(worker, &path, || {
                std::fs::write(&path, &racing).expect("the racing writer writes");
            })
        })
    }

    /// RED ((b)14-15, first half, as this ticket builds it) — **a valid intermediate document is
    /// taken, then the final one.** An in-place writer that pauses between two valid documents is
    /// two saves; a read that raced the second write in between takes nothing, and the next pass
    /// takes the final document.
    ///
    /// MUTATION: act on a `MovedDuringRead` (in `DocumentState::on_read`, read it as no file) —
    /// the raced pass empties the table.
    #[test]
    fn a_valid_intermediate_document_is_taken_then_the_final_one() {
        let root = scratch("intermediate");
        let path = root.join(PINS_FILE_NAME);
        std::fs::write(&path, table(&["起点 start"])).unwrap();
        let mut store = PinsStore::at_on(path.clone(), StorageLane::of_its_own());

        std::fs::write(&path, table(&["起点 start", "中间 middle"])).unwrap();
        assert_eq!(
            store.reread(),
            Heard::Changed,
            "the valid intermediate is taken"
        );
        assert_eq!(
            store.targets(PinKind::Folder),
            vec!["起点 start".to_owned(), "中间 middle".to_owned()]
        );

        let read = raced(&path, table(&["起点 start", "中间 middle", "终点 — final"]));
        assert_eq!(read, StableRead::MovedDuringRead);
        assert_eq!(store.document().hear_read(read).heard(), Heard::Unchanged);
        assert_eq!(
            store.targets(PinKind::Folder),
            vec!["起点 start".to_owned(), "中间 middle".to_owned()],
            "a raced read takes nothing"
        );

        assert_eq!(
            store.reread(),
            Heard::Changed,
            "and the next pass takes the final one"
        );
        assert_eq!(
            store.targets(PinKind::Folder),
            vec![
                "起点 start".to_owned(),
                "中间 middle".to_owned(),
                "终点 — final".to_owned()
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)14-15, second half) — **a read that raced a writer is not merged**: the bytes in
    /// hand are a document nobody saved last, and the read answers `MovedDuringRead`.
    ///
    /// MUTATION: no second `metadata` in `bt_persist::read_stable_between` — the read answers the
    /// bytes it had, and the store takes a document the file no longer holds.
    #[test]
    fn a_read_that_raced_a_writer_is_not_merged() {
        let root = scratch("raced");
        let path = root.join(PINS_FILE_NAME);
        std::fs::write(&path, table(&["起点 start"])).unwrap();
        let mut store = PinsStore::at_on(path.clone(), StorageLane::of_its_own());

        std::fs::write(&path, table(&["手改 hand edit"])).unwrap();
        let read = raced(&path, table(&["手改 hand edit", "又改 — again"]));
        assert_eq!(read, StableRead::MovedDuringRead);
        assert_eq!(store.document().hear_read(read).heard(), Heard::Unchanged);
        assert_eq!(
            store.targets(PinKind::Folder),
            vec!["起点 start".to_owned()],
            "nothing of the raced read is in force"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Owed edits (design note 2026-10-11 §4, (b)6): what a press puts in `desired`, and who owes.
#[cfg(test)]
mod owed_tests {
    use bt_persist::{BindingOverrideV1, KeybindingsV1, StableRead};

    use super::conditional_write_tests::{bytes, next_write, scratch, settings};
    use super::{
        KEYBINDINGS_FILE_NAME, KeybindingsStore, SETTINGS_FILE_NAME, SettingsStore, StorageLane,
        keybindings_document, keybindings_platform,
    };
    use crate::document_request::Heard;

    fn binding(action: &str, chord: &str) -> BindingOverrideV1 {
        BindingOverrideV1 {
            action: action.to_owned(),
            chord: Some(chord.to_owned()),
        }
    }

    /// RED ((b)14-17) — **a value a door refused is not written back.** The file names an action
    /// this build has no row for; the table leaves it out, and a press on another row changes only
    /// that row in the document Folio writes — the line it does not understand is carried, in its
    /// place.
    ///
    /// MUTATION: derive `desired` from the projection (in `StoredDocument::apply_press` for
    /// `KeybindingsV1`, take `after` whole) — the carried line is written out of the file.
    #[test]
    fn a_value_a_door_refused_is_not_written_back() {
        let root = scratch("refused-value");
        let path = root.join(KEYBINDINGS_FILE_NAME);
        let unknown = binding("未知动作-no-such-action", "Ctrl+Alt+K");
        let file = keybindings_document(
            vec![unknown.clone(), binding("new-tab", "Ctrl+Shift+Y")],
            keybindings_platform(),
        );
        std::fs::write(&path, bt_persist::serialize_keybindings(&file).unwrap()).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = KeybindingsStore::at_on(path.clone(), lane);
        // What the table put in force: everything but the line it has no row for.
        store.project(vec![binding("new-tab", "Ctrl+Shift+Y")]);

        assert!(store.store(vec![binding("new-tab", "Ctrl+Shift+U")]));
        let write = next_write(&jobs);
        let written: KeybindingsV1 =
            bt_persist::parse_keybindings(&write.bytes).expect("the written document parses");
        assert_eq!(
            written.bindings,
            vec![unknown, binding("new-tab", "Ctrl+Shift+U")],
            "the refused line is carried, and only the pressed row changed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A keybindings store over `lines` written in `written_in`'s dialect, opened as a launch on
    /// `here` opens it, on a held lane: the store, the table in force, the launch's refusals.
    fn opened_on(
        root: &std::path::Path,
        lines: Vec<BindingOverrideV1>,
        written_in: bt_platform::HostPlatform,
        here: bt_platform::HostPlatform,
    ) -> (
        KeybindingsStore,
        super::HeldJobs,
        crate::shortcuts::Shortcuts,
        Vec<crate::shortcuts::OverrideFault>,
    ) {
        let path = root.join(KEYBINDINGS_FILE_NAME);
        let file = keybindings_document(lines, super::keybindings_platform_on(written_in));
        std::fs::write(&path, bt_persist::serialize_keybindings(&file).unwrap()).unwrap();
        let (lane, jobs) = StorageLane::held();
        let mut store = KeybindingsStore::at_on(path, lane);
        let (table, faults) = crate::settings_bundle::shortcuts_at_start(&mut store, here);
        (store, jobs, table, faults)
    }

    /// The table's departures, as a press stores them.
    fn departures_of(table: &crate::shortcuts::Shortcuts) -> Vec<BindingOverrideV1> {
        table
            .overrides()
            .into_iter()
            .map(|entry| BindingOverrideV1 {
                action: entry.id,
                chord: entry.chord,
            })
            .collect()
    }

    /// The chord `action` holds in `table`, as `platform` spells it.
    fn chord_in(
        table: &crate::shortcuts::Shortcuts,
        action: &str,
        platform: bt_platform::HostPlatform,
    ) -> Option<String> {
        table
            .row(action)
            .and_then(|row| row.chord.as_ref())
            .map(|chord| crate::shortcuts::format_chord_on(chord, platform))
    }

    /// RED (T-STORE-MERGE-KEYBINDINGS round 6, the review's blocker) — **a press on a file
    /// written on the other platform keeps what the projection lacks.** A Mac's file — a line
    /// naming an action this build has no row for, a shadowed New tab line, New tab's line in
    /// force and Close pane's — is opened on Windows before any merge, and one press binds Quit.
    /// The document written is Windows': the line this build cannot name and the shadowed line
    /// stay verbatim in their places, New tab's and Close pane's lines are carried into Windows'
    /// spelling, and Quit's is the press. Read again on Windows and on a Mac, the table is one
    /// table, each in its own keys.
    ///
    /// MUTATION: the whole-replace road (in `KeybindingsV1::apply_press`, a press in another
    /// dialect than `desired` takes the press's document whole) — the two carried lines are lost.
    #[test]
    fn a_press_on_a_file_from_the_other_platform_keeps_what_the_projection_lacks() {
        use bt_platform::HostPlatform::{MacOs, Windows};
        let root = scratch("press-other-dialect");
        let unknown = binding("未来动作-future-action", "Cmd+Shift+F");
        let shadowed = binding("new-tab", "Cmd+Shift+J");
        let (mut store, jobs, mut table, faults) = opened_on(
            &root,
            vec![
                unknown.clone(),
                shadowed.clone(),
                binding("new-tab", "Cmd+Shift+Y"),
                binding("close-pane", "Cmd+Shift+O"),
            ],
            MacOs,
            Windows,
        );
        assert_eq!(
            faults
                .iter()
                .map(|fault| fault.id.as_str())
                .collect::<Vec<_>>(),
            vec![unknown.action.as_str()],
            "the launch refuses only the action it cannot name"
        );
        table.set("quit", crate::shortcuts::parse_chord("Ctrl+u"));
        assert!(store.store_on(
            departures_of(&table),
            super::keybindings_platform_on(Windows)
        ));

        let written: KeybindingsV1 = bt_persist::parse_keybindings(&next_write(&jobs).bytes)
            .expect("the written document parses");
        assert_eq!(
            written.writing_platform,
            super::keybindings_platform_on(Windows)
        );
        assert_eq!(
            &written.bindings[..2],
            &[unknown.clone(), shadowed][..],
            "verbatim, in place"
        );
        assert_eq!(written.bindings[2], binding("new-tab", "Ctrl+Shift+y"));
        assert!(
            written
                .bindings
                .contains(&binding("close-pane", "Ctrl+Shift+o"))
        );
        assert!(written.bindings.contains(&binding("quit", "Ctrl+u")));
        assert_eq!(written.bindings.len(), 5, "{:?}", written.bindings);

        let mut read = Vec::new();
        for platform in [Windows, MacOs] {
            let (table, faults) = crate::settings_bundle::read_keybindings(&written, platform);
            assert_eq!(
                faults
                    .iter()
                    .map(|fault| fault.id.as_str())
                    .collect::<Vec<_>>(),
                vec![unknown.action.as_str()],
                "{platform:?}"
            );
            read.push(
                ["new-tab", "close-pane", "quit"].map(|action| chord_in(&table, action, platform)),
            );
        }
        assert_eq!(
            read,
            vec![
                [
                    Some("Ctrl+Shift+y".to_owned()),
                    Some("Ctrl+Shift+o".to_owned()),
                    Some("Ctrl+u".to_owned())
                ],
                [
                    Some("Cmd+Shift+y".to_owned()),
                    Some("Cmd+Shift+o".to_owned()),
                    Some("Cmd+u".to_owned())
                ],
            ],
            "one table, each platform's keys"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORE-MERGE-KEYBINDINGS round 6, review nit) — **a press on an action whose only
    /// line is refused goes after it**, so the refused line is shadowed and silent: the file's
    /// New tab line is not a chord here; a press binds New tab; the next start reads the press's
    /// chord and refuses nothing.
    ///
    /// MUTATION: no `last_under` (the press's line goes after its nearest predecessor in the
    /// table, here first) — the refused line is the last about New tab and is refused again.
    #[test]
    fn a_press_on_an_action_whose_only_line_is_refused_goes_after_it() {
        use bt_platform::HostPlatform::Windows;
        let root = scratch("press-after-refused");
        let refused = binding("new-tab", "Cmd+Win+K");
        let (mut store, jobs, mut table, faults) = opened_on(
            &root,
            vec![refused.clone(), binding("quit", "Ctrl+u")],
            Windows,
            Windows,
        );
        assert_eq!(faults.len(), 1, "the launch refuses the line: {faults:?}");
        table.set("new-tab", crate::shortcuts::parse_chord("Ctrl+k"));
        assert!(store.store_on(
            departures_of(&table),
            super::keybindings_platform_on(Windows)
        ));
        let written: KeybindingsV1 = bt_persist::parse_keybindings(&next_write(&jobs).bytes)
            .expect("the written document parses");
        assert_eq!(
            written.bindings,
            vec![
                refused,
                binding("new-tab", "Ctrl+k"),
                binding("quit", "Ctrl+u")
            ]
        );
        let (table, faults) = crate::settings_bundle::read_keybindings(&written, Windows);
        assert!(
            faults.is_empty(),
            "the next start refuses nothing: {faults:?}"
        );
        assert_eq!(
            chord_in(&table, "new-tab", Windows).as_deref(),
            Some("Ctrl+k")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (T-STORE-MERGE-KEYBINDINGS round 5) — **a press that takes an action's departure away
    /// takes its shadowed lines with it.** The file holds two lines about New tab, the later one in
    /// force — the earlier one not a chord here, or a chord. The launch reads `Ctrl+K` and says
    /// nothing; New tab is reset with a press; the document written holds no line about New tab,
    /// and the next start reads it at its default with nothing to refuse.
    ///
    /// MUTATION: the press's own selection (in `KeybindingsV1::apply_press`, the last line about
    /// the action, read or not, and nothing removed with it — `InForce::Last`) — an earlier line
    /// outlives the reset: refused at every start, or put in force.
    #[test]
    fn a_reset_by_a_press_takes_the_actions_shadowed_lines_with_it() {
        let platform = bt_platform::host_platform();
        let chord_of = |table: &crate::shortcuts::Shortcuts| {
            table
                .row("new-tab")
                .and_then(|row| row.chord.as_ref())
                .map(|chord| crate::shortcuts::format_chord_on(chord, platform))
        };
        let defaults = crate::shortcuts::Shortcuts::defaults_for(platform);
        for (case, earlier) in [
            ("press-unreadable", "Cmd+Win+K"),
            ("press-readable", "Ctrl+j"),
        ] {
            let root = scratch(case);
            let path = root.join(KEYBINDINGS_FILE_NAME);
            let file = keybindings_document(
                vec![binding("new-tab", earlier), binding("new-tab", "Ctrl+k")],
                keybindings_platform(),
            );
            std::fs::write(&path, bt_persist::serialize_keybindings(&file).unwrap()).unwrap();
            let (lane, jobs) = StorageLane::held();
            let mut store = KeybindingsStore::at_on(path.clone(), lane);
            let (mut table, faults) =
                crate::settings_bundle::shortcuts_at_start(&mut store, platform);
            assert!(
                faults.is_empty(),
                "{case}: the launch says nothing: {faults:?}"
            );
            assert_eq!(chord_of(&table).as_deref(), Some("Ctrl+k"), "{case}");

            table.restore("new-tab");
            let departures: Vec<BindingOverrideV1> = table
                .overrides()
                .into_iter()
                .map(|entry| BindingOverrideV1 {
                    action: entry.id,
                    chord: entry.chord,
                })
                .collect();
            assert!(store.store(departures));
            let written: KeybindingsV1 = bt_persist::parse_keybindings(&next_write(&jobs).bytes)
                .expect("the written document parses");
            assert!(
                !written.bindings.iter().any(|line| line.action == "new-tab"),
                "{case}: no line about New tab is left: {:?}",
                written.bindings
            );

            let next = root.join("next-start.json");
            std::fs::write(&next, bt_persist::serialize_keybindings(&written).unwrap()).unwrap();
            let (lane, _jobs) = StorageLane::held();
            let mut again = KeybindingsStore::at_on(next, lane);
            let (table, faults) = crate::settings_bundle::shortcuts_at_start(&mut again, platform);
            assert!(
                faults.is_empty(),
                "{case}: the next start says nothing: {faults:?}"
            );
            assert_eq!(chord_of(&table), chord_of(&defaults), "{case}: its default");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// RED (carry-forward 1) — **a process that is not the folder's writer queues no write, and a
    /// re-read does not undo its choice.**
    ///
    /// MUTATION: drop the `writer_of_record` check in `DocumentState::send_owed` — the press
    /// queues a write.
    #[test]
    fn a_non_writer_queues_no_write() {
        let root = scratch("non-writer");
        let path = root.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, bytes(&settings("基线 Base Mono"))).unwrap();
        let holder = bt_platform::instance::claim_data_directory(&root)
            .expect("nothing else on this machine has this folder");
        let (lane, jobs) = StorageLane::held();
        let mut store = SettingsStore::read_at(path.clone(), false, lane);

        assert!(store.store(settings("这里选的 Chosen Mono")));
        assert!(jobs.try_recv().is_err(), "no write is queued");

        // Another process writes the file; this one's re-read keeps its own choice.
        let news = store.document().hear_read(StableRead::Bytes(
            bytes(&settings("别处写的 Elsewhere Mono")).into(),
        ));
        assert_eq!(news.heard(), Heard::Unchanged);
        assert_eq!(store.loaded(), &settings("这里选的 Chosen Mono"));
        assert!(jobs.try_recv().is_err(), "and still nothing is queued");
        drop(holder);
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// **The storage worker's adapter for the lane contract's suite** (`crate::lane`;
/// T-STORAGE-WORKER-ON-THE-SEAM): the real [`StorageLane`] — its ledger, its admission under the
/// queue's lock, its worker's loop and its end — started by the suite's scripted executor, its
/// requests asked through the document door ([`StorageLane::send_document`]) and heard as every
/// asker hears them ([`OnTheWorker`]). Only the door the worker serves each job through is the
/// suite's: it tells the gate which question it runs, so a planted panic fails that request
/// where the product's would fail.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError, mpsc};

    use super::{
        NO_STORAGE_WORKER, OnTheWorker, Reply, Served, SessionWriter, StorageId, StorageJob,
        StorageLane, StorageTerminal, storage_task_failed,
    };
    use crate::document_request::{DocumentAnswer, DocumentJob, LinkPolicy, Mode, NoExchange};
    use crate::lane::{
        Admission, Contract, Delivered, Executor, Gate, LaneUnderTest, Outcome, STORAGE, Terminal,
        WakeProbe,
    };

    /// The file a question is asked about, so the door can say which one it runs.
    const WHICH: &str = "storage-lane-contract-";

    fn question_of(which: &Path) -> Option<u64> {
        which.to_str()?.strip_prefix(WHICH)?.parse().ok()
    }

    /// What a terminal the sink took reads as, in the suite's words.
    fn outcome(terminal: &StorageTerminal) -> Outcome {
        match terminal {
            Terminal::Done(()) => Outcome::Answered,
            Terminal::Failed(failure) => Outcome::Failed(storage_task_failed(&failure.to_string())),
            Terminal::WorkerGone => Outcome::WorkerGone,
            Terminal::Cancelled | Terminal::Superseded { .. } | Terminal::Joined { .. } => {
                unreachable!("the storage lane cancels, supersedes and joins nothing")
            }
        }
    }

    struct Adapter {
        lane: StorageLane,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// Every request each open target asked and has not heard the end of, by identity: a
        /// store's, a buffer's or the session's, as the product's askers hold theirs.
        targets: BTreeMap<u32, BTreeMap<StorageId, OnTheWorker<DocumentAnswer>>>,
        /// The suite's own record of which question each identity carried, for its tally.
        questions: HashMap<StorageId, u64>,
        /// What the lane's sink has been handed and the suite not yet read.
        sunk: Arc<Mutex<Vec<(StorageId, StorageTerminal)>>>,
        /// The lines the ledger said, counted through its `say`.
        said: Arc<AtomicUsize>,
    }

    /// A fresh storage lane, its `session-writer` worker started by the suite's executor.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let said = Arc::new(AtomicUsize::new(0));
        let wake = Arc::clone(&probe);
        let (counted, kept) = (Arc::clone(&said), Arc::clone(&sunk));
        let (mut lane, _receipts) = StorageLane::fresh_with(
            Executor::scripted(Arc::clone(&gate)),
            Arc::new(move || wake.woke()),
            Box::new(move |_line: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
            Arc::new(move |id, terminal| {
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((id, terminal));
            }),
        );
        let door = Arc::clone(&gate);
        Arc::get_mut(&mut lane.shared)
            .expect("a lane just made has one owner")
            .door = Some(Arc::new(move |_ctx, job, _receipts| {
            if let StorageJob::Document(job) = job {
                // The suite's file, held for the job's file work and let go before it answers,
                // as `serve_document` lets go of what it read and wrote before its `send`.
                let held = question_of(&job.which).map(|question| door.execute(question));
                drop(held);
                let _ = job.answer.send(DocumentAnswer {
                    generation: job.generation,
                    version: job.version,
                    asker: job.asker,
                    outcome: crate::document_request::Outcome::Landed {
                        bytes: Arc::clone(&job.bytes),
                        kept: None,
                        ordinary_on: None,
                    },
                    not_durable: None,
                    retired: Vec::new(),
                    left: None,
                });
            }
            Served::Done
        }));
        assert!(
            SessionWriter::start(&lane),
            "the suite's executor starts the storage worker"
        );
        Box::new(Adapter {
            lane,
            gate,
            probe,
            targets: BTreeMap::new(),
            questions: HashMap::new(),
            sunk,
            said,
        })
    }

    impl LaneUnderTest for Adapter {
        fn contract(&self) -> &'static Contract {
            &STORAGE
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        /// `DocumentState::send_owed` / `PreviewSaves::submit`: the lane's own admission, the
        /// request held by the asking target beside its road, or the refusal at the door.
        fn submit(&mut self, target: u32, question: u64) -> Admission {
            let (answer, answered) = mpsc::channel();
            let job = DocumentJob {
                which: PathBuf::from(format!("{WHICH}{question}")),
                generation: question,
                version: question,
                expected: None,
                bytes: Arc::from(&b"{\"\xe9\x97\xae\": 1}"[..]),
                mode: Mode::Restore,
                link_policy: LinkPolicy::Refuse,
                no_exchange: NoExchange::Refuse,
                asker: 0,
                kept: Vec::new(),
                answer,
            };
            match self.lane.send_document(job) {
                Ok(request) => {
                    let id = request.id();
                    self.questions.insert(id, question);
                    self.targets
                        .entry(target)
                        .or_default()
                        .insert(id, OnTheWorker::admitted(answered, request));
                    Admission {
                        ticket: Some(id.0),
                        question,
                        refused: None,
                    }
                }
                Err(refused) => Admission {
                    ticket: None,
                    question,
                    refused: Some(refused.said(NO_STORAGE_WORKER)),
                },
            }
        }

        /// A store or a buffer that goes drops what it holds; each request's terminal goes to
        /// the lane's sink.
        fn close_target(&mut self, target: u32) {
            self.targets.remove(&target);
        }

        /// `DocumentState::take_answers` / `PreviewSaves::take_answers`, across every target in
        /// the order the ledger minted: each request heard once it has ended, and the drain
        /// stops at the first that has not, so a later one is never taken before it.
        fn drain(&mut self) -> Vec<Delivered> {
            let mut held: Vec<(StorageId, u32)> = self
                .targets
                .iter()
                .flat_map(|(target, asked)| asked.keys().map(move |id| (*id, *target)))
                .collect();
            held.sort_unstable();
            let mut found = Vec::new();
            for (id, target) in held {
                let Some(asked) = self.targets.get_mut(&target) else {
                    continue;
                };
                let Some(request) = asked.get_mut(&id) else {
                    continue;
                };
                let outcome = match request.hear() {
                    Reply::Waiting => break,
                    Reply::Answered(_) => Outcome::Answered,
                    Reply::Failed(words) => Outcome::Failed(words),
                    Reply::Gone => Outcome::WorkerGone,
                };
                asked.remove(&id);
                found.push(Delivered {
                    target: Some(target),
                    ticket: Some(id.0),
                    question: self.questions.get(&id).copied(),
                    outcome,
                });
            }
            found
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the storage lane's target is the asker, not the application");
        }

        fn sunk(&mut self) -> Vec<Delivered> {
            let sunk =
                std::mem::take(&mut *self.sunk.lock().unwrap_or_else(PoisonError::into_inner));
            sunk.into_iter()
                .map(|(id, terminal)| Delivered {
                    target: None,
                    ticket: Some(id.0),
                    question: self.questions.get(&id).copied(),
                    outcome: outcome(&terminal),
                })
                .collect()
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}
