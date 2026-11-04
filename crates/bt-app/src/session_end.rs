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
//! **And the last word fences the log** (plan row 201). After `WM_ENDSESSION(TRUE)` returns the
//! system may end the process, and `fn main`'s fence never runs for a process the system ends: on
//! the VM reboots of 2026-11-03 Folio was ended about half a second after the question and the
//! lines `diagnostics.log` was owed were still in its writer's queue. So that procedure calls
//! [`at_the_end`] before it answers. The end says a probe line (whether one of Folio's winit
//! callbacks is on the stack, and where the question's wake was), says one line on what became
//! of the question's save, and fences the log (`doors::DiagnosticsSettle`, its own bound). It does
//! that once for the process, because the system sends the end to every top-level window. The
//! question says its own probe line once per span, with the queue as the question found it, and
//! a question that finds no span open opens one on the procedure's stack, so that the span's
//! probe and fence start afresh without waiting for a loop turn. The end drives nothing of the loop's: a save
//! the end found not landed stays not landed, and a change made in the last second before the
//! question stays with the debounced save. Whether the question's turn can be run at the question
//! is plan row 206's, decided on what these probe lines measure.
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
    /// **What this span's question's save came to, once its turn has settled it** (plan row 201).
    /// Set when [`settle`] returns from the question, which is after the write's wait; [`SETTLED`]
    /// is set before it.
    static ANSWER: Cell<Option<Answer>> = const { Cell::new(None) };
    /// **Where this span's question's wake is** (plan row 201's probe).
    static WAKE: Cell<Wake> = const { Cell::new(Wake::NeverPosted) };
    /// **Whether a span is open**: a question heard and not yet taken back, on the window
    /// procedure's stack (plan row 201, re-review RR-N4). A question that finds none open opens
    /// one and starts its probe and fence afresh itself, without waiting for a loop turn to settle
    /// the shutdown taken back before it.
    static SPAN_OPEN: Cell<bool> = const { Cell::new(false) };
    /// **Whether this span's end has fenced the log** (plan row 201, review F2). The system sends
    /// `WM_ENDSESSION(TRUE)` to every top-level window, and every window is this thread's, so this
    /// thread's cell is the process's. The first window's end fences, and the rest find it fenced,
    /// say so and return: one second for the process, however many windows there are.
    static FENCED: Cell<bool> = const { Cell::new(false) };
    /// **How many of `FolioApp`'s winit callbacks are on this thread's stack, and the innermost**
    /// (plan row 201's probe; [`in_a_callback`]).
    static CALLBACKS: Cell<(u32, Option<&'static str>)> = const { Cell::new((0, None)) };
}

/// **What the question's save came to** (plan row 201).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Answer {
    /// Its write landed: the layout is on the disk and `session.lock` is gone.
    Landed,
    /// Its write was refused or ran out of its budget.
    NotLanded,
    /// A quit past its photograph writes the layout, and the end does not drive that write.
    LeftToTheQuit,
}

/// **Where the question's wake is** (plan row 201's probe): what can be read without a wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Wake {
    /// No question this span, so no wake.
    NeverPosted,
    /// The question's post was refused (the loop is gone).
    Refused,
    /// Posted, and its turn has not begun: still on the queue, or taken by winit and held while
    /// its handler is in use.
    Posted,
    /// Its turn began: the loop took the news it brought.
    TurnBegan,
}

/// **What the system said, on the window procedure's stack.** The question holds the document
/// at once; everything is parked for [`take`]. It must do nothing else.
pub(crate) fn hear(end: SessionEnd) {
    match end {
        SessionEnd::Asked => {
            if !SPAN_OPEN.with(Cell::get) {
                SPAN_OPEN.with(|open| open.set(true));
                FENCED.with(|fenced| fenced.set(false));
                WAKE.with(|wake| wake.set(Wake::NeverPosted));
                ANSWER.with(|answer| answer.set(None));
            }
            HELD.with(|held| held.set(true));
        }
        SessionEnd::TakenBack => SPAN_OPEN.with(|open| open.set(false)),
        SessionEnd::Closed => {}
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
    // The turn the wake brings begins here (plan row 201's probe).
    if WAKE.with(Cell::get) == Wake::Posted {
        WAKE.with(|wake| wake.set(Wake::TurnBegan));
    }
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
            ANSWER.with(|answer| answer.set(Some(Answer::LeftToTheQuit)));
            Settled::LeftToTheQuit
        }
        SessionEnd::Asked if SETTLED.with(Cell::get) => Settled::AlreadySaved,
        SessionEnd::Asked => {
            SETTLED.with(|settled| settled.set(true));
            bt_platform::admission::exiting();
            let landed = store.save_for_the_systems_end(asked_at);
            bt_platform::admission::quit_abandoned();
            let answer = if landed.is_ok() {
                Answer::Landed
            } else {
                Answer::NotLanded
            };
            ANSWER.with(|cell| cell.set(Some(answer)));
            Settled::Saved(landed)
        }
        SessionEnd::Closed => Settled::Closed,
        SessionEnd::TakenBack => {
            SETTLED.with(|settled| settled.set(false));
            // A question heard since (re-review RR-N4) has opened the next span: its hold, its
            // wake, its answer and its fence are that span's, and stay.
            if !SPAN_OPEN.with(Cell::get) {
                HELD.with(|held| held.set(false));
                ANSWER.with(|answer| answer.set(None));
                WAKE.with(|wake| wake.set(Wake::NeverPosted));
                FENCED.with(|fenced| fenced.set(false));
            }
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

/// **What the end found of the question's save** (plan row 201), said in one line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Road {
    /// The question's write landed before the end.
    Landed,
    /// The question was asked and no turn has written its save: no write was made at the
    /// question.
    TurnNotRun,
    /// The question's turn ran, and its write was refused or ran out of its budget.
    NotLanded,
    /// The question's turn found a quit past its photograph, which writes the layout; the end
    /// does not drive that write (review N3).
    LeftToTheQuit,
    /// No question came before the end.
    NeverAsked,
    /// An earlier window's end fenced the log (review F2): nothing more here.
    FencedBefore,
}

/// **The one line `diagnostics.log` is owed about the question's save at the end** (plan row 201).
#[must_use]
pub(crate) fn road_line(road: Road) -> &'static str {
    match road {
        Road::Landed => "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save had landed",
        Road::TurnNotRun => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save had not landed: its turn had \
             not written it"
        }
        Road::NotLanded => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save had not landed: its write was \
             refused or ran out of its budget"
        }
        Road::LeftToTheQuit => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save was left to the quit under way"
        }
        Road::NeverAsked => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): the question's save was never asked"
        }
        Road::FencedBefore => {
            "BT_SESSION_END WM_ENDSESSION(TRUE): an earlier window's end fenced the log"
        }
    }
}

/// **What the end finds of the question's save** (plan row 201), read from this span's state.
#[must_use]
pub(crate) fn the_road() -> Road {
    match ANSWER.with(Cell::get) {
        Some(Answer::Landed) => Road::Landed,
        Some(Answer::NotLanded) => Road::NotLanded,
        Some(Answer::LeftToTheQuit) => Road::LeftToTheQuit,
        None if holds_the_document() => Road::TurnNotRun,
        None => Road::NeverAsked,
    }
}

/// **What the probe reads, at either message** (plan row 201): whether one of `FolioApp`'s winit
/// callbacks is on the stack, where the question's wake is, and whether posted messages wait on
/// the queue. Nothing in it waits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Probe {
    /// How many of `FolioApp`'s winit callbacks are on the stack, and the innermost.
    pub(crate) callbacks: (u32, Option<&'static str>),
    /// Where the question's wake is.
    pub(crate) wake: Wake,
    /// Whether posted messages wait on the thread's queue; `None` where there is no such queue.
    pub(crate) queued: Option<bool>,
}

impl Probe {
    /// What the probe reads now, on this thread.
    #[must_use]
    pub(crate) fn now() -> Self {
        Self::found(bt_platform::session_end::posted_messages_waiting())
    }

    /// What the probe reads now, with the queue as it was read before (`queued`): the question's
    /// queue is read before Folio posts its own wake to it (re-review RR-F1).
    #[must_use]
    pub(crate) fn found(queued: Option<bool>) -> Self {
        Self {
            callbacks: CALLBACKS.with(Cell::get),
            wake: WAKE.with(Cell::get),
            queued,
        }
    }
}

/// **The probe's line** (plan row 201), for `message` (`WM_QUERYENDSESSION` or
/// `WM_ENDSESSION(TRUE)`).
#[must_use]
pub(crate) fn probe_line(message: &str, probe: &Probe) -> String {
    let callbacks = match probe.callbacks {
        (0, _) | (_, None) => probe.callbacks.0.to_string(),
        (count, Some(innermost)) => format!("{count} (innermost {innermost})"),
    };
    let wake = match probe.wake {
        Wake::NeverPosted => "never posted",
        Wake::Refused => "its post refused",
        Wake::Posted => "posted, its turn not begun",
        Wake::TurnBegan => "taken by its turn",
    };
    let queued = match probe.queued {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    format!(
        "BT_SESSION_END probe at {message}: Folio's winit callbacks on the stack: {callbacks}; the \
         question's wake: {wake}; posted messages queued: {queued}"
    )
}

/// **The question's wake was posted, or refused**, on the window procedure's stack (plan row 201):
/// called right after [`hear`] with whether `send_event` took it, and with `queued`, whether
/// posted messages waited on the queue when the question came. That is read before the wake is
/// posted, which would otherwise always be found there (re-review RR-F1). For the first question
/// of a span it records the wake and says the question's probe line through `say`; it does nothing
/// for any other news, or for the same question sent to the next window.
pub(crate) fn posted_with(
    end: SessionEnd,
    queued: Option<bool>,
    taken: bool,
    say: &mut dyn FnMut(&str),
) {
    if end != SessionEnd::Asked || WAKE.with(Cell::get) != Wake::NeverPosted {
        return;
    }
    WAKE.with(|wake| wake.set(if taken { Wake::Posted } else { Wake::Refused }));
    say(&probe_line("WM_QUERYENDSESSION", &Probe::found(queued)));
}

/// [`posted_with`], said to `diagnostics.log`.
pub(crate) fn posted(end: SessionEnd, queued: Option<bool>, taken: bool) {
    posted_with(end, queued, taken, &mut crate::diagnostics::note);
}

/// **The end's whole rule** (plan row 201): the probe line, the line on the question's save, and
/// then the fence, so that every line said before it — the question's outcome among them — is on
/// the disk before the answer lets the system end the process. Once for the process: a later
/// window's end finds the log fenced ([`FENCED`]), says so and holds nothing (review F2).
pub(crate) fn end_with(probe: &Probe, say: &mut dyn FnMut(&str), fence: &mut dyn FnMut()) -> Road {
    if FENCED.with(Cell::get) {
        say(road_line(Road::FencedBefore));
        return Road::FencedBefore;
    }
    let road = the_road();
    say(&probe_line("WM_ENDSESSION(TRUE)", probe));
    say(road_line(road));
    fence();
    FENCED.with(|fenced| fenced.set(true));
    road
}

/// **`WM_ENDSESSION(TRUE)`, on the window procedure's stack** (plan row 201): [`end_with`] with
/// what the probe reads now, `diagnostics.log` and its fence. Answers the road it found.
pub(crate) fn at_the_end() -> Road {
    end_with(
        &Probe::now(),
        &mut crate::diagnostics::note,
        &mut fence_the_log,
    )
}

/// **One of `FolioApp`'s winit callbacks, on the stack** (plan row 201's probe): counted from here
/// until the guard drops, with `name` the innermost while it stands.
#[must_use]
pub(crate) fn in_a_callback(name: &'static str) -> InACallback {
    let outer = CALLBACKS.with(Cell::get);
    CALLBACKS.with(|callbacks| callbacks.set((outer.0 + 1, Some(name))));
    InACallback { outer }
}

/// The guard [`in_a_callback`] answers: dropping it puts the count and the innermost back.
pub(crate) struct InACallback {
    outer: (u32, Option<&'static str>),
}

impl Drop for InACallback {
    fn drop(&mut self) {
        CALLBACKS.with(|callbacks| callbacks.set(self.outer));
    }
}

/// **Everything said so far, on the disk**: the process is on its way out — the system may end it
/// once the end is answered — and `diagnostics.log`'s fence is admitted there, within its own
/// bound (`SETTLE_WITHIN`, one second; ruled 2026-11-03, plan row 201: once for the process).
fn fence_the_log() {
    bt_platform::admission::exiting();
    let _ =
        bt_platform::admission::admitted::<doors::DiagnosticsSettle, _>(crate::diagnostics::settle);
}

/// **Nothing held and nothing parked**, for a test that heard the system: `--test-threads=1` runs
/// every test on one thread, and a hold left behind would stop the next test's recordings.
#[cfg(test)]
pub(crate) fn forget() {
    HELD.with(|held| held.set(false));
    SETTLED.with(|settled| settled.set(false));
    ANSWER.with(|answer| answer.set(None));
    WAKE.with(|wake| wake.set(Wake::NeverPosted));
    FENCED.with(|fenced| fenced.set(false));
    SPAN_OPEN.with(|open| open.set(false));
    INBOX.with_borrow_mut(Vec::clear);
}
