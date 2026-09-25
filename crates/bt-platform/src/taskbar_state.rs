//! **The latest answer about the taskbar, numbered** (0.4.5 ticket 62).
//!
//! [`crate::taskbar_is_auto_hidden`] is a question put to another process —
//! `SHAppBarMessage(ABM_GETSTATE)` is a message to Explorer's taskbar, and it
//! waits for Explorer to answer. The owner's stall report of 2026-09-25 caught
//! it at 99 ms and 92 ms twice in one 535 ms hold of the window thread, while
//! Explorer was busy. So the window thread no longer asks: a lane asks, and
//! puts its answer here, and the window thread reads the latest one.
//!
//! **One word, so a reading is never torn.** The answer and the number of the
//! request it answers travel together in one `AtomicU64` — the number in the
//! high 63 bits, the answer in the lowest — so a reader can never see one
//! request's number beside another request's answer, and a read is one load
//! with no lock to wait on.
//!
//! **An older answer never replaces a newer one.** Requests are numbered by
//! the lane that asks; [`TaskbarState::offer`] takes an answer only if its
//! number is larger than the one held. That is `ARCHITECTURE.md` §4.2's rule
//! for an observation of external state — the owner of the observation alone
//! accepts a result against the current request — said where the value lives
//! rather than trusted to how many threads happen to answer.

use std::sync::atomic::{AtomicU64, Ordering};

/// One reading of [`TaskbarState`]: the answer, and which request it answers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskbarReading {
    /// The number of the request this answers. **`0` means no answer has
    /// landed yet**: requests are numbered from `1`.
    pub generation: u64,
    /// The shell's taskbar hides itself. `false` until an answer lands — the
    /// direction [`crate::taskbar_auto_hidden_from_state`] argues for a shell
    /// that does not answer, and the one whose mistake can be taken back: a
    /// flash can be stopped, a desktop message cannot be unsent.
    pub auto_hidden: bool,
}

impl TaskbarReading {
    /// Whether this is a real answer rather than the value held before the
    /// first one landed.
    #[must_use]
    pub fn answered(self) -> bool {
        self.generation != 0
    }

    fn from_word(word: u64) -> Self {
        Self {
            generation: word >> 1,
            auto_hidden: word & 1 != 0,
        }
    }

    fn word(self) -> u64 {
        (self.generation << 1) | u64::from(self.auto_hidden)
    }
}

/// The slot: the latest answer and its number, one writer (the lane), any
/// number of readers that never wait.
#[derive(Debug, Default)]
pub struct TaskbarState {
    word: AtomicU64,
}

impl TaskbarState {
    /// No answer yet: [`TaskbarReading::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            word: AtomicU64::new(0),
        }
    }

    /// The latest answer. One atomic load — nothing to wait on, whatever the
    /// lane is doing.
    #[must_use]
    pub fn read(&self) -> TaskbarReading {
        TaskbarReading::from_word(self.word.load(Ordering::Acquire))
    }

    /// **Hold the answer to request `generation`, unless a newer one is
    /// already held.** `true` when it was taken.
    ///
    /// An answer whose number is not larger than the held one is dropped and
    /// changes nothing. Requests are counted from `1` by one lane, so a number
    /// never reaches the top bit this word gives away to the answer.
    pub fn offer(&self, generation: u64, auto_hidden: bool) -> bool {
        let offered = TaskbarReading {
            generation,
            auto_hidden,
        }
        .word();
        let mut held = self.word.load(Ordering::Acquire);
        loop {
            if generation <= TaskbarReading::from_word(held).generation {
                return false;
            }
            match self.word.compare_exchange_weak(
                held,
                offered,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(now) => held = now,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskbarReading, TaskbarState};

    /// RED (62) — **An older answer about the taskbar never overwrites a newer
    /// one, and the slot reads "not answered, not hidden" until the first
    /// lands.**
    ///
    /// The lane numbers its requests and the slot is the one place that
    /// compares numbers; a second asker, or a retried one, cannot put back an
    /// answer the reader has already moved past.
    ///
    /// MUTATION: drop the generation comparison in `TaskbarState::offer` — red
    /// (the older `false` replaces the newer `true`).
    #[test]
    fn an_older_answer_about_the_taskbar_never_overwrites_a_newer_one() {
        let slot = TaskbarState::new();
        assert_eq!(slot.read(), TaskbarReading::default());
        assert!(!slot.read().answered());

        assert!(slot.offer(2, true), "the first answer is taken");
        assert!(!slot.offer(1, false), "an older answer is dropped");
        assert!(
            !slot.offer(2, false),
            "the same request is not answered twice"
        );
        assert_eq!(
            slot.read(),
            TaskbarReading {
                generation: 2,
                auto_hidden: true
            }
        );
        assert!(slot.offer(3, false), "a newer answer is taken");
        assert_eq!(
            slot.read(),
            TaskbarReading {
                generation: 3,
                auto_hidden: false
            }
        );
    }
}
