//! **winit's wake is the message the session end's drain takes** (plan row 201, review F1).
//!
//! `bt_platform::session_end::drain_what_is_queued` takes only the message that winit's
//! `EventLoopProxy::send_event` posts, and asks for it by the name winit registers it under
//! (`bt_platform::session_end::WINIT_WAKE_MESSAGE_NAME`). That name is winit's private one, so
//! this test builds a real event loop of the locked winit, sends one event, and finds its post
//! under that name, to a window. It runs in a process of its own because winit allows one event
//! loop per process and makes the process DPI-aware. The loop's window is the hidden one winit
//! keeps for its thread messages: nothing is shown.
#![cfg(windows)]

use bt_platform::session_end::wake_target::{Queued, take_what_is_left};
use winit::event_loop::EventLoop;
use winit::platform::windows::EventLoopBuilderExtWindows;

/// PIN (plan row 201, review F1) — **one event sent through winit's proxy is one wake on this
/// thread's queue under the name the drain takes, posted to a window**, and nothing else.
///
/// MUTATION: spell `WINIT_WAKE_MESSAGE_NAME` otherwise — the post is counted as another message,
/// and the drain would never find the question's wake.
#[test]
fn winits_wake_is_posted_under_the_name_the_drain_takes() {
    let event_loop = EventLoop::<()>::with_user_event()
        .with_any_thread(true)
        .build()
        .expect("a winit event loop on this thread");
    let before = take_what_is_left();
    event_loop
        .create_proxy()
        .send_event(())
        .expect("the loop takes the event");
    assert_eq!(
        take_what_is_left(),
        Queued {
            wakes: 1,
            ..Queued::default()
        },
        "one wake, posted to winit's window under the drain's name (queued before the event, \
         and taken off: {before:?})"
    );
}
