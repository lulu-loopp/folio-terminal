//! **`profiles.json`, merged three ways** (`docs/plans/design/settings-reread-2026-10-11.md`
//! §3.4 `profiles.json` as completed by (b)8; ticket T-STORE-MERGE-PROFILES-PINS).
//!
//! The three documents are the settings merge's (`settings_merge`): the base
//! `B`, Folio's `desired` `O`, and the file's document `T`.
//!
//! - **Each side is read the installer's way** (`profiles::merge_on`,
//!   `profiles::installable`; coordinator's ruling, review round 4): an id's
//!   row **in force** is the first row naming it that the installer can use;
//!   an unusable row before it is **refused** — it stays in the file and stays
//!   reported; every row after it is its **duplicate**. A duplicate in `T` is
//!   **carried**: kept verbatim, never merged and never rewritten ((b)8's one
//!   rule). An id's refused rows are one unit of their own, through the
//!   settings merge's table: a side that removed or changed them changed them.
//! - **Rows are matched by id, never by position.** Within a row the unit is
//!   the field, through the settings merge's table ([`unit`]); every field but
//!   `id` is optional and absent is "the shipped value", so a field the file
//!   removed is a change to absent.
//! - **Added and deleted rows**: a row added on one side is kept; a row added
//!   on both sides with one id is the file's row whole — two different rows
//!   under one id are a conflict the file wins, named once, never a mix of the
//!   two; a row deleted on one side and unchanged on the other is deleted; a
//!   row deleted on one side and changed on the other is a conflict, and the
//!   file's side stands, whichever side that was.
//! - **The order is total** ((b)8): the rows all three hold keep `T`'s
//!   relative order if `T` reordered them against `B`, else `O`'s if `O` did,
//!   else `B`'s — the table of [`unit`] over the order itself; every other row
//!   is placed immediately after its nearest predecessor in its own side's
//!   sequence that is in the result (first when it has none), a row the file
//!   holds by the file's sequence and a row only Folio holds by Folio's; two
//!   rows anchored to the same predecessor stand `T`'s before `O`'s, each side's
//!   in its own order.
//! - **A carried row never stands before its id's row.** It is placed last,
//!   in the file's order, immediately after the last row of its own id placed
//!   so far — the id's row in force or an earlier carried row naming it — so
//!   an id's duplicates stand together in the file's order and the installer's
//!   reading puts in force the row the merge kept. A carried row goes with its
//!   id's row: when the merge removes that row, the carried rows naming it go
//!   too — the rule a press's removal follows
//!   (`document_request::remove_in_force_and_its_duplicates`) — so a row that
//!   was never in force never becomes the live one, and the id meets its
//!   shipped value or none. The refused rows an id kept stand where their side has
//!   them — after the nearest row before them the result places — and never
//!   after their id's row in force: the installer meets and refuses them at
//!   their own place, so a clean hand edit leaves them where the file has them.
//! - **Shipped ids the file never names** are appended by the installer as
//!   always; they are not units here.
//!
//! The merge is pure: nothing here puts anything in force.

use std::collections::HashMap;

use bt_persist::{PROFILES_SCHEMA_VERSION, ProfileEntryV1, ProfilesV1};

use crate::i18n::{self, Text};
use crate::settings_merge::{Account, Unit, unit};

/// **One merge of `profiles.json`.**
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Merged {
    /// The merged document `M`: what Folio now wants on disk.
    pub(crate) document: ProfilesV1,
    pub(crate) account: Account,
}

/// A place in the result: an id's row in force, a carried entry of `T` by its
/// index in the file and the id it names, or a refused row — the `l`th of the
/// `k`th id's refused rows the merge kept, and that id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Row(String),
    Carried(usize, String),
    Refused(usize, usize, String),
}

impl Slot {
    fn id(&self) -> &str {
        match self {
            Self::Row(id) | Self::Carried(_, id) | Self::Refused(_, _, id) => id,
        }
    }
}

/// **What the installer makes of one row** (`profiles::merge_on`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    /// The first row naming its id that can be used: the id's row in force.
    InForce,
    /// A row that cannot be used, before its id's row in force (or of an id
    /// with none): refused, and it stays in the file and stays reported.
    Refused,
    /// A row after its id's row in force: its duplicate, which goes with it.
    Duplicate,
}

/// **One side read the installer's way**: each row's [`Reading`], in order.
fn reading_of(document: &ProfilesV1, usable: &dyn Fn(&ProfileEntryV1) -> bool) -> Vec<Reading> {
    let mut in_force: HashMap<&str, usize> = HashMap::new();
    for (at, entry) in document.profiles.iter().enumerate() {
        if usable(entry) {
            in_force.entry(entry.id.as_str()).or_insert(at);
        }
    }
    let mut reading = Vec::with_capacity(document.profiles.len());
    for (at, entry) in document.profiles.iter().enumerate() {
        reading.push(match in_force.get(entry.id.as_str()) {
            Some(&row) if at == row => Reading::InForce,
            Some(&row) if at > row => Reading::Duplicate,
            _ => Reading::Refused,
        });
    }
    reading
}

/// One side's rows in force, in the side's order.
fn rows_in_force<'a>(document: &'a ProfilesV1, reading: &[Reading]) -> Vec<&'a ProfileEntryV1> {
    let mut rows = Vec::new();
    for (at, entry) in document.profiles.iter().enumerate() {
        if reading[at] == Reading::InForce {
            rows.push(entry);
        }
    }
    rows
}

/// One side's refused rows, by id, each id's in the side's order.
fn refused_rows<'a>(
    document: &'a ProfilesV1,
    reading: &[Reading],
) -> HashMap<&'a str, Vec<ProfileEntryV1>> {
    let mut refused: HashMap<&'a str, Vec<ProfileEntryV1>> = HashMap::new();
    for (at, entry) in document.profiles.iter().enumerate() {
        if reading[at] == Reading::Refused {
            refused
                .entry(entry.id.as_str())
                .or_default()
                .push(entry.clone());
        }
    }
    refused
}

/// The ids of a side's refused rows, each once, after those in `ids`.
fn push_refused_ids<'a>(ids: &mut Vec<&'a str>, document: &'a ProfilesV1, reading: &[Reading]) {
    for (at, entry) in document.profiles.iter().enumerate() {
        if reading[at] == Reading::Refused && !ids.contains(&entry.id.as_str()) {
            ids.push(entry.id.as_str());
        }
    }
}

/// **An id's refused rows the merge kept**, and the side whose sequence
/// places them when the id has no row in force in the result.
struct KeptRefused<'a> {
    id: String,
    rows: Vec<ProfileEntryV1>,
    source: &'a ProfilesV1,
}

fn by_id<'a>(rows: &[&'a ProfileEntryV1]) -> HashMap<&'a str, &'a ProfileEntryV1> {
    rows.iter()
        .map(|entry| (entry.id.as_str(), *entry))
        .collect()
}

/// The ids `rows` names, each once, after those already in `ids`.
fn push_ids<'a>(ids: &mut Vec<&'a str>, rows: &[&'a ProfileEntryV1]) {
    for entry in rows {
        if !ids.contains(&entry.id.as_str()) {
            ids.push(entry.id.as_str());
        }
    }
}

/// One row's fields, each through [`unit`].
struct RowMerging<'a> {
    base: &'a ProfileEntryV1,
    ours: &'a ProfileEntryV1,
    title: &'a str,
    account: &'a mut Account,
}

impl RowMerging<'_> {
    fn field<V: Clone + PartialEq>(
        &mut self,
        label: &'static str,
        at: impl Fn(&ProfileEntryV1) -> &V,
        theirs: V,
    ) -> V {
        let (value, row) = unit(at(self.base).clone(), at(self.ours).clone(), theirs);
        self.account
            .count(row, || i18n::merged_profile_value(self.title, label));
        value
    }
}

/// **A row both sides hold, merged field by field** against `base` — the
/// destructuring has no `..`, so a field added to `ProfileEntryV1` does not
/// compile here until somebody has said how it merges.
fn merge_row(
    base: &ProfileEntryV1,
    ours: &ProfileEntryV1,
    theirs: &ProfileEntryV1,
    title: &str,
    account: &mut Account,
) -> ProfileEntryV1 {
    let mut m = RowMerging {
        base,
        ours,
        title,
        account,
    };
    let ProfileEntryV1 {
        id,
        display_title,
        hidden,
        program,
        args,
        env,
        starting_dir,
        start_at,
        mark,
        integration,
        login,
        paste_as,
        paste_paths_as,
    } = theirs.clone();
    ProfileEntryV1 {
        id,
        display_title: m.field(
            Text::ProfilesRowName.text(),
            |e| &e.display_title,
            display_title,
        ),
        hidden: m.field(Text::MergedProfileHidden.text(), |e| &e.hidden, hidden),
        program: m.field(Text::ProfilesRowProgram.text(), |e| &e.program, program),
        args: m.field(Text::ProfilesRowArgs.text(), |e| &e.args, args),
        env: m.field(Text::ProfilesRowEnv.text(), |e| &e.env, env),
        starting_dir: m.field(
            Text::ProfilesRowStartingDir.text(),
            |e| &e.starting_dir,
            starting_dir,
        ),
        start_at: m.field(
            Text::ProfilesRowStartingDir.text(),
            |e| &e.start_at,
            start_at,
        ),
        mark: m.field(Text::ProfilesRowColour.text(), |e| &e.mark, mark),
        integration: m.field(
            Text::ProfilesRowIntegration.text(),
            |e| &e.integration,
            integration,
        ),
        login: m.field(Text::ProfilesRowLogin.text(), |e| &e.login, login),
        // No row of the editor shows these two: named by their key in the file.
        paste_as: m.field("paste_as", |e| &e.paste_as, paste_as),
        paste_paths_as: m.field("paste_paths_as", |e| &e.paste_paths_as, paste_paths_as),
    }
}

/// **Merge `ours` and `theirs` against `base`** (§3.4, (b)8). `shipped_title`
/// answers the title a built-in row draws when no side names one, for the
/// names the account gives.
pub(crate) fn merge(
    base: &ProfilesV1,
    ours: &ProfilesV1,
    theirs: &ProfilesV1,
    shipped_title: &dyn Fn(&str) -> Option<String>,
    usable: &dyn Fn(&ProfileEntryV1) -> bool,
) -> Merged {
    let mut account = Account::default();
    let (b_read, o_read, t_read) = (
        reading_of(base, usable),
        reading_of(ours, usable),
        reading_of(theirs, usable),
    );
    let (b_rows, o_rows) = (rows_in_force(base, &b_read), rows_in_force(ours, &o_read));
    // The file's sequence: each id's row in force, and every row after it
    // carried as its duplicate. Refused rows are units of their own.
    let mut t_sequence: Vec<Slot> = Vec::new();
    for (index, entry) in theirs.profiles.iter().enumerate() {
        match t_read[index] {
            Reading::InForce => t_sequence.push(Slot::Row(entry.id.clone())),
            Reading::Duplicate => t_sequence.push(Slot::Carried(index, entry.id.clone())),
            Reading::Refused => {}
        }
    }
    let t_rows = rows_in_force(theirs, &t_read);
    let (b, o, t) = (by_id(&b_rows), by_id(&o_rows), by_id(&t_rows));
    let title = |id: &str| -> String {
        [t.get(id), o.get(id), b.get(id)]
            .into_iter()
            .flatten()
            .find_map(|entry| entry.display_title.clone())
            .or_else(|| shipped_title(id))
            .unwrap_or_else(|| id.to_owned())
    };

    // Each id's row in the result, or none.
    let mut ids: Vec<&str> = Vec::new();
    for rows in [&t_rows, &o_rows, &b_rows] {
        push_ids(&mut ids, rows);
    }
    let mut result: HashMap<String, ProfileEntryV1> = HashMap::new();
    for id in ids {
        let name = || title(id);
        let row = match (b.get(id), o.get(id), t.get(id)) {
            (Some(base), Some(ours), Some(theirs)) => {
                Some(merge_row(base, ours, theirs, &name(), &mut account))
            }
            // Deleted in the file: unchanged here, deleted; changed here, a
            // conflict the file's side wins — deleted.
            (Some(base), Some(ours), None) => {
                let row = if ours == base {
                    Unit::Theirs
                } else {
                    Unit::Conflict
                };
                account.count(row, name);
                None
            }
            // Deleted here: unchanged in the file, deleted; changed in the
            // file, a conflict the file's side wins — kept as the file has it.
            (Some(base), None, Some(theirs)) => {
                if theirs == base {
                    account.count(Unit::Ours, name);
                    None
                } else {
                    account.count(Unit::Conflict, name);
                    Some((*theirs).clone())
                }
            }
            (Some(_), None, None) => {
                account.count(Unit::Theirs, name);
                None
            }
            (None, Some(ours), None) => {
                account.count(Unit::Ours, name);
                Some((*ours).clone())
            }
            (None, None, Some(theirs)) => {
                account.count(Unit::Theirs, name);
                Some((*theirs).clone())
            }
            // Added on both sides: one row, or two rows under one id — a
            // conflict the file's row wins whole.
            (None, Some(ours), Some(theirs)) => {
                let row = if ours == theirs {
                    Unit::Theirs
                } else {
                    Unit::Conflict
                };
                account.count(row, name);
                Some((*theirs).clone())
            }
            (None, None, None) => None,
        };
        if let Some(row) = row {
            result.insert(id.to_owned(), row);
        }
    }

    // Each id's refused rows, one unit through the table: rows a reader can
    // fix by hand stay in the file and stay reported; a side that removed or
    // changed them changed them.
    let mut refused_ids: Vec<&str> = Vec::new();
    push_refused_ids(&mut refused_ids, theirs, &t_read);
    push_refused_ids(&mut refused_ids, ours, &o_read);
    push_refused_ids(&mut refused_ids, base, &b_read);
    let (b_refused, o_refused, t_refused) = (
        refused_rows(base, &b_read),
        refused_rows(ours, &o_read),
        refused_rows(theirs, &t_read),
    );
    let mut refused: Vec<KeptRefused<'_>> = Vec::new();
    for id in refused_ids {
        let of =
            |rows: &HashMap<&str, Vec<ProfileEntryV1>>| rows.get(id).cloned().unwrap_or_default();
        let (b_rows_refused, o_rows_refused, t_rows_refused) =
            (of(&b_refused), of(&o_refused), of(&t_refused));
        let (rows, row) = unit(
            b_rows_refused,
            o_rows_refused.clone(),
            t_rows_refused.clone(),
        );
        account.count(row, || title(id));
        if rows.is_empty() {
            continue;
        }
        let source = if rows == t_rows_refused {
            theirs
        } else if rows == o_rows_refused {
            ours
        } else {
            base
        };
        refused.push(KeptRefused {
            id: id.to_owned(),
            rows,
            source,
        });
    }

    let profiles = order(
        &b_rows,
        &o_rows,
        &t_rows,
        &t_sequence,
        &result,
        &refused,
        &mut account,
    )
    .into_iter()
    .map(|slot| match slot {
        Slot::Row(id) => result[&id].clone(),
        Slot::Carried(index, _) => theirs.profiles[index].clone(),
        Slot::Refused(k, l, _) => refused[k].rows[l].clone(),
    })
    .collect();
    Merged {
        document: ProfilesV1 {
            // The envelope is this build's: a newer file is refused before any
            // merge, and an older one was migrated forward when it was read.
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles,
        },
        account,
    }
}

/// **The result's total order** ((b)8 steps 1–3).
fn order(
    b_rows: &[&ProfileEntryV1],
    o_rows: &[&ProfileEntryV1],
    t_rows: &[&ProfileEntryV1],
    t_sequence: &[Slot],
    result: &HashMap<String, ProfileEntryV1>,
    refused: &[KeptRefused<'_>],
    account: &mut Account,
) -> Vec<Slot> {
    let holds = |rows: &[&ProfileEntryV1], id: &str| rows.iter().any(|entry| entry.id == id);
    let common = |id: &str| {
        result.contains_key(id) && holds(b_rows, id) && holds(o_rows, id) && holds(t_rows, id)
    };
    let common_order = |rows: &[&ProfileEntryV1]| -> Vec<String> {
        rows.iter()
            .filter(|entry| common(&entry.id))
            .map(|entry| entry.id.clone())
            .collect()
    };
    // Step 1: the common rows' relative order is a unit of its own.
    let (spine, row) = unit(
        common_order(b_rows),
        common_order(o_rows),
        common_order(t_rows),
    );
    account.count(row, || Text::MergedProfilesOrder.text().to_owned());

    // Step 2: every other row, anchored to its nearest predecessor in its own
    // side's sequence that is in the result. Step 3: `T`'s anchored rows before
    // `O`'s, each side's in its own order — the order they are pushed in.
    // The carried rows are placed last, so a row anchors to rows only.
    let a_row_in_result = |slot: &Slot| matches!(slot, Slot::Row(id) if result.contains_key(id));
    let mut anchored: HashMap<Option<Slot>, Vec<Slot>> = HashMap::new();
    for (at, slot) in t_sequence.iter().enumerate() {
        let Slot::Row(id) = slot else {
            continue;
        };
        if !result.contains_key(id) || common(id) {
            continue;
        }
        let anchor = t_sequence[..at]
            .iter()
            .rev()
            .find(|earlier| a_row_in_result(earlier))
            .cloned();
        anchored.entry(anchor).or_default().push(slot.clone());
    }
    for (at, entry) in o_rows.iter().enumerate() {
        let id = entry.id.as_str();
        if !result.contains_key(id) || common(id) || holds(t_rows, id) {
            continue;
        }
        let anchor = o_rows[..at]
            .iter()
            .rev()
            .find(|earlier| result.contains_key(&earlier.id))
            .map(|earlier| Slot::Row(earlier.id.clone()));
        anchored
            .entry(anchor)
            .or_default()
            .push(Slot::Row(id.to_owned()));
    }

    // Each slot, then what is anchored to it, depth first.
    let mut placed = Vec::with_capacity(result.len());
    let mut pending: Vec<Slot> = Vec::new();
    let place = |pending: &mut Vec<Slot>, placed: &mut Vec<Slot>| {
        while let Some(slot) = pending.pop() {
            if let Some(children) = anchored.get(&Some(slot.clone())) {
                pending.extend(children.iter().rev().cloned());
            }
            placed.push(slot);
        }
    };
    if let Some(first) = anchored.get(&None) {
        pending.extend(first.iter().rev().cloned());
    }
    place(&mut pending, &mut placed);
    for id in spine {
        pending.push(Slot::Row(id));
        place(&mut pending, &mut placed);
    }

    // The carried rows, in the file's order: each right after the last slot of
    // its own id placed so far — its id's row, or an earlier carried row naming
    // it — so it never stands before that row, and the rows of one id stay
    // together in the file's order.
    for slot in t_sequence {
        let Slot::Carried(_, id) = slot else {
            continue;
        };
        // A carried row goes with its id's row (the rule of
        // `document_request::remove_in_force_and_its_duplicates`): one the merge
        // removed takes it along.
        if !result.contains_key(id) {
            continue;
        }
        let last_of_its_id = placed.iter().rposition(|placed| placed.id() == id);
        placed.insert(last_of_its_id.map_or(0, |at| at + 1), slot.clone());
    }

    // The refused rows each id kept, in their order, where they stand in the
    // sequence of the side they came from: after the nearest row before them
    // that the result places — and never after their id's row in force, which
    // the installer reaches only past them. A clean hand edit therefore keeps
    // them where the file has them.
    for (k, kept) in refused.iter().enumerate() {
        let slots = (0..kept.rows.len()).map(|l| Slot::Refused(k, l, kept.id.clone()));
        let first = kept
            .source
            .profiles
            .iter()
            .position(|entry| entry.id == kept.id)
            .unwrap_or(0);
        let anchor = kept.source.profiles[..first]
            .iter()
            .rev()
            .find_map(|earlier| placed.iter().rposition(|placed| placed.id() == earlier.id))
            .map_or(0, |at| at + 1);
        let its_row = placed
            .iter()
            .position(|placed| *placed == Slot::Row(kept.id.clone()));
        let at = its_row.map_or(anchor, |row| anchor.min(row));
        placed.splice(at..at, slots);
    }
    placed
}

#[cfg(test)]
mod tests {
    use bt_persist::ProgramV1;

    use super::*;

    /// A row of the reader's own: an id, a title and a program.
    fn own(id: &str, title: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            id: id.to_owned(),
            display_title: Some(title.to_owned()),
            program: Some(ProgramV1::Path(format!("C:\\工具\\{id}.exe"))),
            ..ProfileEntryV1::default()
        }
    }

    /// A built-in row naming only its id.
    fn shipped(id: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            id: id.to_owned(),
            ..ProfileEntryV1::default()
        }
    }

    fn table(rows: &[ProfileEntryV1]) -> ProfilesV1 {
        ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: rows.to_vec(),
        }
    }

    fn ids(document: &ProfilesV1) -> Vec<&str> {
        document
            .profiles
            .iter()
            .map(|entry| entry.id.as_str())
            .collect()
    }

    fn no_titles(_: &str) -> Option<String> {
        None
    }

    /// The merge, reading each side as the installer of this build does.
    fn merge(
        base: &ProfilesV1,
        ours: &ProfilesV1,
        theirs: &ProfilesV1,
        shipped_title: &dyn Fn(&str) -> Option<String>,
    ) -> Merged {
        super::merge(
            base,
            ours,
            theirs,
            shipped_title,
            &crate::profiles::installable(),
        )
    }

    fn titled(id: &str) -> Option<String> {
        (id == "pwsh").then(|| "PowerShell 七".to_owned())
    }

    /// RED ((b)14-11, (b)8 step 2) — **a row only the file added is placed
    /// after its predecessor in the file when only Folio reordered.**
    ///
    /// The base is `a b c`; Folio moved `c` first (`c a b`); the file added
    /// `新` after `a` (`a 新 b c`). The common rows take Folio's order — the
    /// file did not reorder them — and the file's row stands right after `a`.
    ///
    /// MUTATION: revision (a)'s order rule — the file's additions at the end
    /// (in `order`, anchor every row the file added to the last common row) —
    /// the row lands after `b`.
    #[test]
    fn a_file_only_row_is_placed_when_only_folio_reordered() {
        let (a, b, c, new) = (
            own("a", "甲 A"),
            own("b", "乙 B"),
            own("c", "丙 C"),
            own("新", "新的 New"),
        );
        let base = table(&[a.clone(), b.clone(), c.clone()]);
        let ours = table(&[c.clone(), a.clone(), b.clone()]);
        let theirs = table(&[a.clone(), new.clone(), b.clone(), c.clone()]);
        let merged = merge(&base, &ours, &theirs, &no_titles);
        assert_eq!(
            ids(&merged.document),
            vec!["c", "a", "新", "b"],
            "Folio's order of the common rows, the file's row after `a`"
        );
        assert_eq!(merged.account.kept, 1, "the order is Folio's");
        assert_eq!(merged.account.conflicted, 0);
    }

    /// RED ((b)14-11, (b)8's one rule) — **a later row naming an id the file
    /// already named is carried**, verbatim, in the file's position; the first
    /// is the row the merge works on, as the installer's first-wins reading.
    ///
    /// MUTATION: drop the duplicate (in `merge`, map a later `T` row to no slot
    /// instead of `Slot::Carried`) — the second `a` is gone.
    #[test]
    fn a_later_duplicate_id_is_carried() {
        let a = own("a", "甲 A");
        let b = own("b", "乙 B");
        let duplicate = ProfileEntryV1 {
            display_title: Some("重复 Duplicate".to_owned()),
            ..own("a", "unused")
        };
        let base = table(&[a.clone(), b.clone()]);
        let mut a_here = a.clone();
        a_here.args = Some(vec!["-NoLogo".to_owned(), "参数".to_owned()]);
        let ours = table(&[a_here.clone(), b.clone()]);
        let theirs = table(&[a.clone(), duplicate.clone(), b.clone()]);
        let merged = merge(&base, &ours, &theirs, &no_titles);
        assert_eq!(
            merged.document.profiles,
            vec![a_here, duplicate, b],
            "the first `a` merged (Folio's arguments), the second carried in its place"
        );
        let (installed, _) = crate::profiles::table_from(&merged.document);
        let in_force = installed
            .get(installed.position_of_id("a").expect("`a` is a row"))
            .expect("and in the table");
        assert_eq!(
            (in_force.display_title.as_str(), in_force.args.as_slice()),
            ("甲 A", &["-NoLogo".to_owned(), "参数".to_owned()][..]),
            "read back first-wins: the merged row is the one in force, not the carried one"
        );
    }

    /// RED (§8 test 12) — **rows merge by id, and a file that reordered keeps
    /// its order.**
    ///
    /// The file moved `b` first and changed `a`'s title; Folio changed `b`'s
    /// arguments. Matched by id, each change lands on its own row, in the
    /// file's order.
    ///
    /// MUTATION: match rows by position (in `merge`, look each side's row up by
    /// its index in `T` instead of by id) — `b`'s arguments land on `a`.
    #[test]
    fn rows_merge_by_id_and_a_reordered_file_keeps_its_order() {
        let (a, b, c) = (own("a", "甲 A"), own("b", "乙 B"), own("c", "丙 C"));
        let base = table(&[a.clone(), b.clone(), c.clone()]);
        let mut b_here = b.clone();
        b_here.args = Some(vec!["--登录".to_owned()]);
        let ours = table(&[a.clone(), b_here.clone(), c.clone()]);
        let mut a_there = a.clone();
        a_there.display_title = Some("甲 renamed".to_owned());
        let theirs = table(&[b.clone(), a_there.clone(), c.clone()]);
        let merged = merge(&base, &ours, &theirs, &no_titles);
        assert_eq!(
            merged.document.profiles,
            vec![b_here, a_there, c],
            "each change on its own row, in the file's order"
        );
        assert_eq!(merged.account.conflicted, 0);
        assert_eq!(merged.account.kept, 1, "`b`'s arguments");
        assert_eq!(merged.account.took, 2, "`a`'s title and the order");
    }

    /// RED (§3.4) — **a row deleted on one side and changed on the other takes
    /// the file's side**, in both directions, and is named.
    ///
    /// MUTATION: deletion always wins (in `merge`, the `(Some, None, Some)` arm
    /// answers `None` whatever the file did) — the row the file changed and
    /// Folio deleted is gone.
    #[test]
    fn a_row_deleted_on_one_side_and_changed_on_the_other_takes_the_files_side() {
        let (a, b) = (own("a", "甲 A"), own("b", "乙 B"));
        let base = table(&[a.clone(), b.clone()]);

        // Deleted in the file, changed here: deleted.
        let mut a_here = a.clone();
        a_here.hidden = true;
        let merged = merge(
            &base,
            &table(&[a_here, b.clone()]),
            &table(std::slice::from_ref(&b)),
            &no_titles,
        );
        assert_eq!(ids(&merged.document), vec!["b"], "the file deleted it");
        assert_eq!(merged.account.conflicts, vec!["甲 A".to_owned()]);

        // Deleted here, changed in the file: kept as the file has it.
        let mut a_there = a.clone();
        a_there.display_title = Some("甲 改名 renamed".to_owned());
        let merged = merge(
            &base,
            &table(std::slice::from_ref(&b)),
            &table(&[a_there.clone(), b.clone()]),
            &no_titles,
        );
        assert_eq!(
            merged.document.profiles,
            vec![a_there, b.clone()],
            "the file changed it: it stands, not resurrected from Folio, but kept from the file"
        );
        assert_eq!(merged.account.conflicts, vec!["甲 改名 renamed".to_owned()]);

        // Deleted on one side, unchanged on the other: deleted, no conflict.
        let merged = merge(
            &base,
            &table(std::slice::from_ref(&b)),
            &table(&[a.clone(), b.clone()]),
            &no_titles,
        );
        assert_eq!(ids(&merged.document), vec!["b"]);
        assert_eq!(merged.account.conflicted, 0);
    }

    /// RED (§3.3's table, §3.4) — **fields merge per the unit table, and an
    /// absent field is the shipped value**: a field the file removed is a
    /// change to absent; both sides changing one field to two values is a
    /// conflict the file wins, named by the row's title and the field.
    ///
    /// MUTATION: the conflict takes `O` (in `settings_merge::unit`, answer
    /// `ours` for the last row) — the program is Folio's.
    #[test]
    fn fields_merge_per_the_unit_table_and_an_absent_field_is_the_shipped_value() {
        let mut base_row = shipped("pwsh");
        base_row.args = Some(vec!["-NoLogo".to_owned()]);
        base_row.program = Some(ProgramV1::Path("C:\\旧\\pwsh.exe".to_owned()));
        let base = table(std::slice::from_ref(&base_row));

        let mut ours_row = base_row.clone();
        ours_row.hidden = true;
        ours_row.program = Some(ProgramV1::Path("C:\\我们\\pwsh.exe".to_owned()));
        let mut theirs_row = base_row.clone();
        theirs_row.args = None;
        theirs_row.program = Some(ProgramV1::Path("D:\\他们\\pwsh.exe".to_owned()));
        let merged = merge(&base, &table(&[ours_row]), &table(&[theirs_row]), &titled);

        let row = &merged.document.profiles[0];
        assert!(row.hidden, "changed here only: Folio's");
        assert_eq!(
            row.args, None,
            "the file removed the arguments: the shipped value, not the base's"
        );
        assert_eq!(
            row.program,
            Some(ProgramV1::Path("D:\\他们\\pwsh.exe".to_owned())),
            "changed on both: the file's"
        );
        assert_eq!(
            merged.account.conflicts,
            vec![i18n::merged_profile_value(
                "PowerShell 七",
                Text::ProfilesRowProgram.text()
            )],
            "named by the shipped title and the field"
        );
        assert_eq!(merged.account.kept, 1);
        assert_eq!(merged.account.took, 1);
    }

    /// (b)8 steps 2–3 — **two rows anchored to one predecessor stand the file's
    /// before Folio's**, and a row only Folio added follows its own
    /// predecessor; with nothing owed here the result is the file itself.
    #[test]
    fn rows_anchored_together_stand_the_files_first_and_a_clean_edit_is_the_file() {
        let (a, b) = (own("a", "甲 A"), own("b", "乙 B"));
        let (mine, theirs_new) = (own("我的", "我的 Mine"), own("他的", "他的 Theirs"));
        let base = table(&[a.clone(), b.clone()]);
        let ours = table(&[a.clone(), mine.clone(), b.clone()]);
        let theirs = table(&[a.clone(), theirs_new.clone(), b.clone()]);
        let merged = merge(&base, &ours, &theirs, &no_titles);
        assert_eq!(ids(&merged.document), vec!["a", "他的", "我的", "b"]);

        let clean = merge(&base, &base, &theirs, &no_titles);
        assert_eq!(clean.document, theirs, "a clean hand edit is the file");
        assert_eq!(clean.account.conflicted, 0);
    }

    /// The row the installer puts in force for `id` from `document`, and the
    /// entries it refused — the merged document read back first-wins.
    fn in_force(
        document: &ProfilesV1,
        id: &str,
    ) -> (Option<String>, Vec<crate::profiles::ProfileFault>) {
        let (installed, faults) = crate::profiles::table_from(document);
        let title = installed
            .position_of_id(id)
            .and_then(|index| installed.get(index))
            .map(|profile| profile.display_title.clone());
        (title, faults)
    }

    /// RED (review round 2, the blocker) — **a carried row never stands before
    /// its id's row**, so the installer's first-wins reading puts in force the
    /// row the merge kept.
    ///
    /// The base and the file hold `a`, `b`, then `a′` (a later row naming `a`);
    /// the file also renamed `b`. Folio moved `b` up, which keeps `a′` after `a`
    /// in its document. The spine is Folio's order, `b a`; `a′` follows `a`, not
    /// `b`. Read back, the row in force for `a` is `a`, and the installer
    /// refuses the same duplicate it refuses in the file — no new card.
    /// The mirror: the first `a` cannot start (no program), so the installer
    /// takes `a′` from the file; it takes `a′` from the merged document too.
    ///
    /// MUTATION: anchor a carried row to its predecessor in the file only (in
    /// `order`, `after_predecessor` alone, revision one's rule) — `a′` stands
    /// after `b`, ahead of `a`, and is put in force.
    #[test]
    fn a_carried_row_never_stands_before_its_ids_row() {
        let shadow = |first: ProfileEntryV1| {
            let b = own("b", "乙 B");
            let later = own("a", "后来的 a′ later");
            let base = table(&[first.clone(), b.clone(), later.clone()]);
            let mut b_there = b.clone();
            b_there.display_title = Some("乙 改名 renamed".to_owned());
            let theirs = table(&[first.clone(), b_there.clone(), later.clone()]);
            let ours = table(&[b.clone(), first.clone(), later.clone()]);
            let merged = merge(&base, &ours, &theirs, &no_titles);
            assert_eq!(
                merged.document.profiles,
                vec![b_there.clone(), first.clone(), later.clone()],
                "Folio's order of the rows, the carried row after its id's row"
            );
            assert_eq!(
                in_force(&merged.document, "a"),
                in_force(&theirs, "a"),
                "read back first-wins: the same row in force, the same refusals"
            );
            in_force(&merged.document, "a").0
        };
        assert_eq!(
            shadow(own("a", "甲 A")),
            Some("甲 A".to_owned()),
            "the first row is the one in force"
        );
        let unusable = ProfileEntryV1 {
            program: None,
            ..own("a", "不能启动 cannot start")
        };
        assert_eq!(
            shadow(unusable),
            Some("后来的 a′ later".to_owned()),
            "an unusable first row leaves the later one in force, as in the file"
        );
    }

    /// RED (review round 2, coordinator's ruling) — **a row added on both
    /// sides under one id is the file's row whole**: two rows Folio and the
    /// file made apart are a conflict the file wins, named once — never a mix
    /// of the two. One row added the same on both sides is no conflict.
    ///
    /// MUTATION: merge the two field by field against the shipped value
    /// (round one's rule) — the result carries Folio's arguments on the file's
    /// row.
    #[test]
    fn a_row_added_on_both_sides_under_one_id_is_the_files_row_whole() {
        let a = own("a", "甲 A");
        let base = table(std::slice::from_ref(&a));
        let mut mine = own("新", "我的 Mine");
        mine.args = Some(vec!["--我的".to_owned()]);
        let mut theirs_row = own("新", "他的 Theirs");
        theirs_row.env = Some(std::collections::BTreeMap::from([(
            "语言".to_owned(),
            "中文".to_owned(),
        )]));
        let merged = merge(
            &base,
            &table(&[a.clone(), mine.clone()]),
            &table(&[a.clone(), theirs_row.clone()]),
            &no_titles,
        );
        assert_eq!(merged.document.profiles, vec![a.clone(), theirs_row]);
        assert_eq!(merged.account.conflicts, vec!["他的 Theirs".to_owned()]);

        let same = merge(
            &base,
            &table(&[a.clone(), mine.clone()]),
            &table(&[a.clone(), mine.clone()]),
            &no_titles,
        );
        assert_eq!(same.document.profiles, vec![a, mine]);
        assert_eq!(same.account.conflicted, 0);
    }

    /// A built-in id of this build, a row of Folio's own for it, and a later
    /// row naming it again — the duplicate the file carries.
    fn built_in_and_its_shadow() -> (String, ProfileEntryV1, ProfileEntryV1) {
        let id = crate::profiles::shipped_order()[0].to_owned();
        let first = ProfileEntryV1 {
            display_title: Some("我的 Mine".to_owned()),
            ..shipped(&id)
        };
        let shadow = ProfileEntryV1 {
            display_title: Some("影子 shadow".to_owned()),
            ..shipped(&id)
        };
        (id, first, shadow)
    }

    /// The shipped title of `id`, which a document naming nothing of it shows.
    fn shipped_title(id: &str) -> Option<String> {
        in_force(&table(&[]), id).0
    }

    /// RED (review round 3, coordinator's ruling) — **a carried row goes with
    /// its id's row on the merge road**: Folio removed the row, the file carries
    /// a later duplicate of it; the merged document holds neither, and the id
    /// meets its shipped value at the next start — never the duplicate, which
    /// was never in force. A row of the reader's own is simply gone.
    ///
    /// MUTATION: keep the duplicate (in `order`, place a carried row whatever
    /// became of its id's row) — the duplicate is put in force.
    #[test]
    fn a_carried_row_goes_with_its_ids_row_on_the_merge_road() {
        let (id, first, shadow) = built_in_and_its_shadow();
        let b = own("b", "乙 B");
        let base = table(&[first.clone(), b.clone(), shadow.clone()]);
        let mut b_there = b.clone();
        b_there.display_title = Some("乙 改名 renamed".to_owned());
        let theirs = table(&[first.clone(), b_there.clone(), shadow.clone()]);
        let merged = merge(&base, &table(std::slice::from_ref(&b)), &theirs, &no_titles);
        assert_eq!(merged.document.profiles, vec![b_there.clone()]);
        assert_eq!(in_force(&merged.document, &id).0, shipped_title(&id));

        let (a, later) = (own("a", "甲 A"), own("a", "后来的 a′ later"));
        let base = table(&[a.clone(), b.clone(), later.clone()]);
        let theirs = table(&[a.clone(), b_there.clone(), later.clone()]);
        let merged = merge(&base, &table(std::slice::from_ref(&b)), &theirs, &no_titles);
        assert_eq!(merged.document.profiles, vec![b_there]);
        assert_eq!(in_force(&merged.document, "a").0, None);
    }

    /// RED (review round 3, coordinator's ruling) — **a press that removes a
    /// row takes its carried duplicates with it**
    /// (`document_request::remove_in_force_and_its_duplicates`, the merge road's
    /// rule): `desired` holds the row and a later duplicate the
    /// installer refused; the projection, the table in force, has only the row;
    /// the press removes it. The next start shows the id at its shipped value.
    ///
    /// MUTATION: keep the duplicate (in `remove_in_force_and_its_duplicates`,
    /// remove only the entry in force — the Delete before round 3) — the duplicate is the row in
    /// force at the next start.
    #[test]
    fn a_press_that_removes_a_row_takes_its_carried_duplicates() {
        use crate::document_request::StoredDocument;

        let (id, first, shadow) = built_in_and_its_shadow();
        let b = own("b", "乙 B");
        let mut desired = table(&[first.clone(), b.clone(), shadow]);
        let before = table(&[first, b.clone()]);
        let after = table(std::slice::from_ref(&b));
        ProfilesV1::apply_press(&mut desired, &before, &after);
        assert_eq!(desired.profiles, vec![b]);
        assert_eq!(in_force(&desired, &id).0, shipped_title(&id));
    }

    /// RED (review round 3) — **two carried rows of one id keep the file's
    /// relative order**, together after their id's row, even when a common row
    /// between them moved.
    ///
    /// The base and the file hold `a` (an id of the reader's own with no
    /// program, which the installer refuses), `a′`, `c`, then `a″`; Folio moved
    /// `c` up. Read from the file, `a′` is in force and `a″` is refused as a
    /// duplicate; read from the merged document, the same.
    ///
    /// MUTATION: place a carried row after whichever stands later of its file
    /// predecessor and its id's row (round 2's rule) — `a″` lands before `a′`
    /// and is put in force.
    #[test]
    fn two_carried_rows_of_one_id_keep_the_files_order() {
        let first = ProfileEntryV1 {
            program: None,
            ..own("a", "不能启动 cannot start")
        };
        let (second, third) = (own("a", "第二 a′"), own("a", "第三 a″"));
        let c = own("c", "丙 C");
        let file = table(&[first.clone(), second.clone(), c.clone(), third.clone()]);
        let ours = table(&[c.clone(), first.clone(), second.clone(), third.clone()]);
        let merged = merge(&file, &ours, &file, &no_titles);
        assert_eq!(merged.document.profiles, vec![c, first, second, third]);
        assert_eq!(
            in_force(&merged.document, "a"),
            in_force(&file, "a"),
            "read back first-wins: `a′` in force, the same refusals"
        );
    }

    /// An id of the reader's own with no program: the installer refuses it.
    fn cannot_start(id: &str, title: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            program: None,
            ..own(id, title)
        }
    }

    /// PIN (review round 4) — **`installable` is the installer's own test**:
    /// an entry it answers `true` for is put in force, one it answers `false`
    /// for is refused as unusable.
    #[test]
    fn installable_is_the_installers_own_test() {
        let usable = crate::profiles::installable();
        let built_in = shipped(crate::profiles::shipped_order()[0]);
        for entry in [
            built_in,
            own("自己的 own", "我的 Mine"),
            cannot_start("不能 cannot", "不能启动"),
        ] {
            let (_, faults) = crate::profiles::table_from(&table(std::slice::from_ref(&entry)));
            let refused = faults.contains(&crate::profiles::ProfileFault::Unusable {
                id: entry.id.clone(),
            });
            assert_eq!(usable(&entry), !refused, "{}", entry.id);
        }
    }

    /// RED (review round 4, coordinator's ruling) — **an id's row is its first
    /// usable row**: the file holds `a` (refused: no program) and then `a′`,
    /// the row in force; the file edits `a′`, and Folio dropped the refused
    /// `a`. The file's edit to the live row is the file's change — kept, no
    /// conflict — and Folio's removal of the refused row is Folio's.
    ///
    /// MUTATION: read the first row of an id as its row whatever the installer
    /// makes of it (in `merge`, every row usable — round 3's reading) — `a′`
    /// is merged against the refused `a` and the file's edit is lost.
    #[test]
    fn the_files_edit_to_the_live_row_stands_when_folio_dropped_a_refused_row() {
        let refused = cannot_start("a", "不能启动 cannot start");
        let live = own("a", "活的 live");
        let mut edited = live.clone();
        edited.args = Some(vec!["--编辑".to_owned()]);
        let base = table(&[refused.clone(), live.clone()]);
        let ours = table(std::slice::from_ref(&live));
        let theirs = table(&[refused, edited.clone()]);
        let merged = merge(&base, &ours, &theirs, &no_titles);
        assert_eq!(merged.document.profiles, vec![edited]);
        assert_eq!(merged.account.conflicted, 0, "no card");
        assert_eq!(
            in_force(&merged.document, "a").0,
            Some("活的 live".to_owned())
        );
    }

    /// RED (review round 4, coordinator's ruling) — **a Delete takes the row in
    /// force and its duplicates, and leaves a refused row before it**:
    /// `desired` holds `a` (refused), `a′` (in force) and `a″` (its duplicate);
    /// the table in force has `a′`; the press removes it. What stays is the
    /// refused `a`, which the next start refuses as before.
    ///
    /// MUTATION: the first row is the row in force whatever the installer
    /// makes of it (in `ProfilesV1::apply_press`, `InForce::First(&|_| true)`)
    /// — the refused `a` is removed with the rest.
    #[test]
    fn a_delete_takes_the_live_row_and_its_duplicates_and_leaves_a_refused_row() {
        use crate::document_request::StoredDocument;

        let refused = cannot_start("a", "不能启动 cannot start");
        let (live, later) = (own("a", "活的 live"), own("a", "后来的 later"));
        let b = own("b", "乙 B");
        let mut desired = table(&[refused.clone(), live.clone(), b.clone(), later]);
        ProfilesV1::apply_press(
            &mut desired,
            &table(&[live, b.clone()]),
            &table(std::slice::from_ref(&b)),
        );
        assert_eq!(desired.profiles, vec![refused, b]);
        assert_eq!(
            in_force(&desired, "a"),
            (
                None,
                vec![crate::profiles::ProfileFault::Unusable { id: "a".to_owned() }]
            ),
            "the next start refuses `a` as before, and puts nothing of it in force"
        );
    }

    /// RED (review round 5) — **a refused row stays where the file has it**:
    /// the file holds `a` (refused), `x`, then `a′` (the row in force); the
    /// file edits only `x`'s title, and nothing is owed here. The merge is the
    /// file, row for row — a clean hand edit never rewrites the file.
    ///
    /// MUTATION: place an id's refused rows right before its row in force
    /// (round 4's rule) — `a` moves after `x`.
    #[test]
    fn a_refused_row_stays_where_the_file_has_it() {
        let refused = cannot_start("a", "不能启动 cannot start");
        let (x, live) = (own("x", "叉 X"), own("a", "活的 live"));
        let mut x_there = x.clone();
        x_there.display_title = Some("叉 改名 renamed".to_owned());
        let base = table(&[refused.clone(), x, live.clone()]);
        let theirs = table(&[refused, x_there, live]);
        let merged = merge(&base, &base, &theirs, &no_titles);
        assert_eq!(merged.document, theirs, "the clean hand edit is the file");
        assert_eq!(merged.account.conflicted, 0);
    }
}
