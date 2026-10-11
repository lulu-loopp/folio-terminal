//! **How a directory watch ends on its own** (T-DIRWATCH-LIFECYCLE; the B5 design note
//! `docs/plans/design/os-signals-2026-10-11.md`, revision (b)5).
//!
//! A watch that was armed can stop listening without anybody dropping it: on Windows a read the
//! kernel refuses or a handle it closes under the watcher (the directory removed, the share gone);
//! on macOS FSEvents saying the watched root changed or its volume was unmounted. Both arms keep
//! that end here, in one place: the error that ended the watch, and whether it has ended. The
//! watcher thread records it once, then calls the caller's wake with `DirChange::Ended`; the
//! owner's `is_armed` reads it and `take_failure` takes the error, once.
//!
//! A stop the owner asks for by dropping the watch is not an end: it records nothing.

use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};

/// The one store of a watch's terminal failure, shared by its owner and its watcher thread.
#[derive(Default)]
pub(crate) struct WatchEnd {
    /// Set by the watcher thread, after `failure` holds the error, and never cleared.
    ended: AtomicBool,
    /// The error that ended the watch, until the owner takes it.
    failure: Mutex<Option<std::io::Error>>,
}

impl WatchEnd {
    /// **The watcher thread's last word**: the error is stored first and the flag set after it,
    /// so an owner that sees the watch unarmed finds the error waiting.
    pub(crate) fn record(&self, error: std::io::Error) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
        self.ended.store(true, Ordering::Release);
    }

    /// Whether the watch is still listening.
    pub(crate) fn still_armed(&self) -> bool {
        !self.ended.load(Ordering::Acquire)
    }

    /// The error that ended the watch, handed over once.
    pub(crate) fn take_failure(&self) -> Option<std::io::Error> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}
