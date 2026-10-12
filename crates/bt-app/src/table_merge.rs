//! **The storage pass's merge of `profiles.json` and `pins.json`** — the two
//! tables a person may edit by hand while Folio runs
//! (`docs/plans/design/settings-reread-2026-10-11.md` §3.3, §3.4, §3.6 as
//! completed by (b)8; ticket T-STORE-MERGE-PROFILES-PINS).
//!
//! Each table's pass is one order, [`run_pass`], behind a seam the window
//! implements ([`TableMergeWindow`], `runtime/profiles.rs`):
//!
//! 1. **the store's half** — every file stored for the merge merged and folded
//!    into `desired` (`DocumentState::merge_stored`, purely), and the merged
//!    document made what the readers read;
//! 2. **the door, after it** — `profiles.json`: the installer reads the merged
//!    table (`Runtime::take_profile_table`), which follows the editor's draft by
//!    its id and re-points no pane, because a pane holds its profile's id;
//!    `pins.json`: the two PINNED sections, derived from the store whenever a
//!    menu is built, and the chrome presented;
//! 3. **the store's last step** — the merged document sent, conditional on the
//!    file the merge read, when it differs from it;
//! 4. **the pass's one line** for `diagnostics.log`, counted as the settings
//!    merge counts; and the card's data (§3.6) — the values the file won a
//!    conflict over, nothing for a clean hand edit.

use crate::document_request::ReplacedInFile;
use crate::settings_merge::{Account, PassOutcome};

/// **Which of the two tables a pass merges.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Table {
    Profiles,
    Pins,
}

/// **One storage pass's merge of one table**: the file, and every merge of the
/// pass summed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MergePass {
    file: &'static str,
    account: Account,
}

impl MergePass {
    pub(crate) fn new(file: &'static str, account: Account) -> Self {
        Self { file, account }
    }

    /// **The pass's one line for `diagnostics.log`** (§3.6): the settings
    /// merge's counts — units taken from the file without a conflict, units
    /// that kept Folio's value, conflicts.
    pub(crate) fn diagnostics(&self) -> String {
        format!(
            "BT_PERSIST {} merged: took {} from the file, kept {} from Folio, conflicts {}",
            self.file, self.account.took, self.account.kept, self.account.conflicted
        )
    }

    /// **What the pass replaced, for its card** (§3.6): nothing for a clean
    /// hand edit; otherwise the file and each value it won over Folio's, named
    /// once — a profile by its title and field, a pin by its target, a key of
    /// the file's own by its name.
    pub(crate) fn replaced(&self) -> Option<ReplacedInFile> {
        (!self.account.conflicts.is_empty()).then(|| ReplacedInFile {
            file: self.file,
            values: self.account.conflicts.clone(),
            not_applied: Vec::new(),
        })
    }
}

/// **What a table's merge needs from the window it runs in** — the seam
/// `Runtime` implements, so the pass's order is [`run_pass`]'s and nobody
/// else's.
pub(crate) trait TableMergeWindow {
    /// The store's half: `begin_merge` — merged, folded, and made what the
    /// readers read; `None` when nothing was merged.
    fn table_merge_begin(&mut self, table: Table) -> Option<MergePass>;
    /// The table's door: what the readers read, put in force.
    fn table_door(&mut self, table: Table) -> anyhow::Result<()>;
    /// The store's last step: `end_merge` — the merged document sent
    /// conditionally.
    fn table_merge_end(&mut self, table: Table);
    /// The pass's one line, written to `diagnostics.log`.
    fn table_merge_say(&mut self, line: &str);
}

/// **One storage pass's merge of `table`, in §3.3's order**: the merge and
/// the fold first, then the door — never inside the merge — then the send,
/// then the line. The card's data is made whatever the door answered.
pub(crate) fn run_pass(window: &mut impl TableMergeWindow, table: Table) -> PassOutcome {
    let Some(pass) = window.table_merge_begin(table) else {
        return PassOutcome {
            replaced: None,
            presented: Ok(()),
        };
    };
    let presented = window.table_door(table);
    window.table_merge_end(table);
    window.table_merge_say(&pass.diagnostics());
    PassOutcome {
        replaced: pass.replaced(),
        presented,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use bt_persist::{
        PINS_SCHEMA_VERSION, PROFILES_SCHEMA_VERSION, PinEntryV1, PinKind, PinsV1, ProfileEntryV1,
        ProfilesV1, ProgramV1, StableRead,
    };

    use super::*;
    use crate::document_request::StoredDocument;
    use crate::persist::{PROFILES_FILE_NAME, ProfilesStore, StorageLane, UnservedJobs};
    use crate::pins::{PINS_FILE_NAME, PinsStore};
    use crate::profiles::ProfileTable;

    fn scratch(case: &str) -> PathBuf {
        let root = bt_testpath::temp_path(&format!("bt-app-table-merge-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        root
    }

    fn own(id: &str, title: &str) -> ProfileEntryV1 {
        ProfileEntryV1 {
            id: id.to_owned(),
            display_title: Some(title.to_owned()),
            program: Some(ProgramV1::Path(format!("C:\\工具\\{id}.exe"))),
            ..ProfileEntryV1::default()
        }
    }

    fn profiles(rows: &[ProfileEntryV1]) -> ProfilesV1 {
        ProfilesV1 {
            schema_version: PROFILES_SCHEMA_VERSION,
            profiles: rows.to_vec(),
        }
    }

    fn pins(rows: &[PinEntryV1]) -> PinsV1 {
        PinsV1 {
            schema_version: PINS_SCHEMA_VERSION,
            pins: rows.to_vec(),
            extra: serde_json::Map::new(),
        }
    }

    fn bytes<T: StoredDocument>(document: &T) -> Arc<[u8]> {
        document.to_bytes().expect("the document serializes").into()
    }

    /// **A window with the two real stores, and doors that do what the real
    /// ones do**: the installer's pure half builds the table from what the
    /// profile store's readers read, the editor follows its id through
    /// `ProfileTable::position_of_id`, a pane holds its profile's id, and the
    /// pinned folders are read from the pin store. Every step is logged.
    struct Window {
        profiles: ProfilesStore,
        pins: PinsStore,
        table: ProfileTable,
        /// The editor sub-page: the row it is open on, and its draft.
        editor: Option<(usize, String)>,
        /// A pane's profile, as a leaf holds it.
        pane: String,
        pinned: Vec<String>,
        log: Vec<String>,
        _jobs: UnservedJobs,
    }

    impl TableMergeWindow for Window {
        fn table_merge_begin(&mut self, table: Table) -> Option<MergePass> {
            let pass = match table {
                Table::Profiles => self.profiles.begin_merge(),
                Table::Pins => self.pins.begin_merge(),
            };
            self.log.push(format!("{table:?} merged"));
            pass
        }

        fn table_door(&mut self, table: Table) -> anyhow::Result<()> {
            match table {
                Table::Profiles => {
                    let waits = self.profiles.merge_waits();
                    self.log
                        .push(format!("Profiles door, a merge waits: {waits}"));
                    let editing = self.editor.as_ref().and_then(|(index, _)| {
                        self.table.get(*index).map(|profile| profile.id.clone())
                    });
                    self.table = crate::profiles::table_from(self.profiles.loaded()).0;
                    if let (Some(id), Some((index, _))) = (editing, self.editor.as_mut()) {
                        *index = self
                            .table
                            .position_of_id(&id)
                            .expect("the row the editor is open on stays");
                    }
                }
                Table::Pins => {
                    let waits = self.pins.merge_waits();
                    self.log.push(format!("Pins door, a merge waits: {waits}"));
                    self.pinned = self.pins.targets(PinKind::Folder);
                }
            }
            Ok(())
        }

        fn table_merge_end(&mut self, table: Table) {
            match table {
                Table::Profiles => self.profiles.end_merge(),
                Table::Pins => self.pins.end_merge(),
            }
            self.log.push(format!("{table:?} sent"));
        }

        fn table_merge_say(&mut self, line: &str) {
            self.log.push(line.to_owned());
        }
    }

    /// RED (§3.3, §3.4, (b)8; carry-forwards 2 and 5) — **the profiles and pins
    /// merges rebuild after `merge_stored`, never inside it.**
    ///
    /// `profiles.json` holds `甲 a` then `乙 b`. Folio gave `b` arguments (not
    /// on disk); the file moved `b` first and renamed `a`. The editor is open on
    /// `a` with a half-typed draft; a pane runs `b`. `pins.json`: Folio pinned
    /// `D:\我的`, the file pinned `E:\他的`. Each table's door runs once its
    /// merge has folded — nothing waits for a merge — and puts in force the
    /// merged document: both sides' units, in the file's order. The editor
    /// follows `a` to its new place with its draft untouched; the pane's id
    /// names `b` wherever it now stands.
    ///
    /// MUTATION: run the door inside the merge (in [`run_pass`], walk
    /// `table_door` ahead of `table_merge_begin`) — the door finds the file
    /// waiting and installs the table as it was.
    #[test]
    fn profiles_and_pins_merges_rebuild_after_merge_stored_not_inside_it() {
        let root = scratch("doors");
        let (a, b) = (own("a", "甲 A"), own("b", "乙 B"));
        std::fs::write(
            root.join(PROFILES_FILE_NAME),
            &*bytes(&profiles(&[a.clone(), b.clone()])),
        )
        .expect("the profiles file");
        let folder = PinEntryV1::new(PinKind::Folder, "C:\\甲");
        std::fs::write(
            root.join(PINS_FILE_NAME),
            &*bytes(&pins(std::slice::from_ref(&folder))),
        )
        .expect("the pins file");
        let (lane, jobs) = StorageLane::unserved();
        let mut window = Window {
            profiles: ProfilesStore::at_on(root.join(PROFILES_FILE_NAME), lane.clone()),
            pins: PinsStore::at_on(root.join(PINS_FILE_NAME), lane),
            table: crate::profiles::table_from(&profiles(&[a.clone(), b.clone()])).0,
            editor: None,
            pane: "b".to_owned(),
            pinned: Vec::new(),
            log: Vec::new(),
            _jobs: jobs,
        };
        window.editor = Some((
            window.table.position_of_id("a").expect("a is a row"),
            "草稿 half-typed draft".to_owned(),
        ));

        let mut b_here = b.clone();
        b_here.args = Some(vec!["--登录".to_owned()]);
        assert!(
            window
                .profiles
                .store(profiles(&[a.clone(), b_here.clone()]))
        );
        assert!(window.pins.toggle(PinKind::Folder, "D:\\我的"));

        let mut a_there = a.clone();
        a_there.display_title = Some("甲 改名 renamed".to_owned());
        let _ = window
            .profiles
            .hear_read(StableRead::Bytes(bytes(&profiles(&[
                b.clone(),
                a_there.clone(),
            ]))));
        let _ = window
            .pins
            .document()
            .hear_read(StableRead::Bytes(bytes(&pins(&[
                folder.clone(),
                PinEntryV1::new(PinKind::Folder, "E:\\他的"),
            ]))));

        let outcome = run_pass(&mut window, Table::Profiles);
        assert!(outcome.presented.is_ok());
        assert_eq!(outcome.replaced, None, "no conflict: nothing to name");
        let outcome = run_pass(&mut window, Table::Pins);
        assert!(outcome.presented.is_ok());

        assert_eq!(
            window.log,
            vec![
                "Profiles merged".to_owned(),
                "Profiles door, a merge waits: false".to_owned(),
                "Profiles sent".to_owned(),
                "BT_PERSIST profiles.json merged: took 2 from the file, kept 1 from Folio, \
                 conflicts 0"
                    .to_owned(),
                "Pins merged".to_owned(),
                "Pins door, a merge waits: false".to_owned(),
                "Pins sent".to_owned(),
                "BT_PERSIST pins.json merged: took 1 from the file, kept 1 from Folio, \
                 conflicts 0"
                    .to_owned(),
            ],
            "each door after its fold, the send after the door, the line last"
        );
        let installed: Vec<&str> = window
            .table
            .profiles()
            .iter()
            .take(2)
            .map(|profile| profile.display_title.as_str())
            .collect();
        assert_eq!(
            installed,
            vec!["乙 B", "甲 改名 renamed"],
            "the merged table in force, in the file's order"
        );
        let pane = window
            .table
            .get(
                window
                    .table
                    .position_of_id(&window.pane)
                    .expect("the pane's profile is a row"),
            )
            .expect("and the row is there");
        assert_eq!(
            (pane.display_title.as_str(), pane.args.as_slice()),
            ("乙 B", &["--登录".to_owned()][..]),
            "the pane's id names `b`, with Folio's arguments, wherever it stands"
        );
        assert_eq!(
            window.editor,
            Some((1, "草稿 half-typed draft".to_owned())),
            "the editor follows `a` by its id, and its draft is untouched"
        );
        assert_eq!(
            window.pinned,
            vec![
                "C:\\甲".to_owned(),
                "E:\\他的".to_owned(),
                "D:\\我的".to_owned()
            ],
            "the file's pins, then Folio's"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
