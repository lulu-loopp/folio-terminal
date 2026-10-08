//! **`update_job`, as the application drives it.** Tests whose first assertion is about
//! `update_job`, written in the crate root's scope (`use super::*`) rather than in the
//! module's own, with their shared fixtures from [`crate::test_support`].

use super::*;

/// RED (A1d, the owner side of M5) — **a boxed closure and a function pointer that reach an
/// owner-thread door from the window thread are admitted and run.**
///
/// The passing control for A1b's worker-refusal arm: the same two indirections, invoked on a
/// thread that entered as the window thread with its loop running, reach the door. A check that
/// read the caller's shape rather than the thread's role would refuse one of them.
///
/// MUTATION: make `admitted` refuse `Role::Window` too and both answers are `Err`.
#[test]
fn an_owner_door_reached_through_a_boxed_closure_or_a_function_pointer_runs_on_the_window_thread() {
    use bt_platform::admission::{Refused, admitted, doors};
    fn through_a_pointer() -> Result<&'static str, Refused> {
        admitted::<doors::SetCursor, _>(|_token| "the pointer's door ran")
    }
    std::thread::spawn(|| {
        assert!(bt_platform::admission::enter_window_thread());
        assert!(bt_platform::admission::loop_running());
        let boxed: Box<dyn Fn() -> Result<&'static str, Refused>> =
            Box::new(|| admitted::<doors::TitleFlush, _>(|_token| "the box's door ran"));
        let pointer: fn() -> Result<&'static str, Refused> = through_a_pointer;
        assert_eq!(boxed(), Ok("the box's door ran"));
        assert_eq!(pointer(), Ok("the pointer's door ran"));
    })
    .join()
    .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
