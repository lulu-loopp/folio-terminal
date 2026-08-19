//! **One folder's subscription and the clock its notifications feed** — the
//! half of a directory watcher that has nothing to do with what is in the
//! directory.
//!
//! `watch_clock` was lifted out of `git_watch` the day a second directory
//! needed the identical debounce, and its own header says why: "two copies of a
//! debounce is how two surfaces end up disagreeing about what *it stopped
//! changing* means". This is that lift carried one step further, on the day a
//! *third* directory arrived (`%APPDATA%\Folio\`, for `profiles.json`): the
//! clock was already shared, and what was still written twice was the handle
//! beside it, the one-slot mailbox the watcher thread stamps, and the rule that
//! the two are read together once per turn of the loop.
//!
//! What is **not** here is every reason a particular folder is watched: which
//! directory, which [`AppEvent`] wakes the loop, when arming is attempted and
//! what a failure to arm is worth saying. Those differ per folder and are the
//! whole content of [`crate::scheme_watch`] and [`crate::profile_watch`], which
//! are this type wearing one folder's policy each.
//!
//! # What a notification means here
//!
//! Nothing is parsed out of it — `git_watch`'s rule, one level down: the file
//! names are read for nothing, so a save, a rename and a delete cost the same
//! thought and all three arrive as *the folder moved*. What that means is
//! decided on the main thread, where the catalogue and the table are.
//!
//! # Why an always-armed handle is not a timer
//!
//! `DirWatch` blocks in the kernel until the filesystem speaks. A folder nobody
//! touches wakes nothing, arms no clock and asks for no deadline, which is what
//! keeps R31's "a repository is not read because time passed" true of a watch
//! that is never dropped.

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, watch_clock::WatchClock};

/// A folder's watch, the time it last moved, and the debounce between them.
#[derive(Default)]
pub struct DirNews {
    /// `None` until the folder exists and the handle opens; `Some` for the rest
    /// of the process.
    watch: Option<bt_platform::DirWatch>,
    /// When the watcher thread last saw something move, written there and read
    /// here. One `Option<Instant>` and not a queue: every notification says the
    /// same word, so the only thing worth keeping is the most recent time it was
    /// said.
    news: Arc<Mutex<Option<Instant>>>,
    clock: WatchClock,
}

impl DirNews {
    /// Whether the folder's watch is open — the one fact a test can check
    /// without a filesystem event.
    ///
    /// Behind `cfg(test)` because arming is idempotent by itself: no caller ever
    /// has to ask before arming, and a question only the suite asks is a
    /// question the shipped binary should not carry.
    #[cfg(test)]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.watch.is_some()
    }

    /// Open the folder's watch and wake the loop with `event` whenever it moves.
    ///
    /// One `CreateFileW` and no separate existence check: a folder that is not
    /// there fails to open, and asking twice would be two syscalls to learn one
    /// thing. The error is handed back rather than reported here — whether a
    /// missing folder is the ordinary case or a broken installation is a fact
    /// about *which* folder, and this type does not know which folder it is
    /// holding.
    pub fn arm(
        &mut self,
        directory: &Path,
        proxy: &EventLoopProxy<AppEvent>,
        event: AppEvent,
    ) -> Result<(), std::io::Error> {
        if self.watch.is_some() {
            return Ok(());
        }
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        let watch = bt_platform::DirWatch::start(directory, move || {
            *news
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
            // The loop is woken, not told what to do.
            let _ = proxy.send_event(event);
        })?;
        self.watch = Some(watch);
        Ok(())
    }

    /// Fold in whatever the watcher thread has said and answer whether a rescan
    /// is due.
    ///
    /// Called once per turn of the event loop, like `GitWatch::due`, and the
    /// whole of the timing lives in [`WatchClock`].
    pub fn due(&mut self, now: Instant) -> bool {
        if let Some(at) = self
            .news
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            self.clock.note_event(at);
        }
        self.clock.take_due(now)
    }

    /// When the loop must wake to answer news it already has, if it has any.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.clock.due_at()
    }

    /// Stamp the mailbox the way the watcher thread does, so the arithmetic
    /// above can be claimed without a filesystem.
    #[cfg(test)]
    pub fn note(&self, at: Instant) {
        *self
            .news
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// PIN — a folder nobody touches costs nothing: no deadline, no rescan, no
    /// wake-up.
    ///
    /// This is R31's own sentence read for a directory that is always watched:
    /// "always on" must mean the handle, never a clock.
    #[test]
    fn a_folder_nobody_touches_owes_no_rescan_and_no_wake_up() {
        let mut news = DirNews::default();
        let now = Instant::now();
        assert_eq!(news.deadline(), None);
        assert!(!news.due(now));
        assert!(!news.due(now + Duration::from_secs(60)));
        assert_eq!(news.deadline(), None);
    }

    /// PIN — a save is one rescan, a quiet window after the last thing that
    /// moved.
    ///
    /// An editor writing one file produces several notifications — the write,
    /// the rename of a temp file, the attribute change — and this is the claim
    /// that they are one piece of news.
    #[test]
    fn a_burst_of_notifications_is_one_rescan_a_quiet_window_after_the_last() {
        let mut news = DirNews::default();
        let start = Instant::now();
        for step in [0, 4, 11] {
            news.note(start + Duration::from_millis(step));
            assert!(
                !news.due(start + Duration::from_millis(step)),
                "nothing fires while the folder is still moving"
            );
        }
        assert_eq!(
            news.deadline(),
            Some(start + Duration::from_millis(11) + crate::watch_clock::WATCH_QUIET)
        );
        assert!(!news.due(start + Duration::from_millis(310)));
        assert!(news.due(start + Duration::from_millis(311)));
        assert_eq!(news.deadline(), None, "and then it is quiet again");
    }

    /// A watch that never opened still answers every question — the case of a
    /// machine whose `%APPDATA%\Folio\schemes` does not exist.
    #[test]
    fn an_unarmed_watch_is_inert_rather_than_absent() {
        let mut news = DirNews::default();
        assert!(!news.is_armed());
        assert!(!news.due(Instant::now()));
        assert_eq!(news.deadline(), None);
    }
}
