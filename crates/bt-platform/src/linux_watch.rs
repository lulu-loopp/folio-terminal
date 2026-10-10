//! Linux directory subscriptions backed by inotify.
//!
//! A returned `Subscription` is pending until its worker installs the kernel
//! watches. It owns the armed `DirWatch`; dropping it cancels startup or queues
//! the raw watcher for asynchronous retirement. Recursive watches add and remove
//! descendant watches as directories enter and leave the tree.

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError, mpsc};
use std::thread::JoinHandle;

use crate::admission::WorkerCtx;
use crate::{DirChange, ThreadPriority};

type ChangeWake = Box<dyn for<'a> Fn(DirChange<'a>) + Send + 'static>;

const WATCH_MASK: u32 = libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MODIFY
    | libc::IN_MOVE_SELF
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_UNMOUNT
    | libc::IN_ONLYDIR
    | libc::IN_DONT_FOLLOW;
const ACTIVITY_MASK: u32 = libc::IN_ATTRIB
    | libc::IN_CLOSE_WRITE
    | libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_DELETE_SELF
    | libc::IN_MODIFY
    | libc::IN_MOVE_SELF
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_UNMOUNT;
const BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchDepth {
    Tree,
    HereOnly,
}

static RETIREMENT: OnceLock<Mutex<Option<Retirement>>> = OnceLock::new();

struct Retirement {
    sender: mpsc::Sender<RetiredWatch>,
    worker: JoinHandle<()>,
}

/// The raw watcher, owned by a pending or armed [`Subscription`].
struct DirWatch {
    stop: Option<OwnedFd>,
    thread: Option<JoinHandle<()>>,
    retire: mpsc::Sender<RetiredWatch>,
    cancelled: Arc<AtomicBool>,
}

struct RetiredWatch {
    stop: OwnedFd,
    thread: JoinHandle<()>,
}

/// A nonblocking owner for an asynchronous directory-watch start.
pub struct Subscription {
    state: Arc<Mutex<SubscriptionState>>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct SubscriptionState {
    watch: Option<DirWatch>,
    failure: Option<io::Error>,
}

impl Subscription {
    /// Queue a recursive subscription. The initial `Unknown` wake means the
    /// caller must rescan once the worker has installed the kernel watches.
    pub fn start(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Self, io::Error> {
        let wake = Arc::new(Mutex::new(wake));
        let change_wake = Arc::clone(&wake);
        let armed_wake = Arc::clone(&wake);
        Self::queue(
            path.to_path_buf(),
            WatchDepth::Tree,
            move |_| {
                let wake = change_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake();
            },
            move || {
                let wake = armed_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake();
            },
        )
    }

    /// Queue a subscription to immediate entries only.
    pub fn start_shallow(path: &Path, wake: impl Fn() + Send + 'static) -> Result<Self, io::Error> {
        let wake = Arc::new(Mutex::new(wake));
        let change_wake = Arc::clone(&wake);
        let armed_wake = Arc::clone(&wake);
        Self::queue(
            path.to_path_buf(),
            WatchDepth::HereOnly,
            move |_| {
                let wake = change_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake();
            },
            move || {
                let wake = armed_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake();
            },
        )
    }

    /// Queue a shallow subscription that reports kernel names and queue loss.
    pub fn start_shallow_named(
        path: &Path,
        wake: impl for<'a> Fn(DirChange<'a>) + Send + 'static,
    ) -> Result<Self, io::Error> {
        let wake = Arc::new(Mutex::new(wake));
        let change_wake = Arc::clone(&wake);
        let armed_wake = Arc::clone(&wake);
        Self::queue(
            path.to_path_buf(),
            WatchDepth::HereOnly,
            move |change| {
                let wake = change_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake(change);
            },
            move || {
                let wake = armed_wake
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                wake(DirChange::Unknown);
            },
        )
    }

    fn queue(
        path: PathBuf,
        depth: WatchDepth,
        wake: impl for<'a> Fn(DirChange<'a>) + Send + 'static,
        armed_wake: impl FnOnce() + Send + 'static,
    ) -> Result<Self, io::Error> {
        let state = Arc::new(Mutex::new(SubscriptionState::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_state = Arc::clone(&state);
        let worker_cancelled = Arc::clone(&cancelled);
        let callback_cancelled = Arc::clone(&cancelled);
        let wake: ChangeWake = Box::new(move |change: DirChange<'_>| {
            if !callback_cancelled.load(Ordering::Acquire) {
                wake(change);
            }
        });
        let arm_cancelled = Arc::clone(&cancelled);
        let armed_wake = move || {
            if !arm_cancelled.load(Ordering::Acquire) {
                armed_wake();
            }
        };
        let start_worker = crate::spawn_at_priority(
            "bt-dir-watch-start",
            ThreadPriority::Normal,
            move |worker| {
                if worker_cancelled.load(Ordering::Acquire) {
                    return;
                }
                let result = DirWatch::start_scoped(worker, &path, depth, &worker_cancelled, wake);
                match result {
                    Ok(watch) => {
                        let mut state = worker_state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if worker_cancelled.load(Ordering::Acquire) {
                            drop(state);
                            drop(watch);
                            return;
                        }
                        state.watch = Some(watch);
                        drop(state);
                        armed_wake();
                    }
                    Err(error) => {
                        let mut state = worker_state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if worker_cancelled.load(Ordering::Acquire) {
                            return;
                        }
                        state.failure = Some(error);
                        drop(state);
                        armed_wake();
                    }
                }
            },
        )?;
        crate::linux_process::register_helper(start_worker);
        Ok(Self { state, cancelled })
    }

    /// Take the one start error, without waiting for an unfinished start worker.
    pub fn take_failure(&mut self) -> Option<io::Error> {
        match self.state.try_lock() {
            Ok(mut state) => state.failure.take(),
            Err(TryLockError::Poisoned(error)) => error.into_inner().failure.take(),
            Err(TryLockError::WouldBlock) => None,
        }
    }

    /// Whether the worker has installed the raw subscription.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        match self.state.try_lock() {
            Ok(state) => state.watch.is_some(),
            Err(TryLockError::Poisoned(error)) => {
                let state = error.into_inner();
                state.watch.is_some()
            }
            Err(TryLockError::WouldBlock) => false,
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        match self.state.try_lock() {
            Ok(mut state) => drop(state.watch.take()),
            Err(TryLockError::Poisoned(error)) => drop(error.into_inner().watch.take()),
            Err(TryLockError::WouldBlock) => {}
        }
    }
}

impl DirWatch {
    fn start_scoped(
        worker: &WorkerCtx,
        path: &Path,
        depth: WatchDepth,
        cancelled: &AtomicBool,
        wake: ChangeWake,
    ) -> Result<Self, io::Error> {
        check_cancelled(cancelled)?;
        let root = std::fs::canonicalize(path)?;
        if !std::fs::metadata(&root)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "a directory watch needs a directory",
            ));
        }

        let inotify = new_inotify()?;
        let (root_watch, watches) =
            install_initial_watches(worker, cancelled, inotify.as_raw_fd(), &root, depth)?;
        check_cancelled(cancelled)?;
        let stop = new_eventfd()?;
        let retire = retirement_sender()?;
        let stop_raw = stop.as_raw_fd();
        let stop_requested = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_requested);
        let (armed, listening) = mpsc::channel();
        let thread =
            crate::spawn_at_priority("bt-dir-watch", ThreadPriority::Normal, move |watcher| {
                watch_loop(
                    watcher,
                    stop_raw,
                    WatchTree {
                        inotify,
                        root_watch,
                        root,
                        depth,
                        watches,
                    },
                    thread_stop,
                    armed,
                    wake,
                )
            })?;
        match listening.recv() {
            Ok(()) => Ok(Self {
                stop: Some(stop),
                thread: Some(thread),
                retire,
                cancelled: stop_requested,
            }),
            Err(_) => {
                stop_and_join(worker, RetiredWatch { stop, thread });
                Err(io::Error::other(
                    "the directory watcher ended before it armed",
                ))
            }
        }
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let (Some(stop), Some(thread)) = (self.stop.take(), self.thread.take()) {
            let _ = self.retire.send(RetiredWatch { stop, thread });
        }
    }
}

fn retirement_sender() -> Result<mpsc::Sender<RetiredWatch>, io::Error> {
    let mut retirement = RETIREMENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(retirement) = retirement.as_ref() {
        return Ok(retirement.sender.clone());
    }
    let (sender, receiver) = mpsc::channel();
    let worker = crate::spawn_at_priority(
        "bt-dir-watch-retire",
        ThreadPriority::Normal,
        move |worker| retirement_loop(worker, receiver),
    )?;
    *retirement = Some(Retirement {
        sender: sender.clone(),
        worker,
    });
    Ok(sender)
}

/// Drain stopped watches after application owners and startup workers are gone.
pub fn shutdown_watches(_worker: &WorkerCtx) -> Result<(), String> {
    let retirement = RETIREMENT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(Retirement { sender, worker }) = retirement {
        drop(sender);
        let worker: std::thread::JoinHandle<()> = worker;
        worker
            .join()
            .map_err(|_| "Linux directory-watch retirement worker panicked".to_owned())?;
    }
    Ok(())
}

fn retirement_loop(worker: &WorkerCtx, receiver: mpsc::Receiver<RetiredWatch>) {
    while let Ok(retired) = receiver.recv() {
        stop_and_join(worker, retired);
    }
}

fn stop_and_join(_worker: &WorkerCtx, retired: RetiredWatch) {
    let RetiredWatch { stop, thread } = retired;
    let thread: std::thread::JoinHandle<()> = thread;
    let signal = 1_u64;
    // The eventfd stays owned until the watcher has exited and been joined.
    loop {
        // SAFETY: `signal` is aligned and `stop` is the eventfd created above.
        let written = unsafe {
            libc::write(
                stop.as_raw_fd(),
                (&signal as *const u64).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        if written >= 0 {
            break;
        }
        match io::Error::last_os_error().kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => break,
            _ => break,
        }
    }
    let _ = thread.join();
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), io::Error> {
    if cancelled.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "directory watch start was cancelled",
        ))
    } else {
        Ok(())
    }
}

struct Event {
    watch: RawFd,
    mask: u32,
    name: Option<std::ffi::OsString>,
}

enum ReadBatch {
    Events(Vec<Event>),
    Unknown,
}

struct WatchTree {
    inotify: OwnedFd,
    root_watch: RawFd,
    root: PathBuf,
    depth: WatchDepth,
    watches: HashMap<RawFd, Vec<PathBuf>>,
}

fn watch_loop(
    worker: &WorkerCtx,
    stop: RawFd,
    tree: WatchTree,
    stop_requested: Arc<AtomicBool>,
    armed: mpsc::Sender<()>,
    wake: ChangeWake,
) {
    let WatchTree {
        inotify,
        root_watch,
        root,
        depth,
        mut watches,
    } = tree;
    let mut storage = vec![0_u32; BUFFER_BYTES / std::mem::size_of::<u32>()];
    let mut intentional_removals = HashSet::new();
    if armed.send(()).is_err() {
        return;
    }
    loop {
        if stop_requested.load(Ordering::Acquire) {
            break;
        }
        let mut descriptors = [
            libc::pollfd {
                fd: inotify.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stop,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: both descriptors remain open for the whole call and the
        // stack array has exactly the two initialized pollfd records passed in.
        let ready = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, -1) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if descriptors[1].revents & libc::POLLIN != 0 {
            break;
        }
        if descriptors[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            break;
        }
        if descriptors[0].revents & libc::POLLIN == 0 {
            continue;
        }

        // The u32 backing keeps the kernel buffer aligned. The read itself
        // cannot exceed the buffer supplied to it.
        let byte_count = unsafe {
            libc::read(
                inotify.as_raw_fd(),
                storage.as_mut_ptr().cast(),
                std::mem::size_of_val(storage.as_slice()),
            )
        };
        if byte_count < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            break;
        }
        if byte_count == 0 {
            continue;
        }
        // SAFETY: the read count is positive and bounded by the writable
        // storage passed to `read` above.
        let bytes =
            unsafe { std::slice::from_raw_parts(storage.as_ptr().cast(), byte_count as usize) };
        let batch = match parse_events(bytes) {
            Ok(batch) => batch,
            Err(()) => ReadBatch::Unknown,
        };
        let mut changed = false;
        let mut unknown = matches!(&batch, ReadBatch::Unknown);
        let mut names = Vec::new();
        let mut root_removed = false;

        let events = match batch {
            ReadBatch::Events(events) => events,
            ReadBatch::Unknown => Vec::new(),
        };
        for event in &events {
            if event.mask & libc::IN_Q_OVERFLOW != 0 {
                unknown = true;
                changed = true;
                continue;
            }
            if event.watch == root_watch
                && event.mask
                    & (libc::IN_DELETE_SELF
                        | libc::IN_MOVE_SELF
                        | libc::IN_UNMOUNT
                        | libc::IN_IGNORED)
                    != 0
            {
                root_removed = true;
                continue;
            }
            if event.mask & libc::IN_IGNORED != 0 {
                if ignored_watch_was_unexpected(
                    event.watch,
                    &mut intentional_removals,
                    &mut watches,
                ) {
                    unknown = true;
                    changed = true;
                }
                continue;
            }
            if event.mask & libc::IN_UNMOUNT != 0 {
                unknown = true;
                changed = true;
            }
            if event.mask & ACTIVITY_MASK == 0 {
                continue;
            }
            let Some(parent_paths) = watches.get(&event.watch).cloned() else {
                continue;
            };
            changed = true;

            if depth == WatchDepth::HereOnly && event.watch == root_watch {
                if let Some(name) = &event.name {
                    names.push(name.clone());
                } else {
                    unknown = true;
                }
            }

            if depth == WatchDepth::Tree {
                if update_tree_watches_for_event(
                    worker,
                    &stop_requested,
                    inotify.as_raw_fd(),
                    event,
                    &parent_paths,
                    &mut watches,
                    &mut intentional_removals,
                ) {
                    unknown = true;
                }
                if event.mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) != 0
                    && let Some(paths) = watches.get(&event.watch).cloned()
                {
                    for watched_path in paths {
                        if remove_descendants(
                            inotify.as_raw_fd(),
                            &watched_path,
                            &mut watches,
                            &mut intentional_removals,
                        ) {
                            unknown = true;
                        }
                    }
                }
            }
        }
        if root_removed || stop_requested.load(Ordering::Acquire) {
            break;
        }
        if unknown
            && depth == WatchDepth::Tree
            && rebuild_tree(
                worker,
                &stop_requested,
                inotify.as_raw_fd(),
                root_watch,
                &root,
                &mut watches,
                &mut intentional_removals,
            )
            .is_err()
        {
            // The root watch remains installed; a later root-level change
            // still wakes the owner even if a subtree could not be read.
        }
        if stop_requested.load(Ordering::Acquire) {
            break;
        }
        if changed || unknown {
            if depth == WatchDepth::HereOnly && !unknown && names.is_empty() {
                continue;
            }
            if depth == WatchDepth::HereOnly && !unknown {
                wake(DirChange::Named(&names));
            } else {
                wake(DirChange::Unknown);
            }
        }
    }
}

fn parse_events(bytes: &[u8]) -> Result<ReadBatch, ()> {
    let header_size = std::mem::size_of::<libc::inotify_event>();
    let mut offset = 0;
    let mut events = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < header_size {
            return Err(());
        }
        // SAFETY: the length check above proves there is a full header, and an
        // unaligned read avoids making assumptions about the slice's origin.
        let header = unsafe {
            std::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<libc::inotify_event>())
        };
        let name_start = offset + header_size;
        let name_end = name_start.checked_add(header.len as usize).ok_or(())?;
        if name_end > bytes.len() {
            return Err(());
        }
        let name = if header.len == 0 {
            None
        } else {
            let region = &bytes[name_start..name_end];
            let length = region
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(region.len());
            Some(std::ffi::OsString::from_vec(region[..length].to_vec()))
        };
        events.push(Event {
            watch: header.wd,
            mask: header.mask,
            name,
        });
        offset = name_end;
    }
    Ok(ReadBatch::Events(events))
}

fn new_inotify() -> Result<OwnedFd, io::Error> {
    // SAFETY: this call takes only flags and returns a new descriptor on
    // success, which is immediately given one OwnedFd owner.
    let descriptor = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    owned_descriptor(descriptor)
}

fn new_eventfd() -> Result<OwnedFd, io::Error> {
    // SAFETY: this call takes only flags and returns a new descriptor on
    // success, which is immediately given one OwnedFd owner.
    let descriptor = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
    owned_descriptor(descriptor)
}

fn owned_descriptor(descriptor: RawFd) -> Result<OwnedFd, io::Error> {
    if descriptor < 0 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: the raw descriptor came from one of the constructors above
        // and has not yet been wrapped or closed.
        Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
    }
}

fn add_directory_watch(descriptor: RawFd, path: &Path) -> Result<RawFd, io::Error> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory path contains an embedded NUL",
        )
    })?;
    // SAFETY: path is NUL-terminated and lives through the call; descriptor is
    // an open inotify descriptor owned by the caller.
    let watch = unsafe { libc::inotify_add_watch(descriptor, path.as_ptr(), WATCH_MASK) };
    if watch < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(watch)
    }
}
fn install_descendants(
    _worker: &WorkerCtx,
    cancelled: &AtomicBool,
    descriptor: RawFd,
    root: &Path,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
) -> Result<(), io::Error> {
    check_cancelled(cancelled)?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(parent) = pending.pop() {
        check_cancelled(cancelled)?;
        let entries = std::fs::read_dir(&parent)?;
        for entry in entries {
            check_cancelled(cancelled)?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !file_type.is_dir() {
                continue;
            }
            let child = entry.path();
            add_path_watch(descriptor, &child, watches)?;
            pending.push(child);
        }
    }
    Ok(())
}

fn install_initial_watches(
    worker: &WorkerCtx,
    cancelled: &AtomicBool,
    descriptor: RawFd,
    root: &Path,
    depth: WatchDepth,
) -> Result<(RawFd, HashMap<RawFd, Vec<PathBuf>>), io::Error> {
    check_cancelled(cancelled)?;
    let root_watch = add_directory_watch(descriptor, root)?;
    let mut watches = HashMap::from([(root_watch, vec![root.to_path_buf()])]);
    if depth == WatchDepth::Tree {
        install_descendants(worker, cancelled, descriptor, root, &mut watches)?;
    }
    Ok((root_watch, watches))
}

fn install_subtree(
    worker: &WorkerCtx,
    cancelled: &AtomicBool,
    descriptor: RawFd,
    root: &Path,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
) -> Result<(), io::Error> {
    check_cancelled(cancelled)?;
    match add_path_watch(descriptor, root, watches) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    install_descendants(worker, cancelled, descriptor, root, watches)
}

fn add_path_watch(
    descriptor: RawFd,
    path: &Path,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
) -> Result<(), io::Error> {
    let watch = add_directory_watch(descriptor, path)?;
    let paths = watches.entry(watch).or_default();
    if !paths.iter().any(|watched| watched == path) {
        paths.push(path.to_path_buf());
    }
    Ok(())
}

fn update_tree_watches_for_event(
    worker: &WorkerCtx,
    cancelled: &AtomicBool,
    descriptor: RawFd,
    event: &Event,
    parent_paths: &[PathBuf],
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
    intentional_removals: &mut HashSet<RawFd>,
) -> bool {
    if check_cancelled(cancelled).is_err() || event.mask & libc::IN_ISDIR == 0 {
        return false;
    }
    let Some(name) = &event.name else {
        return false;
    };
    let mut lost_tracking = false;
    for parent in parent_paths {
        if check_cancelled(cancelled).is_err() {
            return lost_tracking;
        }
        let child = parent.join(name);
        if event.mask & (libc::IN_DELETE | libc::IN_MOVED_FROM) != 0
            && remove_descendants(descriptor, &child, watches, intentional_removals)
        {
            lost_tracking = true;
        }
        if event.mask & (libc::IN_CREATE | libc::IN_MOVED_TO) != 0
            && install_subtree(worker, cancelled, descriptor, &child, watches).is_err()
        {
            lost_tracking = true;
        }
    }
    lost_tracking
}

fn remove_kernel_watch(descriptor: RawFd, watch: RawFd) -> Result<(), io::Error> {
    loop {
        // SAFETY: the descriptor is an open inotify descriptor and `watch` is
        // an ID returned by one of its `inotify_add_watch` calls.
        let result = unsafe { libc::inotify_rm_watch(descriptor, watch) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EINVAL) => return Ok(()),
            _ => return Err(error),
        }
    }
}

fn ignored_watch_was_unexpected(
    watch: RawFd,
    intentional_removals: &mut HashSet<RawFd>,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
) -> bool {
    if intentional_removals.remove(&watch) {
        false
    } else {
        watches.remove(&watch).is_some()
    }
}

fn remove_descendants(
    descriptor: RawFd,
    root: &Path,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
    intentional_removals: &mut HashSet<RawFd>,
) -> bool {
    let affected: Vec<(RawFd, Vec<PathBuf>)> = watches
        .iter()
        .filter(|(_, paths)| paths.iter().any(|path| path.starts_with(root)))
        .map(|(watch, paths)| (*watch, paths.clone()))
        .collect();
    let mut failed = false;
    for (watch, paths) in affected {
        let remaining: Vec<_> = paths
            .into_iter()
            .filter(|path| !path.starts_with(root))
            .collect();
        if remaining.is_empty() {
            match remove_kernel_watch(descriptor, watch) {
                Ok(()) => {
                    watches.remove(&watch);
                    intentional_removals.insert(watch);
                }
                Err(_) => failed = true,
            }
        } else if let Some(watched_paths) = watches.get_mut(&watch) {
            *watched_paths = remaining;
        }
    }
    failed
}

fn rebuild_tree(
    worker: &WorkerCtx,
    cancelled: &AtomicBool,
    descriptor: RawFd,
    root_watch: RawFd,
    root: &Path,
    watches: &mut HashMap<RawFd, Vec<PathBuf>>,
    intentional_removals: &mut HashSet<RawFd>,
) -> Result<(), io::Error> {
    check_cancelled(cancelled)?;
    let stale: Vec<_> = watches
        .keys()
        .copied()
        .filter(|watch| *watch != root_watch)
        .collect();
    for watch in stale {
        check_cancelled(cancelled)?;
        remove_kernel_watch(descriptor, watch)?;
        watches.remove(&watch);
        intentional_removals.insert(watch);
    }
    check_cancelled(cancelled)?;
    watches.insert(root_watch, vec![root.to_path_buf()]);
    install_descendants(worker, cancelled, descriptor, root, watches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct WatcherExit(mpsc::Sender<()>);

    impl Drop for WatcherExit {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/linux-port-team/worker3-test")
                .join(format!(
                    "{label}-{}-{}",
                    std::process::id(),
                    NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
                ));
            std::fs::create_dir_all(&root).expect("create a disposable test tree under target");
            Self(root)
        }

        fn rename_to(&mut self, destination: PathBuf) {
            std::fs::rename(&self.0, &destination).expect("rename a disposable watched root");
            self.0 = destination;
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn on_worker<R>(work: impl FnOnce(&WorkerCtx) -> R + Send + 'static) -> R
    where
        R: Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let thread = crate::spawn_at_priority(
            "bt-dir-watch-test-worker",
            ThreadPriority::Normal,
            move |worker| {
                let _ = sender.send(work(worker));
            },
        )
        .expect("start a test worker through the thread door");
        let answer = receiver.recv().expect("the test worker returns");
        thread
            .join()
            .expect("the test worker exits after its answer");
        answer
    }

    type InitialWatches = (OwnedFd, RawFd, HashMap<RawFd, Vec<PathBuf>>);

    fn initial_watches(root: PathBuf, depth: WatchDepth) -> Result<InitialWatches, io::Error> {
        on_worker(move |worker| {
            let inotify = new_inotify()?;
            let cancelled = AtomicBool::new(false);
            let (root_watch, watches) =
                install_initial_watches(worker, &cancelled, inotify.as_raw_fd(), &root, depth)?;
            Ok((inotify, root_watch, watches))
        })
    }
    fn queued_events(descriptor: RawFd) -> Vec<Event> {
        let mut storage = vec![0_u32; BUFFER_BYTES / std::mem::size_of::<u32>()];
        let mut events = Vec::new();
        loop {
            // SAFETY: callers pass the open nonblocking inotify descriptor;
            // `storage` is a live writable buffer for the full read.
            let byte_count = unsafe {
                libc::read(
                    descriptor,
                    storage.as_mut_ptr().cast(),
                    std::mem::size_of_val(storage.as_slice()),
                )
            };
            if byte_count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                if error.kind() == io::ErrorKind::WouldBlock {
                    break;
                }
                panic!("read queued inotify events: {error}");
            }
            if byte_count == 0 {
                break;
            }
            // SAFETY: the read count is positive and bounded by the writable
            // storage passed to `read` above.
            let bytes =
                unsafe { std::slice::from_raw_parts(storage.as_ptr().cast(), byte_count as usize) };
            let ReadBatch::Events(batch) =
                parse_events(bytes).expect("decode queued inotify events")
            else {
                panic!("a parsed inotify read contains events");
            };
            events.extend(batch);
        }
        events
    }

    #[test]
    fn initial_recursive_arm_reports_a_new_name_from_the_existing_descendant_watch() {
        let scratch = Scratch::new("tree-watch");
        let nested = scratch.0.join("existing/nested");
        std::fs::create_dir_all(&nested).expect("make descendants before subscribing");
        let (inotify, root_watch, watches) = initial_watches(scratch.0.clone(), WatchDepth::Tree)
            .expect("install the initial recursive subscriptions");
        let nested_watch = watches
            .iter()
            .find_map(|(watch, paths)| paths.contains(&nested).then_some(*watch))
            .expect("the existing nested directory has a kernel watch");
        assert_ne!(nested_watch, root_watch);

        let name = std::ffi::OsString::from("after-start.txt");
        std::fs::write(nested.join(&name), b"watched below an existing directory")
            .expect("write below the subscribed tree");
        let events = queued_events(inotify.as_raw_fd());
        assert!(events.iter().any(|event| {
            event.watch == nested_watch
                && event.name.as_ref() == Some(&name)
                && event.mask & libc::IN_CREATE != 0
        }));
    }

    #[test]
    fn a_new_directory_event_installs_its_subtree_before_a_child_write() {
        let scratch = Scratch::new("dynamic-tree-watch");
        let child = scratch.0.join("new/nested");
        std::fs::create_dir_all(&child).expect("make the directory named by a controlled event");
        let inotify = new_inotify().expect("create the product's inotify descriptor");
        let root_watch = add_directory_watch(inotify.as_raw_fd(), &scratch.0)
            .expect("watch the parent of the new directory");
        let descriptor = inotify.as_raw_fd();
        let watches = HashMap::from([(root_watch, vec![scratch.0.clone()])]);
        let intentional_removals = HashSet::new();
        let root_path = scratch.0.clone();
        let event = Event {
            watch: root_watch,
            mask: libc::IN_CREATE | libc::IN_ISDIR,
            name: Some(std::ffi::OsString::from("new")),
        };
        let (lost_tracking, watches) = on_worker(move |worker| {
            let cancelled = AtomicBool::new(false);
            let mut watches = watches;
            let mut intentional_removals = intentional_removals;
            let lost_tracking = update_tree_watches_for_event(
                worker,
                &cancelled,
                descriptor,
                &event,
                &[root_path],
                &mut watches,
                &mut intentional_removals,
            );
            (lost_tracking, watches)
        });
        assert!(!lost_tracking);
        let nested_watch = watches
            .iter()
            .find_map(|(watch, paths)| paths.contains(&child).then_some(*watch))
            .expect("the nested directory has a kernel watch");
        let file = child.join("after-install.txt");
        std::fs::write(&file, b"the new subtree is watched")
            .expect("write after the controlled create event was handled");
        let events = queued_events(inotify.as_raw_fd());
        assert!(events.iter().any(|event| {
            event.watch == nested_watch
                && event.name.as_deref() == Some(std::ffi::OsStr::new("after-install.txt"))
                && event.mask & libc::IN_CREATE != 0
        }));
    }

    #[test]
    fn a_named_shallow_watch_preserves_non_utf8_entry_names() {
        let scratch = Scratch::new("named-watch");
        let (sender, receiver) = mpsc::channel();
        let watch = Subscription::start_shallow_named(&scratch.0, move |change| {
            let names = match change {
                DirChange::Named(names) => names.to_vec(),
                DirChange::Unknown => Vec::new(),
            };
            let _ = sender.send(names);
        })
        .expect("subscribe to immediate entries");
        assert!(
            receiver
                .recv()
                .expect("the armed subscription requests its initial rescan")
                .is_empty()
        );
        assert!(watch.is_armed());

        let name = std::ffi::OsString::from_vec(b"entry-\xff.txt".to_vec());
        std::fs::write(scratch.0.join(&name), b"named by the kernel")
            .expect("create a non-UTF8 entry after arming");
        let names = receiver.recv().expect("the entry's name arrives");
        assert!(names.iter().any(|candidate| candidate == &name));

        drop(watch);
    }

    #[test]
    fn start_reports_missing_and_non_directory_paths_after_queueing() {
        let scratch = Scratch::new("watch-errors");
        let missing = scratch.0.join("missing");
        let (sender, receiver) = mpsc::channel();
        let mut watch = Subscription::start(&missing, move || {
            let _ = sender.send(());
        })
        .expect("queue a subscription before the worker checks the path");
        receiver.recv().expect("the failed start wakes its owner");
        let missing_error = watch.take_failure().expect("the worker stores its failure");
        assert_eq!(missing_error.kind(), io::ErrorKind::NotFound);
        assert!(!watch.is_armed());

        let file = scratch.0.join("file");
        std::fs::write(&file, b"not a directory").expect("make a regular file");
        let (sender, receiver) = mpsc::channel();
        let mut watch = Subscription::start_shallow(&file, move || {
            let _ = sender.send(());
        })
        .expect("queue a shallow subscription");
        receiver
            .recv()
            .expect("the failed shallow start wakes its owner");
        let file_error = watch.take_failure().expect("the worker stores its failure");
        assert_eq!(file_error.kind(), io::ErrorKind::NotADirectory);
        assert!(!watch.is_armed());
    }

    #[test]
    fn root_rename_ends_the_subscription_without_a_callback() {
        let mut scratch = Scratch::new("root-rename");
        let (callback_sender, callbacks) = mpsc::channel();
        let (worker_exit_sender, worker_exit_receiver) = mpsc::channel();
        let worker_exit = WatcherExit(worker_exit_sender);
        let watch = Subscription::start_shallow_named(&scratch.0, move |_change| {
            let _ = &worker_exit;
            let _ = callback_sender.send(());
        })
        .expect("queue a watch for a disposable root");
        callbacks
            .recv()
            .expect("the armed subscription requests its initial rescan");

        scratch.rename_to(scratch.0.with_extension("renamed"));
        worker_exit_receiver
            .recv()
            .expect("the watcher callback drops after its worker exits");
        assert!(
            callbacks.try_recv().is_err(),
            "a changed root ends the watcher without reporting a stale listing"
        );
        drop(watch);
    }

    #[test]
    fn an_unexpected_descendant_watch_loss_is_reconciled_to_a_live_watch() {
        let scratch = Scratch::new("watch-reconcile");
        let nested = scratch.0.join("existing/nested");
        std::fs::create_dir_all(&nested).expect("make a watched descendant");
        let (inotify, root_watch, mut watches) =
            initial_watches(scratch.0.clone(), WatchDepth::Tree)
                .expect("install the recursive subscriptions");
        let lost_watch = watches
            .iter()
            .find_map(|(watch, paths)| paths.contains(&nested).then_some(*watch))
            .expect("the nested directory has a kernel watch");

        remove_kernel_watch(inotify.as_raw_fd(), lost_watch)
            .expect("simulate the kernel losing a still-existing directory watch");
        assert!(
            queued_events(inotify.as_raw_fd())
                .iter()
                .any(|event| { event.watch == lost_watch && event.mask & libc::IN_IGNORED != 0 })
        );
        let mut intentional_removals = HashSet::new();
        assert!(ignored_watch_was_unexpected(
            lost_watch,
            &mut intentional_removals,
            &mut watches,
        ));
        assert!(!watches.contains_key(&lost_watch));

        let rebuild_root = scratch.0.clone();
        let descriptor = inotify.as_raw_fd();
        (watches, intentional_removals) = on_worker(move |worker| {
            let cancelled = AtomicBool::new(false);
            let mut watches = watches;
            let mut intentional_removals = intentional_removals;
            rebuild_tree(
                worker,
                &cancelled,
                descriptor,
                root_watch,
                &rebuild_root,
                &mut watches,
                &mut intentional_removals,
            )?;
            Ok::<_, io::Error>((watches, intentional_removals))
        })
        .expect("reconcile all descendants from the still-watched root");
        let repaired_watch = watches
            .iter()
            .find_map(|(watch, paths)| paths.contains(&nested).then_some(*watch))
            .expect("the nested directory has been watched again");

        let name = std::ffi::OsString::from("after-reconcile.txt");
        std::fs::write(nested.join(&name), b"the rearmed watch reports changes")
            .expect("change the reconciled descendant");
        let events = queued_events(inotify.as_raw_fd());
        for event in events
            .iter()
            .filter(|event| event.mask & libc::IN_IGNORED != 0)
        {
            assert!(!ignored_watch_was_unexpected(
                event.watch,
                &mut intentional_removals,
                &mut watches,
            ));
        }
        assert!(events.iter().any(|event| {
            event.watch == repaired_watch
                && event.name.as_ref() == Some(&name)
                && event.mask & libc::IN_CREATE != 0
        }));
    }

    #[test]
    fn raw_start_returns_only_after_a_new_entry_is_observable() {
        let scratch = Scratch::new("raw-start-arm");
        let root = scratch.0.clone();
        let (sender, receiver) = mpsc::channel();
        let watch = on_worker(move |worker| {
            let cancelled = AtomicBool::new(false);
            let wake: ChangeWake = Box::new(move |change: DirChange<'_>| {
                if let DirChange::Named(names) = change {
                    let _ = sender.send(names.to_vec());
                }
            });
            DirWatch::start_scoped(worker, &root, WatchDepth::HereOnly, &cancelled, wake)
        })
        .expect("the raw start returns an armed watcher");

        let name = std::ffi::OsString::from("after-raw-return.txt");
        std::fs::write(scratch.0.join(&name), b"the raw watcher was already armed")
            .expect("create an entry after raw start returned");
        let names = receiver
            .recv()
            .expect("the armed watcher reports the new entry");
        assert!(names.iter().any(|candidate| candidate == &name));
        drop(watch);
    }

    #[test]
    fn dropping_a_subscription_queues_retirement_until_its_worker_exits() {
        let scratch = Scratch::new("queued-retirement");
        let (callback_sender, callbacks) = mpsc::channel();
        let (worker_exit_sender, worker_exit_receiver) = mpsc::channel();
        let worker_exit = WatcherExit(worker_exit_sender);
        let watch = Subscription::start_shallow(&scratch.0, move || {
            let _ = &worker_exit;
            let _ = callback_sender.send(());
        })
        .expect("queue a shallow subscription");
        callbacks
            .recv()
            .expect("the watcher is armed before the initial wake");

        drop(watch);
        worker_exit_receiver
            .recv()
            .expect("the retired watcher drops its callback");
        assert!(callbacks.try_recv().is_err());
    }
}
