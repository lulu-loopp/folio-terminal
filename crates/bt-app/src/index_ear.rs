//! The named tree ear that keeps palette file indexes current before the palette opens.
//!
//! Each retained index root is one steward member. Watcher threads apply only the bounded walk's
//! pure path filter, stamp a mailbox and wake the loop. The application turn folds reports and
//! timestamps into per-root `WatchClock`s; silence schedules nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use crate::window_seam::LoopProxy;

use crate::AppEvent;
use crate::palette_index::{self, FileIndexes, IndexRequest};
use crate::watch_clock::WatchClock;
use crate::watch_steward::{Depth, EarHandle, Listener, MemberReport};

struct Watched {
    heard: bool,
    clock: WatchClock,
}

/// One named tree watch and one event-armed clock per retained palette root.
#[derive(Default)]
pub struct IndexEar {
    news: Arc<Mutex<BTreeMap<PathBuf, Instant>>>,
    watched: BTreeMap<PathBuf, Watched>,
    rearm: BTreeSet<PathBuf>,
    ear: Option<EarHandle>,
}

impl IndexEar {
    /// A palette open is a reader-triggered chance to retry roots whose watch failed. The request
    /// is folded into the next application turn, where the process-wide wanted set is known.
    pub fn reader_asked(&mut self, roots: &BTreeSet<PathBuf>) {
        self.rearm.extend(roots.iter().cloned());
    }

    /// Bring the steward's wanted set level with the roots the register retained.
    pub fn sync(
        &mut self,
        roots: &BTreeSet<PathBuf>,
        indexes: &mut FileIndexes,
        now: Instant,
        proxy: &LoopProxy,
    ) {
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        self.sync_with(roots, indexes, now, move || {
            crate::watch_steward::open_ear(Depth::TreeNamed, listener(&news, &proxy))
        });
    }

    fn sync_with(
        &mut self,
        roots: &BTreeSet<PathBuf>,
        indexes: &mut FileIndexes,
        now: Instant,
        make_ear: impl FnOnce() -> EarHandle,
    ) {
        self.watched.retain(|root, _| roots.contains(root));
        for root in roots {
            self.watched.entry(root.clone()).or_insert(Watched {
                heard: false,
                clock: WatchClock::default(),
            });
        }
        lock(&self.news).retain(|root, _| roots.contains(root));
        if !roots.is_empty() && self.ear.is_none() {
            self.ear = Some(make_ear());
        }
        let rearm = std::mem::take(&mut self.rearm)
            .into_iter()
            .filter(|root| roots.contains(root))
            .filter(|root| self.watched.get(root).is_some_and(|watched| !watched.heard))
            .collect();
        if let Some(ear) = self.ear.as_mut() {
            ear.want(roots, rearm);
        }
        self.fold_reports(indexes, now);
    }

    /// Fold watcher news and claim every idle root whose clock is due.
    pub fn advance(&mut self, indexes: &mut FileIndexes, now: Instant) -> Vec<IndexRequest> {
        self.fold_reports(indexes, now);
        for (root, at) in std::mem::take(&mut *lock(&self.news)) {
            let Some(watched) = self.watched.get_mut(&root) else {
                continue;
            };
            indexes.mark_dirty(&root);
            watched.clock.note_event(at);
        }
        let due: Vec<PathBuf> = self
            .watched
            .iter_mut()
            .filter(|(root, _)| !indexes.running(root))
            .filter_map(|(root, watched)| watched.clock.take_due(now).then(|| root.clone()))
            .collect();
        due.into_iter()
            .filter_map(|root| indexes.claim(&root))
            .collect()
    }

    /// The next event-earned wake among idle roots. A running root waits for its answer wake;
    /// exposing its already-due clock here would turn the loop into a spin.
    #[must_use]
    pub fn deadline(&self, indexes: &FileIndexes) -> Option<Instant> {
        self.watched
            .iter()
            .filter(|(root, _)| !indexes.running(root))
            .filter_map(|(_, watched)| watched.clock.due_at())
            .min()
    }

    fn fold_reports(&mut self, indexes: &mut FileIndexes, now: Instant) {
        let Some(ear) = self.ear.as_ref() else {
            return;
        };
        for (root, report) in ear.take_reports() {
            let Some(watched) = self.watched.get_mut(&root) else {
                continue;
            };
            match report.report {
                MemberReport::Armed => {
                    indexes.mark_heard(&root);
                    if !watched.heard {
                        watched.heard = true;
                        indexes.mark_dirty(&root);
                        watched.clock.note_event(now);
                    }
                }
                MemberReport::Failed(_) => {
                    watched.heard = false;
                    indexes.mark_unheard(&root);
                }
            }
        }
    }
}

fn listener(news: &Arc<Mutex<BTreeMap<PathBuf, Instant>>>, proxy: &LoopProxy) -> Listener {
    let news = Arc::clone(news);
    let proxy = proxy.clone();
    Arc::new(move |root, change| {
        if palette_index::bears_on_index(change) {
            lock(&news).insert(root.to_path_buf(), Instant::now());
            let _ = proxy.send_event(AppEvent::FileIndexHeard);
        }
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::wait_for_a_wake;
    use crate::palette_index::{FileIndex, IndexResponse};
    use crate::watch_clock::WATCH_QUIET;
    use crate::watch_steward::Steward;
    use crate::watch_steward::fake::FakeDoor;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join("folio-index-ear").join(name)
    }

    fn ready(indexes: &mut FileIndexes, root: &Path) {
        let request = indexes.claim(root).expect("a first walk");
        indexes.accept(IndexResponse {
            root: root.to_path_buf(),
            epoch: request.epoch,
            index: FileIndex::default(),
        });
    }

    struct Rig {
        ear: IndexEar,
        indexes: FileIndexes,
        steward: &'static Steward,
        door: FakeDoor,
        wakes: mpsc::Receiver<()>,
    }

    impl Rig {
        fn new() -> Self {
            let door = FakeDoor::default();
            let steward = Box::leak(Box::new(Steward::new(door.clone(), |_| {})));
            let (wake, wakes) = mpsc::channel();
            steward.install_wake(move || {
                let _ = wake.send(());
            });
            Self {
                ear: IndexEar::default(),
                indexes: FileIndexes::default(),
                steward,
                door,
                wakes,
            }
        }

        fn sync(&mut self, now: Instant) {
            let roots = self.indexes.roots();
            let steward = self.steward;
            let news = Arc::clone(&self.ear.news);
            self.ear.sync_with(&roots, &mut self.indexes, now, || {
                steward.open_ear(
                    Depth::TreeNamed,
                    Arc::new(move |root, _| {
                        lock(&news).insert(root.to_path_buf(), Instant::now());
                    }),
                )
            });
        }

        fn after_wake(&mut self, now: Instant, what: &str) -> Vec<IndexRequest> {
            wait_for_a_wake(&self.wakes, what);
            self.ear.advance(&mut self.indexes, now)
        }
    }

    /// RED (T-PALETTE-DEEP) — the ear's members are exactly the register's retained roots.
    /// MUTATION: never retire a root; the fake door keeps `alpha` live.
    #[test]
    fn the_ear_follows_the_retained_roots() {
        let mut rig = Rig::new();
        let now = Instant::now();
        let alpha = root("alpha");
        let beta = root("资料");
        ready(&mut rig.indexes, &alpha);
        ready(&mut rig.indexes, &beta);
        rig.sync(now);
        let _ = rig.after_wake(now, "both roots armed");
        assert_eq!(rig.door.live(), vec![alpha.clone(), beta.clone()]);

        rig.indexes.retain(&BTreeSet::from([beta.clone()]));
        rig.sync(now);
        let _ = rig.after_wake(now, "the retired root dropped");
        rig.door.wait_for_drops(1);
        assert_eq!(rig.door.live(), vec![beta]);
    }

    /// RED (T-PALETTE-DEEP) — first `Armed` is news, covering the gap between the opening walk
    /// and the steward's later start. MUTATION: do not note the first `Armed`; no request is due.
    #[test]
    fn a_root_whose_watch_just_armed_is_walked() {
        let mut rig = Rig::new();
        let start = Instant::now();
        let root = root("arms");
        ready(&mut rig.indexes, &root);
        rig.sync(start);
        assert!(rig.after_wake(start, "the arming").is_empty());
        assert_eq!(rig.ear.deadline(&rig.indexes), Some(start + WATCH_QUIET));
        let requests = rig.ear.advance(&mut rig.indexes, start + WATCH_QUIET);
        assert_eq!(requests.len(), 1, "arming earns one covering walk");
    }

    /// RED (T-PALETTE-DEEP) — a passing watcher change claims the root when its event-earned
    /// clock is due, before any palette open.
    /// MUTATION: let the due clock only mark dirty; the request list stays empty.
    #[test]
    fn a_heard_change_walks_the_root_before_the_next_open() {
        let start = Instant::now();
        let root = root("heard-change");
        let mut ear = IndexEar::default();
        let mut indexes = FileIndexes::default();
        ready(&mut indexes, &root);
        indexes.mark_heard(&root);
        ear.watched.insert(
            root.clone(),
            Watched {
                heard: true,
                clock: WatchClock::default(),
            },
        );

        lock(&ear.news).insert(root, start);
        assert!(ear.advance(&mut indexes, start).is_empty());
        assert_eq!(
            ear.advance(&mut indexes, start + WATCH_QUIET).len(),
            1,
            "the background walk is asked without an open"
        );
    }

    /// RED (T-PALETTE-DEEP) — a watch ending makes the root dirty and unheard, so the next open
    /// walks it and can rearm it. MUTATION: omit the dirty mark on `Failed`; the open is current.
    #[test]
    fn a_watch_end_marks_the_root_dirty_and_unheard() {
        let mut rig = Rig::new();
        let start = Instant::now();
        let root = root("ends");
        ready(&mut rig.indexes, &root);
        rig.sync(start);
        let _ = rig.after_wake(start, "the arming");
        let request = rig
            .ear
            .advance(&mut rig.indexes, start + WATCH_QUIET)
            .pop()
            .expect("settle the arming news");
        rig.indexes.accept(IndexResponse {
            root: root.clone(),
            epoch: request.epoch,
            index: FileIndex::default(),
        });

        rig.door.end(&root);
        let _ = rig.after_wake(start + WATCH_QUIET, "the terminal failure");
        assert!(rig.indexes.dirty(&root), "the unheard transition is stale");
        assert!(matches!(
            rig.indexes.reopened(&root),
            crate::palette_index::Reopened::Asked(_)
        ));
    }

    /// RED (T-PALETTE-DEEP) — failure makes a root unheard and the next reader ask retries it
    /// once. MUTATION: omit the failed member from `rearm`; attempts stays one.
    #[test]
    fn a_failed_root_is_rearmed_once_by_the_next_palette_open() {
        let mut rig = Rig::new();
        let now = Instant::now();
        let root = root("returns");
        ready(&mut rig.indexes, &root);
        rig.door.refuse(&root);
        rig.sync(now);
        let _ = rig.after_wake(now, "the refusal");
        assert_eq!(rig.door.attempts(&root), 1);

        rig.door.admit(&root);
        rig.ear.reader_asked(&BTreeSet::from([root.clone()]));
        rig.sync(now);
        let _ = rig.after_wake(now, "the reader-triggered rearm");
        assert_eq!(rig.door.attempts(&root), 2);
        assert_eq!(rig.door.live(), vec![root]);
    }

    /// RED (T-PALETTE-DEEP; design revision (c)6) — fast walks under a continuous stream remain
    /// one claim per floor: at most thirty in sixty simulated seconds.
    /// MUTATION: claim immediately on every accept while news is owed; this is about 1,000.
    #[test]
    fn fast_walks_with_events_during_each_are_at_most_one_per_floor() {
        let start = Instant::now();
        let root = root("storm");
        let mut ear = IndexEar::default();
        let mut indexes = FileIndexes::default();
        ready(&mut indexes, &root);
        indexes.mark_heard(&root);
        ear.watched.insert(
            root.clone(),
            Watched {
                heard: true,
                clock: WatchClock::default(),
            },
        );

        let step = Duration::from_millis(50);
        let mut landing: Option<(Instant, IndexRequest)> = None;
        let mut claims = 0;
        for tick in 0..=1_200 {
            let now = start + step * tick;
            if landing.as_ref().is_some_and(|(at, _)| *at <= now) {
                let (_, request) = landing.take().expect("the due answer");
                indexes.accept(IndexResponse {
                    root: root.clone(),
                    epoch: request.epoch,
                    index: FileIndex::default(),
                });
            }
            lock(&ear.news).insert(root.clone(), now);
            let requests = ear.advance(&mut indexes, now);
            if let Some(request) = requests.into_iter().next() {
                claims += 1;
                landing = Some((now + step, request));
            }
        }
        assert!(claims <= 30, "{claims} watch-driven walks in one minute");
        assert!(
            (29..=30).contains(&claims),
            "the 50 ms landing can place the last floor just outside the minute: {claims}"
        );
    }

    /// RED (T-PALETTE-DEEP) — changes during one walk are one Boolean-shaped debt: no request
    /// overtakes the running walk, and exactly one rerun becomes claimable at the prior claim's
    /// floor.
    ///
    /// MUTATION: on accept, claim whenever `due_at` is merely present rather than when `take_due`
    /// says it is due; the pre-floor assertion receives an immediate rerun.
    #[test]
    fn changes_during_a_walk_are_one_more_walk_and_no_more() {
        let start = Instant::now();
        let root = root("one-owed");
        let mut ear = IndexEar::default();
        let mut indexes = FileIndexes::default();
        ready(&mut indexes, &root);
        indexes.mark_heard(&root);
        ear.watched.insert(
            root.clone(),
            Watched {
                heard: true,
                clock: WatchClock::default(),
            },
        );

        lock(&ear.news).insert(root.clone(), start);
        assert!(ear.advance(&mut indexes, start).is_empty());
        let first_at = start + WATCH_QUIET;
        let first = ear
            .advance(&mut indexes, first_at)
            .pop()
            .expect("the first clock-driven walk");

        for offset in 1..=200 {
            let at = first_at + Duration::from_millis(offset);
            lock(&ear.news).insert(root.clone(), at);
            assert!(
                ear.advance(&mut indexes, at).is_empty(),
                "a running walk is never overtaken"
            );
        }
        let landed = first_at + Duration::from_millis(250);
        indexes.accept(IndexResponse {
            root: root.clone(),
            epoch: first.epoch,
            index: FileIndex::default(),
        });
        assert!(
            ear.advance(&mut indexes, landed).is_empty(),
            "the owed rerun waits for the previous claim's floor"
        );

        let floor = first_at + crate::watch_clock::WATCH_FLOOR;
        let rerun = ear.advance(&mut indexes, floor);
        assert_eq!(rerun.len(), 1, "all two hundred changes owe one rerun");
        assert!(
            ear.advance(&mut indexes, floor).is_empty(),
            "and no further walk is queued"
        );
    }
}
