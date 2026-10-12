//! **`pins.json`, merged three ways** (`docs/plans/design/settings-reread-2026-10-11.md`
//! §3.4 `pins.json` as completed by (b)8; ticket T-STORE-MERGE-PROFILES-PINS).
//!
//! The three documents are the settings merge's (`settings_merge`): the base
//! `B`, Folio's `desired` `O`, and the file's document `T`.
//!
//! - **The identity is `(kind, target)`**, the target verbatim (`pins::row_of`).
//!   Each side is read first-wins, the menus' rule; a later row of one
//!   identity in `T` is **carried** — kept verbatim in the file's position,
//!   never merged and never rewritten ((b)8's one rule) — and so is a row of a
//!   kind this build cannot name (`bt_persist::pins`' header): it is neither
//!   dropped nor retagged. A carried duplicate goes with its identity's
//!   row: when the merge removes that row, the duplicate goes too
//!   (`document_request::remove_key`'s rule, the one an unpin follows).
//! - **A union of the additions, not of the tables.** Per identity, with
//!   presence `p`: if `O` kept `B`'s presence the file's stands, if `T` kept it
//!   Folio's stands, and when both moved they agree. An unpin is as much a
//!   change as a pin, so nothing either side removed comes back.
//! - **Remove against change is a conflict the file wins**: Folio removed a
//!   pin whose `extra` the file changed — kept, with the file's `extra`; the
//!   file removed a pin whose `extra` Folio changed — removed.
//! - **`extra` merges per key** when the pin stands on both sides, and so does
//!   the document's top-level `extra`, each key through the settings merge's
//!   table ([`unit`]): the file wins a key both sides changed.
//! - **Order is display order**: the file's rows in the file's order, then
//!   Folio's additions in Folio's order, appended as a press appends them
//!   (`pins::toggle_pin`).
//!
//! **Not detected, by design** (the design note's Departures): a pin unpinned
//! and pinned again in Folio is the state Folio held before, so a removal of it
//! in the file stands — the merge is a merge of states.
//!
//! The merge is pure: nothing here puts anything in force.

use std::collections::{HashMap, HashSet};

use bt_persist::{PINS_SCHEMA_VERSION, PinEntryV1, PinKind, PinsV1};
use serde_json::{Map, Value};

use crate::settings_merge::{Account, Unit, unit};

/// **One merge of `pins.json`.**
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Merged {
    /// The merged document `M`: what Folio now wants on disk.
    pub(crate) document: PinsV1,
    pub(crate) account: Account,
}

/// A pin's identity: its kind and its target, as the file spells them.
type Identity = (String, String);

/// The identity of a row this build can name; `None` for a kind it cannot.
fn identity(pin: &PinEntryV1) -> Option<Identity> {
    PinKind::parse(&pin.kind).map(|_| (pin.kind.clone(), pin.target.clone()))
}

/// One side's pins in force, first-wins by identity, in the side's order.
fn first_wins(document: &PinsV1) -> Vec<(Identity, &PinEntryV1)> {
    let mut seen = HashSet::new();
    document
        .pins
        .iter()
        .filter_map(|pin| identity(pin).map(|id| (id, pin)))
        .filter(|(id, _)| seen.insert(id.clone()))
        .collect()
}

/// **A map of opaque keys merged key by key** through [`unit`]: the file's
/// keys in the file's order, then Folio's, each named by `name`.
fn merge_keys(
    base: &Map<String, Value>,
    ours: &Map<String, Value>,
    theirs: &Map<String, Value>,
    name: &dyn Fn(&str) -> String,
    account: &mut Account,
) -> Map<String, Value> {
    let mut merged = Map::new();
    let mut keys: Vec<&String> = Vec::new();
    for key in theirs.keys().chain(ours.keys()).chain(base.keys()) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for key in keys {
        let (value, row) = unit(base.get(key), ours.get(key), theirs.get(key));
        account.count(row, || name(key));
        if let Some(value) = value {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// **One identity's row in the result**, or none.
fn resolve(
    base: Option<&PinEntryV1>,
    ours: Option<&PinEntryV1>,
    theirs: Option<&PinEntryV1>,
    account: &mut Account,
) -> Option<PinEntryV1> {
    let empty = Map::new();
    let target = [theirs, ours, base]
        .into_iter()
        .flatten()
        .map(|pin| pin.target.clone())
        .next()
        .unwrap_or_default();
    let name = |_: &str| target.clone();
    match (base, ours, theirs) {
        (Some(base), Some(ours), Some(theirs)) => Some(PinEntryV1 {
            extra: merge_keys(&base.extra, &ours.extra, &theirs.extra, &name, account),
            ..theirs.clone()
        }),
        // Pinned on both sides: they agree on the presence; `extra` against none.
        (None, Some(ours), Some(theirs)) => {
            account.count(Unit::Theirs, || target.clone());
            Some(PinEntryV1 {
                extra: merge_keys(&empty, &ours.extra, &theirs.extra, &name, account),
                ..theirs.clone()
            })
        }
        // Unpinned here: the file's unchanged row goes; a row whose `extra` the
        // file changed is a conflict the file wins — kept, as the file has it.
        (Some(base), None, Some(theirs)) => {
            if theirs.extra == base.extra {
                account.count(Unit::Ours, || target.clone());
                None
            } else {
                account.count(Unit::Conflict, || target.clone());
                Some(theirs.clone())
            }
        }
        // Unpinned in the file: removed, and named when Folio had changed it.
        (Some(base), Some(ours), None) => {
            let row = if ours.extra == base.extra {
                Unit::Theirs
            } else {
                Unit::Conflict
            };
            account.count(row, || target.clone());
            None
        }
        (Some(_), None, None) => {
            account.count(Unit::Theirs, || target.clone());
            None
        }
        (None, Some(ours), None) => {
            account.count(Unit::Ours, || target.clone());
            Some(ours.clone())
        }
        (None, None, Some(theirs)) => {
            account.count(Unit::Theirs, || target.clone());
            Some(theirs.clone())
        }
        (None, None, None) => None,
    }
}

/// **Merge `ours` and `theirs` against `base`** (§3.4, (b)8).
pub(crate) fn merge(base: &PinsV1, ours: &PinsV1, theirs: &PinsV1) -> Merged {
    let mut account = Account::default();
    let b: HashMap<Identity, &PinEntryV1> = first_wins(base).into_iter().collect();
    // Folio's document is read first-wins too. A later duplicate or a row of a
    // kind this build cannot name in it came from an earlier file (no press
    // makes one) and is no unit of Folio's: it is not merged, and what stands
    // of it is what the file still carries, in the file's place.
    let o_rows = first_wins(ours);
    let o: HashMap<Identity, &PinEntryV1> = o_rows.iter().cloned().collect();
    let t: HashMap<Identity, &PinEntryV1> = first_wins(theirs).into_iter().collect();

    let mut pins = Vec::with_capacity(theirs.pins.len() + o_rows.len());
    // The file's rows in the file's order: each identity's first row merged,
    // every other row carried as it stands — unless the merge removed the row
    // of its identity, which it goes with (`document_request::remove_key`'s
    // rule): a row the menus never read never becomes the pin.
    let mut seen = HashSet::new();
    let mut gone = HashSet::new();
    for pin in &theirs.pins {
        match identity(pin) {
            Some(id) if seen.insert(id.clone()) => {
                match resolve(b.get(&id).copied(), o.get(&id).copied(), Some(pin), &mut account) {
                    Some(row) => pins.push(row),
                    None => {
                        gone.insert(id);
                    }
                }
            }
            Some(id) if gone.contains(&id) => {}
            _ => pins.push(pin.clone()),
        }
    }
    // Then what the file does not hold, in Folio's order: Folio's additions,
    // and nothing either side removed.
    for (id, pin) in &o_rows {
        if !t.contains_key(id) {
            pins.extend(resolve(b.get(id).copied(), Some(pin), None, &mut account));
        }
    }
    for (id, pin) in &b {
        if !t.contains_key(id) && !o.contains_key(id) {
            pins.extend(resolve(Some(pin), None, None, &mut account));
        }
    }

    let extra = merge_keys(
        &base.extra,
        &ours.extra,
        &theirs.extra,
        &str::to_owned,
        &mut account,
    );
    Merged {
        document: PinsV1 {
            // The envelope is this build's: a newer file is refused before any
            // merge, and an older one was migrated forward when it was read.
            schema_version: PINS_SCHEMA_VERSION,
            pins,
            extra,
        },
        account,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn pin(kind: &str, target: &str) -> PinEntryV1 {
        PinEntryV1 {
            kind: kind.to_owned(),
            target: target.to_owned(),
            extra: Map::new(),
        }
    }

    fn table(pins: &[PinEntryV1]) -> PinsV1 {
        PinsV1 {
            schema_version: PINS_SCHEMA_VERSION,
            pins: pins.to_vec(),
            extra: Map::new(),
        }
    }

    fn targets(document: &PinsV1) -> Vec<&str> {
        document
            .pins
            .iter()
            .map(|pin| pin.target.as_str())
            .collect()
    }

    /// RED (§3.4, §8 test 11) — **the union is of the additions, not of the
    /// tables, and the order is the file's, then Folio's additions.**
    ///
    /// The base pins `C:\甲`, `C:\乙` and `C:\丙`. Folio unpinned `C:\乙` and
    /// pinned `D:\我的`; the file unpinned `C:\丙` and pinned `E:\他的`. Nothing
    /// either side unpinned comes back.
    ///
    /// MUTATION: a plain union (in `resolve`, keep the row whenever either side
    /// holds it) — `C:\乙` and `C:\丙` return.
    #[test]
    fn the_union_is_of_additions_not_tables_and_order_is_the_files_then_folios() {
        let (a, b, c) = (
            pin("folder", "C:\\甲"),
            pin("folder", "C:\\乙"),
            pin("file", "C:\\丙.md"),
        );
        let (mine, theirs_new) = (pin("folder", "D:\\我的"), pin("file", "E:\\他的.md"));
        let base = table(&[a.clone(), b.clone(), c.clone()]);
        let ours = table(&[a.clone(), c.clone(), mine.clone()]);
        let theirs = table(&[theirs_new.clone(), a.clone(), b.clone()]);
        let merged = merge(&base, &ours, &theirs);
        assert_eq!(
            targets(&merged.document),
            vec!["E:\\他的.md", "C:\\甲", "D:\\我的"],
            "the file's rows in its order, then Folio's addition; neither removal undone"
        );
        assert_eq!(merged.account.conflicted, 0);
        assert_eq!(merged.account.kept, 2, "Folio's unpin and pin");
        assert_eq!(merged.account.took, 2, "the file's unpin and pin");
    }

    /// RED (§3.4; `bt_persist::pins`' header) — **a row of a kind this build
    /// cannot name is carried verbatim**: its kind, target and fields, in the
    /// file's position, never dropped and never retagged.
    ///
    /// MUTATION: drop it (in `merge`, skip a row with no identity instead of
    /// carrying it) — the `workspace` row is gone.
    #[test]
    fn an_unknown_kind_is_carried_verbatim() {
        let a = pin("folder", "C:\\甲");
        let mut workspace = pin("workspace", "团队 team");
        workspace
            .extra
            .insert("members".to_owned(), json!([1, "二"]));
        let mine = pin("url", "https://example.com/中文?a=b#c");
        let base = table(std::slice::from_ref(&a));
        let ours = table(&[a.clone(), mine.clone()]);
        let theirs = table(&[workspace.clone(), a.clone()]);
        let merged = merge(&base, &ours, &theirs);
        assert_eq!(merged.document.pins, vec![workspace, a, mine]);
    }

    /// RED (review round 3, coordinator's ruling) — **a duplicate pin goes
    /// with its identity's row on the merge road**: Folio unpinned a folder the
    /// file pins twice (a later duplicate the menus never read); the file pinned
    /// something else. The merged table holds neither row of the folder.
    ///
    /// MUTATION: keep the duplicate (in `merge`, drop the `gone` arm) — the
    /// later row becomes the pin.
    #[test]
    fn a_duplicate_pin_goes_with_its_row_on_the_merge_road() {
        let folder = pin("folder", "项目 project");
        let mut later = pin("folder", "项目 project");
        later.extra.insert("注".to_owned(), json!("第二 second"));
        let other = pin("file", "笔记 notes.md");
        let base = table(&[folder.clone(), later.clone()]);
        let theirs = table(&[folder.clone(), later.clone(), other.clone()]);
        let merged = merge(&base, &table(&[]), &theirs);
        assert_eq!(merged.document.pins, vec![other]);
    }
}
