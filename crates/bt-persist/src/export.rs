//! **The settings export** — `settings.json`, `profiles.json`,
//! `keybindings.json` and the reader's colour schemes, carried in one JSON file
//! (owner rulings 2026-09-22 and 2026-09-23; `docs/DESIGN.md`, 2026-09-23).
//!
//! # The shape
//!
//! ```json
//! {
//!   "folio_export": 1,
//!   "exported_by": "Folio 0.4.4 (abc1234)",
//!   "settings": { "schema_version": 37, … },
//!   "profiles": { "schema_version": 1, … },
//!   "keybindings": { "schema_version": 1, … },
//!   "schemes": { "Nord (custom).json": { "name": "Nord (custom)", … } }
//! }
//! ```
//!
//! **One JSON document and not an archive**, and that is the ruling's own
//! reason: a file a reader can open, read and diff. No zip, and no new
//! dependency to make one (`docs/ARCHITECTURE.md` §3.3). Pretty-printed, and the
//! top level is a struct written in one fixed order, so two exports of the same
//! state are the same bytes and two exports of different states differ line by
//! line where the states differ.
//!
//! **Two kinds of version, and they answer different questions.**
//! `folio_export` is this bundle's own shape, and moves only when the bundle's
//! shape does — a part added or renamed. Each part keeps its own
//! `schema_version`, because each part *is* the document of that name: an
//! importing build reads it through [`parse_document`], the very chain the file
//! on its own path is read by, so a part from an older build is walked forward
//! by the same migrations and a part from a newer build is refused whole as a
//! hand-edited future file is. There is no second reader for any of the three.
//!
//! **What this module does not know.** Which of the reader's scheme files are
//! worth carrying, what a chord means and what a part does to a running window
//! are `bt-app`'s questions. The schemes travel as JSON values keyed by their
//! file names and are judged by `parse_scheme` on the importing side, which is
//! where a bad one is named.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::migrate::{
    BoundedRead, KEYBINDINGS_MIGRATIONS, MAX_DOCUMENT_BYTES, PROFILES_MIGRATIONS,
    SETTINGS_MIGRATIONS, parse_document, read_bounded,
};
use crate::{
    FallbackReason, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1, PROFILES_SCHEMA_VERSION, ProfilesV1,
    SETTINGS_SCHEMA_VERSION, SettingsV1, WriteError,
};

/// This bundle's own shape. Bumped only when the bundle's parts change — never
/// for a change inside one of them, which that part's own `schema_version`
/// carries.
pub const FOLIO_EXPORT_VERSION: u32 = 1;

/// The name a save dialog offers for a new export.
pub const EXPORT_FILE_NAME: &str = "folio-settings.json";

/// The parts one export is written from, borrowed from whoever holds them.
#[derive(Debug, Clone, Copy)]
pub struct ExportParts<'a> {
    /// The build that wrote the file, for a reader comparing two exports; never
    /// read back.
    pub exported_by: &'a str,
    pub settings: &'a SettingsV1,
    pub profiles: &'a ProfilesV1,
    pub keybindings: &'a KeybindingsV1,
    /// The reader's scheme files, by file name, each as the JSON it holds.
    pub schemes: &'a BTreeMap<String, Value>,
}

/// The wire order of the six keys — a struct and not a map, so the order is the
/// declaration's and not an alphabet's.
#[derive(Serialize)]
struct ExportOut<'a> {
    folio_export: u32,
    exported_by: &'a str,
    settings: &'a SettingsV1,
    profiles: &'a ProfilesV1,
    keybindings: &'a KeybindingsV1,
    schemes: &'a BTreeMap<String, Value>,
}

/// The export as bytes, ready for [`crate::atomic_write`].
pub fn serialize_export(parts: ExportParts<'_>) -> Result<Vec<u8>, WriteError> {
    let mut bytes = serde_json::to_vec_pretty(&ExportOut {
        folio_export: FOLIO_EXPORT_VERSION,
        exported_by: parts.exported_by,
        settings: parts.settings,
        profiles: parts.profiles,
        keybindings: parts.keybindings,
        schemes: parts.schemes,
    })
    .map_err(|source| WriteError::Serialize {
        what: "the settings export",
        source,
    })?;
    // A text file ends in a newline, so the last line of one export diffs
    // against the last line of another like every other line does.
    bytes.push(b'\n');
    Ok(bytes)
}

/// What an export file held, part by part.
///
/// `None` is a part the file does not carry, which an import leaves alone; a
/// part that is there and could not be read is `Some(Err(reason))`, the reason
/// being the one the same document would earn on its own path.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedParts {
    pub exported_by: Option<String>,
    pub settings: Option<Result<SettingsV1, FallbackReason>>,
    pub profiles: Option<Result<ProfilesV1, FallbackReason>>,
    pub keybindings: Option<Result<KeybindingsV1, FallbackReason>>,
    /// Each scheme as the JSON value it was exported as, by file name. Judged by
    /// `parse_scheme` on the importing side, one file at a time.
    pub schemes: Option<Result<BTreeMap<String, Value>, FallbackReason>>,
}

/// Why a file could not be read as an export at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportRefusal {
    #[error("the file could not be read: {0}")]
    Io(String),
    #[error("the file is {bytes} bytes, over the {cap}-byte limit")]
    TooLarge { bytes: u64, cap: u64 },
    #[error("the file is not JSON: {0}")]
    NotJson(String),
    /// Valid JSON with no `folio_export` number at its top.
    #[error("the file is not a Folio settings export")]
    NotAnExport,
    /// A bundle whose shape is newer than this build knows. Refused whole: which
    /// of its keys are the parts this build reads is exactly what it cannot know.
    #[error(
        "the file was written by a newer Folio (export version {found}, this build reads {current})"
    )]
    FutureExport { found: u64, current: u32 },
}

/// Read an export file off a disk — through the same bounded, accounted read
/// every document here goes through (`file_reads`' settings lane, capped at
/// [`MAX_DOCUMENT_BYTES`]) — and then [`parse_export`] it.
pub fn read_export(path: &Path) -> Result<ImportedParts, ExportRefusal> {
    let bytes = match read_bounded(path, MAX_DOCUMENT_BYTES) {
        Ok(bytes) => bytes,
        Err(BoundedRead::NotFound) => {
            return Err(ExportRefusal::Io("the file is not there".to_owned()));
        }
        Err(BoundedRead::Io(message)) => return Err(ExportRefusal::Io(message)),
        Err(BoundedRead::TooLarge { bytes }) => {
            return Err(ExportRefusal::TooLarge {
                bytes,
                cap: MAX_DOCUMENT_BYTES,
            });
        }
    };
    parse_export(&bytes)
}

/// Split an export into its parts and read each one through its own document's
/// chain ([`parse_document`], with that document's version and migrations).
pub fn parse_export(bytes: &[u8]) -> Result<ImportedParts, ExportRefusal> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| ExportRefusal::NotJson(error.to_string()))?;
    let Value::Object(mut top) = value else {
        return Err(ExportRefusal::NotAnExport);
    };
    let Some(version) = top.get("folio_export").and_then(Value::as_u64) else {
        return Err(ExportRefusal::NotAnExport);
    };
    if version > u64::from(FOLIO_EXPORT_VERSION) {
        return Err(ExportRefusal::FutureExport {
            found: version,
            current: FOLIO_EXPORT_VERSION,
        });
    }
    let exported_by = top
        .get("exported_by")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let settings = top
        .remove("settings")
        .map(|part| read_part(&part, SETTINGS_SCHEMA_VERSION, SETTINGS_MIGRATIONS));
    let profiles = top
        .remove("profiles")
        .map(|part| read_part(&part, PROFILES_SCHEMA_VERSION, PROFILES_MIGRATIONS));
    let keybindings = top
        .remove("keybindings")
        .map(|part| read_part(&part, KEYBINDINGS_SCHEMA_VERSION, KEYBINDINGS_MIGRATIONS));
    let schemes = top.remove("schemes").map(|part| match part {
        Value::Object(files) => Ok(files.into_iter().collect()),
        other => Err(FallbackReason::ParseError(format!(
            "`schemes` must be an object of files, and this one is {}",
            json_kind(&other)
        ))),
    });
    Ok(ImportedParts {
        exported_by,
        settings,
        profiles,
        keybindings,
        schemes,
    })
}

/// One part, as the bytes its own file would hold, through its own chain.
///
/// Serialized back to bytes rather than read off the value, so that the reader
/// is [`parse_document`] itself and not a second function that happens to agree
/// with it today.
fn read_part<T>(
    part: &Value,
    current: u32,
    migrations: &[(u32, crate::MigrationStep)],
) -> Result<T, FallbackReason>
where
    T: serde::de::DeserializeOwned,
{
    let bytes =
        serde_json::to_vec(part).map_err(|error| FallbackReason::ParseError(error.to_string()))?;
    parse_document(&bytes, current, migrations)
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BindingOverrideV1, LanguageV1};

    fn state() -> (
        SettingsV1,
        ProfilesV1,
        KeybindingsV1,
        BTreeMap<String, Value>,
    ) {
        let settings = SettingsV1 {
            language: LanguageV1::English,
            git_panel: false,
            terminal_font_size: 15,
            dark_scheme: "Sea (custom)".to_owned(),
            // v37's key (ticket 02), away from its default so the round trip
            // has to carry it.
            multiline_paste_ask: false,
            ..SettingsV1::default()
        };
        let profiles = ProfilesV1::default();
        let keybindings = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: vec![BindingOverrideV1 {
                action: "new-tab".to_owned(),
                chord: Some("Ctrl+Shift+Y".to_owned()),
            }],
        };
        let mut schemes = BTreeMap::new();
        schemes.insert(
            "Sea (custom).json".to_owned(),
            serde_json::json!({ "name": "Sea (custom)", "background": "#102030" }),
        );
        (settings, profiles, keybindings, schemes)
    }

    fn export_of(
        state: &(
            SettingsV1,
            ProfilesV1,
            KeybindingsV1,
            BTreeMap<String, Value>,
        ),
    ) -> Vec<u8> {
        serialize_export(ExportParts {
            exported_by: "Folio test (0000000)",
            settings: &state.0,
            profiles: &state.1,
            keybindings: &state.2,
            schemes: &state.3,
        })
        .expect("an export serializes")
    }

    /// RED (0.4.4 ticket 05) — **an export holds the four parts and its own schema
    /// version, and reading it back gives each part as it was written.**
    ///
    /// The format ruling (2026-09-23) is "ONE JSON file bundling settings +
    /// profiles + keybindings + schemes, with a schema version". A bundle that
    /// dropped a part, or read a part back as anything but the document it was,
    /// would be an export that loses something on the way to the other machine.
    ///
    /// MUTATION: leave `schemes` out of `ExportOut` and the key check goes red;
    /// read `settings` with `KEYBINDINGS_SCHEMA_VERSION` and the round trip does.
    #[test]
    fn an_export_holds_the_four_parts_and_its_own_schema_version() {
        let state = state();
        let bytes = export_of(&state);
        let wire: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            wire["folio_export"],
            Value::from(FOLIO_EXPORT_VERSION),
            "the bundle carries its own version"
        );
        for part in ["settings", "profiles", "keybindings", "schemes"] {
            assert!(wire.get(part).is_some(), "the export carries `{part}`");
        }
        assert_eq!(
            wire["settings"]["schema_version"],
            Value::from(SETTINGS_SCHEMA_VERSION),
            "and each part keeps its own"
        );
        let read = parse_export(&bytes).expect("an export reads back");
        assert_eq!(read.settings, Some(Ok(state.0)));
        assert_eq!(read.profiles, Some(Ok(state.1)));
        assert_eq!(read.keybindings, Some(Ok(state.2)));
        assert_eq!(read.schemes, Some(Ok(state.3)));
        assert_eq!(read.exported_by.as_deref(), Some("Folio test (0000000)"));
    }

    /// RED (0.4.4 ticket 05) — **two exports of the same state are byte for byte
    /// the same file.**
    ///
    /// "Readable and diffable" is the ruling's reason for JSON at all. A key order
    /// that depended on a hash, or two schemes written in whatever order a folder
    /// listed them, would make two exports of one machine differ on every line.
    ///
    /// MUTATION: make `schemes` a `HashMap` and the two exports stop agreeing.
    #[test]
    fn two_exports_of_the_same_state_are_byte_identical() {
        let state = state();
        let mut second = state.clone();
        // The same schemes, arrived at in the other order.
        let mut reversed = BTreeMap::new();
        for (file, scheme) in state.3.iter().rev() {
            reversed.insert(file.clone(), scheme.clone());
        }
        second.3 = reversed;
        let one = export_of(&state);
        let two = export_of(&second);
        assert_eq!(one, two);
        let text = String::from_utf8(one).unwrap();
        let order: Vec<usize> = [
            "\"folio_export\"",
            "\"exported_by\"",
            "\"settings\"",
            "\"profiles\"",
            "\"keybindings\"",
            "\"schemes\"",
        ]
        .iter()
        .map(|key| text.find(key).unwrap_or_else(|| panic!("{key} is written")))
        .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] < pair[1]),
            "the six keys are written in one fixed order: {order:?}"
        );
        assert!(text.ends_with("}\n"), "and the file ends in a newline");
    }

    /// RED (0.4.4 ticket 05) — **a settings part from an older build is walked
    /// forward by the same migrations a hand-edited `settings.json` is.**
    ///
    /// The part is read by `parse_document`, the chain `read_settings` uses, so a
    /// v35 document arrives at this build's version with v36's key carried
    /// forward — not parsed as-is with its old version number left in it.
    ///
    /// MUTATION: read the part with `serde_json::from_value` directly and the
    /// version stays 35.
    #[test]
    fn an_import_from_an_older_build_is_migrated_like_a_hand_edited_file() {
        let mut old = serde_json::to_value(SettingsV1 {
            git_panel: false,
            ..SettingsV1::default()
        })
        .unwrap();
        let object = old.as_object_mut().unwrap();
        object.insert("schema_version".to_owned(), Value::from(35));
        object.remove("repair_row_breaks");
        object.remove("multiline_paste_ask");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "folio_export": 1,
            "settings": old,
        }))
        .unwrap();
        let read = parse_export(&bytes).expect("an export reads");
        let settings = read
            .settings
            .expect("the part is there")
            .expect("and a v35 part is readable");
        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert!(settings.repair_row_breaks, "v36 carries the repair forward");
        assert!(
            settings.multiline_paste_ask,
            "and v37 the question before a multi-line paste"
        );
        assert!(!settings.git_panel, "and the reader's own values survive");
        assert_eq!(
            read.profiles, None,
            "a part the file does not carry is absent"
        );
    }

    /// RED (0.4.4 ticket 05) — **a part from a future build is refused whole, and
    /// the parts beside it are still read.**
    ///
    /// The file door's rule (§1.3 rule 2: never partly parse a future document),
    /// met in a bundle: one refused part is not a reason to lose the other three.
    ///
    /// MUTATION: return the first part's error from `parse_export` and the
    /// keybindings assertion goes red.
    #[test]
    fn an_import_from_a_future_build_refuses_that_part_whole_and_applies_the_others() {
        let mut future = serde_json::to_value(SettingsV1::default()).unwrap();
        future["schema_version"] = Value::from(SETTINGS_SCHEMA_VERSION + 1);
        let bytes = serde_json::to_vec(&serde_json::json!({
            "folio_export": 1,
            "settings": future,
            "keybindings": KeybindingsV1::default(),
        }))
        .unwrap();
        let read = parse_export(&bytes).expect("the bundle itself is readable");
        assert_eq!(
            read.settings,
            Some(Err(FallbackReason::FutureSchemaVersion {
                found: SETTINGS_SCHEMA_VERSION + 1,
                current: SETTINGS_SCHEMA_VERSION,
            }))
        );
        assert_eq!(read.keybindings, Some(Ok(KeybindingsV1::default())));
    }

    /// PIN — a file that is not an export is told apart from one that is, and a
    /// bundle of a newer shape is refused rather than guessed at.
    #[test]
    fn a_file_that_is_not_an_export_is_refused_with_its_reason() {
        assert!(matches!(
            parse_export(b"{ nope"),
            Err(ExportRefusal::NotJson(_))
        ));
        assert_eq!(
            parse_export(br#"{ "schema_version": 36 }"#),
            Err(ExportRefusal::NotAnExport),
            "a settings.json on its own is not an export"
        );
        assert_eq!(
            parse_export(br#"{ "folio_export": 2 }"#),
            Err(ExportRefusal::FutureExport {
                found: 2,
                current: FOLIO_EXPORT_VERSION
            })
        );
        let read = parse_export(br#"{ "folio_export": 1, "schemes": [] }"#).unwrap();
        assert!(matches!(
            read.schemes,
            Some(Err(FallbackReason::ParseError(_)))
        ));
    }

    /// The disk half runs the real bounded read over a real file.
    #[test]
    fn an_export_read_off_a_disk_is_the_export_that_was_written() {
        let state = state();
        let directory = std::env::temp_dir().join(format!(
            "bt-persist-export-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(EXPORT_FILE_NAME);
        crate::atomic_write(&path, &export_of(&state)).unwrap();
        let read = read_export(&path).expect("the file reads");
        assert_eq!(read.settings, Some(Ok(state.0)));
        assert!(matches!(
            read_export(&directory.join("absent.json")),
            Err(ExportRefusal::Io(_))
        ));
        let _ = std::fs::remove_dir_all(&directory);
    }
}
