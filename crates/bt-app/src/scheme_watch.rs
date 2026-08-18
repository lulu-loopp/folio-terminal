//! **The kernel's word that a scheme file was saved** (§7.1.6c-4c).
//!
//! # One directory, one handle, for the life of the process
//!
//! `git_watch` holds a set of subscriptions that comes and goes with what is on
//! screen, because a repository is only watched while a page is showing it and
//! there can be any number of them. This is the opposite shape and gets the
//! opposite treatment: there is exactly one schemes folder, it is small, and a
//! window is always wearing a scheme out of it — so there is nothing to gate the
//! watch on and nothing to be gained by dropping it. It is armed once and kept.
//!
//! It costs nothing to keep, which is the point of using a watch rather than a
//! check: an armed `DirWatch` over a folder nobody touches wakes for nothing, so
//! "always on" here means one kernel handle and one blocked thread, not a timer.
//!
//! # Armed at two moments, and never by asking
//!
//! `%APPDATA%\Folio\schemes\` does not exist on a fresh install — deliberately,
//! since `schemes::user_sources` refuses to create it — so there are two moments
//! at which arming can succeed and this module is driven from both:
//!
//! 1. **Startup**, if the folder is already there.
//! 2. **Immediately after `Customise scheme…` writes into it**, which is the one
//!    way this window itself brings the folder into existence.
//!
//! What it deliberately does *not* do is ask whether the folder exists yet on
//! some schedule. A folder made by hand mid-session is found on the next launch,
//! which is the answer §7.1.6c-4a already gives for a file dropped in mid-session
//! and is stated here rather than left to be discovered. [`SchemeWatch::arm`] is
//! therefore cheap to call and does nothing at all once it has succeeded, so its
//! callers do not have to know which of the two moments they are.
//!
//! # What a notification means here
//!
//! Nothing is parsed out of it — same rule `git_watch` states, one level down:
//! the file names are read for nothing, so a save, a rename and a delete cost
//! the same thought and all three arrive as *the folder moved*. What that means
//! is decided on the main thread, where the catalogue and the palette are.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, watch_clock::WatchClock};

/// The schemes folder's subscription and the clock its notifications feed.
#[derive(Default)]
pub struct SchemeWatch {
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

impl SchemeWatch {
    /// Open the folder's watch, if it is not open and the folder is there.
    ///
    /// One `CreateFileW` and no separate existence check: a folder that is not
    /// there fails to open, and asking twice would be two syscalls to learn one
    /// thing. Failure is quiet and is retried only at the next of the two
    /// moments named in the module header — retrying on a schedule is a timer,
    /// and a timer is the thing this mechanism exists to avoid.
    pub fn arm(&mut self, proxy: &EventLoopProxy<AppEvent>) {
        if self.watch.is_some() {
            return;
        }
        let directory = crate::persist::storage_dir().join(crate::schemes::USER_SCHEME_DIR);
        let news = Arc::clone(&self.news);
        let proxy = proxy.clone();
        match bt_platform::DirWatch::start(&directory, move || {
            *news
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
            // The loop is woken, not told what to do.
            let _ = proxy.send_event(AppEvent::SchemesChanged);
        }) {
            Ok(watch) => self.watch = Some(watch),
            // A folder that is not there is the ordinary case on a fresh
            // install and is not news. Anything else — a permission, a path on
            // a redirected `%APPDATA%` this process may not open — is a line for
            // whoever is holding the door open, and nothing more: the schemes
            // already in force stay in force, the picker still works, and a file
            // edited on disk arrives on the next launch. A toast here would be
            // an alarm about a capability nobody asked for by name.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "recoverable scheme watch failure on {}: {error}",
                directory.display()
            ),
        }
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

    /// Whether the folder's watch is open — the one fact a test can check
    /// without a filesystem event.
    #[cfg(test)]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.watch.is_some()
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
        let mut watch = SchemeWatch::default();
        let now = Instant::now();
        assert_eq!(watch.deadline(), None);
        assert!(!watch.due(now));
        assert!(!watch.due(now + Duration::from_secs(60)));
        assert_eq!(watch.deadline(), None);
    }

    /// PIN — a save is one rescan, a quiet window after the last thing that
    /// moved.
    ///
    /// An editor writing one file produces several notifications — the write,
    /// the rename of a temp file, the attribute change — and this is the claim
    /// that they are one piece of news.
    #[test]
    fn a_burst_of_notifications_is_one_rescan_a_quiet_window_after_the_last() {
        let mut watch = SchemeWatch::default();
        let start = Instant::now();
        for step in [0, 4, 11] {
            *watch.news.lock().unwrap() = Some(start + Duration::from_millis(step));
            assert!(
                !watch.due(start + Duration::from_millis(step)),
                "nothing fires while the folder is still moving"
            );
        }
        assert_eq!(
            watch.deadline(),
            Some(start + Duration::from_millis(11) + crate::watch_clock::WATCH_QUIET)
        );
        assert!(!watch.due(start + Duration::from_millis(310)));
        assert!(watch.due(start + Duration::from_millis(311)));
        assert_eq!(watch.deadline(), None, "and then it is quiet again");
    }

    /// A watch that never opened still answers every question — the case of a
    /// machine whose `%APPDATA%\Folio\schemes` does not exist.
    #[test]
    fn an_unarmed_watch_is_inert_rather_than_absent() {
        let mut watch = SchemeWatch::default();
        assert!(!watch.is_armed());
        assert!(!watch.due(Instant::now()));
        assert_eq!(watch.deadline(), None);
    }
}
