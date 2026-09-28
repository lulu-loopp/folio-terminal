//! **The system ending the session, heard on the window procedure's stack and settled on the
//! loop's turn** (B-ENDSESSION, 0.4.6; `docs/M2-persistence-schema-v1.md` §5.5).
//!
//! A shutdown, a restart or a sign-out is a quit that nobody asked Folio for. On Windows it
//! arrives as `WM_QUERYENDSESSION`, *sent* to every window, and `bt_platform::session_end`'s
//! subclass answers it yes and calls [`hear`] on the way. From that instant the session document
//! is **held**: [`holds_the_document`] is what `App::record_session` asks beside
//! `quit::Quit::document_is_frozen`, so nothing that happens next — above all a shell the system
//! ends, which the ordinary "this shell has exited" road would close as a pane, and the autosave
//! would then write — is a change to the layout this run leaves behind.
//!
//! **Held here and not on the quit**, because the quit is `App`'s and `App` is reachable only on
//! the loop's own turn: the window procedure runs inside the system's `SendMessage`, with the
//! loop anywhere, and may only park news and wake it ([`app_delegate_wire`](crate::app_delegate_wire)'s
//! rule for AppKit's stack, at a second door). What cannot wait for that turn is the hold itself,
//! so the hold is the one thing [`hear`] does besides parking.
//!
//! The turn that follows ([`settle`]) writes the held document through the quit's own road — the
//! one writer, the one bounded wait (`doors::SessionWriteWait`, `SESSION_SAVE_BUDGET`) — and drops
//! `session.lock` once it has landed. A shutdown **taken back** (`WM_ENDSESSION` with `FALSE`:
//! another program, or the person at the shutdown screen, stopped it) lets the document go again
//! and puts the sentinel back: the run goes on.
//!
//! Window-thread state, so a thread-local: the procedure and the loop are the same thread, and a
//! test on a thread of its own starts with nothing held.

use std::cell::{Cell, RefCell};

use bt_platform::session_end::SessionEnd;

use crate::persist::{SaveRefusal, SessionStore};

thread_local! {
    /// Whether the system has asked the session to end and not taken it back.
    static HELD: Cell<bool> = const { Cell::new(false) };
    /// What the system has said and the loop has not read yet, in arrival order.
    static INBOX: RefCell<Vec<SessionEnd>> = const { RefCell::new(Vec::new()) };
}

/// **What the system said, on the window procedure's stack.** The question holds the document
/// at once; everything is parked for [`take`]. It must do nothing else.
pub(crate) fn hear(end: SessionEnd) {
    if end == SessionEnd::Asked {
        HELD.with(|held| held.set(true));
    }
    INBOX.with_borrow_mut(|inbox| inbox.push(end));
}

/// **Whether the system's end holds the session document**, so that no change is recorded.
pub(crate) fn holds_the_document() -> bool {
    HELD.with(Cell::get)
}

/// Everything parked, in arrival order, and the inbox left empty.
#[must_use]
pub(crate) fn take() -> Vec<SessionEnd> {
    INBOX.with_borrow_mut(std::mem::take)
}

/// What the loop's turn did about one piece of news.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Settled {
    /// The held document went to the writer; what became of it. `Ok` is also `session.lock`
    /// gone.
    Saved(Result<(), SaveRefusal>),
    /// A quit had already fixed the document and is writing it: its write is the last word.
    LeftToTheQuit,
    /// The session goes on: the document is let go and the sentinel is back.
    TakenBack,
}

/// **Settle one piece of news, on the loop's turn.**
///
/// `a_quit_holds_the_document` is `quit::Quit::document_is_frozen` for the quit under way: a quit
/// past its photograph is already on the way out and writing the same document through the same
/// writer, and its phases own the window thread's admission from there on.
///
/// For the question, the quit's write step: the way out's phase for the one admitted wait, the
/// write, and back to the running phase — the session may yet be taken back, and until the
/// system ends the process the windows are still Folio's.
pub(crate) fn settle(
    end: SessionEnd,
    store: &mut SessionStore,
    a_quit_holds_the_document: bool,
) -> Settled {
    match end {
        SessionEnd::Asked if a_quit_holds_the_document => Settled::LeftToTheQuit,
        SessionEnd::Asked => {
            bt_platform::admission::exiting();
            let landed = store.save_for_the_systems_end();
            bt_platform::admission::quit_abandoned();
            Settled::Saved(landed)
        }
        SessionEnd::TakenBack => {
            HELD.with(|held| held.set(false));
            store.rearm_after_the_systems_end();
            Settled::TakenBack
        }
    }
}

/// **Nothing held and nothing parked**, for a test that heard the system: `--test-threads=1` runs
/// every test on one thread, and a hold left behind would stop the next test's recordings.
#[cfg(test)]
pub(crate) fn forget() {
    HELD.with(|held| held.set(false));
    INBOX.with_borrow_mut(Vec::clear);
}
