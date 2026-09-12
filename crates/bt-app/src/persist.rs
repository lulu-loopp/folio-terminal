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
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use bt_persist::{
    BindingOverrideV1, Debouncer, ExitState, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1, ProfilesV1,
    ReadReport, SessionV1, SettingsV1, WriteAlertAction, WriteFailureTracker, create_sentinel,
    probe_sentinel, read_keybindings, read_profiles, read_session, read_settings, remove_sentinel,
    write_keybindings_atomic, write_profiles_atomic, write_session_atomic, write_settings_atomic,
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
pub(crate) fn is_writer_of(directory: &Path) -> bool {
    static CLAIMS: OnceLock<
        Mutex<HashMap<String, Option<bt_platform::instance::DataDirectoryClaim>>>,
    > = OnceLock::new();
    CLAIMS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("the claim table is locked to read or take one entry and nothing else")
        .entry(bt_platform::instance::claim_name(directory))
        .or_insert_with(|| bt_platform::instance::claim_data_directory(directory))
        .is_some()
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
    thread: Option<std::thread::JoinHandle<()>>,
    /// The newest request handed over. A receipt older than this is stale.
    sent: u64,
    /// The newest request a receipt has come back for. `sent > landed` is "a document is still
    /// in flight", which is the question a quit has to ask even when nothing is dirty.
    landed: u64,
}

impl SessionWriter {
    fn open() -> Self {
        let (requests, incoming) = mpsc::channel::<SessionWriteRequest>();
        let (outgoing, receipts) = mpsc::channel::<SessionWriteReceipt>();
        // In the workers' band (§1.4): a session write must never be the reason a frame was late,
        // and it is never the thing anybody is waiting to see.
        let thread = bt_platform::spawn_at_priority(
            "session-writer",
            bt_platform::ThreadPriority::BelowNormal,
            move || {
                while let Ok(request) = incoming.recv() {
                    let result = bt_persist::atomic_write(&request.path, &request.bytes)
                        .map_err(|error| error.to_string());
                    if outgoing
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
        Self {
            requests,
            receipts,
            thread,
            sent: 0,
            landed: 0,
        }
    }

    /// The newest document still on its way to the disk, if one is.
    fn in_flight(&self) -> Option<u64> {
        (self.sent > self.landed).then_some(self.sent)
    }

    /// Hand one document over. Answers the generation it was filed under.
    ///
    /// A writer thread that could not be started (`spawn_at_priority` failed) leaves this
    /// returning `None`: there is nowhere to send, and saying so is what lets the caller write it
    /// here rather than pretend it was queued.
    fn send(&mut self, path: &Path, bytes: Vec<u8>) -> Option<u64> {
        self.thread.as_ref()?;
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
    fn wait_for(&self, generation: u64) -> (Vec<SessionWriteReceipt>, Result<(), String>) {
        let mut earlier = Vec::new();
        loop {
            match self.receipts.recv() {
                Ok(receipt) if receipt.generation == generation => {
                    return (earlier, receipt.result);
                }
                Ok(receipt) => earlier.push(receipt),
                // The thread is gone and the answer is never coming. Say so as a failure rather
                // than as a wait: a quit that hangs here is worse than a quit that reports.
                Err(_) => {
                    return (
                        earlier,
                        Err(
                            "the session writer stopped before this document reached the disk"
                                .to_string(),
                        ),
                    );
                }
            }
        }
    }

    /// Let the thread finish what is queued and end. Idempotent.
    fn close(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        // Dropping the sender is what ends the thread's `recv` loop. It is replaced rather than
        // dropped outright so the struct stays whole; nothing sends after this because `send`
        // reads `thread` first.
        let (dead, _) = mpsc::channel();
        let live = std::mem::replace(&mut self.requests, dead);
        drop(live);
        // Bounded by construction: everything queued is already decided, and each item is one
        // atomic write. Waiting here is the process's last act, and the alternative — walking out
        // with a write in flight — is the session file half written.
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
        self.debouncer
            .is_dirty()
            .then(|| Instant::now() + SESSION_DEBOUNCE)
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
        // No writer thread — `spawn_at_priority` refused at startup, or it has been closed. The
        // honest fallback is this thread, because the alternative is a session that is silently
        // never written at all.
        let landed = write_session_atomic(&self.session_path, &self.session)
            .map_err(|error| error.to_string());
        if landed.is_ok() {
            self.debouncer.mark_flushed();
        }
        self.report_write(landed, now);
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
    pub fn flush_judged(&mut self) -> Result<(), String> {
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
            error
        })?;
        let Some(generation) = self.writer.send(&self.session_path, bytes) else {
            // No writer thread. This one does it, and answers for it.
            let landed = write_session_atomic(&self.session_path, &self.session)
                .map_err(|error| error.to_string());
            if landed.is_ok() {
                self.debouncer.mark_flushed();
            }
            self.report_write(landed.clone(), now);
            return landed;
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
    fn wait_for_landing(&mut self, generation: u64, now: Instant) -> Result<(), String> {
        let (earlier, landed) = self.writer.wait_for(generation);
        for receipt in earlier {
            self.apply_receipt(receipt, now);
        }
        self.apply_receipt(
            SessionWriteReceipt {
                generation,
                result: landed.clone(),
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
    pub fn close(&mut self) {
        self.flush();
        self.writer.close();
        if self.armed {
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
        if !self.writer_of_record {
            // The second Folio over this directory (review row R4-5): the choice
            // is live in this window and reaches no file. Not recorded as a
            // failure, because nothing was attempted and nothing is owed.
            return changed;
        }
        self.writes.record(
            SETTINGS_FILE_NAME,
            write_settings_atomic(&self.path, &self.settings).map_err(|error| error.to_string()),
        );
        changed
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
            write_keybindings_atomic(&self.path, &file).map_err(|error| error.to_string()),
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
            write_profiles_atomic(&self.path, &self.loaded).map_err(|error| error.to_string()),
        );
        changed
    }
}

/// The directory this build wrote its files under before the product was named,
/// and the only reason this module knows the old brand at all.
///
/// It exists for one startup, on one machine, once: see [`relocate`].
const PREVIOUS_STORAGE_NAME: &str = "BetterTerminal";

/// The directory the product writes under, which is its name.
const STORAGE_NAME: &str = "Folio";

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
    /// Mutation: call `write_session_atomic` from `flush_if_due` or `hand_over`'s ordinary path
    /// and the first assertion names it.
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
        // The two window-thread fallbacks are the ones with no thread to hand to, plus the
        // `SettingsStore` and friends, which are a human's click rather than a per-turn autosave.
        assert_eq!(
            body("\n    fn hand_over(&mut self, now: Instant) {")
                .matches("write_session_atomic(")
                .count(),
            1,
            "`hand_over` writes here only when there is no writer thread to write for it"
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

        store.writer.close();
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
}
