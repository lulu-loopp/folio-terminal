//! Where `session.json` and `settings.json` live, when they are written, and
//! what a leftover sentinel means.
//!
//! `bt-persist` is deliberately timer-free and path-free: it knows how to read
//! and write two JSON documents given explicit paths, and how to degrade when
//! either is missing or damaged. Everything it left to "the chrome slice that
//! consumes this crate" is here — the actual storage directory, the debounce
//! duration, when `mark_dirty` fires, and who calls `probe_sentinel` at
//! startup.

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use bt_persist::{
    BindingOverrideV1, Debouncer, ExitState, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1, ProfilesV1,
    ReadReport, SessionV1, SettingsV1, WriteAlertAction, WriteFailureTracker, create_sentinel,
    probe_sentinel, read_keybindings, read_profiles, read_session, read_settings, remove_sentinel,
    write_keybindings_atomic, write_profiles_atomic, write_session_atomic, write_settings_atomic,
};

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
    failures: WriteFailureTracker,
    writer: SessionWriter,
    /// True once the sentinel for *this* run exists, so a clean exit knows
    /// there is something to remove.
    armed: bool,
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
        let session_path = dir.join("session.json");
        let sentinel_path = dir.join("session.lock");
        let writable = std::fs::create_dir_all(&dir).is_ok();
        // Probe *before* creating: creating first would make every probe after
        // the first report a crash.
        let previous_exit = probe_sentinel(&sentinel_path).unwrap_or(ExitState::Normal);
        let (session, report, degradation) = read_session(&session_path);
        // §5.4 case 1 — no file yet — is the normal first run and must not
        // alert; every other non-`Loaded` outcome must (§5.3: "explicit alert,
        // never pretend it succeeded").
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST session.json fell back to defaults: {reason:?}");
        }
        if !degradation.is_clean() {
            eprintln!(
                "BT_PERSIST session.json degraded: {} clamped ratios, {} unknown leaves",
                degradation.clamped_ratios, degradation.unknown_leaves
            );
        }
        if previous_exit == ExitState::Crashed {
            eprintln!("BT_PERSIST previous session did not reach its clean-exit path");
        }
        let armed = writable && create_sentinel(&sentinel_path).is_ok();
        Self {
            session_path,
            sentinel_path,
            session,
            debouncer: Debouncer::new(),
            failures: WriteFailureTracker::new(),
            writer: SessionWriter::open(),
            armed,
        }
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
        Self {
            session_path,
            sentinel_path,
            session: SessionV1::default(),
            debouncer: Debouncer::new(),
            failures: WriteFailureTracker::new(),
            writer: SessionWriter::open(),
            armed: false,
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

    /// Hand the current document to the writer, without waiting for it to land.
    ///
    /// The debounce is marked flushed here rather than at the receipt, and the two are different
    /// questions: the debouncer answers "when should this be tried again", and handing it over is
    /// exactly the moment the answer becomes "not until something changes". Whether it *landed*
    /// is [`Self::take_receipts`]'s, and a failure there marks the document dirty again so the
    /// next quiet window retries it.
    fn hand_over(&mut self, now: Instant) {
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

    /// One alert per failure streak (§5.3), and a failure leaves the document owed.
    fn report_write(&mut self, result: Result<(), String>, now: Instant) {
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt. A full
            // disk must not turn into a message every 1.5 seconds.
            eprintln!("BT_PERSIST could not write session.json: {error}");
        }
        if result.is_err() {
            self.debouncer.mark_dirty(now);
        }
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
    failures: WriteFailureTracker,
}

impl SettingsStore {
    /// Read `settings.json`, falling back to defaults on every failure — same
    /// contract as [`SessionStore::open`], and for the same reason: a terminal
    /// that refuses to start because it could not read a preferences file would
    /// be a worse product than one that starts with the default preferences.
    pub fn open() -> Self {
        let dir = storage_dir();
        let path = dir.join("settings.json");
        let _ = std::fs::create_dir_all(&dir);
        let (settings, report) = read_settings(&path);
        // §5.4 case 1 — no file yet — is the normal first run and must not alert.
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST settings.json fell back to defaults: {reason:?}");
        }
        Self {
            path,
            settings,
            failures: WriteFailureTracker::new(),
        }
    }

    /// The settings as they currently stand.
    pub fn loaded(&self) -> &SettingsV1 {
        &self.settings
    }

    /// Record a change and put it on disk now. Returns whether anything changed,
    /// so a caller can skip the repaint when a user picks the value already set.
    pub fn store(&mut self, settings: SettingsV1) -> bool {
        if self.settings == settings {
            return false;
        }
        self.settings = settings;
        let result = write_settings_atomic(&self.path, &self.settings);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write settings.json: {error}");
        }
        true
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
    failures: WriteFailureTracker,
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
        let fault = match &report {
            ReadReport::FellBackToDefaults { reason } => {
                eprintln!("BT_PERSIST {KEYBINDINGS_FILE_NAME} fell back to defaults: {reason:?}");
                Some(crate::i18n::keybindings_file_unreadable(
                    KEYBINDINGS_FILE_NAME,
                ))
            }
            ReadReport::NotFound | ReadReport::Loaded => None,
        };
        Self {
            path,
            overrides: file.bindings,
            fault,
            failures: WriteFailureTracker::new(),
        }
    }

    /// The overrides as they were read.
    pub fn loaded(&self) -> &[BindingOverrideV1] {
        &self.overrides
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
    }

    /// Record the new set of departures and put them on disk now.
    ///
    /// Returns whether anything changed, so a caller can skip the write when a
    /// user records the chord a row already had.
    pub fn store(&mut self, overrides: Vec<BindingOverrideV1>) -> bool {
        if self.overrides == overrides {
            return false;
        }
        self.overrides = overrides;
        let file = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: self.overrides.clone(),
        };
        let result = write_keybindings_atomic(&self.path, &file);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write {KEYBINDINGS_FILE_NAME}: {error}");
        }
        true
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
    failures: WriteFailureTracker,
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
        let fault = match &report {
            ReadReport::FellBackToDefaults { reason } => {
                eprintln!("BT_PERSIST {PROFILES_FILE_NAME} fell back to defaults: {reason:?}");
                Some(crate::i18n::profiles_file_unreadable(PROFILES_FILE_NAME))
            }
            ReadReport::NotFound | ReadReport::Loaded => None,
        };
        Self {
            path,
            loaded: file,
            fault,
            failures: WriteFailureTracker::new(),
        }
    }

    /// The table as it was read.
    pub fn loaded(&self) -> &ProfilesV1 {
        &self.loaded
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
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
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST {PROFILES_FILE_NAME} would not parse: {reason:?}");
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
        if self.loaded == file {
            return false;
        }
        self.loaded = file;
        let result = write_profiles_atomic(&self.path, &self.loaded);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write {PROFILES_FILE_NAME}: {error}");
        }
        true
    }
}

/// The directory this build wrote its files under before the product was named,
/// and the only reason this module knows the old brand at all.
///
/// It exists for one startup, on one machine, once: see [`relocate`].
const PREVIOUS_STORAGE_NAME: &str = "BetterTerminal";

/// The directory the product writes under, which is its name.
const STORAGE_NAME: &str = "Folio";

/// `%APPDATA%\Folio\` (§1.2). Falls back to the process temp directory when the
/// environment has no `APPDATA` — the same reasoning as the panic log's: a
/// diagnostic that cannot be written is worse than one written somewhere less
/// convenient.
///
/// **Resolved once per process, and the move a rename owes the user happens on
/// that first call.** Three callers ask for this directory — the session store,
/// the settings store and the bash script's install path — and whichever asks
/// first is the one that pays for the relocation; the other two find it done.
/// The alternative, a relocation stapled to `main`, would leave the answer
/// depending on whether that line ran, which is exactly the kind of ordering a
/// `OnceLock` exists to remove.
pub fn storage_dir() -> PathBuf {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            let root = match std::env::var_os("APPDATA") {
                Some(appdata) => PathBuf::from(appdata),
                None => std::env::temp_dir(),
            };
            let current = root.join(STORAGE_NAME);
            let previous = root.join(PREVIOUS_STORAGE_NAME);
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
