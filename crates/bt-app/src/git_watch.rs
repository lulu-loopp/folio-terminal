//! **The kernel's word that something under a repository moved** — R31's fourth
//! invalidation moment (DESIGN §7.1.3g ②, D, ratified 2026-08-17).
//!
//! # Why this is allowed to exist under a rule that forbids polling
//!
//! R31's sentence is *a repository is not read because time passed*. Everything
//! in this file is downstream of a `ReadDirectoryChangesW` completion: the
//! kernel says something under a working tree changed, and only then does
//! anything here start counting. A window left open over an untouched repository
//! for an hour runs no timer, wakes for nothing and starts no subprocess — which
//! is the property the rule is actually about, and one a poll can never have
//! however long its interval.
//!
//! The clock below is a *debounce fed by events*, and the difference from a poll
//! is not a matter of degree: a poll's question is "has anything changed yet",
//! asked on a schedule the repository has no say in; this one's is "has it
//! stopped changing", asked only because it already did.
//!
//! # The three rules
//!
//! 1. **Gated by R31's own two conditions.** A watch exists while the master
//!    switch is on and some surface on screen — in **any** open window — is
//!    showing that repository's Git page. Leaving the page, switching tabs away,
//!    turning the switch off or closing the window drops the handle once no
//!    window wants it — see [`GitWatch::want`] and `GitWatch::seat_windows_with`,
//!    which keep each window's seat, and `GitWatch::sync`, which owns the
//!    difference for the union.
//! 2. **Coalesced** by [`WatchClock`]: one re-read after the tree goes quiet, and
//!    at most one per [`crate::watch_clock::WATCH_FLOOR`] while it does not.
//! 3. **An overflow is a change.** The kernel's "I stopped keeping track" is
//!    reported by `bt_platform::DirWatch` exactly like every other notification,
//!    because it carries the same information this file uses.
//!
//! # What is deliberately not here
//!
//! **No `.gitignore` matching.** A notification says *something changed*; whether
//! it changed anything git will report is a question about ignore rules,
//! `core.excludesFile`, `.git/info/exclude` and nested `.gitignore`s — and the
//! program that answers it correctly is `git status`, which is the very thing
//! being scheduled. A filter here would be a second, worse implementation of
//! that answer whose failures would look like the panel being wrong.
//!
//! **No parsing of the notification records.** Same reason, one level down: the
//! names are read for nothing, so a rename storm and a single write cost the same
//! thought.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use winit::event_loop::EventLoopProxy;

use crate::{
    AppEvent,
    watch_clock::WatchClock,
    window_news::{SeatedByDirectory, WindowSeats},
};

/// One repository's subscription, and the clock its notifications feed.
struct Watched {
    /// The working tree's own recursive watch, and — for a linked worktree,
    /// whose `.git` is a *file* pointing elsewhere — a second one on the
    /// directory that file names.
    ///
    /// **Empty is a real state and not a failure to retry.** A repository on a
    /// network share or a `\\wsl$` mount cannot be watched; the entry is kept
    /// anyway, holding no handles, so that the attempt is made once per time the
    /// page is opened rather than once per turn of the event loop. Retrying on a
    /// schedule is the poll this whole file exists to avoid.
    watches: Vec<PathWatch>,
    clock: WatchClock,
}

struct PathWatch {
    path: PathBuf,
    watch: bt_platform::DirWatch,
}

/// **Every repository some open window is showing, and which window shows it.**
///
/// The application's (`App::git_watch`): one kernel subscription and one clock
/// per repository, over the union of what every window's seat wants
/// ([`crate::window_news::WindowSeats`]), and the news fanned out to every window
/// whose drawn Git pages stand on that repository. A window says what it wants
/// and takes what it was told on its own turn ([`Self::want`], [`Self::take`]);
/// the clocks are read once a pass for all of them ([`Self::ripen`]).
pub struct GitWatch<W = winit::window::WindowId> {
    /// Where the watcher threads leave their news: the root, and when its most
    /// recent notification arrived.
    ///
    /// A map and not a channel because the only thing worth keeping about ten
    /// notifications is that there were some and when the last one was — which
    /// is exactly what an insert into a map keyed by root does, for free, on the
    /// thread that would otherwise have queued ten messages.
    ///
    /// Stamped on the watcher thread rather than when the loop gets round to
    /// looking, so that a busy main thread cannot make a storm look like a lull.
    news: Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    watched: BTreeMap<PathBuf, Watched>,
    /// Each open window's roots and the ripened news it has not taken yet.
    seats: WindowSeats<W, PathBuf>,
}

impl<W> Default for GitWatch<W> {
    fn default() -> Self {
        Self {
            news: Arc::default(),
            watched: BTreeMap::new(),
            seats: WindowSeats::default(),
        }
    }
}

impl<W: Ord + Copy> GitWatch<W> {
    /// **Seat every window the directory names and release every one it does
    /// not** — the window directory's walk (`FolioApp::publish_window_directory`).
    ///
    /// A released seat takes its roots out of the union, so a repository only a
    /// closed window was showing loses its subscription here, on the walk that
    /// stopped naming the window, rather than whenever another window next
    /// changes what it shows.
    ///
    /// **This walk only ever drops subscriptions.** A seat it makes wants
    /// nothing until its window's own turn says otherwise, so the union can
    /// shrink here and never grow, and no watch is opened.
    ///
    /// Answers whether the subscriptions changed.
    fn seat_windows_with(&mut self, open_windows: impl IntoIterator<Item = W>) -> bool {
        self.seats.level_with(open_windows) && self.sync(|_| Vec::new())
    }

    /// **What this window's drawn Git pages are showing now** (rule 1), asked
    /// on the window's own turn.
    ///
    /// `wanted` is the set of repository roots that some surface in this
    /// window's tab on screen is showing a Git page for. Nothing is opened or
    /// dropped unless this window's set changed, so the turn of a window whose
    /// pages stayed put touches no subscription and no lock.
    ///
    /// A change of the subscriptions is written to `BT_GIT_TRACE` by
    /// `Self::sync`; nothing else is said about it.
    ///
    /// **The proxy is borrowed here and cloned only where a watch is actually
    /// opened**, which on one of the two platforms is the difference between an
    /// idle window and a burning processor. A clone of an `EventLoopProxy` is an
    /// `Arc` bump on Windows and *not* one on macOS: winit builds a fresh run
    /// loop source, adds it to the main run loop and **wakes the loop**, and
    /// dropping it invalidates the source again. This is called on every turn,
    /// so a clone taken at the top of it would schedule the very turn that takes
    /// the next one — a window nobody is looking at, holding a core at full tilt
    /// for as long as it stays open. So the clone is taken once per
    /// subscription, in [`subscribe`], and never per turn.
    pub fn want(&mut self, window: W, wanted: BTreeSet<PathBuf>, proxy: &EventLoopProxy<AppEvent>) {
        let news = Arc::clone(&self.news);
        self.want_with(window, wanted, |root| subscribe(&news, proxy, root));
    }

    /// [`Self::want`] with the opening of a watch handed in.
    fn want_with(
        &mut self,
        window: W,
        wanted: BTreeSet<PathBuf>,
        open: impl FnMut(&Path) -> Vec<PathWatch>,
    ) {
        if self.seats.want(window, wanted) {
            self.sync(open);
        }
    }

    /// **Fold in what the kernel has said, and tell every window that wants a
    /// repository whose news has ripened** — once a pass, before any window
    /// takes its turn, so every window takes the same news on the same pass.
    pub fn ripen(&mut self, now: Instant) {
        for root in self.due(now) {
            self.seats.tell(&root);
        }
    }

    /// The repositories this window has been told moved and has not yet read
    /// again, handed over once.
    pub fn take(&mut self, window: W) -> BTreeSet<PathBuf> {
        self.seats.take(window)
    }

    /// **Bring the subscriptions level with what the seats want** (rule 1).
    ///
    /// The union of every seat is the whole of the gate: a root that leaves it
    /// has its subscriptions dropped here, which cancels the read and leaves
    /// retirement to the platform. Nothing else in this file decides to watch or
    /// stop watching anything.
    fn sync(&mut self, open: impl FnMut(&Path) -> Vec<PathWatch>) -> bool {
        let wanted = self.seats.wanted();
        let changed = self.sync_with(&wanted, open);
        if changed {
            let (held, watching) = self.counts();
            trace(&format!(
                "{watching} of {held} repositories on screen are being watched"
            ));
        }
        changed
    }

    /// [`Self::sync`]'s bookkeeping over a given set.
    ///
    /// One derivation for the real thing and for the tests: what the gate *is* —
    /// the map follows the set, departures drop their handles, arrivals are
    /// opened once — is the same code whether a kernel subscription is actually
    /// taken out or not. A second copy of it in the tests would be a test of the
    /// copy.
    fn sync_with(
        &mut self,
        wanted: &BTreeSet<PathBuf>,
        mut open: impl FnMut(&Path) -> Vec<PathWatch>,
    ) -> bool {
        // **With no Git page open in any window nothing is touched at all**, not
        // even the mailbox's lock. The mailbox can only hold news for a
        // repository something is watching, so with nothing watched and nothing
        // wanted there is provably nothing to reconcile.
        if wanted.is_empty() && self.watched.is_empty() {
            return false;
        }
        let before = self.watched.len();
        // Departures first; dropping a subscription is the cancellation.
        self.watched.retain(|root, _| wanted.contains(root));
        let mut changed = self.watched.len() != before;
        for root in wanted {
            if self.watched.contains_key(root) {
                continue;
            }
            changed = true;
            self.watched.insert(
                root.clone(),
                Watched {
                    watches: open(root),
                    clock: WatchClock::default(),
                },
            );
        }
        // A root that stopped being watched left its unread news behind, and a
        // stale timestamp for it would be acted on the next time somebody opened
        // its page — after a fresh reading that already answered it.
        lock(&self.news).retain(|root, _| wanted.contains(root));
        changed
    }

    /// **Which repositories are due to be read again**, given everything the
    /// kernel has said since the last time this was asked.
    ///
    /// Folding the news in and answering are one step because they are one
    /// question: a notification that arrived a moment ago may or may not have
    /// made its repository due, and the only way to find out is to give it to
    /// the clock first.
    fn due(&mut self, now: Instant) -> Vec<PathBuf> {
        if self.watched.is_empty() {
            return Vec::new();
        }
        for entry in self.watched.values_mut() {
            entry.watches.retain_mut(|subscription| {
                let Some(error) = subscription.watch.take_failure() else {
                    return true;
                };
                trace(&format!(
                    "cannot watch {}: {error}",
                    subscription.path.display()
                ));
                false
            });
        }
        for (root, at) in std::mem::take(&mut *lock(&self.news)) {
            if let Some(entry) = self.watched.get_mut(&root) {
                entry.clock.note_event(at);
            }
        }
        self.watched
            .iter_mut()
            .filter_map(|(root, entry)| entry.clock.take_due(now).then(|| root.clone()))
            .collect()
    }

    /// When the loop must wake to answer the news it is already holding.
    ///
    /// `None` while nothing is owed, which is the ordinary state of a window
    /// looking at a repository nobody is writing to — and the reason this
    /// mechanism costs no wake-ups at all when nothing is happening.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.watched
            .values()
            .filter_map(|entry| entry.clock.due_at())
            .min()
    }

    /// How many repositories are subscribed to, and how many of those the
    /// platform could actually open a watch on. For the pins and the trace line.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (
            self.watched.len(),
            self.watched
                .values()
                .filter(|entry| {
                    entry
                        .watches
                        .iter()
                        .any(|subscription| subscription.watch.is_armed())
                })
                .count(),
        )
    }
}

/// The window directory's walk (`FolioApp::publish_window_directory`, through
/// [`crate::window_news::seat_every_holder`]): [`GitWatch::seat_windows_with`].
impl<W: Ord + Copy> SeatedByDirectory<W> for GitWatch<W> {
    fn seat_windows(&mut self, open: &[W]) {
        self.seat_windows_with(open.iter().copied());
    }
}

/// Open the one or two watches a repository needs.
///
/// A free function and not a method because it is called from inside a closure
/// that [`GitWatch::sync_with`] holds while it holds `self` mutably; what it
/// needs is the mailbox and the proxy, not the registry.
///
/// **And it is the only place in this file the proxy is cloned** — once per
/// watch opened, for the reason [`GitWatch::want`] states.
fn subscribe(
    news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    proxy: &EventLoopProxy<AppEvent>,
    root: &Path,
) -> Vec<PathWatch> {
    // The working tree, recursively — which already covers `.git` in the ordinary
    // case, because there it is a subdirectory of exactly this tree.
    let mut paths = vec![root.to_path_buf()];
    // A linked worktree's `.git` is a *file* naming a directory somewhere else,
    // and that directory is where its `HEAD` and its index live. Without this
    // second watch, a commit made in another tool inside a worktree would move
    // nothing the first watch can see.
    if let Some(gitdir) = linked_gitdir(root) {
        paths.push(gitdir);
    }
    paths
        .into_iter()
        .filter_map(|path| {
            let news = Arc::clone(news);
            let proxy = proxy.clone();
            let root = root.to_path_buf();
            let started = bt_platform::DirWatch::start(&path, move || {
                lock(&news).insert(root.clone(), Instant::now());
                // The loop is woken, not told what to do: what a change means is
                // decided on the main thread, where the clocks are.
                let _ = proxy.send_event(AppEvent::GitChanged);
            });
            match started {
                Ok(watch) => Some(PathWatch { path, watch }),
                Err(error) => {
                    // **Quietly** (rule 3). A network share, a `\\wsl$` mount, a
                    // folder this process may not open: the answer is to have no
                    // watcher and let the window-focus trigger and the page's own
                    // refresh cover it. There is nothing here a reader could act
                    // on, so nothing is raised — only a line for whoever is
                    // holding the door open.
                    trace(&format!("cannot watch {}: {error}", path.display()));
                    None
                }
            }
        })
        .collect()
}

/// The directory a linked worktree's `.git` file points at, if this root is one.
///
/// `.git` is a directory in an ordinary clone and a one-line file in a linked
/// worktree or a submodule: `gitdir: <path>`, where the path may be relative to
/// the tree. Returning `None` is the ordinary answer and means "the recursive
/// watch on the tree already covers it".
#[must_use]
pub fn linked_gitdir(root: &Path) -> Option<PathBuf> {
    let marker = root.join(".git");
    if !marker.is_file() {
        return None;
    }
    let text =
        bt_platform::file_reads::read_to_string(bt_platform::file_reads::Lane::Settings, &marker)
            .ok()?;
    let named = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    if named.is_empty() {
        return None;
    }
    let path = Path::new(named);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    resolved.is_dir().then_some(resolved)
}

/// A mutex this crate never poisons on purpose, unwrapped without a panic path.
///
/// The only code inside these locks is an insert and a drain. A poisoned lock
/// here would mean a watcher thread panicked mid-insert, and the useful response
/// is to carry on with the map as it stands rather than to take the window down
/// over a repository somebody stopped looking at.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The door for whoever is debugging a watch that did not fire.
///
/// `BT_GIT_TRACE` and not a toast: a repository that cannot be watched is not
/// something a reader can do anything about, and the page still has its refresh
/// button and still re-reads when the window comes back. Set-but-empty is off,
/// on `BT_PERF_TRACE`'s own rule.
fn trace(message: &str) {
    if std::env::var_os("BT_GIT_TRACE").is_some_and(|value| !value.is_empty()) {
        eprintln!("git watch: {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::watch_clock::{WATCH_FLOOR, WATCH_QUIET};

    /// PIN (D, rule 2) — **a burst is one reading, and a storm is one reading
    /// every two seconds.**
    ///
    /// The three shapes this clock has to get right, written as times so that
    /// each is a claim and not a feeling:
    ///
    /// - one `git add` (two notifications a millisecond apart) → one reading,
    ///   three hundred milliseconds after the second of them;
    /// - a `cargo build` (notifications without a gap for seconds) → a reading
    ///   every two seconds, not one every three hundred milliseconds and not
    ///   silence until the build ends;
    /// - nothing at all → no deadline, no wake-up, no reading.
    #[test]
    fn a_burst_is_one_reading_and_a_storm_is_one_every_two_seconds() {
        let start = Instant::now();
        let mut clock = WatchClock::default();

        assert_eq!(clock.due_at(), None, "silence owes nothing");
        assert!(!clock.take_due(start), "and nothing fires");

        // One command: two notifications a millisecond apart.
        clock.note_event(start);
        clock.note_event(start + Duration::from_millis(1));
        assert_eq!(
            clock.due_at(),
            Some(start + Duration::from_millis(1) + WATCH_QUIET),
            "the quiet window runs from the last of them, not the first"
        );
        assert!(
            !clock.take_due(start + Duration::from_millis(200)),
            "and it is not due before it has elapsed"
        );
        assert!(clock.take_due(start + Duration::from_millis(301)));
        assert_eq!(
            clock.due_at(),
            None,
            "taking it clears the news: one command, one reading"
        );

        // A storm: a notification every fifty milliseconds for six seconds. The
        // clock is asked at every one of them, exactly as the event loop does.
        let mut readings = Vec::new();
        let storm_from = start + Duration::from_secs(10);
        let mut at = storm_from;
        while at <= storm_from + Duration::from_secs(6) {
            clock.note_event(at);
            if clock.take_due(at) {
                readings.push(at - storm_from);
            }
            at += Duration::from_millis(50);
        }
        // Two readings in six seconds of unbroken writing: the first at the cap,
        // the second a floor-and-one-sample later. The fifty milliseconds are the
        // storm's own granularity — the floor says "not before four seconds", and
        // four seconds falls between two notifications, so the reading happens at
        // the next one. A third would be due at 6.05s, after this storm stops.
        assert_eq!(
            readings,
            vec![Duration::from_secs(2), Duration::from_millis(4050)],
            "one reading every two seconds through the storm — the tree never \
             goes quiet, so the cap is what fires, and the floor is what spaces \
             them"
        );
        for pair in readings.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                gap >= WATCH_FLOOR && gap < WATCH_FLOOR + Duration::from_millis(100),
                "and no two are closer together than the floor: {gap:?}"
            );
        }

        // And the tail: the storm stops, and its last news is answered once.
        // Which of the three terms decides it is the interesting part — the news
        // began at 4.10s, one sample after the previous reading, so the *cap*
        // (`first_pending + FLOOR`) comes due at 6.10s, ahead of both the quiet
        // window and the floor. A reader who stops a build sees the tree it left
        // behind a tenth of a second later, and does not wait out either clock.
        let quiet_at = at;
        clock.note_event(quiet_at);
        let due = clock.due_at().expect("the last notification is still owed");
        assert_eq!(due, storm_from + Duration::from_millis(6100));
        assert!(
            due <= quiet_at + WATCH_QUIET,
            "never later than a quiet window after the last thing that happened"
        );
        assert!(
            due >= storm_from + Duration::from_millis(4050) + WATCH_FLOOR,
            "and never sooner than a floor after the previous reading"
        );
        assert!(!clock.take_due(due - Duration::from_millis(1)));
        assert!(clock.take_due(due));
        assert_eq!(clock.due_at(), None, "and then it is quiet again");
    }

    /// PIN (D, rule 1) — **a watch is held only while a page is showing it, and
    /// dropped the moment it is not.**
    ///
    /// The gate is the set handed to [`GitWatch::sync_with`] and nothing else. This
    /// checks the bookkeeping half of it — that the map follows the set exactly,
    /// including the case that matters most for R31: an empty set holds nothing
    /// at all, so a window with no Git page open has no subscription open either.
    ///
    /// It runs without an event loop, so no watch is ever really started: the
    /// entries are the record of the attempt, which is the thing the gate is
    /// about.
    #[test]
    fn the_subscriptions_follow_the_pages_that_are_showing() {
        let mut watch = GitWatch::<u32>::default();
        let a = PathBuf::from(r"D:\repo");
        let b = PathBuf::from(r"D:\other");

        assert!(
            !sync_for_test(&mut watch, &BTreeSet::new()),
            "no pages and no watches is nothing to reconcile"
        );
        assert_eq!(watch.counts().0, 0, "nothing showing, nothing watched");

        assert!(sync_for_test(&mut watch, &BTreeSet::from([a.clone()])));
        assert_eq!(watch.counts().0, 1);
        assert!(
            !sync_for_test(&mut watch, &BTreeSet::from([a.clone()])),
            "the same set again is not a change and re-opens nothing"
        );

        assert!(sync_for_test(
            &mut watch,
            &BTreeSet::from([a.clone(), b.clone()])
        ));
        assert_eq!(watch.counts().0, 2);

        // The page is left, or the tab is switched away from, or the master
        // switch goes off: the handle goes with it.
        assert!(sync_for_test(&mut watch, &BTreeSet::from([b.clone()])));
        assert_eq!(watch.counts().0, 1);
        assert!(sync_for_test(&mut watch, &BTreeSet::new()));
        assert_eq!(watch.counts().0, 0);
        assert_eq!(watch.deadline(), None, "and nothing is owed by nobody");
    }

    /// PIN (D) — **news for a repository nobody is watching any more is dropped,
    /// not banked.**
    ///
    /// A notification that arrived while a page was closing must not be waiting
    /// to fire the next time that page is opened: what it was about happened
    /// before this reading of the repository, so the fresh read that opening the
    /// page already performs has answered it.
    #[test]
    fn news_for_a_page_that_closed_is_not_kept_for_the_next_one() {
        let mut watch = GitWatch::<u32>::default();
        let root = PathBuf::from(r"D:\repo");
        let now = Instant::now();

        sync_for_test(&mut watch, &BTreeSet::from([root.clone()]));
        lock(&watch.news).insert(root.clone(), now);
        sync_for_test(&mut watch, &BTreeSet::new());
        assert!(lock(&watch.news).is_empty(), "the stale news went with it");

        sync_for_test(&mut watch, &BTreeSet::from([root.clone()]));
        assert!(
            watch.due(now + Duration::from_secs(60)).is_empty(),
            "a page opened again is not owed a reading by something that happened \
             before it opened"
        );
    }

    /// PIN (D, rule 1) — **a linked worktree's `.git` is a file, and the
    /// directory it names is watched too.**
    ///
    /// The recursive watch on a working tree covers `.git` in an ordinary clone,
    /// because there it *is* a subdirectory of that tree. In a linked worktree —
    /// and in a submodule — it is a one-line file pointing somewhere else
    /// entirely, and that somewhere else is where `HEAD` and the index live. A
    /// commit made in another tool inside such a tree moves files the first watch
    /// can see (the checkout) but the ones that say a commit happened are the
    /// ones it cannot.
    ///
    /// A real directory rather than a fake filesystem: the answer turns on
    /// `is_file` and `is_dir`, which is a question about a disk.
    #[test]
    fn a_linked_worktrees_gitdir_is_resolved_and_an_ordinary_clones_is_not() {
        let base = std::env::temp_dir().join(format!(
            "bt-git-watch-worktree-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let tree = base.join("tree");
        let elsewhere = base.join("main").join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&tree).expect("a working tree");
        std::fs::create_dir_all(&elsewhere).expect("somewhere for it to point");

        // An ordinary clone: `.git` is a directory, and the recursive watch on
        // the tree already covers it. Nothing extra is opened.
        std::fs::create_dir_all(tree.join(".git")).expect("an ordinary .git");
        assert_eq!(linked_gitdir(&tree), None);
        std::fs::remove_dir_all(tree.join(".git")).expect("undo it");

        // A linked worktree: an absolute path, which is what `git worktree add`
        // actually writes.
        std::fs::write(
            tree.join(".git"),
            format!("gitdir: {}\n", elsewhere.display()),
        )
        .expect("write the pointer file");
        assert_eq!(linked_gitdir(&tree).as_deref(), Some(elsewhere.as_path()));

        // And a relative one, which a submodule's may be. It is resolved against
        // the tree, not against whatever the process's current directory happens
        // to be — this window never sets one and every other reader of it would
        // be somewhere else.
        std::fs::write(tree.join(".git"), "gitdir: ../main/.git/worktrees/wt")
            .expect("write a relative pointer");
        assert_eq!(
            linked_gitdir(&tree),
            Some(tree.join("../main/.git/worktrees/wt")),
            "resolved against the tree that names it"
        );

        // A pointer at something that is not there is not a second watch: it is
        // one fewer thing to open, and the tree's own watch is still held.
        std::fs::write(tree.join(".git"), "gitdir: ../nowhere-at-all").expect("write a dud");
        assert_eq!(linked_gitdir(&tree), None);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// PIN (D) — **the clock only fires for a repository the kernel spoke about.**
    #[test]
    fn only_the_repository_that_moved_is_read_again() {
        let mut watch = GitWatch::<u32>::default();
        let moved = PathBuf::from(r"D:\repo");
        let still = PathBuf::from(r"D:\other");
        let now = Instant::now();
        sync_for_test(&mut watch, &BTreeSet::from([moved.clone(), still]));

        lock(&watch.news).insert(moved.clone(), now);
        assert!(
            watch.due(now).is_empty(),
            "not yet: the tree has not been quiet for long enough"
        );
        assert_eq!(
            watch.due(now + WATCH_QUIET),
            vec![moved],
            "and then exactly the one that moved"
        );
    }

    /// [`GitWatch::sync_with`] with no kernel behind it — the gate's bookkeeping on
    /// its own.
    ///
    /// The subscription itself is proved in `bt_platform`'s own
    /// `dir_watch_tests`, against a real directory, which is the only place it
    /// can be proved. What these tests are about is the half that decides *when*
    /// one is held, and that half is [`GitWatch::sync_with`] itself rather than a
    /// second copy of it here.
    fn sync_for_test(watch: &mut GitWatch<u32>, wanted: &BTreeSet<PathBuf>) -> bool {
        watch.sync_with(wanted, |_| Vec::new())
    }

    // ── T-WINDOWS-ALL: one seat per window ─────────────────────────────────
    //
    // What a window's turn does, with no window: its surfaces as
    // `Runtime::git_surfaces_on_screen` lists them (the active tab's, each with
    // whether the frame drew it), the roots it wants (`git_roots_on_glass`), the
    // news it takes, and the surfaces that news re-reads
    // (`git_surfaces_the_kernel_moved`). The kernel's word is put in the mailbox
    // by hand, as the tests above do; the subscription itself is
    // `bt_platform`'s to prove.

    use crate::{GitOrigin, SeatId, git_roots_on_glass, git_surfaces_the_kernel_moved};

    type Surfaces = Vec<(GitOrigin, PathBuf, bool)>;

    /// One window's turn: say what it draws, take what it was told, and answer
    /// which of its surfaces are read again.
    fn turn(watch: &mut GitWatch<u32>, window: u32, surfaces: &Surfaces) -> Vec<GitOrigin> {
        watch.want_with(window, git_roots_on_glass(surfaces), |_| Vec::new());
        git_surfaces_the_kernel_moved(surfaces, &watch.take(window))
    }

    /// The kernel spoke about `root`, and the tree has gone quiet since.
    fn the_disk_moved(watch: &mut GitWatch<u32>, root: &Path, at: Instant) {
        lock(&watch.news).insert(root.to_path_buf(), at);
        watch.ripen(at + crate::watch_clock::WATCH_QUIET);
    }

    /// RED (T-WINDOWS-ALL) — **two windows showing one repository both read it
    /// again when it changes on disk.**
    ///
    /// The census's defect: the subscription, the clock and the reading of the
    /// surfaces lived on the window that turns the application's clocks, so a
    /// second window's Git page never heard the disk.
    ///
    /// MUTATION: fan out to the first window only (`window_news::WindowSeats::tell`
    /// stopping after the first seat that wants the root) — window 2 takes no
    /// news and its column and graph are not read again.
    #[test]
    fn two_windows_showing_one_repository_both_read_it_again() {
        let mut watch = GitWatch::<u32>::default();
        let repo = PathBuf::from(r"D:\仓库\folio");
        let first: Surfaces = vec![(GitOrigin::Column(SeatId(1)), repo.clone(), true)];
        let second: Surfaces = vec![
            (GitOrigin::Column(SeatId(1)), repo.clone(), true),
            (GitOrigin::Graph(repo.clone()), repo.clone(), true),
        ];
        assert!(!watch.seat_windows_with([1, 2]));
        assert!(turn(&mut watch, 1, &first).is_empty());
        assert!(turn(&mut watch, 2, &second).is_empty());
        assert_eq!(watch.counts().0, 1, "one subscription for the union");

        the_disk_moved(&mut watch, &repo, Instant::now());
        assert_eq!(
            turn(&mut watch, 1, &first),
            vec![GitOrigin::Column(SeatId(1))]
        );
        assert_eq!(
            turn(&mut watch, 2, &second),
            vec![GitOrigin::Column(SeatId(1)), GitOrigin::Graph(repo.clone())],
            "the second window's column and graph are read again too"
        );
        assert!(
            turn(&mut watch, 2, &second).is_empty(),
            "once: the next turn has nothing new"
        );
    }

    /// RED (T-WINDOWS-ALL) — **a repository only the second window shows is
    /// subscribed to and read again.**
    ///
    /// The other half of the defect: the first window's turn handed the watch
    /// its own set alone, which dropped every root another window wanted.
    ///
    /// MUTATION: the subscriptions following one window's set rather than the
    /// union (`GitWatch::sync` handed the asking window's roots) — the second
    /// window's repository is unsubscribed on the first window's turn and its
    /// news is dropped with it.
    #[test]
    fn a_repository_only_the_second_window_shows_is_watched() {
        let mut watch = GitWatch::<u32>::default();
        let mine = PathBuf::from(r"D:\work\one");
        let theirs = PathBuf::from(r"D:\工作\two");
        let first: Surfaces = vec![(GitOrigin::Column(SeatId(1)), mine.clone(), true)];
        let second: Surfaces = vec![(GitOrigin::Column(SeatId(3)), theirs.clone(), true)];
        watch.seat_windows_with([1, 2]);
        turn(&mut watch, 2, &second);
        turn(&mut watch, 1, &first);
        assert_eq!(watch.counts().0, 2, "both windows' repositories");

        the_disk_moved(&mut watch, &theirs, Instant::now());
        assert!(turn(&mut watch, 1, &first).is_empty());
        assert_eq!(
            turn(&mut watch, 2, &second),
            vec![GitOrigin::Column(SeatId(3))]
        );
    }

    /// RED (T-WINDOWS-ALL) — **a closed window's seat is released by the
    /// directory walk, with every subscription only it wanted.**
    ///
    /// MUTATION: leak it (`window_news::WindowSeats::level_with` without its `retain`)
    /// — the news still reaches the dropped window's seat, and the repository
    /// only it showed is still subscribed to.
    #[test]
    fn a_closed_windows_seat_and_subscriptions_are_released() {
        let mut watch = GitWatch::<u32>::default();
        let shared = PathBuf::from(r"D:\repo");
        let alone = PathBuf::from(r"D:\只在二号窗");
        let first: Surfaces = vec![(GitOrigin::Column(SeatId(1)), shared.clone(), true)];
        let second: Surfaces = vec![
            (GitOrigin::Column(SeatId(1)), shared.clone(), true),
            (GitOrigin::Column(SeatId(2)), alone.clone(), true),
        ];
        watch.seat_windows_with([1, 2]);
        turn(&mut watch, 1, &first);
        turn(&mut watch, 2, &second);
        assert_eq!(watch.counts().0, 2);

        // Window 2 closes: the next walk of the directory no longer names it.
        assert!(watch.seat_windows_with([1]), "the subscriptions changed");
        assert_eq!(
            watch.counts().0,
            1,
            "the repository only the closed window showed is not watched"
        );
        let now = Instant::now();
        lock(&watch.news).insert(shared.clone(), now);
        watch.ripen(now + crate::watch_clock::WATCH_QUIET);
        assert!(
            watch.take(2).is_empty(),
            "a window the directory dropped is told nothing"
        );
        assert_eq!(
            turn(&mut watch, 1, &first),
            vec![GitOrigin::Column(SeatId(1))]
        );
    }

    /// RED (T-WINDOWS-ALL) — **the drawn rule holds per window: a surface that
    /// is not on the glass wants nothing and is read again for nothing.**
    ///
    /// Window 2's column is on its Files page (or its tab is not on screen, which
    /// `git_surfaces_on_screen` never lists at all): it is not a surface looking
    /// at the repository, so window 2's seat stays empty and the news that
    /// re-reads window 1's page costs window 2 no process.
    ///
    /// MUTATIONS: `git_roots_on_glass` without its `showing` filter — window 2
    /// wants, and would be told about, a repository it does not draw; and
    /// `git_surfaces_the_kernel_moved` without its own — news about the root
    /// re-reads the undrawn column.
    #[test]
    fn a_surface_that_is_not_drawn_is_told_nothing() {
        let mut watch = GitWatch::<u32>::default();
        let repo = PathBuf::from(r"D:\仓库");
        let drawn: Surfaces = vec![(GitOrigin::Column(SeatId(1)), repo.clone(), true)];
        let undrawn: Surfaces = vec![(GitOrigin::Column(SeatId(1)), repo.clone(), false)];
        watch.seat_windows_with([1, 2]);
        turn(&mut watch, 1, &drawn);
        turn(&mut watch, 2, &undrawn);
        assert_eq!(
            git_roots_on_glass(&undrawn),
            BTreeSet::new(),
            "an undrawn page wants no news"
        );

        the_disk_moved(&mut watch, &repo, Instant::now());
        assert!(
            watch.take(2).is_empty(),
            "window 2 does not draw the page, so it is not told"
        );
        assert!(
            git_surfaces_the_kernel_moved(&undrawn, &BTreeSet::from([repo.clone()])).is_empty(),
            "and news about the root does not re-read a surface that is not drawn"
        );
        assert_eq!(
            turn(&mut watch, 1, &drawn),
            vec![GitOrigin::Column(SeatId(1))]
        );
    }

    /// RED (round 2) — **the first window re-reads at exactly the moments it did
    /// before the seats**: the same re-reads for the same news, against a
    /// recorded sequence.
    ///
    /// The sequence is what the first-window-only watch did on each turn (sync
    /// to the window's drawn roots, fold the news, re-read the drawn surfaces on
    /// each ripe root): nothing before the tree is quiet, one re-read per
    /// ripened root and never a second for the same news, every drawn surface on
    /// the root together, nothing for a root no page shows, and nothing banked
    /// for a page that was left — whether it was left before the news ripened or
    /// on the very pass it did. A second window showing another repository is
    /// seated beside it and changes none of it.
    ///
    /// MUTATIONS: news filed twice (`WindowSeats::take` handing over a copy and
    /// keeping the news) — the first window re-reads again on the next pass; the
    /// first seat skipped (`WindowSeats::tell` over `.skip(1)`) — the first
    /// window never re-reads.
    #[test]
    fn the_first_window_re_reads_at_the_moments_it_always_did() {
        let mut watch = GitWatch::<u32>::default();
        let a = PathBuf::from(r"D:\仓库\a");
        let b = PathBuf::from(r"D:\other\b");
        let c = PathBuf::from(r"D:\第二窗\c");
        let column = GitOrigin::Column(SeatId(1));
        let graph = GitOrigin::Graph(a.clone());
        let page: Surfaces = vec![
            (column.clone(), a.clone(), true),
            (GitOrigin::Column(SeatId(2)), b.clone(), false),
        ];
        let page_and_graph: Surfaces = vec![
            (column.clone(), a.clone(), true),
            (graph.clone(), a.clone(), true),
        ];
        let left: Surfaces = vec![(column.clone(), a.clone(), false)];
        let second: Surfaces = vec![(GitOrigin::Column(SeatId(1)), c, true)];
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let quiet = crate::watch_clock::WATCH_QUIET;
        watch.seat_windows_with([1, 2]);

        // One pass of the loop: ripen, then the windows' turns in opening order.
        let pass = |watch: &mut GitWatch<u32>, now: Instant, first: &Surfaces| {
            watch.ripen(now);
            let read = turn(watch, 1, first);
            assert!(
                turn(watch, 2, &second).is_empty(),
                "nothing moved under window 2"
            );
            read
        };
        let heard = |watch: &mut GitWatch<u32>, root: &Path, when: Instant| {
            lock(&watch.news).insert(root.to_path_buf(), when);
        };

        let mut recorded: Vec<Vec<GitOrigin>> = Vec::new();
        recorded.push(pass(&mut watch, at(0), &page));
        heard(&mut watch, &a, at(100));
        recorded.push(pass(&mut watch, at(200), &page));
        recorded.push(pass(&mut watch, at(100) + quiet, &page));
        recorded.push(pass(&mut watch, at(500), &page));
        heard(&mut watch, &b, at(3000));
        recorded.push(pass(&mut watch, at(4000), &page));
        recorded.push(pass(&mut watch, at(5000), &page_and_graph));
        heard(&mut watch, &a, at(5100));
        recorded.push(pass(&mut watch, at(5100) + quiet, &page_and_graph));
        heard(&mut watch, &a, at(8000));
        recorded.push(pass(&mut watch, at(8100), &left));
        recorded.push(pass(&mut watch, at(9000), &left));
        recorded.push(pass(&mut watch, at(10_000), &page));
        heard(&mut watch, &a, at(13_000));
        recorded.push(pass(&mut watch, at(13_000) + quiet, &left));
        recorded.push(pass(&mut watch, at(14_000), &left));

        assert_eq!(
            recorded,
            vec![
                vec![],                      // the page opens: its own first read, not news
                vec![],                      // news, the tree not yet quiet
                vec![column.clone()],        // quiet: one re-read
                vec![],                      // and never a second for it
                vec![],                      // news for a root no page draws
                vec![],                      // a graph joins the page
                vec![column.clone(), graph], // both surfaces on the root, together
                vec![],                      // the page is left before the news ripens
                vec![],                      // and the news is not kept
                vec![],                      // back on the page: nothing banked
                vec![],                      // left on the very pass the news ripened
                vec![],                      // and still nothing
            ]
        );
    }
}
