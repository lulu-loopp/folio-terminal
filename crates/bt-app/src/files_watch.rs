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
//! # Four rules
//!
//! 1. **The subscription follows what is on the glass.** [`FilesWatch::sync`] is
//!    handed the directories this window's columns are currently showing the
//!    contents of — [`crate::files::visible_dirs`] of every column of the tab on
//!    screen, and of every live float — and owns the whole difference. Folding a
//!    folder, switching tabs, re-rooting a column and closing a float all drop
//!    their watches, by the set no longer containing them. Nothing else in
//!    this file decides to watch or stop watching anything, and nothing here
//!    holds a watch: the set goes to this window's ear on the watch steward
//!    ([`crate::watch_steward`], T-DIRWATCH-LIFECYCLE), whose worker starts and
//!    drops the watches, so no arming and no retirement waits on this thread.
//! 2. **One folder, not one tree.** The ear is shallow (the steward's
//!    `Depth::Shallow`, `bt_platform::DirWatch::start_shallow`): the directory's
//!    own entries and nothing deeper. A column rooted at a repository must not
//!    make this process listen to every object file a `cargo build` writes into
//!    `target\debug` — and it would learn nothing from them either, because a
//!    row that is not on screen has no names to be wrong about. The folders that
//!    *are* on screen are watched one apiece, which is the same set and none of
//!    the cost.
//! 3. **Arming is itself a reason to re-read.** A directory whose watch has just
//!    armed is a directory this window was not listening to a moment ago, and
//!    Windows keeps no log for a subscriber who was not yet listening. So a
//!    column that already holds an answer for a directory whose watch has just
//!    armed is told to ask again: that answer was taken while nobody was
//!    watching. A directory nothing has ever asked about is not re-asked,
//!    because the walk that put it on screen is about to ask for it for the
//!    first time.
//! 4. **A folder nobody can hear is read again, and tried again only when a
//!    reader asks** (B5 design note, revisions (b)5, (c)2, (e)3). A folder whose
//!    watch could not start, or whose watch ended on its own (the folder
//!    removed, the share gone), is `Failed` — unheard. Going unheard is itself
//!    a reason to re-read, for rule 3's reason read backwards: whatever moved
//!    between the last notification and the end reached nobody. The watch is
//!    tried again **once at the next reader-triggered event** — the person
//!    refreshing that folder ([`FilesWatch::reader_asked`], from
//!    `Runtime::refresh_files_dir`) — or when a volume arrives over it (the
//!    steward's hook), and **never on a turn of the loop**, which would be a
//!    poll of a folder that is not there.
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

use crate::window_seam::LoopProxy;

use crate::{
    AppEvent,
    watch_clock::WatchClock,
    watch_steward::{Depth, EarHandle, Listener, MemberReport},
};

/// **What the steward last said about one folder.**
#[derive(Clone, Debug, Eq, PartialEq)]
enum Heard {
    /// Asked for, and not answered yet.
    Asked,
    /// The watch is listening.
    Active,
    /// Unheard (rule 4).
    Failed,
}

/// One watched folder: what the steward said about it, and the clock its
/// notifications feed.
struct Watched {
    heard: Heard,
    clock: WatchClock,
}

/// **What a sync found**: the folders whose watch has just armed (rule 3) and
/// the folders that have just gone unheard (rule 4). Both owe a re-read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Synced {
    pub armed: Vec<PathBuf>,
    pub unheard: Vec<PathBuf>,
}

impl Synced {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.armed.is_empty() && self.unheard.is_empty()
    }
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
    /// This window's ear on the watch steward, opened by the first folder it
    /// wants.
    ear: Option<EarHandle>,
    dirs: BTreeMap<PathBuf, Watched>,
    /// The `Failed` folders a reader has asked about since the last sync: the
    /// next sync tries each once (rule 4).
    owed: BTreeSet<PathBuf>,
}

impl FilesWatch {
    /// **Bring the subscriptions level with what the columns are showing** (rule
    /// 1), and say which folders owe a re-read (rules 3 and 4).
    ///
    /// **The proxy is borrowed here and cloned only where the ear is opened**,
    /// on [`crate::git_watch::GitWatch::want`]'s reason and with its weight: a
    /// clone of an `EventLoopProxy` is an `Arc` bump on Windows and, on macOS, a
    /// new run loop source added to the main run loop and a **wake-up** of it.
    /// This is asked on every turn, so a clone taken at the top of it would
    /// schedule the next turn and the turn after that, for ever, over a window
    /// nobody is looking at. The clone belongs in [`listener`], once per window,
    /// and nowhere on this path.
    pub fn sync(&mut self, wanted: &BTreeSet<PathBuf>, proxy: &LoopProxy) -> Synced {
        self.sync_with(wanted, |news| {
            crate::watch_steward::open_ear(Depth::Shallow, listener(news, proxy))
        })
    }

    /// [`Self::sync`]'s bookkeeping, with the opening of the ear handed in — the
    /// product's steward, or a test's.
    fn sync_with(
        &mut self,
        wanted: &BTreeSet<PathBuf>,
        open_ear: impl FnOnce(&Arc<Mutex<BTreeMap<PathBuf, Instant>>>) -> EarHandle,
    ) -> Synced {
        // **A window with no file tree open touches nothing at all**, not even
        // the mailbox's lock. This is asked on every turn of the event loop,
        // mouse moves included.
        if wanted.is_empty() && self.dirs.is_empty() {
            return Synced::default();
        }
        let held = self.dirs.len();
        // Departures first: the ear's next set leaves them out, and the steward
        // drops their watches.
        self.dirs.retain(|directory, _| wanted.contains(directory));
        let departed = self.dirs.len() != held;
        let mut arrived = false;
        for directory in wanted {
            if !self.dirs.contains_key(directory) {
                arrived = true;
                self.dirs.insert(
                    directory.clone(),
                    Watched {
                        heard: Heard::Asked,
                        clock: WatchClock::default(),
                    },
                );
            }
        }
        // Rule 4's one attempt: only a folder still `Failed` and still wanted.
        let rearm: BTreeSet<PathBuf> = std::mem::take(&mut self.owed)
            .into_iter()
            .filter(|directory| {
                self.dirs
                    .get(directory)
                    .is_some_and(|entry| entry.heard == Heard::Failed)
            })
            .collect();
        let news = &self.news;
        let ear = self.ear.get_or_insert_with(|| open_ear(news));
        ear.want(wanted, rearm);
        let mut synced = Synced::default();
        for (directory, numbered) in ear.take_reports() {
            let Some(entry) = self.dirs.get_mut(&directory) else {
                continue;
            };
            match numbered.report {
                MemberReport::Armed => {
                    if entry.heard != Heard::Active {
                        entry.heard = Heard::Active;
                        synced.armed.push(directory);
                    }
                }
                MemberReport::Failed(cause) => {
                    if entry.heard != Heard::Failed {
                        trace(&format!("cannot watch {}: {cause}", directory.display()));
                        entry.heard = Heard::Failed;
                        synced.unheard.push(directory);
                    }
                }
            }
        }
        if departed || arrived || !synced.is_empty() {
            // A folder that stopped being watched left its unread news behind,
            // and acting on that stamp the next time the folder came back would
            // be acting on it *after* the fresh read that rule 3 already owes.
            lock(&self.news).retain(|directory, _| wanted.contains(directory));
            let (held, watching) = self.counts();
            trace(&format!(
                "{watching} of {held} folders on screen are being watched"
            ));
        }
        synced
    }

    /// **A reader asked for this folder again** (rule 4): the person refreshed
    /// it. A folder that is unheard is tried once, at the next sync; any other
    /// folder owes nothing here.
    pub fn reader_asked(&mut self, directory: &Path) {
        if self
            .dirs
            .get(directory)
            .is_some_and(|entry| entry.heard == Heard::Failed)
        {
            self.owed.insert(directory.to_path_buf());
        }
    }

    /// The folders whose requested watches are unheard. A volume arrival re-reads these folders;
    /// the steward's volume hook owns the single rearm attempt itself.
    pub fn failed(&self) -> BTreeSet<PathBuf> {
        self.dirs
            .iter()
            .filter(|(_, entry)| entry.heard == Heard::Failed)
            .map(|(directory, _)| directory.clone())
            .collect()
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

    /// How many folders are subscribed to, and how many of them are being
    /// heard. For the tests and the trace line.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (
            self.dirs.len(),
            self.dirs
                .values()
                .filter(|held| held.heard == Heard::Active)
                .count(),
        )
    }
}

/// **The ear's listener**: what a watcher thread does when a folder moves.
///
/// A free function for `git_watch::subscribe`'s reason, and **the only place in
/// this file the proxy is cloned** — once per window, when its ear opens, for
/// the reason [`FilesWatch::sync`] states.
fn listener(news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>, proxy: &LoopProxy) -> Listener {
    let mailbox = Arc::clone(news);
    let proxy = proxy.clone();
    Arc::new(move |directory: &Path, _| {
        lock(&mailbox).insert(directory.to_path_buf(), Instant::now());
        // The loop is woken, not told what to do: what a change means is decided
        // on the main thread, where the caches and the columns are.
        let _ = proxy.send_event(AppEvent::FilesDirChanged);
    })
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
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::lane::wait_for_a_wake;
    use crate::watch_clock::{WATCH_FLOOR, WATCH_QUIET};
    use crate::watch_steward::Steward;
    use crate::watch_steward::fake::FakeDoor;

    fn dir(name: &str) -> PathBuf {
        PathBuf::from(r"D:\Users\alice\文档").join(name)
    }

    /// **A files watch over a steward of its own**: `door` behind it, and the
    /// wakes its worker sends after each publication.
    struct Rig {
        watch: FilesWatch,
        steward: &'static Steward,
        wakes: mpsc::Receiver<()>,
        /// What the turns since the last answer found: the worker may publish
        /// between a request and the drain of the same turn, so a turn's finds
        /// are kept until the test reads the answer.
        found: Synced,
    }

    impl Rig {
        fn over(door: impl crate::watch_steward::Door + 'static) -> Self {
            let steward: &'static Steward = Box::leak(Box::new(Steward::new(door, |_| {})));
            let (woke, wakes) = mpsc::channel();
            steward.install_wake(move || {
                let _ = woke.send(());
            });
            Self {
                watch: FilesWatch::default(),
                steward,
                wakes,
                found: Synced::default(),
            }
        }

        /// One turn's sync, as `advance_files_watch` makes it.
        fn sync(&mut self, wanted: &BTreeSet<PathBuf>) -> Synced {
            let steward = self.steward;
            let synced = self.watch.sync_with(wanted, |news| {
                let mailbox = Arc::clone(news);
                steward.open_ear(
                    Depth::Shallow,
                    Arc::new(move |directory: &Path, _| {
                        lock(&mailbox).insert(directory.to_path_buf(), Instant::now());
                    }),
                )
            });
            self.found.armed.extend(synced.armed.iter().cloned());
            self.found.unheard.extend(synced.unheard.iter().cloned());
            synced
        }

        /// **The answer**: what the turns since the last answer found, through
        /// the turn the steward's wake brings the loop to.
        fn sync_after_the_wake(&mut self, wanted: &BTreeSet<PathBuf>, what: &str) -> Synced {
            wait_for_a_wake(&self.wakes, what);
            self.sync(wanted);
            std::mem::take(&mut self.found)
        }
    }

    /// PIN (rule 1) — **the subscriptions are the folders on the glass**, and
    /// folding one is what releases its watch, on the steward.
    ///
    /// RED GATE: have `sync_with` skip the `retain` — the folded folder keeps
    /// its watch for the life of the window, and the drop count stays put.
    #[test]
    fn the_subscriptions_are_the_folders_on_screen_and_folding_one_releases_it() {
        let door = FakeDoor::default();
        let mut rig = Rig::over(door.clone());
        let root = dir("Application");
        let inside = root.join("assets");

        assert!(rig.sync(&BTreeSet::new()).is_empty());
        assert_eq!(
            rig.watch.counts().0,
            0,
            "nothing on screen, nothing watched"
        );

        rig.sync(&BTreeSet::from([root.clone()]));
        let synced = rig.sync_after_the_wake(&BTreeSet::from([root.clone()]), "the root armed");
        assert_eq!(
            synced.armed,
            vec![root.clone()],
            "a column rooted somewhere arms it"
        );
        assert_eq!(rig.watch.counts(), (1, 1));

        rig.sync(&BTreeSet::from([root.clone(), inside.clone()]));
        let synced = rig.sync_after_the_wake(
            &BTreeSet::from([root.clone(), inside.clone()]),
            "the unfolded folder armed",
        );
        assert_eq!(
            synced.armed,
            vec![inside.clone()],
            "and only the one unfolded"
        );
        assert_eq!(door.attempts(&root), 1, "the root was not started twice");

        rig.sync(&BTreeSet::from([root.clone()]));
        assert_eq!(
            rig.watch.counts().0,
            1,
            "folding it back drops it from the set"
        );
        door.wait_for_drops(1);
        assert_eq!(
            door.live(),
            vec![root.clone()],
            "and its watch, on the steward"
        );

        rig.sync(&BTreeSet::new());
        door.wait_for_drops(2);
        assert!(
            door.live().is_empty(),
            "the column going away drops the rest"
        );
        assert_eq!(rig.watch.deadline(), None, "and nothing is owed by nobody");
    }

    /// PIN (rule 3) — **a folder that has just been armed is a folder whose
    /// answer was taken while nobody was listening**, reported once, on the turn
    /// its arming lands, and again after it leaves the screen and comes back.
    #[test]
    fn arming_a_folder_is_reported_once_and_only_on_the_turn_it_arrives() {
        let mut rig = Rig::over(FakeDoor::default());
        let root = dir("Application");
        let wanted = BTreeSet::from([root.clone()]);

        rig.sync(&wanted);
        assert_eq!(
            rig.sync_after_the_wake(&wanted, "armed").armed,
            vec![root.clone()]
        );
        assert!(rig.sync(&wanted).is_empty());

        // Away and back — a tab switched off and on again — is a second arming,
        // because in between this window was not listening.
        assert!(rig.sync(&BTreeSet::new()).is_empty());
        rig.sync(&wanted);
        assert_eq!(
            rig.sync_after_the_wake(&wanted, "armed again").armed,
            vec![root]
        );
    }

    /// PIN — **one write is one re-read, a burst is one, and a folder nobody
    /// touches is none.**
    ///
    /// RED GATE: drop the `take_due` filter in [`FilesWatch::due`] — every
    /// watched folder answers on every turn of the loop, and the first
    /// assertion goes red before the file has finished being written.
    #[test]
    fn one_new_file_is_one_re_read_a_quiet_window_after_the_last_thing_that_moved() {
        let mut rig = Rig::over(FakeDoor::default());
        let root = dir("Application");
        let elsewhere = dir("Photos");
        rig.sync(&BTreeSet::from([root.clone(), elsewhere.clone()]));
        let watch = &mut rig.watch;
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

        assert!(watch.due(start + Duration::from_secs(3600)).is_empty());
        assert_eq!(watch.deadline(), None);

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
    /// with it.**
    #[test]
    fn a_folded_folders_unread_news_goes_with_it() {
        let mut rig = Rig::over(FakeDoor::default());
        let root = dir("Application");
        let inside = root.join("assets");
        rig.sync(&BTreeSet::from([root.clone(), inside.clone()]));
        let start = Instant::now();

        lock(&rig.watch.news).insert(inside.clone(), start);
        rig.sync(&BTreeSet::from([root.clone()]));
        assert!(
            lock(&rig.watch.news).is_empty(),
            "the stale stamp went with the folder nothing is watching"
        );
        assert!(rig.watch.due(start + Duration::from_secs(60)).is_empty());

        lock(&rig.watch.news).insert(root.clone(), start);
        rig.sync(&BTreeSet::from([root.clone(), inside]));
        assert_eq!(rig.watch.due(start + WATCH_QUIET), vec![root]);
    }

    /// PIN (B5 design note (b)5, the first of its pair) — **a folder that cannot
    /// be watched at all is unheard from the start, and owes a re-read.**
    ///
    /// The real door, through a real steward: a folder that does not exist, and
    /// a folder on a share whose host does not exist. Both come back `Failed`,
    /// counted as not heard, and named in `unheard` — the list
    /// `advance_files_watch` re-reads and marks the palette's index dirty for.
    ///
    /// MUTATION: make `sync_with` ignore a `Failed` report — `unheard` stays
    /// empty and the folder reads as merely asked.
    #[test]
    fn a_folder_that_cannot_be_watched_is_unheard_from_the_start_and_owes_a_re_read() {
        let mut rig = Rig::over(crate::watch_steward::Kernel);
        let missing = bt_testpath::temp_path("folio-files-watch-never-made");
        let _ = std::fs::remove_dir_all(&missing);
        // Off Windows the share's spelling is a name that is not there, which is
        // the same refusal.
        let share = PathBuf::from(r"\\folio-no-such-host.invalid\share\user");
        let wanted = BTreeSet::from([missing, share]);
        rig.sync(&wanted);
        let mut unheard = Vec::new();
        while unheard.len() < wanted.len() {
            unheard.extend(rig.sync_after_the_wake(&wanted, "the refusals").unheard);
        }
        unheard.sort();
        assert_eq!(unheard, wanted.iter().cloned().collect::<Vec<_>>());
        assert_eq!(rig.watch.counts(), (wanted.len(), 0), "nothing is heard");
    }

    /// PIN (B5 design note (b)5, the second of its pair) — **a folder whose watch
    /// ends after it armed goes unheard, and owes a re-read.**
    ///
    /// The real door, through a real steward: the folder is watched, armed, and
    /// then removed. The platform ends the watch (`is_armed` false, `Ended`
    /// said), the steward takes the end and reports `Failed`, and this sync
    /// names the folder in `unheard`.
    ///
    /// MUTATIONS: make the steward ignore `DirChange::Ended` (pass it to the
    /// listener) — no `Failed` report comes and the wait for it fails; make
    /// `DirWatch::is_armed` answer `true` — unchanged here, which is why the
    /// platform's own test pins that half.
    #[test]
    fn a_folder_whose_watch_ends_after_arming_goes_unheard_and_owes_a_re_read() {
        let mut rig = Rig::over(crate::watch_steward::Kernel);
        let folder = bt_testpath::temp_path("folio-files-watch-ends");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("make the watched folder");
        let wanted = BTreeSet::from([folder.clone()]);
        rig.sync(&wanted);
        assert_eq!(
            rig.sync_after_the_wake(&wanted, "the folder armed").armed,
            vec![folder.clone()]
        );
        std::fs::remove_dir_all(&folder).expect("remove the watched folder");
        let synced = rig.sync_after_the_wake(&wanted, "the end");
        assert_eq!(synced.unheard, vec![folder], "the end is a re-read owed");
        assert_eq!(rig.watch.counts(), (1, 0), "and the folder is not heard");
    }

    /// PIN (B5 design note (c)2, the first of its pair) — **an unheard folder is
    /// tried again once when a reader asks for it, and heard again once it is
    /// back.**
    ///
    /// MUTATION: never rearm (drop `owed` from the request) — the recreated
    /// folder is never started again and the attempt count stays at one.
    #[test]
    fn an_unheard_folder_is_tried_again_when_a_reader_asks_for_it() {
        let door = FakeDoor::default();
        let mut rig = Rig::over(door.clone());
        let folder = dir("Downloads");
        let wanted = BTreeSet::from([folder.clone()]);
        door.refuse(&folder);
        rig.sync(&wanted);
        assert_eq!(
            rig.sync_after_the_wake(&wanted, "refused").unheard,
            vec![folder.clone()]
        );
        assert_eq!(door.attempts(&folder), 1);

        // The folder is back; the reader refreshes it.
        door.admit(&folder);
        rig.watch.reader_asked(&folder);
        rig.sync(&wanted);
        let synced = rig.sync_after_the_wake(&wanted, "the reader's attempt");
        assert_eq!(door.attempts(&folder), 2, "one attempt, for the one ask");
        assert_eq!(synced.armed, vec![folder.clone()], "and it is heard again");
        assert_eq!(rig.watch.counts(), (1, 1));
    }

    /// PIN (B5 design note (c)2, the second of its pair) — **an unheard folder is
    /// not tried again because the loop turned.**
    ///
    /// A hundred turns' syncs of the same set, then a change of the set that the
    /// steward must serve (another folder unfolded) so the test knows every
    /// request before it was served: the unheard folder was started once.
    ///
    /// MUTATION: rearm every `Failed` folder on every sync (put every unheard
    /// folder in `rearm`) — the attempt count rises without an ask.
    #[test]
    fn an_unheard_folder_is_not_tried_again_on_a_turn_of_the_loop() {
        let door = FakeDoor::default();
        let mut rig = Rig::over(door.clone());
        let folder = dir("Downloads");
        let wanted = BTreeSet::from([folder.clone()]);
        door.refuse(&folder);
        rig.sync(&wanted);
        assert_eq!(
            rig.sync_after_the_wake(&wanted, "refused").unheard,
            vec![folder.clone()]
        );
        for _ in 0..100 {
            assert!(rig.sync(&wanted).is_empty(), "a turn owes nothing");
        }
        let other = dir("Music");
        let both = BTreeSet::from([folder.clone(), other.clone()]);
        rig.sync(&both);
        assert_eq!(
            rig.sync_after_the_wake(&both, "the other folder").armed,
            vec![other]
        );
        assert_eq!(
            door.attempts(&folder),
            1,
            "no turn tried the unheard folder again"
        );
    }
}
