//! **What the application delegate said, parked until the loop's own turn**
//! (ticket M3-1).
//!
//! [`launch_wire`](crate::launch_wire)'s shape over a different channel, and the
//! reason it has one is stricter here than there. `bt_platform::AppDelegate`
//! calls the sender it was given **on AppKit's own stack**, inside the delegate
//! method — for a termination, with AppKit spinning a nested run loop on the
//! answer. Anything that turns the event loop from there re-enters winit's
//! handler while its borrow is live, which probe X-4 measured as a live process
//! at 100% CPU, one panic per turn, forever. So the sender does two things and
//! no more: push, and post one wake.
//!
//! The drain is [`take`], spent by `FolioApp::settle_app_delegate_events` on the
//! turn that follows — the loop's own door, where there is an `ActiveEventLoop`
//! and every window at once, which is what opening a window and answering a quit
//! both need and what a `Runtime` has never held.
//!
//! **No bound and no drop-oldest**, which is where this differs from
//! `attention_wire`. That channel is spoken into by strangers and has to survive
//! a flood; this one is spoken into by AppKit, once per gesture a person made,
//! and every one of the four is load-bearing — a dropped termination request is
//! an application that never answers a `⌘Q` from the Dock.

use std::sync::{Mutex, PoisonError};

use bt_platform::AppDelegateEvent;

/// What the delegate has said and nobody has read yet.
static INBOX: Mutex<Vec<AppDelegateEvent>> = Mutex::new(Vec::new());

/// Keep one event. Called on AppKit's stack; it must not do anything else.
pub(crate) fn park(event: AppDelegateEvent) {
    INBOX
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(event);
}

/// Everything parked, in arrival order, and the inbox left empty.
#[must_use]
pub(crate) fn take() -> Vec<AppDelegateEvent> {
    std::mem::take(&mut *INBOX.lock().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_platform::{AppDelegateEventKind, AppDelegateOrigin};

    /// RED — **the inbox hands everything over once, in order.**
    ///
    /// `LaunchAsked`'s own contract at a second channel: one wake stands for any
    /// number of arrivals, so the drain has to be the thing that is exhaustive.
    ///
    /// MUTATION: drain with `pop` and the order fails; clone instead of taking
    /// and the second `take` returns the same events, which is one tab per path
    /// becoming one tab per path per turn.
    #[test]
    fn the_inbox_is_drained_whole_and_in_order() {
        park(AppDelegateEvent {
            origin: AppDelegateOrigin::Reopen,
            kind: AppDelegateEventKind::Reopen {
                had_visible_windows: false,
            },
        });
        park(AppDelegateEvent {
            origin: AppDelegateOrigin::LastWindowClosed,
            kind: AppDelegateEventKind::LastWindowClosed,
        });
        let taken = take();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].origin, AppDelegateOrigin::Reopen);
        assert_eq!(taken[1].origin, AppDelegateOrigin::LastWindowClosed);
        assert!(take().is_empty(), "a drained inbox is empty");
    }
}
