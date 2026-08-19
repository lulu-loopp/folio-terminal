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
//! # What is left in this file
//!
//! The mechanism — the handle, the one-slot mailbox and the debounce — moved to
//! [`crate::dir_news`] on the day `profiles.json` needed the same three things
//! over `%APPDATA%\Folio\` itself. What stays here is the part that is about
//! *this* folder: where it is, which event wakes the loop, and the two moments
//! above. A notification still says only *the folder moved*; what that means is
//! decided on the main thread, where the catalogue and the palette are.

use std::time::Instant;

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, dir_news::DirNews};

/// The schemes folder's subscription and the clock its notifications feed.
#[derive(Default)]
pub struct SchemeWatch {
    news: DirNews,
}

impl SchemeWatch {
    /// Open the folder's watch, if it is not open and the folder is there.
    ///
    /// Failure is quiet and is retried only at the next of the two moments named
    /// in the module header — retrying on a schedule is a timer, and a timer is
    /// the thing this mechanism exists to avoid.
    pub fn arm(&mut self, proxy: &EventLoopProxy<AppEvent>) {
        let directory = crate::persist::storage_dir().join(crate::schemes::USER_SCHEME_DIR);
        match self.news.arm(&directory, proxy, AppEvent::SchemesChanged) {
            Ok(()) => {}
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
    pub fn due(&mut self, now: Instant) -> bool {
        self.news.due(now)
    }

    /// When the loop must wake to answer news it already has, if it has any.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.news.deadline()
    }

    /// Whether the folder's watch is open — the one fact a test can check
    /// without a filesystem event.
    #[cfg(test)]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.news.is_armed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A watch that never opened still answers every question — the case of a
    /// machine whose `%APPDATA%\Folio\schemes` does not exist.
    ///
    /// The arithmetic behind these three answers is claimed in
    /// [`crate::dir_news`]'s own suite; what is claimed here is that the schemes
    /// folder's wrapper hands every question through to it.
    #[test]
    fn an_unarmed_watch_is_inert_rather_than_absent() {
        let mut watch = SchemeWatch::default();
        assert!(!watch.is_armed());
        assert!(!watch.due(Instant::now()));
        assert_eq!(watch.deadline(), None);
    }
}
