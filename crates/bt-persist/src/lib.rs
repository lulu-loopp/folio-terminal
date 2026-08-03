//! Schema v1 persistence for `settings.json`/`session.json`, per
//! `docs/M2-persistence-schema-v1.md` (the authority for every field, file,
//! and failure path here — this crate does not reinterpret it) and, for the
//! restart-time consumers of `session.json`'s `term`/`files` leaves,
//! `docs/M2-restart-shell-contract.md`.
//!
//! This is a pure library: it knows how to read/write the two JSON files
//! given explicit paths, and how to degrade gracefully when either is
//! missing, corrupt, from a future version, or internally inconsistent. It
//! does **not** know where `%APPDATA%\BetterTerminal\` is, does not spawn
//! any timer or background thread, and is not wired into `bt-app` — per the
//! implementation brief that wiring (actual storage paths, the ~1-2s
//! debounce duration, alert UI, actually calling `probe_sentinel` at
//! startup) belongs to the chrome slice that consumes this crate.
//!
//! ## Layout
//! - [`settings`] / [`SettingsV1`] — `settings.json` (§2).
//! - [`session`] / [`SessionV1`] and [`layout`] / [`LayoutNodeV1`] —
//!   `session.json` and the `split{dir,ratio}|leaf` tree it embeds (§3).
//! - [`migrate`] — the read fallback chain (§1.3/§5.4) and forward-migration
//!   scaffold.
//! - [`atomic`] — same-dir-temp-then-rename atomic writes (§5.2).
//! - [`debounce`] — a timer-free debounce state machine (§5.1); the actual
//!   1-2s duration is a caller-supplied `Duration`, not a constant here.
//! - [`write_tracker`] — the write-failure alert cadence (§5.3): one alert
//!   per failure streak, not per attempt.
//! - [`sentinel`] — the crash-vs-clean-exit sentinel file primitives (§5.5).

mod atomic;
mod debounce;
mod error;
mod layout;
mod migrate;
mod sentinel;
mod session;
mod settings;
mod write_tracker;

pub use atomic::atomic_write;
pub use debounce::Debouncer;
pub use error::WriteError;
pub use layout::{
    FilesLeafV1, LayoutNodeV1, LeafNodeV1, RATIO_PPM_MAX, SplitDirV1, SplitNodeV1, TermLeafV1,
};
pub use migrate::{
    FallbackReason, MigrationStep, ReadReport, SESSION_MIGRATIONS, SETTINGS_MIGRATIONS,
};
pub use sentinel::{ExitState, create_sentinel, probe_sentinel, remove_sentinel};
pub use session::{
    DegradationReport, RecentEntryV1, RecentSeedV1, SESSION_SCHEMA_VERSION, SessionV1, TabV1,
    WindowBoundsV1, WindowStateV1,
};
pub use settings::{SETTINGS_SCHEMA_VERSION, SettingsV1, ThemeModeV1};
pub use write_tracker::{WriteAlertAction, WriteFailureTracker};

use std::path::Path;

/// Reads `settings.json` from `path`, applying the full §5.4 fallback chain.
/// Never panics and never returns an `Err` — a failure to load is
/// represented entirely by the returned [`ReadReport`], with `SettingsV1`
/// always a usable value (defaults on any failure).
pub fn read_settings(path: &Path) -> (SettingsV1, ReadReport) {
    migrate::read_with_fallback(path, SETTINGS_SCHEMA_VERSION, SETTINGS_MIGRATIONS)
}

/// Reads `session.json` from `path`, applying the §5.4 fallback chain and
/// then the §5.4-case-3 per-leaf degradation pass (ratio clamping; unknown
/// leaf kinds are already placeholders by the time this returns — see
/// [`LeafNodeV1::Unknown`]). The [`DegradationReport`] is empty whenever the
/// file didn't load at all (there is nothing to degrade in `T::default()`)
/// and otherwise records what had to be fixed, for the caller to decide
/// whether to surface a banner.
pub fn read_session(path: &Path) -> (SessionV1, ReadReport, DegradationReport) {
    let (mut session, report) =
        migrate::read_with_fallback::<SessionV1>(path, SESSION_SCHEMA_VERSION, SESSION_MIGRATIONS);
    let degradation = session.degrade_in_place();
    (session, report, degradation)
}

/// Serializes `settings` and writes it to `path` via [`atomic_write`].
/// Pretty-printed (§1.1: "人可读可 diff" is one of the reasons `settings.json`
/// is JSON at all).
pub fn write_settings_atomic(path: &Path, settings: &SettingsV1) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(settings).map_err(|source| WriteError::Serialize {
        what: "SettingsV1",
        source,
    })?;
    atomic_write(path, &bytes)
}

/// Serializes `session` and writes it to `path` via [`atomic_write`].
/// Pretty-printed, same rationale as [`write_settings_atomic`].
pub fn write_session_atomic(path: &Path, session: &SessionV1) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(session).map_err(|source| WriteError::Serialize {
        what: "SessionV1",
        source,
    })?;
    atomic_write(path, &bytes)
}
