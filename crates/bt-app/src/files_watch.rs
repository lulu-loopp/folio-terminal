//! **The kernel's word that a folder a files column is showing has changed**
//! (user report, 2026-08-27: a program run in this window's own terminal wrote
//! `pi-map-us.html` into the folder the column was rooted at, and the column
//! went on showing the two files it had read minutes earlier).
//!
//! # What was there before this
//!
//! Nothing. `DirWatch` has had three users since it was written — `git_watch`,
//! `scheme_watch`, `storage_watch` — and the files column was not one of them:
//! `docs/UI-UX.md` §11.9 said so in a line under "known not done", and
//! `Runtime::refresh_files_dir` said it again in its own doc, that unfolding a
//! folder *was* the refresh gesture because it was the only one there was. This
//! is the fourth user and it retires both sentences.
//!
//! # Why this is allowed to exist under a rule that forbids polling
//!
//! `git_watch`'s sentence, unchanged: everything here is downstream of a
//! `ReadDirectoryChangesW` completion. A window left open over a folder nobody
//! writes to runs no timer, wakes for nothing and reads no disk. The clock is
//! [`WatchClock`] — the same arithmetic the other three watchers share, and
//! deliberately **not a fourth debounce**, because two copies of a debounce is
//! how two surfaces end up disagreeing about what "it stopped changing" means.
//!
//! # Three rules
//!
//! 1. **The subscription follows what is on the glass.** [`FilesWatch::sync`] is
//!    handed the directories this window's columns are currently showing the
//!    contents of — [`crate::files::visible_dirs`] of every column of the tab on
//!    screen, and of every live float — and owns the whole difference. Folding a
//!    folder, switching tabs, re-rooting a column and closing a float all drop
//!    their handles here, by the set no longer containing them. Nothing else in
//!    this file decides to watch or stop watching anything.
//! 2. **One folder, not one tree.** `bt_platform::DirWatch::start_shallow`: the
//!    directory's own entries and nothing deeper. A column rooted at a
//!    repository must not make this process listen to every object file a
//!    `cargo build` writes into `target\debug` — and it would learn nothing from
//!    them either, because a row that is not on screen has no names to be wrong
//!    about. The folders that *are* on screen are watched one apiece, which is
//!    the same set and none of the cost.
//! 3. **Arming is itself a reason to re-read.** A directory arriving in the set
//!    is a directory this window was not listening to a moment ago, and Windows
//!    keeps no log for a subscriber who was not yet listening. So a column that
//!    already holds an answer for a directory whose watch has just opened is
//!    told to ask again: that answer was taken while nobody was watching. A
//!    directory nothing has ever asked about is not re-asked, because the walk
//!    that put it on screen is about to ask for it for the first time.
//!
//! # What a notification means here
//!
//! Nothing is parsed out of it — `git_watch`'s rule, one level down. A file
//! appearing, a rename, a delete and a write all arrive as *that folder moved*,
//! and what it means is decided on the main thread by re-reading the folder.
//! The comparison that decides whether the re-read was **news** is one level
//! further down again and belongs to the cache, not here: see
//! [`crate::files::DirCache::accept`], which answers `false` for a listing
//! identical to the one it already held. That is what keeps a log file being
//! appended to in a watched folder from repainting the window every floor.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, watch_clock::WatchClock};

/// One watched folder: its subscription, and the clock its notifications feed.
struct Watched {
    /// **`None` is a real state and not a failure to retry.** A folder on a
    /// network share, on a `\\wsl$` mount, or one this process may not open
    /// cannot be watched; the entry is kept anyway, holding no handle, so that
    /// the attempt is made once per time the folder comes on screen rather than
    /// once per turn of the event loop. Retrying on a schedule is the poll this
    /// whole mechanism exists to avoid — and a column over such a folder behaves
    /// exactly as every column behaved before today, which is to say the unfold
    /// is still its refresh.
    watch: Option<bt_platform::DirWatch>,
    clock: WatchClock,
}

/// **Every folder this window's file trees are showing the contents of.**
#[derive(Default)]
pub struct FilesWatch {
    /// Where the watcher threads leave their news: the folder, and when its most
    /// recent notification arrived.
    ///
    /// A map and not a channel for `git_watch`'s reason: the only thing worth
    /// keeping about ten notifications is that there were some and when the last
    /// one was, which is exactly what an insert into a map keyed by folder does,
    /// for free, on the thread that would otherwise have queued ten messages.
    news: Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    dirs: BTreeMap<PathBuf, Watched>,
}

impl FilesWatch {
    /// **Bring the subscriptions level with what the columns are showing** (rule
    /// 1), and say which folders were newly armed (rule 3).
    ///
    /// The arrivals come back rather than being acted on here for the reason
    /// nothing in this file parses a notification: what a column should do about
    /// a folder is the window's business, and this type does not know that a
    /// column exists.
    pub fn sync(
        &mut self,
        wanted: &BTreeSet<PathBuf>,
        proxy: &EventLoopProxy<AppEvent>,
    ) -> Vec<PathBuf> {
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        self.sync_with(wanted, move |directory| subscribe(&news, &proxy, directory))
    }

    /// [`Self::sync`]'s bookkeeping, with the opening of a watch handed in.
    ///
    /// One derivation for the real thing and for the tests, on
    /// `GitWatch::sync_with`'s own reasoning: what the gate *is* — the map
    /// follows the set, departures drop their handles, arrivals are armed once —
    /// is the same code whether a kernel subscription is actually taken out or
    /// not, and a second copy of it in the tests would be a test of the copy.
    fn sync_with(
        &mut self,
        wanted: &BTreeSet<PathBuf>,
        mut open: impl FnMut(&Path) -> Option<bt_platform::DirWatch>,
    ) -> Vec<PathBuf> {
        // **A window with no file tree open touches nothing at all**, not even
        // the mailbox's lock. This is asked on every turn of the event loop,
        // mouse moves included.
        if wanted.is_empty() && self.dirs.is_empty() {
            return Vec::new();
        }
        let held = self.dirs.len();
        // Departures first, and the drop is the cancellation: `DirWatch::drop`
        // sets the stop event and joins its thread.
        self.dirs.retain(|directory, _| wanted.contains(directory));
        let departed = self.dirs.len() != held;
        let mut arrived = Vec::new();
        for directory in wanted {
            if self.dirs.contains_key(directory) {
                continue;
            }
            self.dirs.insert(
                directory.clone(),
                Watched {
                    watch: open(directory),
                    clock: WatchClock::default(),
                },
            );
            arrived.push(directory.clone());
        }
        if departed || !arrived.is_empty() {
            // A folder that stopped being watched left its unread news behind,
            // and acting on that stamp the next time the folder came back would
            // be acting on it *after* the fresh read that rule 3 already owes.
            lock(&self.news).retain(|directory, _| wanted.contains(directory));
            let (held, watching) = self.counts();
            trace(&format!(
                "{watching} of {held} folders on screen are being watched"
            ));
        }
        arrived
    }

    /// **Which watched folders have gone quiet since something moved in them.**
    ///
    /// Folding the news in and answering are one step for `GitWatch::due`'s
    /// reason: a notification that arrived a moment ago may or may not have made
    /// its folder due, and the only way to find out is to give it to the clock
    /// first.
    pub fn due(&mut self, now: Instant) -> Vec<PathBuf> {
        if self.dirs.is_empty() {
            return Vec::new();
        }
        for (directory, at) in std::mem::take(&mut *lock(&self.news)) {
            if let Some(entry) = self.dirs.get_mut(&directory) {
                entry.clock.note_event(at);
            }
        }
        self.dirs
            .iter_mut()
            .filter_map(|(directory, entry)| entry.clock.take_due(now).then(|| directory.clone()))
            .collect()
    }

    /// When the loop must wake to answer news it is already holding.
    ///
    /// `None` while nothing is owed, which is the ordinary state of a window
    /// showing a folder nobody is writing to — and the reason this mechanism
    /// costs no wake-ups at all when nothing is happening.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.dirs
            .values()
            .filter_map(|entry| entry.clock.due_at())
            .min()
    }

    /// How many folders are subscribed to, and how many of them this process
    /// could actually open a watch on. For the tests and the trace line.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (
            self.dirs.len(),
            self.dirs
                .values()
                .filter(|held| held.watch.is_some())
                .count(),
        )
    }
}

/// Open the one watch a folder needs.
///
/// A free function and not a method for `git_watch::subscribe`'s reason: it is
/// called from inside a closure `sync_with` holds while it holds `self` mutably,
/// and what it needs is the mailbox and the proxy rather than the registry.
fn subscribe(
    news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    proxy: &EventLoopProxy<AppEvent>,
    directory: &Path,
) -> Option<bt_platform::DirWatch> {
    let mailbox = Arc::clone(news);
    let proxy = proxy.clone();
    let key = directory.to_path_buf();
    // Shallow (rule 2): this folder's own entries, which is exactly the set of
    // names the rows under it are.
    let started = bt_platform::DirWatch::start_shallow(directory, move || {
        lock(&mailbox).insert(key.clone(), Instant::now());
        // The loop is woken, not told what to do: what a change means is decided
        // on the main thread, where the caches and the columns are.
        let _ = proxy.send_event(AppEvent::FilesDirChanged);
    });
    match started {
        Ok(watch) => Some(watch),
        Err(error) => {
            // **Quietly.** There is nothing here a reader could act on, and the
            // column is not broken — it is the column this product shipped
            // until today, whose refresh gesture is the unfold.
            trace(&format!("cannot watch {}: {error}", directory.display()));
            None
        }
    }
}

/// A mutex this crate never poisons on purpose, unwrapped without a panic path —
/// `git_watch::lock`'s twin and for its reason.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The door for whoever is debugging a column that did not refresh itself.
///
/// `BT_GIT_TRACE` is the watcher family's switch and this is a watcher; a second
/// variable for the same class of question would be one more thing to know
/// before you can ask it. Set-but-empty is off, on `BT_PERF_TRACE`'s own rule.
fn trace(message: &str) {
    if std::env::var_os("BT_GIT_TRACE").is_some_and(|value| !value.is_empty()) {
        eprintln!("files watch: {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::watch_clock::{WATCH_FLOOR, WATCH_QUIET};

    fn dir(name: &str) -> PathBuf {
        PathBuf::from(r"D:\Documents\SyncFolder").join(name)
    }

    /// [`FilesWatch::sync`] with no kernel behind it.
    fn sync_for_test(watch: &mut FilesWatch, wanted: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
        watch.sync_with(wanted, |_| None)
    }

    /// PIN (rule 1) — **the subscriptions are the folders on the glass**, and
    /// folding one is what releases its handle.
    ///
    /// The whole of the gate, in the four moves a column makes: it is rooted, a
    /// folder under it is unfolded, that folder is folded again, and the column
    /// goes away. Nothing else in this type ever decides to hold or drop a
    /// subscription, which is why this is the test that would catch a second
    /// place deciding.
    ///
    /// RED GATE: have `sync_with` skip the `retain` — the folded folder keeps
    /// its handle and its thread for the life of the window, and the last two
    /// assertions go red.
    #[test]
    fn the_subscriptions_are_the_folders_on_screen_and_folding_one_releases_it() {
        let mut watch = FilesWatch::default();
        let root = dir("Application");
        let inside = root.join("assets");

        assert!(sync_for_test(&mut watch, &BTreeSet::new()).is_empty());
        assert_eq!(watch.counts().0, 0, "nothing on screen, nothing watched");

        assert_eq!(
            sync_for_test(&mut watch, &BTreeSet::from([root.clone()])),
            vec![root.clone()],
            "a column rooted somewhere arms that folder"
        );
        assert_eq!(watch.counts().0, 1);
        assert!(
            sync_for_test(&mut watch, &BTreeSet::from([root.clone()])).is_empty(),
            "the same set again arms nothing and re-opens nothing"
        );

        assert_eq!(
            sync_for_test(&mut watch, &BTreeSet::from([root.clone(), inside.clone()])),
            vec![inside.clone()],
            "unfolding a folder arms that folder and only that one"
        );
        assert_eq!(watch.counts().0, 2);

        assert!(
            sync_for_test(&mut watch, &BTreeSet::from([root.clone()])).is_empty(),
            "folding it back arms nothing"
        );
        assert_eq!(watch.counts().0, 1, "and drops the handle it had");

        assert!(sync_for_test(&mut watch, &BTreeSet::new()).is_empty());
        assert_eq!(
            watch.counts().0,
            0,
            "and the column going away drops the rest"
        );
        assert_eq!(watch.deadline(), None, "and nothing is owed by nobody");
    }

    /// PIN (rule 3) — **a folder that has just been armed is a folder whose
    /// answer was taken while nobody was listening.**
    ///
    /// Windows keeps no log for a subscriber who was not yet listening, so the
    /// arrival list is not a diagnostic: it is the caller's instruction to
    /// re-ask. Written as a claim about what `sync` *returns*, because that is
    /// the only channel through which this rule can reach anybody.
    #[test]
    fn arming_a_folder_is_reported_once_and_only_on_the_turn_it_arrives() {
        let mut watch = FilesWatch::default();
        let root = dir("Application");
        let wanted = BTreeSet::from([root.clone()]);

        assert_eq!(sync_for_test(&mut watch, &wanted), vec![root.clone()]);
        assert!(sync_for_test(&mut watch, &wanted).is_empty());
        assert!(sync_for_test(&mut watch, &wanted).is_empty());

        // Away and back — a tab switched off and on again — is a second arming,
        // because in between this window was not listening.
        assert!(sync_for_test(&mut watch, &BTreeSet::new()).is_empty());
        assert_eq!(sync_for_test(&mut watch, &wanted), vec![root]);
    }

    /// PIN — **one write is one re-read, a burst is one, and a folder nobody
    /// touches is none.**
    ///
    /// The user's own case in its first two lines: a program writes one file
    /// into the folder a column is rooted at, and the column re-reads that
    /// folder once, a quiet window later. The third is what makes it affordable
    /// — a build writing into that folder without a three-hundred-millisecond
    /// gap anywhere in it costs one re-read per floor, not one per write.
    ///
    /// RED GATE: drop the `take_due` filter in [`FilesWatch::due`] — every
    /// watched folder answers on every turn of the loop, and the first
    /// assertion goes red before the file has finished being written.
    #[test]
    fn one_new_file_is_one_re_read_a_quiet_window_after_the_last_thing_that_moved() {
        let mut watch = FilesWatch::default();
        let root = dir("Application");
        let elsewhere = dir("Photos");
        sync_for_test(
            &mut watch,
            &BTreeSet::from([root.clone(), elsewhere.clone()]),
        );
        let start = Instant::now();

        // The write, the size change and the attribute touch of one `New-Item`.
        for step in [0, 2, 7] {
            lock(&watch.news).insert(root.clone(), start + Duration::from_millis(step));
            assert!(
                watch.due(start + Duration::from_millis(step)).is_empty(),
                "nothing fires while the folder is still moving"
            );
        }
        assert_eq!(
            watch.deadline(),
            Some(start + Duration::from_millis(7) + WATCH_QUIET)
        );
        assert_eq!(
            watch.due(start + Duration::from_millis(7) + WATCH_QUIET),
            vec![root.clone()],
            "one file, one re-read, and only the folder it landed in"
        );
        assert_eq!(watch.deadline(), None, "and then it is quiet again");

        // A folder nobody has touched owes nothing, however long the window
        // stays open — R31's sentence read for a directory listing.
        assert!(watch.due(start + Duration::from_secs(3600)).is_empty());
        assert_eq!(watch.deadline(), None);

        // And a storm is held to the floor rather than answered at the quiet
        // window, which is what a `cargo build` in the watched folder costs.
        let storm = start + Duration::from_secs(10);
        lock(&watch.news).insert(root.clone(), storm);
        assert_eq!(watch.due(storm + WATCH_QUIET), vec![root.clone()]);
        lock(&watch.news).insert(root.clone(), storm + WATCH_QUIET);
        assert!(
            watch
                .due(storm + WATCH_QUIET + Duration::from_millis(400))
                .is_empty(),
            "held off by the floor"
        );
        assert_eq!(
            watch.due(storm + WATCH_QUIET + WATCH_FLOOR),
            vec![root],
            "and not dropped"
        );
    }

    /// PIN — **news about a folder nothing is watching any more is thrown away
    /// with the handle.**
    ///
    /// `git_watch`'s own rule read one lane over: a notification banked under a
    /// folder that has since been folded is about something that happened before
    /// the fold, and unfolding it again already owes a fresh read (rule 3). Left
    /// in the mailbox it would be answered *after* that read, with a re-read of
    /// a folder that has not moved since.
    #[test]
    fn a_folded_folders_unread_news_goes_with_it() {
        let mut watch = FilesWatch::default();
        let root = dir("Application");
        let inside = root.join("assets");
        sync_for_test(&mut watch, &BTreeSet::from([root.clone(), inside.clone()]));
        let start = Instant::now();

        lock(&watch.news).insert(inside.clone(), start);
        sync_for_test(&mut watch, &BTreeSet::from([root.clone()]));
        assert!(
            lock(&watch.news).is_empty(),
            "the stale stamp went with the folder nothing is watching"
        );
        assert!(watch.due(start + Duration::from_secs(60)).is_empty());

        // And the folder that is still watched keeps its own, which is the other
        // half: this is a retain over the wanted set, not a clearing.
        lock(&watch.news).insert(root.clone(), start);
        sync_for_test(&mut watch, &BTreeSet::from([root.clone(), inside]));
        assert_eq!(watch.due(start + WATCH_QUIET), vec![root]);
    }
}
