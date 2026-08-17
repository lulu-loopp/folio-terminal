//! `keybindings.json` v1 — the shortcut table's **departures**, and nothing else.
//!
//! A file of its own beside `settings.json` (user ruling Q7 = B, 2026-08-17),
//! which is what every product that ships an editable shortcut table does, and
//! for a reason `settings.json` cannot serve: a settings document is a fixed set
//! of named preferences that this crate knows the shape of, while a shortcut
//! table is a *list* whose rows are named by the application and whose length
//! changes with every build that adds a verb. Folding it into `SettingsV1` would
//! have made the settings schema version bump every time a chord was added, and
//! §1.1's "人可读可 diff" — the whole reason these are JSON — is served far
//! better by a small file a user can open, read top to bottom, and edit.
//!
//! **Only departures are written.** A row equal to this build's default is
//! *absent*; a row the user has deliberately taken the key away from is present
//! with `"chord": null`. The two are different sentences and the file has to be
//! able to say both: an absent row takes whatever the default becomes, which is
//! what lets a later build retune a chord for everyone who never touched it,
//! while `null` is a user saying "give this key back to my shell" and must
//! outlive any such retune.
//!
//! This crate does not know what a chord *is*. `"Ctrl+Shift+N"` is an opaque
//! string here and is parsed by `bt-app`'s `shortcuts` module, which owns the
//! grammar because it owns the key types — a chord parser in this crate would
//! have to name winit's `NamedKey`, and this crate is deliberately free of the
//! window system. An id it cannot resolve and a chord it cannot read are both
//! §5.4's "逐叶降级" applied to a table: that row keeps its default and the rest
//! of the file still lands.

use serde::{Deserialize, Serialize};

/// Current `schema_version` for `keybindings.json`.
///
/// v1 is the first, and there is nothing before it: a machine with no such file
/// is a machine that has never customised a chord, which is the ordinary case
/// and not a migration.
pub const KEYBINDINGS_SCHEMA_VERSION: u32 = 1;

/// `keybindings.json` v1:
/// ```json
/// {
///   "schema_version": 1,
///   "bindings": [
///     { "action": "new-tab", "chord": "Ctrl+Shift+N" },
///     { "action": "open-search-alias", "chord": null }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingsV1 {
    pub schema_version: u32,
    /// The rows that depart from the defaults, in the table's own order.
    ///
    /// A `Vec` and not a map, because the file is meant to be read: a list keeps
    /// the rows in the order the panel shows them, and JSON objects have no
    /// order a reader can rely on. Duplicate ids are not rejected here — the
    /// reader applies them in order, so the last line about a row wins, which is
    /// the only rule a person editing by hand would guess.
    pub bindings: Vec<BindingOverrideV1>,
}

/// One departure: which row, and what it says now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingOverrideV1 {
    /// The row's stable id — `"new-tab"`, `"goto-tab-9"`, `"summon-pip-1"`.
    ///
    /// **Never an index.** The table gains and loses rows between builds, and a
    /// number here would come to mean a different verb the first time one was
    /// inserted above it — the same trap `SettingsV1::default_profile` is pinned
    /// against, met again one file over.
    ///
    /// Called `action` on the wire because that is what a person reading the
    /// file thinks it is; the application calls it an id because one action can
    /// own two rows (a chord and its alias), and then the two ids differ while
    /// the action does not.
    pub action: String,
    /// The chord, or `null` for "this row has no key".
    pub chord: Option<String>,
}

impl Default for KeybindingsV1 {
    fn default() -> Self {
        Self {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **a machine that has never customised a chord has an empty file's
    /// worth of departures**, and the emptiness is the whole document.
    ///
    /// Red gate: give `bindings` a non-empty default — every user's first read
    /// starts by overriding something nobody asked for.
    #[test]
    fn a_fresh_table_departs_from_the_defaults_nowhere() {
        let fresh = KeybindingsV1::default();
        assert_eq!(fresh.schema_version, KEYBINDINGS_SCHEMA_VERSION);
        assert!(fresh.bindings.is_empty());
    }

    /// PIN — **`null` survives the round trip as `null`**, because it is a
    /// sentence and not a missing value.
    ///
    /// A serializer that dropped the key, or a reader that read the absence as
    /// "no opinion", would silently hand a chord back to a row the user had
    /// deliberately cleared — and would do it on the next launch, long after
    /// they had stopped connecting the two.
    #[test]
    fn an_explicitly_cleared_row_is_not_the_same_as_an_absent_one() {
        let file = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: vec![
                BindingOverrideV1 {
                    action: "open-search-alias".to_owned(),
                    chord: None,
                },
                BindingOverrideV1 {
                    action: "new-tab".to_owned(),
                    chord: Some("Ctrl+Shift+Y".to_owned()),
                },
            ],
        };
        let wire = serde_json::to_value(&file).unwrap();
        assert_eq!(wire["bindings"][0]["chord"], serde_json::Value::Null);
        assert!(
            wire["bindings"][0].get("chord").is_some(),
            "the key is written even when it is null - absence means something else"
        );
        let read: KeybindingsV1 = serde_json::from_value(wire).unwrap();
        assert_eq!(read, file);
    }

    /// PIN — a row is named by a string, and the name is what comes back.
    #[test]
    fn a_row_is_named_and_never_numbered() {
        let file = KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            bindings: vec![BindingOverrideV1 {
                action: "goto-tab-9".to_owned(),
                chord: Some("Ctrl+Shift+0".to_owned()),
            }],
        };
        let wire = serde_json::to_value(&file).unwrap();
        assert!(wire["bindings"][0]["action"].is_string());
        assert_eq!(
            wire["bindings"][0]["action"],
            serde_json::Value::from("goto-tab-9")
        );
    }
}
