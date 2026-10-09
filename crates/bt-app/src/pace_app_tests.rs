//! **`pace`, as the application drives it.** Tests whose first assertion is about
//! `pace`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;
use std::time::Duration;

/// And it is never *slower* than the frame either: a tick offered exactly
/// one frame after the last is due, not one frame and a bit.
#[test]
fn a_tick_offered_exactly_one_frame_later_is_due() {
    let start = Instant::now();
    let frame = pace::DEFAULT_FRAME_INTERVAL;
    assert!(
        strip_animation_tick_is_due(None, start, frame),
        "the first ever"
    );
    assert!(strip_animation_tick_is_due(
        Some(start),
        start + frame,
        frame
    ));
    assert!(!strip_animation_tick_is_due(
        Some(start),
        start + frame - Duration::from_micros(1),
        frame
    ));
}
