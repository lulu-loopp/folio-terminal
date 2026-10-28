//! **The UI Automation events lane** (T-UIA-SKELETON, 88a; ruling R6): the focus events a
//! window's publication owes are raised here, on one below-normal thread, never on the window
//! thread — a client listening in this process runs its handler inside the raise, and a client in
//! another process is reached through a cross-process call.
//!
//! Detached: an event owes nobody an answer. The thread starts on the first event any window owes
//! (a machine where no client ever asks starts nothing); one that would not start, or that has
//! ended, is said once in `diagnostics.log`, and the windows go on answering every question a
//! client asks — only the events stop.

use std::sync::mpsc;

use bt_platform::uia::UiaEvents;

/// **The process's lane**: the events thread's channel once it has started, and whether it has
/// failed.
#[derive(Default)]
pub(crate) struct UiaEventsLane {
    sender: Option<mpsc::Sender<UiaEvents>>,
    /// The thread would not start, or it has ended: nothing is sent again.
    gone: bool,
}

impl UiaEventsLane {
    /// **Hand one window's owed events over** — the door: a channel send, and on the first event
    /// the thread's start. Waits for nothing.
    pub(crate) fn request(&mut self, events: UiaEvents) {
        if self.gone {
            return;
        }
        if self.sender.is_none() {
            let (sender, receiver) = mpsc::channel::<UiaEvents>();
            if let Err(error) = bt_platform::spawn_at_priority(
                "bt-uia-events",
                bt_platform::ThreadPriority::BelowNormal,
                move |worker| run(worker, &receiver),
            ) {
                self.gone = true;
                crate::diagnostics::note(&format!(
                    "Folio: UI Automation — the events thread did not start ({error}); \
                     focus events are not raised"
                ));
                return;
            }
            self.sender = Some(sender);
        }
        let sent = self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(events).is_ok());
        if !sent {
            self.sender = None;
            self.gone = true;
            crate::diagnostics::note(
                "Folio: UI Automation — the events thread has ended; focus events are not raised",
            );
        }
    }
}

/// The thread's body: each window's events raised in the order they were owed.
fn run(worker: &bt_platform::admission::WorkerCtx, receiver: &mpsc::Receiver<UiaEvents>) {
    while let Ok(events) = receiver.recv() {
        events.raise(worker);
    }
}
