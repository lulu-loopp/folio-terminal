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
use std::time::{Duration, Instant};

use bt_persist::{
    Debouncer, ExitState, ReadReport, SessionV1, SettingsV1, WriteAlertAction, WriteFailureTracker,
    create_sentinel, probe_sentinel, read_session, read_settings, remove_sentinel,
    write_session_atomic, write_settings_atomic,
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
