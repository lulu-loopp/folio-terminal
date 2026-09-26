//! `update-check.json` — **when the releases page was last asked, and what it
//! said** (`docs/DESIGN.md` §7.52).
//!
//! Four fields and no history. It is not a log of checks; it is the answer to
//! the only two questions the check has to ask itself before it runs — *is it
//! time yet*, and *has this reader already been shown this version* — plus the
//! one fact a second window needs so that it can draw the same mark without
//! asking the network again, and, since schema v2, the one version the reader
//! said never to be offered (`skipped_tag`).
//!
//! The file has one owner in `bt-app`, `update::OfferState`, which is its only
//! reader and writer and holds one lock across every read-modify-write of it.
//!
//! # Why it is not a corner of `settings.json`
//!
//! `settings.json` is the reader's file. It is small enough to open, it is
//! documented as theirs to edit, and every key in it is a decision somebody
//! made. A timestamp the program writes behind their back does not belong in
//! it — and there is a mechanical half to the argument too: two windows both
//! rewriting `settings.json` to record a network fact would race over
//! everything *else* in that file, which is a way to lose a setting.
//!
//! The switch that says whether the check happens at all is a decision, so it
//! *is* in `settings.json` (`SettingsV1::update_check`). What the check has
//! learned is bookkeeping, and it is here.
//!
//! # Why the tags are stored as they were found
//!
//! `latest_tag` and `seen_tag` hold the release's tag verbatim —
//! `v0.1.0-preview`, not `0.1.0-preview` and not a parsed triple. The reading of
//! a tag belongs to `bt_app::update`, which can change its mind about what a tag
//! means between builds; the bytes the server said cannot. Storing a parse would
//! mean a build that learned to read one more tag shape could never re-read what
//! an older build had already written down.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The schema version this build writes.
///
/// Two (0.4.6 ticket U-6): v2 adds [`UpdateCheckV1::skipped_tag`], and
/// `migrate_update_check_v1_to_v2` — the first step
/// [`crate::UPDATE_CHECK_MIGRATIONS`] has carried — writes it `null`, because a
/// v1 build had no Skip and nobody has skipped anything.
pub const UPDATE_CHECK_SCHEMA_VERSION: u32 = 2;

/// `update-check.json` — `{ "schema_version": 2, "checked_at_ms": …, … }`.
///
/// Named `V1` for the reason `SettingsV1` is at v39: the type is the document,
/// and its version is the field inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckV1 {
    pub schema_version: u32,
    /// When the releases page was last **asked**, in milliseconds since the Unix
    /// epoch. Zero means never.
    ///
    /// Asked, not answered. It advances on a refusal, a timeout and a machine
    /// with no network exactly as it advances on a `200`, and that is the whole
    /// of the no-retry-storm rule: a laptop that has been on a train all day
    /// makes one attempt, not one per window per minute.
    #[serde(default)]
    pub checked_at_ms: u64,
    /// The tag the last successful answer named, verbatim. `None` until one
    /// arrives.
    #[serde(default)]
    pub latest_tag: Option<String>,
    /// The tag whose mark this reader has already been shown.
    ///
    /// The mark on the gear is lit by `latest_tag != seen_tag`, so writing the
    /// one into the other is how the mark goes out — and why it stays out for
    /// that version and lights again for the next one.
    #[serde(default)]
    pub seen_tag: Option<String>,
    /// The tag the reader said **Skip this version** to, verbatim (schema v2).
    ///
    /// Compared by **precedence**, not equality (`bt_app::update::should_offer`):
    /// a tag at or below it is never offered again, and a tag above it is. So a
    /// withdrawn release does not bring back an older tag that is still newer
    /// than the running build. `None` until the reader skips something.
    #[serde(default)]
    pub skipped_tag: Option<String>,
    /// Top-level keys this build has no name for, kept so that a file written by
    /// a newer build survives a round trip through this one.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for UpdateCheckV1 {
    fn default() -> Self {
        Self {
            schema_version: UPDATE_CHECK_SCHEMA_VERSION,
            checked_at_ms: 0,
            latest_tag: None,
            seen_tag: None,
            skipped_tag: None,
            extra: Map::new(),
        }
    }
}
