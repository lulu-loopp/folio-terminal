//! **The things the user said to keep** — `pins.json`, and the two PINNED
//! sections drawn from it (user ruling 2026-08-19; `docs/plans/web-preview/plan.md`
//! §0 收藏条目).
//!
//! # Why this is a pin and not a bookmark
//!
//! The ruling's first sentence: *"收藏 = 钉 … 不引入书签管理器"*. This product
//! already has a word for "keep this one where I can see it" — a tab wears it,
//! and the focus card wears it — and a second vocabulary would mean two mental
//! models for one gesture. So the control is the tab's own pin glyph
//! ([`crate::marks::ChromeMark::Pin`], regular when you *could* pin and filled
//! when it *is* pinned), it appears on the row under the pointer, and a pinned
//! row keeps it visible because that is a fact about the row rather than an
//! offer. There is no manager, no hierarchy, no import and no export.
//!
//! # One table, three categories, two menus
//!
//! [`bt_persist::PinsV1`] holds folders, files and URLs in one array. Folders are
//! offered by the root menu's PINNED section, files by the preview switcher's,
//! and URLs by nothing yet — the web block (W2) consumes this same store rather
//! than starting one of its own, which is the whole reason the table is written
//! with all three categories in it now.
//!
//! # What identity is
//!
//! **The target string, verbatim.** Not a canonicalised path, not a case-folded
//! one: `profiles::root_choices` de-duplicates the folders it offers on exactly
//! this comparison, and a pin that thought `C:\Work` and `C:\work` were one row
//! while the menu beside it thought they were two would be a menu that cannot
//! draw its own state. Canonicalisation is a filesystem question with a
//! filesystem's failure modes, and answering it here would mean a pin whose
//! identity changes when a drive is unplugged.
//!
//! # What is written, and when
//!
//! Every press. There is no debounce, for [`crate::persist::ProfilesStore`]'s
//! sharpened reason: a pin happens because somebody clicked a pin and watched a
//! row move to the top, and a quiet window is a window in which exactly that can
//! be lost. And nothing is written until something is pinned — a machine where
//! nobody has pinned anything has no such file and gets none.

use std::path::{Path, PathBuf};

use bt_persist::{PinEntryV1, PinKind, PinsV1, read_pins, read_pins_keeping_bytes};

use crate::document_request::{DocumentState, QuitDocument, ReadNews};
use crate::persist::{Revision, StorageLane};

/// The name the pin file wears on disk, which is also what a notice about it has
/// to say out loud.
pub const PINS_FILE_NAME: &str = "pins.json";

/// Pin what is not pinned, unpin what is — and say which way it went.
///
/// **Appended, never inserted.** Array order is display order, so a new pin goes
/// to the end of its section and the rows already there do not move under the
/// pointer of the hand that is still on the menu.
///
/// Rows of other categories, and rows of no category this build knows, are not
/// touched: this walks for one target under one tag and edits its rows only. An
/// unpin takes every row of that target and tag — the first, which the menus
/// read, and the later duplicates they never did, which go with it
/// (`document_request::First::alongside`; every pin row is
/// usable), so a duplicate never becomes the pin.
pub fn toggle_pin(table: &mut PinsV1, kind: PinKind, target: &str) -> bool {
    match row_of(table, kind, target) {
        Some(_) => {
            crate::document_request::unpin(&mut table.pins, kind.tag(), target);
            false
        }
        None => {
            table.pins.push(PinEntryV1::new(kind, target));
            true
        }
    }
}

/// **Every pinned row of any of these categories, in the file's own order.**
///
/// [`pinned_targets`] for a surface that draws more than one category in one
/// list — the preview switcher, which since W2 slice ③ holds files and pages
/// together. The category comes back with the target because that is what
/// identifies a row here ([`row_of`]): the table is one array, and a caller that
/// kept only the strings would have to guess which section a row came from.
///
/// One walk over the file rather than one per category, so the interleaving the
/// user made by pinning in some order is the interleaving the menu draws.
pub fn pinned_rows<'a>(
    table: &'a PinsV1,
    kinds: &'a [PinKind],
) -> impl Iterator<Item = (PinKind, &'a str)> {
    table.pins.iter().filter_map(move |pin| {
        let kind = pin.kind()?;
        kinds.contains(&kind).then_some((kind, pin.target.as_str()))
    })
}

/// Every pinned target of one category, in the file's own order.
pub fn pinned_targets(table: &PinsV1, kind: PinKind) -> impl Iterator<Item = &str> {
    table
        .pins
        .iter()
        .filter(move |pin| pin.kind() == Some(kind))
        .map(|pin| pin.target.as_str())
}

/// Where one target sits under one category, if it sits anywhere.
///
/// The category is part of the question because the table is one array: a folder
/// and a file may in principle be written with the same string, and "is this
/// pinned" asked by the root menu must not be answered by a row the preview
/// switcher put there.
fn row_of(table: &PinsV1, kind: PinKind, target: &str) -> Option<usize> {
    table
        .pins
        .iter()
        .position(|pin| pin.kind() == Some(kind) && pin.target == target)
}

/// `pins.json` — the table, and when it reaches the disk.
///
/// [`crate::persist::ProfilesStore`]'s shape, deliberately and to the letter:
/// three files with the same job — hold a list a person may also edit by hand —
/// must not have three different stores behind them, or "when does the window
/// notice" becomes a question about which file you edited.
pub struct PinsStore {
    /// The table, what the file holds — what a re-read compares with — and the
    /// requests on the worker.
    document: DocumentState<PinsV1>,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    /// **What a quit merged and did not put in force**
    /// ([`crate::persist::ProfilesStore`]'s rule): the next storage pass
    /// rebuilds the menus from it and says it, should the quit be cancelled.
    merged_at_quit: Option<crate::settings_merge::Account>,
}

/// `pins.json` in a quit and on a stop: its [`DocumentState`]'s answers (T-QUIT-DOCUMENTS).
impl QuitDocument for PinsStore {
    fn file(&self) -> &'static str {
        self.document.file()
    }

    fn quit_begin(&mut self) {
        self.document.quit_begin();
    }

    /// A turn that folds a changed file in before it sends: a `Moved` or
    /// `Displaced` answer at quit is merged as the running window merges it,
    /// without the door.
    fn quit_turn(&mut self) {
        let _ = self.document.take_answers();
        self.merge_at_quit();
        if self.document.standing() == crate::quit::Standing::Unsent {
            self.document.send_owed();
        }
    }

    fn standing(&self) -> crate::quit::Standing {
        self.document.standing()
    }

    fn keep_copy(&mut self, folder: &std::path::Path, at: std::time::SystemTime) {
        self.document.keep_copy(folder, at);
    }

    fn copy_standing(&mut self) -> crate::quit::CopyStanding {
        self.document.copy_standing()
    }

    fn drop_owed(&mut self) -> Option<String> {
        self.document.drop_owed()
    }

    fn owed_bytes(&self) -> Option<Result<Vec<u8>, String>> {
        self.document.owed_bytes()
    }
}

impl PinsStore {
    /// Read `pins.json`, falling back to *an empty table* on every failure. Its
    /// writes and re-reads go to the storage worker on `lane`.
    pub(crate) fn open(lane: StorageLane) -> Self {
        let dir = crate::persist::storage_dir();
        crate::persist::make_data_folder(&dir);
        Self::at_on(dir.join(PINS_FILE_NAME), lane)
    }

    /// The same store over a named file, which is what makes every rule below
    /// testable without a `%APPDATA%`.
    pub(crate) fn at_on(path: PathBuf, lane: StorageLane) -> Self {
        let (file, report, bytes) = read_pins_keeping_bytes(&path, crate::update_trial::keeping());
        crate::update_trial::owe_copy(&report, &path, keep_pins);
        // §5.4 case 1 — no file — is the ordinary state of a machine where
        // nobody has pinned anything, and must not alert. Everything else must,
        // naming the file (§5.3).
        let fault =
            crate::persist::read_fault(&report, PINS_FILE_NAME, crate::i18n::pins_file_unreadable);
        // **The directory this file is in, and no other** — see
        // `crate::persist::is_writer_of`. Asking `is_storage_writer` here made
        // the answer a fact about `%APPDATA%\Folio` whatever path this store was
        // handed, which is a question about a directory a store opened anywhere
        // else never touches.
        let writer_of_record = crate::persist::is_writer_of_document(&path);
        Self {
            document: DocumentState::opened(
                PINS_FILE_NAME,
                path,
                Revision::opened(file, bytes),
                writer_of_record,
                crate::update_trial::Writer::Pins,
                lane,
            ),
            fault,
            merged_at_quit: None,
        }
    }

    /// [`Self::at_on`], with a worker of its own.
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self::at_on(path, StorageLane::of_its_own())
    }

    /// What `pins.json` is known to hold.
    #[cfg(test)]
    pub(crate) fn revision(&self) -> &Revision<PinsV1> {
        self.document.revision()
    }

    /// The document half, for a test to look into.
    #[cfg(test)]
    pub(crate) fn document(&mut self) -> &mut DocumentState<PinsV1> {
        &mut self.document
    }

    /// Whether a file's bytes wait for the merge.
    #[cfg(test)]
    pub(crate) fn merge_waits(&self) -> bool {
        self.document.merge_waits()
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame. A write that has stopped landing comes through the same door.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take().or_else(|| self.document.take_fault())
    }

    /// Every pinned target of one category, in the file's own order.
    pub fn targets(&self, kind: PinKind) -> Vec<String> {
        pinned_targets(self.document.projection(), kind)
            .map(str::to_owned)
            .collect()
    }

    /// Every pinned row of any of these categories — see [`pinned_rows`].
    pub fn rows_of(&self, kinds: &[PinKind]) -> Vec<(PinKind, String)> {
        pinned_rows(self.document.projection(), kinds)
            .map(|(kind, target)| (kind, target.to_owned()))
            .collect()
    }

    /// How many writes of this document the reader has asked for
    /// (`storage_watch::StorageWatch::advance`).
    pub(crate) fn write_generation(&self) -> u64 {
        self.document.write_generation()
    }

    /// Pin or unpin one target and hand the table to the storage worker.
    /// Answers whether it is pinned now, which is what the row that was just
    /// clicked has to draw.
    pub fn toggle(&mut self, kind: PinKind, target: &str) -> bool {
        let mut next = self.document.projection().clone();
        let pinned = toggle_pin(&mut next, kind, target);
        self.store(next);
        pinned
    }

    /// **Ask for the file to be read again, because the storage watch heard
    /// it.**
    ///
    /// [`crate::persist::ProfilesStore::ask_reread`]'s three answers and its
    /// whole argument, one file over. The file's bytes are compared with the
    /// revision's, never the table with the one in memory, so a pin this store
    /// holds but has not written is not undone by a read of the older file. A
    /// document that will not parse is **not** taken, and the table already in
    /// force stays in force: emptying the PINNED sections *because* the reader
    /// typed a comma wrong is the one outcome a hand-editable file must not have.
    pub(crate) fn ask_reread(&mut self) {
        self.document.ask_read();
    }

    /// Whether a re-read of `pins.json` is still to be answered.
    pub(crate) fn reading(&self) -> bool {
        self.document.reading()
    }

    /// Record the table as it stands now and hand it to the storage worker.
    ///
    /// Returns whether anything changed, so a press that moved nothing costs no
    /// write.
    pub fn store(&mut self, file: PinsV1) -> bool {
        self.document.store(file, false)
    }

    /// **A storage pass's merge, the store's half** (design note 2026-10-11
    /// §3.4, (b)8, (c)1): every file stored for the merge is merged by
    /// identity and by key ([`crate::pins_merge::merge`]) and folded into
    /// `desired`, which moves the revision to the file the merge read; the
    /// merged table is then what the menus read, built from it whenever one
    /// opens; the turn presents once after every pass. `None` when nothing was merged.
    pub(crate) fn begin_merge(&mut self) -> Option<crate::table_merge::MergePass> {
        let at_quit = self.merged_at_quit.take();
        if !self.document.merge_waits() && at_quit.is_none() {
            return None;
        }
        let merges = self.document.merge_stored(merge_pins);
        if merges.is_empty() && at_quit.is_none() {
            return None;
        }
        let mut account = at_quit.unwrap_or_default();
        for merge in merges {
            account.add(merge);
        }
        self.document.project(self.document.desired().clone());
        Some(crate::table_merge::MergePass::new(PINS_FILE_NAME, account))
    }

    /// **A quit's merge**: [`Self::begin_merge`]'s fold, and nothing put in
    /// force — the windows are leaving. What it merged is kept for the next
    /// storage pass, which puts it in force should the quit be cancelled.
    fn merge_at_quit(&mut self) {
        if !self.document.merge_waits() {
            return;
        }
        let merges = self.document.merge_stored(merge_pins);
        if merges.is_empty() {
            return;
        }
        let account = self.merged_at_quit.get_or_insert_with(Default::default);
        for merge in merges {
            account.add(merge);
        }
    }

    /// **A storage pass's merge, its last step**: the merged table is handed to
    /// the worker, conditional on the file the merge read, if it differs from
    /// it — the pins that kept Folio's state.
    pub(crate) fn end_merge(&mut self) {
        self.document.send_owed();
    }

    /// **An update's trial was committed: the table as it stands is handed
    /// over** (`update_trial`).
    pub fn release_trial(&mut self) {
        self.document.send_owed();
    }

    /// **Take what the storage worker has answered** (`AppEvent::DocumentsAnswered`):
    /// the re-reads' news for the reader.
    pub(crate) fn take_answers(&mut self) -> ReadNews {
        self.document.take_answers()
    }

    /// Let the worker answer everything this store asked, and take it.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) -> ReadNews {
        self.document.settle()
    }

    /// **A storage pass, in a test**: the file read again on the worker, the
    /// answer taken, and the store's merge run with the send after it (the
    /// door is the window's). `Changed` when the pass moved what the readers
    /// read.
    #[cfg(test)]
    pub(crate) fn reread(&mut self) -> crate::document_request::Heard {
        self.ask_reread();
        let news = self.settle();
        let before = self.document.projection().clone();
        if self.begin_merge().is_some() {
            self.end_merge();
        }
        match news.heard() {
            crate::document_request::Heard::Unchanged if *self.document.projection() != before => {
                crate::document_request::Heard::Changed
            }
            heard => heard,
        }
    }
}

/// **`pins.json`'s merge, as the store's seam takes it.**
fn merge_pins(
    base: &PinsV1,
    ours: &PinsV1,
    theirs: &PinsV1,
) -> (PinsV1, crate::settings_merge::Account) {
    let merged = crate::pins_merge::merge(base, ours, theirs);
    (merged.document, merged.account)
}

/// `pins.json` read again with a refused file's copy kept — what an update's
/// trial owed it (`update_trial::owe_copy`).
fn keep_pins(path: &Path) {
    let _ = read_pins(path);
}

/// **One list with the pinned rows lifted to the top, and nothing said twice.**
///
/// The rule both PINNED sections obey (§0: "已钉 URL 再次出现提升同一条目,PINNED
/// 与 MRU 不留双副本"), written once because it is one rule. `recent` is whatever
/// the surface already had — the shells' folders for the root menu, the open
/// buffers for the preview switcher — and `pinned` is the file's own order.
///
/// Two answers rather than one concatenated list, because the caller has to draw
/// a section label and a hairline between them and would otherwise have to
/// re-derive the boundary it was just told.
///
/// **Generic in the identity** since W2 slice ③: the preview switcher's rows are
/// files *and* pages in one list, and a `pins.json` row is identified by its
/// category and its target together ([`row_of`]) — so a lift that compared bare
/// strings would let a pinned file and a pinned page of the same spelling stand
/// for one another. The root menu still hands `String`s, which is the same
/// function with the pair collapsed to the half it has.
#[must_use]
pub fn lift_pinned<K: Clone + PartialEq, T: Clone>(
    pinned: &[K],
    recent: &[T],
    identity: impl Fn(&T) -> K,
) -> (Vec<K>, Vec<T>) {
    let kept: Vec<K> = pinned.to_vec();
    let rest = recent
        .iter()
        .filter(|item| !kept.contains(&identity(item)))
        .cloned()
        .collect();
    (kept, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_request::Heard;

    /// Whether one target is on the table under one category — the question the
    /// suite asks in the shape the store answers it, without a filesystem.
    fn is_pinned(table: &PinsV1, kind: PinKind, target: &str) -> bool {
        row_of(table, kind, target).is_some()
    }

    fn table(rows: &[(&str, &str)]) -> PinsV1 {
        PinsV1 {
            pins: rows
                .iter()
                .map(|(kind, target)| PinEntryV1 {
                    kind: (*kind).to_owned(),
                    target: (*target).to_owned(),
                    extra: serde_json::Map::new(),
                })
                .collect(),
            ..PinsV1::default()
        }
    }

    /// PIN — pinning and unpinning is a round trip, and it is the *target* that
    /// is the identity.
    #[test]
    fn a_pin_and_an_unpin_leave_the_table_where_they_found_it() {
        let mut table = PinsV1::default();
        assert!(!is_pinned(&table, PinKind::Folder, r"C:\work"));
        assert!(toggle_pin(&mut table, PinKind::Folder, r"C:\work"));
        assert!(is_pinned(&table, PinKind::Folder, r"C:\work"));
        assert_eq!(table.pins.len(), 1);
        // A second press takes it off, and the table is the one we started with.
        assert!(!toggle_pin(&mut table, PinKind::Folder, r"C:\work"));
        assert!(!is_pinned(&table, PinKind::Folder, r"C:\work"));
        assert_eq!(table, PinsV1::default());

        // The category is part of the question: a folder pin does not answer for
        // a file that happens to be written the same way.
        let mut table = PinsV1::default();
        toggle_pin(&mut table, PinKind::Folder, r"C:\work");
        assert!(!is_pinned(&table, PinKind::File, r"C:\work"));
        assert!(toggle_pin(&mut table, PinKind::File, r"C:\work"));
        assert_eq!(table.pins.len(), 2);

        // The identity is the string as it was given. A path spelled another way
        // is another row, exactly as `root_choices` already treats it.
        let mut table = PinsV1::default();
        toggle_pin(&mut table, PinKind::Folder, r"C:\work");
        assert!(!is_pinned(&table, PinKind::Folder, r"C:\Work"));
    }

    /// PIN — a new pin goes to the end, so the rows already drawn do not move
    /// under the hand that is still on the menu.
    #[test]
    fn a_new_pin_is_appended_and_the_rows_above_it_stay_put() {
        let mut table = PinsV1::default();
        for folder in [r"C:\a", r"C:\b", r"C:\c"] {
            toggle_pin(&mut table, PinKind::Folder, folder);
        }
        assert_eq!(
            pinned_targets(&table, PinKind::Folder).collect::<Vec<_>>(),
            vec![r"C:\a", r"C:\b", r"C:\c"]
        );
        // Removing the middle one does not disturb its neighbours' order.
        toggle_pin(&mut table, PinKind::Folder, r"C:\b");
        assert_eq!(
            pinned_targets(&table, PinKind::Folder).collect::<Vec<_>>(),
            vec![r"C:\a", r"C:\c"]
        );
    }

    /// PIN — a row this build has no name for is carried through every edit
    /// untouched, and is offered nowhere.
    #[test]
    fn an_unknown_category_survives_a_pin_and_an_unpin() {
        let mut table = table(&[
            ("workspace", "team"),
            ("folder", r"C:\work"),
            ("url", "https://example.com/"),
        ]);
        let before = table.clone();
        toggle_pin(&mut table, PinKind::Folder, r"C:\other");
        toggle_pin(&mut table, PinKind::Folder, r"C:\other");
        assert_eq!(table, before, "an unrelated edit changes nothing else");

        // It is not a folder, not a file, and not a URL — so no section asks for
        // it, and none of them can be handed it by accident.
        for kind in [PinKind::Folder, PinKind::File, PinKind::Url] {
            assert!(!pinned_targets(&table, kind).any(|target| target == "team"));
        }
        // The URL row parses and is offered to whoever asks for URLs — which in
        // this build is nobody, and that is the point: it survives.
        assert_eq!(
            pinned_targets(&table, PinKind::Url).collect::<Vec<_>>(),
            vec!["https://example.com/"]
        );
    }

    /// PIN — the store writes atomically, reads what it wrote, and a damaged
    /// file leaves the table that was already in force alone.
    #[test]
    fn the_store_round_trips_and_a_damaged_file_keeps_the_table_in_force() {
        let dir = bt_testpath::temp_path("folio-pins");
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let path = dir.join("pins.json");
        let _ = std::fs::remove_file(&path);

        // A machine where nobody has pinned anything has no file, and that is
        // not a fault.
        let mut store = PinsStore::at(path.clone());
        assert_eq!(store.take_fault(), None);
        assert!(store.targets(PinKind::Folder).is_empty());
        assert!(!path.exists(), "and nothing is written to announce that");

        assert!(store.toggle(PinKind::Folder, r"C:\work"));
        store.settle();
        assert!(path.exists());
        assert!(
            !dir.read_dir()
                .expect("the folder reads")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().contains(".tmp-")),
            "the temp file the atomic write used is renamed away, never left behind"
        );

        // A second store over the same path reads it back.
        let mut reopened = PinsStore::at(path.clone());
        assert_eq!(reopened.take_fault(), None);
        assert_eq!(
            reopened.targets(PinKind::Folder),
            vec![r"C:\work".to_owned()]
        );

        // The watcher's three answers: this window's own write is `Unchanged`,
        // somebody else's edit is `Changed`, and a broken file is `Unreadable`
        // with the table that was in force still in force.
        assert_eq!(reopened.reread(), Heard::Unchanged);
        let mut other = PinsStore::at(path.clone());
        other.toggle(PinKind::Folder, r"D:\repos");
        other.settle();
        assert_eq!(reopened.reread(), Heard::Changed);
        assert_eq!(
            reopened.targets(PinKind::Folder),
            vec![r"C:\work".to_owned(), r"D:\repos".to_owned()]
        );

        std::fs::write(&path, b"{ not json").expect("the file is writable");
        assert_eq!(reopened.reread(), Heard::Unreadable);
        assert_eq!(
            reopened.targets(PinKind::Folder),
            vec![r"C:\work".to_owned(), r"D:\repos".to_owned()],
            "a comma typed wrong does not empty the reader's PINNED section"
        );

        // At startup there is nothing to keep, so the same damaged file is a
        // fault and an empty table — the one place this differs from a re-read.
        let mut cold = PinsStore::at(path.clone());
        assert!(cold.take_fault().is_some());
        assert!(cold.targets(PinKind::Folder).is_empty());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// PIN — a pinned row appears once: in PINNED, and not again in the list
    /// below it.
    #[test]
    fn what_is_pinned_is_not_also_in_the_list_below() {
        let pinned = vec![r"C:\work".to_owned(), r"D:\repos".to_owned()];
        let recent = vec![
            r"C:\Users\dev".to_owned(),
            r"C:\work".to_owned(),
            r"E:\tmp".to_owned(),
        ];
        let (top, rest) = lift_pinned(&pinned, &recent, Clone::clone);
        assert_eq!(top, pinned, "PINNED is the file's own order, whole");
        assert_eq!(
            rest,
            vec![r"C:\Users\dev".to_owned(), r"E:\tmp".to_owned()],
            "and the row it lifted is gone from below rather than drawn twice"
        );

        // Nothing pinned is the list exactly as it was.
        let (top, rest) = lift_pinned(&[], &recent, Clone::clone);
        assert!(top.is_empty());
        assert_eq!(rest, recent);

        // A pin whose target is not in the list at all is still offered: that is
        // what pinning it was for.
        let (top, rest) = lift_pinned(&[r"Z:\archive".to_owned()], &recent, Clone::clone);
        assert_eq!(top, vec![r"Z:\archive".to_owned()]);
        assert_eq!(rest, recent);

        // **And that is what the switcher's chevron is gated on** (found on a
        // real window, 2026-08-19): one buffer open and one kept file that is
        // not it are *two* rows, so a pane showing a lone file still has a way
        // into the list. Gating the chevron on the pool instead — one buffer,
        // no chevron — is a list with no door, which is what the first frame
        // after a restart looked like.
        let one_open = vec![r"C:\work\open.md".to_owned()];
        let (top, rest) = lift_pinned(&[r"C:\work\kept.md".to_owned()], &one_open, Clone::clone);
        assert_eq!(
            top.len() + rest.len(),
            2,
            "the chevron follows the list, and the list is longer than the pool"
        );
    }

    /// RED — **the writer-of-record question is about the directory this store
    /// writes to, and no other.**
    ///
    /// `at` used to answer it with `is_storage_writer()`, which claims
    /// `%APPDATA%\Folio` — a directory a store opened over a scratch folder
    /// never touches. That made the answer a fact about the machine rather than
    /// about the file: with the reader's own Folio running, a store over a
    /// folder nobody holds refused to write; with no Folio running, a store over
    /// a folder somebody else holds wrote into it.
    ///
    /// The claim on `held` here is taken the way a second Folio takes it, from
    /// the same primitive, so nothing about this test is a mock.
    ///
    /// Red gate: ask `is_storage_writer()` in `at` and one of the two halves
    /// below fails on every machine — the first when a Folio is running, the
    /// second when none is.
    #[test]
    fn a_store_writes_the_folder_it_holds_and_not_the_one_somebody_else_does() {
        let root = bt_testpath::temp_path("folio-pins-claim");
        let held = root.join("held");
        let free = root.join("free");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&held).expect("a scratch folder");
        std::fs::create_dir_all(&free).expect("a scratch folder");

        // Somebody else is the writer of record for `held` — here this test
        // itself, which is exactly what the second Folio is to the first.
        let claim = bt_platform::instance::claim_data_directory(&held)
            .expect("nothing else on this machine has this folder");

        let mut free_store = PinsStore::at(free.join(PINS_FILE_NAME));
        assert!(free_store.toggle(PinKind::Folder, r"C:\work"));
        free_store.settle();
        assert!(
            free.join(PINS_FILE_NAME).exists(),
            "a store over a folder nobody holds writes it, whatever else this machine is running"
        );

        let mut held_store = PinsStore::at(held.join(PINS_FILE_NAME));
        assert!(
            held_store.toggle(PinKind::Folder, r"C:\work"),
            "the table is live in this window either way"
        );
        held_store.settle();
        assert!(
            !held.join(PINS_FILE_NAME).exists(),
            "and a store over a folder somebody else holds writes nothing into it"
        );

        drop(claim);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A pin store over a scratch `pins.json` holding `file`, on a lane nobody
    /// serves: what it would write stays queued, and a test hands it the reads.
    fn merging_store(
        case: &str,
        file: &PinsV1,
    ) -> (std::path::PathBuf, PinsStore, crate::persist::UnservedJobs) {
        let root = bt_testpath::temp_path(&format!("bt-app-pins-merge-{case}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a private storage folder for this test");
        let path = root.join(PINS_FILE_NAME);
        std::fs::write(
            &path,
            bt_persist::serialize_pins(file).expect("it serializes"),
        )
        .expect("the pins file");
        let (lane, jobs) = crate::persist::StorageLane::unserved();
        let store = PinsStore::at_on(path, lane);
        (root, store, jobs)
    }

    /// The file changed to `theirs`, heard by the store and merged: the pass's
    /// store half.
    fn merge_in(store: &mut PinsStore, theirs: &PinsV1) -> crate::table_merge::MergePass {
        let bytes: std::sync::Arc<[u8]> = bt_persist::serialize_pins(theirs)
            .expect("it serializes")
            .into();
        let _ = store
            .document()
            .hear_read(bt_persist::StableRead::Bytes(bytes));
        store.begin_merge().expect("the file waits for the merge")
    }

    fn with_extra(mut pin: PinEntryV1, key: &str, value: serde_json::Value) -> PinEntryV1 {
        pin.extra.insert(key.to_owned(), value);
        pin
    }

    /// RED ((b)14-12, (b)8) — **the document's top-level keys of its own merge
    /// key by key**: a key only Folio's document changed is Folio's, a key only
    /// the file changed or added is the file's, and a key both changed to two
    /// values is the file's, named.
    ///
    /// MUTATION: take `O`'s top-level `extra` (in `pins_merge::merge`, the
    /// document's `extra` is `ours.extra`) — the file's `b` and `c` are lost.
    #[test]
    fn top_level_extra_merges_by_key() {
        let mut base = table(&[("folder", r"C:\甲")]);
        for (key, value) in [
            ("主题", serde_json::json!("暗 dark")),
            ("a", serde_json::json!(1)),
            ("b", serde_json::json!(1)),
        ] {
            base.extra.insert(key.to_owned(), value);
        }
        let (root, mut store, _jobs) = merging_store("top-level", &base);
        let mut ours = base.clone();
        ours.extra.insert("a".to_owned(), serde_json::json!(2));
        ours.extra
            .insert("主题".to_owned(), serde_json::json!("亮 light"));
        assert!(store.store(ours));
        let mut theirs = base.clone();
        theirs.extra.insert("b".to_owned(), serde_json::json!(3));
        theirs
            .extra
            .insert("主题".to_owned(), serde_json::json!("蓝 blue"));
        theirs
            .extra
            .insert("c".to_owned(), serde_json::json!(["新", "new"]));

        let pass = merge_in(&mut store, &theirs);
        let merged = &store.document().desired().extra;
        assert_eq!(merged.get("a"), Some(&serde_json::json!(2)), "Folio's");
        assert_eq!(merged.get("b"), Some(&serde_json::json!(3)), "the file's");
        assert_eq!(merged.get("c"), Some(&serde_json::json!(["新", "new"])));
        assert_eq!(
            merged.get("主题"),
            Some(&serde_json::json!("蓝 blue")),
            "changed on both: the file's"
        );
        assert_eq!(
            pass.replaced().map(|replaced| replaced.values),
            Some(vec!["主题".to_owned()])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)14-12, (b)8) — **remove against change goes to the file**: Folio
    /// unpinned a folder whose fields the file changed — it stays, as the file
    /// has it; the file unpinned a file whose fields Folio changed — it goes.
    /// Both are named.
    ///
    /// MUTATION: removal always wins (in `pins_merge::resolve`, the
    /// `(Some, None, Some)` arm answers `None`) — the folder is gone.
    #[test]
    fn remove_against_change_goes_to_the_file() {
        let folder = with_extra(
            PinEntryV1::new(PinKind::Folder, r"C:\甲"),
            "颜色",
            serde_json::json!("红 red"),
        );
        let file = with_extra(
            PinEntryV1::new(PinKind::File, r"D:\乙 notes.md"),
            "行",
            serde_json::json!(1),
        );
        let base = PinsV1 {
            pins: vec![folder.clone(), file.clone()],
            ..PinsV1::default()
        };
        let (root, mut store, _jobs) = merging_store("remove-change", &base);
        assert!(
            !store.toggle(PinKind::Folder, r"C:\甲"),
            "Folio unpinned it"
        );
        let mut ours = store.document().projection().clone();
        ours.pins[0] = with_extra(file.clone(), "行", serde_json::json!(42));
        assert!(store.store(ours));
        let folder_there = with_extra(folder.clone(), "颜色", serde_json::json!("蓝 blue"));
        let theirs = PinsV1 {
            pins: vec![folder_there.clone()],
            ..PinsV1::default()
        };

        let pass = merge_in(&mut store, &theirs);
        assert_eq!(
            store.document().desired().pins,
            vec![folder_there],
            "the folder the file changed stays; the file the file unpinned goes"
        );
        assert_eq!(
            pass.replaced().map(|replaced| replaced.values),
            Some(vec![r"C:\甲".to_owned(), r"D:\乙 notes.md".to_owned()])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED ((b)14-12, (b)8's one rule) — **a later row of a pin the file already
    /// holds is carried**, verbatim and in its place, and Folio's addition
    /// follows the file's rows.
    ///
    /// MUTATION: collapse duplicates (in `pins_merge::merge`, skip a `T` row
    /// whose identity was seen instead of carrying it) — the second `C:\甲` is
    /// gone.
    #[test]
    fn a_later_duplicate_pin_is_carried() {
        let base = table(&[("folder", r"C:\甲"), ("file", r"D:\乙.md")]);
        let (root, mut store, _jobs) = merging_store("duplicate", &base);
        assert!(store.toggle(PinKind::Folder, r"E:\我的"));
        let second = with_extra(
            PinEntryV1::new(PinKind::Folder, r"C:\甲"),
            "注",
            serde_json::json!("第二 second"),
        );
        let mut theirs = base.clone();
        theirs.pins.push(second.clone());

        let _ = merge_in(&mut store, &theirs);
        let mut expected = theirs.pins.clone();
        expected.push(PinEntryV1::new(PinKind::Folder, r"E:\我的"));
        assert_eq!(store.document().desired().pins, expected);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// RED (review round 3, coordinator's ruling) — **an unpin takes every row
    /// of its target and tag**: the file pins a folder twice; the menus read
    /// the first; unpinning it leaves neither, in the table the press makes and
    /// in the document Folio writes — the later row never becomes the pin.
    ///
    /// MUTATION: keep the duplicate (`document_request::First::alongside`
    /// answers nothing — the press road before round 3) — the later row is
    /// pinned after the unpin.
    #[test]
    fn an_unpin_takes_the_later_duplicates_of_its_pin() {
        let file = table(&[
            ("folder", "项目 project"),
            ("file", "笔记 notes.md"),
            ("folder", "项目 project"),
        ]);
        let mut pressed = file.clone();
        assert!(!toggle_pin(&mut pressed, PinKind::Folder, "项目 project"));
        assert!(!is_pinned(&pressed, PinKind::Folder, "项目 project"));

        let (root, mut store, _jobs) = merging_store("unpin-duplicate", &file);
        assert!(!store.toggle(PinKind::Folder, "项目 project"));
        assert_eq!(
            store.document().desired().pins,
            vec![PinEntryV1::new(PinKind::File, "笔记 notes.md")],
            "the document Folio writes holds neither row of the folder"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
