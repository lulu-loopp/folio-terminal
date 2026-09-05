//! Where a files column has been — `docs/DESIGN.md` §7.5, user ruling 2026-09-05.
//!
//! The root menu's third section. `home`, the folders this window's shells are standing in and the
//! folder above the root are all facts the window can derive by looking around it *now*; this is
//! the one section that is a memory, and a memory needs somewhere to live.
//!
//! **What counts as opening a folder is a gesture and not a state.** A reader who picks a row,
//! chooses one through `Browse…`, drops one on the column, walks into one, or names one to the
//! program on the command line has said "show me this"; a shell that runs `cd` has said nothing to
//! the files column at all. Recording the second would fill five slots with wherever a build script
//! last stepped, which is why the door into this store is [`RecentFolders::record`] and why the
//! doors that call it are counted one by one at the call site rather than derived from the column's
//! root changing.
//!
//! Kept beside the windows rather than inside one, for the reason `bt_persist::SessionV1`'s own
//! `recent` list gives: every root menu in every window offers the same folders, and a copy per
//! window would be as many answers as there are windows.

use std::path::Path;
use std::time::SystemTime;

use bt_persist::RecentFolderV1;

use crate::seed::{format_iso8601_utc, parse_iso8601_utc};

/// How many folders the list keeps — user ruling 2026-09-05.
///
/// Five, and the cap is applied **here** rather than where the menu is built, so that "the list is
/// the five most recently opened folders" is one sentence stored in one place. A store that kept
/// more and let the menu show five would be a store whose contents nobody could read off the
/// screen, and the first question asked of a list like that is always "why is that one not on it".
pub const RECENT_FOLDER_CAPACITY: usize = 5;

/// One folder, and when it was last opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFolder {
    pub path: String,
    /// Absolute, on `crate::seed::RecentEntry::at`'s own argument: what is stored is the instant,
    /// and any phrase about it is computed when it is drawn.
    pub at: SystemTime,
    /// **Whether the folder was on the disk the last time anybody looked** — user ruling
    /// 2026-09-05: a folder that has gone is still listed, and listed grey.
    ///
    /// Not persisted, and not derived inside this store either: this module knows nothing about a
    /// disk, and a field it filled in itself would be a second answer to a question the caller is
    /// already better placed to ask. See [`Self::on_disk`]'s writer, [`RecentFolders::refresh`],
    /// which is called when the menu opens — once per look, because once per look is how often the
    /// answer is read, and because a menu that asked the disk on every frame would ask a dead
    /// network share sixty times a second.
    ///
    /// **True until asked**, so a row recorded and drawn in the same gesture is not grey for the
    /// one frame before the first refresh: the folder was just opened, and the honest default for
    /// "has anybody looked" is the state that says nothing is wrong.
    pub on_disk: bool,
}

/// The store: newest first, deduplicated on the path, capped at
/// [`RECENT_FOLDER_CAPACITY`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecentFolders {
    entries: Vec<RecentFolder>,
}

impl RecentFolders {
    /// **A folder was opened.** Newest first, one row per folder.
    ///
    /// Opening a folder that is already on the list **moves it to the front and updates its
    /// moment** rather than growing a second row — `crate::seed::SeedVault::record`'s rule, and
    /// the same reason it gives: the list is a list of places, and a place is somewhere you can be
    /// more than once.
    ///
    /// Reports whether anything changed, so the caller knows whether the document on disk is now
    /// out of date. Re-opening the folder at the front still changes something — its moment — and
    /// says so, because "when" is half of what this list is.
    ///
    /// An empty path is not a folder and is not recorded. That is the one thing refused here, and
    /// it is refused for `profiles::root_choices`' reason one layer up: a row naming nowhere is a
    /// row a reader cannot choose between.
    pub fn record(&mut self, path: &str, at: SystemTime) -> bool {
        let path = path.trim();
        if path.is_empty() {
            return false;
        }
        let existing = self.entries.iter().position(|entry| entry.path == path);
        // The disk answer travels with the row rather than being reset: this folder was just
        // opened, so nothing that has been observed about it has been contradicted.
        let on_disk = existing.is_none_or(|index| self.entries[index].on_disk);
        if let Some(index) = existing {
            self.entries.remove(index);
        }
        self.entries.insert(
            0,
            RecentFolder {
                path: path.to_owned(),
                at,
                on_disk,
            },
        );
        self.entries.truncate(RECENT_FOLDER_CAPACITY);
        true
    }

    #[must_use]
    pub fn entries(&self) -> &[RecentFolder] {
        &self.entries
    }

    /// **Ask the disk which of these folders are still there**, once.
    ///
    /// The predicate is a parameter because this module is pure and the disk is not — the same
    /// split `profiles::root_choices` keeps between "which places are worth offering" and "which
    /// shells exist". Reports whether any answer changed, which is what tells the caller whether
    /// the menu it is about to draw is different from the one on the screen.
    ///
    /// **Nothing is dropped.** A folder that has gone stays on the list, greyed, on the user's own
    /// ruling: a row that vanished by itself is a list quietly editing the reader's history, and
    /// the reader is the only one who can tell "I deleted that" from "that drive is not plugged
    /// in".
    pub fn refresh(&mut self, exists: &dyn Fn(&Path) -> bool) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            let on_disk = exists(Path::new(&entry.path));
            changed |= entry.on_disk != on_disk;
            entry.on_disk = on_disk;
        }
        changed
    }

    /// Rebuild from what was on disk, newest-first order preserved.
    ///
    /// A row whose moment cannot be read leaves by the door it came in —
    /// `crate::seed::SeedVault::from_persisted`'s rule, for its reason: a row claiming a time we
    /// invented would sort itself into the list against folders whose times are real.
    #[must_use]
    pub fn from_persisted(rows: &[RecentFolderV1]) -> Self {
        Self {
            entries: rows
                .iter()
                .filter(|row| !row.path.trim().is_empty())
                .filter_map(|row| {
                    Some(RecentFolder {
                        path: row.path.clone(),
                        at: parse_iso8601_utc(&row.opened_at)?,
                        on_disk: true,
                    })
                })
                .take(RECENT_FOLDER_CAPACITY)
                .collect(),
        }
    }

    #[must_use]
    pub fn to_persisted(&self) -> Vec<RecentFolderV1> {
        self.entries
            .iter()
            .map(|entry| RecentFolderV1 {
                path: entry.path.clone(),
                opened_at: format_iso8601_utc(entry.at),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_756_000_000 + seconds)
    }

    fn paths(store: &RecentFolders) -> Vec<&str> {
        store
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect()
    }

    /// RED (user ruling 2026-09-05) — **the list is five long and newest first.**
    ///
    /// MUTATION: raise the cap and the menu grows a section taller than the two above it put
    /// together; push to the back instead of the front and the list becomes a record of the first
    /// five folders ever opened, which is the opposite of what it is called.
    #[test]
    fn the_list_keeps_the_five_most_recently_opened_folders() {
        let mut store = RecentFolders::default();
        for (index, path) in [r"D:\a", r"D:\b", r"D:\c", r"D:\d", r"D:\e", r"D:\f"]
            .into_iter()
            .enumerate()
        {
            store.record(path, at(index as u64));
        }
        assert_eq!(store.entries().len(), RECENT_FOLDER_CAPACITY);
        assert_eq!(
            paths(&store),
            vec![r"D:\f", r"D:\e", r"D:\d", r"D:\c", r"D:\b"],
            "newest first, and the oldest left when the sixth arrived"
        );
    }

    /// RED (user ruling 2026-09-05) — **opening a folder again moves it and re-dates it, and does
    /// not copy it.**
    ///
    /// MUTATION: insert without removing and the list fills with one folder; remove without
    /// re-dating and the row sorts as though the second visit never happened, so a folder you were
    /// in a minute ago falls off the end under four you last saw yesterday.
    #[test]
    fn opening_a_folder_again_only_moves_it_and_updates_its_moment() {
        let mut store = RecentFolders::default();
        store.record(r"D:\a", at(0));
        store.record(r"D:\b", at(1));
        store.record(r"D:\a", at(2));
        assert_eq!(
            paths(&store),
            vec![r"D:\a", r"D:\b"],
            "one row, at the front"
        );
        assert_eq!(
            store.entries()[0].at,
            at(2),
            "and it carries the moment of the visit that moved it"
        );
    }

    /// RED (user ruling 2026-09-05) — **a folder that is not on the disk stays on the list and is
    /// marked, and nothing is dropped.**
    ///
    /// MUTATION: filter the missing row out in `refresh` and the reader's history edits itself the
    /// moment a drive is unplugged; leave `on_disk` alone and the menu offers a folder that is not
    /// there in the same ink as four that are.
    #[test]
    fn a_folder_that_has_gone_is_marked_and_kept() {
        let mut store = RecentFolders::default();
        store.record(r"D:\here", at(0));
        store.record(r"D:\gone", at(1));
        let changed = store.refresh(&|path| path != Path::new(r"D:\gone"));
        assert!(
            changed,
            "an answer that changed is an answer worth redrawing"
        );
        assert_eq!(
            paths(&store),
            vec![r"D:\gone", r"D:\here"],
            "the missing folder keeps its place in the order"
        );
        assert!(!store.entries()[0].on_disk);
        assert!(store.entries()[1].on_disk);
        assert!(
            !store.refresh(&|path| path != Path::new(r"D:\gone")),
            "and asking the same question twice changes nothing"
        );
    }

    /// RED — **the list survives the file, in order, with its moments.**
    ///
    /// MUTATION: write the rows oldest-first and every restart reverses the reader's history.
    #[test]
    fn the_list_round_trips_through_the_document() {
        let mut store = RecentFolders::default();
        store.record(r"D:\a", at(0));
        store.record(r"D:\b", at(60));
        let reloaded = RecentFolders::from_persisted(&store.to_persisted());
        assert_eq!(paths(&reloaded), vec![r"D:\b", r"D:\a"]);
        assert_eq!(reloaded.entries()[0].at, at(60));
        assert!(
            reloaded.entries().iter().all(|entry| entry.on_disk),
            "nothing has looked at the disk yet, and the honest default says nothing is wrong"
        );
    }

    /// RED — **a row with no readable moment does not come back.**
    ///
    /// MUTATION: default the unreadable moment to "now" and a folder from a corrupt document sorts
    /// itself above every folder the reader actually opened.
    #[test]
    fn a_row_whose_moment_cannot_be_read_is_not_restored() {
        let store = RecentFolders::from_persisted(&[
            RecentFolderV1 {
                path: r"D:\good".to_owned(),
                opened_at: format_iso8601_utc(at(0)),
            },
            RecentFolderV1 {
                path: r"D:\bad".to_owned(),
                opened_at: "yesterday afternoon".to_owned(),
            },
        ]);
        assert_eq!(paths(&store), vec![r"D:\good"]);
    }
}
