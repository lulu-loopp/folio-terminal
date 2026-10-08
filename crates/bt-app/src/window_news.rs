//! **One seat per open window for news about the machine** (T-WINDOWS-ALL).
//!
//! A watch the application owns — one kernel subscription per subject, one
//! clock per subject — answers *something moved*. What that is worth is a
//! window's question: which of its surfaces are on the glass and standing on
//! that subject. Before this module the git watch answered it for the first
//! window alone, because the subscription, the clock and the reading of the
//! surfaces all lived on the window that turns the application's clocks, and a
//! second window's Git page never heard the disk.
//!
//! So the news is split at the one place it can be:
//!
//! * **the application** keeps the subscriptions and the clocks over the union
//!   of what every seat wants, and, when a subject's news ripens, *tells* every
//!   seat that wants it — the owning layer drains the shared news once and files
//!   it under each window's name (`docs/CONVENTIONS.md` §三, "a shared queue's
//!   address is unique all the way to the layer that takes the answer");
//! * **each window** says on its own turn what it wants (only what it draws:
//!   R31's "drawn, not merely turned to") and *takes* what it was told.
//!
//! **The seats are the window directory's** (`App::windows_open`): a window is
//! seated by the walk that names it — the door that opens it and the turn's head
//! — and unseated by the walk that no longer does, which is how a closed window
//! stops being told anything and its subjects stop being subscribed to. A seat
//! is never made anywhere else, so news can only reach a window the directory
//! says is open.

use std::collections::{BTreeMap, BTreeSet};

/// What one window wants to hear about, and what it has been told and has not
/// yet taken.
struct WindowSeat<S> {
    wanted: BTreeSet<S>,
    owed: BTreeSet<S>,
}

/// **Every open window's seat**, keyed by the window.
///
/// Generic over the window key so the fan-out is tested with plain numbers, as
/// the update card's presenters are (`update_job::Presenters`).
pub struct WindowSeats<W, S> {
    seats: BTreeMap<W, WindowSeat<S>>,
}

impl<W, S> Default for WindowSeats<W, S> {
    fn default() -> Self {
        Self {
            seats: BTreeMap::new(),
        }
    }
}

impl<W: Ord + Copy, S: Ord + Clone> WindowSeats<W, S> {
    /// **Bring the seats level with the window directory**: a window it names
    /// gets a seat (wanting nothing until its own turn says otherwise), and a
    /// window it no longer names loses its seat, with whatever it wanted and
    /// whatever it was owed.
    ///
    /// Answers whether any seat was released, which is whether the union of what
    /// is wanted can have shrunk.
    pub fn level_with(&mut self, open: impl IntoIterator<Item = W>) -> bool {
        let open: BTreeSet<W> = open.into_iter().collect();
        let before = self.seats.len();
        self.seats.retain(|window, _| open.contains(window));
        let released = self.seats.len() != before;
        for window in open {
            self.seats.entry(window).or_insert_with(|| WindowSeat {
                wanted: BTreeSet::new(),
                owed: BTreeSet::new(),
            });
        }
        released
    }

    /// **What this window wants to hear about now.** A window with no seat is
    /// not open by the directory's word, and wants nothing.
    ///
    /// Answers whether its wish changed. A subject it no longer wants is no
    /// longer owed to it either: what it was told about a surface that has left
    /// the glass is answered by the reading that surface takes when it comes
    /// back.
    pub fn want(&mut self, window: W, wanted: BTreeSet<S>) -> bool {
        let Some(seat) = self.seats.get_mut(&window) else {
            return false;
        };
        if seat.wanted == wanted {
            return false;
        }
        seat.owed.retain(|subject| wanted.contains(subject));
        seat.wanted = wanted;
        true
    }

    /// Everything any open window wants: the set the application's
    /// subscriptions follow.
    #[must_use]
    pub fn wanted(&self) -> BTreeSet<S> {
        self.seats
            .values()
            .flat_map(|seat| seat.wanted.iter().cloned())
            .collect()
    }

    /// **News about `subject`, for every window that wants it.**
    pub fn tell(&mut self, subject: &S) {
        for seat in self.seats.values_mut() {
            if seat.wanted.contains(subject) {
                seat.owed.insert(subject.clone());
            }
        }
    }

    /// What this window has been told and not yet acted on, handed over once.
    pub fn take(&mut self, window: W) -> BTreeSet<S> {
        self.seats
            .get_mut(&window)
            .map(|seat| std::mem::take(&mut seat.owed))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(subjects: &[&str]) -> BTreeSet<String> {
        subjects
            .iter()
            .map(|subject| (*subject).to_owned())
            .collect()
    }

    /// PIN — **news reaches every window that wants it, and only those.**
    ///
    /// MUTATION: `tell` inserting into the first seat only — the second window
    /// is never told, which is the defect this module exists for.
    #[test]
    fn news_reaches_every_window_that_wants_it_and_no_other() {
        let mut seats: WindowSeats<u32, String> = WindowSeats::default();
        seats.level_with([1, 2, 3]);
        seats.want(1, set(&["仓库 a"]));
        seats.want(2, set(&["仓库 a", "repo b"]));
        seats.want(3, set(&["repo b"]));
        assert_eq!(seats.wanted(), set(&["仓库 a", "repo b"]));

        seats.tell(&"仓库 a".to_owned());
        assert_eq!(seats.take(1), set(&["仓库 a"]));
        assert_eq!(seats.take(2), set(&["仓库 a"]));
        assert_eq!(seats.take(3), set(&[]), "window 3 does not want it");
        assert_eq!(seats.take(1), set(&[]), "taken once");
    }

    /// PIN — **a window the directory no longer names is told nothing and wants
    /// nothing.**
    ///
    /// MUTATION: `level_with` without its `retain` (the closed window's seat
    /// leaks) — the dropped window is still told, and what it alone wanted is
    /// still in the union the subscriptions follow.
    #[test]
    fn a_window_the_directory_dropped_is_told_nothing_and_wants_nothing() {
        let mut seats: WindowSeats<u32, String> = WindowSeats::default();
        assert!(!seats.level_with([1, 2]));
        seats.want(1, set(&["repo a"]));
        seats.want(2, set(&["repo a", "仓库 b"]));

        assert!(
            seats.level_with([1]),
            "window 2 closed: its seat is released"
        );
        assert_eq!(seats.wanted(), set(&["repo a"]));
        seats.tell(&"repo a".to_owned());
        seats.tell(&"仓库 b".to_owned());
        assert_eq!(seats.take(2), set(&[]), "a closed window is told nothing");
        assert!(
            !seats.want(2, set(&["仓库 b"])),
            "and cannot seat itself by wanting"
        );
        assert_eq!(seats.wanted(), set(&["repo a"]));
        assert_eq!(seats.take(1), set(&["repo a"]));
    }

    /// PIN — **a subject a window stopped wanting is no longer owed to it.**
    #[test]
    fn a_subject_left_behind_is_not_owed() {
        let mut seats: WindowSeats<u32, String> = WindowSeats::default();
        seats.level_with([7]);
        seats.want(7, set(&["repo a", "仓库 b"]));
        seats.tell(&"repo a".to_owned());
        seats.tell(&"仓库 b".to_owned());
        assert!(seats.want(7, set(&["仓库 b"])));
        assert!(
            !seats.want(7, set(&["仓库 b"])),
            "the same wish is no change"
        );
        assert_eq!(seats.take(7), set(&["仓库 b"]));
    }
}
