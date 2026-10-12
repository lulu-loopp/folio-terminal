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
//! whole content of [`crate::scheme_watch`], which is this type wearing the
//! schemes folder's policy. The storage folder has its own named, shallow ear
//! ([`crate::storage_watch`]): which of its documents was named is the news
//! there, and a nameless tree watch cannot say it.
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
//!
//! # Where the watch lives
//!
//! Not here (T-DIRWATCH-LIFECYCLE): the folder is this type's ear on the watch
//! steward ([`crate::watch_steward`]), whose worker starts the watch, keeps it
//! and drops it, so arming never waits on the window thread. Its answer —
//! heard, or not and why — comes back as a report, read on the next turn.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use winit::event_loop::EventLoopProxy;

use crate::{
    AppEvent,
    watch_clock::WatchClock,
    watch_steward::{Cause, Depth, EarHandle, MemberReport},
};

/// A folder's watch, the time it last moved, and the debounce between them.
#[derive(Default)]
pub struct DirNews {
    /// The folder's ear on the watch steward, opened by the first arming.
    ear: Option<EarHandle>,
    /// The folder the ear asks for.
    directory: Option<PathBuf>,
    /// What the steward last said: `None` before its first answer, `Some(true)`
    /// while the folder is heard, `Some(false)` while it is not.
    heard: Option<bool>,
    /// The refusal or end the steward reported and nobody has taken yet.
    failure: Option<Cause>,
    /// When the watcher thread last saw something move, written there and read
    /// here. One `Option<Instant>` and not a queue: every notification says the
    /// same word, so the only thing worth keeping is the most recent time it was
    /// said.
    news: Arc<Mutex<Option<Instant>>>,
    clock: WatchClock,
}

impl DirNews {
    /// Whether the steward says the folder is heard — the one fact a test can
    /// check without a filesystem event.
    ///
    /// Behind `cfg(test)` because arming is idempotent by itself: no caller ever
    /// has to ask before arming, and a question only the suite asks is a
    /// question the shipped binary should not carry.
    #[cfg(test)]
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.heard == Some(true)
    }

    /// The refusal or end the steward reported for this folder, handed over
    /// once.
    pub(crate) fn take_failure(&mut self) -> Option<std::io::Error> {
        self.hear_the_steward();
        self.failure.take().map(|cause| cause.to_error())
    }

    /// **Ask for the folder's watch**, waking the loop with `event` whenever it
    /// moves. Never waits: the steward starts it, and its answer is read on a
    /// later turn ([`Self::take_failure`]).
    ///
    /// An arming of a folder that is not heard is the reader-triggered attempt
    /// the steward's `Failed` rule waits for: it is tried once more. An arming of
    /// a folder that is heard, or still being started, asks nothing new.
    ///
    /// **The ear is opened before the proxy is cloned only once, and that order
    /// is load-bearing** — the reason is written out at
    /// [`crate::git_watch::GitWatch::want`]: a clone is an `Arc` bump on Windows
    /// and, on macOS, a new run loop source and a wake-up of the loop. Arming is
    /// idempotent and every caller leans on that, so a clone taken per arming
    /// would be one taken per asking however many watches were opened.
    pub fn arm(&mut self, directory: &Path, proxy: &EventLoopProxy<AppEvent>, event: AppEvent) {
        self.arm_with(directory, |news| {
            let news = Arc::clone(news);
            let proxy = proxy.clone();
            crate::watch_steward::open_ear(
                Depth::Tree,
                Arc::new(move |_: &Path, _| {
                    *news
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
                    // The loop is woken, not told what to do.
                    let _ = proxy.send_event(event);
                }),
            )
        });
    }

    /// [`Self::arm`] with the opening of the ear handed in — the product's
    /// steward, or a test's.
    fn arm_with(
        &mut self,
        directory: &Path,
        open_ear: impl FnOnce(&Arc<Mutex<Option<Instant>>>) -> EarHandle,
    ) {
        self.hear_the_steward();
        let rearm = if self.heard == Some(false) {
            BTreeSet::from([directory.to_path_buf()])
        } else {
            BTreeSet::new()
        };
        let news = &self.news;
        let ear = self.ear.get_or_insert_with(|| open_ear(news));
        self.directory = Some(directory.to_path_buf());
        ear.want(&BTreeSet::from([directory.to_path_buf()]), rearm);
    }

    /// Fold in what the steward said about the folder.
    ///
    /// **A folder that has just armed is news** (T-DIRWATCH-LIFECYCLE round 2):
    /// the folder was read before the steward armed its watch on its own thread,
    /// and a write in between reached nobody. The arming is stamped in the
    /// mailbox like a notification, so the folder is read once more when it
    /// goes quiet.
    fn hear_the_steward(&mut self) {
        let Some(ear) = self.ear.as_ref() else {
            return;
        };
        for (directory, numbered) in ear.take_reports() {
            if self.directory.as_ref() != Some(&directory) {
                continue;
            }
            match numbered.report {
                MemberReport::Armed => {
                    if self.heard != Some(true) {
                        *self
                            .news
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            Some(Instant::now());
                    }
                    self.heard = Some(true);
                }
                MemberReport::Failed(cause) => {
                    if self.heard != Some(false) {
                        self.failure = Some(cause);
                    }
                    self.heard = Some(false);
                }
            }
        }
    }

    /// Fold in whatever the watcher thread has said and answer whether a rescan
    /// is due.
    ///
    /// Called once per turn of the event loop, like `GitWatch::due`, and the
    /// whole of the timing lives in [`WatchClock`].
    pub fn due(&mut self, now: Instant) -> bool {
        self.hear_the_steward();
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

    /// PIN (T-DIRWATCH-LIFECYCLE round 2) — **a write made between the folder's
    /// read and the steward's arming of its watch is read.**
    ///
    /// The schemes and storage folders are read at startup and armed after, on
    /// the steward's thread; a write in between is heard by no watch. The
    /// arming is the news: once `Armed` is reported, a rescan comes due when
    /// the folder is quiet. The gap is held open by a fake door holding the
    /// start.
    ///
    /// MUTATION: keep the first `Armed` a no-op in `hear_the_steward` (drop the
    /// mailbox stamp) — no rescan ever comes due.
    #[test]
    fn a_write_made_before_the_watch_armed_is_rescanned_once_it_arms() {
        use crate::lane::wait_for_a_wake;
        use crate::watch_steward::{Steward, fake::FakeDoor};
        use std::sync::mpsc;

        let door = FakeDoor::default();
        let steward: &'static Steward = Box::leak(Box::new(Steward::new(door.clone(), |_| {})));
        let (woke, wakes) = mpsc::channel();
        steward.install_wake(move || {
            let _ = woke.send(());
        });
        let mut news = DirNews::default();
        door.hold_starts();
        news.arm_with(Path::new("/Users/alice/配色"), |_| {
            steward.open_ear(Depth::Tree, Arc::new(|_: &Path, _| {}))
        });
        door.wait_for_a_held_start();
        door.release();
        wait_for_a_wake(&wakes, "the arming");

        let start = Instant::now();
        assert!(!news.due(start), "news, the folder not yet quiet");
        assert!(news.is_armed());
        assert!(
            news.due(start + Duration::from_secs(60)),
            "the arming is a rescan owed: what was written before it was heard by nobody"
        );
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
