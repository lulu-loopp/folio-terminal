//! **The kernel's word that the file a preview seat is showing was saved**
//! (Web 预览块 W2 片⑤; `docs/plans/web-preview/plan.md` §0「静态 HTML 存盘自刷 =
//! 抽取/新增通用 preview-file watcher」).
//!
//! # General, and not the web block's
//!
//! The plan names this under the web block because a static page saved in an
//! editor is where it was first asked for, but nothing here knows what a page
//! is. What it watches is **the file one preview seat is currently showing**,
//! and what it answers is *that file moved*. Who acts on that — a page taking
//! an ordinary `Reload`, a document re-reading its head — is decided by the
//! window, one door per content lane, exactly as every other piece of news in
//! this product is.
//!
//! # Why this is allowed to exist under a rule that forbids polling
//!
//! `git_watch`'s sentence, unchanged: everything here is downstream of a
//! `ReadDirectoryChangesW` completion. A window left open over a file nobody
//! writes to runs no timer, wakes for nothing and reads no disk. The clock
//! below is [`WatchClock`] — the same arithmetic the two directory watchers
//! share — and its question is "has it stopped changing", asked only because it
//! already did.
//!
//! # Three rules
//!
//! 1. **The subscription follows the seat.** [`PreviewWatch::sync`] is handed
//!    the set of files this window's preview panes are showing and owns the
//!    whole difference: a seat that changes file drops the old subscription in
//!    the same breath it opens the new one, and a seat that closes drops it
//!    outright. Nothing else in this file decides to watch or stop watching
//!    anything.
//! 2. **One folder, not one tree.** Windows cannot subscribe to a file, so the
//!    handle is on the file's directory — `bt_platform::DirWatch::start_shallow`,
//!    which asks the kernel for that directory's own entries and nothing
//!    deeper. A README previewed out of a repository root must not wake this
//!    thread for every object file a build writes into `target\debug`.
//! 3. **A notification is not an answer.** A shallow watch still speaks for
//!    every *sibling* in the folder, and a folder of documents is the ordinary
//!    case. So when the clock comes due the file itself is asked for its
//!    modified time and its length, and only a file whose answer differs from
//!    the one recorded when it was last read is news. That is one `metadata`
//!    call per file per quiet window, made **because the kernel spoke** — the
//!    difference from a poll that `watch_clock`'s own header states.
//!
//! # What a change means here
//!
//! Nothing is parsed out of the notification — `git_watch`'s rule, one level
//! down. A save, a rename and a delete cost the same thought, and the
//! write-then-rename dance an editor performs arrives as one piece of news
//! because [`WatchClock`] is what turns a burst into one. A file that has *gone*
//! is a change like any other: the stamp moves from `Some` to `None`, the seat
//! is told, and what an absent file looks like is the content lane's business.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, watch_clock::WatchClock};

/// **What a file looked like the last time this window read it.**
///
/// Two fields and not a hash: a hash is a read of the whole file, which is the
/// very thing this exists to schedule rather than to perform. Length alone
/// misses an edit that keeps the size; a modified time alone misses a write
/// inside the filesystem's own resolution while the length changed. Together
/// they are what every build system on this platform uses to answer the same
/// question, and the cost of being wrong is one re-read that finds identical
/// bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    /// Ask the disk. `None` is a real answer — the file is not there — and it
    /// compares unequal to every `Some`, which is how a delete is a change.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// One watched file: the clock its folder's notifications feed, and what the
/// file looked like when this window last acted on it.
struct Watched {
    clock: WatchClock,
    /// `None` before the first look and after the file goes away. Recorded when
    /// the subscription opens, so that a file written *between* the seat opening
    /// it and this arming is not reported as a change the seat has already seen.
    stamp: Option<Stamp>,
}

/// **One file moved, and whether it is still there.**
///
/// The stamp comparison already knows the difference — a `None` is what makes a
/// delete news at all (rule 3) — and throwing it away at the door would make
/// every reader ask the disk a second time to learn what this one already
/// found out. It is also the *only* honest moment to ask: by the time the
/// window has walked its pools, a file deleted and recreated has an answer that
/// no longer describes the notification being answered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileNews {
    pub path: PathBuf,
    /// `false` when the disk says the file is not there any more.
    pub present: bool,
}

/// **Every file this window's preview seats are watching.**
#[derive(Default)]
pub struct PreviewWatch {
    /// Where the watcher threads leave their news, keyed by the **directory**
    /// they are subscribed to — one entry however many notifications arrived,
    /// for `git_watch`'s reason: the only thing worth keeping about ten of them
    /// is that there were some and when the last one was.
    news: Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    /// One live subscription per directory, however many watched files are in
    /// it. **The handle is the subscription**: dropping it cancels.
    folders: BTreeMap<PathBuf, bt_platform::DirWatch>,
    /// **And the directories this process could not take one out on** — a
    /// network share, a `\\wsl$` mount, a folder it may not open.
    ///
    /// A second membership rather than a `None` beside the handles, and the
    /// reason is that this set is *read* and not merely recorded: rule 4's
    /// second road is exactly "the files in these folders", and a state that
    /// something depends on deserves a name rather than a reading of an
    /// `Option`. Kept — rather than the folder simply being absent — so that the
    /// attempt is made once per time a file in it is opened rather than once per
    /// turn of the event loop.
    ///
    /// The two are exclusive by construction: one folder goes into exactly one
    /// of them, in [`Self::sync_with`], and leaves both together.
    unwatchable: BTreeSet<PathBuf>,
    files: BTreeMap<PathBuf, Watched>,
}

impl PreviewWatch {
    /// **Bring the subscriptions level with what the seats are showing** (rule
    /// 1).
    ///
    /// `wanted` is every file some preview pane in this window currently has
    /// open — across tabs, not only the tab on screen. A background tab's
    /// buffer is *content* and is not read again when you come back to it, so
    /// gating this on visibility would make returning to a tab the one place
    /// this window knowingly shows a file it has been told is out of date.
    ///
    /// Answers whether the set changed, which is only of interest to a
    /// diagnostics line.
    pub fn sync(&mut self, wanted: &BTreeSet<PathBuf>, proxy: &EventLoopProxy<AppEvent>) -> bool {
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        let changed = self.sync_with(
            wanted,
            move |directory| subscribe(&news, &proxy, directory),
            Stamp::of,
        );
        if changed {
            let (files, folders) = self.counts();
            trace(&format!(
                "{files} previewed files, in {folders} folders this process could open"
            ));
        }
        changed
    }

    /// [`Self::sync`]'s bookkeeping, with the opening of a watch and the reading
    /// of a stamp handed in.
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
        mut stamp: impl FnMut(&Path) -> Option<Stamp>,
    ) -> bool {
        // **A window with no preview open touches nothing at all**, not even the
        // mailbox's lock. This is asked on every turn of the event loop, mouse
        // moves included.
        if wanted.is_empty() && self.files.is_empty() {
            return false;
        }
        let before = self.files.len();
        // Departures first, and the drop is the cancellation.
        self.files.retain(|path, _| wanted.contains(path));
        let mut changed = self.files.len() != before;
        for path in wanted {
            if self.files.contains_key(path) {
                continue;
            }
            changed = true;
            self.files.insert(
                path.clone(),
                Watched {
                    clock: WatchClock::default(),
                    // **Read at the moment of arming**, so that the seat's own
                    // opening read is the baseline. Without it the first
                    // notification of any kind about this folder would count as
                    // a change to a file this window has just read.
                    stamp: stamp(path),
                },
            );
        }
        let folders: BTreeSet<PathBuf> = self
            .files
            .keys()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        let held = self.folders.len() + self.unwatchable.len();
        self.folders
            .retain(|directory, _| folders.contains(directory));
        self.unwatchable.retain(|directory| folders.contains(directory));
        changed |= self.folders.len() + self.unwatchable.len() != held;
        for directory in &folders {
            if self.folders.contains_key(directory) || self.unwatchable.contains(directory) {
                continue;
            }
            changed = true;
            match open(directory) {
                Some(watch) => {
                    self.folders.insert(directory.clone(), watch);
                }
                None => {
                    self.unwatchable.insert(directory.clone());
                }
            }
        }
        // A folder that stopped being watched left its unread news behind, and a
        // stale timestamp for it would be acted on the next time somebody opened
        // a file in it — after the fresh read that opening already performs.
        lock(&self.news).retain(|directory, _| folders.contains(directory));
        changed
    }

    /// **Which watched files actually moved**, given everything the kernel has
    /// said since the last time this was asked.
    ///
    /// Folding the news in and answering are one step for `GitWatch::due`'s
    /// reason: a notification that arrived a moment ago may or may not have made
    /// its folder due, and the only way to find out is to give it to the clock
    /// first.
    pub fn due(&mut self, now: Instant) -> Vec<FileNews> {
        self.due_with(now, Stamp::of)
    }

    fn due_with(
        &mut self,
        now: Instant,
        mut stamp: impl FnMut(&Path) -> Option<Stamp>,
    ) -> Vec<FileNews> {
        if self.files.is_empty() {
            return Vec::new();
        }
        for (directory, at) in std::mem::take(&mut *lock(&self.news)) {
            for (path, entry) in &mut self.files {
                if path.parent() == Some(directory.as_path()) {
                    entry.clock.note_event(at);
                }
            }
        }
        let mut moved = Vec::new();
        for (path, entry) in &mut self.files {
            if !entry.clock.take_due(now) {
                continue;
            }
            // Rule 3: the folder spoke, so this file is *asked*. A sibling's
            // save is the ordinary reason to arrive here, and it is not news
            // about this file.
            let fresh = stamp(path);
            if fresh == entry.stamp {
                continue;
            }
            entry.stamp = fresh;
            moved.push(FileNews {
                path: path.clone(),
                present: fresh.is_some(),
            });
        }
        moved
    }

    /// **Rule 4 — the files no kernel is speaking for** (user ruling
    /// 2026-08-29).
    ///
    /// A folder on a network share, on a `\wsl$` mount, or one this process may
    /// not open comes back from [`subscribe`] as `None`, and every rule above
    /// this one is then silent about the files in it: nothing arrives in the
    /// mailbox, no clock ever runs, and a document opened off such a folder is
    /// the one document in this window that would go on showing the bytes it
    /// was opened with for the rest of the session. That is the state the whole
    /// of this module exists to end, so it needs a second road — and the ruling
    /// names the two moments it is allowed to travel on: **the window is given
    /// focus**, and **a document is brought to the front**.
    ///
    /// **It is not a poll and it must never become one.** Two properties keep
    /// that true rather than promise it:
    ///
    /// * it asks **only** about files whose folder holds no handle. A watched
    ///   folder has a kernel speaking for it, and asking it here as well would
    ///   be exactly the schedule `watch_clock`'s header refuses;
    /// * it is called **from a gesture** — an activation, a switch — and never
    ///   from a clock. Nothing here asks for a wake-up, and
    ///   [`Self::deadline`] is unchanged by it.
    ///
    /// So the cost is one `metadata` per unwatchable file per time somebody
    /// comes back to this window, which is the price of the answer being right
    /// on a share, and it is zero for every window whose files are all
    /// watchable — which is every window on a local disk.
    pub fn ask_the_unwatched(&mut self) -> Vec<FileNews> {
        self.ask_the_unwatched_with(Stamp::of)
    }

    fn ask_the_unwatched_with(
        &mut self,
        mut stamp: impl FnMut(&Path) -> Option<Stamp>,
    ) -> Vec<FileNews> {
        // **A window whose folders are all watched touches no disk at all**, and
        // this is the line that says so: the common case is that every handle is
        // held, and then there is nothing here to walk.
        if self.unwatchable.is_empty() {
            return Vec::new();
        }
        let mut moved = Vec::new();
        for (path, entry) in &mut self.files {
            let unwatched = path
                .parent()
                .is_some_and(|directory| self.unwatchable.contains(directory));
            if !unwatched {
                continue;
            }
            let fresh = stamp(path);
            if fresh == entry.stamp {
                continue;
            }
            entry.stamp = fresh;
            moved.push(FileNews {
                path: path.clone(),
                present: fresh.is_some(),
            });
        }
        moved
    }

    /// When the loop must wake to answer the news it is already holding.
    ///
    /// `None` while nothing is owed, which is the ordinary state of a window
    /// showing a file nobody is writing to — and the reason this mechanism costs
    /// no wake-ups at all when nothing is happening.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.files
            .values()
            .filter_map(|entry| entry.clock.due_at())
            .min()
    }

    /// How many files are subscribed to, and how many of their folders the
    /// platform could actually open a watch on. For the tests and the trace
    /// line.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.files.len(), self.folders.len())
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
    // Shallow (rule 2): this folder's own entries, and nothing under it.
    let started = bt_platform::DirWatch::start_shallow(directory, move || {
        lock(&mailbox).insert(key.clone(), Instant::now());
        // The loop is woken, not told what to do: what a change means is decided
        // on the main thread, where the clocks and the seats are.
        let _ = proxy.send_event(AppEvent::PreviewFileChanged);
    });
    match started {
        Ok(watch) => Some(watch),
        Err(error) => {
            // **Quietly.** A network share, a `\\wsl$` mount, a folder this
            // process may not open: the answer is to have no watcher, and a
            // preview that does not refresh itself is exactly what this product
            // did until today. There is nothing here a reader could act on.
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

/// The door for whoever is debugging a preview that did not refresh itself.
///
/// `BT_GIT_TRACE` is the watcher family's switch and this is a watcher; a second
/// variable for the same class of question would be one more thing to know
/// before you can ask it. Set-but-empty is off, on `BT_PERF_TRACE`'s own rule.
fn trace(message: &str) {
    if std::env::var_os("BT_GIT_TRACE").is_some_and(|value| !value.is_empty()) {
        eprintln!("preview watch: {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::watch_clock::WATCH_QUIET;

    fn file(name: &str) -> PathBuf {
        PathBuf::from(r"D:\notes").join(name)
    }

    /// [`PreviewWatch::sync`] with no kernel behind it, and a stamp table
    /// instead of a disk.
    fn sync_for_test(
        watch: &mut PreviewWatch,
        wanted: &BTreeSet<PathBuf>,
        stamps: &BTreeMap<PathBuf, Stamp>,
    ) -> bool {
        watch.sync_with(wanted, |_| None, |path| stamps.get(path).copied())
    }

    fn stamp(len: u64) -> Stamp {
        Stamp {
            modified: None,
            len,
        }
    }

    /// How many folders this watch is following at all — the ones it holds a
    /// handle on and the ones it has recorded it cannot. The tests' `open`
    /// answers `None` for everything, so the second is where they all land.
    fn subscribed(watch: &PreviewWatch) -> usize {
        watch.folders.len() + watch.unwatchable.len()
    }

    /// The ordinary piece of news: this file moved and it is still there.
    fn present(path: &Path) -> FileNews {
        FileNews {
            path: path.to_path_buf(),
            present: true,
        }
    }

    /// PIN (rule 1) — **the subscription follows the seat**, and a window with
    /// no preview open holds nothing at all.
    #[test]
    fn the_subscriptions_follow_the_files_the_seats_are_showing() {
        let mut watch = PreviewWatch::default();
        let stamps = BTreeMap::new();
        let a = file("a.md");
        let b = file("b.md");

        assert!(!sync_for_test(&mut watch, &BTreeSet::new(), &stamps));
        assert_eq!(watch.counts().0, 0, "nothing showing, nothing watched");

        assert!(sync_for_test(
            &mut watch,
            &BTreeSet::from([a.clone()]),
            &stamps
        ));
        assert_eq!(watch.counts().0, 1);
        assert!(
            !sync_for_test(&mut watch, &BTreeSet::from([a.clone()]), &stamps),
            "the same set again is not a change and re-opens nothing"
        );

        // Two files in one folder are two clocks and one subscription.
        assert!(sync_for_test(
            &mut watch,
            &BTreeSet::from([a.clone(), b.clone()]),
            &stamps
        ));
        assert_eq!(watch.counts().0, 2);
        assert_eq!(subscribed(&watch), 1, "one folder, one subscription");

        // The seat is pointed at another file, or closed.
        assert!(sync_for_test(
            &mut watch,
            &BTreeSet::from([b.clone()]),
            &stamps
        ));
        assert_eq!(watch.counts().0, 1);
        assert!(sync_for_test(&mut watch, &BTreeSet::new(), &stamps));
        assert_eq!(watch.counts().0, 0);
        assert_eq!(
            subscribed(&watch),
            0,
            "and the folder went with the last file"
        );
        assert_eq!(watch.deadline(), None, "and nothing is owed by nobody");
    }

    /// PIN (rule 3) — **a save is one refresh, not three, and a sibling's save
    /// is none.**
    ///
    /// The two halves of what the ticket asks for, written as one test because
    /// they are one mechanism seen from two sides. An editor saving one file
    /// writes a temporary, renames it over the target and touches its
    /// attributes: three notifications inside a few milliseconds, and a seat
    /// that reloaded a page for each of them would flash three times for one
    /// `Ctrl+S`. A folder of documents is the ordinary case, so the *other*
    /// document being saved arrives at exactly the same door and must produce
    /// nothing.
    ///
    /// RED GATE: drop the stamp comparison in [`PreviewWatch::due_with`] — the
    /// sibling half goes red, and the answer becomes "whatever moved in that
    /// folder".
    #[test]
    fn one_save_is_one_refresh_and_a_siblings_save_is_none() {
        let mut watch = PreviewWatch::default();
        let watched = file("a.md");
        let sibling = file("b.md");
        let mut stamps =
            BTreeMap::from([(watched.clone(), stamp(10)), (sibling.clone(), stamp(20))]);
        sync_for_test(&mut watch, &BTreeSet::from([watched.clone()]), &stamps);
        let start = Instant::now();
        let folder = watched.parent().expect("a folder").to_path_buf();

        // One save: the write, the rename and the attribute change.
        stamps.insert(watched.clone(), stamp(11));
        for step in [0, 3, 9] {
            lock(&watch.news).insert(folder.clone(), start + Duration::from_millis(step));
            assert!(
                watch
                    .due_with(start + Duration::from_millis(step), |path| stamps
                        .get(path)
                        .copied())
                    .is_empty(),
                "nothing fires while the folder is still moving"
            );
        }
        assert_eq!(
            watch.due_with(start + Duration::from_millis(9) + WATCH_QUIET, |path| {
                stamps.get(path).copied()
            }),
            vec![present(&watched)],
            "one save, one refresh, a quiet window after the last thing that moved"
        );
        assert_eq!(watch.deadline(), None, "and then it is quiet again");

        // The sibling is saved. The same folder speaks, the clock comes due, and
        // the file this seat is showing has not moved.
        stamps.insert(sibling, stamp(21));
        let later = start + Duration::from_secs(30);
        lock(&watch.news).insert(folder, later);
        assert!(
            watch
                .due_with(later + WATCH_QUIET, |path| stamps.get(path).copied())
                .is_empty(),
            "a folder is not a file: the neighbour moved, not this one"
        );
    }

    /// PIN (rule 1) - **a seat that changed file stops hearing about the old
    /// one**, whichever folder the new one is in.
    ///
    /// Two halves, because the mailbox is keyed by *folder* and the clocks by
    /// *file*, and the two spellings of "the seat moved on" go through different
    /// parts of that:
    ///
    /// * to another folder, the old subscription is dropped and the news banked
    ///   under it goes with it - `git_watch`'s own rule read one lane over: what
    ///   the notification was about happened before this file was opened, and
    ///   opening it already read it;
    /// * to a neighbour in the *same* folder, the subscription is legitimately
    ///   kept, the leftover news reaches the new file's clock, and the new
    ///   file's own stamp is what says nothing happened to it.
    #[test]
    fn a_seat_that_changed_file_stops_hearing_about_the_old_one() {
        let mut watch = PreviewWatch::default();
        let was = file("was.md");
        let elsewhere = PathBuf::from(r"D:\elsewhere").join("now.md");
        let neighbour = file("neighbour.md");
        let mut stamps = BTreeMap::from([
            (was.clone(), stamp(1)),
            (elsewhere.clone(), stamp(2)),
            (neighbour.clone(), stamp(3)),
        ]);
        sync_for_test(&mut watch, &BTreeSet::from([was.clone()]), &stamps);
        let start = Instant::now();
        let folder = was.parent().expect("a folder").to_path_buf();

        // The old file is written while the seat is being pointed at a file in
        // another folder.
        stamps.insert(was.clone(), stamp(99));
        lock(&watch.news).insert(folder.clone(), start);
        sync_for_test(&mut watch, &BTreeSet::from([elsewhere.clone()]), &stamps);
        assert!(
            lock(&watch.news).is_empty(),
            "the stale news went with the folder nothing is watching any more"
        );
        assert!(
            watch
                .due_with(start + Duration::from_secs(60), |path| stamps
                    .get(path)
                    .copied())
                .is_empty(),
            "the file the seat left behind owes it nothing"
        );

        // And the neighbour case: the folder is still watched, so its news is
        // still there, and the file now on the seat has to answer for itself.
        sync_for_test(&mut watch, &BTreeSet::from([was.clone()]), &stamps);
        stamps.insert(was.clone(), stamp(100));
        let later = start + Duration::from_secs(120);
        lock(&watch.news).insert(folder, later);
        sync_for_test(&mut watch, &BTreeSet::from([neighbour]), &stamps);
        assert!(
            watch
                .due_with(later + WATCH_QUIET, |path| stamps.get(path).copied())
                .is_empty(),
            "the neighbour's folder spoke about a file this seat is not showing"
        );
    }

    /// PIN (rule 3) — **a file that goes away is a change, and so is one that
    /// comes back.**
    ///
    /// The rename-delete-recreate dance `plan.md` §0 names, in the one form that
    /// is not covered by "the length changed": a stamp of `None` compares
    /// unequal to every `Some`, both ways round.
    #[test]
    fn a_file_that_disappears_and_returns_is_two_pieces_of_news() {
        let mut watch = PreviewWatch::default();
        let watched = file("a.md");
        let mut stamps = BTreeMap::from([(watched.clone(), stamp(10))]);
        sync_for_test(&mut watch, &BTreeSet::from([watched.clone()]), &stamps);
        let folder = watched.parent().expect("a folder").to_path_buf();
        let start = Instant::now();

        stamps.remove(&watched);
        lock(&watch.news).insert(folder.clone(), start);
        assert_eq!(
            watch.due_with(start + WATCH_QUIET, |path| stamps.get(path).copied()),
            vec![FileNews {
                path: watched.clone(),
                present: false,
            }],
            "a file that is not there any more is news, and it says which kind"
        );

        let later = start + Duration::from_secs(10);
        stamps.insert(watched.clone(), stamp(12));
        lock(&watch.news).insert(folder, later);
        assert_eq!(
            watch.due_with(later + WATCH_QUIET, |path| stamps.get(path).copied()),
            vec![present(&watched)],
            "and so is one that comes back"
        );
    }

    /// RED (rule 4) — **a folder this process could not subscribe to is asked
    /// by hand, and a folder it could is not** (user ruling 2026-08-29).
    ///
    /// The second road, and both of its halves in one test because they are one
    /// property seen from two sides. A share, a `\\wsl$` mount or a folder the
    /// process may not open comes back from `subscribe` as `None`; every clock
    /// in this file is then silent about the documents in it, and without this
    /// road they would show the bytes they were opened with until the window
    /// closed. What must be equally true is that the road is **closed** over a
    /// folder that has a handle: asking a watched file here would be the poll
    /// this module's header refuses, and the closure below panics rather than
    /// answering, so "does not read the disk" is a claim and not a hope.
    ///
    /// RED GATE 1: drop the `is_some_and(Option::is_none)` filter in
    /// [`PreviewWatch::ask_the_unwatched_with`] and the watched half panics —
    /// the module has become a poll. RED GATE 2: make the function answer
    /// `Vec::new()` unconditionally and the unwatched half goes red, which on
    /// screen is a document on a share that never refreshes.
    #[test]
    fn a_folder_with_no_handle_is_asked_by_hand_and_a_watched_one_never_is() {
        let mut watch = PreviewWatch::default();
        let share = PathBuf::from(r"\\server\team").join("plan.md");
        let local = file("a.md");
        let mut stamps = BTreeMap::from([(share.clone(), stamp(10)), (local.clone(), stamp(20))]);
        // A `DirWatch` is a kernel object and cannot be fabricated, so the
        // difference the test is about is stated where this module keeps it:
        // every folder arrives unwatchable, and the local one is then recorded
        // as having taken a handle. That is exactly what `sync_with` writes on a
        // real machine, said in one line instead of through a `CreateFileW`.
        let wanted = BTreeSet::from([share.clone(), local.clone()]);
        watch.sync_with(&wanted, |_| None, |path| stamps.get(path).copied());
        let local_folder = local.parent().expect("a folder").to_path_buf();
        watch.unwatchable.remove(&local_folder);

        assert!(
            watch
                .ask_the_unwatched_with(|path| stamps.get(path).copied())
                .is_empty(),
            "nothing has moved, so coming back to the window says nothing"
        );

        // Somebody on the other machine saves it. No notification exists, and no
        // clock is running: the only thing that can find out is this road.
        stamps.insert(share.clone(), stamp(11));
        // And the local file moves too, to prove the road does not carry it —
        // its own kernel does, through `due`.
        stamps.insert(local.clone(), stamp(21));
        assert_eq!(
            watch.ask_the_unwatched_with(|path| stamps.get(path).copied()),
            vec![present(&share)],
            "the unwatchable file answers for itself; the watched one is not asked"
        );
        assert_eq!(
            watch.deadline(),
            None,
            "and asking by hand owes the loop no wake-up — it is a gesture, not a clock"
        );
        assert!(
            watch
                .ask_the_unwatched_with(|path| stamps.get(path).copied())
                .is_empty(),
            "the answer was taken, so coming back again says nothing"
        );

        // The share's file is deleted, and the news says which kind it is.
        stamps.remove(&share);
        assert_eq!(
            watch.ask_the_unwatched_with(|path| stamps.get(path).copied()),
            vec![FileNews {
                path: share,
                present: false,
            }]
        );
    }

    /// PIN (rule 4) — **a window whose folders are all watched reads no disk
    /// when it is given focus.**
    ///
    /// The other half of "this is not a poll", stated where it costs nothing to
    /// check: the ordinary window, on a local disk, with every handle held. The
    /// stamp reader panics.
    #[test]
    fn a_window_whose_folders_are_all_watched_reads_no_disk_on_focus() {
        let mut watch = PreviewWatch::default();
        let watched = file("a.md");
        let stamps = BTreeMap::from([(watched.clone(), stamp(10))]);
        watch.sync_with(
            &BTreeSet::from([watched]),
            |_| None,
            |path| stamps.get(path).copied(),
        );
        // Every handle taken — see the test above for why this is written rather
        // than opened.
        watch.unwatchable.clear();
        assert!(
            watch
                .ask_the_unwatched_with(|path| panic!("asked the disk about {}", path.display()))
                .is_empty()
        );
    }

    /// PIN — **a file nobody touches costs no wake-up and no disk.**
    ///
    /// R31's own sentence read for this watcher: the handle blocks in the
    /// kernel, so "always armed" means one handle and one blocked thread, never
    /// a clock. The stamp reader is handed in as a closure that panics, which is
    /// how "no disk" is a claim rather than a hope.
    #[test]
    fn a_file_nobody_touches_owes_no_wake_up_and_reads_no_disk() {
        let mut watch = PreviewWatch::default();
        let watched = file("a.md");
        let stamps = BTreeMap::from([(watched.clone(), stamp(10))]);
        sync_for_test(&mut watch, &BTreeSet::from([watched]), &stamps);
        assert_eq!(watch.deadline(), None);
        let now = Instant::now();
        assert!(
            watch
                .due_with(now, |path| panic!(
                    "asked the disk about {}",
                    path.display()
                ))
                .is_empty()
        );
        assert!(
            watch
                .due_with(now + Duration::from_secs(600), |path| panic!(
                    "asked the disk about {}",
                    path.display()
                ))
                .is_empty()
        );
        assert_eq!(watch.deadline(), None);
    }
}
