// The widened timing scan's fixture (T-TIMING-BOUND-STRAYS): clock arithmetic in a test's own
// body, a budget measured in a test-only helper, and a product function handed `now` that reads
// the clock again — each beside the shapes that are not one, and the shapes the readings refuse.
// Read by bt-source's `timing` test; never compiled.

use std::time::{Duration, Instant};

/// Clock arithmetic: a poll whose deadline is one reading and whose question is another.
#[test]
fn a_poll_on_a_second_reading() {
    let give_up = Instant::now() + Duration::from_secs(30);
    while Instant::now() < give_up {
        work();
    }
}

/// Not clock arithmetic: one reading and its offsets, compared with each other.
#[test]
fn one_origin_and_its_offsets() {
    let now = Instant::now();
    let later = now + Duration::from_millis(5);
    assert!(later > now);
    assert!(now - Duration::from_millis(1) < now);
}

/// Clock arithmetic: a measurement written as a subtraction of two readings.
#[test]
fn a_measurement_by_subtraction() {
    let start = Instant::now();
    work();
    let end = Instant::now();
    assert!(end - start < Duration::from_millis(16));
}

/// Clock arithmetic inside an assertion's arguments.
#[test]
fn a_reading_inside_an_assertion() {
    let begun = Instant::now();
    assert!(
        Instant::now().duration_since(begun) < Duration::from_secs(1),
        "late"
    );
}

/// Refused: a clock value inside macro arguments that do not parse as expressions.
#[test]
fn a_clock_in_a_macro_this_reading_cannot_parse() {
    let begun = Instant::now();
    late_by!(begun => Instant::now());
}

/// A product function handed the turn's instant that reads the clock again: a finding.
pub fn handed_and_read_again(now: Instant) -> Duration {
    Instant::now() - now
}

/// A product function handed `now` whose only other reading is the origin of a measurement:
/// admitted.
pub fn handed_and_measured(now: Instant) -> u128 {
    let began = Instant::now();
    let _ = now;
    began.elapsed().as_micros()
}

/// A product function not handed an instant: nothing to say.
pub fn not_handed() -> Instant {
    Instant::now()
}

/// Refused: a closure handed a `now` whose type is not written reads the clock.
pub fn a_closure_handed_an_untyped_now() {
    let read = |now| {
        let _ = now;
        Instant::now()
    };
    let _ = read(1);
}

/// Refused: a product type carrying a `now`, which a function handed it could read again
/// without the reading seeing a parameter named `now`.
pub struct Carries {
    now: Instant,
}

fn work() {}

pub trait Cost {
    fn cost(&self) -> Duration;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only helper that measures, and hands the measurement back.
    fn costs() -> Duration {
        let started = Instant::now();
        work();
        started.elapsed()
    }

    /// A test-only helper that hands its measurement on through another.
    fn through() -> Duration {
        costs()
    }

    /// A test-only helper whose readings only ever answer "has it been too long": a wait, not a
    /// measurement.
    fn stalls() {
        let quiet = Instant::now();
        loop {
            if quiet.elapsed() > Duration::from_secs(1) {
                panic!("silent for {:?}", quiet.elapsed());
            }
        }
    }

    struct Measured;

    impl Cost for Measured {
        fn cost(&self) -> Duration {
            let started = Instant::now();
            started.elapsed()
        }
    }

    #[test]
    fn a_budget_measured_in_a_helper() {
        assert!(costs() < Duration::from_millis(16));
    }

    #[test]
    fn a_budget_measured_two_helpers_down() {
        let _ = through();
    }

    #[test]
    fn a_stall_timer_in_a_helper() {
        stalls();
    }

    /// Refused: a call on a receiver the call graph cannot type, whose candidates measure.
    #[test]
    fn a_helper_reached_through_an_untyped_receiver() {
        let ask = |measured| measured.cost();
        let _ = ask;
    }

    /// A helper that measures, private to a module the tests are not in.
    mod elsewhere {
        use super::*;

        pub struct Timer;

        impl Timer {
            fn lap(&self) -> Duration {
                let started = Instant::now();
                started.elapsed()
            }
        }
    }

    /// Not refused: the only `lap` that measures is private to `elsewhere`, so an untyped
    /// receiver here cannot be calling it.
    #[test]
    fn a_private_helper_elsewhere_is_not_a_candidate() {
        let ask = |timer| timer.lap();
        let _ = ask;
    }

    /// A helper whose remaining patience is handed to a real wait: a wait, not a measurement.
    fn waits_out(patience: Duration) {
        let since = Instant::now();
        let left = patience.saturating_sub(since.elapsed());
        std::thread::park_timeout(left);
    }

    #[test]
    fn a_remaining_patience_handed_to_a_wait() {
        waits_out(Duration::from_secs(1));
    }
}

/// A product clock that is a function, not an instant: not a type carrying `now`.
pub struct Clocked<'a> {
    now: &'a dyn Fn() -> Instant,
}
