//! **A debounce fed by events** — the arithmetic two directory watchers share.
//!
//! Written for R31's fourth invalidation moment (§7.1.3g ②, D) and lifted out of
//! `git_watch` by §7.1.6c-4c, which needed the identical clock over a second
//! directory: `%APPDATA%\Folio\schemes\`. Two copies of a debounce is how two
//! surfaces end up disagreeing about what "it stopped changing" means, and this
//! one's behaviour is pinned by tests that are claims about *times*.
//!
//! # Why this is allowed to exist under a rule that forbids polling
//!
//! R31's sentence is *a repository is not read because time passed*. Nothing
//! here starts counting until a `ReadDirectoryChangesW` completion says
//! something moved; a window left open over an untouched tree runs no timer and
//! wakes for nothing. The difference from a poll is not one of degree: a poll's
//! question is "has anything changed yet", asked on a schedule its subject has
//! no say in, and this one's is "has it stopped changing", asked only because it
//! already did.
//!
//! # Pure, and separate from anything holding a handle
//!
//! A version of this living inside the watcher thread would only be checkable by
//! writing files and waiting.

use std::time::{Duration, Instant};

/// How long a directory has to hold still before its news is acted on.
///
/// A single `git add` is one or two notifications inside a millisecond of each
/// other; a `git commit` is a few dozen; an editor saving one file is a write, a
/// rename and an attribute change. Three hundred milliseconds is long enough
/// that all of them arrive as one piece of news and short enough that a reader
/// who typed the command — or pressed Ctrl+S — has not looked away yet.
pub const WATCH_QUIET: Duration = Duration::from_millis(300);

/// And the shortest interval between two re-reads of one directory, however much
/// is happening.
///
/// **This is what a `cargo build` costs.** A build writes for thirty seconds
/// without a three-hundred-millisecond gap anywhere in it, so the quiet window
/// alone would either say nothing for thirty seconds or — with no floor —
/// nothing at all until it ended. Two seconds is the compromise the ruling
/// names: the page keeps up with a build in progress, and the tree is asked no
/// more often than a person could read the answer.
///
/// It costs the schemes folder something too, and knowingly: a second save
/// inside two seconds of the first is applied at the floor rather than at the
/// quiet window. That is the right trade there as well, because putting a
/// palette in force throws away every glyph raster keyed on `theme_revision` —
/// a scheme file saved in a loop must not be able to rebuild the window's whole
/// texture cache five times a second.
pub const WATCH_FLOOR: Duration = Duration::from_secs(2);

/// **The debounce, as arithmetic** — one directory's clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WatchClock {
    /// When the first notification since the last re-read arrived — the start of
    /// the news that is currently owed. `None` when nothing is owed, which is
    /// the whole of "this clock is not running".
    first_pending: Option<Instant>,
    /// And the most recent one, which is what the quiet window is measured from.
    last_event: Option<Instant>,
    /// When this directory was last re-read *because of this clock*, and the
    /// floor's own anchor.
    last_reread: Option<Instant>,
}

impl WatchClock {
    /// The kernel said something moved.
    pub fn note_event(&mut self, at: Instant) {
        self.first_pending.get_or_insert(at);
        self.last_event = Some(match self.last_event {
            Some(last) if last > at => last,
            _ => at,
        });
    }

    /// When the re-read this clock owes becomes due, if it owes one.
    ///
    /// Three terms, and each is one of the ruling's three sentences:
    ///
    /// - **quiet**: `last_event + QUIET` — wait for the tree to stop moving.
    /// - **cap**: `first_pending + FLOOR` — but never wait longer than the floor
    ///   after the news *started*, or a build that never goes quiet would keep
    ///   the panel silent for its whole duration.
    /// - **floor**: `last_reread + FLOOR` — and never sooner than that after the
    ///   last one, which is what makes a storm cost one reading every two
    ///   seconds instead of one every three hundred milliseconds.
    #[must_use]
    pub fn due_at(&self) -> Option<Instant> {
        let first = self.first_pending?;
        let last = self.last_event?;
        let natural = (last + WATCH_QUIET).min(first + WATCH_FLOOR);
        Some(match self.last_reread {
            Some(previous) => natural.max(previous + WATCH_FLOOR),
            None => natural,
        })
    }

    /// Is it due now, and if so, take it.
    ///
    /// Taking is what clears the news: after this the clock owes nothing until
    /// the kernel speaks again, which is what makes "nothing changed, nothing
    /// fires" a property of the type rather than of its caller.
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.due_at().is_none_or(|due| due > now) {
            return false;
        }
        self.first_pending = None;
        self.last_event = None;
        self.last_reread = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — **a burst is one reading, a storm is one reading every two seconds,
    /// and silence is nothing at all.**
    ///
    /// The `git_watch` suite pins the same three shapes against a repository's
    /// own story; this one pins them against the type, so that moving the clock
    /// out of that module did not move where its behaviour is guaranteed.
    #[test]
    fn a_burst_is_one_reading_and_silence_owes_nothing() {
        let start = Instant::now();
        let mut clock = WatchClock::default();

        assert_eq!(clock.due_at(), None, "silence owes nothing");
        assert!(!clock.take_due(start), "and nothing fires");

        clock.note_event(start);
        clock.note_event(start + Duration::from_millis(1));
        assert_eq!(
            clock.due_at(),
            Some(start + Duration::from_millis(1) + WATCH_QUIET),
            "the quiet window runs from the last event, not the first"
        );
        assert!(!clock.take_due(start + Duration::from_millis(200)));
        assert!(clock.take_due(start + Duration::from_millis(301)));
        assert_eq!(clock.due_at(), None, "one burst, one reading");
    }

    /// PIN — an event that arrives out of order does not pull the quiet window
    /// backwards.
    ///
    /// Two watcher threads can both wake one clock, and `Instant`s taken on two
    /// threads are not guaranteed to arrive in the order they were taken. A
    /// clock that took the later reading as "last" only when it was later is the
    /// difference between one reading and one that fires before the news ends.
    #[test]
    fn a_notification_that_arrives_late_does_not_move_the_window_back() {
        let start = Instant::now();
        let mut clock = WatchClock::default();
        clock.note_event(start + Duration::from_millis(100));
        clock.note_event(start);
        assert_eq!(
            clock.due_at(),
            Some(start + Duration::from_millis(100) + WATCH_QUIET)
        );
    }

    /// PIN — two savings inside the floor are two readings a floor apart, and
    /// the second is not lost.
    #[test]
    fn a_second_save_inside_the_floor_is_answered_at_the_floor_and_not_dropped() {
        let start = Instant::now();
        let mut clock = WatchClock::default();
        clock.note_event(start);
        let first = start + WATCH_QUIET;
        assert!(clock.take_due(first));

        let second = first + Duration::from_millis(500);
        clock.note_event(second);
        let due = clock.due_at().expect("the second save is owed");
        assert_eq!(due, first + WATCH_FLOOR, "held off by the floor");
        assert!(!clock.take_due(due - Duration::from_millis(1)));
        assert!(clock.take_due(due));
        assert_eq!(clock.due_at(), None);
    }
}
