//! **A playback engine's load: how long it may take, what a state says about a
//! source's length, and when a load that has said nothing is over** — one
//! owner for both playback arms (`video::engine` on Windows, the AVFoundation
//! player on a Mac), which each re-export what a caller names.
//!
//! The two arms write their own [`EngineState`](crate::video::engine::EngineState)
//! because the type is interleaved with each platform's file (see
//! `video_portable.rs`'s module note). What is here is the part that is a rule
//! rather than a platform: the budget, the length's three readings, and the
//! deadline's decision. Each pump asks [`LoadDeadline::judge`] every turn and
//! maps its answer onto its own state, so the rule is written and tested once.

use std::time::{Duration, Instant};

use crate::video::engine::OPEN_BUDGET;

/// **How long a source may take, from the open, to say what it is before not
/// having said so is the engine's own refusal** (`EngineError::NotLoaded`).
///
/// The platforms answer a load with an event either way — the metadata, or an
/// error — and nothing in either platform bounds how long it may take to send
/// one. Without this, "no length, no size, no error" is a state an engine can
/// stand in for ever, and a surface showing it is a player at `0:00` that never
/// starts.
///
/// Measured from the open request, on the engine thread, so it covers the build
/// and then the load. The build half is [`OPEN_BUDGET`], which is measured
/// (a hundred milliseconds cold, forty warm) and set two orders of magnitude
/// past that. **The load half is not measured**: it is given the same five
/// seconds by analogy — a local file's metadata is a read of its header, the
/// same order of work as building the engine — so the whole is twice
/// [`OPEN_BUDGET`]. Each pump reads it on its own turn, at the latest every
/// `IDLE_POLL_INTERVAL`, so a refusal is said at most one poll late and no
/// caller keeps a clock for it.
pub const LOAD_BUDGET: Duration = OPEN_BUDGET.saturating_mul(2);

/// **How long a test waits for an engine to give its terminal answer about a
/// load** — loaded, or refused by the platform, or refused by [`LOAD_BUDGET`].
///
/// Derived from the product's own constants and not a number of its own: the
/// budget, plus the two idle polls the refusal can be late by (the turn that
/// reads the deadline and the one that publishes it), plus one second for a
/// loaded runner to schedule the engine thread at all. A patience equal to the
/// budget would end a moment before the refusal could land, so a wait that ran
/// out would be a coin toss between "no answer" and `NotLoaded`; past this one,
/// "no answer" is a defect.
#[cfg(any(test, feature = "trust-harness"))]
#[doc(hidden)]
pub const LOAD_ANSWER_PATIENCE: Duration = LOAD_BUDGET
    .saturating_add(crate::video::engine::IDLE_POLL_INTERVAL.saturating_mul(2))
    .saturating_add(Duration::from_secs(1));

/// **A source's length, as one of the three things a state can say about it.**
///
/// `duration_secs` is `None` for two different reasons, and a surface draws
/// them differently: a player still waiting for its metadata, and a source that
/// has loaded and declares no length — a live stream, or a container written
/// without a duration. The second is an end state and not a load that has not
/// finished; a player shows it as having no length rather than as a clock that
/// has not started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    /// Nothing has been said yet: the metadata has not arrived, or the engine
    /// refused before it did.
    Unanswered,
    /// The source declares this many seconds.
    Known(f64),
    /// The source has loaded and declares no length.
    Unknown,
}

impl Length {
    /// The reading of a state's `duration_secs` and `ready` — what each arm's
    /// `EngineState::length` answers.
    #[must_use]
    pub fn of(duration_secs: Option<f64>, ready: bool) -> Self {
        match (duration_secs, ready) {
            (Some(secs), _) => Self::Known(secs),
            (None, true) => Self::Unknown,
            (None, false) => Self::Unanswered,
        }
    }
}

/// **When a load is over without having said anything**: [`LOAD_BUDGET`] after
/// the open request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LoadDeadline {
    due: Instant,
}

/// **What a pump publishes about its load this turn.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Load {
    /// Whatever the media says stands: loading, loaded, or refused by the
    /// platform itself.
    AsSaid,
    /// The engine's own refusal: the pump publishes `NotLoaded`, not ready, and
    /// no length or size.
    Refused,
}

impl LoadDeadline {
    /// The deadline of a load asked for at `asked_at`.
    pub(crate) fn from_open(asked_at: Instant) -> Self {
        Self {
            due: asked_at + LOAD_BUDGET,
        }
    }

    /// **The deadline's decision, every turn.**
    ///
    /// * A load already refused stays refused: **the refusal is the terminal**,
    ///   so a source that answers after it — ready, sized, with a length — is
    ///   not published, and no state ever says `ready` beside `NotLoaded`.
    /// * A source that is ready, or that the platform has already refused for a
    ///   reason of its own, is as it says, at any time.
    /// * Otherwise the load is refused once `now` has reached the deadline.
    pub(crate) fn judge(self, now: Instant, ready: bool, errored: bool, refused: bool) -> Load {
        if refused || (!ready && !errored && now >= self.due) {
            Load::Refused
        } else {
            Load::AsSaid
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **a load is the engine's refusal only past its budget, only while
    /// nothing has been said, and for good once it is**
    /// (T-VIDEO-ENGINE-NO-DURATION round 2).
    ///
    /// The one decision both playback arms make every turn, tested once on
    /// every machine; each arm only maps the answer onto its own state. On a
    /// clock this test holds: a breath before the budget is still a load; at
    /// the budget a load that has said nothing is refused; a ready source and
    /// one the platform refused itself are never refused for time; and a load
    /// that was refused stays refused when the source answers later.
    ///
    /// MUTATION: make `judge` answer `Load::AsSaid` always — never overdue — and
    /// the second assertion goes red.
    #[test]
    fn a_load_is_refused_only_past_its_budget_and_for_good() {
        let asked = Instant::now();
        let deadline = LoadDeadline::from_open(asked);
        let due = asked + LOAD_BUDGET;
        let nearly = due - Duration::from_millis(1);
        assert_eq!(
            deadline.judge(nearly, false, false, false),
            Load::AsSaid,
            "a breath before the budget is still a load"
        );
        assert_eq!(
            deadline.judge(due, false, false, false),
            Load::Refused,
            "a load that has said nothing by its budget is refused"
        );
        assert_eq!(
            deadline.judge(due + LOAD_BUDGET, true, false, false),
            Load::AsSaid,
            "a ready source is never refused for time, with a length or without"
        );
        assert_eq!(
            deadline.judge(due + LOAD_BUDGET, false, true, false),
            Load::AsSaid,
            "a source the platform refused keeps the platform's reason"
        );
        assert_eq!(
            deadline.judge(due, true, true, true),
            Load::Refused,
            "a refused load stays refused when the source answers late"
        );
        assert_eq!(
            LOAD_BUDGET,
            OPEN_BUDGET * 2,
            "the build and then the load, each given what the build is given"
        );
    }

    /// RED — **a length is read three ways, and `None` once ready is "no
    /// length"** (T-VIDEO-ENGINE-NO-DURATION).
    ///
    /// MUTATION: read `(None, true)` as `Unanswered` — a loaded source with no
    /// length taken for one still loading — and the second assertion goes red.
    #[test]
    fn a_length_is_known_unknown_or_unanswered() {
        assert_eq!(Length::of(Some(5.0), true), Length::Known(5.0));
        assert_eq!(Length::of(None, true), Length::Unknown);
        assert_eq!(Length::of(None, false), Length::Unanswered);
    }
}
