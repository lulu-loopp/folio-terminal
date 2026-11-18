//! `release-keys.json` — **the release keys a verified statement has retired** (0.4.10 plan row
//! 102, T-UPDATES-TRUST-OUR-OWN-KEY, ruling R8).
//!
//! Every build embeds two public keys a release statement may be signed with, the live key and a
//! spare (`bt_winres::release_statement::RELEASE_KEYS`). A statement signed by the spare may
//! retire the live key — the road out of a leaked or lost live key; the live key retires nothing,
//! so a leaked live key cannot close that road — and a copy that has verified
//! such a statement must refuse the retired key from then on, across restarts, even though the
//! key is still embedded in its own executable. This file is that memory: the ids of the keys
//! retired, and nothing else.
//!
//! The ids are kept as the statement spells them (sixteen lower-case hex digits) rather than
//! parsed: the reading of an id belongs to `bt_winres`, and this crate depends on nothing that
//! knows it.
//!
//! **What it does not defend against.** Whoever can write this file can delete it, and a copy
//! that reads no file remembers nothing retired. That is the same reach that could replace the
//! executable, so the file guards the ordinary case — the next release retires a key, and every
//! copy that saw it stops trusting that key — and claims no more. A damaged file falls back to
//! an empty memory and is kept beside itself, as every document here is.
//!
//! One writer, `bt-app`'s updater, after a statement has verified; written only when a retire
//! adds an id ([`ReleaseKeysV1::remember_retired`]).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The schema version this build writes.
pub const RELEASE_KEYS_SCHEMA_VERSION: u32 = 1;

/// `release-keys.json` — `{ "schema_version": 1, "retired": ["4d8ba64ee6d3ec25"] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseKeysV1 {
    pub schema_version: u32,
    /// The ids of the keys a verified statement retired, sorted and each once. A retire is never
    /// undone: a key once retired stays refused, by this copy, for good.
    ///
    /// **Frozen across versions** (1a review P2-3): every later version of this document keeps
    /// this array under this name, an array of ids spelled as sixteen lower-case hex digits, so
    /// that a build that refuses a newer version of the file can still recover what it retired
    /// from the raw JSON rather than forget it (`DOCUMENT-KEYS.tsv`'s note on `release-keys`).
    #[serde(default)]
    pub retired: Vec<String>,
    /// Top-level keys this build has no name for, kept so that a file written by a newer build
    /// survives a round trip through this one.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ReleaseKeysV1 {
    fn default() -> Self {
        Self {
            schema_version: RELEASE_KEYS_SCHEMA_VERSION,
            retired: Vec::new(),
            extra: Map::new(),
        }
    }
}

impl ReleaseKeysV1 {
    /// **Records `ids` as retired**, keeping the list sorted and each id once, and says whether
    /// the list grew — the caller writes the file only then, so a release that retires nothing
    /// new costs no write.
    pub fn remember_retired<I, S>(&mut self, ids: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let before = self.retired.len();
        self.retired.extend(ids.into_iter().map(Into::into));
        self.retired.sort();
        self.retired.dedup();
        self.retired.len() != before
    }

    /// Whether `id` is retired.
    #[must_use]
    pub fn is_retired(&self, id: &str) -> bool {
        self.retired.iter().any(|held| held == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReadReport, read_release_keys, write_release_keys_atomic};

    /// RED (0.4.10 plan row 102) — **a retire is recorded once, survives the file, and a retire
    /// of nothing new asks for no write**: the list stays sorted with each id once, what is
    /// written reads back whole, and an unknown key a newer build wrote is carried through.
    ///
    /// MUTATION: drop the `dedup` in [`ReleaseKeysV1::remember_retired`] (`plant-102-retire-twice.py`).
    #[test]
    fn a_retire_is_recorded_once_and_survives_the_file() {
        let mut keys = ReleaseKeysV1::default();
        assert!(!keys.is_retired("4d8ba64ee6d3ec25"));
        assert!(keys.remember_retired(["4d8ba64ee6d3ec25", "2802fa4fe65b594a"]));
        assert_eq!(keys.retired, ["2802fa4fe65b594a", "4d8ba64ee6d3ec25"]);
        assert!(
            !keys.remember_retired(["4d8ba64ee6d3ec25"]),
            "nothing new is retired, so nothing is written"
        );
        assert_eq!(keys.retired.len(), 2);
        assert!(keys.is_retired("4d8ba64ee6d3ec25"));

        let dir = bt_testpath::temp_path("bt-persist-release-keys");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release-keys.json");
        let (missing, report) = read_release_keys(&path);
        assert_eq!(report, ReadReport::NotFound);
        assert_eq!(
            missing,
            ReleaseKeysV1::default(),
            "no file remembers nothing"
        );

        keys.extra
            .insert("from_a_newer_build".to_owned(), Value::Bool(true));
        write_release_keys_atomic(&path, &keys).unwrap();
        let (read, report) = read_release_keys(&path);
        assert_eq!(report, ReadReport::Loaded);
        assert_eq!(read, keys);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
