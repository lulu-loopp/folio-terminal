//! **The kernel's word that a file in `%APPDATA%\Folio\` was saved by somebody
//! else** (§7.1.6c-6d).
//!
//! [`crate::scheme_watch`]'s shape one folder up, and the shape is deliberate:
//! two files a person may edit by hand must not be followed by two different
//! mechanisms, or the answer to "when does the window notice" becomes a
//! question about which file you edited. Both are one always-armed
//! `ReadDirectoryChangesW` handle, one one-slot mailbox and one debounce, all
//! three of them [`crate::dir_news`]'s, with a folder's own policy — this file —
//! wrapped round them.
//!
//! # One folder is one subscription, however many files are in it
//!
//! This was `profile_watch` until `pins.json` arrived (2026-08-19), and the
//! rename is the whole of what changed: it never watched `profiles.json`: it
//! watched the folder that file is in, and answered a notification by re-reading
//! the file and comparing. A second handle on the same directory for the second
//! hand-editable file would have been the exact duplication [`crate::dir_news`]
//! was extracted to prevent — "two copies of a debounce is how two surfaces end
//! up disagreeing about what *it stopped changing* means" — and would have paid
//! a second kernel handle and a second thread to learn the same word twice. So
//! the folder has one subscription, and what a notification is *worth* is
//! decided once per file on the main thread, where the documents are.
//!
//! # The folder is `%APPDATA%\Folio\`, and it always exists
//!
//! This is the one place this and the scheme watch genuinely differ. `schemes\`
//! is a folder that may not exist yet, so its watch is armed at two moments and
//! its failure to open is the ordinary case; the storage directory is created by
//! the first store that opens (`SessionStore::open`, before this is ever armed),
//! so arming happens once at startup and a failure is worth a line.
//!
//! It is also the folder every other file this product writes lives in, and
//! `DirWatch` watches a tree. So a `session.json` flush, a `settings.json`
//! toggle, a scheme saved into the subfolder — each of them wakes this clock
//! too. That is answered rather than filtered, and answered where the answer is
//! cheap: the rescan re-reads two small files and compares each with the
//! document already in force, and a rescan that finds the same bytes changes
//! nothing. Filtering here would mean parsing the kernel's
//! `FILE_NOTIFY_INFORMATION` names, which is the one thing `DirWatch` promises
//! never to do — and it would buy nothing that the comparison does not already
//! buy, because **this window's own writes have to be answered by that
//! comparison anyway**. Every keystroke in the profile editor writes
//! `profiles.json`; every press on a pin writes `pins.json`; without the
//! compare, a watcher would read back its own writing and call it news.
//!
//! # What a notification means here
//!
//! *The folder moved.* Which file, and whether it matters, is decided on the
//! main thread by re-reading `profiles.json` and `pins.json` — `reread_profiles`
//! and `reread_pins`, beside `reread_schemes`, once per turn of the loop and
//! only after the folder has held still.

use std::time::Instant;

use winit::event_loop::EventLoopProxy;

use crate::{AppEvent, dir_news::DirNews};

/// The storage folder's one subscription, held for every hand-editable file in
/// it.
#[derive(Default)]
pub struct StorageWatch {
    news: DirNews,
}

impl StorageWatch {
    /// Open the watch on `%APPDATA%\Folio\`, once, at startup.
    ///
    /// A failure leaves the window with the documents it read at launch and no
    /// way to hear about a hand edit until the next one — which is exactly what
    /// this build did before the watch existed, so it is a line for whoever is
    /// holding the door open and not a card. There is no retry: retrying is a
    /// timer, and a timer is the thing this mechanism exists to avoid.
    ///
    /// **And there is deliberately no `NotFound` arm**, which is the one place
    /// this differs from [`crate::scheme_watch::SchemeWatch::arm`] and its
    /// `worth_a_line`. That arm exists over there because `schemes\` legitimately
    /// may not exist; `%APPDATA%\Folio\` is created by the first store that opens
    /// (`SessionStore::open`, before this is ever armed), so its absence is not
    /// an ordinary case at all — it is the interesting half of the news, and
    /// swallowing it would hide the machine where nothing this product writes is
    /// landing anywhere.
    pub fn arm(&mut self, proxy: &EventLoopProxy<AppEvent>) {
        let directory = crate::persist::storage_dir();
        if let Err(error) = self.news.arm(&directory, proxy, AppEvent::StorageChanged) {
            eprintln!(
                "recoverable storage watch failure on {}: {error}",
                directory.display()
            );
        }
    }

    /// Fold in whatever the watcher thread has said and answer whether a re-read
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

    /// A watch that never opened still answers every question, and a window
    /// holding one is a window that simply never hears about a hand edit.
    ///
    /// The arithmetic behind these three answers is claimed in
    /// [`crate::dir_news`]'s own suite; what is claimed here is that this
    /// folder's wrapper hands every question through to it.
    #[test]
    fn an_unarmed_watch_is_inert_rather_than_absent() {
        let mut watch = StorageWatch::default();
        assert!(!watch.is_armed());
        assert!(!watch.due(Instant::now()));
        assert_eq!(watch.deadline(), None);
    }
}
