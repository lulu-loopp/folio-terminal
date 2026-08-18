//! `profiles.json` v1 — the profile table's **departures from the shipped
//! five**, plus whatever profiles the user has made of their own.
//!
//! A file of its own beside `settings.json` (user ruling 2026-08-17, Q1), for
//! the reason `keybindings.json` was given one a slice earlier: `settings.json`
//! is a fixed record of named scalars, every one of which is rewritten whole the
//! moment a switch is thrown, and a profile table is a *list a person may edit
//! by hand*. Folding a hand-edited list into a file that is rewritten on every
//! toggle would give every toggle a race with the hand.
//!
//! It is equally **not** a folder of one-file-per-profile like `schemes\`. A
//! colour scheme is portable — copied out of somebody's gist and dropped in —
//! while a profile names `%ProgramFiles%\Git\bin\bash.exe` *on this machine*.
//! More decisively: **order and uniqueness are properties of the set, not of any
//! member**, and a folder has neither. And "the built-in `cmd` is hidden" is not
//! a fact that can live in `cmd`'s own file, because `cmd` has no file.
//!
//! ## The array is the order
//!
//! There is no `order` key. Two places saying the same thing drift, and the
//! obvious reading of a JSON list is the order it is written in — which is also
//! the only reading a person hand-editing the file would guess.
//!
//! ## A built-in writes only its differences
//!
//! An entry that is nothing but `{ "id": "pwsh" }` means "the shipped profile,
//! unchanged, in this position". Every other key is an override of one field.
//! That is what lets a later build retune a built-in's arguments for everybody
//! who never touched them, and it is the same sentence `keybindings.json` writes
//! about a chord nobody rebound.
//!
//! This crate does not know what a `ChromeMark`, a `ProgramSource` or an
//! `Integration` *is* — those are `bt-app`'s types, and it owns the merge of
//! this file onto its own shipped table. What lives here is the wire shape and
//! nothing else, which is the same division `keybindings.json` keeps against the
//! chord grammar.
//!
//! ## Nothing watches this file
//!
//! Read once at startup; a hand edit needs a relaunch. `schemes\`'s directory
//! watch exists because "save and see it" is that slice's product promise, and
//! a profile table makes no such promise. The consequence is booked rather than
//! papered over: a person hand-editing this file while Folio is open will have
//! their edit overwritten by the next write from the dialog.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Current `schema_version` for `profiles.json`.
///
/// v1 is the first and there is nothing before it: a machine with no such file
/// is a machine that has never touched a profile, which is the ordinary case and
/// not a migration.
pub const PROFILES_SCHEMA_VERSION: u32 = 1;

/// `profiles.json` v1:
/// ```json
/// {
///   "schema_version": 1,
///   "profiles": [
///     { "id": "pwsh" },
///     { "id": "wsl", "display_title": "Ubuntu" },
///     { "id": "cmd", "hidden": true },
///     { "id": "claude-7f3a", "display_title": "Claude",
///       "program": "C:\\Users\\me\\.local\\bin\\claude.exe",
///       "env": { "FORCE_HYPERLINK": "1" } }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesV1 {
    pub schema_version: u32,
    /// The whole table, in the order it is shown.
    ///
    /// A `Vec` and not a map for `keybindings.json`'s reason and one more of its
    /// own: here the order *is* data. Built-in ids the file does not mention are
    /// appended by the reader in shipped order — that is the only honest answer
    /// to "a later build added a sixth built-in": it appears, at the end, not
    /// hidden.
    pub profiles: Vec<ProfileEntryV1>,
}

impl Default for ProfilesV1 {
    fn default() -> Self {
        Self {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: Vec::new(),
        }
    }
}

/// One row of the table: which profile, and everything about it that is not the
/// shipped answer.
///
/// Every field but `id` is optional, and an absent field is *not* the same
/// sentence as a field set to the shipped value: absent means "whatever this
/// build ships", which is what lets the shipped value change. A profile of the
/// user's own has no shipped anything, so its entry carries every field it needs
/// — at minimum a `program`, without which it is a row that cannot start and is
/// dropped by the reader with a notice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntryV1 {
    /// The stable id — `"pwsh"` for a built-in, `"claude-7f3a"` for one of the
    /// user's own.
    ///
    /// **Never an index and never the title.** A title is a display object and
    /// gets renamed; renaming is in fact the commonest edit there is, and an
    /// identity that changed with it would strand every seed on disk that names
    /// this profile. The five built-in slugs are reserved words a user profile
    /// may not take.
    pub id: String,
    /// The name every surface draws, when it is not the shipped one.
    ///
    /// Deliberately *not* the string the OSC 0/2 comparison uses: a built-in
    /// keeps its shipped title invisibly, because that is the word its
    /// integration script announces, and a pane head drops an announcement that
    /// merely repeats the launcher. See `bt-app`'s `profiles::Profile`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// Kept out of the pickers. Built-ins are hidden, never deleted — a row that
    /// is missing looks exactly like a row that was never designed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Which executable a new tab of this profile starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramV1>,
    /// The arguments handed to it, ahead of anything the shell reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Set for this profile's sessions, over what the terminal sets.
    ///
    /// A JSON object and therefore a `BTreeMap`: an environment is a mapping,
    /// duplicate names are not a sentence anybody means, and sorting makes the
    /// file's bytes a function of its content rather than of the order a dialog
    /// happened to build it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// Where a tab of this profile opens when nothing is inherited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_dir: Option<StartingDirV1>,
    /// The mark that names this profile across the window.
    ///
    /// A bare string here, naming one of the shipped marks, because in this
    /// slice the only way to make a profile is to duplicate a built-in and a
    /// duplicate inherits the mark it really is. The editor's eight struck
    /// colours are the next slice's, and they arrive as an *object* form beside
    /// this one rather than by redefining it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
    /// Which shell-integration script serves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<String>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if hands the field by reference"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// How a profile's executable is found.
///
/// A bare string is the everyday case and the hand-editable one — it is a path,
/// and it is what the editor writes when somebody types or browses to a program.
/// The object forms exist so that a *duplicate of a built-in* can carry the
/// built-in's own resolution rather than being frozen to whatever path happened
/// to resolve on the day it was copied: `pwsh` follows `BT_SHELL` and then a
/// probe, and a copy of `pwsh` that stopped doing so would quietly stop being a
/// copy the first time the original moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgramV1 {
    /// `"C:\\Users\\me\\.local\\bin\\claude.exe"` — this program, at this path.
    Path(String),
    /// One of the shipped resolutions, named.
    Resolution(ResolutionV1),
}

/// The object forms of [`ProgramV1`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolutionV1 {
    /// `BT_SHELL`, then a `pwsh` probe — `bt_pty::resolve_powershell_seven`.
    ///
    /// Named on the wire rather than left to the `snake_case` rule, which would
    /// have spelled it `power_shell_seven` — a word nobody hand-editing this
    /// file would type and nobody reading it would recognise.
    #[serde(rename = "powershell7")]
    PowerShellSeven,
    /// The first of these candidates that is a real file, in order.
    FirstOf { candidates: Vec<CandidateV1> },
}

/// One place to look for an executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateV1 {
    /// `%variable%\tail`.
    Under { variable: String, tail: String },
    /// Find `anchor` on `PATH`, climb out of the directory holding it, take
    /// `tail` from there.
    BesideOnPath { anchor: String, tail: String },
}

/// Where a profile's shell stands when it is not told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StartingDirV1 {
    /// `"windows_home"` — `%USERPROFILE%`, handed over as a working directory.
    Named(NamedStartingDirV1),
    /// The place is named to the *launcher* instead, as a flag and one argument,
    /// because the shell does not stand where the launcher does.
    LauncherFlag { flag: String, home: String },
}

/// The string forms of [`StartingDirV1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedStartingDirV1 {
    WindowsHome,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **a machine that has never touched a profile has an empty file's
    /// worth of departures**, and the emptiness is the whole document.
    ///
    /// Red gate: give `profiles` a non-empty default — every reader would start
    /// by overriding a table nobody asked to override, and this crate would have
    /// asserted a fact about `bt-app`'s shipped five that it has no business
    /// knowing.
    #[test]
    fn a_fresh_table_departs_from_the_shipped_five_nowhere() {
        let fresh = ProfilesV1::default();
        assert_eq!(fresh.schema_version, PROFILES_SCHEMA_VERSION);
        assert!(fresh.profiles.is_empty());
    }

    /// PIN — **an untouched built-in writes exactly one key**.
    ///
    /// The whole "only differences" contract is visible in the bytes: an entry
    /// that says nothing but its id is an entry taking whatever this build
    /// ships. A serializer that spelled out `"hidden": false` and
    /// `"display_title": null` beside it would turn every row into a snapshot of
    /// this build's defaults, and the next build's retune would never reach
    /// anybody.
    #[test]
    fn an_untouched_builtin_is_its_id_and_nothing_else() {
        let file = ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: vec![ProfileEntryV1 {
                id: "pwsh".to_owned(),
                ..ProfileEntryV1::default()
            }],
        };
        let wire = serde_json::to_value(&file).unwrap();
        let row = wire["profiles"][0].as_object().unwrap();
        assert_eq!(row.len(), 1, "one key, and it is the id: {row:?}");
        assert_eq!(row["id"], serde_json::Value::from("pwsh"));
    }

    /// PIN — every field survives the round trip, in the order the array gave
    /// them.
    #[test]
    fn a_profile_of_the_users_own_round_trips_whole() {
        let file = ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: vec![
                ProfileEntryV1 {
                    id: "cmd".to_owned(),
                    hidden: true,
                    ..ProfileEntryV1::default()
                },
                ProfileEntryV1 {
                    id: "claude-7f3a".to_owned(),
                    display_title: Some("Claude".to_owned()),
                    program: Some(ProgramV1::Path(
                        r"C:\Users\me\.local\bin\claude.exe".to_owned(),
                    )),
                    args: Some(vec!["--dangerously".to_owned()]),
                    env: Some(BTreeMap::from([(
                        "FORCE_HYPERLINK".to_owned(),
                        "1".to_owned(),
                    )])),
                    starting_dir: Some(StartingDirV1::Named(NamedStartingDirV1::WindowsHome)),
                    mark: Some("powershell".to_owned()),
                    integration: Some("powershell_opt_in".to_owned()),
                    hidden: false,
                },
            ],
        };
        let text = serde_json::to_string_pretty(&file).unwrap();
        let read: ProfilesV1 = serde_json::from_str(&text).unwrap();
        assert_eq!(read, file);
    }

    /// PIN — **a bare string in `program` is a path**, because that is what a
    /// person hand-editing this file writes, and the plan's own worked example
    /// writes it that way.
    #[test]
    fn a_program_written_as_a_string_reads_as_a_path() {
        let read: ProfileEntryV1 =
            serde_json::from_str(r#"{ "id": "x", "program": "C:\\bin\\x.exe" }"#).unwrap();
        assert_eq!(
            read.program,
            Some(ProgramV1::Path(r"C:\bin\x.exe".to_owned()))
        );
    }

    /// PIN — a duplicate of a built-in carries the built-in's *resolution*, not
    /// a path frozen on the day it was copied.
    #[test]
    fn a_named_resolution_round_trips_as_an_object() {
        let source = ProgramV1::Resolution(ResolutionV1::FirstOf {
            candidates: vec![
                CandidateV1::BesideOnPath {
                    anchor: "git.exe".to_owned(),
                    tail: r"bin\bash.exe".to_owned(),
                },
                CandidateV1::Under {
                    variable: "ProgramFiles".to_owned(),
                    tail: r"Git\bin\bash.exe".to_owned(),
                },
            ],
        });
        let wire = serde_json::to_value(&source).unwrap();
        assert_eq!(wire["kind"], serde_json::Value::from("first_of"));
        let read: ProgramV1 = serde_json::from_value(wire).unwrap();
        assert_eq!(read, source);

        let seven = ProgramV1::Resolution(ResolutionV1::PowerShellSeven);
        let wire = serde_json::to_value(&seven).unwrap();
        assert_eq!(wire["kind"], serde_json::Value::from("powershell7"));
        assert_eq!(
            serde_json::from_value::<ProgramV1>(wire).unwrap(),
            seven,
            "the shipped resolutions are named, not spelled out as a path"
        );
    }

    /// PIN — the environment is written as an object and comes back sorted, so
    /// the file's bytes are a function of its content and a diff of two writes
    /// shows only what changed.
    #[test]
    fn the_environment_is_an_object_and_its_bytes_do_not_depend_on_insertion_order() {
        let one = ProfileEntryV1 {
            id: "x".to_owned(),
            env: Some(BTreeMap::from([
                ("B".to_owned(), "2".to_owned()),
                ("A".to_owned(), "1".to_owned()),
            ])),
            ..ProfileEntryV1::default()
        };
        let other = ProfileEntryV1 {
            env: Some(BTreeMap::from([
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned()),
            ])),
            ..one.clone()
        };
        let text = serde_json::to_string(&one).unwrap();
        assert_eq!(text, serde_json::to_string(&other).unwrap());
        assert!(text.contains(r#""env":{"A":"1","B":"2"}"#), "{text}");
    }
}
