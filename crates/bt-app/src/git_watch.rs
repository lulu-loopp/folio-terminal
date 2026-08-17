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
//!    switch is on and some surface on screen is showing that repository's Git
//!    page. Leaving the page, switching tabs away, or turning the switch off
//!    drops the handle — see [`GitWatch::sync`], which is handed the wanted set
//!    and owns the difference.
//! 2. **Coalesced** by [`WatchClock`]: one re-read after the tree goes quiet, and
//!    at most one per [`GIT_WATCH_FLOOR`] while it does not.
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
    time::{Duration, Instant},
};

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// How long a tree has to hold still before its news is acted on.
///
/// A single `git add` is one or two notifications inside a millisecond of each
/// other; a `git commit` is a few dozen. Three hundred milliseconds is long
/// enough that all of them arrive as one piece of news and short enough that a
/// reader who typed the command has not looked away from the panel yet.
pub const GIT_WATCH_QUIET: Duration = Duration::from_millis(300);

/// And the shortest interval between two re-reads of one repository, however
/// much is happening.
///
/// **This is what a `cargo build` costs.** A build writes for thirty seconds
/// without a three-hundred-millisecond gap anywhere in it, so the quiet window
/// alone would either say nothing for thirty seconds or — with no floor —
/// nothing at all until it ended. Two seconds is the compromise the ruling
/// names: the page keeps up with a build in progress, and the repository is
/// asked no more often than a person could read the answer.
pub const GIT_WATCH_FLOOR: Duration = Duration::from_secs(2);

/// **The debounce, as arithmetic** — one repository's clock.
///
/// Pure and separate from everything that owns a handle, because the interesting
/// claims about this mechanism are all claims about *times*: that a burst
/// becomes one re-read, that a storm becomes one every two seconds, and that
/// silence becomes nothing. A version of this living inside the watcher thread
/// would only be checkable by writing files and waiting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatchClock {
    /// When the first notification since the last re-read arrived — the start of
    /// the news that is currently owed. `None` when nothing is owed, which is
    /// the whole of "this clock is not running".
    first_pending: Option<Instant>,
    /// And the most recent one, which is what the quiet window is measured from.
    last_event: Option<Instant>,
    /// When this repository was last re-read *because of this clock*, and the
    /// floor's own anchor.
    last_reread: Option<Instant>,
}

impl WatchClock {
    /// The kernel said something moved.
    pub fn note_event(&mut self, at: Instant) {
        self.first_pending.get_or_insert(at);
        self.last_event = Some(match self.last_event {
            Some(last) if last > at => last,
            _ => at,
        });
    }

    /// When the re-read this clock owes becomes due, if it owes one.
    ///
    /// Three terms, and each is one of the ruling's three sentences:
    ///
    /// - **quiet**: `last_event + QUIET` — wait for the tree to stop moving.
    /// - **cap**: `first_pending + FLOOR` — but never wait longer than the floor
    ///   after the news *started*, or a build that never goes quiet would keep
    ///   the panel silent for its whole duration.
    /// - **floor**: `last_reread + FLOOR` — and never sooner than that after the
    ///   last one, which is what makes a storm cost one reading every two
    ///   seconds instead of one every three hundred milliseconds.
    #[must_use]
    pub fn due_at(&self) -> Option<Instant> {
        let first = self.first_pending?;
        let last = self.last_event?;
        let natural = (last + GIT_WATCH_QUIET).min(first + GIT_WATCH_FLOOR);
        Some(match self.last_reread {
            Some(previous) => natural.max(previous + GIT_WATCH_FLOOR),
            None => natural,
        })
    }

    /// Is it due now, and if so, take it.
    ///
    /// Taking is what clears the news: after this the clock owes nothing until
    /// the kernel speaks again, which is what makes "nothing changed, nothing
    /// fires" a property of the type rather than of its caller.
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.due_at().is_none_or(|due| due > now) {
            return false;
        }
        self.first_pending = None;
        self.last_event = None;
        self.last_reread = Some(now);
        true
    }
}

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
    watches: Vec<bt_platform::DirWatch>,
    clock: WatchClock,
}

/// **Every repository this window is currently subscribed to.**
#[derive(Default)]
pub struct GitWatch {
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
}

impl GitWatch {
    /// **Bring the subscriptions level with what is on screen** (rule 1).
    ///
    /// `wanted` is the set of repository roots that some surface in the tab on
    /// screen is showing a Git page for, and it is the whole of the gate: a root
    /// that leaves it has its handles dropped here, which cancels the read and
    /// joins the thread. Nothing else in this file ever decides to watch or stop
    /// watching anything.
    ///
    /// Answers whether the set changed, which is only of interest to a
    /// diagnostics line.
    pub fn sync(&mut self, wanted: &BTreeSet<PathBuf>, proxy: &EventLoopProxy<AppEvent>) -> bool {
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        let changed = self.sync_with(wanted, move |root| subscribe(&news, &proxy, root));
        if changed {
            let (held, watching) = self.counts();
            trace(&format!(
                "{watching} of {held} repositories on screen are being watched"
            ));
        }
        changed
    }

    /// [`Self::sync`]'s bookkeeping, with the opening of a watch handed in.
    ///
    /// One derivation for the real thing and for the tests: what the gate *is* —
    /// the map follows the set, departures drop their handles, arrivals are
    /// opened once — is the same code whether a kernel subscription is actually
    /// taken out or not. A second copy of it in the tests would be a test of the
    /// copy.
    fn sync_with(
        &mut self,
        wanted: &BTreeSet<PathBuf>,
        mut open: impl FnMut(&Path) -> Vec<bt_platform::DirWatch>,
    ) -> bool {
        // **A window with no Git page open touches nothing at all**, not even the
        // mailbox's lock. This is asked on every turn of the event loop — every
        // mouse move included — and the mailbox can only hold news for a
        // repository something is watching, so with nothing watched and nothing
        // wanted there is provably nothing to reconcile.
        if wanted.is_empty() && self.watched.is_empty() {
            return false;
        }
        let before = self.watched.len();
        // Departures first, and the drop is the cancellation: `DirWatch::drop`
        // sets the stop event and joins its thread.
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
    pub fn due(&mut self, now: Instant) -> Vec<PathBuf> {
        if self.watched.is_empty() {
            return Vec::new();
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
                .filter(|entry| !entry.watches.is_empty())
                .count(),
        )
    }
}

/// Open the one or two watches a repository needs.
///
/// A free function and not a method because it is called from inside a closure
/// that [`GitWatch::sync_with`] holds while it holds `self` mutably; what it
/// needs is the mailbox and the proxy, not the registry.
fn subscribe(
    news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    proxy: &EventLoopProxy<AppEvent>,
    root: &Path,
) -> Vec<bt_platform::DirWatch> {
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
                Ok(watch) => Some(watch),
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
    let text = std::fs::read_to_string(&marker).ok()?;
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
    use super::*;

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
            Some(start + Duration::from_millis(1) + GIT_WATCH_QUIET),
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
                gap >= GIT_WATCH_FLOOR && gap < GIT_WATCH_FLOOR + Duration::from_millis(100),
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
            due <= quiet_at + GIT_WATCH_QUIET,
            "never later than a quiet window after the last thing that happened"
        );
        assert!(
            due >= storm_from + Duration::from_millis(4050) + GIT_WATCH_FLOOR,
            "and never sooner than a floor after the previous reading"
        );
        assert!(!clock.take_due(due - Duration::from_millis(1)));
        assert!(clock.take_due(due));
        assert_eq!(clock.due_at(), None, "and then it is quiet again");
    }

    /// PIN (D, rule 1) — **a watch is held only while a page is showing it, and
    /// dropped the moment it is not.**
    ///
    /// The gate is the set handed to [`GitWatch::sync`] and nothing else. This
    /// checks the bookkeeping half of it — that the map follows the set exactly,
    /// including the case that matters most for R31: an empty set holds nothing
    /// at all, so a window with no Git page open has no subscription open either.
    ///
    /// It runs without an event loop, so no watch is ever really started: the
    /// entries are the record of the attempt, which is the thing the gate is
    /// about.
    #[test]
    fn the_subscriptions_follow_the_pages_that_are_showing() {
        let mut watch = GitWatch::default();
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
        let mut watch = GitWatch::default();
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
        let mut watch = GitWatch::default();
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
            watch.due(now + GIT_WATCH_QUIET),
            vec![moved],
            "and then exactly the one that moved"
        );
    }

    /// [`GitWatch::sync`] with no kernel behind it — the gate's bookkeeping on
    /// its own.
    ///
    /// The subscription itself is proved in `bt_platform`'s own
    /// `dir_watch_tests`, against a real directory, which is the only place it
    /// can be proved. What these tests are about is the half that decides *when*
    /// one is held, and that half is [`GitWatch::sync_with`] itself rather than a
    /// second copy of it here.
    fn sync_for_test(watch: &mut GitWatch, wanted: &BTreeSet<PathBuf>) -> bool {
        watch.sync_with(wanted, |_| Vec::new())
    }
}
