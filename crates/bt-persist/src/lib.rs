//! Versioned persistence for `settings.json`/`session.json`, based on
//! `docs/M2-persistence-schema-v1.md` (the authority for every field, file,
//! and failure path here — this crate does not reinterpret it) and, for the
//! restart-time consumers of `session.json`'s `term`/`files` leaves,
//! `docs/M2-restart-shell-contract.md`.
//!
//! This is a pure library: it knows how to read/write the two JSON files
//! given explicit paths, and how to degrade gracefully when either is
//! missing, corrupt, from a future version, or internally inconsistent. It
//! does **not** know where `%APPDATA%\Folio\` is, does not spawn
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
//! - [`profiles`] / [`ProfilesV1`] — `profiles.json`, the profile table's
//!   departures from the shipped five plus the user's own profiles.
//! - [`pins`] / [`PinsV1`] — `pins.json`, the one table of folders, files and
//!   URLs the user said to keep.
//! - [`update`] / [`UpdateCheckV1`] — `update-check.json`, when the releases
//!   page was last asked and what it said.
//! - [`scheme`] — one Windows Terminal colour-scheme object, parsed. The one
//!   reader here that is not versioned by this crate, because the format is
//!   somebody else's; where scheme files live and which of them exist is
//!   `bt-app`'s question, and deliberately not this crate's.

mod atomic;
mod debounce;
mod error;
mod keybindings;
mod layout;
mod migrate;
mod pins;
mod profiles;
mod scheme;
mod sentinel;
mod session;
mod settings;
mod update;
mod write_tracker;

pub use atomic::atomic_write;
pub use debounce::Debouncer;
pub use error::WriteError;
pub use keybindings::{BindingOverrideV1, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1};
pub use layout::{
    FilesLeafV1, FilesViewV1, LayoutNodeV1, LeafNodeV1, PreviewLeafV1, RATIO_PPM_MAX, SplitDirV1,
    SplitNodeV1, TermLeafV1,
};
pub use migrate::{
    FallbackReason, KEYBINDINGS_MIGRATIONS, MigrationStep, PINS_MIGRATIONS, PROFILES_MIGRATIONS,
    ReadReport, SESSION_MIGRATIONS, SETTINGS_MIGRATIONS, UPDATE_CHECK_MIGRATIONS,
};
pub use pins::{PINS_SCHEMA_VERSION, PinEntryV1, PinKind, PinsV1};
pub use profiles::{
    CandidateV1, MarkV1, NamedStartAtV1, NamedStartingDirV1, PROFILES_SCHEMA_VERSION,
    ProfileEntryV1, ProfilesV1, ProgramV1, ResolutionV1, StartAtV1, StartingDirV1,
};
pub use scheme::{SchemeFileV1, SchemeParseError, parse_scheme, write_scheme};
pub use sentinel::{ExitState, create_sentinel, probe_sentinel, remove_sentinel};
pub use session::{
    DegradationReport, GraphFilterV1, PreviewPaneV1, PreviewPoolEntryV1, PreviewSourceV1,
    QuakePlacementV1, RecentEntryV1, RecentPreviewV1, RecentSeedV1, SESSION_SCHEMA_VERSION,
    SessionCursorStyleV1, SessionSidebarModeV1, SessionTabLayoutV1, SessionThemeV1, SessionV1,
    SessionWindowV1, TabPreviewV1, TabV1, WindowBoundsV1, WindowStateV1,
};
pub use settings::{
    BackgroundFitV1, DEFAULT_BACKGROUND_IMAGE, DEFAULT_BACKGROUND_IMAGE_OPACITY,
    DEFAULT_BACKGROUND_OPACITY, DEFAULT_BLOCK_MAX_HEIGHT, DEFAULT_DARK_SCHEME,
    DEFAULT_FOCUS_CARD_HEIGHT, DEFAULT_LIGHT_SCHEME, DEFAULT_PROFILE_UNSET,
    DEFAULT_QUAKE_DISMISS_ON_BLUR, DEFAULT_QUAKE_HEIGHT, DEFAULT_QUAKE_WIDTH,
    DEFAULT_SCROLLBACK_LINES, DEFAULT_TERMINAL_FONT_FAMILY, DEFAULT_TERMINAL_FONT_SIZE, LanguageV1,
    MINIMUM_BACKGROUND_OPACITY, MINIMUM_QUAKE_HEIGHT, MINIMUM_QUAKE_WIDTH, MinimumContrastV1,
    PsReadLineInviteV1, SETTINGS_SCHEMA_VERSION, SearchEngineV1, SettingsV1, SplitDirectionV1,
    ThemeModeV1,
};
pub use update::{UPDATE_CHECK_SCHEMA_VERSION, UpdateCheckV1};
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

/// Reads `keybindings.json` from `path`, applying the same §5.4 fallback chain
/// the other two files get. A missing file is [`ReadReport::NotFound`] and is
/// the ordinary case — most machines never customise a chord — while a damaged
/// one falls back to *no overrides at all*, which leaves the caller's table at
/// this build's defaults. Never returns an `Err`: a shortcut file that cannot be
/// read must not be a terminal that will not start.
pub fn read_keybindings(path: &Path) -> (KeybindingsV1, ReadReport) {
    migrate::read_with_fallback(path, KEYBINDINGS_SCHEMA_VERSION, KEYBINDINGS_MIGRATIONS)
}

/// Serializes `keybindings` and writes it to `path` via [`atomic_write`].
/// Pretty-printed for [`write_settings_atomic`]'s reason, and rather more so:
/// this is the one file of the three a user is actively invited to hand-edit.
pub fn write_keybindings_atomic(
    path: &Path,
    keybindings: &KeybindingsV1,
) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(keybindings).map_err(|source| WriteError::Serialize {
        what: "KeybindingsV1",
        source,
    })?;
    atomic_write(path, &bytes)
}

/// Reads `profiles.json` from `path`, applying the same §5.4 fallback chain the
/// other three files get. A missing file is [`ReadReport::NotFound`] and is the
/// ordinary case — most machines never touch a profile — while a damaged one
/// falls back to *no departures at all*, which leaves the caller's table at this
/// build's shipped five. Never returns an `Err`: a profile file that cannot be
/// read must not be a terminal that will not start.
pub fn read_profiles(path: &Path) -> (ProfilesV1, ReadReport) {
    migrate::read_with_fallback(path, PROFILES_SCHEMA_VERSION, PROFILES_MIGRATIONS)
}

/// Serializes `profiles` and writes it to `path` via [`atomic_write`].
/// Pretty-printed for [`write_settings_atomic`]'s reason, and as much as
/// [`write_keybindings_atomic`] is: this is the other file a user is invited to
/// open and read top to bottom.
pub fn write_profiles_atomic(path: &Path, profiles: &ProfilesV1) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(profiles).map_err(|source| WriteError::Serialize {
        what: "ProfilesV1",
        source,
    })?;
    atomic_write(path, &bytes)
}

/// Reads `pins.json` from `path`, applying the same §5.4 fallback chain the
/// other four files get. A missing file is [`ReadReport::NotFound`] and is the
/// ordinary case — a machine where nobody has pinned anything yet — while a
/// damaged one falls back to *an empty table*, which is a menu with no PINNED
/// section rather than a window that will not start. The caller is expected to
/// say so out loud and to leave the damaged file alone until the user acts.
pub fn read_pins(path: &Path) -> (PinsV1, ReadReport) {
    migrate::read_with_fallback(path, PINS_SCHEMA_VERSION, PINS_MIGRATIONS)
}

/// Serializes `pins` and writes it to `path` via [`atomic_write`].
/// Pretty-printed for [`write_settings_atomic`]'s reason: this is a short file
/// of paths, and the shortest one there is any point opening by hand.
pub fn write_pins_atomic(path: &Path, pins: &PinsV1) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(pins).map_err(|source| WriteError::Serialize {
        what: "PinsV1",
        source,
    })?;
    atomic_write(path, &bytes)
}

/// Reads `update-check.json` from `path`, on the same §5.4 fallback chain as the
/// rest. A missing file is [`ReadReport::NotFound`] and is what every machine
/// reads before the first check; a damaged one falls back to the default, whose
/// `checked_at_ms` of zero means *ask now*. Both are the same outcome — one
/// question to the releases page — which is why neither is worth telling anybody
/// about.
pub fn read_update_check(path: &Path) -> (UpdateCheckV1, ReadReport) {
    migrate::read_with_fallback(path, UPDATE_CHECK_SCHEMA_VERSION, UPDATE_CHECK_MIGRATIONS)
}

/// Serializes `state` and writes it to `path` via [`atomic_write`].
///
/// Atomic for the reason every other document here is: two windows can reach
/// this at once, and a half-written stamp read by the second one is a check that
/// either never happens again or happens every launch.
pub fn write_update_check_atomic(path: &Path, state: &UpdateCheckV1) -> Result<(), WriteError> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|source| WriteError::Serialize {
        what: "UpdateCheckV1",
        source,
    })?;
    atomic_write(path, &bytes)
}

/// The document as bytes, without touching a disk.
///
/// **Split out of [`write_session_atomic`] because the two halves belong to two threads.**
/// Deciding what the session *is* reads the whole window tree and has to happen where that tree
/// lives; putting the bytes on a disk is an `fsync` under a `%APPDATA%` that may be redirected
/// onto a network share or a cloud-sync folder, and has no business on the thread a message pump
/// runs on. `bt-app`'s `SessionStore` calls this one on the window thread and hands the result to
/// its writer.
///
/// Pretty-printed, same rationale as [`write_settings_atomic`].
pub fn serialize_session(session: &SessionV1) -> Result<Vec<u8>, WriteError> {
    serde_json::to_vec_pretty(session).map_err(|source| WriteError::Serialize {
        what: "SessionV1",
        source,
    })
}

/// Serializes `session` and writes it to `path` via [`atomic_write`].
pub fn write_session_atomic(path: &Path, session: &SessionV1) -> Result<(), WriteError> {
    atomic_write(path, &serialize_session(session)?)
}
