//! **The clipboard lane** — every read and write of the clipboard runs here, never on the window
//! thread (`docs/ARCHITECTURE.md` §5.1; `docs/DESIGN.md`, 2026-10-17 — *the clipboard is asked on
//! its own lane*; registry row 26, repaid).
//!
//! Windows' clipboard is one lock for the whole session: while another program holds it open,
//! `OpenClipboard` fails, and the door asks again with 5, 10, 20 and 40 ms between five attempts —
//! up to 75 ms that used to stall the window's frame. The doors in `bt_platform` keep that one
//! declared contention wait (the side-effects contract's C5 rule 3) and take a worker's capability,
//! so this module's worker is the one place they are called from; the window hears each answer
//! through its event loop.
//!
//! **Built on the completion seam** (`crate::lane`; the side-effects contract's C1–C7 and its
//! revision (c)4; S-7): the worker is started by a [`crate::lane::Executor`] (the thread door's in
//! the product, the suite's scripted one under test), every identity is minted by a
//! [`crate::lane::Ledger`], each request runs inside [`crate::lane::task`], and every request ends
//! through the ledger's hooks. It is the seam's first lane to end a request `Cancelled`.
//!
//! # The contract
//!
//! * **Identity.** Every admitted request gets a [`ClipboardId`] minted by the lane's ledger,
//!   unique for the life of the lane (one per process). The window that asked holds the request in
//!   its [`ClipboardAsks`], which is the target incarnation: a window that closes takes its
//!   requests with it, and their terminals go to the lane's sink.
//! * **What a request is.** A paste's read — the payload a terminal paste spells
//!   ([`Ask::Payload`]), or the text a one-line field or the quick edit takes ([`Ask::Text`]) —
//!   aimed where the gesture was ([`PasteInto`]: the pane, by tab, seat and incarnation; the field;
//!   the page); or a copy's write, carrying its bytes ([`Ask::Write`]) and what the window says once
//!   it has landed (`ClipboardWriteEffect`).
//! * **Ordering.** One thread, one FIFO queue (`lane::Queue`): requests run, and end, in the
//!   order they were minted, so a copy and the paste after it read what was copied.
//! * **Start.** The worker is started by the first request; a start the thread door refuses
//!   refuses that request alone, at the door ([`Declined::NotStarted`], said once by the ledger),
//!   and the next request starts it again.
//! * **Capacity.** A window owes at most [`PASTES_OWED`] pastes (its desk: a paste past it is
//!   refused on the window, said once) and one copy (a newer copy replaces the older,
//!   [`ClipboardLane::supersede`]: the clipboard ends holding the newer either way). A paste is
//!   never replaced by a later one: the later one queues behind it, and both read the clipboard in
//!   order. The ledger owes at most [`BOUND`] identities at once — a full desk and a copy in the
//!   window the person is in, and the same in the window they have just left: a paste is owed no
//!   longer than [`PASTE_DEADLINE`], and the person is in one window at a time, so only a worker
//!   stuck inside the platform (a program that does not answer `WM_DESTROYCLIPBOARD`, a source
//!   that never renders its delayed format) lets a third window's request find the ledger full.
//!   That one is refused at the door, synchronously, with [`LANE_FULL`], and no identity is minted
//!   for it (revision (b)3); a request after the worker has ended is refused the same way with
//!   [`LANE_GONE`]. The gesture says the refusal once. A request withdrawn or replaced leaves the
//!   worker's line with its terminal (`lane::Queue::withdraw`), so the line is bounded as the
//!   ledger is.
//! * **Completion.** Every admitted request ends exactly once: `Done` with the clipboard's answer;
//!   `Failed` when the door refused — the five opens spent, no text on the clipboard, a write the
//!   system refused — or the work panicked, which its task catches (the worker goes on, and the
//!   ledger says the failure in `diagnostics.log` once); `Superseded { by }` when the window's
//!   newer copy replaced it before it was answered; `Cancelled` when the window withdrew
//!   it ([`ClipboardLane::withdraw`]): its pane closed or was restarted, its tab left the front,
//!   its field let the keyboard go, or it outlived [`PASTE_DEADLINE`] or the held-input cap; or
//!   `WorkerGone` for every request still owed when the worker's thread ends, which leaves the lane
//!   faulted — the seam's terminal, not a cancel: the window then releases a paste's held input,
//!   in order, and says the read-failed notice once.
//! * **Withdrawal** (`Cancelled`, its first adopter on the seam). The window withdraws a request by
//!   ending it `Cancelled` and letting it go, so its terminal goes to the lane's sink and never to a
//!   surface; a request withdrawn before the worker reached it leaves the line unrun
//!   (`lane::Queue::withdraw`), and one the worker has just taken is dropped before its work
//!   ([`Ledger::owes`](crate::lane::Ledger::owes)) — "the lane dropped the request before its work
//!   was done, at its requester's word". `Superseded` is the other word for the same act by the
//!   same asker with a newer request in hand: the older's terminal names it.
//! * **Abandonment.** An answer is raised only in the window whose [`ClipboardAsks`] holds its
//!   request. A closed window's requests go to the lane's sink (C6), which lets them go: a success
//!   has nobody left to tell, and a failure was said by the ledger.
//! * **Wake.** The worker publishes the terminal first and then wakes the loop
//!   (`AppEvent::ClipboardAnswered`); the ledger wakes it the same way when the worker ends, after
//!   every owed request has its terminal. The loop takes the answers without waiting and never
//!   blocks on the lane; each window's turn also takes whatever has ended, so a lost wake loses no
//!   answer.
//!
//! **Between a paste's press and its answer** the pane it was aimed at holds the person's input
//! ([`PasteHold`]: keys, input-method commits, mouse reports, another paste or a dropped path),
//! and releases it, in the order it was offered, right after the paste's bytes — or, for a paste
//! that ends with nothing to write, at once; each write is offered as the write it was. A paste
//! that waits for the multi-line card keeps the hold until the card is answered. The hold is
//! bounded by [`PASTE_DEADLINE`] and by `bt_pty::PTY_INPUT_RING_BYTES`: past either the paste is
//! withdrawn and the held input goes to the shell in its order. A program in mouse mode therefore
//! gets the clicks made while a paste is owed up to a second late, in order. A one-line field and
//! the quick edit hold nothing: a key typed into one between the press and the answer lands before
//! the pasted text.
//!
//! **One road on every platform** (T-CLIPBOARD-LANE judgement 4): the same requests, the same
//! terminals and the same hold everywhere; only the door differs. On Windows the worker opens the
//! clipboard itself; on macOS the door hands the pasteboard's calls to the main thread and waits
//! there, because the owner's ruling of 2026-10-11 (Q2) keeps `NSPasteboard` on the owner thread
//! until experiment E-2 says otherwise (`bt_platform`'s `macos_clipboard`).
//!
//! **The picture a paste reads stays on its own road** (`Runtime::save_clipboard_picture`, the
//! `clipboard-picture` worker): the bytes come off the clipboard here, and their encode to a file
//! is a different resource with its own generation, ceiling and answer.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_platform::ClipboardPayload;
use bt_platform::admission::WorkerCtx;

use crate::lane::{self, Executor, Ledger, Owed, Queue, Refused, Relation, Saying, Terminal};
use crate::{ClipboardWriteEffect, PasteTarget, PreviewSurface, TextFieldSeat};

/// **How many pastes one window may owe at once** — its desk: past it, a paste is refused on the
/// window, said once, before it reaches the lane. A paste is a deliberate press (`Ctrl+V`'s key
/// repeat is not one: the keystroke ladder answers only a first press), each owed at most
/// [`PASTE_DEADLINE`], so eight is more presses than a hand makes in that second and as many as a
/// macro or a remote session's burst is given room for before the clipboard has answered once.
pub(crate) const PASTES_OWED: usize = 8;

/// **How many identities the lane's ledger owes at once** — see the module's *Capacity*: a full
/// desk of pastes ([`PASTES_OWED`]) and one copy, in each of two windows.
pub(crate) const BOUND: usize = 2 * (PASTES_OWED + 1);

/// **How long a paste may be owed** — from its admission to its terminal answer, the side-effects
/// contract's revision (c)4: the worker's backlog, the 75 ms of contention the door waits through
/// and the source program's delayed rendering inside `GetClipboardData`, which has no bound of its
/// own. A clock read on the window's turn and never a wait: past it the paste is withdrawn and the
/// keys held behind it are released.
pub(crate) const PASTE_DEADLINE: Duration = Duration::from_secs(1);

/// The refusal a request meets when the ledger already owes [`BOUND`] requests.
pub(crate) const LANE_FULL: &str = "the clipboard lane is full";

/// The refusal a request meets when the lane's thread is gone.
pub(crate) const LANE_GONE: &str = "the clipboard lane has stopped";

/// The lane's name in the lines its ledger says.
const LANE: &str = "the clipboard lane";

/// **One clipboard request's name**, minted by the lane's ledger and unique for the life of the
/// lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct ClipboardId(u64);

impl From<u64> for ClipboardId {
    fn from(minted: u64) -> Self {
        Self(minted)
    }
}

impl ClipboardId {
    /// The number the ledger minted, for the suite's tally.
    #[cfg(test)]
    pub(crate) fn number(self) -> u64 {
        self.0
    }
}

/// **What one request asks of the clipboard.**
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ask {
    /// The text on it, for a one-line field or the quick edit (`bt_platform::clipboard_text`).
    Text,
    /// Whatever it holds, as a terminal paste reads it (`bt_platform::clipboard_payload`).
    Payload,
    /// Put this text on it (`bt_platform::set_clipboard_text`).
    Write(String),
}

/// **What the clipboard answered**, for the ask it answers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Reply {
    Text(String),
    Payload(ClipboardPayload),
    Written,
}

/// How one request ended: the clipboard's answer when it was done; the door's refusal, in its own
/// words, or a panic in the work, when it failed.
pub(crate) type ClipboardTerminal = Terminal<ClipboardId, Reply, String>;

/// **The door the worker serves a request through** — the platform's clipboard
/// ([`PlatformClipboard`]) in the product, a scripted one under test. Handed to the lane with each
/// request by the asking window's `WindowHost::clipboard`, so a headless window's requests reach
/// its own clipboard and never the machine's.
pub(crate) trait ClipboardDoor: Send + Sync {
    fn serve(&self, worker: &WorkerCtx, ask: &Ask) -> Result<Reply, String>;
}

/// **The machine's clipboard**: `bt_platform`'s three worker-only doors, one per ask.
pub(crate) struct PlatformClipboard;

impl ClipboardDoor for PlatformClipboard {
    /// The worker-only door bodies (`window_waits.tsv`'s worker-door effect row): the capability is
    /// made only by the thread door, and the contention wait is inside the entrances named here.
    fn serve(&self, worker: &WorkerCtx, ask: &Ask) -> Result<Reply, String> {
        match ask {
            Ask::Text => bt_platform::clipboard_text(worker).map(Reply::Text),
            Ask::Payload => bt_platform::clipboard_payload(worker).map(Reply::Payload),
            Ask::Write(text) => {
                bt_platform::set_clipboard_text(worker, text).map(|()| Reply::Written)
            }
        }
    }
}

/// The lane's sink for the abandoned and the withdrawn, shared by every request it admits.
type Sink = Arc<dyn Fn(ClipboardId, ClipboardTerminal) + Send + Sync>;

/// **What a request is owed when its window has gone, or has withdrawn it** (C6): the lane's sink,
/// which outlives every window.
pub(crate) struct Abandoned(Sink);

impl Owed<ClipboardId> for Abandoned {
    type Answer = Reply;
    type Error = String;

    fn sink(self, id: ClipboardId, terminal: ClipboardTerminal) {
        (self.0)(id, terminal);
    }
}

/// One admitted request, held by the window that asked: its terminal is taken once, and a window
/// that drops it leaves the terminal to the sink.
pub(crate) type ClipboardRequest = lane::Request<ClipboardId, Abandoned>;

/// What the window thread puts on the queue: what the request asks, and the door it is served
/// through. Its name is the queue's (`lane::Queue`).
struct Job {
    ask: Ask,
    door: Arc<dyn ClipboardDoor>,
}

/// **Why a request was refused at the door**, as a value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Declined {
    /// The lane's ledger already owed [`BOUND`] requests (said [`LANE_FULL`]).
    LaneFull,
    /// The lane's worker had ended (said [`LANE_GONE`]).
    LaneGone,
    /// The thread door would not start the lane's worker for this request, in its words; the lane
    /// is not in fault, and the next request starts it again (revision (b)5, rule 1).
    NotStarted(String),
}

impl Declined {
    /// The refusal's own words.
    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::LaneFull => LANE_FULL,
            Self::LaneGone => LANE_GONE,
            Self::NotStarted(words) => words,
        }
    }
}

/// **The lane's owner, held by the application** — the one first-in-first-out queue on the seam
/// (`lane::Queue`, whose ledger mints and ends every request), the executor its worker is started
/// by, the wake, and the sink a closed window's requests are owed.
pub(crate) struct ClipboardLane {
    queue: Arc<Queue<ClipboardId, Abandoned, Job>>,
    executor: Executor,
    wake: Arc<dyn Fn() + Send + Sync>,
    sink: Sink,
}

impl ClipboardLane {
    /// **The lane** — one thread, `bt-clipboard`, in the workers' band (somebody is waiting for
    /// this with a hand on the keyboard), started through the thread door by the first request;
    /// its ledger's lines said in `diagnostics.log`, and what reaches its sink let go.
    pub(crate) fn spawn(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::start(
            &Executor::thread_door(bt_platform::ThreadPriority::Normal),
            wake,
            crate::diagnostics::note,
            |_, _| {},
        )
    }

    /// The lane whose worker `executor` starts — the production one above, or the suite's
    /// scripted one — at its first request. `say` is where the ledger's lines go, and `sink` what
    /// a closed window's or a withdrawn request ends in.
    pub(crate) fn start(
        executor: &Executor,
        wake: impl Fn() + Send + Sync + 'static,
        say: impl Fn(&str) + Send + Sync + 'static,
        sink: impl Fn(ClipboardId, ClipboardTerminal) + Send + Sync + 'static,
    ) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let ledger_wake = Arc::clone(&wake);
        let ledger = Ledger::new(LANE, Some(BOUND), Saying::PerRequest, say, move || {
            ledger_wake()
        });
        Self {
            queue: Arc::new(Queue::new(ledger)),
            executor: executor.clone(),
            wake,
            sink: Arc::new(sink),
        }
    }

    /// **Put one request on the lane and return at once** with it — starting the worker if no
    /// request has yet — or refuse it at the door: [`Declined::LaneFull`] past [`BOUND`],
    /// [`Declined::LaneGone`] once the worker has ended, [`Declined::NotStarted`] when the thread
    /// door would not start it. A refused request mints no identity. `door` is what the worker
    /// serves it through.
    pub(crate) fn submit(
        &self,
        door: Arc<dyn ClipboardDoor>,
        ask: Ask,
    ) -> Result<ClipboardRequest, Declined> {
        let queue = Arc::clone(&self.queue);
        let wake = Arc::clone(&self.wake);
        self.queue
            .ask(
                Job { ask, door },
                Abandoned(Arc::clone(&self.sink)),
                // Nothing is coalesced by the lane: a window's newer request replaces its older
                // one by its own act (`Self::supersede`).
                |_waiting, _asked| Relation::Other,
                |ran| {
                    self.executor.start_worker("bt-clipboard", ran, move |ctx| {
                        run_clipboard_lane(ctx, &queue, &*wake);
                    })
                },
            )
            .map_err(|(refused, _)| match refused {
                Refused::Full { .. } => Declined::LaneFull,
                Refused::Faulted(_) => Declined::LaneGone,
                Refused::NotStarted(words) => Declined::NotStarted(words),
                Refused::TwinEnded => unreachable!("the clipboard lane joins nothing"),
            })
    }

    /// **The asker withdraws `request`** (`Cancelled`): it is ended `Cancelled` — unless its
    /// terminal came first, which then stands — and let go, so whichever terminal it has goes to the
    /// lane's sink and never to a surface. A request the worker has not reached yet is dropped by the
    /// worker unrun. Answers whether it was this call that ended it.
    pub(crate) fn withdraw(&self, request: ClipboardRequest) -> bool {
        let cancelled = self.queue.withdraw(request.id(), Terminal::Cancelled);
        drop(request);
        cancelled
    }

    /// **The asker's newer request `by` replaces `older`** (`Superseded { by }`): `older` is ended
    /// so — unless its terminal came first — and its terminal taken, here, by the asker that holds
    /// it; it reaches no surface. A request the worker has not reached yet is dropped unrun.
    pub(crate) fn supersede(
        &self,
        mut older: ClipboardRequest,
        by: ClipboardId,
    ) -> ClipboardTerminal {
        self.queue.withdraw(older.id(), Terminal::Superseded { by });
        older
            .take()
            .expect("a request still held has a terminal once it is superseded")
    }
}

#[cfg(test)]
impl ClipboardLane {
    /// How many requests wait in the worker's line (`lane::Queue::in_line`).
    pub(crate) fn in_line(&self) -> usize {
        self.queue.in_line()
    }
}

impl Drop for ClipboardLane {
    /// The lane let go: its worker runs what is in line, and returns.
    fn drop(&mut self) {
        self.queue.close();
    }
}

/// **The lane's body**: one request at a time, its door inside its task, its terminal published,
/// then the wake — `lane::Queue::serve`'s shape, with one question more: a request already ended
/// — withdrawn or replaced by its asker — is dropped here before its work, which is what its
/// `Cancelled` or `Superseded` says.
fn run_clipboard_lane(
    worker: &WorkerCtx,
    queue: &Queue<ClipboardId, Abandoned, Job>,
    wake: &dyn Fn(),
) {
    while let Some((id, Job { ask, door })) = queue.next(worker) {
        if !queue.ledger().owes(id) {
            continue;
        }
        let terminal = match lane::task(worker, |ctx| door.serve(ctx, &ask)) {
            Ok(reply) => Terminal::Done(reply),
            Err(failure) => Terminal::Failed(failure),
        };
        queue.ledger().publish(id, terminal);
        wake();
    }
}

/// **Where a paste was aimed when its gesture was made.**
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PasteInto {
    /// A terminal pane, by tab, seat and incarnation: its pty holds the person's input meanwhile.
    Terminal(PasteTarget),
    /// The one-line field that held the keyboard.
    Field(TextFieldSeat),
    /// The quick edit's page that held the keyboard.
    Preview(PreviewSurface),
}

/// **A paste a window is owed**: its identity, its request until its terminal is in hand, where
/// it goes, and when it was asked (the deadline's clock). A terminal paste's answer is spent only
/// when it reaches the front of its pane's [`PasteHold`], so an answer can be in hand and wait
/// behind an earlier paste of the same pane.
pub(crate) struct OwedPaste {
    pub(crate) id: ClipboardId,
    pub(crate) request: Option<ClipboardRequest>,
    pub(crate) ended: Option<ClipboardTerminal>,
    pub(crate) into: PasteInto,
    pub(crate) asked_at: Instant,
}

impl OwedPaste {
    /// The instant this paste is withdrawn if it is still owed ([`PASTE_DEADLINE`]).
    pub(crate) fn deadline(&self) -> Instant {
        self.asked_at + PASTE_DEADLINE
    }
}

/// **A copy a window is owed**: its request, and what the window says once it has landed.
pub(crate) struct OwedCopy {
    pub(crate) request: ClipboardRequest,
    pub(crate) effect: ClipboardWriteEffect,
}

/// **What one window owes the clipboard's answers** — its pastes, in the order they were asked
/// (at most [`PASTES_OWED`]), and one copy (the module's *Capacity*). Held by the window, so that
/// closing it is what abandons them.
#[derive(Default)]
pub(crate) struct ClipboardAsks {
    pub(crate) pastes: Vec<OwedPaste>,
    pub(crate) copy: Option<OwedCopy>,
}

impl ClipboardAsks {
    /// The instant the window's turn must come round by to withdraw a paste, if one is owed.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.pastes
            .iter()
            .filter(|owed| owed.ended.is_none())
            .map(OwedPaste::deadline)
            .min()
    }

    /// Whether the window owes nothing: no paste, and no copy.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.pastes.is_empty() && self.copy.is_none()
    }
}

/// One thing a pane holds, in the order the person offered it.
#[derive(Debug)]
pub(crate) enum Held {
    /// A paste the window asked for this pane: what it writes goes here, when it has answered.
    Paste(ClipboardId),
    /// One write of the person's, kept as the write it was.
    Typed(Vec<u8>),
}

/// **The person's input to one pane, held behind the pastes it is owed** (the side-effects
/// contract's revision (b)9: keys typed after a paste are held behind it in the pane's input order,
/// the way a pane in birth holds them). One queue per pane: each paste asked for it takes its
/// place ([`Held::Paste`]), and every write offered after it waits behind it as the write it was
/// ([`Held::Typed`]), so `Ctrl+V`, `k`, `Ctrl+V` reaches the shell as the first paste, `k`, the
/// second. Held on the pane's `LeafSession`, so a pane that closes takes what it held with it.
#[derive(Debug, Default)]
pub(crate) struct PasteHold {
    items: RefCell<VecDeque<Held>>,
    /// Whether a write was offered that took what is typed here past what the pane's input ring
    /// holds ([`Self::over_the_cap`]).
    crossed: Cell<bool>,
}

impl PasteHold {
    /// A paste asked for this pane takes its place behind everything held.
    pub(crate) fn queue_paste(&self, id: ClipboardId) {
        self.items.borrow_mut().push_back(Held::Paste(id));
    }

    /// Keep one write, behind whatever is held. Taken whole, as the write it was: the cap is the
    /// window's to enforce ([`Self::over_the_cap`]), on the turn the write was offered, by
    /// withdrawing the pastes held here and releasing everything, this write included, in its
    /// order — each write offered as it was typed.
    pub(crate) fn hold(&self, bytes: &[u8]) {
        let typed: usize = self
            .items
            .borrow()
            .iter()
            .map(|held| match held {
                Held::Typed(bytes) => bytes.len(),
                Held::Paste(_) => 0,
            })
            .sum();
        if typed > 0 && typed + bytes.len() > bt_pty::PTY_INPUT_RING_BYTES.get() {
            self.crossed.set(true);
        }
        self.items
            .borrow_mut()
            .push_back(Held::Typed(bytes.to_vec()));
    }

    /// Whether a write offered here would have taken what is held past the pane's input ring's
    /// capacity (`bt_pty::PTY_INPUT_RING_BYTES`, what a pane in birth holds) — the side-effects
    /// contract's (c)4: such a write withdraws the pastes held here.
    pub(crate) fn over_the_cap(&self) -> bool {
        self.crossed.get()
    }

    /// The first thing held, taken off.
    pub(crate) fn pop_front(&self) -> Option<Held> {
        self.items.borrow_mut().pop_front()
    }

    /// Put `held` back at the front.
    pub(crate) fn push_front(&self, held: Held) {
        self.items.borrow_mut().push_front(held);
    }

    /// The pastes whose places this holds.
    pub(crate) fn pastes(&self) -> Vec<ClipboardId> {
        self.items
            .borrow()
            .iter()
            .filter_map(|held| match held {
                Held::Paste(id) => Some(*id),
                Held::Typed(_) => None,
            })
            .collect()
    }
}

/// **The lane contract's adapter** (`crate::lane`, `lane_contract_tests`): the real lane — its
/// ledger, its queue, its worker body and its sink, and each window's [`ClipboardAsks`]-shaped
/// holding of its requests — with the suite's scripted [`crate::lane::Executor`] starting the
/// worker, so the suite's [`crate::lane::Gate`] holds each request, ends the worker or fails a
/// read, and its [`crate::lane::WakeProbe`] stands for the loop's wake. Only the door is a
/// stand-in; nothing of the lane is copied.
#[cfg(test)]
pub(crate) mod contract_adapter {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use bt_platform::admission::WorkerCtx;

    use super::{
        Ask, ClipboardDoor, ClipboardId, ClipboardLane, ClipboardRequest, ClipboardTerminal, Reply,
    };
    use crate::lane::{
        Admission, CLIPBOARD, Contract, Delivered, Executor, Gate, LaneUnderTest, Outcome,
        Terminal, WakeProbe,
    };

    /// The text a question is written as, so the door can say which one it runs.
    const PREFIX: &str = "剪贴板 question ";

    fn question_of(ask: &Ask) -> Option<u64> {
        match ask {
            Ask::Write(text) => text.strip_prefix(PREFIX)?.parse().ok(),
            Ask::Text | Ask::Payload => None,
        }
    }

    /// The door the suite's gate is told about: every question is a write, and the gate learns
    /// which one runs; a planted panic is the gate's.
    struct GateDoor(Arc<Gate>);

    impl ClipboardDoor for GateDoor {
        fn serve(&self, _worker: &WorkerCtx, ask: &Ask) -> Result<Reply, String> {
            if let Some(question) = question_of(ask) {
                self.0.execute(question);
            }
            Ok(Reply::Written)
        }
    }

    pub(crate) fn outcome(terminal: &ClipboardTerminal) -> Outcome {
        match terminal {
            Terminal::Done(_) => Outcome::Answered,
            Terminal::Failed(failure) => Outcome::Failed(failure.to_string()),
            Terminal::Cancelled => Outcome::Cancelled,
            Terminal::Superseded { by } => Outcome::Superseded { by: by.number() },
            Terminal::Joined {
                with,
                outcome: carried,
            } => Outcome::Joined {
                with: with.number(),
                carried: Box::new(outcome(carried)),
            },
            Terminal::WorkerGone => Outcome::WorkerGone,
        }
    }

    struct ClipboardAdapter {
        lane: ClipboardLane,
        door: Arc<dyn ClipboardDoor>,
        gate: Arc<Gate>,
        probe: Arc<WakeProbe>,
        /// One window's requests per open target, in the order asked; a closed window's are
        /// dropped, as `WindowRuntime`'s `ClipboardAsks` is.
        windows: BTreeMap<u32, BTreeMap<ClipboardId, ClipboardRequest>>,
        /// The suite's own record of which question each id carried, for its tally.
        questions: HashMap<ClipboardId, u64>,
        /// What the lane's sink has been handed and the suite not yet read.
        sunk: Arc<Mutex<Vec<(ClipboardId, ClipboardTerminal)>>>,
        /// The lines the ledger said, counted through its `say`.
        said: Arc<AtomicUsize>,
    }

    /// A fresh lane, its `bt-clipboard` worker started by the suite's executor.
    pub(crate) fn make() -> Box<dyn LaneUnderTest> {
        let gate = Arc::new(Gate::default());
        let probe = Arc::new(WakeProbe::default());
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let said = Arc::new(AtomicUsize::new(0));
        let wake = Arc::clone(&probe);
        let (kept, counted) = (Arc::clone(&sunk), Arc::clone(&said));
        let lane = ClipboardLane::start(
            &Executor::scripted(Arc::clone(&gate)),
            move || wake.woke(),
            move |_line: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
            },
            move |id, terminal| {
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((id, terminal));
            },
        );
        Box::new(ClipboardAdapter {
            lane,
            door: Arc::new(GateDoor(Arc::clone(&gate))),
            gate,
            probe,
            windows: BTreeMap::new(),
            questions: HashMap::new(),
            sunk,
            said,
        })
    }

    impl LaneUnderTest for ClipboardAdapter {
        fn contract(&self) -> &'static Contract {
            &CLIPBOARD
        }

        fn gate(&self) -> &Gate {
            &self.gate
        }

        fn probe(&self) -> &WakeProbe {
            &self.probe
        }

        /// `WindowHost::clipboard`: the lane's own admission, the request held in the asking
        /// window's asks, or the refusal at the door as the gesture's terminal.
        fn submit(&mut self, target: u32, question: u64) -> Admission {
            match self.lane.submit(
                Arc::clone(&self.door),
                Ask::Write(format!("{PREFIX}{question}")),
            ) {
                Ok(request) => {
                    let id = request.id();
                    self.questions.insert(id, question);
                    self.windows.entry(target).or_default().insert(id, request);
                    Admission {
                        ticket: Some(id.number()),
                        question,
                        refused: None,
                    }
                }
                Err(declined) => Admission {
                    ticket: None,
                    question,
                    refused: Some(declined.reason().to_owned()),
                },
            }
        }

        fn close_target(&mut self, target: u32) {
            self.windows.remove(&target);
        }

        /// `Runtime::withdraw_paste`: the window takes the request out of its asks and withdraws
        /// it.
        fn withdraw(&mut self, target: u32, ticket: u64) {
            let request = self
                .windows
                .get_mut(&target)
                .and_then(|asks| asks.remove(&ClipboardId(ticket)))
                .expect("the suite withdraws a request it holds");
            self.lane.withdraw(request);
        }

        /// `Runtime::settle_clipboard`: every open window takes what has ended, in the order its
        /// requests were minted.
        fn drain(&mut self) -> Vec<Delivered> {
            let mut found: Vec<(ClipboardId, Delivered)> = Vec::new();
            for (window, asks) in &mut self.windows {
                let ended: Vec<(ClipboardId, ClipboardTerminal)> = asks
                    .iter_mut()
                    .filter_map(|(id, request)| Some((*id, request.take()?)))
                    .collect();
                for (id, terminal) in ended {
                    asks.remove(&id);
                    found.push((
                        id,
                        Delivered {
                            target: Some(*window),
                            ticket: Some(id.number()),
                            question: self.questions.get(&id).copied(),
                            outcome: outcome(&terminal),
                        },
                    ));
                }
            }
            found.sort_by_key(|(id, _)| *id);
            found.into_iter().map(|(_, delivered)| delivered).collect()
        }

        fn offer_stale(&mut self, _ticket: u64) {
            unreachable!("the clipboard lane's target is the asking window, not the application");
        }

        fn in_line(&self) -> Option<usize> {
            Some(self.lane.in_line())
        }

        fn sunk(&mut self) -> Vec<Delivered> {
            let sunk =
                std::mem::take(&mut *self.sunk.lock().unwrap_or_else(PoisonError::into_inner));
            sunk.into_iter()
                .map(|(id, terminal)| Delivered {
                    target: None,
                    ticket: Some(id.number()),
                    question: self.questions.get(&id).copied(),
                    outcome: outcome(&terminal),
                })
                .collect()
        }

        fn said(&self) -> Option<usize> {
            Some(self.said.load(Ordering::SeqCst))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use bt_platform::admission::WorkerCtx;

    use super::*;
    use crate::lane::{Gate, wait_for_a_wake, wait_until, wake_channel};

    /// A door that refuses every read in the words the platform's door uses when five opens were
    /// refused, and counts what it was asked.
    struct Busy(Arc<AtomicUsize>);

    const FIVE_OPENS: &str =
        "OpenClipboard failed after 5 attempts (retry wait capped at 75 ms): Access is denied.";

    impl ClipboardDoor for Busy {
        fn serve(&self, _worker: &WorkerCtx, _ask: &Ask) -> Result<Reply, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(FIVE_OPENS.to_owned())
        }
    }

    /// RED (T-CLIPBOARD-LANE) — **a read the door refused is that request's `Failed`, said once in
    /// the ledger's words, and the worker goes on** to the next request.
    ///
    /// MUTATION: publish `Done(Reply::Text(String::new()))` for a door's `Err` in
    /// `run_clipboard_lane` — the refusal reads as an empty clipboard and nothing is said.
    #[test]
    fn a_refused_read_is_its_requests_failure_said_once_and_the_worker_goes_on() {
        let (wake, wakes) = wake_channel();
        let said = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&said);
        let lane = ClipboardLane::start(
            &Executor::scripted(Arc::new(Gate::default())),
            wake,
            move |line: &str| {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(line.to_owned());
            },
            |_, _| {},
        );
        let asked = Arc::new(AtomicUsize::new(0));
        let door: Arc<dyn ClipboardDoor> = Arc::new(Busy(Arc::clone(&asked)));
        let mut first = lane
            .submit(Arc::clone(&door), Ask::Payload)
            .expect("admitted");
        let mut second = lane.submit(door, Ask::Text).expect("admitted");
        wait_for_a_wake(&wakes, "the first refusal");
        wait_until("both requests' terminals", || {
            asked.load(Ordering::SeqCst) == 2
        });
        let take = |request: &mut ClipboardRequest| {
            let mut ended = None;
            wait_until("a terminal", || {
                ended = request.take();
                ended.is_some()
            });
            ended.expect("taken")
        };
        for request in [&mut first, &mut second] {
            match take(request) {
                Terminal::Failed(failure) => assert_eq!(failure.to_string(), FIVE_OPENS),
                other => panic!("a refused read is its request's failure, not {other:?}"),
            }
        }
        let said = said.lock().unwrap_or_else(PoisonError::into_inner).clone();
        assert_eq!(said.len(), 2, "each failure said once: {said:?}");
        assert!(
            said.iter().all(|line| line.contains(FIVE_OPENS)),
            "{said:?}"
        );
    }

    /// RED (T-CLIPBOARD-LANE) — **a request withdrawn before the worker reached it is never run**:
    /// its terminal is `Cancelled`, it goes to the sink, and the door never hears it.
    ///
    /// MUTATION: leave the withdrawn job in line (drop the `remove` in `lane::Queue::withdraw`) and
    /// drop the `queue.ledger().owes(id)` question in `run_clipboard_lane` — the withdrawn write
    /// reaches the door. Either alone keeps it from the door: the line is the first guard, the
    /// worker's question the second, for a job it took just as it was withdrawn.
    #[test]
    fn a_withdrawn_request_is_dropped_unrun_and_its_cancel_reaches_the_sink() {
        let gate = Arc::new(Gate::default());
        let (wake, wakes) = wake_channel();
        let sunk = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&sunk);
        let lane = ClipboardLane::start(
            &Executor::scripted(Arc::clone(&gate)),
            wake,
            |_: &str| {},
            move |id, terminal| {
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((id, terminal));
            },
        );
        let ran = Arc::new(Mutex::new(Vec::new()));
        struct Record(Arc<Mutex<Vec<String>>>);
        impl ClipboardDoor for Record {
            fn serve(&self, _worker: &WorkerCtx, ask: &Ask) -> Result<Reply, String> {
                if let Ask::Write(text) = ask {
                    self.0
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push(text.clone());
                }
                Ok(Reply::Written)
            }
        }
        let door: Arc<dyn ClipboardDoor> = Arc::new(Record(Arc::clone(&ran)));
        gate.hold();
        let mut running = lane
            .submit(Arc::clone(&door), Ask::Write("第一 first".to_owned()))
            .expect("admitted");
        assert!(gate.wait_entered(1), "the first write is on the worker");
        let withdrawn = lane
            .submit(Arc::clone(&door), Ask::Write("撤回 withdrawn".to_owned()))
            .expect("admitted");
        let withdrawn_id = withdrawn.id();
        let mut after = lane
            .submit(door, Ask::Write("之后 after".to_owned()))
            .expect("admitted");
        assert!(lane.withdraw(withdrawn), "the withdrawal ends it");
        gate.release();
        wait_for_a_wake(&wakes, "the first write's answer");
        for request in [&mut running, &mut after] {
            let mut ended = None;
            wait_until("a write's answer", || {
                ended = request.take();
                ended.is_some()
            });
            assert_eq!(ended, Some(Terminal::Done(Reply::Written)));
        }
        assert_eq!(
            *ran.lock().unwrap_or_else(PoisonError::into_inner),
            ["第一 first".to_owned(), "之后 after".to_owned()],
            "the withdrawn write never reached the door"
        );
        assert_eq!(
            *sunk.lock().unwrap_or_else(PoisonError::into_inner),
            [(withdrawn_id, Terminal::Cancelled)],
            "its one terminal went to the sink"
        );
    }
}
