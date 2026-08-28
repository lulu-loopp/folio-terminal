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
//! ## This file is watched, and the hand wins the field it is typing in
//!
//! It was not, and the sentence here said why: `schemes\` is watched because
//! "save and see it" is that slice's product promise, and a profile table made
//! no such promise. What retired that reasoning is the shape of the promise
//! rather than its strength — a file this crate's own header calls *a list a
//! person may edit by hand* is a file somebody will edit by hand, and the
//! version of that which needs a relaunch is one where the dialog silently
//! overwrites their work.
//!
//! So `bt-app` watches `%APPDATA%\Folio\` and re-reads this file when it stops
//! moving (§7.1.6c-6d). What that means for the two writers:
//!
//! * **a document identical to the one in force is not news**, which is what
//!   lets a window watch a folder it writes into itself;
//! * **a document that will not parse is not taken**, and the table already in
//!   force stays in force — the reader is looking at their own half-typed JSON,
//!   and emptying their list because of it would be the window fighting them;
//! * **the last write still wins, and both writers are still real.** A hand edit
//!   made while the dialog is open reaches the window; a keystroke in the dialog
//!   afterwards writes the whole table back. What is gone is the case where the
//!   hand edit was never seen at all.

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
///       "env": { "FORCE_HYPERLINK": "1" },
///       "start_at": { "fixed": "D:\\Developer" },
///       "mark": { "chassis": "shell", "colour": "amber" } }
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
    /// Where this profile's *home* is, and how its launcher is told about it.
    ///
    /// **A machine fact and not a preference** — see [`StartAtV1`], which is the
    /// key the editor writes. A Windows shell is handed a working directory and
    /// `wsl.exe` is handed `--cd ~`, and which of the two applies is a property
    /// of the program rather than an answer anybody should be able to pick
    /// wrongly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starting_dir: Option<StartingDirV1>,
    /// Whether a new tab of this profile takes the folder it was opened beside,
    /// its own home, or one fixed place.
    ///
    /// Absent means [`NamedStartAtV1::Inherit`], which is what every shipped
    /// profile is and what this terminal has always done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<StartAtV1>,
    /// The mark that names this profile across the window.
    ///
    /// A bare string names one of the shipped marks, which is what a *duplicate*
    /// of a built-in wears — a copy of a PowerShell really is a PowerShell, and
    /// the mark is telling the truth. A profile drawn from nothing has no
    /// shipped logo to inherit, so it wears the neutral chassis in one of the
    /// editor's eight struck colours, and that arrives as the **object** form:
    /// the promise this field's own comment made a slice ago — "they arrive as
    /// an *object* form beside this one rather than by redefining it" — kept
    /// exactly, so that no file the previous slice wrote reads differently now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<MarkV1>,
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
///
/// **[`Self::OnPath`] joined the vocabulary on 2026-08-28 and the document did
/// not change version, on [`MarkV1`]'s own precedent.** That field grew its
/// object form the same way a slice after it was written — "an *object* form
/// beside this one rather than by redefining it" — and the test beside it pins
/// the reason: no file the previous slice wrote reads differently now. The same
/// holds here. Every `under` and `beside_on_path` on every disk parses exactly
/// as it did; what is new is a third word this build can *write*, and it can
/// only ever appear in a file this build wrote — a built-in writes nothing but
/// its id, so the word reaches a disk only when somebody duplicates one of the
/// agent rows. A `schema_version` step buys nothing an older build could act on
/// (it would refuse the whole document rather than the one row) and would put
/// every untouched v1 file through a migration that changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateV1 {
    /// `%variable%\tail`.
    Under { variable: String, tail: String },
    /// Find `anchor` on `PATH`, climb out of the directory holding it, take
    /// `tail` from there.
    BesideOnPath { anchor: String, tail: String },
    /// `name`, wherever `PATH` says it is.
    ///
    /// The everyday shape of a tool installed by a package manager: the
    /// installer's whole job is to put the thing on the path, and where it put
    /// the file is an implementation detail that differs between npm, a native
    /// installer and whatever the user's version manager does. The two above
    /// both start from a *place* and this one starts from a name, which is why
    /// it is a third form rather than a degenerate case of either.
    OnPath { name: String },
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

/// Which folder a new tab of this profile opens in — **the editor's own
/// question**, and the three answers its combo offers.
///
/// # Why this is a second key and not a fourth variant of [`StartingDirV1`]
///
/// The two answer different questions, and only one of them is anybody's to
/// choose. [`StartingDirV1`] says *where this profile's home is and how its
/// launcher is told about it*: a Windows shell takes a working directory handed
/// to `CreateProcess`, while `wsl.exe` is a launcher standing somewhere its
/// shell is not and takes `--cd ~` instead — one directory that has no Windows
/// spelling at all. That is a property of the program, derived from the shipped
/// table, and a reader who picked it wrongly would get a WSL tab opening at
/// `/mnt/c/Users/…`: a real directory, and not the one a shell opens in when you
/// start it yourself.
///
/// This key says what the reader actually decides, in the mock-up's own three
/// items: does a new tab take the folder of the pane it was opened beside, does
/// it always open at the profile's own home whatever that pane was standing in,
/// or does it always open at one fixed place. Folding the two into one enum
/// would have put "which flag does the launcher take" into a list somebody picks
/// from.
///
/// **Absent is [`NamedStartAtV1::Inherit`]**, which is what this terminal has
/// always done and what every shipped profile is: an untouched built-in still
/// writes one key, and nothing moves for anybody who never opens the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StartAtV1 {
    /// `"inherit"` or `"home"`.
    Named(NamedStartAtV1),
    /// `{ "fixed": "D:\\Developer" }` — this place, always.
    ///
    /// An object rather than a bare string, because a bare string here would be
    /// ambiguous against the two words above the moment somebody named a folder
    /// `home` — and because the untagged reader would have to guess which of the
    /// two a string was.
    Fixed { fixed: String },
}

/// The string forms of [`StartAtV1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedStartAtV1 {
    /// The folder of the pane this tab was opened beside, and the profile's own
    /// home when there was none. Today's behaviour, and the default.
    Inherit,
    /// The profile's own home, whatever the pane it was opened beside was
    /// standing in.
    Home,
}

/// The mark that names a profile across the window.
///
/// Two shapes because there are two kinds of answer, and the difference is
/// whose colour it is. A *duplicate of a built-in* wears the built-in's own
/// mark — a copy of a PowerShell is a PowerShell, and the blue is Microsoft's,
/// so it is named and never re-stated as pixels. A profile drawn from nothing
/// has no logo to inherit and must not borrow one, so it wears the neutral
/// chassis in one of the editor's eight struck colours: the same rounded panel
/// `p-pwsh` and `p-cmd` are already the same drawing of, differing only in what
/// it is filled with.
///
/// **A built-in's own row never writes this key at all.** The five identity
/// colours are not this product's to repaint (S98/S31 — the ruling that also
/// stopped a custom colour scheme repainting them), so the file cannot say what
/// the dialog will not offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MarkV1 {
    /// `"powershell"` — one of the shipped marks, named.
    Named(String),
    /// `{ "chassis": "shell", "colour": "amber" }` — the neutral chassis, tinted.
    ///
    /// `chassis` is carried rather than assumed even though there is exactly one
    /// of them today, because the alternative — a bare `"colour"` key — would
    /// have to grow a second key the day a second chassis is struck, and a
    /// reader hand-editing this file would then be looking at two spellings of
    /// one idea.
    Generic { chassis: String, colour: String },
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
                    start_at: Some(StartAtV1::Fixed {
                        fixed: r"D:\Developer".to_owned(),
                    }),
                    mark: Some(MarkV1::Generic {
                        chassis: "shell".to_owned(),
                        colour: "amber".to_owned(),
                    }),
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

    /// PIN — **the third candidate form is additive**: a document written
    /// before it existed reads exactly as it did, and a document that uses it
    /// round trips (user ruling 2026-08-28, the agent profiles).
    ///
    /// Red gate: spell `OnPath` as a rename of `BesideOnPath` — the first half
    /// goes red on every `beside_on_path` already on a disk, which is the
    /// failure a version bump would have been owed for and this shape is not.
    #[test]
    fn a_third_candidate_form_leaves_the_two_before_it_reading_as_they_did() {
        let old: ProgramV1 = serde_json::from_str(
            r#"{ "kind": "first_of", "candidates": [
                   { "beside_on_path": { "anchor": "git.exe", "tail": "bin\\bash.exe" } },
                   { "under": { "variable": "ProgramFiles", "tail": "Git\\bin\\bash.exe" } } ] }"#,
        )
        .unwrap();
        assert_eq!(
            old,
            ProgramV1::Resolution(ResolutionV1::FirstOf {
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
            })
        );

        let fresh = ProgramV1::Resolution(ResolutionV1::FirstOf {
            candidates: vec![CandidateV1::OnPath {
                name: "claude.cmd".to_owned(),
            }],
        });
        let wire = serde_json::to_value(&fresh).unwrap();
        assert_eq!(
            wire["candidates"][0]["on_path"]["name"],
            serde_json::Value::from("claude.cmd"),
            "the word a person hand-editing this file would read: {wire}"
        );
        assert_eq!(serde_json::from_value::<ProgramV1>(wire).unwrap(), fresh);
    }

    /// PIN — **a bare `mark` string still names a shipped mark**, which is the
    /// whole of the compatibility promise the object form was added under.
    ///
    /// Red gate: replace the untagged pair with a struct-only shape — every
    /// `profiles.json` the previous slice wrote for a duplicated built-in would
    /// stop parsing, and the profile would come back wearing the neutral chassis
    /// instead of the logo it is a copy of.
    #[test]
    fn a_mark_written_as_a_string_still_names_a_shipped_mark() {
        let read: ProfileEntryV1 =
            serde_json::from_str(r#"{ "id": "x", "mark": "powershell" }"#).unwrap();
        assert_eq!(read.mark, Some(MarkV1::Named("powershell".to_owned())));
        assert_eq!(
            serde_json::to_value(&read).unwrap()["mark"],
            serde_json::Value::from("powershell"),
            "and it writes back as the same bare string it was read from"
        );
    }

    /// PIN — **the eight struck colours arrive as an object**, chassis and
    /// colour, and round trip whole.
    #[test]
    fn a_profile_drawn_from_nothing_wears_a_chassis_and_a_colour() {
        let read: ProfileEntryV1 = serde_json::from_str(
            r#"{ "id": "claude-7f3a", "mark": { "chassis": "shell", "colour": "amber" } }"#,
        )
        .unwrap();
        assert_eq!(
            read.mark,
            Some(MarkV1::Generic {
                chassis: "shell".to_owned(),
                colour: "amber".to_owned(),
            })
        );
        let wire = serde_json::to_value(&read).unwrap();
        assert_eq!(wire["mark"]["chassis"], serde_json::Value::from("shell"));
        assert_eq!(wire["mark"]["colour"], serde_json::Value::from("amber"));
    }

    /// PIN — **`start_at` is three answers and absence is the first of them**.
    ///
    /// Red gate: give the field a `#[serde(default)]` that is `Some(Inherit)` —
    /// every untouched built-in would start writing a key stating the behaviour
    /// it already had, and `an_untouched_builtin_is_its_id_and_nothing_else`
    /// would go red beside it.
    #[test]
    fn where_a_tab_opens_is_inherit_home_or_one_fixed_place() {
        let inherit: ProfileEntryV1 =
            serde_json::from_str(r#"{ "id": "x", "start_at": "inherit" }"#).unwrap();
        assert_eq!(
            inherit.start_at,
            Some(StartAtV1::Named(NamedStartAtV1::Inherit))
        );
        let home: ProfileEntryV1 =
            serde_json::from_str(r#"{ "id": "x", "start_at": "home" }"#).unwrap();
        assert_eq!(home.start_at, Some(StartAtV1::Named(NamedStartAtV1::Home)));
        let fixed: ProfileEntryV1 =
            serde_json::from_str(r#"{ "id": "x", "start_at": { "fixed": "D:\\Developer" } }"#)
                .unwrap();
        assert_eq!(
            fixed.start_at,
            Some(StartAtV1::Fixed {
                fixed: r"D:\Developer".to_owned(),
            })
        );
        let silent: ProfileEntryV1 = serde_json::from_str(r#"{ "id": "x" }"#).unwrap();
        assert_eq!(
            silent.start_at, None,
            "a file that says nothing means inherit, and says nothing"
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
