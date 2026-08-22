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

use std::path::PathBuf;

use bt_persist::{
    PinEntryV1, PinKind, PinsV1, ReadReport, WriteAlertAction, WriteFailureTracker, read_pins,
    write_pins_atomic,
};

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
/// touched: this walks for one target under one tag and edits one row.
pub fn toggle_pin(table: &mut PinsV1, kind: PinKind, target: &str) -> bool {
    match row_of(table, kind, target) {
        Some(index) => {
            table.pins.remove(index);
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
    path: PathBuf,
    loaded: PinsV1,
    /// Why the file on disk was not usable, if it was not.
    fault: Option<String>,
    failures: WriteFailureTracker,
}

/// What a re-read of `pins.json` found — [`PinsStore::reread`]'s answer.
///
/// [`crate::persist::ProfilesNews`]'s three outcomes, for its reason: a document
/// that will not parse is neither "nothing happened" nor "here is the new table".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinsNews {
    /// The file says exactly what the table already in force says — the ordinary
    /// answer, including for this window's own writes.
    Unchanged,
    /// The file has been read and is now the table in force.
    Changed,
    /// The file would not parse, so nothing was taken from it and the last table
    /// that did parse is still in force.
    Unreadable,
}

impl PinsStore {
    /// Read `pins.json`, falling back to *an empty table* on every failure.
    pub fn open() -> Self {
        let dir = crate::persist::storage_dir();
        let _ = std::fs::create_dir_all(&dir);
        Self::at(dir.join(PINS_FILE_NAME))
    }

    /// The same store over a named file, which is what makes every rule below
    /// testable without a `%APPDATA%`.
    fn at(path: PathBuf) -> Self {
        let (file, report) = read_pins(&path);
        // §5.4 case 1 — no file — is the ordinary state of a machine where
        // nobody has pinned anything, and must not alert. Everything else must,
        // naming the file (§5.3).
        let fault = match &report {
            ReadReport::FellBackToDefaults { reason } => {
                eprintln!("BT_PERSIST {PINS_FILE_NAME} fell back to defaults: {reason:?}");
                Some(crate::i18n::pins_file_unreadable(PINS_FILE_NAME))
            }
            ReadReport::NotFound | ReadReport::Loaded => None,
        };
        Self {
            path,
            loaded: file,
            fault,
            failures: WriteFailureTracker::new(),
        }
    }

    /// Take the read fault, so a notice about it is raised once and not once a
    /// frame.
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
    }

    /// Every pinned target of one category, in the file's own order.
    pub fn targets(&self, kind: PinKind) -> Vec<String> {
        pinned_targets(&self.loaded, kind)
            .map(str::to_owned)
            .collect()
    }

    /// Every pinned row of any of these categories — see [`pinned_rows`].
    pub fn rows_of(&self, kinds: &[PinKind]) -> Vec<(PinKind, String)> {
        pinned_rows(&self.loaded, kinds)
            .map(|(kind, target)| (kind, target.to_owned()))
            .collect()
    }

    /// Pin or unpin one target and put the table on disk. Answers whether it is
    /// pinned now, which is what the row that was just clicked has to draw.
    pub fn toggle(&mut self, kind: PinKind, target: &str) -> bool {
        let mut next = self.loaded.clone();
        let pinned = toggle_pin(&mut next, kind, target);
        self.store(next);
        pinned
    }

    /// **Read the file again, because the storage folder moved.**
    ///
    /// [`crate::persist::ProfilesStore::reread`]'s three answers and its whole
    /// argument, one file over — including the third and least obvious one: a
    /// document that will not parse is **not** taken, and the table already in
    /// force stays in force. Emptying the PINNED sections *because* the reader
    /// typed a comma wrong is the one outcome a hand-editable file must not have.
    pub fn reread(&mut self) -> PinsNews {
        let (file, report) = read_pins(&self.path);
        if let ReadReport::FellBackToDefaults { reason } = &report {
            eprintln!("BT_PERSIST {PINS_FILE_NAME} would not parse: {reason:?}");
            return PinsNews::Unreadable;
        }
        if self.loaded == file {
            return PinsNews::Unchanged;
        }
        self.loaded = file;
        PinsNews::Changed
    }

    /// Record the table as it stands now and put it on disk.
    ///
    /// Returns whether anything changed, so a press that moved nothing costs no
    /// write.
    pub fn store(&mut self, file: PinsV1) -> bool {
        if self.loaded == file {
            return false;
        }
        self.loaded = file;
        let result = write_pins_atomic(&self.path, &self.loaded);
        if self.failures.record(result.is_ok()) == WriteAlertAction::AlertOnce
            && let Err(error) = &result
        {
            // §5.3: one alert per failure streak, not one per attempt.
            eprintln!("BT_PERSIST could not write {PINS_FILE_NAME}: {error}");
        }
        true
    }
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
        let dir = std::env::temp_dir().join(format!(
            "folio-pins-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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
        assert_eq!(reopened.reread(), PinsNews::Unchanged);
        let mut other = PinsStore::at(path.clone());
        other.toggle(PinKind::Folder, r"D:\repos");
        assert_eq!(reopened.reread(), PinsNews::Changed);
        assert_eq!(
            reopened.targets(PinKind::Folder),
            vec![r"C:\work".to_owned(), r"D:\repos".to_owned()]
        );

        std::fs::write(&path, b"{ not json").expect("the file is writable");
        assert_eq!(reopened.reread(), PinsNews::Unreadable);
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
}
