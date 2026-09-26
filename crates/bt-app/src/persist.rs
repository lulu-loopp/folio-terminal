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

use bt_platform::admission::{WaitToken, admitted, doors};

use bt_persist::{
    BindingOverrideV1, Debouncer, ExitState, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1, ProfilesV1,
    ReadReport, SessionV1, SettingsV1, WriteAlertAction, WriteFailureTracker, create_sentinel,
    probe_sentinel, read_keybindings, read_profiles, read_session, read_settings, remove_sentinel,
    write_keybindings_atomic, write_profiles_atomic, write_settings_atomic,
};

/// The name the session document wears on disk, which is also what a notice
/// about it has to say out loud — [`KEYBINDINGS_FILE_NAME`]'s rule, one file
/// over.
pub const SESSION_FILE_NAME: &str = "session.json";

/// And the preferences document's, on the same terms.
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// **How many times in a row a document may fail to reach the disk before the
/// reader is told and the retrying stops** (review rows R4-10 and R4-11).
///
/// The session store's retry lives on the debounce, so an unbounded one is a
/// write attempt every 1.5 seconds for as long as the window is open — against a
/// disk that is full, a folder that has gone read-only or a volume that was
/// unplugged, none of which the next attempt is going to fix. Five is enough to
/// ride out the transient case (an antivirus holding the file open for a moment,
/// a sync client mid-rename) and short enough that the reader hears about the
/// permanent one while they can still do something about it.
///
/// **It is a bound on retries and not a surrender.** Any new change re-arms the
/// counter — see [`DocumentWrites::rearm`] — because a reader who has just
/// changed something is a reader asking for it to be saved, and because the
/// condition may well have gone away in the meantime.
const MAX_WRITE_ATTEMPTS: u32 = 5;

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
/// this records that it is **not on the disk**, which is what makes the next
/// press a real attempt rather than a comparison that matches.
#[derive(Debug)]
pub(crate) struct DocumentWrites {
    failures: WriteFailureTracker,
    /// The document the store holds has not reached the disk.
    unsaved: bool,
    /// Failures since the last success or the last re-arming.
    streak: u32,
    /// The sentence the reader is owed, once, when the streak has run out.
    fault: Option<String>,
}

impl DocumentWrites {
    pub(crate) fn new() -> Self {
        Self {
            failures: WriteFailureTracker::new(),
            unsaved: false,
            streak: 0,
            fault: None,
        }
    }

    /// Whether a store asked to hold a document it already holds should
    /// nonetheless write it.
    ///
    /// The whole of R4-10's fix in one line: an unchanged value is normally
    /// nothing to do, and is exactly something to do when the last attempt at it
    /// did not land.
    pub(crate) fn wants_write(&self, changed: bool) -> bool {
        changed || self.unsaved
    }

    /// Whether there is any point trying again on a clock rather than on a
    /// press.
    pub(crate) fn may_retry(&self) -> bool {
        self.streak < MAX_WRITE_ATTEMPTS
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
        self.unsaved = !landed;
        if landed {
            self.streak = 0;
            return true;
        }
        self.streak = self.streak.saturating_add(1);
        if self.streak == MAX_WRITE_ATTEMPTS {
            self.fault
                .get_or_insert_with(|| crate::i18n::persisted_file_unwritable(file));
        }
        false
    }

    /// A new decision has been made, so the retrying starts again.
    pub(crate) fn rearm(&mut self) {
        self.streak = 0;
    }

    /// Take the sentence, so a card about it is raised once and not once a frame.
    pub(crate) fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
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
/// Asked once and remembered, because the claim is held for the life of the
/// process: the answer cannot change while this process runs, and a second call
/// that took a second claim would refuse itself.
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
/// **A refusal is remembered too** (self-update R-3, as refined by revision
/// 2026-09-25 (b)): a process told once that it is not the writer is not made
/// the writer later by this question, because everything it opened in between
/// was opened as a non-writer. The one road into the table for a claim taken
/// after the first ask is [`adopt_claim`], and it does not overwrite an answer.
pub(crate) fn is_writer_of(directory: &Path) -> bool {
    claim_table()
        .entry(bt_platform::instance::claim_name(directory))
        .or_insert_with(|| bt_platform::instance::claim_data_directory(directory))
        .is_some()
}

/// **The claim table** — one row per claim name, holding either the claim this
/// process took (it is the writer, and the guard lives here for the life of
/// the process) or `None` (it asked and was refused). Written by
/// [`is_writer_of`] on a first ask and by [`adopt_claim`]; read by nothing
/// else.
fn claim_table() -> std::sync::MutexGuard<
    'static,
    HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>,
> {
    static CLAIMS: OnceLock<
        Mutex<HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>>,
    > = OnceLock::new();
    CLAIMS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("the claim table is locked to read or take one entry and nothing else")
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
/// The refusal says which of two things happened
/// ([`bt_platform::instance::ClaimRefusal`]): a live holder, which is worth
/// asking again, or a question the platform did not answer, which is not
/// evidence that anybody holds anything.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "self-update R-3's claim API lands before its caller: the health-check                   process arrives in U-17/U-18 ((b).5)"
    )
)]
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "self-update R-3's claim API lands before its caller: the health-check                   process arrives in U-17/U-18 ((b).5)"
    )
)]
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
            "BT_PERSIST a claim was adopted for {name} after this process had already answered              whether it writes there (writer={was_writer}); the earlier answer stands"
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
const SESSION_SAVE_BUDGET: Duration = Duration::from_secs(3);

/// How often a bounded join asks whether the writer thread has finished.
const SESSION_JOIN_POLL: Duration = Duration::from_millis(2);

/// What a store with nowhere to send a document says (release review X-9).
///
/// `spawn_at_priority` is `Builder::spawn` with a priority set inside the thread, so this is the
/// operating system refusing a thread — which is a state, not a verdict about the disk. The
/// document stays owed and the next hand-over asks for a thread again.
const NO_WRITER_THREAD: &str = "the session writer could not be started";

/// What a save that ran out of budget says, on `stderr` and to the caller.
///
/// One sentence for both, because they are the same fact: the document in hand did not reach
/// the disk, and whatever the last completed save left there is still what the next start
/// reads.
fn save_did_not_finish() -> String {
    let seconds = SESSION_SAVE_BUDGET.as_secs();
    format!("session save did not finish within {seconds} s; the last completed save stands")
}

/// Say it once, in the words the log is read in.
fn report_save_did_not_finish() {
    eprintln!("Folio: {}", save_did_not_finish());
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
    /// The write did not finish inside [`SESSION_SAVE_BUDGET`], and the sentence to say.
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

/// The channel ends one writer thread works from: requests in, receipts out.
struct SessionWriterEnds {
    incoming: mpsc::Receiver<SessionWriteRequest>,
    outgoing: mpsc::Sender<SessionWriteReceipt>,
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
    requests: mpsc::Sender<SessionWriteRequest>,
    receipts: mpsc::Receiver<SessionWriteReceipt>,
    /// **The two ends the writer thread needs, until one thread has them**
    /// (T-QUIT-TIMEOUT-PROCEEDS, release review X-9).
    ///
    /// `spawn_at_priority` is `Builder::spawn` with a priority set inside the thread, so the
    /// only way it fails is that the operating system would not start a thread at all — a state
    /// a machine is in for a moment, not for a run. Holding the ends here is what lets the next
    /// hand-over try again ([`Self::start`]) instead of the store growing a second road to the
    /// file; taking them under the lock, inside the thread, is what makes "a second thread"
    /// impossible rather than merely unlikely.
    ends: Arc<Mutex<Option<SessionWriterEnds>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// The newest request handed over. A receipt older than this is stale.
    sent: u64,
    /// The newest request a receipt has come back for. `sent > landed` is "a document is still
    /// in flight", which is the question a quit has to ask even when nothing is dirty.
    landed: u64,
    /// **A wait on this writer has already run out of budget** (T-QUIT-HAS-A-DEADLINE).
    ///
    /// Once one has, the thread is inside a call nobody in this process can bound, and every
    /// later wait would be the same budget spent again for the same answer — a quit that asks
    /// three times turns a three-second deadline into a nine-second one. So the first expiry is
    /// remembered, and from then on this writer answers immediately with the same sentence.
    stalled: bool,
}

impl SessionWriter {
    fn open() -> Self {
        let (requests, incoming) = mpsc::channel::<SessionWriteRequest>();
        let (outgoing, receipts) = mpsc::channel::<SessionWriteReceipt>();
        let mut writer = Self {
            requests,
            receipts,
            ends: Arc::new(Mutex::new(Some(SessionWriterEnds { incoming, outgoing }))),
            thread: None,
            sent: 0,
            landed: 0,
            stalled: false,
        };
        writer.start();
        writer
    }

    /// **Make sure there is one writer thread, and never a second.**
    ///
    /// Idempotent, and the only place a thread is started. The ends are taken *inside* the
    /// thread, under the lock, so a spawn that failed leaves them where the next attempt finds
    /// them and a spawn that succeeded leaves nothing for anybody else to take — which is what
    /// makes "one writer, so ordering is not a question" a property of the type rather than of
    /// the paths that call it (release review X-9).
    fn start(&mut self) -> bool {
        if self.thread.is_some() {
            return true;
        }
        if self.ends.lock().is_ok_and(|ends| ends.is_none()) {
            // A thread already took them and has since ended, or `close` let them go. Either way
            // there is nothing left for a new thread to work from.
            return false;
        }
        let ends = Arc::clone(&self.ends);
        // In the workers' band (§1.4): a session write must never be the reason a frame was late,
        // and it is never the thing anybody is waiting to see.
        self.thread = bt_platform::spawn_at_priority(
            "session-writer",
            bt_platform::ThreadPriority::BelowNormal,
            move |_ctx| {
                let Some(ends) = ends.lock().ok().and_then(|mut held| held.take()) else {
                    return;
                };
                while let Ok(request) = ends.incoming.recv() {
                    let result = bt_persist::atomic_write(&request.path, &request.bytes)
                        .map_err(|error| error.to_string());
                    if ends
                        .outgoing
                        .send(SessionWriteReceipt {
                            generation: request.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            },
        )
        .ok();
        self.thread.is_some()
    }

    /// The newest document still on its way to the disk, if one is.
    fn in_flight(&self) -> Option<u64> {
        (self.sent > self.landed).then_some(self.sent)
    }

    /// Hand one document over. Answers the generation it was filed under.
    ///
    /// A thread the operating system would not start leaves this returning `None`, and the
    /// caller reports that rather than writing the document itself: this store has exactly one
    /// road to `session.json` and it is not the window thread (release review X-9). The attempt
    /// is made again here on every hand-over, so a machine that could not spare a thread for a
    /// moment gets its writer at the next quiet window rather than going the rest of the run
    /// without one.
    fn send(&mut self, path: &Path, bytes: Vec<u8>) -> Option<u64> {
        if !self.start() {
            return None;
        }
        let generation = self.sent + 1;
        self.requests
            .send(SessionWriteRequest {
                generation,
                path: path.to_path_buf(),
                bytes,
            })
            .ok()?;
        self.sent = generation;
        Some(generation)
    }

    /// Every receipt that has arrived, newest-relevant last. Never waits.
    fn collect(&self) -> Vec<SessionWriteReceipt> {
        self.receipts.try_iter().collect()
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
    ) -> SessionWaitAnswer {
        let _ = token;
        let mut earlier = Vec::new();
        if self.stalled {
            return (earlier, Err(SaveRefusal::TimedOut(save_did_not_finish())));
        }
        let deadline = Instant::now() + SESSION_SAVE_BUDGET;
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
                    report_save_did_not_finish();
                    return (earlier, Err(SaveRefusal::TimedOut(save_did_not_finish())));
                }
                // The thread is gone and the answer is never coming. That is a refusal and not a
                // wait that ran long: nothing is on its way, so there is nothing to go on past.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return (
                        earlier,
                        Err(SaveRefusal::Refused(
                            "the session writer stopped before this document reached the disk"
                                .to_string(),
                        )),
                    );
                }
            }
        }
    }

    /// Let the thread finish what is queued and end. Idempotent.
    ///
    /// **An owner-thread door** (`doors::SessionWriterRetire`, §5.3 row 16b): its bounded poll
    /// and its join are admitted only on the way out, minted in [`SessionStore::close`].
    fn close(&mut self, token: WaitToken<'_, doors::SessionWriterRetire>) {
        let _ = token;
        // Dropping the sender is what ends the thread's `recv` loop. It is replaced rather than
        // dropped outright so the struct stays whole, and the ends go with it so no later
        // hand-over can start a writer over a channel nobody feeds.
        let (dead, _) = mpsc::channel();
        let live = std::mem::replace(&mut self.requests, dead);
        drop(live);
        if let Ok(mut ends) = self.ends.lock() {
            *ends = None;
        }
        let Some(thread) = self.thread.take() else {
            return;
        };
        // **Bounded, and for the reason the wait above is** (T-QUIT-HAS-A-DEADLINE). Everything
        // queued is already decided and each item is one atomic write, but "one atomic write" is
        // exactly the call that has no bound on storage that has stopped answering — an
        // unbounded join here would simply move the hang one statement later. Past the budget
        // the thread is let go rather than waited on: it is not holding anything this process
        // still needs, and the file it may yet replace it replaces whole.
        if self.stalled {
            return;
        }
        let deadline = Instant::now() + SESSION_SAVE_BUDGET;
        while !thread.is_finished() {
            if Instant::now() >= deadline {
                self.stalled = true;
                report_save_did_not_finish();
                return;
            }
            std::thread::sleep(SESSION_JOIN_POLL);
        }
        let _ = thread.join();
    }
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
    /// True once the sentinel for *this* run exists, so a clean exit knows
    /// there is something to remove.
    armed: bool,
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
        let sentinel_path = dir.join("session.lock");
        let writable = std::fs::create_dir_all(&dir).is_ok();
        // **Asked before the sentinel and before the read** (review row R4-5),
        // because both of those are things only the writer of record may do: a
        // second process that armed a sentinel would clear the first one's crash
        // record on its own clean exit. Asked of `dir`, which is the directory
        // this store's two files are in — see `is_writer_of`.
        let writer_of_record = is_writer_of(&dir);
        // Probe *before* creating: creating first would make every probe after
        // the first report a crash.
        let previous_exit = probe_sentinel(&sentinel_path).unwrap_or(ExitState::Normal);
        let (session, report, degradation) = read_session(&session_path);
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
        let armed = writable && writer_of_record && create_sentinel(&sentinel_path).is_ok();
        Self {
            session_path,
            sentinel_path,
            session,
            debouncer: Debouncer::new(),
            writes: DocumentWrites::new(),
            writer: SessionWriter::open(),
            armed,
            writer_of_record,
            fault,
        }
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
            armed: false,
            writer_of_record,
            fault: None,
        }
    }

    /// The session document as it was read. The caller owns what the fields
    /// mean; this type owns only when they reach the disk.
    pub fn loaded(&self) -> &SessionV1 {
        &self.session
    }

    /// Replace the in-memory document and start the debounce window. Called on
    /// a meaningful change: a tree edit, or the end of a resize.
    pub fn record(&mut self, session: SessionV1, now: Instant) {
        if self.session == session {
            return;
        }
        self.session = session;
        // **A new arrangement re-arms the retrying** (review row R4-11). The
        // bound below is on repeating one failed write on a clock; a reader who
        // has just moved a divider is asking for *this* document to be kept, and
        // the condition that refused the last one may well be gone.
        self.writes.rearm();
        self.debouncer.mark_dirty(now);
    }

    /// When the event loop should wake to write, if it should.
    pub fn deadline(&self) -> Option<Instant> {
        self.debouncer.deadline(SESSION_DEBOUNCE)
    }

    /// **Take the answers the writer has brought back, and hand it the document if the quiet
    /// window has elapsed.** The window thread's whole part in a session write.
    ///
    /// Both halves in one call because both belong to the same turn of the event loop and the
    /// second one's bookkeeping depends on the first: a receipt is what says the store is clean,
    /// and it has to be read before this turn decides whether anything is still owed.
    pub fn flush_if_due(&mut self, now: Instant) {
        crate::hang_watch::at(crate::hang_watch::Station::Autosave);
        self.take_receipts(now);
        if self.debouncer.should_flush(now, SESSION_DEBOUNCE) {
            self.hand_over(now);
        }
    }

    /// Whether this process is the one that writes `session.json` — see
    /// [`is_writer_of`], asked of the directory this store's file is in. A store
    /// that is not stays exactly as useful as one that is, in memory; it simply
    /// reaches no disk.
    fn writes_to_disk(&self) -> bool {
        self.writer_of_record
    }

    /// Hand the current document to the writer, without waiting for it to land.
    ///
    /// The debounce is marked flushed here rather than at the receipt, and the two are different
    /// questions: the debouncer answers "when should this be tried again", and handing it over is
    /// exactly the moment the answer becomes "not until something changes". Whether it *landed*
    /// is [`Self::take_receipts`]'s, and a failure there marks the document dirty again so the
    /// next quiet window retries it.
    fn hand_over(&mut self, now: Instant) {
        if !self.writes_to_disk() {
            // The second Folio over this directory (review row R4-5). Marked
            // flushed rather than left dirty, because nothing is owed: there is
            // no attempt to retry and no failure to report — this process was
            // never the one keeping this file.
            self.debouncer.mark_flushed();
            return;
        }
        let bytes = match bt_persist::serialize_session(&self.session) {
            Ok(bytes) => bytes,
            // A document that cannot be turned into JSON is not a disk problem and no thread will
            // fix it. It goes through the same one-alert-per-streak tracker so a broken document
            // does not print every 1.5 seconds.
            Err(error) => {
                self.report_write(Err(error.to_string()), now);
                return;
            }
        };
        if self.writer.send(&self.session_path, bytes).is_some() {
            self.debouncer.mark_flushed();
            return;
        }
        // **No writer thread, and therefore no write** (release review X-9). The only way
        // `spawn_at_priority` fails is that the operating system would not start a thread at
        // all, and the one thing this document may not be written on is this thread, which is
        // the window's. So it is reported instead, which puts it back on the clock: the next
        // quiet window tries the thread again, and the bounded retry in [`Self::report_write`]
        // is what stops a machine that will never spare one from asking for ever.
        self.report_write(Err(NO_WRITER_THREAD.to_string()), now);
    }

    /// Read whatever the writer has answered, and let a failure put the document back on the
    /// clock.
    ///
    /// A receipt older than the newest request is **stale and dropped**: a newer document has
    /// already been handed over and will bring its own answer, so letting an old failure mark the
    /// store dirty would schedule a retry of something that has since been superseded, and
    /// letting an old success mark it clean would be answering for a document nobody wrote.
    fn take_receipts(&mut self, now: Instant) {
        for receipt in self.writer.collect() {
            self.apply_receipt(receipt, now);
        }
    }

    fn apply_receipt(&mut self, receipt: SessionWriteReceipt, now: Instant) {
        // Recorded for every receipt, stale or not: what this answers is "is anything still on
        // its way", and a stale receipt is still one fewer document in flight.
        self.writer.landed = self.writer.landed.max(receipt.generation);
        if receipt.generation < self.writer.sent {
            return;
        }
        self.report_write(receipt.result, now);
    }

    /// One alert per failure streak (§5.3), and a failure leaves the document
    /// owed — **for a bounded number of tries** (review row R4-11).
    ///
    /// The retry used to be unconditional, which over a full disk or a volume
    /// that was unplugged meant one `atomic_write` every 1.5 seconds for as long
    /// as the window stayed open, each of them leaving a temp file behind
    /// (`bt_persist::atomic`'s own half of that row). Past the bound the document
    /// stops going back on the clock and the reader is told instead — and the
    /// next change re-arms it, because [`Self::record`] does.
    fn report_write(&mut self, result: Result<(), String>, now: Instant) {
        if self.writes.record(SESSION_FILE_NAME, result) {
            return;
        }
        if self.writes.may_retry() {
            self.debouncer.mark_dirty(now);
            return;
        }
        // **Past the bound the clock is stopped, not merely left un-wound.**
        // Leaving the document dirty would keep `flush_if_due` handing it over
        // every quiet window, which is the loop this bound exists to end; and
        // it would keep the event loop waking for a deadline it can do nothing
        // about. Marked flushed is the honest reading of "this store is not
        // going to try again" — it is not a claim that anything landed, which
        // is what the fault sentence taken by the window says instead. The next
        // real change re-arms both, in [`Self::record`].
        self.debouncer.mark_flushed();
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
        let now = Instant::now();
        if !self.writes_to_disk() {
            // The second Folio over this directory (review row R4-5). `Ok(())`
            // and not an error, because the quit's question is "did what this
            // process owed the disk reach it" and this process owes it nothing —
            // an error here would refuse to close a window over a document that
            // was never this window's.
            self.debouncer.mark_flushed();
            return Ok(());
        }
        self.take_receipts(now);
        if !self.debouncer.is_dirty() {
            // Clean, but not necessarily *landed*: a document handed over a moment ago can still
            // be on the writer's channel.
            let Some(outstanding) = self.writer.in_flight() else {
                return Ok(());
            };
            return self.wait_for_landing(outstanding, now);
        }
        let bytes = bt_persist::serialize_session(&self.session).map_err(|error| {
            let error = error.to_string();
            self.report_write(Err(error.clone()), now);
            SaveRefusal::Refused(error)
        })?;
        let Some(generation) = self.writer.send(&self.session_path, bytes) else {
            // No writer thread and no second road to the file (release review X-9). This is a
            // refusal and not a wait that ran long: nothing was queued, nothing is on its way,
            // and a window hidden over it would be the session gone.
            self.report_write(Err(NO_WRITER_THREAD.to_string()), now);
            return Err(SaveRefusal::Refused(NO_WRITER_THREAD.to_string()));
        };
        let landed = self.wait_for_landing(generation, now);
        if landed.is_ok() {
            self.debouncer.mark_flushed();
        }
        landed
    }

    /// Stand still until one named document has an answer, and book every answer that arrives on
    /// the way — a synchronous wait must not swallow the receipts the ordinary path was going to
    /// read.
    fn wait_for_landing(&mut self, generation: u64, now: Instant) -> Result<(), SaveRefusal> {
        // The one wait in this store is an owner-thread door. A refusal did not wait: it is the
        // stalled answer a spent budget gives, so a quit goes on rather than stopping over a
        // document that may still be on its way.
        let (earlier, landed) =
            admitted::<doors::SessionWriteWait, _>(|token| self.writer.wait_for(token, generation))
                .unwrap_or_else(|_refused| {
                    (
                        Vec::new(),
                        Err(SaveRefusal::TimedOut(save_did_not_finish())),
                    )
                });
        for receipt in earlier {
            self.apply_receipt(receipt, now);
        }
        // The tracker books one fact — "this document is not on the disk" — and both refusals
        // are that fact. Which of the two it was is the caller's question, not the clock's.
        self.apply_receipt(
            SessionWriteReceipt {
                generation,
                result: landed
                    .clone()
                    .map_err(|refusal| refusal.message().to_string()),
            },
            now,
        );
        landed
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

/// `settings.json` and when it reaches the disk.
///
/// No debouncer, unlike [`SessionStore`], and §1.1 is explicit about why the two
/// files are separate at all: "设置改动应立即落盘(用户在设置面板点一下就该生效并
/// 持久,丢失更痛)". A settings write happens when a human clicks a row in a
/// dialog — it cannot arrive at the rate divider drags do, so there is nothing
/// to coalesce and a quiet window would only be a window in which the choice can
/// be lost.
pub struct SettingsStore {
    path: PathBuf,
    settings: SettingsV1,
    writes: DocumentWrites,
    /// The sentence a startup owes about this file, if it owes one.
    fault: Option<String>,
    /// Whether this process writes `settings.json` at all — [`SessionStore`]'s
    /// field of this name, and review row R4-5's whole answer.
    writer_of_record: bool,
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
    /// ordinary state: every store writes at once.
    held: Option<bool>,
}

impl SettingsStore {
    /// Read `settings.json`, falling back to defaults on every failure — same
    /// contract as [`SessionStore::open`], and for the same reason: a terminal
    /// that refuses to start because it could not read a preferences file would
    /// be a worse product than one that starts with the default preferences.
    pub fn open() -> Self {
        let dir = storage_dir();
        let path = dir.join(SETTINGS_FILE_NAME);
        let _ = std::fs::create_dir_all(&dir);
        let (settings, report) = read_settings(&path);
        // §5.4 case 1 — no file yet — is the normal first run and must not alert.
        let fault = read_fault(
            &report,
            SETTINGS_FILE_NAME,
            crate::i18n::settings_file_unreadable,
        );
        Self {
            path,
            settings,
            writes: DocumentWrites::new(),
            fault,
            writer_of_record: is_writer_of(&dir),
            missing: report == ReadReport::NotFound,
            held: None,
        }
    }

    /// Take the sentence this store owes the reader — [`SessionStore::take_fault`]
    /// and every other store's, one file over.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.writes.take_fault())
    }

    /// The same store over a named file, for the tests that have to watch a
    /// write **fail** — [`SessionStore::at`]'s door and its reason: neither
    /// resolving `%APPDATA%` nor writing into it is a thing a test may do to the
    /// machine it runs on.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        let writer_of_record = is_writer_of_document(&path);
        Self {
            path,
            settings: SettingsV1::default(),
            writes: DocumentWrites::new(),
            fault: None,
            // [`SessionStore::at`]'s rule: asked of the directory this store
            // writes, rather than asserted about it.
            writer_of_record,
            missing: true,
            held: None,
        }
    }

    /// The settings as they currently stand.
    pub fn loaded(&self) -> &SettingsV1 {
        &self.settings
    }

    /// Whether this machine had no `settings.json` when the process started.
    ///
    /// Still true after the first write: it is a fact about the launch, not
    /// about the file, and the surface that asks it (the first-run card) is
    /// raised after the card's own state has already been written down.
    pub fn was_missing(&self) -> bool {
        self.missing
    }

    /// Record a change and put it on disk now. Returns whether anything changed,
    /// so a caller can skip the repaint when a user picks the value already set.
    ///
    /// **A value that did not reach the disk is written again next time it is
    /// chosen** (review row R4-10). The early return below used to be on
    /// equality alone, so re-picking the row a failed save had already adopted
    /// matched and retried nothing — and re-picking is exactly the gesture
    /// somebody makes when a setting did not take.
    pub fn store(&mut self, settings: SettingsV1) -> bool {
        let changed = self.settings != settings;
        if !self.writes.wants_write(changed) {
            return false;
        }
        self.settings = settings;
        if changed {
            self.writes.rearm();
        }
        if let Some(stored) = self.held.as_mut() {
            *stored = true;
            return changed;
        }
        self.write_now();
        changed
    }

    /// **Hold every write until [`Self::release_writes`]** — one gesture's worth
    /// of stores, landing as one write. See the `held` field.
    pub fn hold_writes(&mut self) {
        self.held.get_or_insert(false);
    }

    /// Let the held writes go: the document as it now stands reaches the disk
    /// once, if anything was stored while they were held.
    pub fn release_writes(&mut self) {
        if self.held.take() == Some(true) {
            self.write_now();
        }
    }

    /// Put the document in force on disk, now.
    fn write_now(&mut self) {
        if !self.writer_of_record {
            // The second Folio over this directory (review row R4-5): the choice
            // is live in this window and reaches no file. Not recorded as a
            // failure, because nothing was attempted and nothing is owed.
            return;
        }
        self.writes.record(
            SETTINGS_FILE_NAME,
            crate::hang_watch::during(crate::hang_watch::Station::SettingsWrite, || {
                write_settings_atomic(&self.path, &self.settings)
            })
            .map_err(|error| error.to_string()),
        );
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
/// a quiet window would be a window in which exactly that can be lost.
///
/// **A damaged file is reported, never repaired.** `fault` carries the sentence
/// a notice will say, and the read path leaves the file itself untouched — a
/// build that silently rewrote a shortcut file it could not parse would destroy
/// the one copy of a customisation the user could have fixed by hand. It is only
/// replaced when the user changes something, which is them choosing.
pub struct KeybindingsStore {
    path: PathBuf,
    overrides: Vec<BindingOverrideV1>,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    writes: DocumentWrites,
    /// Whether this process writes this file at all — review row R4-5.
    writer_of_record: bool,
}

impl KeybindingsStore {
    /// Read `keybindings.json`, falling back to *no overrides* on every failure.
    pub fn open() -> Self {
        let dir = storage_dir();
        let path = dir.join(KEYBINDINGS_FILE_NAME);
        let _ = std::fs::create_dir_all(&dir);
        let (file, report) = read_keybindings(&path);
        // §5.4 case 1 — no file — is the ordinary state of nearly every machine
        // and must not alert. Everything else must, naming the file (§5.3).
        let fault = read_fault(
            &report,
            KEYBINDINGS_FILE_NAME,
            crate::i18n::keybindings_file_unreadable,
        );
        Self {
            path,
            overrides: file.bindings,
            fault,
            writes: DocumentWrites::new(),
            writer_of_record: is_writer_of(&dir),
        }
    }

    /// The overrides as they were read.
    pub fn loaded(&self) -> &[BindingOverrideV1] {
        &self.overrides
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame. A write that has stopped landing is owed the same card, and is
    /// taken through the same door.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.writes.take_fault())
    }

    /// Record the new set of departures and put them on disk now.
    ///
    /// Returns whether anything changed, so a caller can skip the write when a
    /// user records the chord a row already had — and writes anyway when the last
    /// attempt did not land ([`SettingsStore::store`]'s rule, review row R4-10).
    pub fn store(&mut self, overrides: Vec<BindingOverrideV1>) -> bool {
        let changed = self.overrides != overrides;
        if !self.writes.wants_write(changed) {
            return false;
        }
        self.overrides = overrides;
        if changed {
            self.writes.rearm();
        }
        if !self.writer_of_record {
            return changed;
        }
        let file = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: self.overrides.clone(),
        };
        self.writes.record(
            KEYBINDINGS_FILE_NAME,
            crate::hang_watch::during(crate::hang_watch::Station::KeybindingsWrite, || {
                write_keybindings_atomic(&self.path, &file)
            })
            .map_err(|error| error.to_string()),
        );
        changed
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
    path: PathBuf,
    loaded: ProfilesV1,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    writes: DocumentWrites,
    /// Whether this process writes this file at all — review row R4-5.
    writer_of_record: bool,
}

/// What a re-read of `profiles.json` found — [`ProfilesStore::reread`]'s answer.
///
/// Three outcomes and not a `bool`, because the middle one is the whole reason
/// the watcher can be trusted with a file somebody is typing into: a document
/// that will not parse is neither "nothing happened" nor "here is the new
/// table".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilesNews {
    /// The file says exactly what the document already in force says. This is
    /// the ordinary answer — the folder moved because something else in it was
    /// written, or because this window wrote `profiles.json` itself.
    Unchanged,
    /// The file has been read and is now the document in force.
    Changed,
    /// The file would not parse, so nothing was taken from it and the last
    /// document that did parse is still in force.
    Unreadable,
}

impl ProfilesStore {
    /// Read `profiles.json`, falling back to *no departures* on every failure.
    pub fn open() -> Self {
        let dir = storage_dir();
        let _ = std::fs::create_dir_all(&dir);
        Self::at(dir.join(PROFILES_FILE_NAME))
    }

    /// The same store over a named file, which is what makes the re-read
    /// testable: everything below this line is about a path, and only [`open`]
    /// knows which path this process's is.
    ///
    /// [`open`]: Self::open
    fn at(path: PathBuf) -> Self {
        let (file, report) = read_profiles(&path);
        // §5.4 case 1 — no file — is the ordinary state of nearly every machine
        // and must not alert. Everything else must, naming the file (§5.3).
        let fault = read_fault(
            &report,
            PROFILES_FILE_NAME,
            crate::i18n::profiles_file_unreadable,
        );
        let writer_of_record = is_writer_of_document(&path);
        Self {
            path,
            loaded: file,
            fault,
            writes: DocumentWrites::new(),
            writer_of_record,
        }
    }

    /// The table as it was read.
    pub fn loaded(&self) -> &ProfilesV1 {
        &self.loaded
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame. A write that has stopped landing comes through the same door.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.writes.take_fault())
    }

    /// **Read the file again, because the folder moved** (§7.1.6c-6d).
    ///
    /// Three answers, and each of them is a rule this slice had to choose:
    ///
    /// * a document identical to the one in force is [`ProfilesNews::Unchanged`]
    ///   and nothing else happens. This is what makes an always-armed watch over
    ///   the *storage directory* affordable — every other file this product
    ///   writes lives in it, and this window's own writes to `profiles.json` are
    ///   the loudest of them all. Comparing the document rather than filtering
    ///   the kernel's notifications is also the only comparison that is right:
    ///   two writes with the same content are the same file, whoever made them.
    /// * a document that parses and differs is taken, and taking it is what
    ///   makes the next re-read quiet;
    /// * a document that will not parse is **not** taken, and what was already
    ///   in force stays in force. Falling back to the shipped table here would
    ///   be the worst of both — the reader's list would empty *because* they
    ///   typed a comma wrong, and they would be reading the error against a
    ///   table that is not the one they are editing. That is `reread_schemes`'s
    ///   own ruling for the scheme file in use, met one file over. Startup is
    ///   different and stays different ([`Self::open`]): there is nothing yet in
    ///   force to keep.
    ///
    /// The line printed here is §5.3's, and the card the window raises for it is
    /// the caller's — this type has no way to say anything to anybody.
    pub fn reread(&mut self) -> ProfilesNews {
        let (file, report) = read_profiles(&self.path);
        if let ReadReport::FellBackToDefaults { reason, kept } = &report {
            eprintln!("BT_PERSIST {PROFILES_FILE_NAME} would not parse: {reason:?} kept={kept:?}");
            return ProfilesNews::Unreadable;
        }
        if self.loaded == file {
            return ProfilesNews::Unchanged;
        }
        self.loaded = file;
        ProfilesNews::Changed
    }

    /// Record the table as it stands now and put it on disk.
    ///
    /// Returns whether anything changed, so a caller can skip the write when a
    /// press moved nothing.
    pub fn store(&mut self, file: ProfilesV1) -> bool {
        let changed = self.loaded != file;
        if !self.writes.wants_write(changed) {
            return false;
        }
        self.loaded = file;
        if changed {
            self.writes.rearm();
        }
        if !self.writer_of_record {
            return changed;
        }
        self.writes.record(
            PROFILES_FILE_NAME,
            crate::hang_watch::during(crate::hang_watch::Station::ProfilesWrite, || {
                write_profiles_atomic(&self.path, &self.loaded)
            })
            .map_err(|error| error.to_string()),
        );
        changed
    }
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
/// macOS — [`storage_location`] holds the rule and the reasons.
///
/// **Resolved once per process, and the move a rename owes the user happens on
/// that first call.** Three callers ask for this directory — the session store,
/// the settings store and the bash script's install path — and whichever asks
/// first is the one that pays for the relocation; the other two find it done.
/// The alternative, a relocation stapled to `main`, would leave the answer
/// depending on whether that line ran, which is exactly the kind of ordering a
/// `OnceLock` exists to remove. On a platform with no previous name there is
/// nothing to pay for and the directory is the answer straight away.
pub fn storage_dir() -> PathBuf {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
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
        })
        .clone()
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
mod tests {
    use super::*;

    /// The writer's retirement through its admitted door (`doors::SessionWriterRetire`), on a test
    /// thread already entered as the window thread on its way out.
    fn retire(writer: &mut SessionWriter) {
        admitted::<doors::SessionWriterRetire, _>(|token| writer.close(token))
            .expect("admitted on the way out");
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
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-quit-flush-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);

        // Nothing has created `root`, so `root/session.json` has nowhere to be
        // written and the atomic write's own temporary has nowhere to live.
        let mut refused = SessionStore::at(root.join("session.json"), root.join("session.lock"));
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
        landed.record(document, Instant::now());
        assert_eq!(landed.flush_judged(), Ok(()));
        assert!(root.join("session.json").is_file(), "and it is on the disk");
        // A second flush with nothing pending is honestly `Ok` and writes nothing.
        assert_eq!(landed.flush_judged(), Ok(()));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Far above any honest cost of one `atomic_write`, and reached only when the answer this is
    /// waiting for is never coming. The judgement it protects is "did that happen yet", not "has
    /// enough time passed" — see CONVENTIONS §三.
    const RECEIPT_CEILING: Duration = Duration::from_secs(60);

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
    /// failure arrives later, as a receipt, which is what puts the document back on the clock.
    ///
    /// Red gate: put `write_session_atomic` back into `hand_over` and the first assertion goes —
    /// the store is already dirty again when the call returns, because the `fsync` happened on
    /// this thread.
    #[test]
    fn the_autosave_hands_the_document_over_and_hears_the_verdict_later() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-writer-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
            !store.debouncer.is_dirty(),
            "the turn that hands the document over owes nothing more; a store that is already \
             dirty again knows the verdict, which means the fsync happened on this thread"
        );

        // And the verdict does arrive — asked as "has it happened yet", not as "have I slept
        // long enough", so a busy machine delivers the same answer later rather than a different
        // one.
        let waiting_since = Instant::now();
        while !store.debouncer.is_dirty() {
            assert!(
                waiting_since.elapsed() < RECEIPT_CEILING,
                "no receipt ever came back for a write that cannot have succeeded"
            );
            store.take_receipts(Instant::now());
            std::thread::yield_now();
        }

        store.close();
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
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-stall-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
            Err(SaveRefusal::TimedOut(save_did_not_finish())),
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
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-one-writer-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let path = root.join("session.json");

        let mut writer = SessionWriter::open();
        let started = writer
            .thread
            .as_ref()
            .expect("a writer thread")
            .thread()
            .id();

        // Two documents, in the order they were decided, down one channel.
        let first = writer.send(&path, b"first".to_vec()).expect("queued");
        let second = writer.send(&path, b"second".to_vec()).expect("queued");
        assert_eq!((first, second), (1, 2));
        let (earlier, landed) =
            admitted::<doors::SessionWriteWait, _>(|token| writer.wait_for(token, second))
                .expect("admitted on the way out");
        assert_eq!(landed, Ok(()));
        assert_eq!(
            earlier
                .iter()
                .map(|receipt| receipt.generation)
                .collect::<Vec<_>>(),
            vec![first],
            "the older document answered first, which is what one writer buys"
        );
        assert_eq!(
            std::fs::read(&path).expect("the file the writer left"),
            b"second".to_vec(),
            "and the document decided last is the one on the disk"
        );

        // Asking again is the same thread and never a second.
        assert!(writer.start(), "the writer is already running");
        assert_eq!(
            writer
                .thread
                .as_ref()
                .expect("still the same thread")
                .thread()
                .id(),
            started
        );

        // And a writer whose handle has gone — which is what a timed-out fallback used to leave
        // behind — cannot raise another beside the one that is still reading the channel.
        writer.thread = None;
        assert!(
            !writer.start(),
            "the ends are taken, so there is nothing to start a second writer on"
        );

        retire(&mut writer);
        let _ = std::fs::remove_dir_all(&root);
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
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-no-writer-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
        let (requests, incoming) = mpsc::channel::<SessionWriteRequest>();
        let (outgoing, receipts) = mpsc::channel::<SessionWriteReceipt>();
        let (release, released) = mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            // Both ends are held for as long as this thread stands, which is what makes the
            // wait on it a timeout rather than a disconnect.
            let _requests = incoming;
            let _receipts = outgoing;
            let _ = released.recv();
        });
        let writer = SessionWriter {
            requests,
            receipts,
            // Nothing left for `start` to hand a second thread, which is the state a writer
            // that is already running is in.
            ends: Arc::new(Mutex::new(None)),
            thread: Some(thread),
            sent: 0,
            landed: 0,
            stalled: false,
        };
        (writer, release)
    }

    /// PIN — **a receipt for a document that has already been replaced answers for nothing.**
    ///
    /// One writer thread means the writes land in the order they were decided, so the *last*
    /// request is the one that says what is on the disk. An older receipt arriving afterwards is
    /// news about a document that no longer exists: letting its success mark the store clean
    /// would be answering for bytes nobody wrote, and letting its failure mark the store dirty
    /// would schedule a retry of something already superseded.
    ///
    /// Red gate: drop the generation comparison in `apply_receipt` and the stale failure below
    /// puts a document back on the clock that a newer write has already carried.
    #[test]
    fn a_receipt_for_a_superseded_document_is_dropped() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-stale-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).expect("a private directory for this test");
        let mut store = SessionStore::at(root.join("session.json"), root.join("session.lock"));
        // Two documents have gone out; the second is the one that speaks for the store.
        store.writer.sent = 2;
        let now = Instant::now();

        store.apply_receipt(
            SessionWriteReceipt {
                generation: 1,
                result: Err("the volume went away".to_string()),
            },
            now,
        );
        assert!(
            !store.debouncer.is_dirty(),
            "an older document's failure is not this document's problem"
        );

        store.apply_receipt(
            SessionWriteReceipt {
                generation: 2,
                result: Err("the volume went away".to_string()),
            },
            now,
        );
        assert!(
            store.debouncer.is_dirty(),
            "the newest one's failure is, and it is what schedules the retry"
        );

        store.close();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PIN — **one writer, and it is not the window thread.**
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
        const SOURCE: &str = include_str!("persist.rs");
        let body = |head: &str| -> &'static str {
            let start = SOURCE
                .find(head)
                .unwrap_or_else(|| panic!("`{head}` is declared as written here"))
                + head.len();
            &SOURCE[start..start + SOURCE[start..].find("\n    }\n").expect("a method ends")]
        };
        assert!(
            !body("\n    pub fn flush_if_due(&mut self, now: Instant) {").contains("atomic"),
            "the turn-by-turn autosave does not touch a disk"
        );
        // Split so this test's own text is not one of the matches it counts.
        let raw = ["bt_persist::atomic_", "write("].concat();
        assert_eq!(
            SOURCE.matches(raw.as_str()).count(),
            1,
            "the raw atomic write appears once, inside the writer thread's loop"
        );
        assert!(
            SOURCE
                .find(raw.as_str())
                .is_some_and(|at| SOURCE[..at].contains("ThreadPriority::BelowNormal")),
            "and that one is downstream of the spawn that puts it on its own thread"
        );
        // And the session's own named write is not called anywhere in this module
        // (release review X-9): the store reaches the file through the writer thread's loop or
        // it does not reach it at all. `SettingsStore` and friends still write where they are
        // called, and that is a human's click rather than a per-turn autosave.
        let named = ["write_session", "_atomic("].concat();
        assert_eq!(
            SOURCE.matches(named.as_str()).count(),
            0,
            "no road to `session.json` outside the one thread that owns it"
        );
        assert!(
            !body("\n    fn hand_over(&mut self, now: Instant) {").contains("atomic"),
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
        let root =
            std::env::temp_dir().join(format!("bt-app-relocate-{case}-{}", std::process::id()));
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
        store.release_writes();
        let written = read(&path).expect("the batch is written when it is let go");
        let on_disk: SettingsV1 = serde_json::from_str(&written).unwrap();
        assert_eq!(on_disk, second, "as the document in force");
        std::fs::remove_file(&path).unwrap();
        store.release_writes();
        assert!(!path.exists(), "and a second release writes nothing more");
        assert!(store.store(SettingsV1::default()));
        assert!(path.exists(), "once let go, a store writes at once again");
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
        assert!(
            free.join(SETTINGS_FILE_NAME).is_file(),
            "a store over a folder nobody holds writes it"
        );

        let mut held_store = SettingsStore::at(held.join(SETTINGS_FILE_NAME));
        assert!(
            held_store.store(chosen),
            "the choice is live in this window"
        );
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
        let taken = try_claim(&directory).expect("asked again, the platform answers again");
        assert!(
            matches!(
                try_claim(&directory),
                Err(bt_platform::instance::ClaimRefusal::Held)
            ),
            "and a third ask is a third answer: this call's own claim now holds it"
        );

        drop(taken);
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

    /// RED (review rows R4-10 and R4-11) — **a write that keeps failing stops
    /// being retried and is said out loud.**
    ///
    /// The other half. `SessionStore` marked the document dirty again on every
    /// failure, unconditionally, so a full disk or a volume that was unplugged
    /// meant one `atomic_write` every 1.5 seconds for as long as the window
    /// stayed open — each of them leaving a temp file behind, which is
    /// `bt_persist::atomic`'s own half of R4-11 — and one line on a console
    /// nobody was watching.
    ///
    /// Red gate: mark dirty without asking `may_retry` and the store is still
    /// dirty after the bound; never fill `fault` and the card is never owed.
    #[test]
    fn a_document_that_will_not_write_stops_retrying_and_says_so() {
        // The session's waits are owner-thread doors, admitted only on the way out.
        crate::tests::on_the_window_thread_exiting();
        let root = appdata("give-up");
        let gone = root.join("not-here");
        let mut store = SessionStore::at(gone.join("session.json"), gone.join("session.lock"));

        let mut document = SessionV1::default();
        document.recent_folders.push(bt_persist::RecentFolderV1 {
            path: r"D:\work".to_owned(),
            opened_at: "2026-09-08T00:00:00Z".to_owned(),
        });
        let now = Instant::now();
        store.record(document, now);

        // Every attempt the bound allows. `flush_judged` rather than the
        // autosave's own `hand_over`, because it is the one that waits for the
        // writer thread's answer — the autosave hands a document over and hears
        // about it a turn or two later, which is a race a test may not run on.
        // The retry rule is the same either way: it lives in `report_write`.
        for _ in 0..MAX_WRITE_ATTEMPTS {
            assert!(store.debouncer.is_dirty(), "still owed");
            assert!(store.flush_judged().is_err(), "and it still cannot be kept");
        }
        assert!(
            !store.debouncer.is_dirty(),
            "past the bound the document stops going back on the clock"
        );
        let fault = store.take_fault().expect("and the reader is told");
        assert!(
            fault.contains(SESSION_FILE_NAME),
            "naming the file, so it can be acted on: {fault}"
        );
        assert!(store.take_fault().is_none(), "once, and not once a frame");

        // And a new arrangement re-arms it, because a reader who has just moved
        // something is asking for that to be kept.
        let mut moved = SessionV1::default();
        moved.recent_folders.push(bt_persist::RecentFolderV1 {
            path: r"D:\other".to_owned(),
            opened_at: "2026-09-08T00:00:01Z".to_owned(),
        });
        store.record(moved, Instant::now());
        assert!(store.debouncer.is_dirty());
        assert!(store.flush_judged().is_err());
        assert!(
            store.debouncer.is_dirty(),
            "the retrying starts again from a fresh count"
        );

        retire(&mut store.writer);
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
    /// this window*. The comparison against the document already in force is the
    /// whole of what keeps that from being a re-read that reinstalls the table
    /// under the reader's hands.
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
        assert!(matches!(store.reread(), ProfilesNews::Unchanged));

        // Written again, byte for byte — which is what an editor that saves an
        // unmodified buffer does, and what this window does on a keystroke that
        // changes nothing.
        bt_persist::write_profiles_atomic(&path, &one_profile("pwsh")).unwrap();
        assert!(matches!(store.reread(), ProfilesNews::Unchanged));
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
        assert!(matches!(store.reread(), ProfilesNews::Changed));
        assert_eq!(store.loaded().profiles[0].id, "cmd");
        assert!(
            matches!(store.reread(), ProfilesNews::Unchanged),
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
        assert!(matches!(store.reread(), ProfilesNews::Unreadable));
        assert_eq!(
            store.loaded().profiles[0].id,
            "pwsh",
            "the last document that parsed is still the one in force"
        );

        // And the way back is the file itself becoming readable again — no
        // relaunch, and no flag left set by the failure.
        bt_persist::write_profiles_atomic(&path, &one_profile("cmd")).unwrap();
        assert!(matches!(store.reread(), ProfilesNews::Changed));
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
        assert!(matches!(store.reread(), ProfilesNews::Changed));
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
        crate::tests::on_the_window_thread_exiting();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-admitted-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
        crate::tests::on_the_window_thread();
        let root = std::env::temp_dir().join(format!(
            "bt-app-session-refused-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
            Err(SaveRefusal::TimedOut(save_did_not_finish())),
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
