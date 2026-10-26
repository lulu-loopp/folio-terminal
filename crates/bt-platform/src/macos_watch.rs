//! **A subscription to a directory, over FSEvents** (ticket M2-1).
//!
//! The macOS twin of `windows_impl`'s [`DirWatch`], and the one door in this
//! crate whose *contract* is four doors: `start` watches a nameless tree, `start_named` watches a
//! named tree, `start_shallow` watches one directory's own entries, and
//! `start_shallow_named` watches the same entries and says which moved. The backend inventory
//! (`docs/plans/port/backend-inventory-2026-09-12.md` §6 ⑤) is explicit that the
//! depth enum is **not** part of the public interface — `bt-app` reaches the
//! four contracts through the four constructors — so preserving the contracts
//! means preserving the four doors, and this module does not export a depth.
//!
//! # Why FSEvents and not kqueue
//!
//! `ReadDirectoryChangesW` is a subscription: the kernel speaks and nothing here
//! asks. That is the whole reason `DirWatch` is allowed to exist under
//! `docs/DESIGN.md` §7.1.3g ② (R31) — a repository is not read because time
//! passed. FSEvents has the same shape and the same guarantee; `kqueue` does
//! not, because a `kqueue` subscription is per **file descriptor**, so watching
//! a tree means opening a descriptor for every directory in it and re-walking
//! the tree whenever one appears. That is not a watcher, it is a crawler with a
//! trigger.
//!
//! # The four contracts, and the one that costs something here
//!
//! **`Tree` is free.** FSEvents is recursive by nature: a stream is created over
//! a root and reports everything beneath it. `start` is therefore the natural
//! shape of the API, and the notification it delivers is the same *something
//! changed* the Windows arm delivers — the paths are read but the caller of
//! `start` never sees them, because its next move is to ask `git status`, which
//! is the one thing that can say what a change means.
//!
//! **`HereOnly` is a filter, and the filter is what it costs.** FSEvents cannot
//! be told to stop at one level — there is no `bWatchSubtree` to pass — so the
//! stream is still recursive and the depth is decided here, on the watcher
//! thread, by asking whether the changed path's parent *is* the watched root.
//! What that costs, said out loud: a README previewed out of a repository root
//! still wakes this thread for every object file a build writes into `target/`,
//! and pays one CFString decode and one prefix comparison for each. On Windows
//! the kernel never generated those events at all. Two things keep the bill
//! small and neither is a mitigation invented here — FSEvents coalesces per path
//! inside the latency window ([`LATENCY_SECONDS`]), so a file written a hundred
//! times in a second arrives once; and the work per path is a comparison on a
//! thread that does nothing else, which is orders of magnitude cheaper than the
//! wake-up it prevents. That was the measured argument for reading the names at
//! all (`docs/DESIGN.md` §7.1.3k: 400 files into `target\` cost the window thread
//! 188ms of CPU in 655ms), and it is the same argument one layer down.
//!
//! **Named `HereOnly` is the same filter plus the names**, which is the door
//! `preview_watch` opens: it is watching *one file* through a subscription to the
//! folder it lives in, so it needs the entries by name to answer "is this mine"
//! before a mailbox is touched. The atomic replace an editor performs — write a
//! temporary, rename it over the target — arrives as
//! [`ITEM_RENAMED`] on **both** paths with
//! `kFSEventStreamCreateFlagFileEvents` on, so the destination's own name is in
//! the batch and the caller's name filter matches it. Nothing here special-cases
//! a rename; what makes it work is that the flag is not consulted at all for
//! naming, only the path is, and the renamed-over file's path is one of the paths.
//!
//! # The thread, and why a run loop rather than a dispatch queue
//!
//! A stream has to be scheduled somewhere, and FSEvents offers two: a run loop
//! (`FSEventStreamScheduleWithRunLoop`) or a GCD queue
//! (`FSEventStreamSetDispatchQueue`). **This arm takes a dedicated thread and
//! its own run loop**, for three reasons that are all the same reason:
//!
//! 1. It is the Windows arm restated. There, one watcher thread per set waits
//!    on the set's completion port, `start` waits for that thread to say the
//!    member is listening, and a removal is *post a request and wait for the
//!    word*. Here, one watcher thread per set owns every member's stream,
//!    `start` waits for it to say the stream is running, and a removal is
//!    signal and wait. A reader who knows one knows the other.
//! 2. **Teardown happens on the thread that scheduled the stream.** That is the
//!    hazard this ticket names: `FSEventStreamInvalidate` from a thread other
//!    than the scheduling one races the callback it is trying to stop. With a
//!    dedicated run loop there is no way to write that bug — the owner cannot
//!    reach a stream at all, only the run loop, and `Stop` / `Invalidate` /
//!    `Release` run on the watcher thread between two runs of its loop, where no
//!    callback is in flight ([`tear_down`]).
//! 3. A dispatch queue would need the same rendezvous anyway (`SetDispatchQueue`
//!    with `NULL` before invalidating, and a `dispatch_sync` to know when the
//!    last callback has finished), spelled with a second concurrency primitive.
//!
//! **One thread per set, one stream per member** (T-DIR-WATCH-ONE-THREAD,
//! 2026-10-26). Until then the cost was one thread per watch, and the PATH ear
//! alone opened one per `PATH` directory. A [`DirWatchSet`] carries any number of
//! streams on its one run loop; the watch steward holds one set for the process.
//! Each member keeps a stream of its own rather than a share of one stream over a
//! path array, because a stream's paths are fixed when it is created — an
//! arrival would mean a new stream replaying history from the old one's last
//! event — and because each member's root, depth filter and `WATCH_ROOT` end are
//! read against its own stream exactly as before. The cost the ticket was about
//! is the thread, and that is one.

//! # A request is a run loop source, not a flag
//!
//! `CFRunLoopStop` on a loop that has not started yet does nothing, and the
//! window between "the watcher thread said it is running" and "the watcher
//! thread is inside `CFRunLoopRun`" is exactly where a caller that drops
//! immediately would fall. So an owner does not stop the loop; it queues its
//! request (an arrival, a removal, the stop) and **signals a source**, and a
//! signalled source is *pending* whether or not the loop is running — it fires
//! the moment the loop starts. The source's perform callback is what calls
//! `CFRunLoopStop`, on the loop's own thread, and the thread answers the queue
//! before it runs again. There is no window and there is no timer.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::admission::WorkerCtx;
use crate::watch_end::WatchEnd;

use objc2_core_foundation::{
    CFArray, CFIndex, CFRetained, CFRunLoop, CFRunLoopMode, CFRunLoopSource,
    CFRunLoopSourceContext, CFString, kCFRunLoopDefaultMode,
};

// ── the FSEvents entry points ──────────────────────────────────────────────
//
// Declared here rather than taken from a crate, for `docs/DESIGN.md` §8's
// reason: `objc2-core-foundation` is already in `Cargo.lock` (`objc2-foundation`
// pulls it in for `NSGeometry`) and carries every Core Foundation type below,
// while FSEvents itself lives in CoreServices and has no binding in that family.
// A package for seven C declarations would be a line in `THIRD-PARTY-NOTICES.md`
// bought with nothing — and this crate is the workspace's declared unsafe
// boundary, which is precisely the place seven C declarations belong.

/// The stream object itself, which is opaque and is only ever held as a pointer.
#[repr(C)]
struct FsEventStream {
    _opaque: [u8; 0],
}

/// `FSEventStreamRef`.
type FsEventStreamRef = *mut FsEventStream;

/// `FSEventStreamEventId` — the monotonic identity the FSEvents daemon gives
/// every event it records.
type FsEventStreamEventId = u64;

/// `FSEventStreamCreateFlags`.
type FsEventStreamCreateFlags = u32;

/// `FSEventStreamEventFlags` — what one event says about itself.
type FsEventStreamEventFlags = u32;

/// `FSEventStreamContext`: the `info` pointer the callback is handed back, and
/// three memory-management callbacks this arm does not use because the pointer
/// outlives the stream by construction.
#[repr(C)]
struct FsEventStreamContext {
    version: CFIndex,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    copy_description: Option<unsafe extern "C" fn(*const c_void) -> *const CFString>,
}

/// `FSEventStreamCallback`. `event_paths` is a `CFArrayRef` of `CFStringRef`
/// because [`USE_CF_TYPES`] is on.
type FsEventStreamCallback = unsafe extern "C" fn(
    stream: FsEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const FsEventStreamEventFlags,
    event_ids: *const FsEventStreamEventId,
);

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: FsEventStreamCallback,
        context: *const FsEventStreamContext,
        paths_to_watch: &CFArray,
        since_when: FsEventStreamEventId,
        latency: f64,
        flags: FsEventStreamCreateFlags,
    ) -> FsEventStreamRef;

    fn FSEventStreamScheduleWithRunLoop(
        stream: FsEventStreamRef,
        run_loop: &CFRunLoop,
        run_loop_mode: &CFRunLoopMode,
    );

    fn FSEventStreamStart(stream: FsEventStreamRef) -> u8;
    fn FSEventStreamStop(stream: FsEventStreamRef);
    fn FSEventStreamInvalidate(stream: FsEventStreamRef);
    fn FSEventStreamRelease(stream: FsEventStreamRef);
}

/// `kFSEventStreamCreateFlagUseCFTypes` — the callback is handed a `CFArray` of
/// `CFString` rather than a `char **`, so the paths arrive as objects with a
/// length and this arm never walks a NUL-terminated array of pointers.
const USE_CF_TYPES: FsEventStreamCreateFlags = 0x0000_0001;

/// `kFSEventStreamCreateFlagNoDefer` — **the first event of a quiet period is
/// delivered at once** and only the ones behind it are held for the latency.
/// Without it the latency would be a delay on every notification, including the
/// one a reader caused by pressing Ctrl+S, and this watcher is not allowed to be
/// slower than the one it replaces.
const NO_DEFER: FsEventStreamCreateFlags = 0x0000_0002;

/// `kFSEventStreamCreateFlagWatchRoot` — report when the watched root itself is
/// created, deleted, renamed or moved, which is the only way
/// [`ROOT_CHANGED`] is ever set. The ticket's flag list does not
/// name it and the root-replacement behaviour it asks for cannot be delivered
/// without it: a root that is renamed away produces no item event inside a tree
/// that no longer exists.
const WATCH_ROOT: FsEventStreamCreateFlags = 0x0000_0004;

/// `kFSEventStreamCreateFlagFileEvents` — one event per **item** rather than one
/// per directory. It is what makes the two shallow contracts possible at all: a
/// directory-granular stream would say "something under this folder moved" and
/// the entry names the named contract exists to deliver would have to be
/// recovered by listing the directory, which is the disk read this whole
/// mechanism is built to avoid.
const FILE_EVENTS: FsEventStreamCreateFlags = 0x0000_0010;

/// `kFSEventStreamEventIdSinceNow`.
///
/// **The daemon keeps a log, and this arm deliberately does not read it.** An
/// event id older than now would replay history: every caller in this workspace
/// arms a watch and then immediately lists or reads the very thing it armed, so
/// replayed events are not news, they are the same news twice — and on a
/// repository root the "history" is every build since the machine was last
/// booted. `SinceNow` is resolved by the daemon at `FSEventStreamStart`, which is
/// what makes the readiness promise below say what it says: `start` returns after
/// that call, so the instant `start` returns is the instant the stream is
/// speaking from.
const SINCE_NOW: FsEventStreamEventId = u64::MAX;

/// `kFSEventStreamEventFlagMustScanSubDirs` — too much happened under this path
/// to write down; look again.
const MUST_SCAN_SUB_DIRS: FsEventStreamEventFlags = 0x0000_0001;
/// `kFSEventStreamEventFlagUserDropped` — the events were lost in this process.
const USER_DROPPED: FsEventStreamEventFlags = 0x0000_0002;
/// `kFSEventStreamEventFlagKernelDropped` — the events were lost in the kernel.
const KERNEL_DROPPED: FsEventStreamEventFlags = 0x0000_0004;
/// `kFSEventStreamEventFlagRootChanged` — the watched root itself appeared,
/// disappeared or moved (needs [`WATCH_ROOT`]).
const ROOT_CHANGED: FsEventStreamEventFlags = 0x0000_0020;
/// `kFSEventStreamEventFlagUnmount` — the volume mounted at the event's path was
/// unmounted. When that volume holds the watched root, the watch has nothing left
/// to listen to.
const UNMOUNT: FsEventStreamEventFlags = 0x0000_0080;
/// `kFSEventStreamEventFlagItemRemoved`.
const ITEM_REMOVED: FsEventStreamEventFlags = 0x0000_0200;
/// `kFSEventStreamEventFlagItemRenamed` — set on **both** halves of a rename,
/// which is what makes an editor's write-temporary-and-rename-over arrive as a
/// change of the file that was replaced.
const ITEM_RENAMED: FsEventStreamEventFlags = 0x0000_0800;
/// `kFSEventStreamEventFlagItemIsDir`.
const ITEM_IS_DIR: FsEventStreamEventFlags = 0x0002_0000;

/// The three flags that all mean the same thing: *something changed and nothing
/// can say what*.
const LOST_TRACK: FsEventStreamEventFlags = MUST_SCAN_SUB_DIRS | USER_DROPPED | KERNEL_DROPPED;

/// **How long FSEvents may hold events back before delivering a batch**, in
/// seconds — and it is `bt_app::watch_clock::WATCH_QUIET`'s own number, 300ms.
///
/// The debounce this product runs on is portable and lives above this crate: a
/// directory has to hold still for `WATCH_QUIET` before its news is acted on,
/// with a `WATCH_FLOOR` of two seconds so that a build in progress is still
/// answered. Nothing here duplicates that arithmetic — `watch_clock`'s own
/// header says why two copies of a debounce is how two surfaces come to disagree
/// about what "it stopped changing" means.
///
/// What the latency does is coalesce *below* it. With [`NO_DEFER`] the first
/// event of a burst arrives immediately, so the reader who pressed Ctrl+S is
/// answered on the same schedule Windows answers them on; the rest of the burst
/// arrives in batches no closer together than this, which is the interval the
/// layer above was going to collapse them into anyway. Choosing the same number
/// is therefore not a coincidence to be maintained, it is the statement that this
/// stream must never deliver more finely than the policy that consumes it — a
/// smaller number would buy wake-ups nobody reads, and a larger one would start
/// deciding "it stopped changing" down here, where the decision does not belong.
const LATENCY_SECONDS: f64 = 0.300;

// ── the four contracts ─────────────────────────────────────────────────────

/// **How far under a watched directory a notification is allowed to come from.**
///
/// Private, exactly as it is on Windows and for the reason the backend inventory
/// gives (§6 ⑤): the four contracts are the four constructors, and the enum is
/// the value one of them passes rather than a thing a caller names. It carries
/// no `recursive()` here because FSEvents is not asked — the stream is always
/// recursive and this is what the watcher thread does with what arrives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchDepth {
    /// The directory and everything under it — a working tree.
    Tree,
    /// The directory's own entries and nothing deeper — the folder one watched
    /// file happens to live in.
    HereOnly,
}

/// **What one batch of FSEvents said changed.**
///
/// The twin of `windows_impl::DirChange`, and the same two answers, because the
/// callers are the same callers: the kernel — either one — says *these entries*
/// or *more than I could write down*, and never "nothing changed".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirChange<'a> {
    /// The entries that moved, relative to the watched directory. A tree watch
    /// spells a nested change the way `FILE_NOTIFY_INFORMATION` does — the whole
    /// relative path, separators and all — and a shallow watch, by construction,
    /// only ever names one component.
    Named(&'a [OsString]),
    /// FSEvents lost track. Something changed; nothing can say what. **A name
    /// filter must let this through**, because it is exactly the burst that was
    /// too big to write down.
    Unknown,
    /// **The watch ended on its own** (T-DIRWATCH-LIFECYCLE): the watched root
    /// changed (removed, renamed, moved) or its volume was unmounted. The last
    /// call `wake` gets; the watch is no longer armed and
    /// [`DirWatch::take_failure`] holds why. A drop is not an end and sends
    /// nothing.
    Ended,
}

impl DirChange<'_> {
    /// **What a nameless door says**: every change is [`Self::Unknown`] —
    /// *something moved* — and the end is still the end. The two nameless doors
    /// hand their callers this, so the names never leave the door.
    #[must_use]
    pub fn nameless(self) -> DirChange<'static> {
        match self {
            Self::Ended => DirChange::Ended,
            Self::Named(_) | Self::Unknown => DirChange::Unknown,
        }
    }
}

/// **A subscription to a directory's change notifications, over FSEvents.**
///
/// One stream, scheduled on the run loop of the [`DirWatchSet`] it is a member of —
/// the set it was started on, or for the four `DirWatch::start*` doors a set of one
/// that it owns. See the module header for why it is a run loop rather than a
/// dispatch queue, and why every request is a run loop source rather than a flag.
///
/// **It returns already listening.** Not "a thread has been started that will
/// begin listening": when a constructor hands back a `DirWatch`,
/// `FSEventStreamStart` has returned `true` on the watcher thread and the
/// daemon has resolved [`SINCE_NOW`] to a real event id, so every change from
/// that instant onwards is news the caller will be told about. The promise has to
/// be that strong for the same reason it is on Windows: every caller's next move
/// is to touch or to list the very directory it has just armed.
///
/// **Retiring it cancels** ([`DirWatch::retire_within`], on the worker that
/// holds it), and the cancellation is complete when the door answers `true`:
/// the watcher thread is signalled, it stops and invalidates and releases the
/// stream on its own run loop's thread, and says the member is gone. After that
/// there is no callback of this member in flight, because there is no stream. A
/// drop without the door signals and waits for nothing.
///
/// **It can also end on its own** (T-DIRWATCH-LIFECYCLE): FSEvents saying the
/// root changed, or that the volume holding it was unmounted, ends the member.
/// The error is kept in `crate::watch_end::WatchEnd`, [`DirWatch::is_armed`]
/// turns false, and `wake` is called once with [`DirChange::Ended`]. The set's
/// other members go on listening.
pub struct DirWatch {
    /// What the watcher thread says about this member: its end, and whether its
    /// stream is gone.
    member: Arc<WatchMember>,
    /// The removal of this member, posted to its set's run loop. Held by the
    /// owner so that the door (or a drop) can signal without reaching the stream.
    stopper: Stopper,
    /// The set of one the four `DirWatch::start*` doors open, owned by the one
    /// member it carries and retired with it.
    own: Option<DirWatchSet>,
    /// Not yet retired through its door.
    live: bool,
}

/// **One member's removal, ready to be posted to its set's run loop.**
struct Stopper {
    set: Arc<SetShared>,
    id: u64,
}

impl Stopper {
    /// Ask the watcher thread to take this member's stream down.
    ///
    /// A signalled source is **pending**: if the run loop is not running yet the
    /// signal is remembered and fires the instant it does.
    fn signal(&self) {
        self.set.post(Request::Remove(self.id));
    }
}

/// **One member, as its owner and the watcher thread both see it.**
#[derive(Default)]
struct WatchMember {
    /// Its end, if it came to one on its own.
    end: WatchEnd,
    /// Its stream is stopped, invalidated and released, and its subscriber freed.
    gone: AtomicBool,
}

impl WatchMember {
    fn is_gone(&self) -> bool {
        self.gone.load(Ordering::Acquire)
    }
}

impl DirWatch {
    /// Whether the watcher is still listening: a returned watcher has started
    /// its stream, and stays armed until it ends on its own.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.member.end.still_armed()
    }

    /// The error that ended the watcher on its own, handed over once.
    pub fn take_failure(&mut self) -> Option<std::io::Error> {
        self.member.end.take_failure()
    }

    /// **The door: stop the watch, and wait — up to `bound`, on the worker `ctx`
    /// lends — for its stream to be gone.** Answers whether it went in time
    /// (T-DROP-DOORS, D-40): *a directory watch is retired through its bounded
    /// door on the worker that holds it, never by its drop.*
    ///
    /// The thread tears its own stream down: the removal is posted to the set's
    /// run loop, and an `FSEventStreamInvalidate` from here would be a second
    /// thread reaching into a stream whose callback may be running on the first.
    /// A stream still standing past `bound` is let go, and goes on its own.
    ///
    /// A watch opened by one of the four `DirWatch::start*` doors owns the set of
    /// one it was started on, and the door retires that set too — the join of its
    /// one thread, within what is left of `bound`.
    pub fn retire_within(
        mut self,
        ctx: &crate::admission::WorkerCtx,
        bound: std::time::Duration,
    ) -> bool {
        self.live = false;
        let deadline = std::time::Instant::now() + bound;
        if !self.member.is_gone() {
            self.stopper.signal();
        }
        while !self.member.is_gone() {
            if std::time::Instant::now() >= deadline {
                if let Some(set) = self.own.take() {
                    set.let_go();
                }
                return false;
            }
            crate::wait::sleep_within(ctx, crate::DIR_WATCH_RETIRE_POLL);
        }
        match self.own.take() {
            Some(set) => set.retire_within(
                ctx,
                deadline.saturating_duration_since(std::time::Instant::now()),
            ),
            None => true,
        }
    }

    /// Start watching `path` and everything under it.
    ///
    /// `wake` is called on the watcher thread, once per batch, with
    /// [`DirChange::Unknown`] (this door reads no names) and, if the watch ends
    /// on its own, once more with [`DirChange::Ended`]. It is expected to
    /// do nothing but record the news and nudge whatever loop is going to act on
    /// it — the same haste the Windows arm asks for, and for a reason that
    /// survives the platform change: this thread is the only thing between the
    /// daemon's queue and a dropped event.
    ///
    /// **Failure is quiet and final.** A path that is not there, a directory this
    /// process may not open: the answer is to have no watcher for that
    /// repository, not to try again in a moment, because retrying is a timer and
    /// a timer is the thing this whole mechanism exists to avoid. `NotFound` is
    /// answered as `NotFound` — `scheme_watch` reads that distinction to keep a
    /// fresh install's missing `schemes` folder off stderr.
    ///
    /// **Only a worker may start one** (`ctx`, the thread door's capability):
    /// the wait for the stream and the join on refusal happen here, and the
    /// window thread never waits (B5 design note, revision (c)3).
    ///
    /// This door opens a [`DirWatchSet`] of one for the watch, which the watch
    /// owns; a caller with many directories opens one set and starts each on it,
    /// so that they share its one thread.
    pub fn start(
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(ctx, path, WatchDepth::Tree, move |change| {
            wake(change.nameless())
        })
    }

    /// **The same tree subscription, told each relative path that moved.**
    ///
    /// FSEvents already supplies paths below the root. The palette index uses them to refuse
    /// generated subtrees with the same pure rule as its walk, while an overflow remains
    /// [`DirChange::Unknown`] and is conservatively admitted.
    pub fn start_named(
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(ctx, path, WatchDepth::Tree, wake)
    }

    /// The same subscription over **this directory and no deeper**.
    ///
    /// On Windows this is one boolean handed to the kernel. Here it is a filter
    /// on the watcher thread, because FSEvents has no depth argument — see the
    /// module header for what that costs and why it is still the right shape.
    /// The caller that wants it is the files column: the folders whose names are
    /// on the glass, and a collapsed `target/` is not one of them.
    pub fn start_shallow(
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(ctx, path, WatchDepth::HereOnly, move |change| {
            wake(change.nameless())
        })
    }

    /// **The same shallow subscription, told which entry moved.**
    ///
    /// `preview_watch`'s door: `wake` is handed the [`DirChange`] this batch
    /// carried, so a folder full of things nobody is reading can be answered on
    /// the watcher thread and cost the event loop nothing at all. An editor's
    /// atomic replace — write a temporary, rename it over the target — names the
    /// target, because the rename is an event on the target's own path.
    pub fn start_shallow_named(
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(ctx, path, WatchDepth::HereOnly, wake)
    }

    /// **A set of one, owned by the watch it carries.** The root is resolved
    /// first, so a path that cannot be watched is refused before any thread
    /// exists; a stream that is refused retires the set it opened, so no thread is
    /// left behind the error.
    fn start_scoped(
        ctx: &WorkerCtx,
        path: &Path,
        depth: WatchDepth,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        let root = resolve(ctx, path)?;
        let set = DirWatchSet::open(ctx)?;
        match set.enlist(ctx, root, depth, wake) {
            Ok(mut watch) => {
                watch.own = Some(set);
                Ok(watch)
            }
            Err(error) => {
                let _ = set.retire_within(ctx, crate::DIR_WATCH_RETIRE_BUDGET);
                Err(error)
            }
        }
    }
}

impl Drop for DirWatch {
    /// Waits for nothing. After [`DirWatch::retire_within`] there is nothing
    /// left; without it, the stream is signalled away and let go of — the
    /// watcher thread tears its own stream down (a set of one is told to stop) —
    /// and the drop says so once.
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        match self.own.take() {
            Some(set) => set.let_go(),
            None => {
                if !self.member.is_gone() {
                    self.stopper.signal();
                }
            }
        }
        crate::door_skipped::say(
            "a directory watch (`DirWatch`)",
            "its retire door",
            "its watcher was told to stop and let go of",
        );
    }
}

/// **Many directory watches on one thread, over FSEvents** (T-DIR-WATCH-ONE-THREAD).
///
/// One `bt-dir-watch` thread and its run loop, carrying one `FSEventStream` per
/// member: each member keeps its own root, its own depth filter, its own names and
/// its own `WATCH_ROOT` end, exactly as a `DirWatch` with a thread of its own did,
/// and a stream is created, scheduled, started, stopped, invalidated and released
/// on this one thread. Every request the owners make — an arrival
/// ([`DirWatchSet::start`] and its three siblings, which return already listening),
/// a removal ([`DirWatch::retire_within`]) and the set's stop
/// ([`DirWatchSet::retire_within`]) — is queued and the run loop's request source
/// signalled; the source stops the loop, and the thread answers the queue between
/// two runs, where no FSEvents callback can be in flight.
///
/// **Every `wake` runs on the one thread, one after another.** The contract
/// already asks each to filter, stamp and wake and nothing slower; the daemon
/// holds a member's events while another member's `wake` runs, and the worst a
/// long wait costs it is a batch that says it lost track — [`DirChange::Unknown`],
/// news every caller already takes.
///
/// **Retiring it stops every member and joins the thread.** Members still held
/// after that answer their own door at once: their streams went with the set.
pub struct DirWatchSet {
    set: Arc<SetShared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// What a set's owners and its thread share: the queue of requests, and the run
/// loop with the source that wakes it for them.
struct SetShared {
    requests: Mutex<Requests>,
    next: AtomicU64,
    run_loop: CFRetained<CFRunLoop>,
    source: CFRetained<CFRunLoopSource>,
}

// SAFETY: the queue is behind its mutex. `CFRunLoopSourceSignal`,
// `CFRunLoopWakeUp` and `CFRetain`/`CFRelease` are documented thread-safe, and
// those are the only calls an owner makes on the run loop or the source;
// `CFRunLoopStop` is called by the source's perform callback, on the loop's own
// thread. The pair is created on the watcher thread, handed here once, and the
// thread removes and invalidates the source before it returns.
unsafe impl Send for SetShared {}
unsafe impl Sync for SetShared {}

struct Requests {
    /// Whether the thread still takes requests. Cleared, under this lock, by the
    /// thread as it leaves, before it drops what is still queued — so an arrival is
    /// either refused here or answered there, and never left waiting on a thread
    /// that has gone.
    open: bool,
    queue: VecDeque<Request>,
}

/// What an owner asks of the set's thread.
enum Request {
    /// A member's arrival: its stream is to be created, scheduled and started,
    /// and `armed` told whether it is running.
    Add {
        id: u64,
        root: PathBuf,
        depth: WatchDepth,
        wake: Box<dyn Fn(DirChange<'_>) + Send>,
        member: Arc<WatchMember>,
        armed: std::sync::mpsc::Sender<Result<(), std::io::Error>>,
    },
    /// A member's removal.
    Remove(u64),
    /// The set's stop.
    Stop,
}

impl SetShared {
    /// Queue a request and wake the run loop for it. Answers whether it was
    /// queued: a set whose thread has left takes nothing.
    fn post(&self, request: Request) -> bool {
        {
            let mut requests = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
            if !requests.open {
                return false;
            }
            requests.queue.push_back(request);
        }
        self.source.signal();
        self.run_loop.wake_up();
        true
    }
}

impl DirWatchSet {
    /// **Open a set: its one `bt-dir-watch` thread and run loop**, which says it is
    /// ready to take members before this returns.
    ///
    /// **Only a worker may open one** (`ctx`), as only a worker may start a
    /// watch.
    pub fn open(_ctx: &WorkerCtx) -> Result<Self, std::io::Error> {
        let (ready, opened) = std::sync::mpsc::channel::<Result<Arc<SetShared>, std::io::Error>>();
        let thread =
            crate::spawn_at_priority("bt-dir-watch", crate::ThreadPriority::Normal, move |_ctx| {
                watch_loop(&ready);
            })?;
        // A `RecvError` is the thread ending without a word, which is a panic
        // between the spawn and the run loop — there is no return path there that
        // does not send. The watcher is not contained, so under the resident
        // panic hook that panic ends the process before the channel closes: this
        // arm is reached only where no fatal hook is installed.
        match opened.recv() {
            Ok(Ok(set)) => Ok(Self {
                set,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(std::io::Error::other(
                    "the directory watcher ended before it listened",
                ))
            }
        }
    }

    /// Whether the set's thread has finished, so the set hears nothing more and
    /// starts nothing.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }

    /// [`DirWatch::start`]'s contract, as a member of this set.
    pub fn start(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        self.start_member(ctx, path, WatchDepth::Tree, move |change| {
            wake(change.nameless())
        })
    }

    /// [`DirWatch::start_named`]'s contract, as a member of this set.
    pub fn start_named(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        self.start_member(ctx, path, WatchDepth::Tree, wake)
    }

    /// [`DirWatch::start_shallow`]'s contract, as a member of this set.
    pub fn start_shallow(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        self.start_member(ctx, path, WatchDepth::HereOnly, move |change| {
            wake(change.nameless())
        })
    }

    /// [`DirWatch::start_shallow_named`]'s contract, as a member of this set.
    pub fn start_shallow_named(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        self.start_member(ctx, path, WatchDepth::HereOnly, wake)
    }

    /// **The door: stop every member, and wait — up to `bound`, on the worker `ctx`
    /// lends — for the set's thread to leave.** Answers whether it left in time.
    /// The thread tears every stream down on its own run loop and returns; a
    /// thread still standing past `bound` is let go and finishes on its own.
    pub fn retire_within(mut self, ctx: &WorkerCtx, bound: std::time::Duration) -> bool {
        self.set.post(Request::Stop);
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let deadline = std::time::Instant::now() + bound;
        while !thread.is_finished() {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            crate::wait::sleep_within(ctx, crate::DIR_WATCH_RETIRE_POLL);
        }
        let _ = thread.join();
        true
    }

    /// Stopped and let go of, waiting for nothing and saying nothing: the set of
    /// one a watch owned, when the watch itself was let go of.
    fn let_go(mut self) {
        self.set.post(Request::Stop);
        drop(self.thread.take());
    }

    /// **One member started**: its root resolved here, on the worker, and its
    /// stream created and started by the set's thread, which says so before this
    /// returns.
    fn start_member(
        &self,
        ctx: &WorkerCtx,
        path: &Path,
        depth: WatchDepth,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        self.enlist(ctx, resolve(ctx, path)?, depth, wake)
    }

    /// **A resolved root joins the set**: handed to the set's thread, and its
    /// stream running when this returns.
    fn enlist(
        &self,
        _ctx: &WorkerCtx,
        root: PathBuf,
        depth: WatchDepth,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<DirWatch, std::io::Error> {
        let id = self.set.next.fetch_add(1, Ordering::Relaxed);
        let member = Arc::new(WatchMember::default());
        // The word that makes `start` mean what it says: the watcher thread sends
        // it once, the instant the stream is running — or sends the refusal
        // instead, which is this function's error and not a member that ends in
        // private after the caller has been told it has a watcher.
        let (armed, listening) = std::sync::mpsc::channel::<Result<(), std::io::Error>>();
        if !self.set.post(Request::Add {
            id,
            root,
            depth,
            wake: Box::new(wake),
            member: Arc::clone(&member),
            armed,
        }) {
            return Err(std::io::Error::other(
                "the directory watch set takes no more directories",
            ));
        }
        // A `RecvError` is the arrival dropped unanswered, which only the set's
        // thread leaving does — the set was stopped, or its thread panicked, which
        // under the resident panic hook ends the process first.
        match listening.recv() {
            Ok(Ok(())) => Ok(DirWatch {
                member,
                stopper: Stopper {
                    set: Arc::clone(&self.set),
                    id,
                },
                own: None,
                live: true,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(std::io::Error::other(
                "the directory watcher ended before it listened",
            )),
        }
    }
}

/// **A root resolved for watching**, or the refusal that is its start's error.
/// It reads the file system, so only a worker resolves one (`ctx`).
fn resolve(_ctx: &WorkerCtx, path: &Path) -> Result<PathBuf, std::io::Error> {
    // **The root is resolved before the stream is created, and this is not a
    // convenience.** FSEvents reports fully resolved paths — `/tmp` is a
    // symbolic link to `/private/tmp` and the daemon says `/private/tmp` —
    // so a depth filter written against the caller's spelling of the root
    // would reject every event it was given. `realpath(3)` on macOS also
    // answers with the volume's own spelling of each component, which is
    // what makes the comparison exact on a case-insensitive APFS volume.
    //
    // It is also where the refusals come from, and they are the refusals the
    // caller is already written to survive: a missing directory is
    // `NotFound`, an unreadable one is `PermissionDenied`, an embedded NUL is
    // `InvalidInput`. `FSEventStreamCreate` refuses none of these — it
    // accepts a path that does not exist and simply never speaks — so
    // without this a caller would be handed a watcher that is not watching.
    let root = std::fs::canonicalize(path)?;
    if !std::fs::metadata(&root)?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "a directory watch needs a directory",
        ));
    }
    Ok(root)
}

impl Drop for DirWatchSet {
    /// Waits for nothing. After [`DirWatchSet::retire_within`] there is nothing
    /// left; without it, the thread is told to stop and let go of — it tears its
    /// own streams down — and the drop says so once.
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.set.post(Request::Stop);
            drop(thread);
            crate::door_skipped::say(
                "a directory watch set (`DirWatchSet`)",
                "its retire door",
                "its watcher was told to stop and let go of",
            );
        }
    }
}

// ── the watcher thread ─────────────────────────────────────────────────────

/// What the watcher thread keeps for one stream, and what the FSEvents callback
/// is handed back through the stream context's `info` pointer.
///
/// Every field is touched by one thread only — the watcher's — because the
/// callback runs on the run loop this same thread is blocked in. The `RefCell`
/// is therefore never contended; it is here so that the names buffer can be
/// reused across batches without the callback needing a `&mut` it has no way to
/// obtain through a `*mut c_void`.
struct Subscriber {
    /// The resolved root every event path is measured against.
    root: PathBuf,
    depth: WatchDepth,
    /// The caller's callback.
    wake: Box<dyn Fn(DirChange<'_>) + Send>,
    /// Reused across batches so that a busy folder does not allocate per
    /// notification.
    names: RefCell<Vec<OsString>>,
    /// Set when the root itself went away, and read by the loop below.
    root_gone: AtomicBool,
    /// The run loop to stop when it does.
    run_loop: CFRetained<CFRunLoop>,
}

/// **One running stream, as the watcher thread holds it.**
struct Running {
    stream: NonNull<FsEventStream>,
    /// The `Box` leaked into the stream's context, freed after the stream is
    /// released.
    subscriber: *mut Subscriber,
    member: Arc<WatchMember>,
}

impl Running {
    fn root_gone(&self) -> bool {
        // SAFETY: the subscriber is the live `Box` leaked when the stream was
        // created, freed only in `tear_down`, which consumes this.
        unsafe { &*self.subscriber }
            .root_gone
            .load(Ordering::Acquire)
    }

    /// **The root went, and nobody asked for a stop**: the member has ended on its
    /// own. Said before the teardown, from this thread, as the last call `wake`
    /// gets; a removal the owner asked for says nothing.
    fn end(self) {
        self.member.end.record(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the watched directory was removed, moved or unmounted",
        ));
        // SAFETY: as in `root_gone`.
        (unsafe { &*self.subscriber }.wake)(DirChange::Ended);
        self.leave();
    }

    /// The stream torn down on the thread that scheduled it, and the member told
    /// last, after the `wake` it carried is gone too.
    fn leave(self) {
        tear_down(self.stream, self.subscriber);
        self.member.gone.store(true, Ordering::Release);
    }
}

/// **On the thread that scheduled it**, and in this order: stop delivering,
/// unschedule, release, and only then free what the callback reads. Anything
/// else is a callback running against a `Subscriber` that has been freed.
fn tear_down(stream: NonNull<FsEventStream>, subscriber: *mut Subscriber) {
    // SAFETY: this is the thread that scheduled the stream, no callback of it is
    // running (the thread is between two runs of its loop), and nothing holds the
    // stream afterwards.
    unsafe {
        FSEventStreamStop(stream.as_ptr());
        FSEventStreamInvalidate(stream.as_ptr());
        FSEventStreamRelease(stream.as_ptr());
    }
    // SAFETY: the stream is invalid and released, so the callback that held this
    // pointer can no longer run. The pointer came from `Box::into_raw` and has not
    // been freed.
    drop(unsafe { Box::from_raw(subscriber) });
}

/// The watcher thread: make the run loop's request source, say the set is ready,
/// and then be the run loop — answering the queue between two runs, and taking
/// down every stream whose root went (T-DIRWATCH-LIFECYCLE) — until the stop.
fn watch_loop(ready: &std::sync::mpsc::Sender<Result<Arc<SetShared>, std::io::Error>>) {
    let run_loop = CFRunLoop::current().expect("every thread has a run loop");
    // SAFETY: `kCFRunLoopDefaultMode` is a Core Foundation constant, initialised
    // before any code of this process runs and never written to.
    let mode = unsafe { kCFRunLoopDefaultMode }
        .expect("the default run loop mode is a Core Foundation constant");

    let Some(source) = request_source() else {
        let _ = ready.send(Err(std::io::Error::other(
            "a run loop source for the watcher's requests could not be created",
        )));
        return;
    };
    run_loop.add_source(Some(&source), Some(mode));
    let set = Arc::new(SetShared {
        requests: Mutex::new(Requests {
            open: true,
            queue: VecDeque::new(),
        }),
        next: AtomicU64::new(1),
        run_loop: run_loop.clone(),
        source: source.clone(),
    });
    // `open` gave up on us — there is nobody to watch for — and the loop below
    // is skipped for the teardown rather than run until nothing ever stops it.
    let mut stopping = ready.send(Ok(Arc::clone(&set))).is_err();

    let mut running: BTreeMap<u64, Running> = BTreeMap::new();
    while !stopping {
        // `CFRunLoopRun` returns when the request source's perform callback calls
        // `CFRunLoopStop`, or when an FSEvents callback does because a root went;
        // any other return is answered the same way, with whatever is queued.
        CFRunLoop::run();
        let queued = std::mem::take(
            &mut set
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .queue,
        );
        for request in queued {
            match request {
                Request::Add {
                    id,
                    root,
                    depth,
                    wake,
                    member,
                    armed,
                } => {
                    let started = if stopping {
                        Err(std::io::Error::other(
                            "the directory watch set takes no more directories",
                        ))
                    } else {
                        arm(&run_loop, mode, root, depth, wake, member)
                    };
                    match started {
                        Ok(stream) => {
                            running.insert(id, stream);
                            let _ = armed.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = armed.send(Err(error));
                        }
                    }
                }
                Request::Remove(id) => {
                    if let Some(stream) = running.remove(&id) {
                        stream.leave();
                    }
                }
                Request::Stop => stopping = true,
            }
        }
        let ended: Vec<u64> = running
            .iter()
            .filter(|(_, stream)| stream.root_gone())
            .map(|(id, _)| *id)
            .collect();
        for id in ended {
            if let Some(stream) = running.remove(&id) {
                stream.end();
            }
        }
    }

    // Leaving: no request is taken from here on, and whatever was queued before
    // that is dropped — an arrival's dropped answer is its start's error.
    let left = {
        let mut requests = set.requests.lock().unwrap_or_else(PoisonError::into_inner);
        requests.open = false;
        std::mem::take(&mut requests.queue)
    };
    drop(left);
    for (_, stream) in std::mem::take(&mut running) {
        stream.leave();
    }
    run_loop.remove_source(Some(&source), Some(mode));
    source.invalidate();
}

/// **One member's stream**: created, scheduled on this thread's run loop and
/// started, or the refusal that is its start's error.
fn arm(
    run_loop: &CFRetained<CFRunLoop>,
    mode: &CFRunLoopMode,
    root: PathBuf,
    depth: WatchDepth,
    wake: Box<dyn Fn(DirChange<'_>) + Send>,
    member: Arc<WatchMember>,
) -> Result<Running, std::io::Error> {
    let subscriber = Box::into_raw(Box::new(Subscriber {
        root,
        depth,
        wake,
        names: RefCell::new(Vec::new()),
        root_gone: AtomicBool::new(false),
        run_loop: run_loop.clone(),
    }));
    // SAFETY: the pointer is the live `Box` just leaked; its root is read before
    // any stream holds it.
    let stream = create_stream(&unsafe { &*subscriber }.root, subscriber.cast::<c_void>());
    let Some(stream) = stream else {
        // SAFETY: nothing else ever held this pointer — the stream that would
        // have was never created.
        drop(unsafe { Box::from_raw(subscriber) });
        return Err(std::io::Error::other(
            "FSEventStreamCreate refused the watched path",
        ));
    };

    // SAFETY: `stream` was just created and has not been scheduled anywhere;
    // `run_loop` is this thread's own and `mode` is a Core Foundation constant.
    unsafe { FSEventStreamScheduleWithRunLoop(stream.as_ptr(), run_loop, mode) };

    #[cfg(test)]
    stream_gate::wait_if_held();
    // SAFETY: the stream is scheduled on a run loop that exists.
    let started = unsafe { FSEventStreamStart(stream.as_ptr()) } != 0;
    #[cfg(test)]
    if started {
        stream_gate::note_stream_running();
    }

    if !started {
        // SAFETY: the stream was scheduled on this thread's run loop and never
        // started, so no callback can be in flight; this is the thread that
        // scheduled it.
        unsafe {
            FSEventStreamInvalidate(stream.as_ptr());
            FSEventStreamRelease(stream.as_ptr());
        }
        // SAFETY: the stream that held this pointer is invalid and released.
        drop(unsafe { Box::from_raw(subscriber) });
        return Err(std::io::Error::other(
            "FSEventStreamStart could not arm the watch",
        ));
    }
    Ok(Running {
        stream,
        subscriber,
        member,
    })
}

/// `FSEventStreamCreate` over one path, with the four flags this arm depends on.
fn create_stream(root: &Path, info: *mut c_void) -> Option<NonNull<FsEventStream>> {
    // A path that is not UTF-8 cannot become a `CFString`, and macOS file names
    // are UTF-8 by the volume format's own rule — so this is a refusal about a
    // path that cannot exist rather than a limitation of this arm.
    let root = root.to_str()?;
    let path = CFString::from_str(root);
    let paths = CFArray::from_retained_objects(&[path]);
    // SAFETY: an array of `CFString` is an array; the element type is a Rust
    // convenience and `FSEventStreamCreate` takes the untyped `CFArrayRef` the
    // header declares.
    let paths = unsafe { CFRetained::cast_unchecked::<CFArray>(paths) };
    let context = FsEventStreamContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copy_description: None,
    };
    // SAFETY: `paths` is a live `CFArray` of `CFString`, `context` is a valid
    // `FSEventStreamContext` whose `info` outlives the stream (the watcher
    // thread frees it only after `FSEventStreamRelease`), and `on_events` has
    // the signature `FSEventStreamCallback` names.
    let stream = unsafe {
        FSEventStreamCreate(
            std::ptr::null(),
            on_events,
            &raw const context,
            &paths,
            SINCE_NOW,
            LATENCY_SECONDS,
            USE_CF_TYPES | NO_DEFER | WATCH_ROOT | FILE_EVENTS,
        )
    };
    NonNull::new(stream)
}

/// The run loop source whose only job is to stop the loop it is attached to, so
/// that the thread answers the queue.
fn request_source() -> Option<CFRetained<CFRunLoopSource>> {
    /// The source's perform callback. Runs on the run loop's own thread, which
    /// is the watcher thread, which is why `CFRunLoopStop` here is not the
    /// cross-thread call an owner would otherwise have had to make.
    unsafe extern "C-unwind" fn perform(_info: *mut c_void) {
        if let Some(run_loop) = CFRunLoop::current() {
            run_loop.stop();
        }
    }

    let mut context = CFRunLoopSourceContext {
        version: 0,
        info: std::ptr::null_mut(),
        retain: None,
        release: None,
        copyDescription: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(perform),
    };
    // SAFETY: the context is a valid `CFRunLoopSourceContext` for the whole of
    // the call, and its perform callback reads nothing through `info`.
    unsafe { CFRunLoopSource::new(None, 0, &raw mut context) }
}

/// The FSEvents callback: one batch, one reading, at most one `wake`.
unsafe extern "C" fn on_events(
    _stream: FsEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const FsEventStreamEventFlags,
    _event_ids: *const FsEventStreamEventId,
) {
    // SAFETY: `info` is the `Subscriber` the watcher thread leaked into the
    // stream's context, and the stream is released before it is freed.
    let subscriber = unsafe { &*info.cast::<Subscriber>() };
    // SAFETY: `USE_CF_TYPES` is set, so `event_paths` is a `CFArrayRef` of
    // `CFStringRef` holding `num_events` entries, and `event_flags` is an array
    // of that many flags. Both belong to FSEvents for the duration of this call.
    let paths = unsafe { event_paths.cast::<CFArray>().as_ref() };
    let Some(paths) = paths else { return };

    let mut names = subscriber.names.borrow_mut();
    names.clear();
    let mut lost = false;
    let mut root_gone = false;

    for index in 0..num_events {
        // SAFETY: `index` is below `num_events`, which is the length of both
        // arrays FSEvents handed this call.
        let flags = unsafe { *event_flags.add(index) };
        // SAFETY: the array holds `num_events` `CFString`s, and this borrow ends
        // before the callback returns, which is while FSEvents still owns them.
        let path = unsafe {
            paths
                .value_at_index(CFIndex::try_from(index).unwrap_or(CFIndex::MAX))
                .cast::<CFString>()
                .as_ref()
        };
        let Some(path) = path.map(CFString::to_string) else {
            // A path this arm could not read is the overflow's twin: an event
            // arrived, so something moved, and the honest answer is to say so
            // without naming it.
            lost = true;
            continue;
        };
        match read_event(&subscriber.root, subscriber.depth, flags, Path::new(&path)) {
            Reading::LostTrack => lost = true,
            Reading::RootGone => root_gone = true,
            Reading::Entry(name) => names.push(name),
            Reading::NotOurs => {}
        }
    }

    if lost {
        (subscriber.wake)(DirChange::Unknown);
    } else if !names.is_empty() {
        (subscriber.wake)(DirChange::Named(&names));
    }
    drop(names);

    if root_gone {
        // **The answer the Windows arm gives when the root goes**: there is
        // nothing left to watch. There, the next `ReadDirectoryChangesW` fails
        // and the watcher thread records that member's end and closes it. Here
        // the run loop is stopped and `watch_loop` does the same between two
        // runs: it records the end, says `Ended`, and tears this stream down —
        // outside its own callback, on the thread that scheduled it. The set's
        // other streams go on, and the `DirWatch` stays droppable: its door
        // answers at once.
        subscriber.root_gone.store(true, Ordering::Release);
        subscriber.run_loop.stop();
    }
}

/// What one event turned out to be.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Reading {
    /// FSEvents lost track: the whole batch becomes [`DirChange::Unknown`].
    LostTrack,
    /// The watched root itself appeared, disappeared or moved.
    RootGone,
    /// An entry of the watched directory, spelled relative to it.
    Entry(OsString),
    /// Something this contract does not speak for — a path below a shallow
    /// watch's root, or the root itself moving in a way that is not its removal.
    NotOurs,
}

/// **One event, read against one contract.**
///
/// Split out of the callback for the reason every other decision in this
/// workspace is split out of its syscall: it is the whole of what the three
/// contracts *are*, and a table is a better place to argue with it than a live
/// filesystem. It is also the only way the overflow path can be tested at all —
/// FSEvents will not drop events on demand.
///
/// The order of the arms is the order of the claims:
///
/// 1. **Lost track outranks everything**, including the depth filter. An
///    overflow on a path five directories below a shallow watch is still an
///    overflow *of that watch's stream*, and a filter that answered "not mine"
///    to it would drop precisely the burst that was too big to write down.
/// 2. **The root going outranks the names.** Its own removal or rename is
///    reported by `WATCH_ROOT` as [`ROOT_CHANGED`]; the same event is also what
///    a removal of the root looks like from inside, so both spellings are read.
/// 3. **Then the depth**, which is a question about the path and nothing else.
fn read_event(
    root: &Path,
    depth: WatchDepth,
    flags: FsEventStreamEventFlags,
    path: &Path,
) -> Reading {
    if flags & LOST_TRACK != 0 {
        return Reading::LostTrack;
    }
    if flags & ROOT_CHANGED != 0 {
        return Reading::RootGone;
    }
    // The volume holding the root was unmounted: the event names the mount
    // point, which is the root or one of its ancestors. A volume mounted *below*
    // the root going away is an entry of the tree, read by the arms below.
    if flags & UNMOUNT != 0 && root.starts_with(path) {
        return Reading::RootGone;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        // A path outside the root at all. FSEvents does not produce one for a
        // stream over a single path, so this is the shape a root that has been
        // replaced underneath us would take rather than an event to act on.
        return Reading::NotOurs;
    };
    let mut components = relative.components();
    let Some(first) = components.next() else {
        // The root itself. Its removal ends the watch for the same reason
        // `ROOT_CHANGED` does; anything else said about it — an attribute, a
        // modification time — is not a change of an entry and has no name to
        // report.
        if flags & (ITEM_REMOVED | ITEM_RENAMED) != 0 && flags & ITEM_IS_DIR != 0 {
            return Reading::RootGone;
        }
        return Reading::NotOurs;
    };
    let deeper = components.next().is_some();
    match depth {
        // A tree watch spells a nested change whole, exactly as
        // `FILE_NOTIFY_INFORMATION` does. Its one caller reads no names at all,
        // and the shape is kept so that the two arms answer the same question
        // the same way if a second one ever does.
        WatchDepth::Tree => Reading::Entry(relative.as_os_str().to_owned()),
        // A shallow watch speaks for the root's own entries. A path deeper than
        // that is the `target/debug` a build is writing into, and the whole
        // point of the contract is that it costs the loop nothing.
        WatchDepth::HereOnly if deeper => Reading::NotOurs,
        WatchDepth::HereOnly => Reading::Entry(first.as_os_str().to_owned()),
    }
}

/// A `&str` as the entry name a [`Reading`] carries, for the tests below and for
/// nothing else.
#[cfg(test)]
fn entry(name: &str) -> OsString {
    OsString::from(name)
}

/// **Test-only: a place to hold the watcher thread just short of starting the
/// stream.**
///
/// The Windows arm's `first_read_gate`, restated against the call that makes the
/// promise here. The defect both exist to pin is a *window*, not a value: a
/// `start` that returned as soon as the watcher thread had been **spawned**
/// would hand the caller a watch whose stream had not been started, and
/// everything that moved before `FSEventStreamStart` resolved
/// [`SINCE_NOW`] would reach nobody. The window is microseconds wide, so a test
/// that tried to open it by loading the machine would be rolling dice; the
/// thread is given a place to be held instead.
///
/// **The gate can only ever delay.** It is waited on before the stream is
/// started, and `start` — once it keeps its promise — is on the far side of that
/// start. Holding the gate holds `start` itself.
#[cfg(test)]
pub(crate) mod stream_gate {
    use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
    use std::time::Duration;

    /// How long the watcher thread waits to be let through before going on
    /// regardless.
    ///
    /// **It breaks a deadlock and decides nothing.** `start` waits for the start
    /// this gate holds up, so a test that held the gate and then waited for
    /// `start` would be waiting for itself; the cap is what lets the green arm
    /// finish. What it must not be is shorter than the single `fs::write` a test
    /// makes while the gate is held.
    const HELD_AT_MOST: Duration = Duration::from_millis(250);

    struct Gate {
        held: bool,
        running: bool,
    }

    static GATE: Mutex<Gate> = Mutex::new(Gate {
        held: false,
        running: false,
    });
    static RELEASED: Condvar = Condvar::new();

    /// The suite's own turn-taking: the gate is one flag for the whole process,
    /// so a second watcher thread running at the same time would trip it.
    static ONE_WATCHER_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// The turn itself, for a test that only needs to not collide.
    pub(crate) fn watchers_take_turns() -> MutexGuard<'static, ()> {
        ONE_WATCHER_AT_A_TIME
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// A held gate, and the turn that comes with it.
    pub(crate) struct Held(#[allow(dead_code)] MutexGuard<'static, ()>);

    /// Hold the watcher thread's stream back until the guard is released.
    pub(crate) fn hold() -> Held {
        let turn = watchers_take_turns();
        *GATE.lock().unwrap_or_else(PoisonError::into_inner) = Gate {
            held: true,
            running: false,
        };
        Held(turn)
    }

    impl Held {
        /// Whether the stream is running *right now* — which is the whole of the
        /// promise the word `start` makes.
        pub(crate) fn the_stream_is_running(&self) -> bool {
            GATE.lock().unwrap_or_else(PoisonError::into_inner).running
        }

        /// Let the watcher thread through.
        pub(crate) fn release(&self) {
            GATE.lock().unwrap_or_else(PoisonError::into_inner).held = false;
            RELEASED.notify_all();
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            *gate = Gate {
                held: false,
                running: false,
            };
            RELEASED.notify_all();
        }
    }

    /// Called by the watcher thread immediately before it starts the stream.
    pub(crate) fn wait_if_held() {
        let gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = RELEASED.wait_timeout_while(gate, HELD_AT_MOST, |gate| gate.held);
    }

    /// Called by the watcher thread the moment `FSEventStreamStart` said yes.
    pub(crate) fn note_stream_running() {
        GATE.lock().unwrap_or_else(PoisonError::into_inner).running = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DirChange, DirWatch, ITEM_IS_DIR, ITEM_REMOVED, ITEM_RENAMED, KERNEL_DROPPED,
        MUST_SCAN_SUB_DIRS, ROOT_CHANGED, Reading, UNMOUNT, USER_DROPPED, WatchDepth, WorkerCtx,
        entry, read_event, stream_gate,
    };
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// **A watch is started where the product starts one: on a worker** — the
    /// thread door lends the `WorkerCtx` every start takes. The test thread waits
    /// for the worker's answer, as the steward does for its own starts.
    fn on_a_worker<T: Send + 'static>(start: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        crate::spawn_at_priority("bt-dir-watch-test", crate::ThreadPriority::Normal, start)
            .expect("start a worker for the watch")
            .join()
            .expect("the worker answered")
    }

    /// The next thing a watch said, within [`ARRIVES_WITHIN`].
    fn next_word<T>(rx: &mpsc::Receiver<T>) -> Option<T> {
        rx.recv_timeout(ARRIVES_WITHIN).ok()
    }

    /// Generous, because this is a claim about *eventually* and the machine
    /// running it may be building something. FSEvents' own latency is 300ms by
    /// [`super::LATENCY_SECONDS`] and the first event of a burst is not held at
    /// all; five seconds is the difference between "slow" and "never", which is
    /// the only difference these tests are about.
    const ARRIVES_WITHIN: Duration = Duration::from_secs(5);
    /// And the other way round, where the claim is "nothing at all": long enough
    /// that a notification which was going to come would have, and longer than
    /// the stream's own latency by a wide margin.
    const SILENCE_FOR: Duration = Duration::from_millis(1_200);

    /// A scratch directory **beside the test binary**, which is to say inside
    /// this build's own target directory.
    ///
    /// Not `std::env::temp_dir()`, which is where the Windows arm's twin puts
    /// its own: a Mac ticket of the port is given one corner of somebody's
    /// personal machine and writes nothing outside it
    /// (`docs/plans/port/mac-mini-venue.md`), and the target directory is the
    /// one path this crate can derive rather than be told. `current_exe` is
    /// `<target>/<profile>/deps/<binary>`, so its grandparent is the profile
    /// directory `cargo clean` already owns.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let here = std::env::current_exe().expect("a test binary knows where it is");
            let profile = here
                .parent()
                .and_then(Path::parent)
                .expect("a test binary lives in <target>/<profile>/deps")
                .to_path_buf();
            let dir = profile.join(bt_testpath::unique_name(&format!("bt-dir-watch-{name}")));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("make a scratch directory");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Every name this receiver has been handed, until one of `wanted` is among
    /// them or the budget runs out.
    ///
    /// A list rather than one name because of the overflow: a batch that lost
    /// track carries no names at all, and a test waiting for a sentinel that was
    /// swallowed by the burst it followed would be waiting for a name the stream
    /// is entitled never to say.
    fn heard(rx: &mpsc::Receiver<Vec<String>>, wanted: &[&str]) -> (bool, Vec<String>) {
        let mut seen = Vec::new();
        let deadline = Instant::now() + ARRIVES_WITHIN;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(names) => {
                    let hit = names.iter().any(|name| wanted.contains(&name.as_str()));
                    seen.extend(names);
                    if hit {
                        return (true, seen);
                    }
                }
                Err(_) => return (false, seen),
            }
        }
        (false, seen)
    }

    /// A named watch that reports its batches as lowercase strings.
    fn named_watch_at(
        root: &Path,
        start: impl FnOnce(
            &WorkerCtx,
            &Path,
            Box<dyn Fn(DirChange<'_>) + Send>,
        ) -> Result<DirWatch, std::io::Error>
        + Send
        + 'static,
    ) -> (DirWatch, mpsc::Receiver<Vec<String>>) {
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        let root = root.to_path_buf();
        let watch = on_a_worker(move |ctx| {
            start(
                ctx,
                &root,
                Box::new(move |change| {
                    let names = match change {
                        DirChange::Named(names) => names
                            .iter()
                            .map(|name| name.to_string_lossy().to_lowercase())
                            .collect(),
                        DirChange::Unknown => vec!["<unknown>".to_owned()],
                        DirChange::Ended => vec!["<ended>".to_owned()],
                    };
                    let _ = tx.send(names);
                }),
            )
        })
        .expect("watch a directory this process just made");
        (watch, rx)
    }

    fn named_watch(root: &Path) -> (DirWatch, mpsc::Receiver<Vec<String>>) {
        named_watch_at(root, |ctx, root, wake| {
            DirWatch::start_shallow_named(ctx, root, wake)
        })
    }

    /// PIN — **a tree watch hears a file three levels down.**
    ///
    /// The contract `git_watch` opens: a repository is a tree and a commit
    /// touches any depth of it, so a watcher that heard only the root would be
    /// silent for exactly the change the git page exists to show. FSEvents is
    /// recursive by nature, which makes this the contract that costs nothing —
    /// and the one that would fail loudest if the depth filter were ever applied
    /// to it by accident.
    ///
    /// MUTATION: give `start` `WatchDepth::HereOnly` and this goes red; the
    /// shallow test below stays green, which is the pair that says the two
    /// contracts are really two.
    #[test]
    fn a_tree_watch_hears_a_file_three_levels_down() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("tree");
        let (tx, rx) = mpsc::channel::<()>();
        let path = scratch.0.clone();
        let watch = on_a_worker(move |ctx| {
            DirWatch::start(ctx, &path, move |_| {
                let _ = tx.send(());
            })
        })
        .expect("watch a directory this process just made");

        let deep = scratch.0.join("one").join("two").join("three");
        std::fs::create_dir_all(&deep).expect("make a subtree");
        std::fs::write(deep.join("deep.txt"), b"hi").expect("write three levels down");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("a tree watch hears a file three directories under its root");
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// RED (T-PALETTE-DEEP) — **a named tree watch reports a nested file by its relative
    /// path.**
    ///
    /// MUTATION: give `start_named` `WatchDepth::HereOnly`; the nested name is filtered out.
    #[test]
    fn a_file_two_levels_down_is_named_relative_to_the_root() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("named-tree");
        let (watch, rx) = named_watch_at(&scratch.0, |ctx, root, wake| {
            DirWatch::start_named(ctx, root, wake)
        });
        let nested = scratch.0.join("one").join("two");
        std::fs::create_dir_all(&nested).expect("make the nested directory");
        std::fs::write(nested.join("deep.txt"), b"deep").expect("write the nested file");
        let (hit, seen) = heard(&rx, &["one/two/deep.txt"]);
        assert!(
            hit,
            "the named tree watch reports its relative path: {seen:?}"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **a shallow watch hears only the root's own entries.**
    ///
    /// `preview_watch`'s and the files column's contract. FSEvents cannot be told
    /// to stop at one level, so this is the one contract that is a decision of
    /// this arm rather than of the daemon, and the thing it must never do is let
    /// a path from below through under its own name.
    ///
    /// **The sentinel is what makes the silence sound.** "`deep.txt` never
    /// arrived" is also what a watcher that never worked reports, so the test
    /// writes into the subtree *and then* beside the root, and reads the
    /// conclusion off the batches that arrived up to and including the sentinel.
    /// A watch that was really recursive would have named `one/deep.txt` — or
    /// `deep.txt` — among them.
    ///
    /// What is deliberately **not** asserted is that no batch arrived at all for
    /// the subtree write: APFS may report the intermediate directory as an entry
    /// of the root, and `one` *is* an entry of the root. The Windows arm makes
    /// exactly the same concession for exactly the same reason (NTFS moves a
    /// directory's write time when anything is added to it), and the named
    /// contract is what lets a caller act on the difference.
    ///
    /// MUTATION: give `start_shallow_named` `WatchDepth::Tree` and the deep name
    /// appears in `seen`.
    #[test]
    fn a_shallow_watch_hears_only_the_roots_own_entries() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("shallow");
        let subtree = scratch.0.join("one");
        std::fs::create_dir_all(&subtree).expect("make the subtree before arming");
        let (watch, rx) = named_watch(&scratch.0);

        std::fs::write(scratch.0.join("watched.md"), b"hello").expect("write beside the root");
        let (hit, _) = heard(&rx, &["watched.md"]);
        assert!(hit, "a shallow watch still hears its own directory");

        std::fs::write(subtree.join("deep.txt"), b"not ours").expect("write into the subtree");
        std::fs::write(scratch.0.join("sentinel.md"), b"and this").expect("write beside it again");
        let (hit, seen) = heard(&rx, &["sentinel.md"]);
        assert!(hit, "the watch is still delivering after the subtree write");
        assert!(
            !seen.iter().any(|name| name == "deep.txt"),
            "a shallow watch named a file from below its root: {seen:?}"
        );
        assert!(
            !seen.iter().any(|name| name.contains('/')),
            "a shallow watch only ever names one component: {seen:?}"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **a named watch hears an atomic replace of its file.**
    ///
    /// The pattern every serious editor uses to save: write the new contents to a
    /// temporary beside the target, then `rename(2)` it over the target, so that
    /// no reader ever sees a half-written file. A watcher that only understood
    /// "this file was modified" would hear nothing about the file that matters —
    /// its inode was never written to, it was replaced.
    ///
    /// What makes it work here is that nothing special-cases the rename:
    /// `kFSEventStreamCreateFlagFileEvents` reports the event under **both**
    /// paths, so the target's own name is in the batch and `preview_watch`'s name
    /// filter matches it without knowing a rename happened.
    ///
    /// MUTATION: drop `FILE_EVENTS` from the create flags and the batch names the
    /// directory rather than either file.
    #[test]
    fn a_named_watch_hears_an_atomic_replace_of_its_file() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("replace");
        let page = scratch.0.join("page.md");
        std::fs::write(&page, b"before").expect("the file exists before it is watched");
        let (watch, rx) = named_watch(&scratch.0);

        let temporary = scratch.0.join("page.md.tmp");
        std::fs::write(&temporary, b"after").expect("write the temporary");
        std::fs::rename(&temporary, &page).expect("rename it over the target");

        let (hit, seen) = heard(&rx, &["page.md"]);
        assert!(
            hit,
            "an editor's write-temporary-and-rename-over is a change of the file it replaced, \
             and this watch did not say so: {seen:?}"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **an overflow is reported as a rescan, through every contract.**
    ///
    /// FSEvents will not drop events on demand: the daemon's queue is generous
    /// and a burst that empties it on one machine will not on another, so a test
    /// that only wrote files would be asserting a property of the machine. The
    /// claim is therefore made where the decision is — [`read_event`] — and the
    /// burst is kept beside it to say that the real stream survives one.
    ///
    /// Three flags mean it and each is a different place the news was lost:
    /// `MustScanSubDirs` (too much to write down), `KernelDropped`,
    /// `UserDropped`. All three answer [`Reading::LostTrack`], and — this is the
    /// half that matters — **they answer it from a path five directories below a
    /// shallow watch's root**, where the depth filter would otherwise have thrown
    /// them away. A filter that dropped an overflow would drop exactly the burst
    /// that was too big to write down.
    ///
    /// MUTATION: put the depth filter above the flag check in `read_event` and
    /// the shallow arms go red.
    #[test]
    fn an_overflow_is_reported_as_a_rescan() {
        let root = Path::new("/private/tmp/folio-watch-root");
        let deep = Path::new("/private/tmp/folio-watch-root/a/b/c/d/e.txt");
        for flag in [MUST_SCAN_SUB_DIRS, KERNEL_DROPPED, USER_DROPPED] {
            for depth in [WatchDepth::Tree, WatchDepth::HereOnly] {
                assert_eq!(
                    read_event(root, depth, flag, deep),
                    Reading::LostTrack,
                    "flag {flag:#x} under {depth:?} is a rescan wherever its path is"
                );
            }
        }

        // And a real burst beside the table, to say that the stream survives one
        // — whichever of the two answers the daemon gives under it.
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("overflow");
        let (watch, rx) = named_watch(&scratch.0);
        for index in 0..4_000u32 {
            std::fs::write(scratch.0.join(format!("burst-{index}.txt")), b"x")
                .expect("write a file of the burst");
        }

        // Let the storm arrive and be counted before the sentinel is written, so
        // that the sentinel is asked for on a quiet stream rather than from
        // inside the batch that may have swallowed it.
        let mut rescans = 0usize;
        let mut batches = 0usize;
        let settle = Instant::now() + Duration::from_secs(3);
        while let Some(left) = settle.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left.min(Duration::from_millis(800))) {
                Ok(names) => {
                    batches += 1;
                    if names.iter().any(|name| name == "<unknown>") {
                        rescans += 1;
                    }
                }
                Err(_) => break,
            }
        }
        // Printed rather than asserted in either direction: whether the daemon
        // drops under four thousand files is a property of the machine and of
        // what else it is doing, and the flag path is pinned over the table
        // above. What a run of this test can say is which it was.
        println!("a burst of 4000 files arrived as {batches} batches, {rescans} of them rescans");

        std::fs::write(scratch.0.join("sentinel.md"), b"done").expect("write the sentinel");
        let (hit, seen) = heard(&rx, &["sentinel.md", "<unknown>"]);
        assert!(
            hit,
            "the stream survived a burst of four thousand files and is still delivering: {seen:?}"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **dropping the watch stops the stream, and does not crash doing it.**
    ///
    /// The hazard this arm is shaped around: `FSEventStreamInvalidate` from a
    /// thread other than the one that scheduled the stream races the callback it
    /// is trying to stop, and the crash it produces is a use-after-free in
    /// CoreServices with this process's name on it. The shape that makes it
    /// impossible is that the owner cannot reach the stream at all — it signals a
    /// run loop source and joins — so the proof is that a drop taken **while the
    /// callback is being fed** is silent and complete.
    ///
    /// Twenty rounds, each dropping into a directory being written to, because
    /// once is not a race test. Then the ordinary claim: after the drop, nothing
    /// written reaches the callback, which is the cancellation itself.
    ///
    /// MUTATION: move the `FSEventStreamStop`/`Invalidate`/`Release` into `drop`
    /// and this is where it shows up.
    #[test]
    fn dropping_the_watch_stops_the_stream_without_a_crash() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("drop");

        for round in 0..20u32 {
            let (tx, rx) = mpsc::channel::<()>();
            let path = scratch.0.clone();
            let watch = on_a_worker(move |ctx| {
                DirWatch::start(ctx, &path, move |_| {
                    let _ = tx.send(());
                })
            })
            .expect("watch a directory this process just made");
            for index in 0..40u32 {
                std::fs::write(scratch.0.join(format!("r{round}-{index}.txt")), b"x")
                    .expect("write into a watched directory");
            }
            // Dropped with a batch very likely in flight: the join inside `drop`
            // is what makes that safe rather than lucky.
            assert!(retired(watch), "the watcher left within its bound");
            drop(rx);
        }

        let (tx, rx) = mpsc::channel::<()>();
        let path = scratch.0.clone();
        let watch = on_a_worker(move |ctx| {
            DirWatch::start(ctx, &path, move |_| {
                let _ = tx.send(());
            })
        })
        .expect("watch a directory this process just made");
        std::fs::write(scratch.0.join("before.txt"), b"x").expect("write before the drop");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("the watch is awake, so the silence below means something");
        assert!(retired(watch), "the watcher left within its bound");

        // Drain whatever was already in flight when the watch was dropped — the
        // claim is about what happens *after* the cancellation.
        while rx.try_recv().is_ok() {}
        std::fs::write(scratch.0.join("after.txt"), b"and this").expect("write after the drop");
        let deadline = Instant::now() + SILENCE_FOR;
        while Instant::now() < deadline {
            assert!(
                rx.try_recv().is_err(),
                "a dropped watch has stopped: nothing written afterwards reaches it"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// PIN — **a watch is live before `start` returns.**
    ///
    /// The word is a promise about *when*: every caller in this workspace opens a
    /// folder and then goes on to list it or to read out of it, so anything that
    /// moves between the two has to be news rather than a change that fell down
    /// the gap. FSEvents resolves [`super::SINCE_NOW`] at `FSEventStreamStart`,
    /// so a `start` that returned on the spawn would hand back a watch speaking
    /// from a moment that had not happened yet.
    ///
    /// The gate holds the watcher thread on the near side of
    /// `FSEventStreamStart`, so "the stream is running" can only mean `start`
    /// waited for it; the file is written strictly after `start` returned and
    /// strictly before the gate opens.
    ///
    /// MUTATIONS: send the armed word before `FSEventStreamStart` and the flag
    /// goes false; move it before the spawn's own body and the file written
    /// afterwards reaches the callback never rather than in milliseconds.
    #[test]
    fn a_watch_is_live_before_start_returns() {
        let scratch = Scratch::new("armed");
        let (tx, rx) = mpsc::channel::<()>();
        let gate = stream_gate::hold();
        let path = scratch.0.clone();
        let watch = on_a_worker(move |ctx| {
            DirWatch::start(ctx, &path, move |_| {
                let _ = tx.send(());
            })
        })
        .expect("watch a directory this process just made");
        let running_when_start_returned = gate.the_stream_is_running();

        std::fs::write(scratch.0.join("appeared.txt"), b"hello").expect("write a file");
        gate.release();

        rx.recv_timeout(ARRIVES_WITHIN).expect(
            "a change made after `start` returned reaches the callback: it returned listening",
        );
        assert!(
            running_when_start_returned,
            "`start` returned with the stream not yet running — everything that moved in that \
             window was before the event id the daemon resolved, and it is not sent again"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **a path that cannot be watched is refused, and the refusal says
    /// which refusal it is.**
    ///
    /// `NotFound` and not merely "an error", because that is the distinction
    /// `scheme_watch` acts on: `~/Library/Application Support/Folio/schemes` does
    /// not exist until somebody customises a scheme, and a line on stderr about
    /// it would be an alarm about the ordinary case. `FSEventStreamCreate`
    /// **accepts** a path that is not there and simply never speaks, so without
    /// the resolution `start_scoped` does first, every one of these would have
    /// been a watcher that was not watching.
    #[test]
    fn a_directory_that_is_not_there_is_refused_as_not_found() {
        let scratch = Scratch::new("refusals");
        let missing = scratch.0.join("never-created");
        let path = missing.clone();
        let error = on_a_worker(move |ctx| DirWatch::start(ctx, &path, |_| {}))
            .err()
            .expect("a folder that is not there cannot be watched");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");

        let deeper = missing.join("nor").join("this");
        let error = on_a_worker(move |ctx| DirWatch::start(ctx, &deeper, |_| {}))
            .err()
            .expect("nor this one");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");

        let file = scratch.0.join("a-file.txt");
        std::fs::write(&file, b"x").expect("write a file to point the watch at");
        let error = on_a_worker(move |ctx| DirWatch::start_shallow(ctx, &file, |_| {}))
            .err()
            .expect("a file is not a directory");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory, "{error}");
    }

    /// PIN (T-DIRWATCH-LIFECYCLE; B5 design note, revision (b)5) — **a watch whose
    /// root is renamed away after it armed ends: unarmed, its error kept once,
    /// and `Ended` said last.**
    ///
    /// Until this ticket the run loop was stopped on `RootGone` and entered again
    /// at once (the loop read only the stop flag), `is_armed` answered `true` for
    /// ever and `take_failure` `None`: a folder whose watch had ended was believed
    /// heard, and its thread lived on.
    ///
    /// MUTATIONS: make `is_armed` answer `true` again — the unarmed assertion goes
    /// red; drop the `root_gone()` condition from the run loop's `while` — no
    /// `Ended` is ever said and the first assertion goes red.
    #[test]
    fn a_watch_whose_root_is_renamed_away_ends_unarmed_and_says_so() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("ends");
        let root = scratch.0.join("watched");
        std::fs::create_dir_all(&root).expect("make the watched directory");
        let (mut watch, rx) = named_watch(&root);
        assert!(watch.is_armed(), "a watch that has just started is armed");
        std::fs::rename(&root, scratch.0.join("moved-away")).expect("rename the root away");
        let mut ended = false;
        while let Some(names) = next_word(&rx) {
            if names.iter().any(|name| name == "<ended>") {
                ended = true;
                break;
            }
        }
        assert!(ended, "the watch says `Ended` when its root goes");
        assert!(
            next_word(&rx).is_none(),
            "and nothing after it: the watcher thread has finished"
        );
        assert!(!watch.is_armed(), "a watch whose root is gone is not armed");
        assert!(
            watch.take_failure().is_some(),
            "the error that ended it is kept"
        );
        assert!(
            watch.take_failure().is_none(),
            "and handed over once, not every time it is asked"
        );
        assert!(retired(watch), "the watcher left within its bound");
    }

    /// PIN — **the root going away ends the watch the way Windows ends it.**
    ///
    /// There, the next `ReadDirectoryChangesW` fails and the watcher thread
    /// records the end and says `Ended` (T-DIRWATCH-LIFECYCLE). Here the two spellings the daemon uses
    /// for it — `kFSEventStreamEventFlagRootChanged`, which only arrives because
    /// `WATCH_ROOT` is set, and a removal of the root's own path from inside —
    /// both answer [`Reading::RootGone`], which is what stops the run loop.
    ///
    /// Read over the table rather than by deleting a directory out from under a
    /// live stream, because what is being claimed is *which events mean it*, and
    /// a filesystem can be relied upon to produce only one of the two.
    #[test]
    fn the_root_going_away_ends_the_watch() {
        let root = Path::new("/private/tmp/folio-watch-root");
        for depth in [WatchDepth::Tree, WatchDepth::HereOnly] {
            assert_eq!(
                read_event(root, depth, ROOT_CHANGED, root),
                Reading::RootGone,
                "the daemon's own word for the root moving"
            );
            assert_eq!(
                read_event(root, depth, ITEM_REMOVED | ITEM_IS_DIR, root),
                Reading::RootGone,
                "and the root's removal seen from inside"
            );
            assert_eq!(
                read_event(root, depth, ITEM_RENAMED | ITEM_IS_DIR, root),
                Reading::RootGone,
                "and its rename, which is a removal by another name"
            );
        }

        // The volume holding the root unmounted — the event names its mount
        // point, an ancestor of the root or the root itself — ends it as well
        // (T-DIRWATCH-LIFECYCLE). MUTATION: drop the `UNMOUNT` arm of
        // `read_event` and these two go red.
        for mount in [Path::new("/private/tmp"), root] {
            assert_eq!(
                read_event(root, WatchDepth::Tree, UNMOUNT, mount),
                Reading::RootGone,
                "the volume under the root was unmounted"
            );
        }
        // A volume mounted *inside* the tree going away is an entry of it.
        assert_eq!(
            read_event(root, WatchDepth::HereOnly, UNMOUNT, &root.join("disk")),
            Reading::Entry(entry("disk")),
        );

        // A directory *inside* the root being removed is not the root going: it
        // is an entry, and it is news under both contracts.
        let inside = root.join("one");
        assert_eq!(
            read_event(
                root,
                WatchDepth::HereOnly,
                ITEM_REMOVED | ITEM_IS_DIR,
                &inside
            ),
            Reading::Entry(entry("one")),
        );
    }

    /// PIN — **the four contracts, read off the table that decides their two depths.**
    ///
    /// The depth filter stated once, over paths rather than over a filesystem: a
    /// tree watch names the whole relative path the way `FILE_NOTIFY_INFORMATION`
    /// does, a shallow watch names one component and refuses anything deeper, and
    /// the root itself has no name to report.
    #[test]
    fn the_depth_filter_is_the_whole_of_the_two_shallow_contracts() {
        let root = Path::new("/private/tmp/folio-watch-root");
        let here = root.join("page.md");
        let below = root.join("target").join("debug").join("thing.o");

        assert_eq!(
            read_event(root, WatchDepth::Tree, 0, &here),
            Reading::Entry(entry("page.md"))
        );
        assert_eq!(
            read_event(root, WatchDepth::Tree, 0, &below),
            Reading::Entry(entry("target/debug/thing.o")),
            "a tree watch spells a nested change whole"
        );
        assert_eq!(
            read_event(root, WatchDepth::HereOnly, 0, &here),
            Reading::Entry(entry("page.md"))
        );
        assert_eq!(
            read_event(root, WatchDepth::HereOnly, 0, &below),
            Reading::NotOurs,
            "the object file a build wrote is not an entry of the folder being previewed out of"
        );
        assert_eq!(
            read_event(root, WatchDepth::Tree, 0, root),
            Reading::NotOurs
        );
        assert_eq!(
            read_event(
                root,
                WatchDepth::Tree,
                0,
                Path::new("/private/tmp/elsewhere")
            ),
            Reading::NotOurs
        );
    }

    /// **A watch retired the way the product retires one**: through its door, on a worker the
    /// thread door started, within `bound`. Answers whether the watcher left in time.
    fn retired_within(watch: DirWatch, bound: Duration) -> bool {
        on_a_worker(move |ctx| watch.retire_within(ctx, bound))
    }

    /// [`retired_within`] within the product's own bound.
    fn retired(watch: DirWatch) -> bool {
        retired_within(watch, crate::DIR_WATCH_RETIRE_BUDGET)
    }

    /// A watch on `path` whose watcher stands inside its wake until the test lets it go (drops
    /// the sender: every wake after that passes at once, since one write can be several
    /// notifications), and the flag the wake raises on its way out. Written into, so the watcher is in the wake when this
    /// returns.
    fn a_held_watch(
        path: &Path,
    ) -> (
        DirWatch,
        mpsc::Sender<()>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let (entered, inside) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let left = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let leaving = std::sync::Arc::clone(&left);
        let watched = path.to_path_buf();
        let watch = on_a_worker(move |ctx| {
            DirWatch::start(ctx, &watched, move |_| {
                let _ = entered.send(());
                let _ = held.recv();
                leaving.store(true, std::sync::atomic::Ordering::SeqCst);
            })
        })
        .expect("watch a directory this process just made");
        std::fs::write(path.join("hold.txt"), b"x").expect("write into the watched directory");
        crate::door_skipped::received(&inside, "the watcher inside its wake");
        (watch, release, left)
    }

    /// RED (T-DROP-DOORS, D-40) — **the door waits for the watcher to leave, and a watcher that
    /// does not leave within the bound is let go.**
    ///
    /// The watcher is held inside its wake. A door with no time at all answers `false` at once
    /// and lets it go; a door called while the hold is let go answers `true`, and when it does
    /// the wake has returned.
    ///
    /// MUTATION: return `true` from `retire_within` without the wait and the first door says the
    /// watcher left while it stands in the wake; drop the `is_finished` loop for a bare `join`
    /// and the first door never answers within the suite's patience.
    #[test]
    fn the_door_waits_for_the_watcher_within_its_bound() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("retire-held");
        let (watch, release, _left) = a_held_watch(&scratch.0);
        assert!(
            !retired_within(watch, Duration::ZERO),
            "a watcher standing in its wake has not left within no time at all"
        );
        drop(release);

        let (watch, release, left) = a_held_watch(&scratch.0);
        let (going, on_its_way) = mpsc::channel();
        let (answered, heard) = mpsc::channel();
        let leaving = std::sync::Arc::clone(&left);
        std::thread::spawn(move || {
            let _ = going.send(());
            let left_in_time = retired_within(watch, crate::door_skipped::PATIENCE);
            let _ = answered.send((
                left_in_time,
                leaving.load(std::sync::atomic::Ordering::SeqCst),
            ));
        });
        crate::door_skipped::received(&on_its_way, "the retiring thread");
        drop(release);
        let (left_in_time, wake_returned) =
            crate::door_skipped::received(&heard, "the door, once the watcher is let go");
        assert!(left_in_time, "the watcher left within the door's bound");
        assert!(
            wake_returned,
            "the door answered before the watcher left its wake"
        );
    }

    /// RED (T-DROP-DOORS, D-40) — **a watch dropped without its door waits for nothing, and says
    /// so once.**
    ///
    /// The watcher is held inside its wake for the whole of the drop: a drop that joined it would
    /// stand until the test let go, and the test lets go only after the drop has answered.
    ///
    /// MUTATION: put the join back in `Drop` and the drop's thread does not answer within the
    /// suite's patience; take the saying out and the count is zero.
    #[test]
    fn a_watch_dropped_without_its_door_waits_for_nothing_and_says_so_once() {
        let _turn = stream_gate::watchers_take_turns();
        let scratch = Scratch::new("drop-held");
        let (watch, release, _left) = a_held_watch(&scratch.0);
        let skipped = crate::door_skipped::dropped_alone(watch, "a drop while the watcher is held");
        assert_eq!(skipped, 1, "the drop says its skipped door once");
        drop(release);
    }
}

/// **Many directory watches on one thread** (T-DIR-WATCH-ONE-THREAD): the macOS arm of
/// `DirWatchSet`, the twin of `crate::dir_watch_set_tests` on Windows and read against the same
/// promises — each member's stream running when its start returns, hearing its own root by its
/// own contract, leaving alone, ending alone, and every member on the set's one run loop thread.
#[cfg(test)]
mod set_tests {
    use super::{DirChange, DirWatch, DirWatchSet, WorkerCtx, stream_gate};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    /// **A set is opened, and a member started, where the product does it: on a worker.**
    fn on_a_worker<T: Send + 'static>(start: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        crate::spawn_at_priority("bt-dir-watch-test", crate::ThreadPriority::Normal, start)
            .expect("start a worker for the watch")
            .join()
            .expect("the worker answered")
    }

    /// The lone watch's budgets in `tests`, for the same two claims: *eventually* (FSEvents'
    /// first event is not held), and *nothing at all* (well past the stream's latency).
    const ARRIVES_WITHIN: Duration = Duration::from_secs(5);
    const SILENCE_FOR: Duration = Duration::from_millis(1_200);

    /// A scratch directory beside the test binary, as `tests` keeps its own.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let here = std::env::current_exe().expect("a test binary knows where it is");
            let profile = here
                .parent()
                .and_then(Path::parent)
                .expect("a test binary lives in <target>/<profile>/deps")
                .to_path_buf();
            let dir = profile.join(bt_testpath::unique_name(&format!(
                "bt-dir-watch-set-{name}"
            )));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("make a scratch directory");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn opened() -> Arc<DirWatchSet> {
        Arc::new(on_a_worker(DirWatchSet::open).expect("open a directory watch set"))
    }

    /// Which of the four doors a member is started through.
    #[derive(Clone, Copy)]
    enum Door {
        Tree,
        TreeNamed,
        Shallow,
        ShallowNamed,
    }

    /// What a member's `wake` heard, as the test reads it.
    #[derive(Debug, Eq, PartialEq)]
    enum Word {
        Named(Vec<String>),
        Unknown,
        Ended,
    }

    /// A member of `set` on `path`, through `door`, telling `tx` every word with its `label`.
    fn member(
        set: &Arc<DirWatchSet>,
        door: Door,
        path: &Path,
        label: usize,
        tx: &mpsc::Sender<(usize, Word)>,
    ) -> DirWatch {
        let (set, path, tx) = (Arc::clone(set), path.to_path_buf(), tx.clone());
        on_a_worker(move |ctx| {
            let wake = move |change: DirChange<'_>| {
                let word = match change {
                    DirChange::Named(names) => Word::Named(
                        names
                            .iter()
                            .map(|name| name.to_string_lossy().into_owned())
                            .collect(),
                    ),
                    DirChange::Unknown => Word::Unknown,
                    DirChange::Ended => Word::Ended,
                };
                let _ = tx.send((label, word));
            };
            match door {
                Door::Tree => set.start(ctx, &path, wake),
                Door::TreeNamed => set.start_named(ctx, &path, wake),
                Door::Shallow => set.start_shallow(ctx, &path, wake),
                Door::ShallowNamed => set.start_shallow_named(ctx, &path, wake),
            }
        })
        .expect("watch a directory this process just made")
    }

    fn retired_within(watch: DirWatch, bound: Duration) -> bool {
        on_a_worker(move |ctx| watch.retire_within(ctx, bound))
    }

    fn retired(watch: DirWatch) -> bool {
        retired_within(watch, crate::DIR_WATCH_RETIRE_BUDGET)
    }

    fn set_retired(set: Arc<DirWatchSet>) -> bool {
        let set = Arc::try_unwrap(set).unwrap_or_else(|_| panic!("the test holds the set alone"));
        on_a_worker(move |ctx| set.retire_within(ctx, crate::DIR_WATCH_RETIRE_BUDGET))
    }

    /// The next word `label` said within the budget, skipping everyone else's.
    fn next_from(rx: &mpsc::Receiver<(usize, Word)>, label: usize) -> Option<Word> {
        let deadline = Instant::now() + ARRIVES_WITHIN;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok((from, word)) if from == label => return Some(word),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Whether `label` says nothing for [`SILENCE_FOR`], whatever the others say.
    fn silent(rx: &mpsc::Receiver<(usize, Word)>, label: usize) -> bool {
        let deadline = Instant::now() + SILENCE_FOR;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok((from, _)) if from == label => return false,
                Ok(_) => {}
                Err(_) => return true,
            }
        }
        true
    }

    /// The first word each of `count` labels said, or `None` if one stayed quiet past the budget.
    fn first_word_of_each<T>(
        rx: &mpsc::Receiver<(usize, T)>,
        count: usize,
    ) -> Option<std::collections::BTreeMap<usize, T>> {
        let mut heard = std::collections::BTreeMap::new();
        let deadline = Instant::now() + ARRIVES_WITHIN;
        while heard.len() < count {
            let left = deadline.checked_duration_since(Instant::now())?;
            let (label, word) = rx.recv_timeout(left).ok()?;
            heard.entry(label).or_insert(word);
        }
        Some(heard)
    }

    /// Whether `label` names `wanted` within the budget.
    fn names(rx: &mpsc::Receiver<(usize, Word)>, label: usize, wanted: &str) -> bool {
        while let Some(word) = next_from(rx, label) {
            if let Word::Named(names) = &word
                && names.iter().any(|name| name == wanted)
            {
                return true;
            }
        }
        false
    }

    /// RED (T-DIR-WATCH-ONE-THREAD) — **eight directories, through all four doors, are heard on
    /// one watcher thread.**
    ///
    /// Each member's `wake` says which thread it runs on, and every member is made to speak, so
    /// the one thread is one that listens rather than one that does nothing. The thread's own
    /// identity is the probe, not a census of the process: a lone watch another test let go of
    /// may still be leaving while this one runs, and a count would read it.
    ///
    /// MUTATION: start every member as a lone `DirWatch` of its own (a thread per directory) —
    /// the eight members speak on eight threads.
    #[test]
    fn a_set_watches_many_directories_on_one_thread() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let scratch: Vec<Scratch> = (0..8)
            .map(|index| Scratch::new(&format!("many-{index}")))
            .collect();
        let doors = [
            Door::Tree,
            Door::TreeNamed,
            Door::Shallow,
            Door::ShallowNamed,
        ];
        let (tx, rx) = mpsc::channel::<(usize, (std::thread::ThreadId, Option<String>))>();
        let watches: Vec<DirWatch> = scratch
            .iter()
            .enumerate()
            .map(|(label, dir)| {
                let (set, path, tx) = (Arc::clone(&set), dir.0.clone(), tx.clone());
                let door = doors[label % doors.len()];
                on_a_worker(move |ctx| {
                    let wake = move |_: DirChange<'_>| {
                        let here = std::thread::current();
                        let _ = tx.send((label, (here.id(), here.name().map(str::to_owned))));
                    };
                    match door {
                        Door::Tree => set.start(ctx, &path, wake),
                        Door::TreeNamed => set.start_named(ctx, &path, wake),
                        Door::Shallow => set.start_shallow(ctx, &path, wake),
                        Door::ShallowNamed => set.start_shallow_named(ctx, &path, wake),
                    }
                })
                .expect("watch a directory this process just made")
            })
            .collect();
        for dir in &scratch {
            std::fs::write(dir.0.join("appeared.txt"), b"hello").expect("write a file");
        }
        let heard = first_word_of_each(&rx, scratch.len())
            .expect("every member of the set reports its own directory");
        let threads: std::collections::HashSet<std::thread::ThreadId> =
            heard.values().map(|(thread, _)| *thread).collect();
        assert_eq!(
            threads.len(),
            1,
            "eight directories under four contracts are heard on one watcher thread"
        );
        assert!(
            heard
                .values()
                .all(|(_, name)| name.as_deref() == Some("bt-dir-watch")),
            "and that thread is the set's own watcher"
        );
        for watch in watches {
            assert!(retired(watch), "each member left its set within its bound");
        }
        assert!(set_retired(set), "the set's thread left within its bound");
    }

    /// PIN (T-DIR-WATCH-ONE-THREAD) — **two directories of one set both report, each by its own
    /// contract**: a shallow named member names its own entry, a tree named member names a change
    /// two levels down relative to its own root, a nameless member hears only that something
    /// moved.
    ///
    /// MUTATION: hand every batch to the first member's `wake` — the others never speak.
    #[test]
    fn two_directories_of_one_set_both_report() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let (here, tree, plain) = (
            Scratch::new("both-here"),
            Scratch::new("both-tree"),
            Scratch::new("both-plain"),
        );
        let (tx, rx) = mpsc::channel();
        let a = member(&set, Door::ShallowNamed, &here.0, 0, &tx);
        let b = member(&set, Door::TreeNamed, &tree.0, 1, &tx);
        let c = member(&set, Door::Tree, &plain.0, 2, &tx);

        std::fs::write(here.0.join("a.txt"), b"a").expect("write into the shallow member");
        assert!(
            names(&rx, 0, "a.txt"),
            "the shallow named member names its own entry"
        );
        std::fs::create_dir_all(tree.0.join("sub")).expect("make a subtree");
        std::fs::write(tree.0.join("sub").join("b.txt"), b"b").expect("write two levels down");
        assert!(
            names(&rx, 1, "sub/b.txt"),
            "the tree named member names the change relative to its own root"
        );
        std::fs::write(plain.0.join("c.txt"), b"c").expect("write into the nameless member");
        assert_eq!(
            next_from(&rx, 2),
            Some(Word::Unknown),
            "the nameless member hears that something moved, and no names"
        );
        for watch in [a, b, c] {
            assert!(retired(watch), "each member left its set within its bound");
        }
        assert!(set_retired(set), "the set's thread left within its bound");
    }

    /// PIN (T-DIR-WATCH-ONE-THREAD) — **removing one member mid-run stops its news and keeps the
    /// other's.**
    ///
    /// MUTATION: make the removal stop the whole set — the kept member goes silent too.
    #[test]
    fn removing_one_member_mid_run_silences_it_and_keeps_the_other() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let (gone, kept) = (Scratch::new("remove-gone"), Scratch::new("remove-kept"));
        let (tx, rx) = mpsc::channel();
        let a = member(&set, Door::Shallow, &gone.0, 0, &tx);
        let b = member(&set, Door::Shallow, &kept.0, 1, &tx);
        std::fs::write(gone.0.join("before.txt"), b"x").expect("write before the removal");
        assert!(
            next_from(&rx, 0).is_some(),
            "the member to be removed is awake"
        );
        std::fs::write(kept.0.join("before.txt"), b"x").expect("write before the removal");
        assert!(
            next_from(&rx, 1).is_some(),
            "the member to be kept is awake"
        );

        assert!(
            retired(a),
            "the removed member left its set within its bound"
        );
        while rx.try_recv().is_ok() {}
        std::fs::write(gone.0.join("after.txt"), b"x").expect("write after the removal");
        std::fs::write(kept.0.join("after.txt"), b"x").expect("write after the removal");
        assert!(
            next_from(&rx, 1).is_some(),
            "the kept member still hears its directory"
        );
        assert!(
            silent(&rx, 0),
            "the removed member hears nothing written afterwards"
        );
        assert!(retired(b), "the kept member left its set within its bound");
        assert!(set_retired(set), "the set's thread left within its bound");
    }

    /// RED (T-DIR-WATCH-ONE-THREAD) — **retiring the set stops every member and joins its one
    /// thread**; the members' own doors then answer at once.
    ///
    /// MUTATION: let the set's door return without stopping its members (a thread per member
    /// left behind it) — the members go on hearing their directories.
    #[test]
    fn retiring_the_set_stops_every_member_and_joins_its_thread() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let (one, two) = (Scratch::new("stop-one"), Scratch::new("stop-two"));
        let (tx, rx) = mpsc::channel();
        let a = member(&set, Door::Tree, &one.0, 0, &tx);
        let b = member(&set, Door::ShallowNamed, &two.0, 1, &tx);
        std::fs::write(one.0.join("before.txt"), b"x").expect("write before the stop");
        assert!(next_from(&rx, 0).is_some(), "the first member is awake");
        std::fs::write(two.0.join("before.txt"), b"x").expect("write before the stop");
        assert!(next_from(&rx, 1).is_some(), "the second member is awake");

        assert!(set_retired(set), "the set's thread left within its bound");
        while rx.try_recv().is_ok() {}
        std::fs::write(one.0.join("after.txt"), b"x").expect("write after the stop");
        std::fs::write(two.0.join("after.txt"), b"x").expect("write after the stop");
        assert!(
            silent(&rx, 0),
            "the first member hears nothing after the set stopped"
        );
        assert!(silent(&rx, 1), "nor does the second");
        assert!(
            retired_within(a, Duration::ZERO) && retired_within(b, Duration::ZERO),
            "a member of a stopped set has nothing left to wait for"
        );
    }

    /// PIN (T-DIR-WATCH-ONE-THREAD; T-DIRWATCH-LIFECYCLE) — **a member whose root goes away ends
    /// alone**: `Ended` once, unarmed, its error kept; the set's other member goes on hearing.
    ///
    /// MUTATION: stop the whole run loop for good on one member's root going — the other
    /// member's write is never heard.
    #[test]
    fn a_member_whose_directory_goes_away_ends_alone() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let (outer, staying) = (Scratch::new("end-going"), Scratch::new("end-staying"));
        let going = outer.0.join("watched");
        std::fs::create_dir_all(&going).expect("make the watched directory");
        let (tx, rx) = mpsc::channel();
        let mut a = member(&set, Door::ShallowNamed, &going, 0, &tx);
        let b = member(&set, Door::Tree, &staying.0, 1, &tx);
        std::fs::rename(&going, outer.0.join("moved-away")).expect("rename the root away");
        let mut ended = false;
        while let Some(word) = next_from(&rx, 0) {
            if word == Word::Ended {
                ended = true;
                break;
            }
        }
        assert!(ended, "the member whose root went says `Ended`");
        assert!(!a.is_armed(), "and is no longer armed");
        assert!(
            a.take_failure().is_some(),
            "and keeps the error that ended it"
        );
        std::fs::write(staying.0.join("still.txt"), b"x").expect("write into the other member");
        assert_eq!(
            next_from(&rx, 1),
            Some(Word::Unknown),
            "the set's other member still hears its directory"
        );
        assert!(retired(a), "the ended member's door answers");
        assert!(retired(b), "the other member left its set within its bound");
        assert!(set_retired(set), "the set's thread left within its bound");
    }

    /// PIN (T-DIR-WATCH-ONE-THREAD) — **a member joining a set that is already running is
    /// listening when its start returns**: `tests::a_watch_is_live_before_start_returns`, against
    /// a set that already carries a member.
    ///
    /// MUTATION: answer the arrival before `FSEventStreamStart` — the flag is false and the write
    /// made before the gate opens is never heard.
    #[test]
    fn a_new_member_is_listening_before_its_start_returns() {
        let set = opened();
        let (first, second) = (Scratch::new("join-first"), Scratch::new("join-second"));
        let (tx, rx) = mpsc::channel();
        let a = {
            let _turn = stream_gate::watchers_take_turns();
            member(&set, Door::Shallow, &first.0, 0, &tx)
        };
        let gate = stream_gate::hold();
        let b = member(&set, Door::Shallow, &second.0, 1, &tx);
        let running_when_start_returned = gate.the_stream_is_running();
        std::fs::write(second.0.join("appeared.txt"), b"hello").expect("write a file");
        gate.release();
        assert!(
            next_from(&rx, 1).is_some(),
            "a change made after the newcomer's start returned reaches it"
        );
        assert!(
            running_when_start_returned,
            "the newcomer's start returned with its stream not yet running"
        );
        assert!(retired(a), "the first member left its set within its bound");
        assert!(retired(b), "the newcomer left its set within its bound");
        assert!(set_retired(set), "the set's thread left within its bound");
        drop(gate);
    }

    /// PIN (T-DIR-WATCH-ONE-THREAD; T-DROP-DOORS, D-40) — **a member whose `wake` the set's
    /// thread is standing in is retired within its bound**, and the door answers only after the
    /// wake returned.
    ///
    /// MUTATION: mark the member gone when its removal is posted rather than when its stream is
    /// released — the first door answers `true`.
    #[test]
    fn a_member_held_in_its_wake_is_retired_within_its_bound() {
        let _turn = stream_gate::watchers_take_turns();
        let set = opened();
        let scratch = Scratch::new("held");
        let held_member = |set: &Arc<DirWatchSet>| {
            let (entered, inside) = mpsc::channel();
            let (release, held) = mpsc::channel::<()>();
            let left = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let leaving = Arc::clone(&left);
            let (set, path) = (Arc::clone(set), scratch.0.clone());
            let watch = on_a_worker(move |ctx| {
                set.start(ctx, &path, move |_| {
                    let _ = entered.send(());
                    let _ = held.recv();
                    leaving.store(true, std::sync::atomic::Ordering::SeqCst);
                })
            })
            .expect("watch a directory this process just made");
            std::fs::write(scratch.0.join("hold.txt"), b"x").expect("write into the directory");
            crate::door_skipped::received(&inside, "the set's thread inside the member's wake");
            (watch, release, left)
        };

        let (watch, release, _left) = held_member(&set);
        assert!(
            !retired_within(watch, Duration::ZERO),
            "a member whose wake is running has not left within no time at all"
        );
        drop(release);

        let (watch, release, left) = held_member(&set);
        let (going, on_its_way) = mpsc::channel();
        let (answered, heard) = mpsc::channel();
        let leaving = Arc::clone(&left);
        std::thread::spawn(move || {
            let _ = going.send(());
            let left_in_time = retired_within(watch, crate::door_skipped::PATIENCE);
            let _ = answered.send((
                left_in_time,
                leaving.load(std::sync::atomic::Ordering::SeqCst),
            ));
        });
        crate::door_skipped::received(&on_its_way, "the retiring thread");
        drop(release);
        let (left_in_time, wake_returned) =
            crate::door_skipped::received(&heard, "the door, once the wake is let go");
        assert!(left_in_time, "the member left within the door's bound");
        assert!(
            wake_returned,
            "the door answered before the member's wake returned"
        );
        assert!(set_retired(set), "the set's thread left within its bound");
    }
}
