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

/// Whether a refusal to arm this folder's watch is worth saying out loud.
///
/// **A folder that is not there is the ordinary case on a fresh install and is
/// not news.** `%APPDATA%\Folio\schemes` is created by nothing but
/// `Customise scheme…`, so on a machine where nobody has customised one its
/// absence is the state this product ships in. Anything else — a permission, a
/// path on a redirected `%APPDATA%` this process may not open — is a line for
/// whoever is holding the door open, and nothing more: the schemes already in
/// force stay in force, the picker still works, and a file edited on disk
/// arrives on the next launch. A toast here would be an alarm about a capability
/// nobody asked for by name.
///
/// A named predicate rather than a match arm, because until 2026-08-19 the arm
/// was **unreachable** and nothing said so: `bt_platform`'s `win32_io_error`
/// built its `io::Error` from the raw `HRESULT`, so `DirWatch::start` on a
/// missing folder answered `Uncategorized` and every fresh install printed the
/// line this rule exists to suppress. What was missing was not the rule but a
/// way to ask it a question.
#[must_use]
fn worth_a_line(error: &std::io::Error) -> bool {
    error.kind() != std::io::ErrorKind::NotFound
}

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
        if let Err(error) = self.news.arm(&directory, proxy, AppEvent::SchemesChanged)
            && worth_a_line(&error)
        {
            eprintln!(
                "recoverable scheme watch failure on {}: {error}",
                directory.display()
            );
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

    /// PIN — **the schemes folder not being there is not news**, asked of the
    /// refusal the platform actually produces.
    ///
    /// The rule was written the day the watch was, and for seven weeks it was
    /// not reachable: `bt_platform::win32_io_error` built its `io::Error` from
    /// the raw `HRESULT`, so the kind was `Uncategorized` and the `NotFound`
    /// arm never matched. Every fresh install printed a line about a folder
    /// whose absence is the ordinary case.
    ///
    /// So the error here is **not constructed** — `io::Error::from(NotFound)`
    /// would have passed on every one of those seven weeks. It is the one
    /// `DirWatch::start` hands back for a folder that is not there, which is the
    /// only thing this arm will ever be shown.
    ///
    /// MUTATION: put the raw `HRESULT` back in `win32_io_error` and this goes
    /// red, which is the stderr line coming back with it.
    #[test]
    fn a_schemes_folder_that_is_not_there_is_not_a_line_on_stderr() {
        let missing = std::env::temp_dir().join("folio-schemes-that-were-never-customised");
        let _ = std::fs::remove_dir_all(&missing);
        let error = bt_platform::DirWatch::start(&missing, || {})
            .err()
            .expect("a folder that is not there cannot be watched");
        assert!(
            !worth_a_line(&error),
            "a fresh install is not a fault to report: {error}"
        );

        // And the other half of the rule, so that the predicate is not simply
        // "never say anything": a refusal that is not an absence still speaks.
        assert!(worth_a_line(&std::io::Error::from(
            std::io::ErrorKind::PermissionDenied
        )));
    }
}
