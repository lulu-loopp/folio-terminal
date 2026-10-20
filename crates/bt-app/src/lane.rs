//! **The lane contract** — one shape every lane is held to, the seam a lane is built on to keep it,
//! and the one suite that holds it (D-33; `docs/plans/design/window-thread-budget-2026-09-25.md` §3
//! and §R-D; `docs/plans/design/side-effects-contract-2026-10-11.md` C1–C7 and revision (b)7;
//! `docs/ARCHITECTURE.md` §5.1, *the lane contract*).
//!
//! A lane is a worker the window thread hands requests to and hears answers from without waiting.
//!
//! **The seam** (product code, S-3) is what a lane is built from so that every request it admits
//! ends exactly once:
//!
//! - an [`Executor`], the worker factory: the lane hands it a body and never calls the thread door
//!   itself. The one the product uses starts the body through `bt_platform::spawn_at_priority`; the
//!   suite's runs the same body under a scripted gate. A task — one request's work — runs inside
//!   [`task`] (`lane::task(ctx, …)`), which turns a panic into that request's `Failed(Panicked)`
//!   and lets the worker go on; the worker's own end (its body returning, or a panic outside a
//!   task) is reported by the executor through [`Ends::worker_ended`]. Both the task and the
//!   worker's body run inside `contained_panic::contain`, so under the resident panic hook, which
//!   ends the process for any other panic, a panic in either reaches the seam (S-5 round 3);
//!   a thread-door refusal is returned synchronously as `WorkerEnd::NotStarted` and also reported
//!   through `Ends`; a lane started before its request door may treat that as its failed start,
//!   while [`Coalescing`] interposes before minting, refuses only that request — said like a
//!   failure, once per streak where the lane declares one, after the lane's lock is let go — and
//!   lets the next new request try the start again without faulting the lane (revision (b)5,
//!   rule 1);
//! - a [`Ledger`], which mints each request's identity or refuses it at the door (C1's admission),
//!   records what each identity is owed — the sink its terminal goes to when its requester has
//!   gone (C6) — and turns every identity into exactly one [`Terminal`]: `Done`, `Failed`,
//!   `Cancelled`, `Superseded { by }`, `Joined { with }` or `WorkerGone` (C2, C4). The requester
//!   holds a [`Request`] and takes its terminal once, as `pty_door::ShellBirth::take` does;
//! - the hooks: [`Ledger::publish`], which ends one identity (never a joined one: a join ends only
//!   with the request it joined, carrying its terminal), and [`Ledger::worker_ended`], which ends
//!   every request still owed as `WorkerGone` (its joins as `Joined` carrying it), leaves the lane
//!   in a fault the consumer can read ([`Ledger::fault`]) and every later admission is refused
//!   with, and then wakes the consumer through the wake the ledger was built with. A lane wakes
//!   its consumer itself after it publishes.
//!
//! A failure reaches `diagnostics.log` once, said by the ledger as its terminal is delivered —
//! once per failed request, a joined request carrying a failure included, or once per streak of
//! the same failure where the lane declares [`Saying::PerStreak`] (C3). The line goes through the
//! `say` the ledger is built with, which is `diagnostics::note` in the product and a counter in
//! the suite.
//!
//! **The suite** (compiled with the tests) states each lane's policy as data (`Contract`, one
//! constant per lane) and checks it against the lane's real behaviour through an adapter
//! (`LaneUnderTest`) that drives the lane's own admission, publication and acceptance. The
//! obligations, one `Claim` each: a full lane answers without making the asker wait, and admits no
//! more than it declares; every request carries its own identity; execution and delivery follow
//! the declared order; an abandoned answer, or one older than the answer adopted, is not raised;
//! every admitted request ends exactly once; the wake follows the publication, and a lost wake
//! loses no answer; the answers held for a consumer that has not drained are bounded; a worker that
//! dies leaves an observable terminal or fault state; a superseded request says so; a joined
//! request carries its twin's outcome; a failure is said once; and an orphaned answer reaches its
//! sink.
//!
//! **A lane that fails a claim is not declared conformant to make the suite pass.** Its failure is
//! a row of `EXPECTED_FAILURES`, with the exact kind of failure and the ledger row that repairs it
//! (`docs/plans/structural-debt.md`). `judge` is red on an unexpected pass, on a failure of a
//! different kind, on a lane or claim that was not run, and on a claim that exercised no request.
//! The OS hand-off lane (`handoff_lane`, S-5) is built on the seam, and so are the font, taskbar
//! and program-walk lanes (`settings::FontLane`, `taskbar_lane`, `programs_lane`; S-4), each a
//! latest-value lane on [`Coalescing`], the computation lane (`computation_lane`, S-8), three
//! first-in-first-out shares on [`Queue`], each with its own ledger, and the git mutation lane
//! (`git_mutation_lane`), a first-in-first-out share on [`Queue`] per repository, and the storage
//! worker (`persist::StorageLane`, T-STORAGE-WORKER-ON-THE-SEAM), on [`Ledger`] and [`Executor`]
//! with a line of its own — one FIFO, the jobs it serves ahead of it, and each kind's own answer
//! road — and the clipboard lane (`clipboard_lane`, S-7, T-CLIPBOARD-LANE), one FIFO on
//! [`Ledger`] and [`Executor`] whose askers withdraw a request (`Cancelled`) or replace it with a
//! newer one (`Superseded`); each adapter hands its real lane a scripted [`Executor`], and every
//! lane keeps every claim.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bt_platform::ThreadPriority;
use bt_platform::admission::WorkerCtx;

#[cfg(test)]
use std::sync::{Condvar, mpsc};
#[cfg(test)]
use std::thread::ThreadId;
#[cfg(test)]
use std::time::{Duration, Instant};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------------------------------------
// The seam
// ------------------------------------------------------------------------------------------------

/// **How one request ended** — exactly one per identity a [`Ledger`] minted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Terminal<Id, T, E> {
    /// The work was done, and this is its answer.
    Done(T),
    /// The work failed, or panicked inside its task.
    Failed(TaskFailure<E>),
    /// The lane dropped the request before its work was done, at its requester's word: the
    /// clipboard lane's paste whose pane closed, whose field let the keyboard go, or that outlived
    /// its deadline or its held-input cap (side-effects contract (c)4; T-CLIPBOARD-LANE).
    Cancelled,
    /// A newer, different request displaced this one (C4), and `by` is that request.
    Superseded { by: Id },
    /// This request was the same operation as `with`, admitted earlier, and ends with `with`'s own
    /// terminal (revision (b)4).
    Joined {
        with: Id,
        outcome: Box<Terminal<Id, T, E>>,
    },
    /// The worker ended with this request still owed.
    WorkerGone,
}

/// **Why one task failed**: the lane's own error, or a panic caught around the task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TaskFailure<E> {
    Error(E),
    Panicked(String),
}

impl<E: fmt::Display> fmt::Display for TaskFailure<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error(error) => error.fmt(f),
            Self::Panicked(message) => write!(f, "the task panicked: {message}"),
        }
    }
}

/// **How a lane's worker ended**, as [`Ends::worker_ended`] is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerEnd {
    /// The body returned.
    Returned,
    /// A panic outside any task ended the thread.
    Panicked(String),
    /// The thread door refused to start it.
    NotStarted(String),
}

impl fmt::Display for WorkerEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Returned => f.write_str("its body returned"),
            Self::Panicked(message) => write!(f, "it panicked: {message}"),
            Self::NotStarted(error) => write!(f, "it could not be started: {error}"),
        }
    }
}

/// **Why a request was refused at the door** — synchronously; the refusal is the request's terminal
/// outcome, and no identity was minted for it (revision (b)3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Refused {
    /// As many identities as the ledger's bound are owed.
    Full { bound: usize },
    /// The lane's worker has ended.
    Faulted(WorkerEnd),
    /// The request asked to join one that has already ended.
    TwinEnded,
    /// The lane's worker could not be started for this request, in the thread door's words: the
    /// request is refused, and the lane is not in fault — its worker never ran — so the next
    /// request starts it again (revision (b)5, rule 1: a new request, not a retry). Answered only
    /// by a lane that starts its worker on a request ([`Coalescing`]).
    NotStarted(String),
}

/// How often the ledger says a failure (C3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Saying {
    /// One line per failed request.
    PerRequest,
    /// One line per streak of the same failure: a failure said like the last one said, with no
    /// answer delivered between them, is not said again. A worker start the thread door refused
    /// ([`Refused::NotStarted`]) is a streak of its own kind: refused again in the same words, with
    /// nothing else said between, it is not said again; a delivered failure and a refused start
    /// never continue each other's streak.
    PerStreak,
}

/// **What one identity is owed**: the sink its terminal goes to when its requester has gone (C6) —
/// the registry entry that outlives the requester. The sink may discard a success; a failure it
/// receives has been said already.
pub(crate) trait Owed<Id>: Send + 'static {
    type Answer: Clone + Send + 'static;
    type Error: Clone + fmt::Display + Send + 'static;

    fn sink(self, id: Id, terminal: Terminal<Id, Self::Answer, Self::Error>);
}

type TerminalOf<Id, O> = Terminal<Id, <O as Owed<Id>>::Answer, <O as Owed<Id>>::Error>;

/// The executor's hook: what the thread's end is reported to.
pub(crate) trait Ends: Send + Sync {
    fn worker_ended(&self, cause: WorkerEnd);
}

impl WorkerEnd {
    /// Whether the worker ran at all: `false` only for a start the thread door refused, so its
    /// asker knows nothing it asked for was done.
    pub(crate) fn started(&self) -> bool {
        !matches!(self, Self::NotStarted(_))
    }
}

/// **The end of a worker that serves one asking and no ledger** (T-PROFILE-INSTALL-REFUSED-START):
/// told how the worker ended, it says an end other than a return once — `"{what} ended:
/// {cause}"`, in the voice it was handed (`diagnostics::note` in the product) — and hands the
/// cause to `answer`, which sets the asker's own failure state and wakes it. A body that returned
/// gave its own answer, so nothing is said or answered for it.
///
/// A refused start is told on the asking thread, before [`Executor::start_worker`] answers it; a
/// panic, on the worker after its body unwound. The `$PROFILE` observation, the one-click install,
/// its Undo and the Settings removal, the Explorer probe and press, the stale-claim sweep and
/// three door processes' waits each end through one.
pub(crate) struct Answered<A> {
    what: &'static str,
    say: Box<dyn Fn(&str) + Send + Sync>,
    answer: A,
}

impl<A: Fn(&WorkerEnd) + Send + Sync + 'static> Answered<A> {
    pub(crate) fn new(
        what: &'static str,
        say: impl Fn(&str) + Send + Sync + 'static,
        answer: A,
    ) -> Arc<Self> {
        Arc::new(Self {
            what,
            say: Box::new(say),
            answer,
        })
    }
}

impl<A: Fn(&WorkerEnd) + Send + Sync> Ends for Answered<A> {
    fn worker_ended(&self, cause: WorkerEnd) {
        if matches!(cause, WorkerEnd::Returned) {
            return;
        }
        (self.say)(&format!("{} ended: {cause}", self.what));
        (self.answer)(&cause);
    }
}

/// **A lane's request ledger.** Shared between the requester's side ([`Ledger::admit`],
/// [`Request`]) and the worker's ([`Ledger::publish`], [`Ledger::worker_ended`]); one mutex, held
/// for a map operation and never across a sink or a line said.
pub(crate) struct Ledger<Id, O: Owed<Id>> {
    lane: &'static str,
    bound: Option<usize>,
    saying: Saying,
    say: Box<dyn Fn(&str) + Send + Sync>,
    /// The lane's wake: what tells its consumer to drain. The worker's end is announced through it
    /// (C2: a completion read by a drain is announced by a wake); a publication is announced by the
    /// lane, which wakes after it publishes.
    wake: Box<dyn Fn() + Send + Sync>,
    book: Mutex<Book<Id, O>>,
}

struct Book<Id, O: Owed<Id>> {
    minted: u64,
    /// Every identity minted and not yet delivered: owed, or ended and waiting to be taken.
    entries: BTreeMap<Id, Entry<Id, O>>,
    fault: Option<WorkerEnd>,
    /// The last failure said, while no answer has been delivered since ([`Saying::PerStreak`]).
    streak: Option<Streak>,
}

/// **What the current streak is a streak of** ([`Saying::PerStreak`]): a delivered failure, in its
/// words, or a worker start the thread door refused, in the door's. The two never continue each
/// other's streak.
#[derive(PartialEq)]
enum Streak {
    Failed(String),
    NotStarted(String),
}

struct Entry<Id, O: Owed<Id>> {
    owed: O,
    requester_gone: bool,
    /// The identity this one joined; it ends when that one does.
    twin: Option<Id>,
    /// The terminal, once ended and until taken.
    terminal: Option<TerminalOf<Id, O>>,
}

/// What leaves the lock: the terminals for sinks, and the lines to say.
struct Out<Id, O: Owed<Id>> {
    sunk: Vec<(O, Id, TerminalOf<Id, O>)>,
    lines: Vec<String>,
}

impl<Id, O: Owed<Id>> Out<Id, O> {
    fn new() -> Self {
        Self {
            sunk: Vec::new(),
            lines: Vec::new(),
        }
    }
}

impl<Id, O> Ledger<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    /// A ledger for the lane named `lane`, owing at most `bound` identities at once (`None`: no
    /// bound), saying its failures through `say` — `diagnostics::note` in the product — and waking
    /// its consumer through `wake`, the lane's own wake (an `AppEvent` sent, in the product).
    pub(crate) fn new(
        lane: &'static str,
        bound: Option<usize>,
        saying: Saying,
        say: impl Fn(&str) + Send + Sync + 'static,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            lane,
            bound,
            saying,
            say: Box::new(say),
            wake: Box::new(wake),
            book: Mutex::new(Book {
                minted: 0,
                entries: BTreeMap::new(),
                fault: None,
                streak: None,
            }),
        })
    }

    /// **Admit a request** (C1): mint its identity and record what it is owed, or refuse it at once.
    pub(crate) fn admit(self: &Arc<Self>, owed: O) -> Result<Request<Id, O>, Refused> {
        self.mint(owed, None)
    }

    /// **Admit a request as the same operation as `with`** (revision (b)4): it ends with `with`'s
    /// terminal, as `Joined { with }`. A request joined to one that is itself joined joins the
    /// first. Refused with [`Refused::TwinEnded`] when `with` has already ended: the request is then
    /// a new operation, to be admitted as one.
    pub(crate) fn admit_joined(
        self: &Arc<Self>,
        with: Id,
        owed: O,
    ) -> Result<Request<Id, O>, Refused> {
        self.mint(owed, Some(with))
    }

    fn mint(self: &Arc<Self>, owed: O, with: Option<Id>) -> Result<Request<Id, O>, Refused> {
        let mut book = lock(&self.book);
        if let Some(end) = &book.fault {
            return Err(Refused::Faulted(end.clone()));
        }
        if let Some(bound) = self.bound
            && book.entries.len() >= bound
        {
            return Err(Refused::Full { bound });
        }
        let twin = match with {
            None => None,
            Some(with) => match book.entries.get(&with) {
                Some(entry) if entry.terminal.is_none() => Some(entry.twin.unwrap_or(with)),
                _ => return Err(Refused::TwinEnded),
            },
        };
        book.minted += 1;
        let id = Id::from(book.minted);
        book.entries.insert(
            id,
            Entry {
                owed,
                requester_gone: false,
                twin,
                terminal: None,
            },
        );
        Ok(Request {
            id,
            ledger: Arc::clone(self),
            settled: false,
        })
    }

    /// **The worker's hook: end `id` with `terminal`.** Answers whether it was this identity's
    /// terminal: an identity ends once, and a later terminal for it — an answer that raced a cancel,
    /// a second publication — is dropped. Every identity joined to `id` ends with it, as
    /// `Joined { with: id, outcome }`. A joined identity is never published itself: it is the same
    /// operation as the one it joined, and ends only with it (revision (b)4), so a terminal
    /// published for it is refused.
    pub(crate) fn publish(&self, id: Id, terminal: TerminalOf<Id, O>) -> bool {
        let mut out = Out::new();
        {
            let mut book = lock(&self.book);
            match book.entries.get(&id) {
                Some(entry) if entry.terminal.is_none() && entry.twin.is_none() => {}
                _ => return false,
            }
            self.end_with_twins(&mut book, id, terminal, &mut out);
        }
        self.release(out);
        true
    }

    /// **The executor's hook: the worker has ended.** Every operation still owed ends as
    /// `WorkerGone`, and every identity joined to one as `Joined { with, outcome: WorkerGone }`;
    /// the lane is in fault from now on ([`Self::fault`]), and every later admission is refused
    /// with it. Said once, unless the body returned owing nothing; and when it owed anything, the
    /// consumer is woken after the terminals are recorded, since no publication of the lane's will
    /// announce them. A worker that ends owing nothing — a lane let go, a body that returned — ends
    /// no request, so it wakes nobody: there is nothing to take, and the fault is answered at the
    /// next admission.
    pub(crate) fn worker_ended(&self, cause: WorkerEnd) {
        let mut out = Out::new();
        let owed = {
            let mut book = lock(&self.book);
            let owed = book
                .entries
                .values()
                .filter(|entry| entry.terminal.is_none())
                .count();
            if cause != WorkerEnd::Returned || owed > 0 {
                out.lines.push(format!(
                    "{}: the worker ended ({cause}); {owed} request(s) owed end as WorkerGone",
                    self.lane,
                ));
            }
            let operations: Vec<Id> = book
                .entries
                .iter()
                .filter(|(_, entry)| entry.terminal.is_none() && entry.twin.is_none())
                .map(|(id, _)| *id)
                .collect();
            for id in operations {
                self.end_with_twins(&mut book, id, Terminal::WorkerGone, &mut out);
            }
            book.fault.get_or_insert(cause);
            owed
        };
        self.release(out);
        if owed > 0 {
            (self.wake)();
        }
    }

    /// End the operation `id` with `terminal`, and every identity joined to it with
    /// `Joined { with: id, outcome: terminal }`.
    fn end_with_twins(
        &self,
        book: &mut Book<Id, O>,
        id: Id,
        terminal: TerminalOf<Id, O>,
        out: &mut Out<Id, O>,
    ) {
        let twins: Vec<Id> = book
            .entries
            .iter()
            .filter(|(_, entry)| entry.twin == Some(id) && entry.terminal.is_none())
            .map(|(twin, _)| *twin)
            .collect();
        for twin in twins {
            let joined = Terminal::Joined {
                with: id,
                outcome: Box::new(terminal.clone()),
            };
            self.end(book, twin, joined, out);
        }
        self.end(book, id, terminal, out);
    }

    /// **Say a worker start the thread door refused** (C3): the refusal is the terminal of the
    /// request that asked for the start, given at the door, and the lane is not in fault. Said like
    /// any failure — once per request, or once per streak where the lane declares
    /// [`Saying::PerStreak`], so a door that keeps refusing in the same words is said once. A start
    /// that succeeds is the lane's last: its worker is never started again, and after its end every
    /// request is refused `Faulted` before any start, so no refused-start streak outlives it to be
    /// ended. Said after the ledger's lock is let go.
    fn say_not_started(&self, end: &WorkerEnd) {
        let said = end.to_string();
        let line = {
            let mut book = lock(&self.book);
            let again = matches!(&book.streak, Some(Streak::NotStarted(last)) if *last == said);
            let line = (self.saying == Saying::PerRequest || !again).then(|| {
                format!(
                    "{}: the worker was not started — {said}; the request is refused, and the \
                     next request starts it again",
                    self.lane
                )
            });
            book.streak = Some(Streak::NotStarted(said));
            line
        };
        if let Some(line) = line {
            (self.say)(&line);
        }
    }

    /// **The earliest identity still owed a terminal**, or `None` when every identity minted and
    /// not yet delivered has one. On a lane that ends its identities in the order it minted them
    /// (one worker, first in first out, its end ending the rest), every identity below this one
    /// has ended, so a consumer that takes only those takes a prefix, and the next prefix it
    /// takes follows it.
    pub(crate) fn earliest_owed(&self) -> Option<Id> {
        lock(&self.book)
            .entries
            .iter()
            .find(|(_, entry)| entry.terminal.is_none())
            .map(|(id, _)| *id)
    }

    /// **The last identity this ledger minted**, or `None` before its first. On a lane that ends
    /// its identities in the order it minted them, every identity owed now that will end before a
    /// later admission is at or below it.
    pub(crate) fn last_minted(&self) -> Option<Id> {
        let minted = lock(&self.book).minted;
        (minted > 0).then(|| Id::from(minted))
    }

    /// **Whether `id` is still owed its terminal** — what a worker asks before it begins a
    /// request, so that one its requester has withdrawn (`Cancelled`) or replaced (`Superseded`)
    /// is dropped before its work rather than run for nobody.
    pub(crate) fn owes(&self, id: Id) -> bool {
        lock(&self.book)
            .entries
            .get(&id)
            .is_some_and(|entry| entry.terminal.is_none())
    }

    /// The fault the lane is in: how its worker ended, once it has. A lane's consumer learns it
    /// from the terminals and the refusals; the proof reads it here.
    #[cfg(test)]
    pub(crate) fn fault(&self) -> Option<WorkerEnd> {
        lock(&self.book).fault.clone()
    }

    /// Record `id`'s terminal: kept for its requester, or handed to its sink when the requester has
    /// gone. Its twin link is cut, so it ends once.
    fn end(
        &self,
        book: &mut Book<Id, O>,
        id: Id,
        terminal: TerminalOf<Id, O>,
        out: &mut Out<Id, O>,
    ) {
        let Some(entry) = book.entries.get_mut(&id) else {
            return;
        };
        entry.twin = None;
        if !entry.requester_gone {
            entry.terminal = Some(terminal);
            return;
        }
        if let Some(entry) = book.entries.remove(&id) {
            self.delivered(book, id, &terminal, out);
            out.sunk.push((entry.owed, id, terminal));
        }
    }

    /// A terminal reaches its consumer: the failure line it owes (C3), and the streak it ends.
    fn delivered(
        &self,
        book: &mut Book<Id, O>,
        id: Id,
        terminal: &TerminalOf<Id, O>,
        out: &mut Out<Id, O>,
    ) {
        // A joined request is a request of its own: it fails, or is answered, with the outcome it
        // carries, and is said (or ends a streak) like any other (C3, once per request).
        let (outcome, joined) = match terminal {
            Terminal::Joined { with, outcome } => (outcome.as_ref(), Some(*with)),
            _ => (terminal, None),
        };
        match outcome {
            Terminal::Failed(failure) => {
                let said = failure.to_string();
                let again = matches!(&book.streak, Some(Streak::Failed(last)) if *last == said);
                if self.saying == Saying::PerRequest || !again {
                    out.lines.push(match joined {
                        Some(with) => format!(
                            "{}: request {id:?} (joined to {with:?}) failed: {said}",
                            self.lane
                        ),
                        None => format!("{}: request {id:?} failed: {said}", self.lane),
                    });
                }
                book.streak = Some(Streak::Failed(said));
            }
            Terminal::Done(_) => book.streak = None,
            Terminal::Cancelled
            | Terminal::Superseded { .. }
            | Terminal::Joined { .. }
            | Terminal::WorkerGone => {}
        }
    }

    fn release(&self, out: Out<Id, O>) {
        for line in &out.lines {
            (self.say)(line);
        }
        for (owed, id, terminal) in out.sunk {
            owed.sink(id, terminal);
        }
    }

    /// The requester takes `id`'s terminal, if it has ended and `wanted` says so of it.
    fn take(
        &self,
        id: Id,
        wanted: impl FnOnce(&TerminalOf<Id, O>) -> bool,
    ) -> Option<TerminalOf<Id, O>> {
        let mut out = Out::new();
        let terminal = {
            let mut book = lock(&self.book);
            if !wanted(book.entries.get(&id)?.terminal.as_ref()?) {
                return None;
            }
            let terminal = book.entries.remove(&id)?.terminal?;
            self.delivered(&mut book, id, &terminal, &mut out);
            terminal
        };
        self.release(out);
        Some(terminal)
    }

    /// The requester has gone: a terminal already there goes to the sink now, a later one when it
    /// comes.
    fn requester_gone(&self, id: Id) {
        let mut out = Out::new();
        {
            let mut book = lock(&self.book);
            let Some(entry) = book.entries.get_mut(&id) else {
                return;
            };
            entry.requester_gone = true;
            if entry.terminal.is_some()
                && let Some(entry) = book.entries.remove(&id)
                && let Some(terminal) = entry.terminal
            {
                self.delivered(&mut book, id, &terminal, &mut out);
                out.sunk.push((entry.owed, id, terminal));
            }
        }
        self.release(out);
    }
}

impl<Id, O> Ends for Ledger<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    fn worker_ended(&self, cause: WorkerEnd) {
        Ledger::worker_ended(self, cause);
    }
}

/// **One admitted request, held by its requester.** [`Self::take`] hands over its terminal once;
/// dropped untaken — the requester has gone — it leaves its terminal to the identity's sink.
pub(crate) struct Request<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    id: Id,
    ledger: Arc<Ledger<Id, O>>,
    settled: bool,
}

impl<Id, O> Request<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    pub(crate) fn id(&self) -> Id {
        self.id
    }

    /// The terminal, once the request has ended; `None` before, and after it was taken.
    pub(crate) fn take(&mut self) -> Option<TerminalOf<Id, O>> {
        self.take_if(|_| true)
    }

    /// The terminal, once the request has ended, if `wanted` says so of it; left for a later take
    /// otherwise.
    pub(crate) fn take_if(
        &mut self,
        wanted: impl FnOnce(&TerminalOf<Id, O>) -> bool,
    ) -> Option<TerminalOf<Id, O>> {
        if self.settled {
            return None;
        }
        let terminal = self.ledger.take(self.id, wanted);
        self.settled = terminal.is_some();
        terminal
    }
}

impl<Id, O> Drop for Request<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    fn drop(&mut self) {
        if !self.settled {
            self.ledger.requester_gone(self.id);
        }
    }
}

/// **A worker factory.** A lane hands it its body; the body runs on a thread of its own and the
/// thread's end is reported to the lane's ledger. The product's starts the thread through the
/// thread door at `band`; the suite's (`Executor::scripted`) starts the same body under the suite's
/// gate, which can hold each task, end the worker, or make a task panic.
#[derive(Clone)]
pub(crate) struct Executor {
    band: ThreadPriority,
    /// The stack its workers are given, in bytes; `None`, the thread door's default.
    stack: Option<usize>,
    #[cfg(test)]
    script: Option<Arc<Gate>>,
}

impl Executor {
    /// Workers started through `bt_platform::spawn_at_priority` at `band`.
    pub(crate) fn thread_door(band: ThreadPriority) -> Self {
        Self {
            band,
            stack: None,
            #[cfg(test)]
            script: None,
        }
    }

    /// **This executor, its workers given a stack of `bytes`** — for a worker whose work says how
    /// deep it may go (the formula renderer's `bt_math::MATH_WORKER_STACK_BYTES`).
    pub(crate) fn with_stack(&self, bytes: usize) -> Self {
        Self {
            band: self.band,
            stack: Some(bytes),
            #[cfg(test)]
            script: self.script.clone(),
        }
    }

    /// **Start a lane's worker**, `name` its thread's name. When the thread ends — the body
    /// returns, or a panic outside a task unwinds it — `ends` is told how. A thread the door
    /// refuses is reported to `ends` too, and the refusal is answered here. The body is lent the
    /// thread's `WorkerCtx`, as every body the thread door starts is, and runs each request's work
    /// through [`task`].
    pub(crate) fn start_worker(
        &self,
        name: &'static str,
        ends: Arc<impl Ends + 'static>,
        body: impl FnOnce(&WorkerCtx) + Send + 'static,
    ) -> Result<(), WorkerEnd> {
        #[cfg(test)]
        let script = self.script.clone();
        // The suite's gate can have the thread door refuse this start, as a machine out of
        // threads would (`Gate::refuse_next_start`); the refusal is reported as the door's is.
        #[cfg(test)]
        if script
            .as_ref()
            .is_some_and(|gate| gate.take_refused_start())
        {
            let cause = WorkerEnd::NotStarted(REFUSED_START.to_owned());
            ends.worker_ended(cause.clone());
            return Err(cause);
        }
        let ended = Arc::clone(&ends);
        let started =
            bt_platform::spawn_at_priority_with_stack(name, self.band, self.stack, move |ctx| {
                #[cfg(test)]
                SCRIPT.with(|held| *held.borrow_mut() = script);
                // Contained (`contained_panic::contain`): the resident panic hook lets a panic
                // outside a task unwind to here, where it is the worker's end, not the process's.
                let cause = match crate::contained_panic::contain(|| body(ctx)) {
                    Ok(()) => WorkerEnd::Returned,
                    Err(panic) => WorkerEnd::Panicked(
                        crate::pty_door::panic_message(panic.as_ref()).to_owned(),
                    ),
                };
                ended.worker_ended(cause);
            });
        match started {
            Ok(_) => Ok(()),
            Err(error) => {
                let cause = WorkerEnd::NotStarted(error.to_string());
                ends.worker_ended(cause.clone());
                Err(cause)
            }
        }
    }
}

/// **Run one request's task on a lane's worker** (`ctx`, the capability the thread door lent its
/// body). A panic inside it is that request's failure, `TaskFailure::Panicked`, and the worker
/// goes on (C3; `pty_door`'s birth does the same).
pub(crate) fn task<T, E>(
    ctx: &WorkerCtx,
    run: impl FnOnce(&WorkerCtx) -> Result<T, E>,
) -> Result<T, TaskFailure<E>> {
    arrive();
    // Contained (`contained_panic::contain`): the resident panic hook lets a panic inside the
    // task unwind to here, where it is this request's failure, not the process's end.
    match crate::contained_panic::contain(|| run(ctx)) {
        Ok(result) => result.map_err(TaskFailure::Error),
        Err(panic) => Err(TaskFailure::Panicked(
            crate::pty_door::panic_message(panic.as_ref()).to_owned(),
        )),
    }
}

/// **What a request is owed on a lane whose requester is the application** (C6): the application
/// asks, holds every request it made ([`Coalescing`]) for the life of the process, and does not
/// go, so it is both the requester and the sink. A request reaches this sink only when the lane
/// itself is dropped with the request untaken, at the process's end; its terminal is let go there
/// — a success with nobody left to tell, a failure said already by the ledger as it was delivered.
pub(crate) struct ToTheApplication<A, E>(std::marker::PhantomData<fn() -> (A, E)>);

impl<A, E> ToTheApplication<A, E> {
    pub(crate) const fn owed() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<Id, A, E> Owed<Id> for ToTheApplication<A, E>
where
    A: Clone + Send + 'static,
    E: Clone + fmt::Display + Send + 'static,
{
    type Answer = A;
    type Error = E;

    fn sink(self, _id: Id, _terminal: Terminal<Id, A, E>) {}
}

/// **The run a latest-value lane's worker does next**: the operation it ends (`Done` or `Failed`,
/// and with it every request joined to it), the newest request that run answers — the operation
/// or the last request joined to it — and what the run is asked to do.
pub(crate) struct Waiting<Id, Op> {
    pub(crate) operation: Id,
    pub(crate) newest: Id,
    pub(crate) op: Op,
    /// How many of the requests this run stands for — the operation and every request joined to
    /// it — no [`Coalescing::count_arrivals`] has counted yet.
    uncounted: usize,
}

/// What [`Coalescing::ask`] admitted: the request's identity.
pub(crate) struct Admitted<Id> {
    pub(crate) id: Id,
}

/// **What [`Coalescing::count_arrivals`] counted**: of the requests made since the count before
/// it and not taken by the worker as the run it does, how many the run waiting now stands for —
/// the next run answers them — and how many a newer, different request displaced, each ended
/// `Superseded { by }`.
pub(crate) struct Arrivals {
    pub(crate) waiting: usize,
    pub(crate) superseded: usize,
}

/// **A latest-value lane on the seam** (the side-effects contract's C2 and C4, revision (b)4):
/// coalescing kept as the policy — one run out, at most one waiting, and a newer answer serves
/// every request standing — with every request its own terminal.
///
/// - A request asked while one is waiting and **the same operation** as it (`same`) joins it
///   ([`Ledger::admit_joined`]): it ends `Joined { with }` carrying the waiting request's own
///   terminal, and one run answers both.
/// - A request asked while one is waiting and **a different operation** displaces it: the
///   waiting one ends `Superseded { by }` the new one (its joins carrying that), and the new one
///   waits in its place.
/// - A request asked while a run is out and none is waiting waits for the next run, because the
///   run out may have read the world before the change the request is about.
///
/// `fold` makes the waiting operation from the one it joins or replaces and the newer: what the
/// newer asks, plus whatever of the older's the lane keeps (the program walk's rows asked first
/// and its urgency).
///
/// The application holds every request it made here and takes their terminals in the order the
/// ledger minted them ([`Self::ended`]) — below the earliest request still owed, so a request a
/// supersession ended early is taken after the run that was out before it.
pub(crate) struct Coalescing<Id, O, Op>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    ledger: Arc<Ledger<Id, O>>,
    book: Mutex<Standing<Id, O, Op>>,
    asked: std::sync::Condvar,
}

struct Standing<Id, O, Op>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    waiting: Option<Waiting<Id, Op>>,
    /// How many requests a newer, different one has displaced that no
    /// [`Coalescing::count_arrivals`] has counted yet — kept here, in the lock every admission
    /// and every take holds, so a displacement is counted whenever it happens.
    superseded: usize,
    /// Every request the application made and has not taken the terminal of, by identity.
    held: BTreeMap<Id, Request<Id, O>>,
    /// Whether the lane's worker has been started — by the first request whose start the thread
    /// door did not refuse.
    started: bool,
}

/// **The ledger as a started worker reports its end to it** — what [`Coalescing::ask`] hands the
/// executor. A worker that ran and ended faults the lane ([`Ledger::worker_ended`]); a start the
/// thread door refused is not such an end — the worker never ran, and nothing is owed on it — and
/// is answered at the door instead, as the refusal of the request that would have started it.
pub(crate) struct Ran<Id, O: Owed<Id>>(Arc<Ledger<Id, O>>);

impl<Id, O> Ends for Ran<Id, O>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    fn worker_ended(&self, cause: WorkerEnd) {
        if !matches!(cause, WorkerEnd::NotStarted(_)) {
            self.0.worker_ended(cause);
        }
    }
}

impl<Id, O, Op> Coalescing<Id, O, Op>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    pub(crate) fn new(ledger: Arc<Ledger<Id, O>>) -> Self {
        Self {
            ledger,
            book: Mutex::new(Standing {
                waiting: None,
                superseded: 0,
                held: BTreeMap::new(),
                started: false,
            }),
            asked: std::sync::Condvar::new(),
        }
    }

    /// The lane's ledger: what its worker publishes through and the executor reports the
    /// worker's end to.
    pub(crate) fn ledger(&self) -> &Arc<Ledger<Id, O>> {
        &self.ledger
    }

    /// **Admit one request (C1): mint its identity, join it to the waiting run or displace that
    /// run, and hold it for the application** — or refuse it at once, minting nothing, with the
    /// ledger's refusal (revision (b)3). Never waits for the worker.
    ///
    /// **The lane's worker is started by the first request that finds it not started**, through
    /// `start` — the lane's own `Executor::start_worker` call, handed the ledger as a started
    /// worker reports to it ([`Ran`]) — before anything is minted. A start the thread door refuses
    /// refuses this request at the door, [`Refused::NotStarted`], said once per streak where the
    /// lane declares streaks ([`Saying::PerStreak`]); the lane is not in
    /// fault, and the next request starts the worker again (revision (b)5, rule 1: a new request
    /// tries again, and nothing is retried on its own).
    pub(crate) fn ask(
        &self,
        op: Op,
        owed: O,
        same: impl Fn(&Op, &Op) -> bool,
        fold: impl FnOnce(Op, Op) -> Op,
        start: impl FnOnce(Arc<Ran<Id, O>>) -> Result<(), WorkerEnd>,
    ) -> Result<Admitted<Id>, Refused> {
        let mut book = lock(&self.book);
        if !book.started {
            if let Err(end) = start(Arc::new(Ran(Arc::clone(&self.ledger)))) {
                // Said after the lane's lock is let go. A lock of the calling lane's own may still
                // be held (the program walk's, across its admission); that is harmless: a refused
                // start means the lane has no worker to wait on it, and a requester or consumer
                // waiting on it waits only for one diagnostics line's write — the door answered
                // without waiting for a thread, and nothing here waits for one either.
                drop(book);
                self.ledger.say_not_started(&end);
                return Err(Refused::NotStarted(end.to_string()));
            }
            book.started = true;
        }
        let (request, waiting) = match book.waiting.take() {
            Some(waiting) if same(&waiting.op, &op) => {
                match self.ledger.admit_joined(waiting.operation, owed) {
                    Ok(request) => {
                        let waiting = Waiting {
                            newest: request.id(),
                            op: fold(waiting.op, op),
                            uncounted: waiting.uncounted + 1,
                            ..waiting
                        };
                        (request, waiting)
                    }
                    Err(refused) => {
                        book.waiting = Some(waiting);
                        return Err(refused);
                    }
                }
            }
            Some(waiting) => match self.ledger.admit(owed) {
                Ok(request) => {
                    let by = request.id();
                    self.ledger
                        .publish(waiting.operation, Terminal::Superseded { by });
                    book.superseded += waiting.uncounted;
                    let waiting = Waiting {
                        operation: by,
                        newest: by,
                        op: fold(waiting.op, op),
                        uncounted: 1,
                    };
                    (request, waiting)
                }
                Err(refused) => {
                    book.waiting = Some(waiting);
                    return Err(refused);
                }
            },
            None => {
                let request = self.ledger.admit(owed)?;
                let id = request.id();
                let waiting = Waiting {
                    operation: id,
                    newest: id,
                    op,
                    uncounted: 1,
                };
                (request, waiting)
            }
        };
        let id = request.id();
        book.waiting = Some(waiting);
        book.held.insert(id, request);
        drop(book);
        self.asked.notify_one();
        Ok(Admitted { id })
    }

    /// **The worker's: the next run**, waiting on the lane's condition variable until a request
    /// stands. The run is taken out, so a request asked from here on waits for the run after it.
    /// Called only on the lane's worker (`ctx`, the capability the thread door lent its body).
    pub(crate) fn next(&self, ctx: &WorkerCtx) -> Waiting<Id, Op> {
        let _ = ctx;
        let mut book = lock(&self.book);
        loop {
            if let Some(waiting) = book.waiting.take() {
                return waiting;
            }
            book = self
                .asked
                .wait(book)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// **Count the requests made since the count before this one, and not taken as a run** (the
    /// worker's, when a run ends): those the run waiting now stands for, and those a newer,
    /// different request displaced — whenever the displacement happened, during the run or before
    /// the worker took it. Each request is counted once: by the count after it is made, or never
    /// when the worker takes it as the run it does. One lock; never waits for the worker.
    pub(crate) fn count_arrivals(&self) -> Arrivals {
        let mut book = lock(&self.book);
        let waiting = book
            .waiting
            .as_mut()
            .map_or(0, |waiting| std::mem::take(&mut waiting.uncounted));
        let superseded = std::mem::take(&mut book.superseded);
        Arrivals {
            waiting,
            superseded,
        }
    }

    /// What `look` makes of the run waiting now, if one is. One lock; never waits for the worker.
    pub(crate) fn waiting<R>(&self, look: impl FnOnce(Option<&Waiting<Id, Op>>) -> R) -> R {
        look(lock(&self.book).waiting.as_ref())
    }

    /// **The application takes every terminal it is owed below the earliest request still owed**
    /// ([`Ledger::earliest_owed`], read once, before the requests are looked at), in the order the
    /// ledger minted them, each once. A request at or past the mark waits for the next take even
    /// when it has ended since. One lock; never waits for the worker.
    pub(crate) fn ended(&self) -> Vec<(Id, TerminalOf<Id, O>)> {
        let owed = self.ledger.earliest_owed();
        let mut book = lock(&self.book);
        let ended: Vec<(Id, TerminalOf<Id, O>)> = book
            .held
            .iter_mut()
            .take_while(|(id, _)| owed.is_none_or(|owed| **id < owed))
            .filter_map(|(id, request)| Some((*id, request.take()?)))
            .collect();
        for (id, _) in &ended {
            book.held.remove(id);
        }
        ended
    }
}

/// **How a request asked of a [`Queue`] relates to one waiting there** — the lane's per-target
/// policy, stated for each pair (revision (b)4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Relation {
    /// Unrelated: the newer request waits behind the older one.
    Other,
    /// The same operation: the newer request joins the waiting one (`Joined { with }`) and is not
    /// run itself.
    Same,
    /// A newer, different operation on the same target: the waiting one ends
    /// `Superseded { by }` the newer, which takes the back of the line.
    Displaces,
}

/// **A first-in, first-out lane on the seam** (the side-effects contract's C1, C2 and C4): every
/// request admitted is run, one at a time, in the order its identity was minted — unless a newer
/// request for the same target displaces it while it waits ([`Relation::Displaces`]) or is the
/// same operation and joins it ([`Relation::Same`]). Each request ends exactly once, through the
/// ledger.
///
/// The worker is started by the first request that finds it not started, as [`Coalescing`]'s is,
/// and a start the thread door refuses refuses that request alone, at the door, with no identity
/// minted ([`Refused::NotStarted`], said once per streak where the lane declares streaks); the
/// lane is not in fault, and the next request starts it again (revision (b)5, rule 1).
///
/// A request a newer one displaces ends `Superseded { by }` at once, and the lane's wake announces
/// it, so its consumer can take that terminal — which carries no answer — without waiting for the
/// run before it.
///
/// The requester holds what [`Self::ask`] admitted. The identities end in the order they were
/// minted — one worker, first in first out, a displaced or joined request ending no later than
/// the one that ends it, the worker's end ending the rest — so a consumer may take the terminals
/// below [`Ledger::earliest_owed`] as a prefix.
pub(crate) struct Queue<Id, O, Job>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    ledger: Arc<Ledger<Id, O>>,
    line: Mutex<Line<Id, Job>>,
    asked: std::sync::Condvar,
}

struct Line<Id, Job> {
    /// The requests admitted and not yet taken by the worker, in the order they were minted.
    waiting: std::collections::VecDeque<(Id, Job)>,
    /// Whether the worker has been started — by the first request whose start the thread door did
    /// not refuse.
    started: bool,
    /// Whether the lane's owner has let it go: the worker runs what is in line, then returns.
    closed: bool,
}

impl<Id, O, Job> Queue<Id, O, Job>
where
    Id: Copy + Ord + fmt::Debug + From<u64> + Send + 'static,
    O: Owed<Id>,
{
    pub(crate) fn new(ledger: Arc<Ledger<Id, O>>) -> Self {
        Self {
            ledger,
            line: Mutex::new(Line {
                waiting: std::collections::VecDeque::new(),
                started: false,
                closed: false,
            }),
            asked: std::sync::Condvar::new(),
        }
    }

    /// The lane's ledger: what its worker publishes through and the executor reports the
    /// worker's end to.
    pub(crate) fn ledger(&self) -> &Arc<Ledger<Id, O>> {
        &self.ledger
    }

    /// **Admit one request (C1): start the worker if it is not started, mint the request's
    /// identity — joined to the waiting request it is the same operation as, or displacing the one
    /// it supersedes — and put its job in line**; or refuse it at once, minting nothing, and hand
    /// the job back with the refusal (revision (b)3). Never waits for the worker.
    ///
    /// `relate(waiting, asked)` says how the asked job relates to each one waiting, oldest first;
    /// the first that is not [`Relation::Other`] decides. `start` is the lane's own
    /// `Executor::start_worker` call, handed the ledger as a started worker reports to it
    /// ([`Ran`]).
    pub(crate) fn ask(
        &self,
        job: Job,
        owed: O,
        relate: impl Fn(&Job, &Job) -> Relation,
        start: impl FnOnce(Arc<Ran<Id, O>>) -> Result<(), WorkerEnd>,
    ) -> Result<Request<Id, O>, (Refused, Job)> {
        let mut line = lock(&self.line);
        if !line.started {
            if let Err(end) = start(Arc::new(Ran(Arc::clone(&self.ledger)))) {
                // Said after the lane's lock is let go.
                drop(line);
                self.ledger.say_not_started(&end);
                return Err((Refused::NotStarted(end.to_string()), job));
            }
            line.started = true;
        }
        // Whether this request displaced one: its terminal is announced by the wake, after the
        // line's lock is let go, so the consumer can take it at once and free its place.
        let mut superseded = false;
        let related = line
            .waiting
            .iter()
            .enumerate()
            .find_map(|(at, (_, waiting))| match relate(waiting, &job) {
                Relation::Other => None,
                relation => Some((at, relation)),
            });
        let request = match related {
            Some((at, Relation::Same)) => {
                let with = line.waiting[at].0;
                return self
                    .ledger
                    .admit_joined(with, owed)
                    .map_err(|refused| (refused, job));
            }
            Some((at, Relation::Displaces)) => {
                let request = match self.ledger.admit(owed) {
                    Ok(request) => request,
                    Err(refused) => return Err((refused, job)),
                };
                if let Some((displaced, _)) = line.waiting.remove(at) {
                    self.ledger
                        .publish(displaced, Terminal::Superseded { by: request.id() });
                    superseded = true;
                }
                request
            }
            _ => match self.ledger.admit(owed) {
                Ok(request) => request,
                Err(refused) => return Err((refused, job)),
            },
        };
        line.waiting.push_back((request.id(), job));
        drop(line);
        self.asked.notify_one();
        if superseded {
            (self.ledger.wake)();
        }
        Ok(request)
    }

    /// **The worker's: the next job**, waiting on the lane's condition variable until one is in
    /// line; `None` once the lane is let go ([`Self::close`]) and nothing waits. Called only on
    /// the lane's worker (`ctx`, the capability the thread door lent its body).
    pub(crate) fn next(&self, ctx: &WorkerCtx) -> Option<(Id, Job)> {
        let _ = ctx;
        let mut line = lock(&self.line);
        loop {
            if let Some(next) = line.waiting.pop_front() {
                return Some(next);
            }
            if line.closed {
                return None;
            }
            line = self
                .asked
                .wait(line)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// **The worker's body**: each job in line run by `run` inside its own [`task`], its terminal
    /// published, then the wake — until the lane is let go.
    pub(crate) fn serve(
        &self,
        ctx: &WorkerCtx,
        mut run: impl FnMut(&WorkerCtx, Job) -> Result<O::Answer, O::Error>,
        wake: &dyn Fn(),
    ) {
        while let Some((id, job)) = self.next(ctx) {
            let terminal = match task(ctx, |ctx| run(ctx, job)) {
                Ok(answer) => Terminal::Done(answer),
                Err(failure) => Terminal::Failed(failure),
            };
            self.ledger.publish(id, terminal);
            wake();
        }
    }

    /// **The lane's owner lets it go**: the worker runs what is in line and then returns, owing
    /// nothing.
    pub(crate) fn close(&self) {
        lock(&self.line).closed = true;
        self.asked.notify_all();
    }

    /// **The asker's: end `id` with `terminal` and take its job out of line** — a request its
    /// asker withdrew (`Cancelled`) or replaced (`Superseded { by }`) leaves the line with its
    /// terminal, its job dropped under the line's lock, as [`Relation::Displaces`] takes a
    /// displaced one out, so what waits in line is never more than what the ledger owes. A job
    /// the worker has already taken stays with the worker, which asks [`Ledger::owes`] before it
    /// begins it. Answers whether this was `id`'s terminal (the ledger's [`Ledger::publish`]).
    pub(crate) fn withdraw(&self, id: Id, terminal: TerminalOf<Id, O>) -> bool {
        let taken = {
            let mut line = lock(&self.line);
            let at = line.waiting.iter().position(|(waiting, _)| *waiting == id);
            at.and_then(|at| line.waiting.remove(at))
        };
        drop(taken);
        self.ledger.publish(id, terminal)
    }

    /// How many requests wait in line, admitted and not yet taken by the worker — where a test
    /// sees a request wait for the one before it.
    #[cfg(test)]
    pub(crate) fn in_line(&self) -> usize {
        lock(&self.line).waiting.len()
    }
}

/// The answer a terminal carries, its own or — for a request that joined another — its twin's.
pub(crate) fn answer_of<Id, T, E>(terminal: Terminal<Id, T, E>) -> Option<T> {
    match terminal {
        Terminal::Done(answer) => Some(answer),
        Terminal::Joined { outcome, .. } => answer_of(*outcome),
        Terminal::Failed(_)
        | Terminal::Cancelled
        | Terminal::Superseded { .. }
        | Terminal::WorkerGone => None,
    }
}

/// Where a task begins, outside its panic catch: the suite's gate holds it here, or ends the
/// worker here, on a worker the suite's executor started.
fn arrive() {
    #[cfg(test)]
    if let Some(gate) = SCRIPT.with(|held| held.borrow().clone()) {
        gate.arrive();
    }
}

#[cfg(test)]
thread_local! {
    /// The gate of the suite's executor, on a worker it started.
    static SCRIPT: std::cell::RefCell<Option<Arc<Gate>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
impl Executor {
    /// The suite's executor: workers started on threads the test starts, each task passing `gate`
    /// (`Gate::arrive`) before it runs.
    pub(crate) fn scripted(gate: Arc<Gate>) -> Self {
        Self {
            band: ThreadPriority::Normal,
            stack: None,
            script: Some(gate),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// The suite
// ------------------------------------------------------------------------------------------------
/// The lanes the suite runs. Every one has an adapter; [`judge`] refuses a run that skipped one.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum LaneName {
    /// `handoff_lane`: the OS hand-off lane, the reference instance.
    Handoff,
    /// `settings::FontLane`: the machine's font families.
    Font,
    /// `taskbar_lane`: whether the taskbar hides itself.
    Taskbar,
    /// `computation_lane`'s formula, decode and glance share (`bt-math-worker`).
    Computation,
    /// `computation_lane`'s resample share (`bt-image-scale-worker`).
    ComputationScale,
    /// `computation_lane`'s path-verification share (`bt-path-verify-worker`).
    ComputationPath,
    /// `programs_lane`: what this machine can start (T-PROGRAMS-REFRESH).
    Programs,
    /// `git_mutation_lane`: one repository's share of the Git pages' writes and checkouts
    /// (`bt-git-mutation`).
    GitMutation,
    /// `persist::StorageLane`: the storage worker (`session-writer`) — the session, the four
    /// hand-editable documents and the preview's saves.
    Storage,
    /// `clipboard_lane`: every read and write of the clipboard (`bt-clipboard`).
    Clipboard,
}

#[cfg(test)]
impl LaneName {
    pub(crate) const ALL: [Self; 10] = [
        Self::Handoff,
        Self::Font,
        Self::Taskbar,
        Self::Computation,
        Self::ComputationScale,
        Self::ComputationPath,
        Self::Programs,
        Self::GitMutation,
        Self::Storage,
        Self::Clipboard,
    ];
}

/// How a newer request relates to an older one.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Replacement {
    /// Every request is executed and answered.
    Fifo,
    /// A newer request supersedes one not yet started; "latest" is the newest **requested**, and
    /// an answer older than the one **adopted** is dropped.
    LatestValue,
}

/// The order requests are executed in.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Order {
    /// The order they were admitted in.
    Submission,
    /// The newest request standing when a round starts; the ones between are not executed.
    NewestRequested,
}

/// The order answers reach the consumer in, stated apart from execution.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Delivery {
    /// In admission order.
    Submission,
    /// Only newer than what the consumer adopted; the adopted number never goes back.
    NewestAdopted,
}

/// A count a lane promises not to exceed, or the declaration that it promises none.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Bound {
    At(usize),
    Unbounded,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Bounds {
    /// Requests admitted and not started. For a latest-value lane, the pending rounds.
    pub(crate) waiting: Bound,
    /// Requests executing at once.
    pub(crate) executing: usize,
    /// Answers published and not yet drained by the consumer.
    pub(crate) answers_held: Bound,
}

/// What the asker's going does to its request.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cancellation {
    /// The call cannot be interrupted; the target incarnation that asked is gone, and its answer
    /// is dropped when it arrives, never raised in another.
    Abandon,
    /// The target is the application, which does not go: there is nothing to abandon, and what
    /// the consumer must refuse is an answer older than the one it has adopted.
    Never,
}

/// Whether a lane's asker may withdraw a request it still holds.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Withdrawal {
    /// It may: the request ends `Cancelled`, once, in the lane's sink and on no surface, and a
    /// request withdrawn before its work began is not run.
    ByTheAsker,
    /// It may not, and the lane never says `Cancelled`.
    Never,
}

/// **One lane's declared policy.** Each field is checked by the claim that reads it.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Contract {
    /// The lane, or `None` for a lane the suite builds on the seam itself (`lane_contract_tests`'s
    /// scripted lanes), which is no row of the ledger.
    pub(crate) lane: Option<LaneName>,
    pub(crate) replacement: Replacement,
    pub(crate) execution: Order,
    pub(crate) delivery: Delivery,
    pub(crate) bounds: Bounds,
    pub(crate) cancellation: Cancellation,
    /// Whether the asker withdraws requests ([`Claim::AWithdrawnRequestEndsCancelledOnce`]).
    pub(crate) withdrawal: Withdrawal,
    /// How often a failure is said (C3): once per request, unless the lane declares streaks.
    pub(crate) saying: Saying,
}

/// `handoff_lane`, on the seam (S-5): one `bt-os-handoff` thread; ids minted by the lane's
/// `Ledger`, unique within one lane (the product starts one lane for the application's life); the
/// asking window's `Pending` holds the request and is the target incarnation, and a closed
/// window's request ends in the lane's sink; the ledger owes 33 at once — the one running and 32
/// waiting, an answer not yet taken holding its place — and a press past that is refused at the
/// door with `LANE_FULL`; each window takes its answers in the order they were minted.
#[cfg(test)]
pub(crate) const HANDOFF: Contract = Contract {
    lane: Some(LaneName::Handoff),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `handoff_lane::CAPACITY` and `handoff_lane::BOUND`, written out rather than read, so
        // that a change to either the declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(32),
        executing: 1,
        answers_held: Bound::At(33),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerRequest,
};

/// `settings::FontLane`, on the seam (S-4): one `font-families` worker; ids minted by the lane's
/// `Ledger`; one walk out and one waiting, every request asked while one waits joined to it (the
/// same question: the machine's families now); the application holds every request and takes
/// their terminals in mint order (`adopt_scanned_families`), adopting a walk's lists only when
/// not older than the list on screen; the ledger owes 16 at once — a terminal not yet taken
/// holding its place — and refuses the rest at the door; a failed walk is said once per streak.
#[cfg(test)]
pub(crate) const FONT: Contract = Contract {
    lane: Some(LaneName::Font),
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        // `settings::FONT_BOUND`, written out rather than read, so that a change to either the
        // declaration or the lane is a disagreement the suite reports.
        answers_held: Bound::At(16),
    },
    cancellation: Cancellation::Never,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `taskbar_lane`, on the seam (S-4): one `taskbar-state` worker; ids minted by the lane's
/// `Ledger`; one ask out and one waiting, every request made while one waits joined to it (the
/// same question: does the bar hide itself now); the application holds every request, and the
/// window's reading takes their terminals in mint order and offers each answer to a
/// `bt_platform::TaskbarState` slot that takes only newer numbers; the ledger owes 16 at once — a
/// terminal not yet taken holding its place — and refuses the rest at the door; a failure (a
/// panic in the ask: the question has no error form) is said once per streak.
#[cfg(test)]
pub(crate) const TASKBAR: Contract = Contract {
    lane: Some(LaneName::Taskbar),
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        // `taskbar_lane::BOUND`, written out rather than read.
        answers_held: Bound::At(16),
    },
    cancellation: Cancellation::Never,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `computation_lane`'s decoration share, on the seam (S-8): one `bt-math-worker` thread; ids
/// minted by the share's `Ledger`; every request run and answered in the order it was minted, none
/// coalesced but a window's animation open and its read of a file's pixels, which a newer ask of
/// the same file by the same window supersedes while it waits (T-ANIMATION-ANSWER-IDENTITY,
/// T-PEEK-ANSWER-IDENTITY); the asking tab (or window) is the target incarnation, and a closed
/// tab's request ends in the lane's sink, which ignores it; the ledger owes 196 at once — a sizing
/// choice, what one pane's session holds queued for this worker (`bt_term::DECORATION_TASKS_HELD`),
/// one running and the rest waiting, an answer not yet taken holding its place — and refuses the
/// rest at the door; a failure (a panic in one request's work) is said once per streak.
#[cfg(test)]
pub(crate) const COMPUTATION: Contract = Contract {
    lane: Some(LaneName::Computation),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `computation_lane::DECORATION_BOUND`, written out rather than read, so that a change to
        // either the declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(195),
        executing: 1,
        answers_held: Bound::At(196),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `computation_lane`'s resample share, on the seam (S-8): one `bt-image-scale-worker` thread;
/// ids minted by the share's own `Ledger`; run and ended in the order minted, a newer request for
/// the same picture, pane and surface superseding the one waiting or joining it at the same size;
/// the asking tab (or window) is the target incarnation, a closed tab's request ending in the
/// lane's ignoring sink; the ledger owes 68 at once (`bt_term::DECODE_TASKS_HELD`, one hand-out
/// pass's resamples) and refuses the rest at the door; a failure is said once per streak.
#[cfg(test)]
pub(crate) const COMPUTATION_SCALE: Contract = Contract {
    lane: Some(LaneName::ComputationScale),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `computation_lane::SCALE_BOUND`, written out rather than read.
        waiting: Bound::At(67),
        executing: 1,
        answers_held: Bound::At(68),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `computation_lane`'s path-verification share, on the seam (S-8): one `bt-path-verify-worker`
/// thread; ids minted by the share's own `Ledger`; run and ended in the order minted, none
/// coalesced; the asking tab is the target incarnation, a closed tab's request ending in the
/// lane's ignoring sink; the ledger owes 512 at once (`bt_term::PATH_TASKS_HELD`, one hand-out
/// pass's printed paths) and refuses the rest at the door; a failure is said once per streak.
#[cfg(test)]
pub(crate) const COMPUTATION_PATH: Contract = Contract {
    lane: Some(LaneName::ComputationPath),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `computation_lane::PATH_BOUND`, written out rather than read.
        waiting: Bound::At(511),
        executing: 1,
        answers_held: Bound::At(512),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `programs_lane`, on the seam (S-4): one `program-walk` worker; ids minted by the lane's
/// `Ledger`; one walk out and one waiting — a request made while one waits joins it when it asks
/// about the same table and supersedes it when it asks about a different one; the walk's rows
/// published into a mailbox that keeps the newest per row and adopted by `ProfilePrograms::adopt`
/// only when not older than the answer held for the row; the application holds every request and
/// takes their terminals in mint order (`ProgramsLane::take`); the ledger owes 64 at once — a
/// terminal not yet taken holding its place — and refuses the rest at the door; a failed walk is
/// said once per streak and not walked again on its own.
#[cfg(test)]
pub(crate) const PROGRAMS: Contract = Contract {
    lane: Some(LaneName::Programs),
    replacement: Replacement::LatestValue,
    execution: Order::NewestRequested,
    delivery: Delivery::NewestAdopted,
    bounds: Bounds {
        waiting: Bound::At(1),
        executing: 1,
        // `programs_lane::BOUND`, written out rather than read.
        answers_held: Bound::At(64),
    },
    cancellation: Cancellation::Never,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerStreak,
};

/// `git_mutation_lane`, on the seam: one repository's share, one `bt-git-mutation` thread; ids
/// minted by the share's `Ledger`; every write and checkout run and ended in the order it was
/// minted, none coalesced; the asking page's tab (or window) is the target incarnation, and a
/// closed page's request ends in the lane's sink, which ignores it; the ledger owes 2001 at once —
/// what one Git page can hand it (`git::GIT_STATUS_CAP` rows, each with one write in flight, and
/// its one checkout), one running and the rest waiting, a receipt not yet taken holding its place —
/// and refuses the rest at the door; a failure (a panic in one request's work) is said once per
/// request.
#[cfg(test)]
pub(crate) const GIT_MUTATION: Contract = Contract {
    lane: Some(LaneName::GitMutation),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `git_mutation_lane::MUTATION_BOUND`, written out rather than read, so that a change to
        // either the declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(2000),
        executing: 1,
        answers_held: Bound::At(2001),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerRequest,
};

/// `persist::StorageLane`, on the seam (T-STORAGE-WORKER-ON-THE-SEAM): one `session-writer`
/// thread; ids minted by the lane's `Ledger` for every kind it serves; every job run and ended in
/// the order minted — but a trial's receipt, served ahead of what is queued, which no consumer
/// takes by a prefix of that order — none coalesced; the asking store, buffer or session is the
/// target incarnation, and a request whose asker has gone ends in the lane's sink, which lets it
/// go; the ledger owes 50 at once (`persist::STORAGE_BOUND`: the fixed askers' 18 and the
/// preview's 32), one running and the rest waiting, an answer not yet taken holding its place,
/// and refuses the rest at the door; a failure (a panic in one request's work) is said once per
/// request, and the worker goes on.
#[cfg(test)]
pub(crate) const STORAGE: Contract = Contract {
    lane: Some(LaneName::Storage),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `persist::STORAGE_BOUND`, written out rather than read, so that a change to either the
        // declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(49),
        executing: 1,
        answers_held: Bound::At(50),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::Never,
    saying: Saying::PerRequest,
};

/// `clipboard_lane`, on the seam (S-7, T-CLIPBOARD-LANE): one `bt-clipboard` thread, started by
/// the first request, a FIFO share on `Queue`; ids minted by the lane's `Ledger`; every request
/// run and ended in the order minted, none coalesced by the lane — a window's pastes queue behind
/// each other, its newer copy replaces its older one (`Superseded`), and it withdraws a paste
/// whose target has gone or whose deadline or held-input cap has passed (`Cancelled`), each the
/// asker's act through the lane, a withdrawn or replaced request leaving the line unrun
/// (`Queue::withdraw`); the asking window is the target incarnation, and a closed window's
/// request, or a withdrawn one, ends in the lane's sink, which lets it go; the ledger owes 18 at
/// once (`clipboard_lane::BOUND`: a desk of eight pastes and one copy in each of two windows), one
/// running and the rest waiting, an answer not yet taken holding its place, and refuses the rest
/// at the door; a failure (the door's refusal, or a panic in one request's work) is said once per
/// request, and the worker goes on.
#[cfg(test)]
pub(crate) const CLIPBOARD: Contract = Contract {
    lane: Some(LaneName::Clipboard),
    replacement: Replacement::Fifo,
    execution: Order::Submission,
    delivery: Delivery::Submission,
    bounds: Bounds {
        // `clipboard_lane::BOUND`, written out rather than read, so that a change to either the
        // declaration or the lane is a disagreement the suite reports.
        waiting: Bound::At(17),
        executing: 1,
        answers_held: Bound::At(18),
    },
    cancellation: Cancellation::Abandon,
    withdrawal: Withdrawal::ByTheAsker,
    saying: Saying::PerRequest,
};

/// The obligations of §R-D, one each.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) enum Claim {
    /// With the executor held, a flood of requests never makes the asker wait, and the lane admits
    /// exactly its declared bound: a bounded queue refuses the rest with a terminal, a latest-value
    /// lane runs at most its declared rounds and answers the newest request.
    FullLaneAnswersWithoutWaiting,
    /// The same question asked twice is two requests whose answers can be told apart.
    EveryRequestHasItsOwnIdentity,
    /// Execution and delivery follow the declared order.
    OrderIsAsDeclared,
    /// An answer whose target has gone is raised in no other target; an answer older than the one
    /// adopted is not raised.
    AnAbandonedOrOlderAnswerIsNotRaised,
    /// Every admitted request reaches exactly one terminal outcome the consumer can see.
    EveryRequestEndsExactlyOnce,
    /// A wake finds its answer already published, and an answer whose wake was lost is found by
    /// the next drain.
    TheWakeFollowsThePublication,
    /// Answers published while the consumer does not drain are bounded as declared.
    AnswersHeldAreBounded,
    /// A worker that dies leaves every admitted request a terminal outcome, or the lane a fault the
    /// consumer can see, and wakes the consumer to see it (C2).
    ADeadWorkerIsObservable,
    /// A request a newer, different one displaced ends with `Superseded { by }`, and `by` names a
    /// request admitted after it that has itself ended (C4).
    ASupersededRequestSaysSo,
    /// A request that is the same operation as one already admitted ends with `Joined { with }`,
    /// carrying exactly the outcome `with` ended with — never `Superseded` by its twin, never
    /// silent (revision (b)4).
    AJoinedRequestCarriesItsTwin,
    /// A failed request ends with `Failed` — a panic inside its task included — and
    /// `diagnostics.log` hears it once: one line per failed request, or one per streak where the
    /// lane declares streaks, counted through the lane's seam over `diagnostics::note` (C3).
    AFailureIsSaidOnce,
    /// A request whose requester has gone ends in the sink it is owed, and in no target (C6). For a
    /// lane whose target is the application, which does not go, the application is both.
    AnOrphanedAnswerReachesItsSink,
    /// A request its asker withdrew ends `Cancelled` exactly once, in the lane's sink and in no
    /// target, and is not run when its work had not begun (C2's terminal set; T-CLIPBOARD-LANE).
    /// A lane whose askers withdraw nothing never says `Cancelled`.
    AWithdrawnRequestEndsCancelledOnce,
    /// A request its asker withdrew leaves the lane's line with its terminal: with the worker
    /// held, requests withdrawn one after another, past the declared bound, leave nothing waiting,
    /// so the bound holds of the line as of the ledger (T-CLIPBOARD-LANE round 2). A lane whose
    /// askers withdraw nothing is asked one request.
    AWithdrawnRequestLeavesTheLine,
    /// A request's answer is observable only once its work has let go of every handle it took for
    /// it (T-ANSWER-AFTER-THE-FILE-IS-CLOSED): the moment the consumer finds the terminal, nobody
    /// holds the file the work held — the order is the lane's, never a race its consumer wins.
    AnAnswerIsPublishedAfterItsResourcesAreReleased,
}

#[cfg(test)]
impl Claim {
    pub(crate) const ALL: [Self; 15] = [
        Self::FullLaneAnswersWithoutWaiting,
        Self::EveryRequestHasItsOwnIdentity,
        Self::OrderIsAsDeclared,
        Self::AnAbandonedOrOlderAnswerIsNotRaised,
        Self::EveryRequestEndsExactlyOnce,
        Self::TheWakeFollowsThePublication,
        Self::AnswersHeldAreBounded,
        Self::ADeadWorkerIsObservable,
        Self::ASupersededRequestSaysSo,
        Self::AJoinedRequestCarriesItsTwin,
        Self::AFailureIsSaidOnce,
        Self::AnOrphanedAnswerReachesItsSink,
        Self::AWithdrawnRequestEndsCancelledOnce,
        Self::AWithdrawnRequestLeavesTheLine,
        Self::AnAnswerIsPublishedAfterItsResourcesAreReleased,
    ];

    fn check(self, run: &mut Run<'_>) -> Result<(), Failure> {
        match self {
            Self::FullLaneAnswersWithoutWaiting => full_lane_answers_without_waiting(run),
            Self::EveryRequestHasItsOwnIdentity => every_request_has_its_own_identity(run),
            Self::OrderIsAsDeclared => order_is_as_declared(run),
            Self::AnAbandonedOrOlderAnswerIsNotRaised => an_abandoned_or_older_answer(run),
            Self::EveryRequestEndsExactlyOnce => every_request_ends_exactly_once(run),
            Self::TheWakeFollowsThePublication => the_wake_follows_the_publication(run),
            Self::AnswersHeldAreBounded => answers_held_are_bounded(run),
            Self::ADeadWorkerIsObservable => a_dead_worker_is_observable(run),
            Self::ASupersededRequestSaysSo => a_superseded_request_says_so(run),
            Self::AJoinedRequestCarriesItsTwin => a_joined_request_carries_its_twin(run),
            Self::AFailureIsSaidOnce => a_failure_is_said_once(run),
            Self::AnOrphanedAnswerReachesItsSink => an_orphaned_answer_reaches_its_sink(run),
            Self::AWithdrawnRequestEndsCancelledOnce => {
                a_withdrawn_request_ends_cancelled_once(run)
            }
            Self::AWithdrawnRequestLeavesTheLine => a_withdrawn_request_leaves_the_line(run),
            Self::AnAnswerIsPublishedAfterItsResourcesAreReleased => {
                an_answer_is_published_after_its_resources_are_released(run)
            }
        }
    }
}

/// **How a claim failed**, with what was seen.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    /// The executor never started a request the suite put on the lane.
    NeverStarted,
    /// An answer that should have arrived within the suite's patience did not.
    NoAnswer,
    /// A submission blocked the asker.
    SubmissionWaited { longest_ms: u128 },
    /// The lane admitted or ran more (or fewer) than its declared bound.
    BoundBroken { declared: usize, observed: usize },
    /// A lane with no admission bound took every request of the flood and refused none.
    AdmissionUnbounded { admitted: usize },
    /// A lane declared without a bound turned out to keep one.
    DeclaredUnboundedButBounded { observed: usize },
    /// Two requests were given one identity.
    IdentityRepeated { ticket: u64 },
    /// The lane mints no identity: the answers to one question asked twice cannot be told apart.
    IdentityAmbiguous { answers: usize },
    /// Answers or executions arrived out of the declared order.
    OrderBroken {
        expected: Vec<u64>,
        actual: Vec<u64>,
    },
    /// An answer whose target had gone was taken by another target.
    RaisedInAnotherTarget { target: u32 },
    /// An answer older than the adopted one was raised.
    StaleAnswerRaised { key: u64 },
    /// Admitted requests that no terminal outcome ever reached.
    NoTerminal { requests: usize },
    /// A request that reached two terminal outcomes.
    MoreThanOneTerminal { key: u64, terminals: usize },
    /// A published answer never woke the consumer.
    NoWake,
    /// The consumer was woken before the answer it was woken for could be drained.
    WakeBeforePublication,
    /// An answer whose wake was lost was not found by the next drain.
    AnswerLostWithTheWake,
    /// Every answer published while the consumer did not drain was still held.
    AnswersUnbounded { held: usize },
    /// The worker died and neither its admitted requests nor the lane showed it.
    DeathUnobservable { unanswered: usize },
    /// The worker died and its terminals or fault were recorded, and no wake announced them: a
    /// consumer that drains only when woken never learns of the death.
    DeathUnannounced,
    /// A request was delivered a terminal after it had one — to its requester and its sink, or to
    /// its sink twice: a second delivery.
    Twice { key: u64, deliveries: usize },
    /// Requests older than one that ended were displaced with no outcome of their own.
    SupersededInSilence { requests: usize },
    /// A request said it was superseded by one that is not a later admitted request, or by one that
    /// never ended.
    SupersededByNoAnswer { key: u64, by: u64 },
    /// A request said it was superseded by its twin, the same operation asked again.
    SupersededByItsTwin { key: u64 },
    /// Requests asked as the same operation as one that ended got no outcome of their own.
    JoinedInSilence { requests: usize },
    /// A request said it joined one that is not its twin, or one that never ended.
    JoinedWithoutItsTwin { key: u64, with: u64 },
    /// A joined request carried an outcome other than the one its twin ended with.
    JoinedCarriesAnotherAnswer { key: u64 },
    /// A request whose task failed did not end with `Failed`.
    NoFailedTerminal { key: u64 },
    /// Failures were delivered, and the lane has no seam the suite can count its lines through.
    LinesUncounted { failed: usize },
    /// `diagnostics.log` heard the failures other than once each (or once per streak).
    SaidOtherThanOnce { expected: usize, said: usize },
    /// A request whose requester had gone did not end in its sink.
    OrphanNotSunk { key: u64 },
    /// A request its asker withdrew did not end `Cancelled`.
    CancelledInSilence { key: u64 },
    /// A withdrawn request's terminal reached a target.
    CancelledRaised { target: u32 },
    /// A request withdrawn before its work began was run.
    CancelledButRun { key: u64 },
    /// A lane declaring that its askers withdraw nothing said `Cancelled`.
    CancelledUndeclared { key: u64 },
    /// Requests their asker withdrew were still waiting in the lane's line.
    WithdrawnStaysInLine { in_line: usize },
    /// The lane declares withdrawal and gives the suite no reading of its line.
    LineUnobserved,
    /// A request was refused at round `round` of `rounds`, every request before it withdrawn: a
    /// withdrawn request kept its place.
    RefusedAfterWithdrawals { round: usize, rounds: usize },
    /// The consumer found a request's terminal while the file its work had taken was still held.
    HeldPastTheAnswer { key: u64 },
    /// The lane's work never took the file the suite lent it: the claim cannot reach the lane.
    NothingTaken,
}

/// A [`Failure`]'s kind, which is what [`EXPECTED_FAILURES`] pins: the counts beside it depend on
/// the flood's size and are reported, not matched.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailureKind {
    NeverStarted,
    NoAnswer,
    SubmissionWaited,
    BoundBroken,
    AdmissionUnbounded,
    DeclaredUnboundedButBounded,
    IdentityRepeated,
    IdentityAmbiguous,
    OrderBroken,
    RaisedInAnotherTarget,
    StaleAnswerRaised,
    NoTerminal,
    MoreThanOneTerminal,
    NoWake,
    WakeBeforePublication,
    AnswerLostWithTheWake,
    AnswersUnbounded,
    DeathUnobservable,
    DeathUnannounced,
    Twice,
    SupersededInSilence,
    SupersededByNoAnswer,
    SupersededByItsTwin,
    JoinedInSilence,
    JoinedWithoutItsTwin,
    JoinedCarriesAnotherAnswer,
    NoFailedTerminal,
    LinesUncounted,
    SaidOtherThanOnce,
    OrphanNotSunk,
    CancelledInSilence,
    CancelledRaised,
    CancelledButRun,
    CancelledUndeclared,
    WithdrawnStaysInLine,
    LineUnobserved,
    RefusedAfterWithdrawals,
    HeldPastTheAnswer,
    NothingTaken,
}

#[cfg(test)]
impl Failure {
    pub(crate) fn kind(&self) -> FailureKind {
        match self {
            Self::NeverStarted => FailureKind::NeverStarted,
            Self::NoAnswer => FailureKind::NoAnswer,
            Self::SubmissionWaited { .. } => FailureKind::SubmissionWaited,
            Self::BoundBroken { .. } => FailureKind::BoundBroken,
            Self::AdmissionUnbounded { .. } => FailureKind::AdmissionUnbounded,
            Self::DeclaredUnboundedButBounded { .. } => FailureKind::DeclaredUnboundedButBounded,
            Self::IdentityRepeated { .. } => FailureKind::IdentityRepeated,
            Self::IdentityAmbiguous { .. } => FailureKind::IdentityAmbiguous,
            Self::OrderBroken { .. } => FailureKind::OrderBroken,
            Self::RaisedInAnotherTarget { .. } => FailureKind::RaisedInAnotherTarget,
            Self::StaleAnswerRaised { .. } => FailureKind::StaleAnswerRaised,
            Self::NoTerminal { .. } => FailureKind::NoTerminal,
            Self::MoreThanOneTerminal { .. } => FailureKind::MoreThanOneTerminal,
            Self::NoWake => FailureKind::NoWake,
            Self::WakeBeforePublication => FailureKind::WakeBeforePublication,
            Self::AnswerLostWithTheWake => FailureKind::AnswerLostWithTheWake,
            Self::AnswersUnbounded { .. } => FailureKind::AnswersUnbounded,
            Self::DeathUnobservable { .. } => FailureKind::DeathUnobservable,
            Self::DeathUnannounced => FailureKind::DeathUnannounced,
            Self::Twice { .. } => FailureKind::Twice,
            Self::SupersededInSilence { .. } => FailureKind::SupersededInSilence,
            Self::SupersededByNoAnswer { .. } => FailureKind::SupersededByNoAnswer,
            Self::SupersededByItsTwin { .. } => FailureKind::SupersededByItsTwin,
            Self::JoinedInSilence { .. } => FailureKind::JoinedInSilence,
            Self::JoinedWithoutItsTwin { .. } => FailureKind::JoinedWithoutItsTwin,
            Self::JoinedCarriesAnotherAnswer { .. } => FailureKind::JoinedCarriesAnotherAnswer,
            Self::NoFailedTerminal { .. } => FailureKind::NoFailedTerminal,
            Self::LinesUncounted { .. } => FailureKind::LinesUncounted,
            Self::SaidOtherThanOnce { .. } => FailureKind::SaidOtherThanOnce,
            Self::OrphanNotSunk { .. } => FailureKind::OrphanNotSunk,
            Self::CancelledInSilence { .. } => FailureKind::CancelledInSilence,
            Self::CancelledRaised { .. } => FailureKind::CancelledRaised,
            Self::CancelledButRun { .. } => FailureKind::CancelledButRun,
            Self::CancelledUndeclared { .. } => FailureKind::CancelledUndeclared,
            Self::WithdrawnStaysInLine { .. } => FailureKind::WithdrawnStaysInLine,
            Self::LineUnobserved => FailureKind::LineUnobserved,
            Self::RefusedAfterWithdrawals { .. } => FailureKind::RefusedAfterWithdrawals,
            Self::HeldPastTheAnswer { .. } => FailureKind::HeldPastTheAnswer,
            Self::NothingTaken => FailureKind::NothingTaken,
        }
    }
}

/// **A claim a lane is known to fail**, the exact kind of the failure, and the ledger row that
/// repairs it.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExpectedFailure {
    pub(crate) lane: LaneName,
    pub(crate) claim: Claim,
    pub(crate) failure: FailureKind,
    /// The `docs/plans/structural-debt.md` row.
    pub(crate) repair: &'static str,
    pub(crate) why: &'static str,
}

/// **The lanes' declared failures** — data, not `#[ignore]`. Removing a row while its failure
/// stands is red (an undeclared failure); repairing a lane without removing its row is red too (an
/// unexpected pass). Empty since S-8: every lane keeps every claim, and only a claim new to the
/// suite may bring rows (`scripts/ci/check-expected-failures.ps1`).
#[cfg(test)]
pub(crate) const EXPECTED_FAILURES: &[ExpectedFailure] = &[];

/// **One claim, run on one lane**: what it found and how many requests it put on the lane.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Verdict {
    pub(crate) lane: LaneName,
    pub(crate) claim: Claim,
    pub(crate) requests: usize,
    pub(crate) result: Result<(), Failure>,
}

/// **What the suite asks of a lane.** One adapter per lane, each driving the lane's own admission,
/// publication and acceptance; only the executor (through [`Gate`]) and the wake (through
/// [`WakeProbe`]) are the suite's, so it can hold the one and watch the other.
#[cfg(test)]
pub(crate) trait LaneUnderTest {
    fn contract(&self) -> &'static Contract;
    /// The executor's gate: the lane's scripted [`Executor`] passes it where each task begins
    /// ([`Gate::arrive`]), and the adapter's task tells it which question it executes
    /// ([`Gate::execute`]).
    fn gate(&self) -> &Gate;
    /// The wake's probe. The adapter's wake calls [`WakeProbe::woke`] where the lane wakes the
    /// event loop.
    fn probe(&self) -> &WakeProbe;
    /// Put request `question` on the lane for `target` through the lane's own admission. Returns
    /// at once — how long it took is the suite's to measure.
    fn submit(&mut self, target: u32, question: u64) -> Admission;
    /// The target incarnation `target` goes (a window closes, a tab closes).
    fn close_target(&mut self, target: u32);
    /// What the consumer finds now, through the lane's own acceptance. Never waits.
    fn drain(&mut self) -> Vec<Delivered>;
    /// Publish, through the lane's own publication, an answer to the older request `ticket` — as
    /// a second asker or a retried one would. Only asked of a lane whose target is the application.
    fn offer_stale(&mut self, ticket: u64);
    /// The asker `target` withdraws its request `ticket`, through the lane's own withdrawal. Only
    /// asked of a lane that declares [`Withdrawal::ByTheAsker`].
    fn withdraw(&mut self, _target: u32, _ticket: u64) {
        unreachable!("this lane's askers withdraw nothing");
    }
    /// How many requests wait in the lane's line, admitted and not yet taken by its worker; `None`
    /// for a lane the suite cannot read it of. Asked of a lane that declares
    /// [`Withdrawal::ByTheAsker`].
    fn in_line(&self) -> Option<usize> {
        None
    }
    /// What the lane's sink has taken since last asked: the terminals of requests whose requester
    /// had gone (C6). A lane with no sink of its own — one that drops such an answer, or raises it
    /// nowhere — has nothing here.
    fn sunk(&mut self) -> Vec<Delivered> {
        Vec::new()
    }
    /// How many lines the lane has said to `diagnostics.log`, counted through its seam over
    /// `diagnostics::note` (the ledger's `say`). `None`: the lane says its lines to
    /// `diagnostics::note` itself, and nothing counts them.
    fn said(&self) -> Option<usize> {
        None
    }
}

/// **An adapter**, named by the lane it drives; `make` starts a fresh lane for each claim.
#[cfg(test)]
pub(crate) struct Adapter {
    pub(crate) lane: LaneName,
    pub(crate) make: fn() -> Box<dyn LaneUnderTest>,
}

/// What admission said.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Admission {
    /// The identity the lane gave the request, if it gives one.
    pub(crate) ticket: Option<u64>,
    pub(crate) question: u64,
    /// A refusal the lane gave at the door, synchronously. (A refusal delivered through the drain
    /// is a [`Delivered`].)
    pub(crate) refused: Option<String>,
}

#[cfg(test)]
impl Admission {
    /// The name the suite tallies this request under: the lane's identity if it gives one, else
    /// the question.
    fn key(&self) -> u64 {
        self.ticket.unwrap_or(self.question)
    }
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Answered,
    Refused(String),
    /// The lane says it can no longer answer.
    Fault(String),
    /// The request's work failed, in these words.
    Failed(String),
    Cancelled,
    /// A newer request, `by`, displaced it.
    Superseded {
        by: u64,
    },
    /// It joined `with`, and carries the outcome `with` ended with.
    Joined {
        with: u64,
        carried: Box<Outcome>,
    },
    /// The worker ended with the request owed.
    WorkerGone,
}

/// One thing the consumer found.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Delivered {
    /// The target that took it; `None` when no open target did (it was dropped).
    pub(crate) target: Option<u32>,
    pub(crate) ticket: Option<u64>,
    pub(crate) question: Option<u64>,
    pub(crate) outcome: Outcome,
}

#[cfg(test)]
impl Delivered {
    fn key(&self) -> Option<u64> {
        self.ticket.or(self.question)
    }
}

/// How long the suite waits for something that must happen.
#[cfg(test)]
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// **A wake a test can wait on**: the closure a test hands a lane as its wake, and the receiving
/// end that hears one `()` for every time the lane wakes its loop.
#[cfg(test)]
pub(crate) fn wake_channel() -> (impl Fn() + Clone + Send + 'static, mpsc::Receiver<()>) {
    let (woke, wakes) = mpsc::channel();
    (
        move || {
            let _ = woke.send(());
        },
        wakes,
    )
}

/// **One wake, within the lane suite's patience** ([`PATIENCE`]): a lane that never wakes fails
/// the test that awaited `what`, by name, rather than hanging it. The wait ends on the wake, so a
/// loaded machine is waited for and a quick one pays nothing.
#[cfg(test)]
pub(crate) fn wait_for_a_wake(wakes: &mpsc::Receiver<()>, what: &str) {
    receive(wakes, what);
}

/// **One message, within the lane suite's patience** ([`PATIENCE`]): what [`wait_for_a_wake`]
/// waits for, when the message carries something. A sender that never sends fails the test that
/// awaited `what`, by name, rather than hanging it.
#[cfg(test)]
pub(crate) fn receive<T>(messages: &mpsc::Receiver<T>, what: &str) -> T {
    match messages.recv_timeout(PATIENCE) {
        Ok(message) => message,
        Err(_) => panic!("nothing within the lane suite's patience while awaiting {what}"),
    }
}

/// **A state another thread reaches that announces itself to nobody** — a worker gone, a mark
/// written, a queue that let go — asked again until `done` holds or [`PATIENCE`] has run out;
/// whether it held. The wait ends on the state, so a loaded machine is waited for and a quick one
/// pays nothing. Where a signal exists ([`wait_for_a_wake`], [`Hold::wait_standing`]) the test
/// waits on that instead.
#[cfg(test)]
pub(crate) fn within_patience(mut done: impl FnMut() -> bool) -> bool {
    let since = Instant::now();
    while !done() {
        if since.elapsed() >= PATIENCE {
            return done();
        }
        std::thread::yield_now();
    }
    true
}

/// [`within_patience`], failing the test that awaited `what`, by name, when the state never comes
/// rather than hanging it.
#[cfg(test)]
pub(crate) fn wait_until(what: &str, done: impl FnMut() -> bool) {
    assert!(
        within_patience(done),
        "not so within the lane suite's patience while awaiting {what}"
    );
}

/// **A hold a fake puts its work behind**: the suite holds it; the fake passes it where its work
/// begins and stands there while it is held; the suite waits, within [`PATIENCE`], until a worker
/// is standing in it — the signal that the work has been taken and has not ended.
///
/// Its limit: a worker woken by [`Hold::release`] is counted as standing until it has run again,
/// so a `hold` straight after a `release` can let the next [`Hold::wait_standing`] return on that
/// worker rather than on the next one. Every test that holds twice awaits the held work's end
/// (its answer, through the lane's wake) between the two holds, which is when the count is zero.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Hold {
    state: Mutex<HoldState>,
    changed: Condvar,
}

#[cfg(test)]
#[derive(Default)]
struct HoldState {
    held: bool,
    /// How many workers stand in the hold now.
    standing: usize,
}

#[cfg(test)]
impl Hold {
    /// From now on every pass stands until [`Self::release`].
    pub(crate) fn hold(&self) {
        lock(&self.state).held = true;
    }

    pub(crate) fn release(&self) {
        lock(&self.state).held = false;
        self.changed.notify_all();
    }

    /// **Called by the fake where its work begins**: counted as standing, and kept there while the
    /// hold is held.
    pub(crate) fn pass(&self) {
        let mut state = lock(&self.state);
        state.standing += 1;
        self.changed.notify_all();
        while state.held {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.standing -= 1;
    }

    /// Waits until a worker stands in the hold; a worker that never comes fails the test that
    /// awaited `what`, by name, rather than hanging it.
    pub(crate) fn wait_standing(&self, what: &str) {
        let (state, waited) = self
            .changed
            .wait_timeout_while(lock(&self.state), PATIENCE, |state| state.standing == 0)
            .unwrap_or_else(PoisonError::into_inner);
        drop(state);
        assert!(
            !waited.timed_out(),
            "nothing stood in the hold within the lane suite's patience while awaiting {what}"
        );
    }
}
/// How long it watches before concluding that something does not happen.
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(300);
/// A submission slower than this made the asker wait.
#[cfg(test)]
const PROMPT: Duration = Duration::from_secs(1);
/// A held executor lets go after this on its own, so a submission that blocks on it comes back
/// (slow, and caught) instead of hanging the suite.
#[cfg(test)]
const HOLD_LIMIT: Duration = Duration::from_secs(5);

/// **The executor's gate**: the suite holds it, counts what passed it, and can make the worker die
/// in it.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[cfg(test)]
#[derive(Default)]
struct GateState {
    held: bool,
    /// How many of the next worker starts the thread door refuses.
    refused_starts: usize,
    kill: bool,
    killed: bool,
    entered: usize,
    executed: Vec<u64>,
    /// The questions whose execution panics.
    planted: Vec<u64>,
    /// The file the suite lends every execution, when it does: each one holds it, opened and
    /// locked, for as long as it keeps the [`Taken`] it was handed.
    lent: Option<std::path::PathBuf>,
    /// How many executions took the lent file.
    taken: usize,
    /// Whether the suite has looked at the lent file since the last lend.
    looked: bool,
}

/// **What one execution holds of the file the suite lent it** ([`Gate::lend`]): the file, opened
/// and exclusively locked, or nothing when nothing was lent. It stands for the handle a lane's
/// real work takes for its request — the file a decode reads, the pipe a child writes into — and
/// the adapter keeps it for exactly as long as its work runs.
#[cfg(test)]
#[must_use = "an execution holds the lent file for as long as its work runs"]
pub(crate) struct Taken {
    _held: Option<std::fs::File>,
}

/// What a panic the suite plants in a request says — the same words each time, so that two in a
/// row are one streak.
#[cfg(test)]
pub(crate) const PLANTED_PANIC: &str = "the lane contract suite fails this request on purpose";

/// What the thread door says when the suite has it refuse a worker's start.
#[cfg(test)]
pub(crate) const REFUSED_START: &str = "the lane contract suite refuses this thread on purpose";

#[cfg(test)]
impl Gate {
    /// **Called by the seam's scripted worker where a task begins**, outside the task's panic
    /// catch ([`task`]): counted, held, and the worker ended here when the suite asks.
    pub(crate) fn arrive(&self) {
        let mut state = lock(&self.state);
        state.entered += 1;
        self.changed.notify_all();
        let since = Instant::now();
        while state.held && since.elapsed() < HOLD_LIMIT {
            state = self
                .changed
                .wait_timeout(state, HOLD_LIMIT)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if state.kill {
            state.kill = false;
            state.killed = true;
            self.changed.notify_all();
            drop(state);
            panic!("the lane contract suite terminates this worker on purpose");
        }
    }

    /// **Called by a scripted task as it executes `question`**, inside its panic catch: recorded,
    /// the file the suite lent taken (opened and exclusively locked) and handed back for the work
    /// to hold while it runs, and the panic planted in it fired.
    pub(crate) fn execute(&self, question: u64) -> Taken {
        let lent = {
            let mut state = lock(&self.state);
            state.executed.push(question);
            state.lent.clone()
        };
        let taken = Taken {
            _held: lent.map(|lent| {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lent)
                    .expect("the file the suite lent opens");
                file.lock().expect("the file the suite lent locks");
                lock(&self.state).taken += 1;
                file
            }),
        };
        self.changed.notify_all();
        self.fire(question);
        taken
    }

    /// From now on every execution takes `file` ([`Self::execute`]), and the suite has not looked.
    pub(crate) fn lend(&self, file: std::path::PathBuf) {
        let mut state = lock(&self.state);
        state.lent = Some(file);
        state.looked = false;
    }

    /// How many executions took the lent file.
    fn taken(&self) -> usize {
        lock(&self.state).taken
    }

    /// The suite has looked at the lent file: a planted lane that kept it past its answer lets
    /// it go now.
    fn looked(&self) {
        let mut state = lock(&self.state);
        state.lent = None;
        state.looked = true;
        self.changed.notify_all();
    }

    /// **For a planted lane only**: wait, within the suite's patience, until the suite has looked
    /// at the lent file.
    pub(crate) fn wait_looked(&self) -> bool {
        self.wait_for(|state| state.looked)
    }

    /// The next worker start the suite's executor is asked for is refused by the thread door,
    /// with [`REFUSED_START`].
    pub(crate) fn refuse_next_start(&self) {
        lock(&self.state).refused_starts += 1;
    }

    fn take_refused_start(&self) -> bool {
        let mut state = lock(&self.state);
        let refused = state.refused_starts > 0;
        state.refused_starts = state.refused_starts.saturating_sub(1);
        refused
    }

    /// Every later execution of `question` panics with [`PLANTED_PANIC`].
    pub(crate) fn plant_panic(&self, question: u64) {
        lock(&self.state).planted.push(question);
    }

    fn fire(&self, question: u64) {
        if lock(&self.state).planted.contains(&question) {
            panic!("{PLANTED_PANIC}");
        }
    }

    pub(crate) fn hold(&self) {
        lock(&self.state).held = true;
    }

    pub(crate) fn release(&self) {
        lock(&self.state).held = false;
        self.changed.notify_all();
    }

    /// The next request to pass (or the one held now) ends the worker it passes on — that one
    /// worker, once: a lane of several workers keeps the others.
    pub(crate) fn kill(&self) {
        lock(&self.state).kill = true;
    }

    pub(crate) fn entered(&self) -> usize {
        lock(&self.state).entered
    }

    fn executed(&self) -> Vec<u64> {
        lock(&self.state).executed.clone()
    }

    fn wait_for(&self, done: impl Fn(&GateState) -> bool) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while !done(&state) {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// Whether `count` requests have reached the executor within the suite's patience.
    pub(crate) fn wait_entered(&self, count: usize) -> bool {
        self.wait_for(|state| state.entered >= count)
    }

    fn wait_killed(&self) -> bool {
        self.wait_for(|state| state.killed)
    }
}

/// **The wake's probe**: counts wakes, can lose them, and can park one on the worker's thread so
/// the suite drains while the worker is still inside its wake.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct WakeProbe {
    state: Mutex<WakeState>,
    changed: Condvar,
}

#[cfg(test)]
#[derive(Default)]
struct WakeState {
    wakes: usize,
    lost: bool,
    /// The suite's thread, when a rendezvous is armed: a wake on any other thread parks.
    rendezvous: Option<ThreadId>,
    parked: bool,
}

#[cfg(test)]
impl WakeProbe {
    /// **Called by the lane's wake.**
    pub(crate) fn woke(&self) {
        let mut state = lock(&self.state);
        state.wakes += 1;
        self.changed.notify_all();
        if state.lost {
            return;
        }
        let here = std::thread::current().id();
        if state.rendezvous.is_some_and(|suite| suite != here) {
            state.rendezvous = None;
            state.parked = true;
            self.changed.notify_all();
            let since = Instant::now();
            while state.parked && since.elapsed() < PATIENCE {
                state = self
                    .changed
                    .wait_timeout(state, PATIENCE)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
    }

    fn wakes(&self) -> usize {
        lock(&self.state).wakes
    }

    fn arm_rendezvous(&self) {
        lock(&self.state).rendezvous = Some(std::thread::current().id());
    }

    fn wait_parked(&self) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while !state.parked {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    fn let_go(&self) {
        let mut state = lock(&self.state);
        state.parked = false;
        state.rendezvous = None;
        self.changed.notify_all();
    }

    fn lose_wakes(&self) {
        lock(&self.state).lost = true;
    }

    fn wait_wakes_past(&self, count: usize) -> bool {
        let mut state = lock(&self.state);
        let since = Instant::now();
        while state.wakes <= count {
            let left = PATIENCE.saturating_sub(since.elapsed());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// One claim's run: the lane, and the count of what it was asked.
#[cfg(test)]
struct Run<'a> {
    lane: &'a mut dyn LaneUnderTest,
    requests: usize,
}

#[cfg(test)]
impl Run<'_> {
    fn contract(&self) -> &'static Contract {
        self.lane.contract()
    }

    fn submit(&mut self, target: u32, question: u64) -> (Admission, Duration) {
        self.requests += 1;
        let started = Instant::now();
        let admission = self.lane.submit(target, question);
        (admission, started.elapsed())
    }

    /// Drain until `done` holds for everything found so far, or [`PATIENCE`] runs out.
    /// [`Self::settle`], gathering what the lane's sink takes into `sunk` as it goes.
    fn settle_with_sink(
        &mut self,
        found: &mut Vec<Delivered>,
        sunk: &mut Vec<Delivered>,
        done: impl Fn(&[Delivered], &[Delivered]) -> bool,
    ) -> bool {
        let since = Instant::now();
        loop {
            found.extend(self.lane.drain());
            sunk.extend(self.lane.sunk());
            if done(found, sunk) {
                return true;
            }
            if since.elapsed() > PATIENCE {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn settle(&mut self, found: &mut Vec<Delivered>, done: impl Fn(&[Delivered]) -> bool) -> bool {
        let since = Instant::now();
        loop {
            found.extend(self.lane.drain());
            if done(found) {
                return true;
            }
            if since.elapsed() > PATIENCE {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Drain for [`QUIET`], adding whatever arrives.
    fn watch(&mut self, found: &mut Vec<Delivered>) {
        let since = Instant::now();
        while since.elapsed() < QUIET {
            found.extend(self.lane.drain());
            std::thread::sleep(Duration::from_millis(5));
        }
        found.extend(self.lane.drain());
    }

    /// Hold the executor and put request `0` on it, waiting until it has started.
    fn hold_one(&mut self) -> Result<Admission, Failure> {
        self.lane.gate().hold();
        let (first, _) = self.submit(0, 0);
        if self.lane.gate().wait_entered(1) {
            Ok(first)
        } else {
            self.lane.gate().release();
            Err(Failure::NeverStarted)
        }
    }
}

/// Everything found that answers `key`.
#[cfg(test)]
fn terminals(found: &[Delivered], key: u64) -> usize {
    found
        .iter()
        .filter(|delivered| delivered.key() == Some(key))
        .count()
}

#[cfg(test)]
fn has_key(found: &[Delivered], key: u64) -> bool {
    terminals(found, key) > 0
}

/// Whether every admitted request not refused at the door has a terminal among `found`.
#[cfg(test)]
fn every_admission_ended(admissions: &[Admission], found: &[Delivered]) -> bool {
    admissions
        .iter()
        .filter(|admission| admission.refused.is_none())
        .all(|admission| has_key(found, admission.key()))
}

#[cfg(test)]
fn full_lane_answers_without_waiting(run: &mut Run<'_>) -> Result<(), Failure> {
    let contract = run.contract();
    let first = run.hold_one()?;
    let flood = match contract.bounds.waiting {
        Bound::At(declared) => declared + 4,
        Bound::Unbounded => 64,
    };
    let mut admissions = vec![first];
    let mut longest = Duration::ZERO;
    for question in 1..=flood as u64 {
        let (admission, took) = run.submit(0, question);
        longest = longest.max(took);
        admissions.push(admission);
    }
    // What the consumer hears while the executor is still held: the refusals, if the lane gives
    // them.
    let while_held = run.lane.drain();
    run.lane.gate().release();
    if longest > PROMPT {
        return Err(Failure::SubmissionWaited {
            longest_ms: longest.as_millis(),
        });
    }
    let mut found = while_held;
    match contract.replacement {
        Replacement::Fifo => {
            let refused = admissions
                .iter()
                .filter(|admission| admission.refused.is_some())
                .count()
                + found
                    .iter()
                    .filter(|delivered| matches!(delivered.outcome, Outcome::Refused(_)))
                    .count();
            let admitted = flood - refused;
            if !run.settle(&mut found, |found| {
                every_admission_ended(&admissions, found)
            }) {
                return Err(Failure::NoAnswer);
            }
            match contract.bounds.waiting {
                Bound::At(declared) if admitted != declared => Err(Failure::BoundBroken {
                    declared,
                    observed: admitted,
                }),
                Bound::At(_) => Ok(()),
                Bound::Unbounded if refused == 0 => Err(Failure::AdmissionUnbounded { admitted }),
                Bound::Unbounded => {
                    Err(Failure::DeclaredUnboundedButBounded { observed: admitted })
                }
            }
        }
        Replacement::LatestValue => {
            let newest = admissions.last().map_or(0, Admission::key);
            if !run.settle(&mut found, |found| has_key(found, newest)) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            let rounds = run.lane.gate().entered();
            let allowed = contract.bounds.executing
                + match contract.bounds.waiting {
                    Bound::At(declared) => declared,
                    Bound::Unbounded => flood,
                };
            if rounds > allowed {
                return Err(Failure::BoundBroken {
                    declared: allowed,
                    observed: rounds,
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
fn every_request_has_its_own_identity(run: &mut Run<'_>) -> Result<(), Failure> {
    let (first, _) = run.submit(0, 7);
    let (second, _) = run.submit(0, 7);
    let mut found = Vec::new();
    let settled = match run.contract().replacement {
        Replacement::LatestValue => {
            let newest = second.key();
            run.settle(&mut found, |found| has_key(found, newest))
        }
        Replacement::Fifo => run.settle(&mut found, |found| found.len() >= 2),
    };
    if !settled {
        return Err(Failure::NoAnswer);
    }
    match (first.ticket, second.ticket) {
        (Some(one), Some(other)) if one == other => Err(Failure::IdentityRepeated { ticket: one }),
        (Some(one), Some(other)) => {
            if found
                .iter()
                .all(|delivered| delivered.ticket == Some(one) || delivered.ticket == Some(other))
            {
                Ok(())
            } else {
                Err(Failure::IdentityAmbiguous {
                    answers: found.len(),
                })
            }
        }
        _ => Err(Failure::IdentityAmbiguous {
            answers: found.len(),
        }),
    }
}

#[cfg(test)]
fn order_is_as_declared(run: &mut Run<'_>) -> Result<(), Failure> {
    let contract = run.contract();
    let first = run.hold_one()?;
    let more = match (contract.replacement, contract.bounds.waiting) {
        (Replacement::LatestValue, _) => 2,
        // One past the bound, so the declared delivery order meets a refusal.
        (_, Bound::At(declared)) => declared + 1,
        (_, Bound::Unbounded) => 5,
    };
    let mut admissions = vec![first];
    for question in 1..=more as u64 {
        // A lane that answers every request is asked from two targets in turn, so a consumer
        // that takes one target's answers and then the other's is out of the declared order.
        let target = match contract.replacement {
            Replacement::LatestValue => 0,
            Replacement::Fifo => (question % 2) as u32,
        };
        admissions.push(run.submit(target, question).0);
    }
    run.lane.gate().release();
    let mut found = Vec::new();
    match contract.replacement {
        Replacement::LatestValue => {
            let newest = admissions.last().map_or(0, Admission::key);
            if !run.settle(&mut found, |found| has_key(found, newest)) {
                return Err(Failure::NoAnswer);
            }
            let delivered: Vec<u64> = found.iter().filter_map(Delivered::key).collect();
            let rising = delivered.windows(2).all(|pair| pair[0] < pair[1]);
            if contract.delivery != Delivery::NewestAdopted
                || !rising
                || delivered.last() != Some(&newest)
            {
                return Err(Failure::OrderBroken {
                    expected: vec![newest],
                    actual: delivered,
                });
            }
            // The gate records the question each run executes, as it does on every lane: the
            // last run executes the newest question asked.
            let newest_question = admissions.last().map_or(0, |admission| admission.question);
            let executed = run.lane.gate().executed();
            let newest_last = executed.windows(2).all(|pair| pair[0] < pair[1])
                && executed.last().is_none_or(|last| *last == newest_question);
            if contract.execution != Order::NewestRequested || !newest_last {
                return Err(Failure::OrderBroken {
                    expected: vec![newest_question],
                    actual: executed,
                });
            }
            Ok(())
        }
        Replacement::Fifo => {
            // A request refused at the door has its terminal already; the rest are found.
            let owed = admissions
                .iter()
                .filter(|admission| admission.refused.is_none())
                .count();
            let all_ended = |found: &[Delivered]| {
                every_admission_ended(&admissions, found) && found.len() >= owed
            };
            if !run.settle(&mut found, all_ended) {
                return Err(Failure::NoAnswer);
            }
            let questions = |refused: bool| -> Vec<u64> {
                found
                    .iter()
                    .filter(|delivered| matches!(delivered.outcome, Outcome::Refused(_)) == refused)
                    .filter_map(|delivered| delivered.question)
                    .collect()
            };
            let actual: Vec<u64> = found
                .iter()
                .filter_map(|delivered| delivered.question)
                .collect();
            let expected: Vec<u64> = match contract.delivery {
                Delivery::Submission => {
                    let mut all = actual.clone();
                    all.sort_unstable();
                    all
                }
                // A lane that answers every request cannot deliver only the newest.
                Delivery::NewestAdopted => Vec::new(),
            };
            if actual != expected {
                return Err(Failure::OrderBroken { expected, actual });
            }
            let executed = run.lane.gate().executed();
            let mut answered = questions(false);
            answered.sort_unstable();
            if contract.execution != Order::Submission
                || (!executed.is_empty() && executed != answered)
            {
                return Err(Failure::OrderBroken {
                    expected: answered,
                    actual: executed,
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
fn an_abandoned_or_older_answer(run: &mut Run<'_>) -> Result<(), Failure> {
    match run.contract().cancellation {
        Cancellation::Abandon => {
            let gone = run.hold_one()?;
            let (kept, _) = run.submit(1, 1);
            run.lane.close_target(0);
            run.lane.gate().release();
            let mut found = Vec::new();
            let mut sunk = Vec::new();
            // The gone target's answer is found dropped, or in the lane's sink.
            if !run.settle_with_sink(&mut found, &mut sunk, |found, sunk| {
                (has_key(found, gone.key()) || has_key(sunk, gone.key()))
                    && has_key(found, kept.key())
            }) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            for delivered in &found {
                if delivered.key() == Some(gone.key())
                    && let Some(target) = delivered.target
                {
                    return Err(Failure::RaisedInAnotherTarget { target });
                }
            }
            if found
                .iter()
                .any(|delivered| delivered.key() == Some(kept.key()) && delivered.target != Some(1))
            {
                return Err(Failure::RaisedInAnotherTarget { target: 1 });
            }
            Ok(())
        }
        Cancellation::Never => {
            let (older, _) = run.submit(0, 0);
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| has_key(found, older.key())) {
                return Err(Failure::NoAnswer);
            }
            let (newer, _) = run.submit(0, 1);
            if !run.settle(&mut found, |found| has_key(found, newer.key())) {
                return Err(Failure::NoAnswer);
            }
            run.lane.offer_stale(older.key());
            let mut after = Vec::new();
            run.watch(&mut after);
            match after.first() {
                Some(raised) => Err(Failure::StaleAnswerRaised {
                    key: raised.key().unwrap_or_default(),
                }),
                None => Ok(()),
            }
        }
    }
}

#[cfg(test)]
fn every_request_ends_exactly_once(run: &mut Run<'_>) -> Result<(), Failure> {
    let first = run.hold_one()?;
    let mut admissions = vec![first];
    for question in 1..=5 {
        admissions.push(run.submit(0, question).0);
    }
    run.lane.gate().release();
    let mut found = Vec::new();
    let newest = admissions.last().map_or(0, Admission::key);
    let settled = match run.contract().replacement {
        Replacement::LatestValue => run.settle(&mut found, |found| has_key(found, newest)),
        Replacement::Fifo => run.settle(&mut found, |found| {
            every_admission_ended(&admissions, found)
        }),
    };
    if !settled {
        return Err(Failure::NoAnswer);
    }
    run.watch(&mut found);
    let sunk = run.lane.sunk();
    let mut unanswered = 0;
    for admission in &admissions {
        let key = admission.key();
        let taken = match &admission.refused {
            // A refusal at the door is a terminal with no identity; an answer found under the
            // same question and no identity would be a second one.
            Some(_) => {
                1 + found
                    .iter()
                    .filter(|delivered| {
                        delivered.ticket.is_none() && delivered.question == Some(admission.question)
                    })
                    .count()
            }
            None => terminals(&found, key),
        };
        if taken > 1 {
            return Err(Failure::MoreThanOneTerminal {
                key,
                terminals: taken,
            });
        }
        // No requester here goes, so whatever its sink took is a second delivery.
        let in_sink = if admission.refused.is_some() {
            0
        } else {
            terminals(&sunk, key)
        };
        let deliveries = taken + in_sink;
        if deliveries > 1 {
            return Err(Failure::Twice { key, deliveries });
        }
        unanswered += usize::from(deliveries == 0);
    }
    if unanswered > 0 {
        return Err(Failure::NoTerminal {
            requests: unanswered,
        });
    }
    Ok(())
}

#[cfg(test)]
fn the_wake_follows_the_publication(run: &mut Run<'_>) -> Result<(), Failure> {
    // The worker parks inside its wake; what the consumer drains then was published before it.
    run.lane.probe().arm_rendezvous();
    let (first, _) = run.submit(0, 0);
    if !run.lane.probe().wait_parked() {
        run.lane.probe().let_go();
        return Err(Failure::NoWake);
    }
    let seen = run.lane.drain();
    run.lane.probe().let_go();
    if !has_key(&seen, first.key()) {
        return Err(Failure::WakeBeforePublication);
    }
    // A wake that reaches nobody: the next drain, whatever brings it, still finds the answer.
    run.lane.probe().lose_wakes();
    let before = run.lane.probe().wakes();
    let (second, _) = run.submit(0, 1);
    if !run.lane.probe().wait_wakes_past(before) {
        return Err(Failure::NoWake);
    }
    let seen = run.lane.drain();
    if !has_key(&seen, second.key()) {
        return Err(Failure::AnswerLostWithTheWake);
    }
    Ok(())
}

#[cfg(test)]
fn answers_held_are_bounded(run: &mut Run<'_>) -> Result<(), Failure> {
    let declared = run.contract().bounds.answers_held;
    let asked = match declared {
        Bound::At(bound) => bound + 8,
        Bound::Unbounded => 40,
    };
    for question in 0..asked as u64 {
        let before = run.lane.probe().wakes();
        let (admission, _) = run.submit(0, question);
        // A request refused at the door has its terminal in hand, and nothing to wake for.
        if admission.refused.is_none() && !run.lane.probe().wait_wakes_past(before) {
            return Err(Failure::NoWake);
        }
    }
    let held = run.lane.drain().len();
    match declared {
        Bound::At(bound) if held > bound => Err(Failure::BoundBroken {
            declared: bound,
            observed: held,
        }),
        Bound::At(_) => Ok(()),
        Bound::Unbounded if held >= asked => Err(Failure::AnswersUnbounded { held }),
        Bound::Unbounded => Err(Failure::DeclaredUnboundedButBounded { observed: held }),
    }
}

#[cfg(test)]
fn a_dead_worker_is_observable(run: &mut Run<'_>) -> Result<(), Failure> {
    let first = run.hold_one()?;
    let mut admissions = vec![first];
    for question in 1..=2 {
        admissions.push(run.submit(0, question).0);
    }
    let before = run.lane.probe().wakes();
    run.lane.gate().kill();
    run.lane.gate().release();
    if !run.lane.gate().wait_killed() {
        return Err(Failure::NeverStarted);
    }
    // The death is announced by a wake (C2), and the drain it brings finds every terminal; a lane
    // that does not wake is still watched, to tell an unannounced death from an unobservable one.
    let announced = run.lane.probe().wait_wakes_past(before);
    let mut found = Vec::new();
    if announced {
        found.extend(run.lane.drain());
    } else {
        run.watch(&mut found);
    }
    let faulted = found
        .iter()
        .any(|delivered| matches!(delivered.outcome, Outcome::Fault(_)));
    let unanswered = admissions
        .iter()
        .filter(|admission| admission.refused.is_none() && !has_key(&found, admission.key()))
        .count();
    if !faulted && unanswered > 0 {
        return Err(Failure::DeathUnobservable { unanswered });
    }
    if !announced {
        return Err(Failure::DeathUnannounced);
    }
    Ok(())
}

/// The terminal `found` holds for `key`, if one.
#[cfg(test)]
fn outcome_of(found: &[Delivered], key: u64) -> Option<&Outcome> {
    found
        .iter()
        .find(|delivered| delivered.key() == Some(key))
        .map(|delivered| &delivered.outcome)
}

#[cfg(test)]
fn a_superseded_request_says_so(run: &mut Run<'_>) -> Result<(), Failure> {
    // One running, three different questions behind it: a lane that replaces requests replaces
    // the waiting ones.
    let first = run.hold_one()?;
    let mut admissions = vec![first];
    for question in 1..=3 {
        admissions.push(run.submit(0, question).0);
    }
    run.lane.gate().release();
    let newest = admissions.last().map_or(0, Admission::key);
    let mut found = Vec::new();
    if !run.settle(&mut found, |found| has_key(found, newest)) {
        return Err(Failure::NoAnswer);
    }
    // A drain takes one request after another, so an older request looked at a moment before its
    // terminal landed can sit beside a newer one found: one drain more, now that the newest has
    // ended, finds whatever ended before it.
    found.extend(run.lane.drain());
    let order: Vec<u64> = admissions.iter().map(Admission::key).collect();
    let mut silent = 0;
    for (at, admission) in admissions.iter().enumerate() {
        if admission.refused.is_some() {
            continue;
        }
        let key = admission.key();
        let later = &order[at + 1..];
        match outcome_of(&found, key) {
            Some(Outcome::Superseded { by }) => {
                if !later.contains(by) || !has_key(&found, *by) {
                    return Err(Failure::SupersededByNoAnswer { key, by: *by });
                }
            }
            Some(_) => {}
            None if later.iter().any(|later| has_key(&found, *later)) => silent += 1,
            None => {}
        }
    }
    if silent > 0 {
        return Err(Failure::SupersededInSilence { requests: silent });
    }
    Ok(())
}

#[cfg(test)]
fn a_joined_request_carries_its_twin(run: &mut Run<'_>) -> Result<(), Failure> {
    // One running, and the same question asked twice behind it.
    const TWIN: u64 = 7;
    run.hold_one()?;
    let twins = [run.submit(0, TWIN).0, run.submit(0, TWIN).0];
    run.lane.gate().release();
    let keys = [twins[0].key(), twins[1].key()];
    let mut found = Vec::new();
    if !run.settle(&mut found, |found| {
        has_key(found, keys[0]) || has_key(found, keys[1])
    }) {
        return Err(Failure::NoAnswer);
    }
    // Whatever the other twin is still owed arrives by the time the running request's answer
    // and the first twin's have.
    run.watch(&mut found);
    let mut silent = 0;
    for (at, admission) in twins.iter().enumerate() {
        if admission.refused.is_some() {
            continue;
        }
        let key = keys[at];
        let twin = keys[1 - at];
        match outcome_of(&found, key) {
            Some(Outcome::Superseded { by }) if *by == twin => {
                return Err(Failure::SupersededByItsTwin { key });
            }
            Some(Outcome::Joined { with, carried }) => {
                let own = outcome_of(&found, *with)
                    .filter(|_| *with == twin)
                    .filter(|own| !matches!(own, Outcome::Joined { .. }));
                match own {
                    None => return Err(Failure::JoinedWithoutItsTwin { key, with: *with }),
                    Some(own) if own != carried.as_ref() => {
                        return Err(Failure::JoinedCarriesAnotherAnswer { key });
                    }
                    Some(_) => {}
                }
            }
            Some(_) => {}
            None if has_key(&found, twin) => silent += 1,
            None => {}
        }
    }
    if silent > 0 {
        return Err(Failure::JoinedInSilence { requests: silent });
    }
    Ok(())
}

/// The rounds [`a_failure_is_said_once`] asks, one after another: in each the first request is
/// held on the executor while the rest are asked behind it, so the same question asked twice
/// is a join on a lane that joins. Each request is its question and whether its task panics: a
/// failed request and its twin (one streak), an answered one and its twin, and one failure more.
#[cfg(test)]
const FAILURE_ROUNDS: [&[(u64, bool)]; 3] = [
    &[(1, true), (1, true)],
    &[(3, false), (3, false)],
    &[(4, true)],
];

/// The failure's words a delivered outcome carries, its own or its twin's.
#[cfg(test)]
fn failed_words(outcome: &Outcome) -> Option<&str> {
    match outcome {
        Outcome::Failed(words) => Some(words),
        Outcome::Joined { carried, .. } => failed_words(carried),
        _ => None,
    }
}

#[cfg(test)]
fn a_failure_is_said_once(run: &mut Run<'_>) -> Result<(), Failure> {
    for (question, fails) in FAILURE_ROUNDS.iter().copied().flatten() {
        if *fails {
            run.lane.gate().plant_panic(*question);
        }
    }
    let mut found = Vec::new();
    // What each request ended with, in the order asked: the failure's words, or `None`.
    let mut ended: Vec<Option<String>> = Vec::new();
    for round in FAILURE_ROUNDS {
        let entered = run.lane.gate().entered();
        run.lane.gate().hold();
        let mut asked = Vec::new();
        for (at, (question, fails)) in round.iter().enumerate() {
            asked.push((run.submit(0, *question).0, *fails));
            if at == 0 && !run.lane.gate().wait_entered(entered + 1) {
                run.lane.gate().release();
                return Err(Failure::NeverStarted);
            }
        }
        run.lane.gate().release();
        let keys: Vec<u64> = asked.iter().map(|(admission, _)| admission.key()).collect();
        let settled = asked
            .iter()
            .all(|(admission, _)| admission.refused.is_none())
            && run.settle(&mut found, |found| {
                keys.iter().all(|key| has_key(found, *key))
            });
        for (admission, fails) in &asked {
            let key = admission.key();
            let words = settled
                .then(|| outcome_of(&found, key))
                .flatten()
                .map(failed_words);
            match (words, fails) {
                (Some(Some(words)), _) => ended.push(Some(words.to_owned())),
                (_, true) => return Err(Failure::NoFailedTerminal { key }),
                (None, false) => return Err(Failure::NoAnswer),
                (Some(None), false) => ended.push(None),
            }
        }
    }
    let failed = ended.iter().flatten().count();
    let expected = match run.contract().saying {
        Saying::PerRequest => failed,
        Saying::PerStreak => ended
            .iter()
            .enumerate()
            .filter(|(at, words)| words.is_some() && (*at == 0 || ended[at - 1] != **words))
            .count(),
    };
    match run.lane.said() {
        None => Err(Failure::LinesUncounted { failed }),
        Some(said) if said != expected => Err(Failure::SaidOtherThanOnce { expected, said }),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
fn an_orphaned_answer_reaches_its_sink(run: &mut Run<'_>) -> Result<(), Failure> {
    match run.contract().cancellation {
        Cancellation::Abandon => {
            let gone = run.hold_one()?;
            let (kept, _) = run.submit(1, 1);
            run.lane.close_target(0);
            run.lane.gate().release();
            let mut found = Vec::new();
            let mut sunk = Vec::new();
            // An abandoning lane runs its requests in order: by the time the kept request's answer
            // is found, the gone one's has landed wherever the lane puts it.
            if !run.settle_with_sink(&mut found, &mut sunk, |found, _| has_key(found, kept.key())) {
                return Err(Failure::NoAnswer);
            }
            sunk.extend(run.lane.sunk());
            if let Some(target) = found
                .iter()
                .filter(|delivered| delivered.key() == Some(gone.key()))
                .find_map(|delivered| delivered.target)
            {
                return Err(Failure::RaisedInAnotherTarget { target });
            }
            match terminals(&sunk, gone.key()) {
                0 => Err(Failure::OrphanNotSunk { key: gone.key() }),
                1 => Ok(()),
                deliveries => Err(Failure::Twice {
                    key: gone.key(),
                    deliveries,
                }),
            }
        }
        Cancellation::Never => {
            // The application asked, does not go, and holds the answer: it is the sink.
            let (asked, _) = run.submit(0, 0);
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| has_key(found, asked.key())) {
                return Err(Failure::NoAnswer);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
fn a_withdrawn_request_ends_cancelled_once(run: &mut Run<'_>) -> Result<(), Failure> {
    let first = run.hold_one()?;
    let (second, _) = run.submit(0, 1);
    match run.contract().withdrawal {
        Withdrawal::Never => {
            // Nothing is withdrawn here, so nothing may end `Cancelled`.
            run.lane.gate().release();
            let mut found = Vec::new();
            let mut sunk = Vec::new();
            if !run.settle_with_sink(&mut found, &mut sunk, |found, _| {
                has_key(found, second.key())
            }) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            sunk.extend(run.lane.sunk());
            match found
                .iter()
                .chain(&sunk)
                .find(|delivered| delivered.outcome == Outcome::Cancelled)
            {
                Some(cancelled) => Err(Failure::CancelledUndeclared {
                    key: cancelled.key().unwrap_or_default(),
                }),
                None => Ok(()),
            }
        }
        Withdrawal::ByTheAsker => {
            // One running, the one to withdraw waiting behind it, and one more behind that.
            let (kept, _) = run.submit(0, 2);
            let Some(ticket) = second.ticket else {
                run.lane.gate().release();
                return Err(Failure::IdentityAmbiguous { answers: 0 });
            };
            let _ = first;
            run.lane.withdraw(0, ticket);
            run.lane.gate().release();
            let key = second.key();
            let mut found = Vec::new();
            let mut sunk = Vec::new();
            if !run.settle_with_sink(&mut found, &mut sunk, |found, sunk| {
                has_key(found, kept.key()) && (has_key(found, key) || has_key(sunk, key))
            }) {
                return Err(Failure::NoAnswer);
            }
            run.watch(&mut found);
            sunk.extend(run.lane.sunk());
            if let Some(target) = found
                .iter()
                .filter(|delivered| delivered.key() == Some(key))
                .find_map(|delivered| delivered.target)
            {
                return Err(Failure::CancelledRaised { target });
            }
            let deliveries = terminals(&found, key) + terminals(&sunk, key);
            if deliveries > 1 {
                return Err(Failure::Twice { key, deliveries });
            }
            if outcome_of(&found, key).or_else(|| outcome_of(&sunk, key))
                != Some(&Outcome::Cancelled)
            {
                return Err(Failure::CancelledInSilence { key });
            }
            if run.lane.gate().executed().contains(&second.question) {
                return Err(Failure::CancelledButRun { key });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
fn a_withdrawn_request_leaves_the_line(run: &mut Run<'_>) -> Result<(), Failure> {
    let contract = run.contract();
    let first = run.hold_one()?;
    match contract.withdrawal {
        Withdrawal::Never => {
            run.lane.gate().release();
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| has_key(found, first.key())) {
                return Err(Failure::NoAnswer);
            }
            Ok(())
        }
        Withdrawal::ByTheAsker => {
            // Past the declared bound, one at a time: each is admitted only because the one before
            // it left, and none of them may stay behind in line.
            let rounds = match contract.bounds.waiting {
                Bound::At(declared) => 2 * declared + 2,
                Bound::Unbounded => 64,
            };
            for question in 1..=rounds as u64 {
                let (asked, _) = run.submit(0, question);
                let Some(ticket) = asked.ticket else {
                    run.lane.gate().release();
                    return Err(Failure::RefusedAfterWithdrawals {
                        round: question as usize,
                        rounds,
                    });
                };
                run.lane.withdraw(0, ticket);
            }
            let in_line = run.lane.in_line();
            run.lane.gate().release();
            let mut found = Vec::new();
            if !run.settle(&mut found, |found| has_key(found, first.key())) {
                return Err(Failure::NoAnswer);
            }
            match in_line {
                None => Err(Failure::LineUnobserved),
                Some(0) => Ok(()),
                Some(in_line) => Err(Failure::WithdrawnStaysInLine { in_line }),
            }
        }
    }
}

#[cfg(test)]
fn an_answer_is_published_after_its_resources_are_released(
    run: &mut Run<'_>,
) -> Result<(), Failure> {
    // A file the suite owns, lent to the work: it holds it, opened and locked, while it runs.
    let file = crate::test_support::ScratchFile(bt_testpath::temp_path("lane-suite-lent"));
    std::fs::write(&file.0, b"held by the work while it runs").expect("the suite's own file");
    run.lane.gate().lend(file.0.clone());
    let (asked, _) = run.submit(0, 0);
    let mut found = Vec::new();
    let answered =
        asked.refused.is_none() && run.settle(&mut found, |found| has_key(found, asked.key()));
    // Looked at the moment the terminal is found, with no wait: whatever still holds the file
    // held it when the answer became observable.
    let free = answered && bt_testpath::nobody_holds(&file.0);
    let taken = run.lane.gate().taken();
    run.lane.gate().looked();
    if !answered {
        return Err(Failure::NoAnswer);
    }
    if taken == 0 {
        return Err(Failure::NothingTaken);
    }
    if !free {
        return Err(Failure::HeldPastTheAnswer { key: asked.key() });
    }
    Ok(())
}

/// **Run one claim on one lane**: how many requests it asked, and what it found. Whatever the
/// claim left held is let go after it.
#[cfg(test)]
pub(crate) fn check(lane: &mut dyn LaneUnderTest, claim: Claim) -> (usize, Result<(), Failure>) {
    let mut run = Run { lane, requests: 0 };
    let result = claim.check(&mut run);
    let requests = run.requests;
    run.lane.gate().release();
    run.lane.probe().let_go();
    (requests, result)
}

/// **Run every claim on one lane**, each on a fresh lane from the adapter.
#[cfg(test)]
pub(crate) fn run_suite(adapter: &Adapter, claims: &[Claim]) -> Vec<Verdict> {
    claims
        .iter()
        .map(|&claim| {
            let mut lane = (adapter.make)();
            assert_eq!(
                lane.contract().lane,
                Some(adapter.lane),
                "the {:?} adapter reports another lane's contract",
                adapter.lane
            );
            let (requests, result) = check(lane.as_mut(), claim);
            Verdict {
                lane: adapter.lane,
                claim,
                requests,
                result,
            }
        })
        .collect()
}

/// **What is wrong with a run of the suite**, as sentences — empty when nothing is.
///
/// `lanes` and `claims` are what the run was meant to cover: a verdict missing for any pair of
/// them is a skipped adapter. Every verdict must have exercised at least one request. A verdict
/// passes only if [`EXPECTED_FAILURES`] (here `expected`) holds no row for it, and fails only with
/// the kind its row names.
#[cfg(test)]
pub(crate) fn judge(
    verdicts: &[Verdict],
    expected: &[ExpectedFailure],
    lanes: &[LaneName],
    claims: &[Claim],
) -> Vec<String> {
    let mut problems = Vec::new();
    for (index, row) in expected.iter().enumerate() {
        if expected[..index]
            .iter()
            .any(|earlier| earlier.lane == row.lane && earlier.claim == row.claim)
        {
            problems.push(format!(
                "{:?} × {:?} is declared twice in the expected failures",
                row.lane, row.claim
            ));
        }
        if row
            .repair
            .strip_prefix("D-")
            .is_none_or(|number| number.parse::<u32>().is_err())
        {
            problems.push(format!(
                "{:?} × {:?} names no ledger row as its repair: {:?}",
                row.lane, row.claim, row.repair
            ));
        }
    }
    for &lane in lanes {
        for &claim in claims {
            let found: Vec<&Verdict> = verdicts
                .iter()
                .filter(|verdict| verdict.lane == lane && verdict.claim == claim)
                .collect();
            let [verdict] = found.as_slice() else {
                problems.push(format!(
                    "{lane:?} × {claim:?} was run {} times, not once: a skipped adapter or claim",
                    found.len()
                ));
                continue;
            };
            if verdict.requests == 0 {
                problems.push(format!("{lane:?} × {claim:?} exercised no request"));
            }
            let row = expected
                .iter()
                .find(|row| row.lane == lane && row.claim == claim);
            match (&verdict.result, row) {
                (Ok(()), None) => {}
                (Ok(()), Some(row)) => problems.push(format!(
                    "{lane:?} × {claim:?} passed, but is declared to fail with {:?} ({}; repair \
                     {}): an unexpected pass — the repair has landed, or the claim no longer \
                     reaches the lane",
                    row.failure, row.why, row.repair
                )),
                (Err(failure), None) => problems.push(format!(
                    "{lane:?} × {claim:?} failed with {failure:?}, and no expected failure \
                     declares it"
                )),
                (Err(failure), Some(row)) if failure.kind() != row.failure => {
                    problems.push(format!(
                        "{lane:?} × {claim:?} failed with {failure:?}, a different failure from \
                         the declared {:?} (repair {})",
                        row.failure, row.repair
                    ));
                }
                (Err(_), Some(_)) => {}
            }
        }
    }
    problems
}
