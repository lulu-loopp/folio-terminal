//! `pins.json` — **the one table of things the user said to keep** (§0 of
//! `docs/plans/web-preview/plan.md`, user ruling 2026-08-19).
//!
//! Three categories in one small file: a folder the root menu offers first, a
//! file the preview switcher offers first, and — written by nobody yet — a URL
//! the web block will offer when it lands. One file and not three, because "the
//! command palette will one day list all of them together" is a sentence about a
//! single table, and three files would make it a join.
//!
//! # Why the category is a string
//!
//! Every other tagged union this crate reads uses `#[serde(other)]` for the
//! variant a newer build might write — [`LeafNodeV1::Unknown`](crate::LeafNodeV1)
//! is the model, and it is the right model *there*, because a leaf this build
//! cannot draw is a placeholder pane and the tree it sits in is rewritten from
//! the live window anyway.
//!
//! Here it would lose data. This table is rewritten wholesale every time the
//! user pins anything, and `#[serde(other)]` remembers only *that* the category
//! was unknown, not what it said — so one click in an older build would silently
//! retag every pin a newer build had made. The category is therefore carried as
//! the string the file actually holds, and [`PinKind`] is a *reading* of that
//! string rather than its storage. A row this build has no name for is still a
//! row: it keeps its place, keeps its bytes, and is simply not offered anywhere.
//!
//! The same argument applies one level down, which is what [`PinEntryV1::extra`]
//! is for: fields a newer writer added inside a row it and this build both call
//! `file` survive the round trip.
//!
//! # What is *not* here
//!
//! Whether a pinned path still exists, whether a pinned URL is one the navigation
//! policy would allow, and what order the two menus draw their sections in. The
//! first is a filesystem question, the second is a policy that is checked again
//! at every navigation (§0: "钉不是授权"), and the third is `bt-app`'s. This
//! module is the wire format and nothing else.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The schema version this build writes.
pub const PINS_SCHEMA_VERSION: u32 = 1;

/// `pins.json` — `{ "schema_version": 1, "pins": [ … ] }`.
///
/// **Array order is display order**, the rule `profiles.json` already follows: a
/// newly pinned row is appended, so the PINNED section of either menu reads
/// oldest first and a re-pin does not reshuffle the list under the pointer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinsV1 {
    pub schema_version: u32,
    #[serde(default)]
    pub pins: Vec<PinEntryV1>,
    /// Top-level keys this build has no name for, kept for the reason
    /// [`PinEntryV1::extra`] is kept.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for PinsV1 {
    fn default() -> Self {
        Self {
            schema_version: PINS_SCHEMA_VERSION,
            pins: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// One pinned thing.
///
/// Two fields and no id: the target *is* the identity. Two rows naming one path
/// are one pin said twice, which is a thing the writer prevents rather than a
/// thing the reader has to reconcile — see `bt_app::pins`, which owns that rule
/// because it is the same rule the menus draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinEntryV1 {
    /// `"folder"`, `"file"`, `"url"` — or whatever a newer build wrote. Read
    /// through [`PinKind::parse`]; stored as it was found.
    pub kind: String,
    /// A path for `folder` and `file`, an absolute URL for `url`.
    ///
    /// Carried unvalidated, for [`TermLeafV1::cwd`](crate::TermLeafV1)'s reason:
    /// whether the path is still there is a filesystem question and this crate
    /// has no filesystem, and whether the URL is one this product will navigate
    /// to is a policy checked at the moment of navigating and not at the moment
    /// of reading a file.
    pub target: String,
    /// Fields of this row that this build has no name for.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl PinEntryV1 {
    /// A row this build writes: a known category and a target.
    #[must_use]
    pub fn new(kind: PinKind, target: impl Into<String>) -> Self {
        Self {
            kind: kind.tag().to_owned(),
            target: target.into(),
            extra: Map::new(),
        }
    }

    /// What this row is, if it is anything this build knows about.
    #[must_use]
    pub fn kind(&self) -> Option<PinKind> {
        PinKind::parse(&self.kind)
    }
}

/// A category this build can read.
///
/// Deliberately not the storage type — see this module's header. `Url` is parsed
/// and carried and has no surface yet: the web block (W2) is what will offer it,
/// and until then a URL pin is a row that survives every read and write this
/// build does without ever being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinKind {
    Folder,
    File,
    Url,
}

impl PinKind {
    /// The string this category is written as.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::File => "file",
            Self::Url => "url",
        }
    }

    /// Read a category, or `None` for one this build has no name for.
    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        match tag {
            "folder" => Some(Self::Folder),
            "file" => Some(Self::File),
            "url" => Some(Self::Url),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — a category this build has never heard of survives being read and
    /// written, **including the string that names it**.
    ///
    /// The bug this forbids: an older build opening a newer one's `pins.json`,
    /// pinning one folder, and writing back a file in which every `workspace`
    /// row has become something else. `#[serde(other)]` would do exactly that,
    /// which is why the category is a string here and a reading everywhere else.
    #[test]
    fn a_category_this_build_cannot_read_keeps_its_name_and_its_place() {
        let text = r#"{
            "schema_version": 1,
            "pins": [
                { "kind": "folder", "target": "C:\\work" },
                { "kind": "workspace", "target": "team", "members": [1, 2] },
                { "kind": "url", "target": "https://example.com/a?b=c#d" }
            ]
        }"#;
        let file: PinsV1 = serde_json::from_str(text).expect("a v1 file reads");
        assert_eq!(file.pins.len(), 3);
        assert_eq!(file.pins[0].kind(), Some(PinKind::Folder));
        assert_eq!(
            file.pins[1].kind(),
            None,
            "a category with no name here is read as no category, not as an error"
        );
        assert_eq!(file.pins[1].kind, "workspace", "and keeps what it said");
        assert_eq!(
            file.pins[1].extra.get("members"),
            Some(&serde_json::json!([1, 2])),
            "and keeps the fields that came with it"
        );
        // A URL pin parses. Nothing draws it in this build; it round-trips whole,
        // query and fragment included, which is the persistence policy §0 states
        // outright.
        assert_eq!(file.pins[2].kind(), Some(PinKind::Url));
        assert_eq!(file.pins[2].target, "https://example.com/a?b=c#d");

        let written: PinsV1 =
            serde_json::from_str(&serde_json::to_string(&file).expect("it serialises"))
                .expect("and reads back");
        assert_eq!(written, file, "a read-write round trip changes nothing");
    }

    /// PIN — the table this build writes says its own version, and an empty one
    /// is an empty list rather than a missing key.
    #[test]
    fn a_fresh_table_is_this_version_and_no_pins() {
        let fresh = PinsV1::default();
        assert_eq!(fresh.schema_version, PINS_SCHEMA_VERSION);
        assert!(fresh.pins.is_empty());
        let text = serde_json::to_string(&fresh).expect("it serialises");
        assert!(text.contains("\"schema_version\":1"), "{text}");
        assert!(text.contains("\"pins\":[]"), "{text}");
        assert!(
            !text.contains("extra"),
            "an empty leftover map flattens to nothing: {text}"
        );
    }

    /// PIN — every category this build knows round-trips through its own tag,
    /// and nothing else parses.
    #[test]
    fn the_three_categories_are_their_own_names() {
        for kind in [PinKind::Folder, PinKind::File, PinKind::Url] {
            assert_eq!(PinKind::parse(kind.tag()), Some(kind));
            assert_eq!(PinEntryV1::new(kind, "x").kind(), Some(kind));
        }
        assert_eq!(PinKind::parse("Folder"), None, "the tag is not case-folded");
        assert_eq!(PinKind::parse(""), None);
    }
}
