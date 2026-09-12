//! **A subscription to a directory, over FSEvents** (ticket M2-1).
//!
//! The macOS twin of `windows_impl`'s [`DirWatch`], and the one door in this
//! crate whose *contract* is three doors: `start` watches a tree, `start_shallow`
//! watches one directory's own entries, and `start_shallow_named` watches the
//! same entries and says which of them moved. The backend inventory
//! (`docs/plans/port/backend-inventory-2026-09-12.md` §6 ⑤) is explicit that the
//! depth enum is **not** part of the public interface — `bt-app` reaches the
//! three contracts through the three constructors — so preserving the contracts
//! means preserving the three doors, and this module does not export a depth.
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
//! # The three contracts, and the one that costs something here
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
//! 1. It is the Windows arm restated. There, one watcher thread per watch owns
//!    the handle, `start` waits for that thread to say it is listening, and
//!    `drop` is *signal the thread and join it*. Here, one watcher thread per
//!    watch owns the stream, `start` waits for it to say the stream is running,
//!    and `drop` is signal and join. A reader who knows one knows the other.
//! 2. **Teardown happens on the thread that scheduled the stream.** That is the
//!    hazard this ticket names: `FSEventStreamInvalidate` from a thread other
//!    than the scheduling one races the callback it is trying to stop. With a
//!    dedicated run loop there is no way to write that bug — the owner cannot
//!    reach the stream at all, only the run loop, and `Stop` / `Invalidate` /
//!    `Release` are the last three statements the watcher thread executes before
//!    it returns and the owner's `join` observes it.
//! 3. A dispatch queue would need the same rendezvous anyway (`SetDispatchQueue`
//!    with `NULL` before invalidating, and a `dispatch_sync` to know when the
//!    last callback has finished), spelled with a second concurrency primitive.
//!
//! The cost is one thread per watch — which the Windows arm already pays, and
//! `git_watch` already opens one per repository on the screen.
//!
//! # The stop is a run loop source, not a flag
//!
//! `CFRunLoopStop` on a loop that has not started yet does nothing, and the
//! window between "the watcher thread said it is running" and "the watcher
//! thread is inside `CFRunLoopRun`" is exactly where a caller that drops
//! immediately would fall. So the owner does not stop the loop; it **signals a
//! source**, and a signalled source is *pending* whether or not the loop is
//! running — it fires the moment the loop starts. The source's perform callback
//! is what calls `CFRunLoopStop`, on the loop's own thread. There is no window
//! and there is no timer.

use std::cell::RefCell;
use std::ffi::{OsString, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

// ── the three contracts ────────────────────────────────────────────────────

/// **How far under a watched directory a notification is allowed to come from.**
///
/// Private, exactly as it is on Windows and for the reason the backend inventory
/// gives (§6 ⑤): the three contracts are the three constructors, and the enum is
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
}

/// **A subscription to a directory's change notifications, over FSEvents.**
///
/// One stream, one thread, one run loop. See the module header for why it is a
/// run loop rather than a dispatch queue, and why the stop is a run loop source
/// rather than a flag.
///
/// **It returns already listening.** Not "a thread has been started that will
/// begin listening": when a constructor hands back a `DirWatch`,
/// `FSEventStreamStart` has returned `true` on the watcher thread and the
/// daemon has resolved [`SINCE_NOW`] to a real event id, so every change from
/// that instant onwards is news the caller will be told about. The promise has to
/// be that strong for the same reason it is on Windows: every caller's next move
/// is to touch or to list the very directory it has just armed.
///
/// **Dropping it cancels**, and the cancellation is complete when `drop`
/// returns: the watcher thread is signalled, it stops and invalidates and
/// releases the stream on its own run loop's thread, and `drop` joins it. After
/// that there is no callback in flight, because there is no thread.
pub struct DirWatch {
    /// The watcher thread's run loop and the source that stops it. Held by the
    /// owner so that `drop` can signal without reaching the stream.
    stopper: Stopper,
    /// Taken by `drop`.
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The run loop the stream is scheduled on, and the source whose perform
/// callback stops it.
///
/// # Safety of the `Send`
///
/// `CFRunLoopSourceSignal`, `CFRunLoopWakeUp` and `CFRetain`/`CFRelease` are
/// documented thread-safe, and those are the only calls the owner makes on
/// either object — `CFRunLoopStop` is called by the perform callback, which runs
/// on the loop's own thread. The pair is created on the watcher thread and
/// handed here exactly once, and the watcher thread is joined before the owner's
/// copies are dropped.
struct Stopper {
    run_loop: CFRetained<CFRunLoop>,
    source: CFRetained<CFRunLoopSource>,
}

// SAFETY: see the type's own note.
unsafe impl Send for Stopper {}

impl Stopper {
    /// Ask the watcher thread to finish.
    ///
    /// A signalled source is **pending**: if the run loop has not started yet the
    /// signal is remembered and fires the instant it does, which is what closes
    /// the window a bare `CFRunLoopStop` would leave open.
    fn signal(&self) {
        self.source.signal();
        self.run_loop.wake_up();
    }
}

impl DirWatch {
    /// Start watching `path` and everything under it.
    ///
    /// `wake` is called on the watcher thread, once per batch, and is expected to
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
    pub fn start(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Self, std::io::Error> {
        Self::start_scoped(path, WatchDepth::Tree, move |_| wake())
    }

    /// The same subscription over **this directory and no deeper**.
    ///
    /// On Windows this is one boolean handed to the kernel. Here it is a filter
    /// on the watcher thread, because FSEvents has no depth argument — see the
    /// module header for what that costs and why it is still the right shape.
    /// The caller that wants it is the files column: the folders whose names are
    /// on the glass, and a collapsed `target/` is not one of them.
    pub fn start_shallow(
        path: &Path,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(path, WatchDepth::HereOnly, move |_| wake())
    }

    /// **The same shallow subscription, told which entry moved.**
    ///
    /// `preview_watch`'s door: `wake` is handed the [`DirChange`] this batch
    /// carried, so a folder full of things nobody is reading can be answered on
    /// the watcher thread and cost the event loop nothing at all. An editor's
    /// atomic replace — write a temporary, rename it over the target — names the
    /// target, because the rename is an event on the target's own path.
    pub fn start_shallow_named(
        path: &Path,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
        Self::start_scoped(path, WatchDepth::HereOnly, wake)
    }

    fn start_scoped(
        path: &Path,
        depth: WatchDepth,
        wake: impl Fn(DirChange<'_>) + Send + 'static,
    ) -> Result<Self, std::io::Error> {
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

        // The word that makes `start` mean what it says: the watcher thread sends
        // it once, the instant the stream is running — or sends the refusal
        // instead, which is this function's error and not a thread that dies in
        // private after the caller has been told it has a watcher.
        let (armed, listening) = std::sync::mpsc::channel::<Result<Stopper, std::io::Error>>();
        let thread = std::thread::Builder::new()
            .name("bt-dir-watch".to_owned())
            .spawn(move || watch_loop(&root, depth, &armed, wake))?;

        // A `RecvError` is the thread ending without a word, which is a panic
        // between the spawn and the start — there is no return path there that
        // does not send. It is still an answer and not an unreachable: this is a
        // terminal, and a panic taken as a panic takes somebody's scrollback
        // with it.
        match listening.recv() {
            Ok(Ok(stopper)) => Ok(Self {
                stopper,
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
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        // The thread tears its own stream down: the stop source is one of the
        // things its run loop is waiting on, so signalling it is enough, and an
        // `FSEventStreamInvalidate` from here would be a second thread reaching
        // into a stream whose callback may be running on the first.
        self.stopper.signal();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ── the watcher thread ─────────────────────────────────────────────────────

/// What the watcher thread keeps, and what the FSEvents callback is handed back
/// through the stream context's `info` pointer.
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
    wake: Box<dyn Fn(DirChange<'_>)>,
    /// Reused across batches so that a busy folder does not allocate per
    /// notification.
    names: RefCell<Vec<OsString>>,
    /// Set when the root itself went away, and read by the loop below.
    root_gone: AtomicBool,
    /// The run loop to stop when it does.
    run_loop: CFRetained<CFRunLoop>,
}

/// The watcher thread: create the stream, schedule it on this thread's run loop,
/// start it, say so, and then be the run loop until somebody signals the stop.
fn watch_loop(
    root: &Path,
    depth: WatchDepth,
    armed: &std::sync::mpsc::Sender<Result<Stopper, std::io::Error>>,
    wake: impl Fn(DirChange<'_>) + 'static,
) {
    let run_loop = CFRunLoop::current().expect("every thread has a run loop");
    // SAFETY: `kCFRunLoopDefaultMode` is a Core Foundation constant, initialised
    // before any code of this process runs and never written to.
    let mode = unsafe { kCFRunLoopDefaultMode }
        .expect("the default run loop mode is a Core Foundation constant");

    // The stop, built before anything can need it. `stopped` is what the perform
    // callback sets and the loop below reads; the `Arc` is the one thing the
    // callback needs to reach, and it is leaked into the source's context and
    // reclaimed after the source is invalidated.
    let stopped = Arc::new(AtomicBool::new(false));
    let source = match stop_source(&stopped) {
        Some(source) => source,
        None => {
            let _ = armed.send(Err(std::io::Error::other(
                "a run loop source for the watcher's stop could not be created",
            )));
            return;
        }
    };
    run_loop.add_source(Some(&source), Some(mode));

    let subscriber = Box::new(Subscriber {
        root: root.to_path_buf(),
        depth,
        wake: Box::new(wake),
        names: RefCell::new(Vec::new()),
        root_gone: AtomicBool::new(false),
        run_loop: run_loop.clone(),
    });
    let subscriber = Box::into_raw(subscriber);

    let stream = create_stream(root, subscriber.cast::<c_void>());
    let Some(stream) = stream else {
        run_loop.remove_source(Some(&source), Some(mode));
        source.invalidate();
        // SAFETY: nothing else ever held this pointer — the stream that would
        // have was never created.
        drop(unsafe { Box::from_raw(subscriber) });
        let _ = armed.send(Err(std::io::Error::other(
            "FSEventStreamCreate refused the watched path",
        )));
        return;
    };

    // SAFETY: `stream` was just created and has not been scheduled anywhere;
    // `run_loop` is this thread's own and `mode` is a Core Foundation constant.
    unsafe { FSEventStreamScheduleWithRunLoop(stream.as_ptr(), &run_loop, mode) };

    #[cfg(test)]
    stream_gate::wait_if_held();
    // SAFETY: the stream is scheduled on a run loop that exists.
    let running = unsafe { FSEventStreamStart(stream.as_ptr()) } != 0;
    #[cfg(test)]
    if running {
        stream_gate::note_stream_running();
    }

    if !running {
        // SAFETY: the stream was scheduled on this thread's run loop and never
        // started, so no callback can be in flight; this is the thread that
        // scheduled it.
        unsafe {
            FSEventStreamInvalidate(stream.as_ptr());
            FSEventStreamRelease(stream.as_ptr());
        }
        run_loop.remove_source(Some(&source), Some(mode));
        source.invalidate();
        // SAFETY: the stream that held this pointer is invalid and released.
        drop(unsafe { Box::from_raw(subscriber) });
        let _ = armed.send(Err(std::io::Error::other(
            "FSEventStreamStart could not arm the watch",
        )));
        return;
    }

    if armed
        .send(Ok(Stopper {
            run_loop: run_loop.clone(),
            source: source.clone(),
        }))
        .is_err()
    {
        // `start` gave up on us — there is nobody to watch for. Fall through to
        // the teardown rather than into a run loop nothing will ever stop.
        stopped.store(true, Ordering::Release);
    }

    // The loop. `CFRunLoopRun` returns when the stop source's perform callback
    // calls `CFRunLoopStop`, and re-entering on any other return is right: the
    // only other reasons are a mode that emptied, which cannot happen while the
    // stop source is attached, and a stop somebody else's code asked for, which
    // is not this watch's cancellation.
    while !stopped.load(Ordering::Acquire) {
        CFRunLoop::run();
    }

    // **On the thread that scheduled it**, and in this order: stop delivering,
    // unschedule, release. Anything else is a callback running against a
    // `Subscriber` that has been freed.
    //
    // SAFETY: this is the thread that scheduled the stream, the run loop it was
    // scheduled on is no longer running, and nothing holds the stream afterwards.
    unsafe {
        FSEventStreamStop(stream.as_ptr());
        FSEventStreamInvalidate(stream.as_ptr());
        FSEventStreamRelease(stream.as_ptr());
    }
    run_loop.remove_source(Some(&source), Some(mode));
    source.invalidate();
    // SAFETY: the stream is invalid and released, so the callback that held this
    // pointer can no longer run. The pointer came from `Box::into_raw` above and
    // has not been freed.
    drop(unsafe { Box::from_raw(subscriber) });
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

/// The run loop source whose only job is to stop the loop it is attached to.
fn stop_source(stopped: &Arc<AtomicBool>) -> Option<CFRetained<CFRunLoopSource>> {
    /// The source's perform callback. Runs on the run loop's own thread, which
    /// is the watcher thread, which is why `CFRunLoopStop` here is not the
    /// cross-thread call the owner would otherwise have had to make.
    unsafe extern "C-unwind" fn perform(info: *mut c_void) {
        // SAFETY: `info` is the `Arc` this module leaked into the context below
        // and is alive until the source is invalidated and the leak reclaimed.
        let stopped = unsafe { &*info.cast::<AtomicBool>() };
        stopped.store(true, Ordering::Release);
        if let Some(run_loop) = CFRunLoop::current() {
            run_loop.stop();
        }
    }

    let mut context = CFRunLoopSourceContext {
        version: 0,
        info: Arc::as_ptr(stopped).cast::<c_void>().cast_mut(),
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
    // the call, and its `info` points at an `AtomicBool` owned by an `Arc` the
    // watcher thread holds for longer than the source: the source is invalidated
    // before `watch_loop` returns, and the `Arc` is dropped after that.
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
        // nothing left to watch and nothing to report. There, the next
        // `ReadDirectoryChangesW` fails and the watcher thread returns without a
        // word; the caller keeps whatever it last knew and the page's own
        // refresh is still there. Here the run loop is stopped, which ends the
        // same thread the same way. The `DirWatch` stays droppable and drops
        // into a thread that has already finished.
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
        MUST_SCAN_SUB_DIRS, ROOT_CHANGED, Reading, USER_DROPPED, WatchDepth, entry, read_event,
        stream_gate,
    };
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

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
            let dir = profile.join(format!(
                "bt-dir-watch-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
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
    fn named_watch(root: &Path) -> (DirWatch, mpsc::Receiver<Vec<String>>) {
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        let watch = DirWatch::start_shallow_named(root, move |change| {
            let names = match change {
                DirChange::Named(names) => names
                    .iter()
                    .map(|name| name.to_string_lossy().to_lowercase())
                    .collect(),
                DirChange::Unknown => vec!["<unknown>".to_owned()],
            };
            let _ = tx.send(names);
        })
        .expect("watch a directory this process just made");
        (watch, rx)
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
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");

        let deep = scratch.0.join("one").join("two").join("three");
        std::fs::create_dir_all(&deep).expect("make a subtree");
        std::fs::write(deep.join("deep.txt"), b"hi").expect("write three levels down");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("a tree watch hears a file three directories under its root");
        drop(watch);
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
        drop(watch);
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
        drop(watch);
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
        drop(watch);
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
            let watch = DirWatch::start(&scratch.0, move || {
                let _ = tx.send(());
            })
            .expect("watch a directory this process just made");
            for index in 0..40u32 {
                std::fs::write(scratch.0.join(format!("r{round}-{index}.txt")), b"x")
                    .expect("write into a watched directory");
            }
            // Dropped with a batch very likely in flight: the join inside `drop`
            // is what makes that safe rather than lucky.
            drop(watch);
            drop(rx);
        }

        let (tx, rx) = mpsc::channel::<()>();
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");
        std::fs::write(scratch.0.join("before.txt"), b"x").expect("write before the drop");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("the watch is awake, so the silence below means something");
        drop(watch);

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
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
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
        drop(watch);
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
        let error = DirWatch::start(&missing, || {})
            .err()
            .expect("a folder that is not there cannot be watched");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");

        let deeper = missing.join("nor").join("this");
        let error = DirWatch::start(&deeper, || {}).err().expect("nor this one");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");

        let file = scratch.0.join("a-file.txt");
        std::fs::write(&file, b"x").expect("write a file to point the watch at");
        let error = DirWatch::start_shallow(&file, || {})
            .err()
            .expect("a file is not a directory");
        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory, "{error}");
    }

    /// PIN — **the root going away ends the watch the way Windows ends it.**
    ///
    /// There, the next `ReadDirectoryChangesW` fails and the watcher thread
    /// returns without a word: the caller keeps whatever it last knew and the
    /// page's own refresh is still there. Here the two spellings the daemon uses
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

    /// PIN — **the three contracts, read off the table that decides them.**
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
}
