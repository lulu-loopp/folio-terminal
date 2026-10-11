//! **The watch steward: every directory watch starts, changes and ends on one worker**
//! (T-DIRWATCH-LIFECYCLE, 0.4.8 B5 ticket 1; `docs/plans/design/os-signals-2026-10-11.md`,
//! revisions (b)4, (b)5, (c)2, (c)3 and (e)3).
//!
//! A `bt_platform::DirWatch` waits twice: its start waits for the watcher thread's first read
//! (or stream) with no deadline, and its drop sets the stop and joins that thread. Until this lane
//! both waits ran on the window thread at every arming and every retirement (`window_waits.tsv`
//! row 8). Now the window thread holds no watch at all. It holds an [`EarHandle`] per ear — the
//! files column of a window, the git pages of the application, the preview seats of a window, the
//! schemes folder, the storage folder — and says what that ear wants with a nonblocking
//! [`EarHandle::want`]. The one `watch-steward` worker ([`WatchSteward::serve`], started by the
//! first request through `bt_platform::spawn_at_priority` at `BelowNormal`) starts, keeps and drops
//! every watch, and the start handshake and the join happen there. `DirWatch` keeps its own handle
//! ownership; only the thread that holds it moved.
//!
//! **The contract** (`crate::lane`'s shape): requests are the newest wanted set per ear, plus the
//! failed members the reader asked to be tried again, merged until served. The worker serves every
//! ear whose request changed, publishes one [`MemberReport`] per member of each ear it served into
//! [`REPORTS`] (newest per member), then wakes the loop with `AppEvent::WatchStewardAnswered` — the
//! report first, the wake after. Each ear drains its own reports on its own turn
//! ([`EarHandle::take_reports`]). Nothing waits: a request is a lock and a notify, and the worker
//! never runs on a clock.
//!
//! **Two member states** ((e)3): `Armed` (the watch is listening) and `Failed` (unheard). A member
//! is `Failed` when its start was refused, or when its watch ended on its own (`DirChange::Ended`:
//! a read error, a handle the system closed, an FSEvents root change or unmount). **A `Failed`
//! member is tried again once at the next reader-triggered event** — the ear names it in
//! [`EarHandle::want`]'s `rearm` — **or when a volume arrives that covers it**
//! ([`Steward::volume_arrived`]), **and never because time passed or a turn ran**: the ear's
//! per-turn request carries no `rearm`, and an unchanged set is not even sent. A failed attempt stays
//! `Failed`, and `diagnostics.log` hears the transition once, not each attempt.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};

use bt_platform::DirChange;
use bt_platform::admission::WorkerCtx;

/// Whether program-search directories on removable volumes are watched. E-2 owns turning this
/// on; until then arrivals can rearm explicitly named folders but PATH watches stay off them.
pub const WATCH_REMOVABLE_VOLUMES: bool = false;

/// **How far under a member the kernel listens, and whether the names reach the listener** — the
/// four doors of `bt_platform::DirWatch`, one each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Depth {
    /// The directory and everything under it (`DirWatch::start`): a repository, the storage
    /// folder.
    Tree,
    /// The directory and everything under it, with relative names
    /// (`DirWatch::start_named`): a palette file-index root.
    TreeNamed,
    /// The directory's own entries, nameless (`DirWatch::start_shallow`): a files-column folder.
    Shallow,
    /// The directory's own entries, by name (`DirWatch::start_shallow_named`): the folder a
    /// previewed file lives in.
    ShallowNamed,
}

/// **One ear**: a set of members wanted together, at one depth, heard by one listener. Numbered
/// process-wide, so every steward's reports can share [`REPORTS`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Ear(u64);

/// Where ear numbers come from.
static NEXT_EAR: AtomicU64 = AtomicU64::new(1);

/// **Why a member is unheard**: the refusal's kind and its words, kept as values because an
/// `io::Error` cannot be copied out of the worker's report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cause {
    kind: std::io::ErrorKind,
    said: String,
}

impl Cause {
    fn of(error: &std::io::Error) -> Self {
        Self {
            kind: error.kind(),
            said: error.to_string(),
        }
    }

    /// The refusal as an error again, kind and words, for an owner that reports it the way it
    /// reported a start error before this lane (the schemes folder reads the kind to keep a
    /// missing folder quiet).
    #[must_use]
    pub fn to_error(&self) -> std::io::Error {
        std::io::Error::new(self.kind, self.said.clone())
    }
}

impl std::fmt::Display for Cause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.said)
    }
}

/// **A member's state, as the worker last published it.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemberReport {
    /// The watch is listening.
    Armed,
    /// Unheard: its start was refused, or its watch ended on its own.
    Failed(Cause),
}

/// A report with the number the worker gave it, so a reader can tell two reports of one member
/// apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Numbered {
    pub number: u64,
    pub report: MemberReport,
}

/// **What the worker has published and the ears have not drained**: newest per member, keyed by
/// ear and path. One map for every steward in the process; the ear numbers keep them apart.
pub static REPORTS: Mutex<BTreeMap<(Ear, PathBuf), Numbered>> = Mutex::new(BTreeMap::new());

/// **What an ear hears**: the member that moved, and what the kernel said (`Named` or `Unknown`).
/// Called on that member's watcher thread, so it must only record and wake.
pub type Listener = Arc<dyn Fn(&Path, DirChange<'_>) + Send + Sync>;

/// **The door the worker starts a watch through**: the platform's in the product
/// ([`Kernel`]), a fake in the tests that holds a start or a drop on a gate.
pub trait Door: Send + Sync {
    /// Start one member's watch. `wake` hears every change and, if the watch ends on its own,
    /// `DirChange::Ended`, once.
    ///
    /// # Errors
    ///
    /// The refusal of the start, which makes the member `Failed`.
    fn start(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        depth: Depth,
        wake: Box<dyn Fn(DirChange<'_>) + Send>,
    ) -> std::io::Result<Box<dyn Held>>;
}

/// **A started watch, held by the worker**: dropping it stops it (on the worker), and an end it
/// came to on its own is taken once.
pub trait Held: Send {
    /// The error that ended the watch on its own, handed over once.
    fn take_failure(&mut self) -> Option<std::io::Error>;
}

/// **The platform's door**: `bt_platform::DirWatch`, by depth.
pub struct Kernel;

impl Door for Kernel {
    fn start(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        depth: Depth,
        wake: Box<dyn Fn(DirChange<'_>) + Send>,
    ) -> std::io::Result<Box<dyn Held>> {
        let watch = match depth {
            Depth::Tree => bt_platform::DirWatch::start(ctx, path, wake),
            Depth::TreeNamed => bt_platform::DirWatch::start_named(ctx, path, wake),
            Depth::Shallow => bt_platform::DirWatch::start_shallow(ctx, path, wake),
            Depth::ShallowNamed => bt_platform::DirWatch::start_shallow_named(ctx, path, wake),
        }?;
        Ok(Box::new(watch))
    }
}

impl Held for bt_platform::DirWatch {
    fn take_failure(&mut self) -> Option<std::io::Error> {
        bt_platform::DirWatch::take_failure(self)
    }
}

/// One ear's standing request.
struct EarAsk {
    depth: Depth,
    listener: Listener,
    /// The newest wanted set.
    wanted: BTreeSet<PathBuf>,
    /// The failed members the reader asked to be tried again, merged until served.
    rearm: BTreeSet<PathBuf>,
}

/// The requests, and the worker that serves them.
#[derive(Default)]
struct Asks {
    /// Every open ear's standing request. An ear that closed is gone from here; the worker drops
    /// its members on its next round.
    ears: BTreeMap<Ear, EarAsk>,
    /// The ears whose request changed since the worker last served them, and the ears that
    /// closed.
    dirty: BTreeSet<Ear>,
    /// A watch said `Ended`: the worker asks every member it holds which one.
    ended: bool,
    /// The volume roots that arrived and have not been served.
    arrived: Vec<PathBuf>,
    /// A worker thread is running.
    worker: bool,
}

/// **The steward's shared half**: the door, the standing requests, the wake. The worker's half,
/// which holds the watches, is [`WatchSteward`].
pub struct Steward {
    door: Box<dyn Door>,
    asks: Mutex<Asks>,
    asked: Condvar,
    wake: OnceLock<Box<dyn Fn() + Send + Sync>>,
    note: Box<dyn Fn(&str) + Send + Sync>,
    /// The number of the newest report published.
    published: AtomicU64,
}

impl Steward {
    /// A steward starting watches through `door`, saying its diagnostics lines through `note`.
    pub fn new(door: impl Door + 'static, note: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            door: Box::new(door),
            asks: Mutex::new(Asks::default()),
            asked: Condvar::new(),
            wake: OnceLock::new(),
            note: Box::new(note),
            published: AtomicU64::new(0),
        }
    }

    /// Teach the worker how to bring the event loop round. Once; a second call is ignored.
    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        let _ = self.wake.set(Box::new(wake));
    }

    /// **Open an ear** at `depth`, heard by `listener`. Wants nothing until its first
    /// [`EarHandle::want`]; one lock, never a wait.
    pub fn open_ear(&'static self, depth: Depth, listener: Listener) -> EarHandle {
        let ear = Ear(NEXT_EAR.fetch_add(1, Ordering::Relaxed));
        self.lock().ears.insert(
            ear,
            EarAsk {
                depth,
                listener,
                wanted: BTreeSet::new(),
                rearm: BTreeSet::new(),
            },
        );
        EarHandle {
            steward: self,
            ear,
            sent: BTreeSet::new(),
        }
    }

    /// **A volume arrived** at `root`: every `Failed` member under it, of every ear, is tried again
    /// once. The hook the `Volumes` signal calls (B5 ticket 2); one lock, never a wait.
    pub fn volume_arrived(&'static self, root: PathBuf) {
        let mut asks = self.lock();
        asks.arrived.push(root);
        self.see_it_served(asks);
    }

    /// The request side of every door above: mark the work and see that a worker serves it.
    /// Never waits: a notify, or — the first time, and after a worker ended — a thread start.
    fn see_it_served(&'static self, mut asks: MutexGuard<'_, Asks>) {
        if asks.worker {
            self.asked.notify_one();
            return;
        }
        asks.worker = true;
        // A worker that starts serves every open ear: one that ended took its watches with it.
        let open: Vec<Ear> = asks.ears.keys().copied().collect();
        asks.dirty.extend(open);
        drop(asks);
        let started = bt_platform::spawn_at_priority(
            "watch-steward",
            bt_platform::ThreadPriority::BelowNormal,
            move |ctx| WatchSteward::for_lane(self).serve(ctx),
        );
        if let Err(error) = started {
            // The work stays marked and the next request tries the thread again; nothing on the
            // window thread waits for an answer meanwhile.
            self.lock().worker = false;
            (self.note)(&format!(
                "watch steward: its worker would not start ({error}); the next request starts it"
            ));
        }
    }

    /// A watch said `Ended`, on its watcher thread: the worker is told, and finds which.
    fn an_end_was_said(&'static self) {
        let mut asks = self.lock();
        asks.ended = true;
        self.see_it_served(asks);
    }

    fn publish(&self, reports: Vec<(Ear, PathBuf, MemberReport)>) {
        if reports.is_empty() {
            return;
        }
        {
            let mut held = lock(&REPORTS);
            for (ear, path, report) in reports {
                let number = self.published.fetch_add(1, Ordering::Relaxed) + 1;
                held.insert((ear, path), Numbered { number, report });
            }
        }
        // After the reports are in the map and never before: a wake that raced the publication
        // would send the loop to drain an empty map.
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }

    fn lock(&self) -> MutexGuard<'_, Asks> {
        lock(&self.asks)
    }
}

/// **An ear, held by its owner on the window thread.** Dropping it closes the ear: the worker drops
/// its members on its next round, there.
pub struct EarHandle {
    steward: &'static Steward,
    ear: Ear,
    /// The set last sent, so an unchanged set is not sent again.
    sent: BTreeSet<PathBuf>,
}

impl std::fmt::Debug for EarHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("EarHandle").field(&self.ear).finish()
    }
}

impl EarHandle {
    /// **Say what this ear wants now**, and which of its `Failed` members the reader asked to be
    /// tried again. Never waits: an unchanged set with nothing to rearm is not even sent, and a
    /// changed one is a lock and a notify.
    pub fn want(&mut self, wanted: &BTreeSet<PathBuf>, rearm: BTreeSet<PathBuf>) {
        if *wanted == self.sent && rearm.is_empty() {
            return;
        }
        self.sent.clone_from(wanted);
        let mut asks = self.steward.lock();
        let Some(ask) = asks.ears.get_mut(&self.ear) else {
            return;
        };
        ask.wanted.clone_from(wanted);
        ask.rearm.extend(rearm);
        asks.dirty.insert(self.ear);
        self.steward.see_it_served(asks);
    }

    /// **This ear's reports since the last drain**, newest per member. One lock; never waits for
    /// the worker.
    pub fn take_reports(&self) -> Vec<(PathBuf, Numbered)> {
        let mut held = lock(&REPORTS);
        let keys: Vec<(Ear, PathBuf)> = held
            .range((self.ear, PathBuf::new())..)
            .take_while(|((ear, _), _)| *ear == self.ear)
            .map(|(key, _)| key.clone())
            .collect();
        keys.into_iter()
            .filter_map(|key| {
                let report = held.remove(&key)?;
                Some((key.1, report))
            })
            .collect()
    }
}

impl Drop for EarHandle {
    fn drop(&mut self) {
        let mut asks = self.steward.lock();
        asks.ears.remove(&self.ear);
        asks.dirty.insert(self.ear);
        self.steward.see_it_served(asks);
    }
}

/// One member, as the worker holds it.
enum Member {
    /// Listening: its watch, held here.
    Armed(Box<dyn Held>),
    /// Unheard, and why.
    Failed(Cause),
}

impl Member {
    fn report(&self) -> MemberReport {
        match self {
            Self::Armed(_) => MemberReport::Armed,
            Self::Failed(cause) => MemberReport::Failed(cause.clone()),
        }
    }

    fn failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// **The steward's worker half**: every watch, owned by the worker that serves the requests.
pub struct WatchSteward {
    lane: &'static Steward,
    members: BTreeMap<(Ear, PathBuf), Member>,
}

/// One ear's request, as the worker took it.
struct Round {
    ear: Ear,
    /// `None` when the ear closed.
    ask: Option<(Depth, Listener, BTreeSet<PathBuf>, BTreeSet<PathBuf>)>,
}

impl WatchSteward {
    fn for_lane(lane: &'static Steward) -> Self {
        Self {
            lane,
            members: BTreeMap::new(),
        }
    }

    /// **The worker's whole body**: wait for work, serve it, publish, and wait again. Every watch
    /// start, change and drop of the process happens in here.
    pub fn serve(mut self, ctx: &WorkerCtx) {
        // A worker that unwinds leaves the lane able to answer: the next request starts another,
        // which serves every open ear again.
        let _gone = WorkerGone(self.lane);
        loop {
            let (rounds, ended, arrived, open) = {
                let mut asks = self.lane.lock();
                while asks.dirty.is_empty() && !asks.ended && asks.arrived.is_empty() {
                    asks = self
                        .lane
                        .asked
                        .wait(asks)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                let dirty = std::mem::take(&mut asks.dirty);
                let rounds: Vec<Round> = dirty
                    .into_iter()
                    .map(|ear| Round {
                        ear,
                        ask: asks.ears.get_mut(&ear).map(|ask| {
                            (
                                ask.depth,
                                Arc::clone(&ask.listener),
                                ask.wanted.clone(),
                                std::mem::take(&mut ask.rearm),
                            )
                        }),
                    })
                    .collect();
                let ended = std::mem::take(&mut asks.ended);
                let arrived = std::mem::take(&mut asks.arrived);
                let open: BTreeMap<Ear, (Depth, Listener)> = asks
                    .ears
                    .iter()
                    .map(|(ear, ask)| (*ear, (ask.depth, Arc::clone(&ask.listener))))
                    .collect();
                (rounds, ended, arrived, open)
            };
            let mut reports = Vec::new();
            if ended {
                self.take_the_ends(&mut reports);
            }
            for round in rounds {
                self.serve_ear(ctx, round, &mut reports);
            }
            if !arrived.is_empty() {
                self.rearm_under(ctx, &arrived, &open, &mut reports);
            }
            self.lane.publish(reports);
        }
    }

    /// Every held watch that ended on its own becomes `Failed`, and its watch is dropped here.
    fn take_the_ends(&mut self, reports: &mut Vec<(Ear, PathBuf, MemberReport)>) {
        for ((ear, path), member) in &mut self.members {
            let Member::Armed(held) = member else {
                continue;
            };
            let Some(error) = held.take_failure() else {
                continue;
            };
            let cause = Cause::of(&error);
            (self.lane.note)(&unheard_line(path, &cause));
            reports.push((*ear, path.clone(), MemberReport::Failed(cause.clone())));
            // The ended watch is dropped here, on the worker: its thread has already finished.
            *member = Member::Failed(cause);
        }
    }

    /// One ear's request: departures dropped, arrivals started, the named `Failed` members tried
    /// once; then the state of every member of the ear is published.
    fn serve_ear(
        &mut self,
        ctx: &WorkerCtx,
        round: Round,
        reports: &mut Vec<(Ear, PathBuf, MemberReport)>,
    ) {
        let ear = round.ear;
        let wanted = round
            .ask
            .as_ref()
            .map(|(_, _, wanted, _)| wanted.clone())
            .unwrap_or_default();
        let departed: Vec<(Ear, PathBuf)> = self
            .members
            .keys()
            .filter(|(owner, path)| *owner == ear && !wanted.contains(path))
            .cloned()
            .collect();
        for key in departed {
            // The drop — the stop and the join — happens here, on the worker. A
            // report the ear never drained goes with it.
            self.members.remove(&key);
            lock(&REPORTS).remove(&key);
        }
        // The ear closed: every member went above, and there is nothing to report.
        let Some((depth, listener, wanted, rearm)) = round.ask else {
            return;
        };
        for path in &wanted {
            let key = (ear, path.clone());
            match self.members.get(&key) {
                Some(member) if !(member.failed() && rearm.contains(path)) => {
                    reports.push((ear, path.clone(), member.report()));
                }
                held => {
                    let was_failed = held.is_some();
                    let report = self.attempt(ctx, ear, path, depth, &listener, was_failed);
                    reports.push((ear, path.clone(), report));
                }
            }
        }
    }

    /// Every `Failed` member under one of `arrived`, of every open ear, tried once.
    fn rearm_under(
        &mut self,
        ctx: &WorkerCtx,
        arrived: &[PathBuf],
        open: &BTreeMap<Ear, (Depth, Listener)>,
        reports: &mut Vec<(Ear, PathBuf, MemberReport)>,
    ) {
        let covered: Vec<(Ear, PathBuf)> = self
            .members
            .iter()
            .filter(|((_, path), member)| {
                member.failed() && arrived.iter().any(|root| path.starts_with(root))
            })
            .map(|(key, _)| key.clone())
            .collect();
        for (ear, path) in covered {
            let Some((depth, listener)) = open.get(&ear) else {
                continue;
            };
            let report = self.attempt(ctx, ear, &path, *depth, listener, true);
            reports.push((ear, path, report));
        }
    }

    /// **One start**, its member recorded either way. A failure is said in `diagnostics.log` only
    /// on the transition: a member already `Failed` that fails again is not said again.
    fn attempt(
        &mut self,
        ctx: &WorkerCtx,
        ear: Ear,
        path: &Path,
        depth: Depth,
        listener: &Listener,
        was_failed: bool,
    ) -> MemberReport {
        let lane = self.lane;
        let heard = Arc::clone(listener);
        let key = path.to_path_buf();
        let wake: Box<dyn Fn(DirChange<'_>) + Send> = Box::new(move |change| match change {
            DirChange::Ended => lane.an_end_was_said(),
            change => heard(&key, change),
        });
        let member = match self.lane.door.start(ctx, path, depth, wake) {
            Ok(held) => Member::Armed(held),
            Err(error) => {
                let cause = Cause::of(&error);
                if !was_failed {
                    (self.lane.note)(&unheard_line(path, &cause));
                }
                Member::Failed(cause)
            }
        };
        let report = member.report();
        self.members.insert((ear, path.to_path_buf()), member);
        report
    }
}

/// The `diagnostics.log` line of a member going unheard.
fn unheard_line(path: &Path, cause: &Cause) -> String {
    format!(
        "watch steward: {} is not heard ({cause}); it is tried again at the next reader's ask \
         or volume arrival",
        path.display()
    )
}

/// **The worker's end**, by return or by unwind: the lane is marked without a worker, so the next
/// request starts one.
struct WorkerGone(&'static Steward);

impl Drop for WorkerGone {
    fn drop(&mut self) {
        self.0.lock().worker = false;
    }
}

/// A mutex this crate never poisons on purpose, unwrapped without a panic path.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The product's steward: the platform's door, its lines to `diagnostics.log`.
static STEWARD: LazyLock<Steward> =
    LazyLock::new(|| Steward::new(Kernel, crate::diagnostics::note));

/// [`Steward::install_wake`] on the product's steward. Once, at startup, before the first request.
pub fn install_wake(wake: impl Fn() + Send + Sync + 'static) {
    STEWARD.install_wake(wake);
}

/// [`Steward::open_ear`] on the product's steward.
pub fn open_ear(depth: Depth, listener: Listener) -> EarHandle {
    STEWARD.open_ear(depth, listener)
}

/// [`Steward::volume_arrived`] on the product's steward. `WM_DEVICECHANGE` deliberately reads
/// only the fixed header, which carries no root, so the empty path covers every failed member.
pub fn volume_arrived() {
    STEWARD.volume_arrived(PathBuf::new());
}

/// **A door a test writes** (T-DIRWATCH-LIFECYCLE): starts that succeed or are refused by path,
/// counted per path; watches that a test can end as the platform would (`Ended`); and a gate that
/// holds a start or a drop where it happens, recording the thread it happens on.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};

    use bt_platform::DirChange;
    use bt_platform::admission::WorkerCtx;

    use super::{Depth, Door, Held};

    /// How long a held start or drop waits before going on by itself, so a mutation that runs it
    /// on the test's own thread comes back (slow, and caught) instead of hanging the suite.
    const HOLD_LIMIT: Duration = Duration::from_secs(5);

    #[derive(Default)]
    struct State {
        refused: BTreeSet<PathBuf>,
        attempts: BTreeMap<PathBuf, usize>,
        live: BTreeSet<PathBuf>,
        ended: BTreeMap<PathBuf, std::io::Error>,
        drops: usize,
        hold_starts: bool,
        hold_drops: bool,
        /// Starts and drops in progress now.
        starting: usize,
        dropping: usize,
        /// The thread each start and each drop ran on, in order.
        start_threads: Vec<String>,
        drop_threads: Vec<String>,
    }

    /// One member's wake, as the steward handed it to the door.
    type Wake = Box<dyn Fn(DirChange<'_>) + Send>;

    #[derive(Default)]
    struct Shared {
        state: Mutex<State>,
        changed: Condvar,
        wakes: Mutex<BTreeMap<PathBuf, Wake>>,
    }

    /// The fake door; clones share one state.
    #[derive(Clone, Default)]
    pub(crate) struct FakeDoor(Arc<Shared>);

    fn thread_name() -> String {
        std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned()
    }

    impl FakeDoor {
        fn state(&self) -> MutexGuard<'_, State> {
            self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// Wait (within the lane suite's patience) until `done` holds.
        fn wait_until(&self, what: &str, done: impl Fn(&State) -> bool) {
            let since = Instant::now();
            let mut state = self.state();
            while !done(&state) {
                assert!(
                    since.elapsed() < crate::lane::PATIENCE,
                    "the fake door waited past the suite's patience for {what}"
                );
                state = self
                    .0
                    .changed
                    .wait_timeout(state, crate::lane::PATIENCE)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }

        /// Refuse every start of `path` from now on, as a missing folder is refused.
        pub(crate) fn refuse(&self, path: &Path) {
            self.state().refused.insert(path.to_path_buf());
        }

        /// Let `path` start again: the folder is back.
        pub(crate) fn admit(&self, path: &Path) {
            self.state().refused.remove(path);
        }

        /// How many starts of `path` were attempted.
        pub(crate) fn attempts(&self, path: &Path) -> usize {
            self.state().attempts.get(path).copied().unwrap_or(0)
        }

        /// The members whose watches are started and not dropped.
        pub(crate) fn live(&self) -> Vec<PathBuf> {
            self.state().live.iter().cloned().collect()
        }

        /// Wait until `count` watches have been dropped in all.
        pub(crate) fn wait_for_drops(&self, count: usize) {
            self.wait_until("the drops", |state| state.drops >= count);
        }

        /// **End `path`'s watch as the platform would**: the error kept, then `Ended` said.
        pub(crate) fn end(&self, path: &Path) {
            self.state().ended.insert(
                path.to_path_buf(),
                std::io::Error::from(std::io::ErrorKind::NotFound),
            );
            let wakes = self.0.wakes.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(wake) = wakes.get(path) {
                wake(DirChange::Ended);
            }
        }

        /// Say that `path` moved, as its watcher thread would.
        pub(crate) fn moved(&self, path: &Path) {
            let wakes = self.0.wakes.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(wake) = wakes.get(path) {
                wake(DirChange::Unknown);
            }
        }

        /// Say that named entries moved, as a named watcher thread would.
        pub(crate) fn named(&self, path: &Path, names: &[std::ffi::OsString]) {
            let wakes = self.0.wakes.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(wake) = wakes.get(path) {
                wake(DirChange::Named(names));
            }
        }

        /// Hold every start where it happens until [`Self::release`].
        pub(crate) fn hold_starts(&self) {
            self.state().hold_starts = true;
        }

        /// Hold every drop where it happens until [`Self::release`].
        pub(crate) fn hold_drops(&self) {
            self.state().hold_drops = true;
        }

        /// Let held starts and drops go.
        pub(crate) fn release(&self) {
            let mut state = self.state();
            state.hold_starts = false;
            state.hold_drops = false;
            self.0.changed.notify_all();
        }

        /// Wait until a start is in progress, held.
        pub(crate) fn wait_for_a_held_start(&self) {
            self.wait_until("a start to be held", |state| state.starting > 0);
        }

        /// Wait until a drop is in progress, held.
        pub(crate) fn wait_for_a_held_drop(&self) {
            self.wait_until("a drop to be held", |state| state.dropping > 0);
        }

        /// Starts and drops in progress now.
        pub(crate) fn in_progress(&self) -> (usize, usize) {
            let state = self.state();
            (state.starting, state.dropping)
        }

        /// The threads the starts and the drops ran on.
        pub(crate) fn threads(&self) -> (Vec<String>, Vec<String>) {
            let state = self.state();
            (state.start_threads.clone(), state.drop_threads.clone())
        }

        /// Wait while `held` says so, up to [`HOLD_LIMIT`], counting the wait in `busy`.
        fn pass(&self, busy: fn(&mut State) -> &mut usize, held: fn(&State) -> bool) {
            let mut state = self.state();
            *busy(&mut state) += 1;
            self.0.changed.notify_all();
            let since = Instant::now();
            while held(&state) && since.elapsed() < HOLD_LIMIT {
                state = self
                    .0
                    .changed
                    .wait_timeout(state, HOLD_LIMIT)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
            *busy(&mut state) -= 1;
            self.0.changed.notify_all();
        }
    }

    impl Door for FakeDoor {
        fn start(
            &self,
            _ctx: &WorkerCtx,
            path: &Path,
            _depth: Depth,
            wake: Box<dyn Fn(DirChange<'_>) + Send>,
        ) -> std::io::Result<Box<dyn Held>> {
            {
                let mut state = self.state();
                *state.attempts.entry(path.to_path_buf()).or_default() += 1;
                state.start_threads.push(thread_name());
            }
            self.pass(|state| &mut state.starting, |state| state.hold_starts);
            if self.state().refused.contains(path) {
                return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
            }
            self.state().live.insert(path.to_path_buf());
            self.0
                .wakes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(path.to_path_buf(), wake);
            Ok(Box::new(FakeWatch {
                door: self.clone(),
                path: path.to_path_buf(),
            }))
        }
    }

    /// A started fake watch.
    struct FakeWatch {
        door: FakeDoor,
        path: PathBuf,
    }

    impl Held for FakeWatch {
        fn take_failure(&mut self) -> Option<std::io::Error> {
            self.door.state().ended.remove(&self.path)
        }
    }

    impl Drop for FakeWatch {
        fn drop(&mut self) {
            self.door.state().drop_threads.push(thread_name());
            self.door
                .pass(|state| &mut state.dropping, |state| state.hold_drops);
            self.door
                .0
                .wakes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&self.path);
            let mut state = self.door.state();
            state.live.remove(&self.path);
            state.drops += 1;
            self.door.0.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};

    use super::fake::FakeDoor;
    use super::{Depth, EarHandle, MemberReport, Steward};
    use crate::lane::wait_for_a_wake;

    /// A steward of the test's own over `door`, and the wakes it sends after each publication.
    fn steward_over(door: &FakeDoor) -> (&'static Steward, mpsc::Receiver<()>) {
        let steward: &'static Steward = Box::leak(Box::new(Steward::new(door.clone(), |_| {})));
        let (woke, wakes) = mpsc::channel();
        steward.install_wake(move || {
            let _ = woke.send(());
        });
        (steward, wakes)
    }

    fn quiet_ear(steward: &'static Steward) -> EarHandle {
        steward.open_ear(Depth::Shallow, Arc::new(|_: &Path, _| {}))
    }

    fn folder(name: &str) -> PathBuf {
        PathBuf::from("/Users/alice/项目").join(name)
    }

    fn states(ear: &EarHandle) -> Vec<(PathBuf, MemberReport)> {
        ear.take_reports()
            .into_iter()
            .map(|(path, numbered)| (path, numbered.report))
            .collect()
    }

    /// PIN (B5 design note, revision (c)3) — **a sync returns while a watch start is held**, and
    /// the loop's drain runs: the start is the worker's, and the window thread waits for none of
    /// it.
    ///
    /// The fake door holds every start where it happens. The test thread stands for the window
    /// thread: it asks for a member, sees the start held on the steward's thread, drains the
    /// reports (nothing yet — the loop's arm runs), and only then lets the start go.
    ///
    /// MUTATION: make `EarHandle::want` start the member on the calling thread (call
    /// `Door::start` there) — `want` does not return until the hold gives up, no start is in
    /// progress when it does, and the start's thread is the test's.
    #[test]
    fn a_sync_returns_while_a_watch_start_is_held() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let mut ear = quiet_ear(steward);
        door.hold_starts();
        ear.want(&BTreeSet::from([folder("held")]), BTreeSet::new());
        door.wait_for_a_held_start();
        assert_eq!(
            door.in_progress().0,
            1,
            "the start is still held, so the sync returned without it"
        );
        assert!(
            states(&ear).is_empty(),
            "the loop's drain runs and finds nothing yet"
        );
        door.release();
        wait_for_a_wake(&wakes, "the held start's report");
        assert_eq!(states(&ear), vec![(folder("held"), MemberReport::Armed)]);
        assert_eq!(door.threads().0, vec!["watch-steward".to_owned()]);
    }

    /// PIN (B5 design note, revision (c)3) — **a retirement returns while a watch drop is held**,
    /// and the drop happens on the steward's thread.
    ///
    /// MUTATION: have the worker hand the retired watch back through its report, to be dropped by
    /// the drain on the calling thread — the drop's thread is the test's, and it is not in
    /// progress when the retirement returns.
    #[test]
    fn a_retirement_returns_while_a_watch_drop_is_held() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let mut ear = quiet_ear(steward);
        ear.want(&BTreeSet::from([folder("kept")]), BTreeSet::new());
        wait_for_a_wake(&wakes, "the arming");
        let _ = states(&ear);
        door.hold_drops();
        ear.want(&BTreeSet::new(), BTreeSet::new());
        door.wait_for_a_held_drop();
        let _ = states(&ear);
        assert_eq!(
            door.in_progress().1,
            1,
            "the drop is still held, so the retirement and the drain returned without it"
        );
        door.release();
        door.wait_for_drops(1);
        assert_eq!(door.threads().1, vec!["watch-steward".to_owned()]);
        assert!(door.live().is_empty());
    }

    /// PIN (B5 design note, revision (b)5) — **a watch that ends on its own makes its member
    /// `Failed`**, reported and woken for, and its watch is dropped on the steward.
    ///
    /// MUTATION: have the worker pass `DirChange::Ended` to the listener like any change — no
    /// report arrives and the wait for it fails.
    #[test]
    fn a_watch_that_ends_makes_its_member_failed() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let mut ear = quiet_ear(steward);
        ear.want(&BTreeSet::from([folder("goes")]), BTreeSet::new());
        wait_for_a_wake(&wakes, "the arming");
        let _ = states(&ear);
        door.end(&folder("goes"));
        wait_for_a_wake(&wakes, "the end");
        let reports = states(&ear);
        assert_eq!(reports.len(), 1);
        assert!(
            matches!(reports[0].1, MemberReport::Failed(_)),
            "the member is unheard: {reports:?}"
        );
        door.wait_for_drops(1);
        assert_eq!(door.threads().1, vec!["watch-steward".to_owned()]);
    }

    /// PIN (B5 design note, revisions (c)2 and (e)3) — **a volume arriving over a `Failed` member
    /// tries it again once; one elsewhere does not.** The hook the `Volumes` signal will call
    /// (B5 ticket 2), driven here by hand.
    ///
    /// MUTATION: drop the `starts_with` test in `rearm_under` (every arrival rearms every
    /// `Failed` member) — the arrival elsewhere is an attempt.
    #[test]
    fn a_volume_arriving_over_a_failed_member_tries_it_again_once() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let mut ear = quiet_ear(steward);
        let member = PathBuf::from("/Volumes/USB 盘/notes");
        door.refuse(&member);
        ear.want(&BTreeSet::from([member.clone()]), BTreeSet::new());
        wait_for_a_wake(&wakes, "the refusal");
        assert_eq!(door.attempts(&member), 1);

        // A volume elsewhere, then a request the worker must answer: once its
        // answer is in, the arrival before it has been served too.
        steward.volume_arrived(PathBuf::from("/Volumes/Elsewhere"));
        let other = folder("other");
        ear.want(
            &BTreeSet::from([member.clone(), other.clone()]),
            BTreeSet::new(),
        );
        wait_for_a_wake(&wakes, "the other member's arming");
        assert_eq!(
            door.attempts(&member),
            1,
            "a volume that does not cover the member is no attempt"
        );

        door.admit(&member);
        steward.volume_arrived(PathBuf::from("/Volumes/USB 盘"));
        wait_for_a_wake(&wakes, "the volume's attempt");
        assert_eq!(
            door.attempts(&member),
            2,
            "one attempt, for the volume that covers it"
        );
        assert!(
            states(&ear).contains(&(member, MemberReport::Armed)),
            "and the member is heard again"
        );
    }

    /// PIN — **an ear that closes takes its watches with it, dropped on the steward.**
    ///
    /// MUTATION: make `EarHandle::drop` only remove the ear without marking it for the worker —
    /// the watches are never dropped and the wait for them fails.
    #[test]
    fn a_closed_ear_drops_its_watches_on_the_steward() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let mut ear = quiet_ear(steward);
        ear.want(&BTreeSet::from([folder("a"), folder("b")]), BTreeSet::new());
        wait_for_a_wake(&wakes, "the arming");
        drop(ear);
        door.wait_for_drops(2);
        assert!(door.live().is_empty());
        assert_eq!(
            door.threads().1,
            vec!["watch-steward".to_owned(), "watch-steward".to_owned()]
        );
    }

    /// PIN (B5 design note, revisions (b)4, (c)3 and (e)3) — **only the steward starts or drops a
    /// watch.**
    ///
    /// Read through `bt_source`, by item scope rather than as a file's text: `DirWatch` is named
    /// in the product only inside `crate::watch_steward`. A type nobody else can name is a value
    /// nobody else can start, hold or drop — so no ear can take a watch back onto the window
    /// thread, whatever file it is written in. The starts themselves need a `WorkerCtx`, which
    /// the compiler checks; this holds the drops.
    ///
    /// MUTATION: give `FilesWatch` a field `held: Option<bt_platform::DirWatch>` (a watch the
    /// window thread would drop) — the occurrence outside the steward is named.
    #[test]
    fn only_the_steward_starts_or_drops_a_watch() {
        use bt_source::{Index, ModuleSpec, Pattern, Scope, Search, View, needle};

        let index = Index::of_package("bt-app");
        let named_in = |scope: Scope| {
            index
                .search(
                    &Search::new(needle!(Pattern::identifier("DirWatch")), View::Identifiers)
                        .in_scope(scope),
                )
                .unwrap_or_else(|failure| panic!("{failure}"))
                .in_the_product(index)
        };
        let everywhere = named_in(Scope::Everything);
        let steward = named_in(Scope::Modules(vec![ModuleSpec::tree(
            "crate::watch_steward",
        )]));
        assert!(
            !steward.is_empty(),
            "the steward itself no longer names the watch, so this gate is reading nothing"
        );
        assert_eq!(
            everywhere.len(),
            steward.len(),
            "`DirWatch` is named outside the watch steward — a watch held there is started or \
             dropped off the steward's thread:\n{}",
            everywhere.report(index)
        );
    }

    /// PIN — **a change reaches the ear's listener with the member it is about.**
    #[test]
    fn a_change_reaches_the_listener_with_its_member() {
        let door = FakeDoor::default();
        let (steward, wakes) = steward_over(&door);
        let (heard, said) = mpsc::channel();
        let mut ear = steward.open_ear(
            Depth::Tree,
            Arc::new(move |path: &Path, _| {
                let _ = heard.send(path.to_path_buf());
            }),
        );
        ear.want(&BTreeSet::from([folder("repo")]), BTreeSet::new());
        wait_for_a_wake(&wakes, "the arming");
        door.moved(&folder("repo"));
        assert_eq!(said.try_recv().ok(), Some(folder("repo")));
    }
}
