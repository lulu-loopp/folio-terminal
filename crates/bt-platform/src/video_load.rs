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

/// **When an engine that has not come into being is the fault**:
/// [`OPEN_BUDGET`] after the open request (T-VIDEO-ENGINE-OPEN-BUDGET-ON-A-CLOCK,
/// plan row 152).
///
/// Neither arm's `Engine::open` waits for its engine to be built, so this is
/// read, not waited on: each arm's `Engine::state` hands it the instant it is
/// reading at — the real clock in the product, an instant a test chose in a
/// test — and answers `EngineError::Unresponsive` while it says missed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenDeadline {
    due: Instant,
}

impl OpenDeadline {
    /// The deadline of an engine asked for at `asked_at`.
    pub(crate) fn from_open(asked_at: Instant) -> Self {
        Self {
            due: asked_at + OPEN_BUDGET,
        }
    }

    /// **Whether a reading at `now` finds the build's budget missed.**
    ///
    /// `built` and `now` are asked lazily, in that order and only when due —
    /// the build only while nothing has been said, the clock only while
    /// nothing is built — so a built engine's every reading costs no clock read
    /// at all, as the `&&` chain this replaced did not.
    ///
    /// Only while nothing is built and nothing has been said: an engine thread
    /// that has published a refusal of its own keeps that refusal as the
    /// answer, whenever it is read, and one that has built its engine is no
    /// longer a build that can run out. Strictly past the budget, so a reading
    /// at the budget itself is still in time.
    ///
    /// It is a reading, not a latch: once the thread builds its engine late, or
    /// publishes a refusal, the next reading at the same `now` no longer says
    /// missed. Each arm's window retires a seat at the first reading that does
    /// (`bt-app`'s `video_seat::retirement`), so no surface sees the reading
    /// change.
    pub(crate) fn missed(
        self,
        now: impl FnOnce() -> Instant,
        built: impl FnOnce() -> bool,
        errored: bool,
    ) -> bool {
        !errored && !built() && now() > self.due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **an open is missed only past its budget, and only while nothing
    /// is built and nothing has been said** (plan row 152).
    ///
    /// The one decision both playback arms' `Engine::state` makes on every
    /// reading, tested once on every machine on instants the test chooses.
    ///
    /// And **lazily**: the clock is not read once the engine is built, and
    /// neither the build nor the clock once a refusal has been said — each
    /// counted by the closure that would have answered it.
    ///
    /// MUTATION: compare with `>=` and the reading at the budget is missed; drop
    /// `!built()` and the late build is still missed; drop `!errored` and the
    /// refusal is masked; read the clock first and the built engine's count is
    /// one.
    #[test]
    fn an_open_is_missed_only_past_its_budget_while_nothing_is_built_or_said() {
        use std::cell::Cell;
        let asked = Instant::now();
        let deadline = OpenDeadline::from_open(asked);
        let due = asked + OPEN_BUDGET;
        let past = due + Duration::from_nanos(1);
        assert!(
            !deadline.missed(|| due, || false, false),
            "a reading at the budget itself is still in time"
        );
        assert!(
            deadline.missed(|| past, || false, false),
            "past the budget with nothing built and nothing said is missed"
        );
        let clock_reads = Cell::new(0);
        let at_past = || {
            clock_reads.set(clock_reads.get() + 1);
            past
        };
        assert!(
            !deadline.missed(at_past, || true, false),
            "an engine built late is not missed"
        );
        assert_eq!(
            clock_reads.get(),
            0,
            "a built engine's reading reads no clock"
        );
        let build_asks = Cell::new(0);
        let unbuilt = || {
            build_asks.set(build_asks.get() + 1);
            false
        };
        assert!(
            !deadline.missed(at_past, unbuilt, true),
            "a refusal the engine thread said keeps its own cause"
        );
        assert_eq!(
            (build_asks.get(), clock_reads.get()),
            (0, 0),
            "a said refusal asks neither the build nor the clock"
        );
    }

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
