// The timing scan's waiting-helper fixture (T-TIMING-GUARD-FOLLOWS-WAITING-HELPERS, plan row 208): a
// test that waits or measures through a test-only helper, one helper down or two, beside a door that
// stops the walk, a product wait that ends it, and the shapes the walk refuses. Read by bt-source's
// `timing` test; never compiled.

use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::Win32::System::Threading::WaitForSingleObject;

/// H10: a product function that waits. A test that calls it is not found: the walk ends at a
/// product function.
pub fn product_wait(messages: &mpsc::Receiver<()>) {
    let _ = messages.recv_timeout(Duration::from_secs(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// H1: a std wait in a helper.
    fn receive_within(messages: &mpsc::Receiver<()>) {
        messages
            .recv_timeout(Duration::from_secs(5))
            .expect("a message");
    }

    #[test]
    fn a_wait_in_a_helper() {
        let (_tx, rx) = mpsc::channel();
        receive_within(&rx);
    }

    /// H2: a helper that waits nothing itself and hands on to one that sleeps.
    fn relays_the_wait() {
        pauses();
    }

    fn pauses() {
        std::thread::sleep(Duration::from_millis(5));
    }

    #[test]
    fn a_wait_two_helpers_down() {
        relays_the_wait();
    }

    /// H3: a kernel wait in a method.
    struct Child(isize);

    impl Child {
        fn gone(&self) {
            unsafe { WaitForSingleObject(self.0, 10_000) };
        }
    }

    #[test]
    fn a_kernel_wait_in_a_helper() {
        let child: Child = Child(0);
        child.gone();
    }

    /// H4: a `libc` wait by path, in a closure inside a helper.
    fn polls() {
        let look = || unsafe { libc::poll(std::ptr::null_mut(), 0, 100) };
        let _ = look;
    }

    #[test]
    fn a_libc_wait_in_a_helpers_closure() {
        polls();
    }

    /// H5: a stall timer — a reading that only answers "too long?".
    fn quiet_for_a_second(since: Instant) -> bool {
        since.elapsed() > Duration::from_secs(1)
    }

    #[test]
    fn a_stall_timer_in_a_helper() {
        let _ = quiet_for_a_second(Instant::now());
    }

    /// H5′: an `elapsed` only printed, once handed and once captured — no clock: not found
    /// (row 20 review N2).
    fn shows_how_long(since: Instant) {
        eprintln!("took {:?}", since.elapsed());
        let took = since.elapsed();
        eprintln!("took {took:?} in all");
    }

    #[test]
    fn a_printed_elapsed_in_a_helper_is_no_clock() {
        shows_how_long(Instant::now());
    }

    /// H6: a drop that waits, and no door: refused, because no edge leads into it.
    struct Ends;

    impl Drop for Ends {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_drop_that_waits() {
        let _ends = Ends;
    }

    /// H6′: a drop that waits through a helper, and no door: refused as H6 is (row 20 review
    /// N1).
    struct Settles;

    impl Drop for Settles {
        fn drop(&mut self) {
            settle_down();
        }
    }

    fn settle_down() {
        std::thread::sleep(Duration::from_millis(1));
    }

    #[test]
    fn a_drop_that_waits_through_a_helper() {
        let _settles = Settles;
    }

    /// H7: a door. The list names it, so the walk stops there; neither test is found, and the
    /// door is.
    fn door_wait(messages: &mpsc::Receiver<()>) {
        let _ = messages.recv_timeout(Duration::from_secs(10));
    }

    fn calls_the_door(messages: &mpsc::Receiver<()>) {
        door_wait(messages);
    }

    #[test]
    fn a_wait_through_a_door() {
        let (_tx, rx) = mpsc::channel();
        door_wait(&rx);
    }

    #[test]
    fn a_wait_through_a_helper_that_calls_a_door() {
        let (_tx, rx) = mpsc::channel();
        calls_the_door(&rx);
    }

    /// H7′: a drop that waits and is a door: not refused, and found as a door.
    struct Cleans;

    impl Drop for Cleans {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_drop_behind_a_door() {
        let _cleans = Cleans;
    }

    /// H8: refused — a receiver the call graph cannot type, whose candidate waits.
    pub struct Waiter;

    impl Waiter {
        pub fn hold_on(&self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_wait_behind_an_untyped_receiver() {
        let ask = |waiter| waiter.hold_on();
        let _ = ask;
    }

    /// H8′: not refused — a receiver the call graph cannot type, whose only candidate is a
    /// door: the door is met as a candidate, so it is in use and found.
    pub struct Patient;

    impl Patient {
        pub fn bide(&self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_wait_behind_an_untyped_receiver_at_a_door() {
        let ask = |patient| patient.bide();
        let _ = ask;
    }

    /// H8″: refused — an untyped receiver whose candidates are a door and a waiter that is
    /// none: the door answers no, and the waiter is still asked (row 20 review N9).
    pub struct Calm;

    impl Calm {
        pub fn settle_in(&self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub struct Restless;

    impl Restless {
        pub fn settle_in(&self) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_door_beside_a_waiter_behind_an_untyped_receiver() {
        let ask = |either| either.settle_in();
        let _ = ask;
    }

    /// H9: not refused — the only `doze` that waits is private to a module the tests are not in.
    mod elsewhere {
        use super::*;

        pub struct Sleeper;

        impl Sleeper {
            fn doze(&self) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    #[test]
    fn a_private_wait_elsewhere_is_not_a_candidate() {
        let ask = |sleeper| sleeper.doze();
        let _ = ask;
    }

    #[test]
    fn a_product_wait_ends_the_walk() {
        let (_tx, rx) = mpsc::channel();
        product_wait(&rx);
    }

    /// H11: refused — a macro whose definition here waits.
    macro_rules! settle {
        () => {
            std::thread::sleep(Duration::from_millis(1))
        };
    }

    #[test]
    fn a_wait_in_a_macro() {
        settle!();
    }

    /// H12: a zero bound only looks, and still counts.
    fn looks(child: isize) -> u32 {
        unsafe { WaitForSingleObject(child, 0) }
    }

    #[test]
    fn a_zero_bound_wait_in_a_helper() {
        let _ = looks(0);
    }

    /// A budget measured in a helper: a measuring helper, as before.
    fn costs() -> Duration {
        let started = Instant::now();
        started.elapsed()
    }

    #[test]
    fn a_budget_in_a_helper_is_still_a_measurement() {
        let _ = costs();
    }

    /// Control: a helper that neither waits nor measures, which the fixture's list also names
    /// (a stale door).
    fn quiet() {}

    #[test]
    fn a_helper_that_does_not_wait() {
        quiet();
    }
}
