//! The second package of the handed-clock fixture (plan row 205): a clock trait and a carrier
//! that `timing_clocks` names across the package line. No target compiles it.

use std::time::Instant;

/// A clock trait in another package.
pub trait Ticker {
    fn tick(&self) -> Instant;
}

/// A carrier in another package.
pub struct Carried {
    clock: fn() -> Instant,
}
