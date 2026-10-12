//! **`keybindings.json`, merged three ways** (`docs/plans/design/settings-reread-2026-10-11.md`
//! §3.4 `keybindings.json` and (b)8 `keybindings.json`, with (b)3, (b)6, (c)1 and (d)1 as
//! ticket 6 applied them; ticket T-STORE-MERGE-KEYBINDINGS).
//!
//! The store keeps a file's bytes for the merge exactly as `settings.json`'s does
//! (`document_request::DocumentState::merge_stored`): a re-read that found the file
//! changed, a write that answered `Moved`, a write a hand edit crossed (`Displaced`).
//! The merge takes three documents — the **base** `B`, **ours** `O` (`desired`, or the
//! document written for a displaced file) and **theirs** `T` (the file; no file is the
//! default document, "deleted file = shipped table") — and answers the document Folio
//! now wants on disk.
//!
//! **Each side is read as the reader reads it** ((b)8):
//! - in this platform's dialect: a side written on another platform has each chord
//!   carried across with `translate_command_modifier`, as an import does; a line that
//!   cannot be carried across is **carried**;
//! - one departure per action, the last line about it in force (the reader's rule); the
//!   earlier lines about it are **carried**;
//! - a line naming an action this build has no row for is **carried**.
//!
//! **The one rule for what is carried** ((b)8): it is in the merged document verbatim,
//! in the file's position, and never rewritten, applied or owed. It is the file's — a
//! line carried in `O` that the file no longer holds is gone with it.
//!
//! **The unit is one action's departure**, merged by §3.3's table
//! (`settings_merge::unit`): changed on one side takes that side; changed differently
//! on both is a **conflict, and the file wins** (owner ruling Q1, 2026-10-11), named by
//! the action's name on the Shortcuts page. Departures are compared by meaning — the
//! chord as this build reads it — so a chord spelled two ways is one value.
//!
//! **Then by chord** (§3.4; DESIGN §7.1.6c-2′ ruling two: one chord answers one action).
//! The merged departures are dealt out by the two-pass reader with the file's first: a
//! chord the file's action and Folio's action both claim stays with the file's, and
//! Folio's action gets what the reader gives a refused line — its default, or no chord
//! when another action holds the default. That outcome is what the merged document says
//! for it, so the document describes the table it builds; the pass names it.
//!
//! **The order is deterministic** ((b)8): `T`'s lines in `T`'s order, each action's line
//! holding the merged departure; then the actions only Folio departs on, in the table's
//! own order (`Shortcuts::overrides`' walk); carried lines where the file has them.
//!
//! **The merge is pure.** It builds no table anybody reads and runs no door. The door —
//! the table rebuilt from this build's defaults and the merged departures by the
//! two-pass reader ([`rebuild`]), then the store told what is in force — runs after
//! `merge_stored`, on the window thread ([`run_pass`]).

use std::collections::BTreeMap;

use bt_persist::{BindingOverrideV1, KEYBINDINGS_SCHEMA_VERSION, KeybindingsV1};

use crate::document_request::ReplacedInFile;
use crate::persist::KEYBINDINGS_FILE_NAME;
use crate::settings_bundle::dialect_of;
use crate::settings_merge::{Account, PassOutcome, Unit, unit};
use crate::shortcuts::{
    ChordVerdict, Override, OverrideFault, Shortcuts, format_chord_on, parse_chord,
    translate_command_modifier,
};

/// **One action's departure as a side states it**: the chord as the line spells it in
/// this platform's dialect (`None`: the file's `null`, no chord), and what it means.
#[derive(Clone, Debug)]
struct Departure {
    /// What the merged document writes for it.
    text: Option<String>,
    /// The chord as this build reads and spells it on this platform; a chord it cannot
    /// read is its own text.
    meaning: Option<String>,
}

impl Departure {
    fn new(text: Option<String>, platform: bt_platform::HostPlatform) -> Self {
        let meaning = text.as_ref().map(|text| {
            parse_chord(text)
                .map_or_else(|| text.clone(), |chord| format_chord_on(&chord, platform))
        });
        Self { text, meaning }
    }
}

/// Two departures are one value when they mean one chord.
impl PartialEq for Departure {
    fn eq(&self, other: &Self) -> bool {
        self.meaning == other.meaning
    }
}

/// One line of a side, in the file's order.
#[derive(Clone, Debug)]
enum Line {
    /// The line that puts this action's departure in force.
    InForce(String),
    /// A line the reader does not put in force, kept verbatim.
    Carried(BindingOverrideV1),
}

/// **A side read as the reader reads it.**
struct Side {
    lines: Vec<Line>,
    /// One departure per action this build knows.
    units: BTreeMap<String, Departure>,
    /// The lines that could not be carried into this platform's dialect, and why.
    untranslatable: Vec<(BindingOverrideV1, &'static str)>,
}

/// **Read `document` in `platform`'s dialect** against this build's `table`.
fn read(document: &KeybindingsV1, table: &Shortcuts, platform: bt_platform::HostPlatform) -> Side {
    // A side spelled in this dialect, or in none recorded, is read as it is spelled
    // (`import_shortcuts`' rule); another platform's is carried across.
    let from = dialect_of(document.writing_platform).filter(|source| *source != platform);
    let mut untranslatable = Vec::new();
    let read: Vec<Option<(String, Departure)>> = document
        .bindings
        .iter()
        .map(|line| {
            table.row(&line.action)?;
            let text = match (&line.chord, from) {
                (None, _) => None,
                (Some(chord), None) => Some(chord.clone()),
                (Some(chord), Some(from)) => {
                    match translate_command_modifier(chord, Some(from), platform) {
                        Ok(chord) => Some(chord),
                        Err(why) => {
                            untranslatable.push((line.clone(), why));
                            return None;
                        }
                    }
                }
            };
            Some((line.action.clone(), Departure::new(text, platform)))
        })
        .collect();
    let mut units = BTreeMap::new();
    let mut lines = Vec::with_capacity(read.len());
    for (index, (line, departure)) in document.bindings.iter().zip(&read).enumerate() {
        let in_force = departure.as_ref().filter(|(action, _)| {
            !read[index + 1..]
                .iter()
                .flatten()
                .any(|(later, _)| later == action)
        });
        match in_force {
            Some((action, departure)) => {
                units.insert(action.clone(), departure.clone());
                lines.push(Line::InForce(action.clone()));
            }
            None => lines.push(Line::Carried(line.clone())),
        }
    }
    Side {
        lines,
        units,
        untranslatable,
    }
}

/// **The name a reader knows an action by**: its title on the Shortcuts page, or its id
/// for an action this build has no row for.
fn name(table: &Shortcuts, action: &str) -> String {
    table
        .row(action)
        .map_or_else(|| action.to_owned(), |row| row.title.text().to_owned())
}

fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

/// **What one merge says about itself.**
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct About {
    /// The counts and names of [`settings_merge::Account`]: a chord Folio's action gave
    /// up to the file's is one of the conflicts, the file winning it.
    ///
    /// [`settings_merge::Account`]: crate::settings_merge::Account
    pub(crate) account: Account,
    /// Each chord Folio's action gave up to the file's, said as a sentence.
    pub(crate) chords_given: Vec<String>,
    /// The file's lines this edit added that could not be carried into this platform's
    /// dialect, by the action's name, with why.
    pub(crate) untranslatable: Vec<(String, String)>,
    /// The actions whose departure the merge took from the file.
    pub(crate) from_the_file: Vec<String>,
    /// **The lines the merged document carries**, in its order: never dealt out by
    /// [`rebuild`]. Once the document is written in this platform's dialect, a line
    /// another platform's dialect could not carry across reads like any other, and a
    /// shadowed line whose later line the merge left out would be the last about its
    /// action — what was carried is known here, where it was decided, and nowhere else.
    pub(crate) carried: Vec<BindingOverrideV1>,
}

impl About {
    /// Add one merge's account to the pass's.
    pub(crate) fn add(&mut self, other: About) {
        self.account.add(other.account);
        for sentence in other.chords_given {
            push_once(&mut self.chords_given, sentence);
        }
        for line in other.untranslatable {
            if !self.untranslatable.contains(&line) {
                self.untranslatable.push(line);
            }
        }
        for action in other.from_the_file {
            push_once(&mut self.from_the_file, action);
        }
        // Each merge's document is built whole from its file: the latest one's
        // carried lines are the ones `desired` carries.
        self.carried = other.carried;
    }
}

/// **One merge of `keybindings.json`.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Merged {
    /// The merged document: what Folio now wants on disk.
    pub(crate) document: KeybindingsV1,
    pub(crate) about: About,
}

/// **Merge `ours` and `theirs` against `base`**, by action, then by chord (§3.4, (b)8).
pub(crate) fn merge(
    base: &KeybindingsV1,
    ours: &KeybindingsV1,
    theirs: &KeybindingsV1,
    platform: bt_platform::HostPlatform,
) -> Merged {
    let table = Shortcuts::defaults_for(platform);
    let (b, o, t) = (
        read(base, &table, platform),
        read(ours, &table, platform),
        read(theirs, &table, platform),
    );
    let mut about = About::default();

    // By action, in the table's order, so every name is said in one order.
    let mut merged: BTreeMap<String, (Option<Departure>, Unit)> = BTreeMap::new();
    for row in table.rows() {
        let action = row.id;
        let sides = [&b, &o, &t].map(|side| side.units.get(action).cloned());
        if sides.iter().all(Option::is_none) {
            continue;
        }
        let [base, ours, theirs] = sides;
        let (value, chosen) = unit(base, ours, theirs.clone());
        let name = row.title.text().to_owned();
        match chosen {
            Unit::Base => {}
            Unit::Ours => about.account.kept += 1,
            Unit::Theirs => {
                about.account.took += 1;
                push_once(&mut about.account.taken, name);
                about.from_the_file.push(action.to_owned());
            }
            Unit::Conflict => {
                about.account.conflicted += 1;
                push_once(&mut about.account.conflicts, name);
                about.from_the_file.push(action.to_owned());
            }
        }
        // A unit that is the file's is written as the file spells it.
        let value = if chosen == Unit::Ours { value } else { theirs };
        merged.insert(action.to_owned(), (value, chosen));
    }

    give_the_files_actions_their_chords(&table, platform, &t, &mut merged, &mut about);

    for (line, why) in &t.untranslatable {
        if !base.bindings.contains(line) {
            about
                .untranslatable
                .push((name(&table, &line.action), (*why).to_owned()));
        }
    }

    // The order: the file's, then Folio's own in the table's, carried lines in place.
    let mut bindings = Vec::with_capacity(t.lines.len() + merged.len());
    let mut named = Vec::new();
    for line in &t.lines {
        match line {
            Line::Carried(line) => {
                bindings.push(line.clone());
                about.carried.push(line.clone());
            }
            Line::InForce(action) => {
                if let Some((Some(departure), _)) = merged.get(action) {
                    bindings.push(BindingOverrideV1 {
                        action: action.clone(),
                        chord: departure.text.clone(),
                    });
                }
                named.push(action.as_str());
            }
        }
    }
    for row in table.rows() {
        if named.contains(&row.id) {
            continue;
        }
        if let Some((Some(departure), _)) = merged.get(row.id) {
            bindings.push(BindingOverrideV1 {
                action: row.id.to_owned(),
                chord: departure.text.clone(),
            });
        }
    }
    Merged {
        document: KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            writing_platform: crate::persist::keybindings_platform_on(platform),
            bindings,
        },
        about,
    }
}

/// **The chord pass** (§3.4): the merged departures dealt out by the two-pass reader, the
/// file's first — `T`'s lines in `T`'s order, then any other the file holds, then
/// Folio's own in the table's order. A chord one of Folio's departures claims and an
/// action of the file's holds after the deal is the file's: Folio's action takes what
/// the reader gave it, which the merged document then says.
fn give_the_files_actions_their_chords(
    table: &Shortcuts,
    platform: bt_platform::HostPlatform,
    t: &Side,
    merged: &mut BTreeMap<String, (Option<Departure>, Unit)>,
    about: &mut About,
) {
    let deal = |action: &str, departure: &Departure| Override {
        id: action.to_owned(),
        chord: departure.meaning.clone(),
    };
    let mut dealt: Vec<Override> = Vec::new();
    for line in &t.lines {
        if let Line::InForce(action) = line
            && let Some((Some(departure), chosen)) = merged.get(action)
            && *chosen != Unit::Ours
        {
            dealt.push(deal(action, departure));
        }
    }
    let files_or_folios = |folios: bool| {
        table
            .rows()
            .iter()
            .filter_map(|row| match merged.get(row.id) {
                Some((Some(departure), chosen)) if (*chosen == Unit::Ours) == folios => {
                    Some(deal(row.id, departure))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for entry in files_or_folios(false) {
        if !dealt.iter().any(|already| already.id == entry.id) {
            dealt.push(entry);
        }
    }
    let folios = files_or_folios(true);
    dealt.extend(folios.iter().cloned());
    let mut dealt_table = table.clone();
    let _ = dealt_table.apply_overrides(&dealt);

    for entry in folios {
        let Some(chord) = entry.chord.as_deref().and_then(parse_chord) else {
            continue;
        };
        let Some(row) = dealt_table.row(&entry.id) else {
            continue;
        };
        if row.chord.as_ref() == Some(&chord) {
            continue;
        }
        // Held by an action the file changed — bound, or given back its default. A
        // chord an untouched row holds refused Folio's line on its own, as it would
        // at start.
        let ChordVerdict::AlreadyUsed { holder, title } =
            dealt_table.verdict_for(&entry.id, &chord)
        else {
            continue;
        };
        if !matches!(merged.get(holder), Some((_, Unit::Theirs | Unit::Conflict))) {
            continue;
        }
        let given = row.chord.clone();
        let shipped = table.row(&entry.id).and_then(|row| row.chord.clone());
        let value = (given != shipped).then(|| {
            Departure::new(
                given.as_ref().map(|chord| format_chord_on(chord, platform)),
                platform,
            )
        });
        let loser = name(table, &entry.id);
        about.account.kept -= 1;
        about.account.conflicted += 1;
        push_once(&mut about.account.conflicts, loser.clone());
        about.chords_given.push(format!(
            "{loser} gave {} to {}",
            format_chord_on(&chord, platform),
            title.text()
        ));
        merged.insert(entry.id, (value, Unit::Conflict));
    }
}

/// **The table this build puts in force from a merged `document`** — this build's
/// defaults, then the document's departures dealt out by the two-pass reader, exactly as
/// at start and on an import (`Shortcuts::apply_overrides`). The lines the merge
/// `carried` are passed over, in order, and the rest read as [`merge`] reads them. The
/// lines the reader refused, by action.
pub(crate) fn rebuild(
    document: &KeybindingsV1,
    carried: &[BindingOverrideV1],
    platform: bt_platform::HostPlatform,
) -> (Shortcuts, Vec<OverrideFault>) {
    let mut table = Shortcuts::defaults_for(platform);
    let mut passed_over = carried.iter().peekable();
    let dealt_lines = KeybindingsV1 {
        bindings: document
            .bindings
            .iter()
            .filter(|line| passed_over.next_if(|carried| carried == line).is_none())
            .cloned()
            .collect(),
        ..document.clone()
    };
    let side = read(&dealt_lines, &table, platform);
    let dealt: Vec<Override> = side
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::InForce(action) => side.units.get(action).map(|departure| Override {
                id: action.clone(),
                chord: departure.meaning.clone(),
            }),
            Line::Carried(_) => None,
        })
        .collect();
    let faults = table.apply_overrides(&dealt);
    (table, faults)
}

/// **One storage pass's merge of `keybindings.json`** — what
/// `KeybindingsStore::begin_merge` hands the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeybindingsPass {
    /// `desired` after every stored file was merged and folded: what the door puts in
    /// force.
    pub(crate) document: KeybindingsV1,
    /// The dialect the merge read in, and the table is rebuilt in.
    pub(crate) platform: bt_platform::HostPlatform,
    /// Every merge of the pass, summed.
    pub(crate) about: About,
    /// The departures this pass took from the file that the reader refused, by name,
    /// with why.
    pub(crate) refused: Vec<(String, String)>,
}

impl KeybindingsPass {
    /// A pass over `document`, with `about` to say; the door not run yet.
    pub(crate) fn new(
        document: KeybindingsV1,
        platform: bt_platform::HostPlatform,
        about: About,
    ) -> Self {
        Self {
            document,
            platform,
            about,
            refused: Vec::new(),
        }
    }

    /// **The pass's one line for `diagnostics.log`** (§3.6): the counts, then each chord
    /// Folio's action gave up to the file's, and each line of the file not put in force.
    pub(crate) fn diagnostics(&self) -> String {
        let mut line = format!(
            "BT_PERSIST {KEYBINDINGS_FILE_NAME} merged: took {} from the file, kept {} from \
             Folio, conflicts {}",
            self.about.account.took, self.about.account.kept, self.about.account.conflicted
        );
        for sentence in &self.about.chords_given {
            line.push_str(&format!("; {sentence}"));
        }
        for (name, why) in self.about.untranslatable.iter().chain(&self.refused) {
            line.push_str(&format!("; {name} not applied: {why}"));
        }
        line
    }

    /// **What the pass replaced and could not apply, for its card** (§3.6): nothing for a
    /// clean hand edit; otherwise the file, every action whose departure or chord the
    /// file won over Folio's, and every line of the file not put in force, each named
    /// once.
    pub(crate) fn replaced(&self) -> Option<ReplacedInFile> {
        let mut not_applied = Vec::new();
        for (name, _) in self.about.untranslatable.iter().chain(&self.refused) {
            push_once(&mut not_applied, name.clone());
        }
        if self.about.account.conflicts.is_empty() && not_applied.is_empty() {
            return None;
        }
        Some(ReplacedInFile {
            file: KEYBINDINGS_FILE_NAME,
            values: self.about.account.conflicts.clone(),
            not_applied,
        })
    }
}

/// **What a keybindings merge needs from the window it runs in** — the product seam
/// `Runtime` implements (`runtime/configuration.rs`), so the pass's order is
/// [`run_pass`]'s and nobody else's.
pub(crate) trait KeybindingsWindow {
    /// `KeybindingsStore::begin_merge`: every stored file merged and folded, purely.
    fn keybindings_merge_begin(&mut self) -> Option<KeybindingsPass>;
    /// **The door**: `table` is the one table dispatch reads, and the store is told the
    /// departures it put in force.
    fn keybindings_put_in_force(&mut self, table: Shortcuts);
    /// `KeybindingsStore::end_merge`: the merged document sent, conditional on the file.
    fn keybindings_merge_end(&mut self);
    /// The pass's one line, written to `diagnostics.log`.
    fn keybindings_say(&mut self, line: &str);
    /// An open Shortcuts page re-derived and the chrome presented.
    fn keybindings_present(&mut self) -> anyhow::Result<()>;
}

/// **One storage pass's merge of `keybindings.json`, in §3.3's order**: the store merges
/// and folds (no door runs inside it); then the table is rebuilt from the merged
/// document and put in force — after `merge_stored`, so a press laid back over a
/// displaced merge is in it; then the merged document is sent conditionally, the pass's
/// line is written and its card's data made, all before the presentation, which cannot
/// take them away.
pub(crate) fn run_pass(window: &mut impl KeybindingsWindow) -> PassOutcome {
    let Some(mut pass) = window.keybindings_merge_begin() else {
        return PassOutcome {
            replaced: None,
            presented: Ok(()),
        };
    };
    let (table, faults) = rebuild(&pass.document, &pass.about.carried, pass.platform);
    for fault in faults {
        if pass.about.from_the_file.contains(&fault.id) {
            pass.refused.push((name(&table, &fault.id), fault.reason));
        }
    }
    window.keybindings_put_in_force(table);
    window.keybindings_merge_end();
    window.keybindings_say(&pass.diagnostics());
    let replaced = pass.replaced();
    let presented = window.keybindings_present();
    PassOutcome {
        replaced,
        presented,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_persist::KeybindingsPlatformV1;

    use super::*;
    use crate::document_request::{DocumentAnswer, Outcome};
    use crate::persist::{KeybindingsStore, ScriptedJob, ScriptedJobs, StorageLane};
    use crate::shortcuts::Chord;

    const PLATFORM: bt_platform::HostPlatform = bt_platform::HostPlatform::Windows;

    fn line(action: &str, chord: Option<&str>) -> BindingOverrideV1 {
        BindingOverrideV1 {
            action: action.to_owned(),
            chord: chord.map(str::to_owned),
        }
    }

    fn document(
        writing_platform: KeybindingsPlatformV1,
        bindings: Vec<BindingOverrideV1>,
    ) -> KeybindingsV1 {
        KeybindingsV1 {
            schema_version: KEYBINDINGS_SCHEMA_VERSION,
            writing_platform,
            bindings,
        }
    }

    fn windows(bindings: Vec<BindingOverrideV1>) -> KeybindingsV1 {
        document(KeybindingsPlatformV1::Windows, bindings)
    }

    /// The chord `action` holds in `table`, as `platform` spells it.
    fn chord_of(
        table: &Shortcuts,
        action: &str,
        platform: bt_platform::HostPlatform,
    ) -> Option<String> {
        table
            .row(action)
            .and_then(|row| row.chord.as_ref())
            .map(|chord| format_chord_on(chord, platform))
    }

    fn title(action: &str) -> String {
        Shortcuts::defaults_for(PLATFORM)
            .row(action)
            .expect("a row of this build")
            .title
            .text()
            .to_owned()
    }

    /// RED ((b)14-10, (b)8) — **a file written on another platform merges in this
    /// dialect**, meaning kept: a Mac's `Cmd` is this Windows' `Ctrl`, and a Windows
    /// file's `Ctrl` is a Mac's `Cmd`; the result says it is this platform's.
    ///
    /// MUTATION: skip the translation (in `read`, read every side as it is spelled) —
    /// the Windows result says `Cmd+Shift+Y`, a `Win` chord here.
    #[test]
    fn a_file_written_on_another_platform_merges_in_this_dialect() {
        for (written_on, chord, here, dialect, expected) in [
            (
                KeybindingsPlatformV1::MacOs,
                "Cmd+Shift+Y",
                bt_platform::HostPlatform::Windows,
                KeybindingsPlatformV1::Windows,
                "Ctrl+Shift+y",
            ),
            (
                KeybindingsPlatformV1::Windows,
                "Ctrl+Shift+Y",
                bt_platform::HostPlatform::MacOs,
                KeybindingsPlatformV1::MacOs,
                "Cmd+Shift+y",
            ),
        ] {
            let base = document(dialect, Vec::new());
            let theirs = document(
                written_on,
                vec![line("new-tab", Some(chord)), line("close-pane", None)],
            );
            let merged = merge(&base, &base, &theirs, here);
            assert_eq!(merged.document.writing_platform, dialect, "{written_on:?}");
            assert_eq!(
                merged.document.bindings,
                vec![line("new-tab", Some(expected)), line("close-pane", None)],
                "in {dialect:?}'s dialect"
            );
            let (table, faults) = rebuild(&merged.document, &merged.about.carried, here);
            assert!(faults.is_empty(), "{faults:?}");
            assert_eq!(
                chord_of(&table, "new-tab", here).as_deref(),
                Some(expected),
                "the meaning kept"
            );
            assert_eq!(chord_of(&table, "close-pane", here), None);
            assert_eq!(merged.about.account.took, 2);
        }
    }

    /// RED ((b)14-10, (b)8's one rule) — **an earlier duplicate line, an unknown action and
    /// an untranslatable line are carried verbatim in their positions, and none is
    /// applied.** The file is a Mac's: `Cmd+Ctrl+K` cannot be carried to Windows (its
    /// `Cmd` is this `Ctrl`, which it already holds).
    ///
    /// MUTATION: drop carried lines (in `merge`, push nothing for `Line::Carried`) — the
    /// document loses the three lines.
    #[test]
    fn duplicate_and_unknown_lines_are_carried() {
        let unknown = line("未知动作-no-such-action", Some("Cmd+Alt+K"));
        let shadowed = line("new-tab", Some("Cmd+Shift+J"));
        let untranslatable = line("zoom-pane", Some("Cmd+Ctrl+K"));
        let theirs = document(
            KeybindingsPlatformV1::MacOs,
            vec![
                unknown.clone(),
                shadowed.clone(),
                untranslatable.clone(),
                line("new-tab", Some("Cmd+Shift+K")),
            ],
        );
        let base = windows(Vec::new());
        let merged = merge(&base, &base, &theirs, PLATFORM);
        assert_eq!(
            merged.document.bindings,
            vec![
                unknown,
                shadowed,
                untranslatable,
                line("new-tab", Some("Ctrl+Shift+k")),
            ],
            "verbatim, in the file's positions"
        );
        assert_eq!(
            merged.about.untranslatable,
            vec![(
                title("zoom-pane"),
                "the source command modifier collides with a destination modifier".to_owned()
            )],
            "named"
        );

        let (table, _) = rebuild(&merged.document, &merged.about.carried, PLATFORM);
        let defaults = Shortcuts::defaults_for(PLATFORM);
        assert_eq!(
            chord_of(&table, "new-tab", PLATFORM).as_deref(),
            Some("Ctrl+Shift+k"),
            "the last line is in force"
        );
        assert_eq!(
            chord_of(&table, "zoom-pane", PLATFORM),
            chord_of(&defaults, "zoom-pane", PLATFORM),
            "the untranslatable line is not applied"
        );
        let mut others = table.clone();
        others.set(
            "new-tab",
            defaults.row("new-tab").and_then(|row| row.chord.clone()),
        );
        assert_eq!(others, defaults, "nothing else is in force");
        assert_eq!(merged.about.account.took, 1, "one unit, the in-force line");
    }

    /// RED (§8-14, §3.4) — **a chord claimed on both sides goes to the file's action.**
    /// Folio bound `Ctrl+K` to New tab; the file bound it to Close pane. Close pane keeps
    /// it; New tab gets what the reader gives a refused line — its default, which is
    /// free, or no chord when the file gave its default to another action — and the
    /// pass names it.
    ///
    /// MUTATION: Folio's action keeps the chord (deal Folio's departures before the
    /// file's in `give_the_files_actions_their_chords`) — New tab keeps `Ctrl+K`.
    #[test]
    fn a_chord_claimed_on_both_sides_goes_to_the_files_action() {
        let base = windows(Vec::new());
        let ours = windows(vec![line("new-tab", Some("Ctrl+k"))]);

        // Its default is free: New tab has it back, and the file says nothing for it.
        let theirs = windows(vec![line("close-pane", Some("Ctrl+k"))]);
        let merged = merge(&base, &ours, &theirs, PLATFORM);
        assert_eq!(
            merged.document.bindings,
            vec![line("close-pane", Some("Ctrl+k"))]
        );
        let (table, faults) = rebuild(&merged.document, &merged.about.carried, PLATFORM);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(
            chord_of(&table, "close-pane", PLATFORM).as_deref(),
            Some("Ctrl+k")
        );
        assert_eq!(
            chord_of(&table, "new-tab", PLATFORM),
            chord_of(&Shortcuts::defaults_for(PLATFORM), "new-tab", PLATFORM),
            "its default"
        );
        assert_eq!(merged.about.account.conflicts, vec![title("new-tab")]);
        assert_eq!(
            (merged.about.account.kept, merged.about.account.conflicted),
            (0, 1)
        );
        let pass = KeybindingsPass::new(merged.document.clone(), PLATFORM, merged.about);
        assert_eq!(
            pass.diagnostics(),
            format!(
                "BT_PERSIST keybindings.json merged: took 1 from the file, kept 0 from Folio, \
                 conflicts 1; {} gave Ctrl+k to {}",
                title("new-tab"),
                title("close-pane")
            )
        );
        assert_eq!(
            pass.replaced().map(|replaced| replaced.values),
            Some(vec![title("new-tab")]),
            "the card names it"
        );

        // Its default is the file's too: New tab is left with no chord, and says so.
        let theirs = windows(vec![
            line("close-pane", Some("Ctrl+k")),
            line("quit", Some("Ctrl+Shift+N")),
        ]);
        let merged = merge(&base, &ours, &theirs, PLATFORM);
        assert_eq!(
            merged.document.bindings,
            vec![
                line("close-pane", Some("Ctrl+k")),
                line("quit", Some("Ctrl+Shift+N")),
                line("new-tab", None),
            ]
        );
        let (table, faults) = rebuild(&merged.document, &merged.about.carried, PLATFORM);
        assert!(faults.is_empty(), "{faults:?}");
        assert_eq!(chord_of(&table, "new-tab", PLATFORM), None, "unset");
        assert_eq!(
            chord_of(&table, "quit", PLATFORM).as_deref(),
            Some("Ctrl+Shift+n")
        );
    }

    /// RED (§3.3's table, per action) — **same on both sides, changed here only, changed in
    /// the file only, changed on both to one chord, changed on both to two**: the last is
    /// a conflict, the file's chord, named by the action's name.
    ///
    /// MUTATION: the conflict takes O (in `merge`, keep `ours`' departure for
    /// `Unit::Conflict`) — Files pane is written with Folio's chord.
    #[test]
    fn the_table_merge_follows_the_unit_table() {
        let base = windows(vec![
            line("new-tab", Some("Ctrl+j")),
            line("close-pane", Some("Ctrl+k")),
        ]);
        let ours = windows(vec![
            line("new-tab", Some("Ctrl+j")),
            line("close-pane", Some("Ctrl+m")),
            line("zoom-pane", Some("Ctrl+i")),
            line("files-pane", Some("Ctrl+o")),
        ]);
        let theirs = windows(vec![
            line("new-tab", Some("Ctrl+J")),
            line("close-pane", Some("Ctrl+k")),
            line("quit", Some("Ctrl+u")),
            line("zoom-pane", Some("Ctrl+i")),
            line("files-pane", Some("Ctrl+h")),
        ]);
        let merged = merge(&base, &ours, &theirs, PLATFORM);
        assert_eq!(
            merged.document.bindings,
            vec![
                line("new-tab", Some("Ctrl+J")),
                line("close-pane", Some("Ctrl+m")),
                line("quit", Some("Ctrl+u")),
                line("zoom-pane", Some("Ctrl+i")),
                line("files-pane", Some("Ctrl+h")),
            ],
            "same (spelled as the file spells it), ours, theirs, both alike, conflict"
        );
        let account = &merged.about.account;
        assert_eq!((account.took, account.kept, account.conflicted), (2, 1, 1));
        assert_eq!(account.conflicts, vec![title("files-pane")]);
        let (_, faults) = rebuild(&merged.document, &merged.about.carried, PLATFORM);
        assert!(faults.is_empty(), "{faults:?}");
    }

    /// RED ((b)8's order) — **the file's lines in the file's order, then the actions only
    /// Folio departs on in the table's order, and the carried lines where the file has
    /// them**, whatever order Folio's own document lists them in.
    ///
    /// MUTATION: emit Folio's departures first (in `merge`, the table-order walk before
    /// the file's lines) — Quit and Zoom pane lead.
    #[test]
    fn the_output_order_is_the_files_then_folios_then_carried() {
        let unknown = line("未知 unknown-action", Some("Ctrl+Alt+F1"));
        let base = windows(vec![line("new-tab", Some("Ctrl+j")), unknown.clone()]);
        let ours = windows(vec![
            line("zoom-pane", Some("Ctrl+i")),
            line("new-tab", Some("Ctrl+j")),
            line("quit", Some("Ctrl+u")),
            unknown.clone(),
        ]);
        let theirs = windows(vec![
            unknown.clone(),
            line("close-pane", Some("Ctrl+k")),
            line("new-tab", Some("Ctrl+j")),
        ]);
        let merged = merge(&base, &ours, &theirs, PLATFORM);
        assert_eq!(
            merged.document.bindings,
            vec![
                unknown,
                line("close-pane", Some("Ctrl+k")),
                line("new-tab", Some("Ctrl+j")),
                line("quit", Some("Ctrl+u")),
                line("zoom-pane", Some("Ctrl+i")),
            ]
        );
        assert_eq!(
            merge(&base, &ours, &theirs, PLATFORM),
            merged,
            "the same three documents, the same answer"
        );
    }

    /// A window for the keybindings pass: the store, the one table, and what happened
    /// in order. A real `Runtime` needs a window, which a test may not open; the door
    /// here does what `Runtime`'s does — the table replaced whole, the store told its
    /// departures.
    struct Window {
        store: KeybindingsStore,
        jobs: ScriptedJobs,
        table: Shortcuts,
        log: Vec<String>,
        /// What the table and the store's departures were when the merge returned.
        at_begin: Option<(Shortcuts, Vec<BindingOverrideV1>)>,
    }

    impl KeybindingsWindow for Window {
        fn keybindings_merge_begin(&mut self) -> Option<KeybindingsPass> {
            let pass = self.store.begin_merge(PLATFORM);
            self.at_begin = Some((self.table.clone(), self.store.loaded().to_vec()));
            self.log.push("merged".to_owned());
            pass
        }

        fn keybindings_put_in_force(&mut self, table: Shortcuts) {
            self.store
                .document()
                .project(crate::persist::keybindings_document(
                    departures(&table),
                    crate::persist::keybindings_platform_on(PLATFORM),
                ));
            self.table = table;
            self.log.push("put in force".to_owned());
        }

        fn keybindings_merge_end(&mut self) {
            self.store.end_merge();
            self.log.push("sent".to_owned());
        }

        fn keybindings_say(&mut self, line: &str) {
            self.log.push(line.to_owned());
        }

        fn keybindings_present(&mut self) -> anyhow::Result<()> {
            self.log.push("presented".to_owned());
            Ok(())
        }
    }

    fn departures(table: &Shortcuts) -> Vec<BindingOverrideV1> {
        table
            .overrides()
            .into_iter()
            .map(|entry| BindingOverrideV1 {
                action: entry.id,
                chord: entry.chord,
            })
            .collect()
    }

    /// The next document write on a scripted lane.
    fn next_write(jobs: &ScriptedJobs) -> crate::document_request::DocumentJob {
        loop {
            match jobs.next() {
                Some(ScriptedJob::Document(job)) => return job,
                Some(_) => {}
                None => panic!("no write is queued"),
            }
        }
    }

    fn scratch(case: &str) -> std::path::PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-keybindings-merge-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        root
    }

    /// A press on the window: one row's chord, the table's departures stored.
    fn press(window: &mut Window, action: &str, chord: &str) {
        let chord: Chord = parse_chord(chord).expect("a chord");
        window.table.set(action, Some(chord));
        assert!(window.store.store(departures(&window.table)));
    }

    /// RED (§3.3, carry-forward 2) — **the door rebuilds the shortcuts after
    /// `merge_stored`, not inside it.** Folio presses New tab; a hand edit giving Close
    /// pane a chord crosses that write (`Displaced`); Folio presses Quit while the
    /// answer is on its way. The merge runs no door — the table and the store's
    /// departures are as they were when it returns — and the table put in force after
    /// it holds the file's chord, the first press, and the press laid back over the
    /// merge.
    ///
    /// MUTATION: rebuild inside the merge (in `KeybindingsStore::begin_merge`, hand the
    /// window the document the merge closure made, before `merge_stored` laid the later
    /// press back over it) — Quit's chord is missing from the table.
    #[test]
    fn keybindings_merge_rebuilds_the_shortcuts_after_merge_stored_not_inside_it() {
        let root = scratch("door-after");
        let (lane, jobs) = StorageLane::scripted();
        let store = KeybindingsStore::at_on(root.join(KEYBINDINGS_FILE_NAME), lane);
        let mut window = Window {
            store,
            jobs,
            table: Shortcuts::defaults_for(PLATFORM),
            log: Vec::new(),
            at_begin: None,
        };
        press(&mut window, "new-tab", "Ctrl+j");
        let write = next_write(&window.jobs);
        press(&mut window, "quit", "Ctrl+u");
        let before = (window.table.clone(), window.store.loaded().to_vec());

        let theirs: Arc<[u8]> =
            bt_persist::serialize_keybindings(&windows(vec![line("close-pane", Some("Ctrl+k"))]))
                .expect("a keybindings document serializes")
                .into();
        write
            .answer
            .send(DocumentAnswer {
                generation: write.generation,
                version: write.version,
                asker: write.asker,
                outcome: Outcome::Displaced {
                    bytes: Arc::clone(&theirs),
                    kept: root.join("keybindings.json.displaced-交叉 crossed"),
                },
                not_durable: None,
                retired: Vec::new(),
                left: None,
            })
            .expect("the store waits for its answer");
        let _ = window.store.take_answers();

        let outcome = run_pass(&mut window);
        assert_eq!(
            window.at_begin,
            Some(before),
            "the merge itself runs no door"
        );
        assert_eq!(&window.log[..3], ["merged", "put in force", "sent"]);
        for (action, chord) in [
            ("close-pane", "Ctrl+k"),
            ("new-tab", "Ctrl+j"),
            ("quit", "Ctrl+u"),
        ] {
            assert_eq!(
                chord_of(&window.table, action, PLATFORM).as_deref(),
                Some(chord),
                "{action} in force after the merge"
            );
        }
        assert_eq!(outcome.replaced, None, "a clean edit says nothing");
        let sent = next_write(&window.jobs);
        let written = bt_persist::parse_keybindings(&sent.bytes).expect("it parses");
        assert_eq!(
            written.bindings,
            vec![
                line("close-pane", Some("Ctrl+k")),
                line("new-tab", Some("Ctrl+j")),
                line("quit", Some("Ctrl+u")),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
