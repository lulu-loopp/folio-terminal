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
//! **The question is settled once per span** (T-ENDSESSION-SAVES): the system sends it to every
//! top-level window, so the inbox holds one `Asked` per window, and only the first of a span — the
//! one [`owes_the_write`] answers for — hands the edits over and writes the document; the rest are
//! [`Settled::AlreadySaved`]. A shutdown taken back opens the next span.
//!
//! **The Restart Manager's close is the question and then a leaving** (T-RESTART-REGISTRATION):
//! it asks as a shutdown asks, and the question is settled the same way; then its
//! `WM_ENDSESSION` says [`Settled::Closed`], and since the session goes on and nobody ends the
//! process, the loop leaves by itself.
//!
//! **And the last word waits for the write** (plan row 201). After `WM_ENDSESSION(TRUE)` returns
//! the system may end the process, so its procedure calls [`at_the_end`] and answers only after
//! it. The write is not in flight then: the question's turn blocks this thread until it lands
//! or its budget runs out, and a message from another process is delivered only while a thread
//! waits for messages. What can be missing is the turn itself — the end overtook the wake the
//! question posted, and the turn never ran — and the lines `diagnostics.log` was owed, still in
//! its writer's queue. So the end runs the messages already queued until the question's turn has
//! run ([`hold_at_the_end`]; nothing is waited for there: the turn's own write waits inside its
//! own budget), says which road it took, and fences the log (`doors::DiagnosticsSettle`, its own
//! bound). On the VM reboots of 2026-11-03 Folio was ended about half a second after the question
//! and neither outcome line was in the log.
//!
//! Window-thread state, so a thread-local: the procedure and the loop are the same thread, and a
//! test on a thread of its own starts with nothing held.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use bt_platform::admission::doors;
use bt_platform::session_end::SessionEnd;

use crate::persist::{SaveRefusal, SessionStore};

thread_local! {
    /// Whether the system has asked the session to end and not taken it back.
    static HELD: Cell<bool> = const { Cell::new(false) };
    /// Whether this span's question has been settled — its edits handed over and its document
    /// written — so that the same question, sent to the next window, does nothing again.
    static SETTLED: Cell<bool> = const { Cell::new(false) };
    /// What the system has said and the loop has not read yet, in arrival order.
    static INBOX: RefCell<Vec<SessionEnd>> = const { RefCell::new(Vec::new()) };
    /// **Whether this span's question has had its turn** (plan row 201): its write has an answer
    /// — landed, refused or out of budget — or a quit under way writes it. Set when [`settle`]
    /// returns from the question, which is after the write's wait; [`SETTLED`] is set before it.
    static ANSWERED: Cell<bool> = const { Cell::new(false) };
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

/// **Whether `end` is the question this span still owes its write** (T-ENDSESSION-SAVES): the
/// first `Asked` of a span that no quit holds. The caller hands every edit over to the storage
/// worker before [`settle`] writes the document behind them, and keeps what did not land after it.
#[must_use]
pub(crate) fn owes_the_write(end: SessionEnd, a_quit_holds_the_document: bool) -> bool {
    end == SessionEnd::Asked && !a_quit_holds_the_document && !SETTLED.with(Cell::get)
}

/// What the loop's turn did about one piece of news.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Settled {
    /// The held document went to the writer; what became of it. `Ok` is also `session.lock`
    /// gone.
    Saved(Result<(), SaveRefusal>),
    /// This span's question was settled already, when the system asked another window.
    AlreadySaved,
    /// A quit had already fixed the document and is writing it: its write is the last word.
    LeftToTheQuit,
    /// The session goes on: the document is let go and the sentinel is back.
    TakenBack,
    /// **The Restart Manager closed the application** (`ENDSESSION_CLOSEAPP`,
    /// T-RESTART-REGISTRATION): the session goes on and nobody ends this process, so it leaves
    /// by itself, with the document its question wrote.
    Closed,
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
///
/// `asked_at` is the instant the question's turn began: the write's one budget counts from it, so
/// the hand-overs ahead of the document spend it rather than add to it (T-ENDSESSION-SAVES).
pub(crate) fn settle(
    end: SessionEnd,
    store: &mut SessionStore,
    a_quit_holds_the_document: bool,
    asked_at: Instant,
) -> Settled {
    match end {
        SessionEnd::Asked if a_quit_holds_the_document => {
            ANSWERED.with(|answered| answered.set(true));
            Settled::LeftToTheQuit
        }
        SessionEnd::Asked if SETTLED.with(Cell::get) => Settled::AlreadySaved,
        SessionEnd::Asked => {
            SETTLED.with(|settled| settled.set(true));
            bt_platform::admission::exiting();
            let landed = store.save_for_the_systems_end(asked_at);
            bt_platform::admission::quit_abandoned();
            ANSWERED.with(|answered| answered.set(true));
            Settled::Saved(landed)
        }
        SessionEnd::Closed => Settled::Closed,
        SessionEnd::TakenBack => {
            HELD.with(|held| held.set(false));
            SETTLED.with(|settled| settled.set(false));
            ANSWERED.with(|answered| answered.set(false));
            store.rearm_after_the_systems_end();
            store.wait_again_after_the_systems_end();
            Settled::TakenBack
        }
    }
}

/// **The one line `diagnostics.log` is owed about what the system's end did with the layout**
/// (plan row 191), or `None` for news that wrote nothing: the question's write — landed, with what
/// it holds, or refused, with why — or the quit under way that writes it instead.
///
/// The counts are of `document`, the one the write was handed: the windows, their tabs and how
/// many of those are pinned, which is what a next start's restore is read against. Before this
/// line a landed save said nothing, and a diagnosis had to read the save from silence.
#[must_use]
pub(crate) fn outcome_line(settled: &Settled, document: &bt_persist::SessionV1) -> Option<String> {
    let app = crate::APP_NAME;
    match settled {
        Settled::Saved(Ok(())) => {
            let tabs = document.windows.iter().flat_map(|window| &window.tabs);
            let (count, pinned) = tabs.fold((0, 0), |(count, pinned), tab| {
                (count + 1, pinned + usize::from(tab.pinned))
            });
            Some(format!(
                "{app}: the session is ending and its layout was saved: windows {}, tabs \
                 {count}, pinned {pinned}",
                document.windows.len()
            ))
        }
        Settled::Saved(Err(refusal)) => Some(format!(
            "{app}: the session is ending and its layout was not saved: {}",
            refusal.message()
        )),
        Settled::LeftToTheQuit => Some(format!(
            "{app}: the session is ending and the quit under way writes its layout"
        )),
        Settled::AlreadySaved | Settled::TakenBack | Settled::Closed => None,
    }
}

/// **Whether this span's question has had its turn** (plan row 201): what the end holds its
/// answer for.
#[must_use]
pub(crate) fn the_question_has_its_answer() -> bool {
    ANSWERED.with(Cell::get)
}

/// **The road the end took to the question's answer** (plan row 201), said in one line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Road {
    /// The question's turn had already run: its write had its answer before the end arrived.
    AlreadyRan,
    /// The end overtook the question's wake; running the queued messages ran the turn.
    RanItHere,
    /// The end overtook the question's wake, and the messages queued did not run the turn — the
    /// loop was inside its own handler when the question came (which holds the wake for after),
    /// or no question came.
    CouldNotRun,
}

/// **The one line `diagnostics.log` is owed about the road the end took** (plan row 201).
#[must_use]
pub(crate) fn road_line(road: Road) -> &'static str {
    match road {
        Road::AlreadyRan => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had already run"
        }
        Road::RanItHere => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had not run; the messages \
             already queued ran it"
        }
        Road::CouldNotRun => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's turn had not run, and the messages \
             already queued did not run it"
        }
    }
}

/// **Hold the end until the question has had its turn** (plan row 201): at once when it has;
/// otherwise `drain` runs the messages already queued on this thread — the question's wake among
/// them — until the answer is there (it says whether it is). Nothing here waits: the bound of a
/// write the turn runs is that write's own budget, and a budget already spent is not spent again.
pub(crate) fn hold_at_the_end(drain: &mut dyn FnMut(&dyn Fn() -> bool) -> bool) -> Road {
    if the_question_has_its_answer() {
        return Road::AlreadyRan;
    }
    if drain(&the_question_has_its_answer) {
        Road::RanItHere
    } else {
        Road::CouldNotRun
    }
}

/// **The end's whole rule** (plan row 201): [`hold_at_the_end`], the road said, and then the
/// fence, so that every line said before it — the question's outcome among them — is on the
/// disk before the answer lets the system end the process.
pub(crate) fn end_with(
    drain: &mut dyn FnMut(&dyn Fn() -> bool) -> bool,
    say: &mut dyn FnMut(&str),
    fence: &mut dyn FnMut(),
) -> Road {
    let road = hold_at_the_end(drain);
    say(road_line(road));
    fence();
    road
}

/// **`WM_ENDSESSION(TRUE)`, on the window procedure's stack** (plan row 201): [`end_with`] over
/// this thread's queue, `diagnostics.log` and its fence.
pub(crate) fn at_the_end() {
    let _ = end_with(
        &mut |answered| {
            bt_platform::session_end::drain_what_is_queued(answered)
                == bt_platform::session_end::Drained::Answered
        },
        &mut crate::diagnostics::note,
        &mut fence_the_log,
    );
}

/// **Everything said so far, on the disk**: the process is on its way out — the system may end it
/// once the end is answered — and `diagnostics.log`'s fence is admitted there, within its own
/// bound (`SETTLE_WITHIN`; ruled 2026-11-03, plan row 201: after the write's budget, at most four
/// seconds in all).
fn fence_the_log() {
    bt_platform::admission::exiting();
    let _ = bt_platform::admission::admitted::<doors::DiagnosticsSettle, _>(
        crate::diagnostics::settle,
    );
}

/// **Nothing held and nothing parked**, for a test that heard the system: `--test-threads=1` runs
/// every test on one thread, and a hold left behind would stop the next test's recordings.
#[cfg(test)]
pub(crate) fn forget() {
    HELD.with(|held| held.set(false));
    SETTLED.with(|settled| settled.set(false));
    ANSWERED.with(|answered| answered.set(false));
    INBOX.with_borrow_mut(Vec::clear);
}
