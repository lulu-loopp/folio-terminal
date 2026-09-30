// The timing-bound scan's fixture: a controlled clock whose method is named
// like a wait, and a test that really sleeps. Read by bt-source's `timing`
// test; never compiled.

use std::time::{Duration, Instant};

struct CardLoop {
    now: Instant,
}

impl CardLoop {
    /// Advances the model clock; nothing waits.
    fn sleep_until(&mut self, until: Instant) {
        self.now = until;
    }
}

#[test]
fn a_test_on_a_controlled_clock() {
    let start = Instant::now();
    let mut card = CardLoop { now: start };
    card.sleep_until(start + Duration::from_secs(3));
    assert_eq!(card.now, start + Duration::from_secs(3));
}

#[test]
fn a_test_that_really_sleeps() {
    std::thread::sleep(Duration::from_millis(1));
}
