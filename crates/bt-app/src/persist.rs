//! Where `session.json` lives, when it is written, and what a leftover
//! sentinel means.
//!
//! `bt-persist` is deliberately timer-free and path-free: it knows how to read
//! and write two JSON documents given explicit paths, and how to degrade when
//! either is missing or damaged. Everything it left to "the chrome slice that
//! consumes this crate" is here — the actual storage directory, the debounce
//! duration, when `mark_dirty` fires, and who calls `probe_sentinel` at
//! startup.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bt_persist::{
    Debouncer, ExitState, ReadReport, SessionV1, WriteAlertAction, WriteFailureTracker,
    create_sentinel, probe_sentinel, read_session, remove_sentinel, write_session_atomic,
};

/// docs/M2-persistence-schema-v1.md §5.1 rules "debounce roughly 1-2 seconds
/// after a meaningful change", and leaves the exact figure to the call site.
/// The slower end of the band: a session write is never urgent, and every
/// hundred milliseconds spent waiting is a divider drag that does not turn into
/// its own disk write.
const SESSION_DEBOUNCE: Duration = Duration::from_millis(1_500);

/// The session file, its sentinel, and the debounce that stands between a
/// change and the disk.
pub struct SessionStore {
    session_path: PathBuf,
    sentinel_path: PathBuf,
    session: SessionV1,
    debouncer: Debouncer,
    failures: WriteFailureTracker,
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
            armed,
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

    /// Write if the quiet window has elapsed.
    pub fn flush_if_due(&mut self, now: Instant) {
        if self.debouncer.should_flush(now, SESSION_DEBOUNCE) {
            self.flush();
        }
    }

    /// Write now, whatever the debounce says. The clean-exit path uses this:
    /// a pending change must not be lost because the window closed 300ms after
    /// it happened.
    pub fn flush(&mut self) {
        if !self.debouncer.is_dirty() {
            return;
        }
        let result = write_session_atomic(&self.session_path, &self.session);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt. A full
            // disk must not turn into a message every 1.5 seconds.
            eprintln!("BT_PERSIST could not write session.json: {error}");
        }
        if result.is_ok() {
            self.debouncer.mark_flushed();
        }
    }

    /// Flush anything pending and drop this run's sentinel. Idempotent.
    pub fn close(&mut self) {
        self.flush();
        if self.armed {
            let _ = remove_sentinel(&self.sentinel_path);
            self.armed = false;
        }
    }
}

/// `%APPDATA%\BetterTerminal\` (§1.2). Falls back to the process temp
/// directory when the environment has no `APPDATA` — the same reasoning as the
/// panic log's: a diagnostic that cannot be written is worse than one written
/// somewhere less convenient.
fn storage_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => Path::new(&appdata).join("BetterTerminal"),
        None => std::env::temp_dir().join("BetterTerminal"),
    }
}
